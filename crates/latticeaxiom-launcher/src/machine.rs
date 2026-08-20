//! Bounded shell/world handoff and launcher bootstrap state machines.

use latticeaxiom_core::CanonicalHash;
use thiserror::Error;

use crate::model::RecoveryLaunchDraftV1;

use crate::{
    AtomicLaunchIntentStore, BootObservationV1, BootstrapAckV1, BootstrapReportV1,
    ClientRuntimeStateV1, FailureDispositionV1, IntentSlot, IntentStoreError, LaunchFailureCodeV1,
    LaunchFailureDetailV1, LaunchFailureReceiptV1, LaunchGeneration, LaunchIntentDraftV1,
    LaunchIntentError, LaunchIntentV1, LaunchModelError, LaunchPhaseV1, LaunchTargetV1,
    MAX_BOOTSTRAP_ACK_WAIT_MS, PriorChildStatusV1, ProcessControl, ProcessLaunchRequestV1,
    PublishDisposition, RecoveryLaunchRequestV1, RecoveryReasonV1, ShutdownBarrierFailureV1,
    ShutdownBarrierReceiptV1, SlotDisposition, SpawnedProcess, StoreOperation, TransitionOutcomeV1,
    TransitionValidationPolicy,
};

const MAX_REPORT_FAILURES: usize = 4;

/// Current-process shutdown boundary required before intent publication.
pub trait TransitionBarrier {
    /// Flushes settings, confirms durability where applicable, stops package
    /// instances, joins bounded tasks, and closes the writer.
    ///
    /// # Errors
    ///
    /// Returns [`ShutdownBarrierFailureV1`] without publishing a launch intent.
    fn reach(
        &mut self,
        current: ClientRuntimeStateV1,
        target: LaunchTargetV1,
    ) -> Result<ShutdownBarrierReceiptV1, ShutdownBarrierFailureV1>;
}

/// Invalid construction or use of a current-process transition machine.
#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum TransitionMachineError {
    /// Only active shell, world, or recovery states may publish a handoff.
    #[error("transition publisher requires an active shell, world, or recovery state")]
    InactiveState,
    /// Active role provenance did not match its generation and durable slot.
    #[error("active client state has inconsistent boot-slot provenance")]
    InconsistentBootProvenance,
    /// The draft generation was not exactly the active generation plus one.
    #[error("draft generation {actual:?} is not the required next generation {expected:?}")]
    NonMonotonicGeneration {
        /// Required next generation.
        expected: LaunchGeneration,
        /// Rejected generation.
        actual: LaunchGeneration,
    },

    /// A generation counter could not advance.
    #[error(transparent)]
    Counter(#[from] LaunchModelError),
}

/// Result of current-process barrier and atomic intent publication.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TransitionPublishReport {
    outcome: TransitionOutcomeV1,
    failure: Option<LaunchFailureReceiptV1>,
    barrier_failure: Option<ShutdownBarrierFailureV1>,
    intent: Option<LaunchIntentV1>,
    recovery_request: Option<RecoveryLaunchRequestV1>,
}

impl TransitionPublishReport {
    /// Returns the transition outcome.
    #[must_use]
    pub const fn outcome(&self) -> TransitionOutcomeV1 {
        self.outcome
    }

    /// Returns a deterministic failure, if the transition did not publish.
    #[must_use]
    pub const fn failure(&self) -> Option<LaunchFailureReceiptV1> {
        self.failure
    }

    /// Returns the typed barrier failure, if the barrier rejected its contract.
    #[must_use]
    pub const fn barrier_failure(&self) -> Option<ShutdownBarrierFailureV1> {
        self.barrier_failure
    }

    /// Returns the sealed intent when publication succeeded or was idempotent.
    #[must_use]
    pub const fn intent(&self) -> Option<&LaunchIntentV1> {
        self.intent.as_ref()
    }

    /// Returns the durable recovery request published after a destructive barrier failure.
    #[must_use]
    pub const fn recovery_request(&self) -> Option<&RecoveryLaunchRequestV1> {
        self.recovery_request.as_ref()
    }
}

/// Current-process publisher for one normal shell/world/recovery handoff.
#[derive(Debug)]
pub struct ClientTransitionMachine {
    state: ClientRuntimeStateV1,
}

impl ClientTransitionMachine {
    /// Creates a publisher from an active client role.
    ///
    /// # Errors
    ///
    /// Returns [`TransitionMachineError::InactiveState`] for a published or
    /// halted state.
    pub const fn from_active(state: ClientRuntimeStateV1) -> Result<Self, TransitionMachineError> {
        match state {
            ClientRuntimeStateV1::Shell {
                generation,
                launch_blob_hash,
                ..
            } if (generation.get() == LaunchGeneration::FIRST.get())
                == launch_blob_hash.is_none() =>
            {
                Ok(Self { state })
            }
            ClientRuntimeStateV1::World { generation, .. }
                if generation.get() != LaunchGeneration::FIRST.get() =>
            {
                Ok(Self { state })
            }
            ClientRuntimeStateV1::Recovery { .. } => Ok(Self { state }),
            ClientRuntimeStateV1::Shell { .. } | ClientRuntimeStateV1::World { .. } => {
                Err(TransitionMachineError::InconsistentBootProvenance)
            }
            ClientRuntimeStateV1::RecoveryHandoffPublished { .. }
            | ClientRuntimeStateV1::HandoffPublished { .. }
            | ClientRuntimeStateV1::Halted { .. } => Err(TransitionMachineError::InactiveState),
        }
    }

    /// Returns the current state.
    #[must_use]
    pub const fn state(&self) -> ClientRuntimeStateV1 {
        self.state
    }

    /// Runs the shutdown barrier and atomically publishes the next intent.
    ///
    /// The newer pending blob is published beside its exact terminal
    /// predecessor. The predecessor remains authoritative until validation and
    /// claim, so a launcher crash cannot create an empty replay-watermark gap.
    ///
    /// # Errors
    ///
    /// Returns [`TransitionMachineError`] when the draft is not the exact next
    /// generation or is invalid before the barrier begins. Operational
    /// barrier/store failures are returned as a deterministic report.
    pub fn publish_handoff<S, B>(
        &mut self,
        draft: LaunchIntentDraftV1,
        store: &mut S,
        barrier: &mut B,
        supervisor_identity: crate::ProcessSupervisorIdentityV1,
    ) -> Result<TransitionPublishReport, TransitionMachineError>
    where
        S: AtomicLaunchIntentStore,
        B: TransitionBarrier,
    {
        let current_generation = match self.state {
            ClientRuntimeStateV1::Shell { generation, .. }
            | ClientRuntimeStateV1::World { generation, .. }
            | ClientRuntimeStateV1::Recovery { generation, .. } => generation,
            ClientRuntimeStateV1::RecoveryHandoffPublished { .. }
            | ClientRuntimeStateV1::HandoffPublished { .. }
            | ClientRuntimeStateV1::Halted { .. } => {
                return Err(TransitionMachineError::InactiveState);
            }
        };
        let expected = current_generation.next()?;
        if draft.generation != expected {
            return Err(TransitionMachineError::NonMonotonicGeneration {
                expected,
                actual: draft.generation,
            });
        }

        // Seal every caller-controlled field before destructive shutdown work.
        let intent = LaunchIntentV1::seal(draft)?;
        let bytes = intent.canonical_bytes()?;
        let barrier_receipt = match barrier.reach(self.state, intent.target()) {
            Ok(receipt) => receipt,
            Err(failure) => {
                return Ok(self.barrier_failure_report(
                    store,
                    &intent,
                    current_generation,
                    supervisor_identity,
                    failure,
                ));
            }
        };

        if let Some(failure) = barrier_receipt_failure(self.state, &intent, barrier_receipt) {
            return Ok(self.barrier_failure_report(
                store,
                &intent,
                current_generation,
                supervisor_identity,
                failure,
            ));
        }
        let terminal_blob_hash =
            match current_terminal_blob_hash(store, current_generation, self.state) {
                Ok(blob_hash) => blob_hash,
                Err(error) => return Ok(self.storage_failure_report(&intent, &error)),
            };
        let publication = if let Some(expected_terminal_blob_hash) = terminal_blob_hash {
            store.publish_replacing_terminal(expected_terminal_blob_hash, &bytes)
        } else {
            store.publish(&bytes)
        };
        match publication {
            Ok(
                PublishDisposition::Published
                | PublishDisposition::PublishedIndeterminate
                | PublishDisposition::AlreadyPublished
                | PublishDisposition::AlreadyHandled(_),
            ) => {
                self.state = ClientRuntimeStateV1::HandoffPublished {
                    generation: intent.generation(),
                    target: intent.target(),
                    checksum: intent.checksum(),
                };
                Ok(TransitionPublishReport {
                    outcome: TransitionOutcomeV1::HandoffPublished {
                        generation: intent.generation(),
                        checksum: intent.checksum(),
                    },
                    failure: None,
                    barrier_failure: None,
                    intent: Some(intent),
                    recovery_request: None,
                })
            }
            Err(error) => Ok(self.storage_failure_report(&intent, &error)),
        }
    }

    fn storage_failure_report(
        &mut self,
        intent: &LaunchIntentV1,
        error: &IntentStoreError,
    ) -> TransitionPublishReport {
        let (phase, code) = store_failure_code(error);
        self.state = ClientRuntimeStateV1::Halted {
            generation: Some(intent.generation()),
            reason: RecoveryReasonV1::InvalidIntent,
        };
        TransitionPublishReport {
            outcome: TransitionOutcomeV1::Halted,
            failure: Some(LaunchFailureReceiptV1::new(
                Some(intent.generation()),
                Some(intent.attempt()),
                Some(intent.target()),
                phase,
                code,
            )),
            barrier_failure: None,
            intent: None,
            recovery_request: None,
        }
    }

    fn barrier_failure_report<S>(
        &mut self,
        store: &mut S,
        intent: &LaunchIntentV1,
        current_generation: LaunchGeneration,
        supervisor_identity: crate::ProcessSupervisorIdentityV1,
        barrier_failure: ShutdownBarrierFailureV1,
    ) -> TransitionPublishReport
    where
        S: AtomicLaunchIntentStore,
    {
        let primary = target_failure(
            intent,
            LaunchPhaseV1::Barrier,
            LaunchFailureCodeV1::BarrierRejected,
        );
        if barrier_failure.disposition() == FailureDispositionV1::CurrentProcessUsable {
            return TransitionPublishReport {
                outcome: TransitionOutcomeV1::CurrentProcessRetained,
                failure: Some(primary),
                barrier_failure: Some(barrier_failure),
                intent: None,
                recovery_request: None,
            };
        }
        if matches!(self.state, ClientRuntimeStateV1::Recovery { .. }) {
            self.state = ClientRuntimeStateV1::Halted {
                generation: Some(current_generation),
                reason: RecoveryReasonV1::RecoveryFailure,
            };
            return TransitionPublishReport {
                outcome: TransitionOutcomeV1::Halted,
                failure: Some(primary),
                barrier_failure: Some(barrier_failure),
                intent: None,
                recovery_request: None,
            };
        }
        let Ok(source_blob_hash) =
            current_terminal_blob_hash(store, current_generation, self.state)
        else {
            return self.failed_barrier_recovery_report(
                current_generation,
                primary,
                barrier_failure,
            );
        };
        let Ok(request) = RecoveryLaunchRequestV1::seal(RecoveryLaunchDraftV1 {
            recovery_generation: current_generation,
            source_generation: Some(current_generation),
            source_blob_hash,
            reason: RecoveryReasonV1::BarrierFailure,
            shell_lock_hash: intent.shell_lock_hash(),
            confirmed_setting_transaction_revision: intent.confirmed_setting_transaction_revision(),
            supervisor_identity,
            failure: primary,
        }) else {
            return self.failed_barrier_recovery_report(
                current_generation,
                primary,
                barrier_failure,
            );
        };
        let Ok(bytes) = request.canonical_bytes() else {
            return self.failed_barrier_recovery_report(
                current_generation,
                primary,
                barrier_failure,
            );
        };
        match store.claim_recovery(source_blob_hash, &bytes) {
            Ok(outcome) if outcome.blob_hash() == CanonicalHash::digest(&bytes) => {}
            Ok(_) | Err(_) => {
                return self.failed_barrier_recovery_report(
                    current_generation,
                    primary,
                    barrier_failure,
                );
            }
        }
        self.state = ClientRuntimeStateV1::RecoveryHandoffPublished {
            generation: current_generation,
            checksum: request.checksum(),
        };
        TransitionPublishReport {
            outcome: TransitionOutcomeV1::RecoveryHandoffPublished {
                generation: current_generation,
                checksum: request.checksum(),
            },
            failure: Some(primary),
            barrier_failure: Some(barrier_failure),
            intent: None,
            recovery_request: Some(request),
        }
    }

    fn failed_barrier_recovery_report(
        &mut self,
        current_generation: LaunchGeneration,
        primary: LaunchFailureReceiptV1,
        barrier_failure: ShutdownBarrierFailureV1,
    ) -> TransitionPublishReport {
        self.state = ClientRuntimeStateV1::Halted {
            generation: Some(current_generation),
            reason: RecoveryReasonV1::RecoveryFailure,
        };
        TransitionPublishReport {
            outcome: TransitionOutcomeV1::Halted,
            failure: Some(primary),
            barrier_failure: Some(barrier_failure),
            intent: None,
            recovery_request: None,
        }
    }
}

/// External launcher state machine that validates, claims, boots, acknowledges,
/// and consumes exactly one pending intent.
#[derive(Debug)]
pub struct BootstrapMachine {
    state: Option<ClientRuntimeStateV1>,
    activation_started: bool,
    recovery_generation: LaunchGeneration,
}

struct BootstrapContext<'a, S, P> {
    store: &'a mut S,
    policy: &'a TransitionValidationPolicy,
    process: &'a mut P,
}

#[derive(Clone, Debug)]
struct RecoveryCause {
    source_blob_hash: Option<CanonicalHash>,
    source_generation: Option<LaunchGeneration>,
    reason: RecoveryReasonV1,
    failures: Vec<LaunchFailureReceiptV1>,
}

#[derive(Clone, Copy)]
struct TargetAttempt<'a> {
    intent: &'a LaunchIntentV1,
    blob_hash: CanonicalHash,
    child: SpawnedProcess,
}

impl Default for BootstrapMachine {
    fn default() -> Self {
        Self::new()
    }
}

impl BootstrapMachine {
    /// Creates an idle launcher bootstrap machine.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            state: None,
            activation_started: false,
            recovery_generation: LaunchGeneration::FIRST,
        }
    }

    /// Returns the active or halted state after a bootstrap attempt.
    #[must_use]
    pub const fn state(&self) -> Option<ClientRuntimeStateV1> {
        self.state
    }

    /// Activates a pending intent or reconciles one durable recovery request.
    ///
    /// Target and recovery children are acquired only after their exact
    /// request is durable. Indeterminate namespace durability and unproven
    /// child termination halt without another spawn.
    pub fn activate<S, P>(
        &mut self,
        store: &mut S,
        policy: &TransitionValidationPolicy,
        process: &mut P,
    ) -> BootstrapReportV1
    where
        S: AtomicLaunchIntentStore,
        P: ProcessControl,
    {
        if self.activation_started {
            return self.suppress_machine_reentry();
        }
        self.activation_started = true;
        let slot = match store.read() {
            Ok(slot) => slot,
            Err(error) => {
                let (phase, code) = store_failure_code(&error);
                return self.halt(
                    None,
                    RecoveryReasonV1::RecoveryFailure,
                    vec![LaunchFailureReceiptV1::new(None, None, None, phase, code)],
                );
            }
        };
        let mut context = BootstrapContext {
            store,
            policy,
            process,
        };
        self.activate_slot(&mut context, slot)
    }

    fn suppress_machine_reentry(&self) -> BootstrapReportV1 {
        let generation = self.state.and_then(ClientRuntimeStateV1::generation);
        BootstrapReportV1::new(
            TransitionOutcomeV1::Halted,
            vec![LaunchFailureReceiptV1::new(
                generation,
                None,
                None,
                LaunchPhaseV1::RecoverySpawn,
                LaunchFailureCodeV1::RecoveryLoopSuppressed,
            )],
        )
    }

    fn activate_slot<S, P>(
        &mut self,
        context: &mut BootstrapContext<'_, S, P>,
        slot: IntentSlot,
    ) -> BootstrapReportV1
    where
        S: AtomicLaunchIntentStore,
        P: ProcessControl,
    {
        let IntentSlot::Occupied {
            disposition,
            bytes,
            blob_hash,
            predecessor,
        } = slot
        else {
            self.recovery_generation = LaunchGeneration::FIRST;
            return self.claim_recovery_and_recover(
                context,
                RecoveryCause {
                    source_blob_hash: None,
                    source_generation: None,
                    reason: RecoveryReasonV1::InvalidIntent,
                    failures: vec![LaunchFailureReceiptV1::new(
                        None,
                        None,
                        None,
                        LaunchPhaseV1::IntentRead,
                        LaunchFailureCodeV1::IntentMissing,
                    )],
                },
            );
        };
        match disposition {
            SlotDisposition::Pending => {
                let expected_generation = match expected_generation(predecessor.as_ref()) {
                    Ok(generation) => generation,
                    Err(error) => {
                        let (phase, code) = store_failure_code(&error);
                        return self.halt(
                            None,
                            RecoveryReasonV1::RecoveryFailure,
                            vec![LaunchFailureReceiptV1::new(None, None, None, phase, code)],
                        );
                    }
                };
                self.recovery_generation = expected_generation;
                self.activate_pending(context, &bytes, blob_hash, expected_generation)
            }
            SlotDisposition::Claimed | SlotDisposition::Consumed => {
                self.reconcile_prior_intent(context, disposition, &bytes, blob_hash)
            }
            SlotDisposition::Quarantined => {
                let recovery_generation = match expected_generation(predecessor.as_ref()) {
                    Ok(generation) => generation,
                    Err(error) => {
                        let (phase, code) = store_failure_code(&error);
                        return self.halt(
                            None,
                            RecoveryReasonV1::RecoveryFailure,
                            vec![LaunchFailureReceiptV1::new(None, None, None, phase, code)],
                        );
                    }
                };
                self.recovery_generation = recovery_generation;
                let intent = LaunchIntentV1::authenticate_at_rest(&bytes).ok();
                let source_generation = intent
                    .as_ref()
                    .map(LaunchIntentV1::generation)
                    .filter(|generation| *generation <= recovery_generation);
                self.claim_recovery_and_recover(
                    context,
                    RecoveryCause {
                        source_blob_hash: Some(blob_hash),
                        source_generation,
                        reason: RecoveryReasonV1::InvalidIntent,
                        failures: vec![LaunchFailureReceiptV1::new(
                            intent.as_ref().map(LaunchIntentV1::generation),
                            intent.as_ref().map(LaunchIntentV1::attempt),
                            intent.as_ref().map(LaunchIntentV1::target),
                            LaunchPhaseV1::Quarantine,
                            LaunchFailureCodeV1::IntentStale,
                        )],
                    },
                )
            }
            SlotDisposition::RecoveryClaimed => {
                self.resume_recovery_claim(context, &bytes, blob_hash, predecessor.as_ref())
            }
        }
    }

    fn resume_recovery_claim<S, P>(
        &mut self,
        context: &mut BootstrapContext<'_, S, P>,
        bytes: &[u8],
        blob_hash: CanonicalHash,
        predecessor: Option<&crate::TerminalPredecessor>,
    ) -> BootstrapReportV1
    where
        S: AtomicLaunchIntentStore,
        P: ProcessControl,
    {
        let Ok(request) = RecoveryLaunchRequestV1::authenticate_at_rest(bytes) else {
            return self.halt(
                None,
                RecoveryReasonV1::RecoveryFailure,
                vec![LaunchFailureReceiptV1::new(
                    None,
                    None,
                    None,
                    LaunchPhaseV1::RecoveryClaim,
                    LaunchFailureCodeV1::IntentCorrupt,
                )],
            );
        };
        if predecessor.is_some_and(|source| request.source_blob_hash() != Some(source.blob_hash()))
        {
            return self.halt(
                Some(request.recovery_generation()),
                RecoveryReasonV1::RecoveryFailure,
                vec![LaunchFailureReceiptV1::new(
                    Some(request.recovery_generation()),
                    None,
                    None,
                    LaunchPhaseV1::RecoveryClaim,
                    LaunchFailureCodeV1::IntentChecksumMismatch,
                )],
            );
        }
        if request
            .validate_context(
                request.recovery_generation(),
                context.policy,
                context.process.supervisor_identity(),
            )
            .is_err()
        {
            return self.halt(
                Some(request.recovery_generation()),
                RecoveryReasonV1::RecoveryFailure,
                vec![LaunchFailureReceiptV1::new(
                    Some(request.recovery_generation()),
                    None,
                    None,
                    LaunchPhaseV1::RecoveryClaim,
                    LaunchFailureCodeV1::IntentSelectionMismatch,
                )],
            );
        }
        self.recovery_generation = request.recovery_generation();
        self.recover_claimed(context.process, &request, blob_hash, Vec::new())
    }

    fn reconcile_prior_intent<S, P>(
        &mut self,
        context: &mut BootstrapContext<'_, S, P>,
        disposition: SlotDisposition,
        bytes: &[u8],
        blob_hash: CanonicalHash,
    ) -> BootstrapReportV1
    where
        S: AtomicLaunchIntentStore,
        P: ProcessControl,
    {
        let Ok(intent) = LaunchIntentV1::authenticate_at_rest(bytes) else {
            return self.halt(
                None,
                RecoveryReasonV1::RecoveryFailure,
                vec![LaunchFailureReceiptV1::new(
                    None,
                    None,
                    None,
                    LaunchPhaseV1::IntentClaim,
                    LaunchFailureCodeV1::IntentCorrupt,
                )],
            );
        };
        self.recovery_generation = intent.generation();
        let status = context.process.reconcile_prior_child(&intent);
        match status {
            PriorChildStatusV1::Exited { exit_code } => self.quarantine_and_recover(
                context,
                &intent,
                blob_hash,
                RecoveryReasonV1::BootFailure,
                target_failure_with_detail(
                    &intent,
                    if disposition == SlotDisposition::Claimed {
                        LaunchPhaseV1::Boot
                    } else {
                        LaunchPhaseV1::Ack
                    },
                    LaunchFailureCodeV1::ProcessCrashed,
                    LaunchFailureDetailV1::ProcessExit { exit_code },
                ),
            ),
            PriorChildStatusV1::Running | PriorChildStatusV1::Unknown => self.halt(
                Some(intent.generation()),
                RecoveryReasonV1::RecoveryFailure,
                vec![target_failure_with_detail(
                    &intent,
                    LaunchPhaseV1::IntentClaim,
                    LaunchFailureCodeV1::AttemptAlreadyClaimed,
                    LaunchFailureDetailV1::PriorChild { status },
                )],
            ),
        }
    }
    fn activate_pending<S, P>(
        &mut self,
        context: &mut BootstrapContext<'_, S, P>,
        bytes: &[u8],
        blob_hash: CanonicalHash,
        expected_generation: LaunchGeneration,
    ) -> BootstrapReportV1
    where
        S: AtomicLaunchIntentStore,
        P: ProcessControl,
    {
        let authenticated = LaunchIntentV1::authenticate_at_rest(bytes).ok();
        let intent =
            match LaunchIntentV1::decode_and_validate(bytes, expected_generation, context.policy) {
                Ok(intent) => intent,
                Err(error) => {
                    let generation = authenticated.as_ref().map(LaunchIntentV1::generation);
                    let mut failures = vec![intent_failure_receipt(&error, authenticated.as_ref())];
                    if context.store.quarantine(blob_hash).is_err() {
                        push_failure(
                            &mut failures,
                            LaunchFailureReceiptV1::new(
                                generation,
                                authenticated.as_ref().map(LaunchIntentV1::attempt),
                                authenticated.as_ref().map(LaunchIntentV1::target),
                                LaunchPhaseV1::Quarantine,
                                LaunchFailureCodeV1::QuarantineFailed,
                            ),
                        );
                        return self.halt(generation, RecoveryReasonV1::RecoveryFailure, failures);
                    }
                    return self.claim_recovery_and_recover(
                        context,
                        RecoveryCause {
                            source_blob_hash: Some(blob_hash),
                            source_generation: generation
                                .filter(|value| *value <= expected_generation),
                            reason: RecoveryReasonV1::InvalidIntent,
                            failures,
                        },
                    );
                }
            };
        match context.store.claim(blob_hash) {
            Ok(crate::MutationDurability::Durable) => {}
            Ok(crate::MutationDurability::Indeterminate) => {
                return self.halt(
                    Some(intent.generation()),
                    RecoveryReasonV1::RecoveryFailure,
                    vec![target_failure(
                        &intent,
                        LaunchPhaseV1::IntentSync,
                        LaunchFailureCodeV1::IntentSyncFailed,
                    )],
                );
            }
            Err(error) => {
                let (phase, code) = store_failure_code(&error);
                return self.halt(
                    Some(intent.generation()),
                    RecoveryReasonV1::RecoveryFailure,
                    vec![target_failure(&intent, phase, code)],
                );
            }
        }
        let request = ProcessLaunchRequestV1::Intent(intent.clone());
        let child = match context.process.spawn(&request) {
            Ok(child) => child,
            Err(failure) => {
                return self.quarantine_and_recover(
                    context,
                    &intent,
                    blob_hash,
                    RecoveryReasonV1::SpawnFailure,
                    target_failure_with_detail(
                        &intent,
                        LaunchPhaseV1::Spawn,
                        LaunchFailureCodeV1::SpawnFailed,
                        LaunchFailureDetailV1::Spawn { failure },
                    ),
                );
            }
        };
        self.finish_target_boot(
            context,
            TargetAttempt {
                intent: &intent,
                blob_hash,
                child,
            },
            &request,
        )
    }

    fn finish_target_boot<S, P>(
        &mut self,
        context: &mut BootstrapContext<'_, S, P>,
        attempt: TargetAttempt<'_>,
        request: &ProcessLaunchRequestV1,
    ) -> BootstrapReportV1
    where
        S: AtomicLaunchIntentStore,
        P: ProcessControl,
    {
        match context
            .process
            .await_bootstrap(attempt.child, request, MAX_BOOTSTRAP_ACK_WAIT_MS)
        {
            BootObservationV1::Acknowledged(ack) if ack.matches_intent(attempt.intent) => {
                self.accept_target_ack(context, attempt, ack)
            }
            BootObservationV1::Acknowledged(_) | BootObservationV1::RecoveryAcknowledged(_) => self
                .terminate_target_then_recover(
                    context,
                    attempt,
                    RecoveryReasonV1::AckFailure,
                    target_failure(
                        attempt.intent,
                        LaunchPhaseV1::Ack,
                        LaunchFailureCodeV1::AckInvalid,
                    ),
                ),
            BootObservationV1::BootFailed => self.terminate_target_then_recover(
                context,
                attempt,
                RecoveryReasonV1::BootFailure,
                target_failure(
                    attempt.intent,
                    LaunchPhaseV1::Boot,
                    LaunchFailureCodeV1::BootFailed,
                ),
            ),
            BootObservationV1::Crashed { exit_code } => self.quarantine_and_recover(
                context,
                attempt.intent,
                attempt.blob_hash,
                RecoveryReasonV1::BootFailure,
                target_failure_with_detail(
                    attempt.intent,
                    LaunchPhaseV1::Boot,
                    LaunchFailureCodeV1::ProcessCrashed,
                    LaunchFailureDetailV1::ProcessExit { exit_code },
                ),
            ),
            BootObservationV1::TimedOut => self.terminate_target_then_recover(
                context,
                attempt,
                RecoveryReasonV1::AckFailure,
                target_failure(
                    attempt.intent,
                    LaunchPhaseV1::Ack,
                    LaunchFailureCodeV1::AckTimedOut,
                ),
            ),
        }
    }

    fn accept_target_ack<S, P>(
        &mut self,
        context: &mut BootstrapContext<'_, S, P>,
        attempt: TargetAttempt<'_>,
        ack: BootstrapAckV1,
    ) -> BootstrapReportV1
    where
        S: AtomicLaunchIntentStore,
        P: ProcessControl,
    {
        if context.store.consume(attempt.blob_hash).is_err() {
            return self.terminate_target_then_recover(
                context,
                attempt,
                RecoveryReasonV1::ConsumeFailure,
                target_failure(
                    attempt.intent,
                    LaunchPhaseV1::Consume,
                    LaunchFailureCodeV1::ConsumeFailed,
                ),
            );
        }
        self.state = Some(match attempt.intent.target() {
            LaunchTargetV1::Shell => ClientRuntimeStateV1::Shell {
                generation: attempt.intent.generation(),
                process_epoch: ack.process_epoch(),
                launch_blob_hash: Some(attempt.blob_hash),
            },
            LaunchTargetV1::World { world_id } => ClientRuntimeStateV1::World {
                generation: attempt.intent.generation(),
                process_epoch: ack.process_epoch(),
                world_id,
                launch_blob_hash: attempt.blob_hash,
            },
        });
        BootstrapReportV1::new(
            TransitionOutcomeV1::Activated {
                generation: attempt.intent.generation(),
                target: attempt.intent.target(),
                process_epoch: ack.process_epoch(),
            },
            Vec::new(),
        )
    }

    fn terminate_target_then_recover<S, P>(
        &mut self,
        context: &mut BootstrapContext<'_, S, P>,
        attempt: TargetAttempt<'_>,
        reason: RecoveryReasonV1,
        primary: LaunchFailureReceiptV1,
    ) -> BootstrapReportV1
    where
        S: AtomicLaunchIntentStore,
        P: ProcessControl,
    {
        if let Err(failure) = context.process.terminate(attempt.child) {
            return self.halt(
                Some(attempt.intent.generation()),
                RecoveryReasonV1::RecoveryFailure,
                vec![
                    primary,
                    target_failure_with_detail(
                        attempt.intent,
                        LaunchPhaseV1::Terminate,
                        LaunchFailureCodeV1::TerminationFailed,
                        LaunchFailureDetailV1::Termination { failure },
                    ),
                ],
            );
        }
        self.quarantine_and_recover(context, attempt.intent, attempt.blob_hash, reason, primary)
    }

    fn quarantine_and_recover<S, P>(
        &mut self,
        context: &mut BootstrapContext<'_, S, P>,
        intent: &LaunchIntentV1,
        blob_hash: CanonicalHash,
        reason: RecoveryReasonV1,
        primary: LaunchFailureReceiptV1,
    ) -> BootstrapReportV1
    where
        S: AtomicLaunchIntentStore,
        P: ProcessControl,
    {
        let mut failures = vec![primary];
        if context.store.quarantine(blob_hash).is_err() {
            push_failure(
                &mut failures,
                LaunchFailureReceiptV1::new(
                    Some(intent.generation()),
                    Some(intent.attempt()),
                    Some(intent.target()),
                    LaunchPhaseV1::Quarantine,
                    LaunchFailureCodeV1::QuarantineFailed,
                ),
            );
            return self.halt(
                Some(intent.generation()),
                RecoveryReasonV1::RecoveryFailure,
                failures,
            );
        }
        self.claim_recovery_and_recover(
            context,
            RecoveryCause {
                source_blob_hash: Some(blob_hash),
                source_generation: Some(intent.generation()),
                reason,
                failures,
            },
        )
    }
    fn claim_recovery_and_recover<S, P>(
        &mut self,
        context: &mut BootstrapContext<'_, S, P>,
        mut cause: RecoveryCause,
    ) -> BootstrapReportV1
    where
        S: AtomicLaunchIntentStore,
        P: ProcessControl,
    {
        let primary_failure = cause.failures.first().copied().unwrap_or_else(|| {
            LaunchFailureReceiptV1::new(
                cause.source_generation,
                None,
                None,
                LaunchPhaseV1::RecoveryClaim,
                LaunchFailureCodeV1::IntentCorrupt,
            )
        });
        let Ok(request) = RecoveryLaunchRequestV1::seal(RecoveryLaunchDraftV1 {
            recovery_generation: self.recovery_generation,
            source_generation: cause.source_generation,
            source_blob_hash: cause.source_blob_hash,
            reason: cause.reason,
            shell_lock_hash: context.policy.expected_shell_lock_hash(),
            confirmed_setting_transaction_revision: context
                .policy
                .expected_confirmed_setting_transaction_revision(),
            supervisor_identity: context.process.supervisor_identity(),
            failure: primary_failure,
        }) else {
            push_recovery_claim_failure(&mut cause.failures, cause.source_generation);
            return self.halt(
                cause.source_generation,
                RecoveryReasonV1::RecoveryFailure,
                cause.failures,
            );
        };
        let Ok(request_bytes) = request.canonical_bytes() else {
            push_recovery_claim_failure(&mut cause.failures, cause.source_generation);
            return self.halt(
                cause.source_generation,
                RecoveryReasonV1::RecoveryFailure,
                cause.failures,
            );
        };
        let Ok(outcome) = context
            .store
            .claim_recovery(cause.source_blob_hash, &request_bytes)
        else {
            push_recovery_claim_failure(&mut cause.failures, cause.source_generation);
            return self.halt(
                cause.source_generation,
                RecoveryReasonV1::RecoveryFailure,
                cause.failures,
            );
        };
        if outcome.blob_hash() != CanonicalHash::digest(&request_bytes) {
            push_recovery_claim_failure(&mut cause.failures, cause.source_generation);
            return self.halt(
                cause.source_generation,
                RecoveryReasonV1::RecoveryFailure,
                cause.failures,
            );
        }
        if outcome.durability() == crate::MutationDurability::Indeterminate {
            push_failure(
                &mut cause.failures,
                LaunchFailureReceiptV1::new(
                    cause.source_generation,
                    None,
                    None,
                    LaunchPhaseV1::IntentSync,
                    LaunchFailureCodeV1::IntentSyncFailed,
                ),
            );
            return self.halt(
                cause.source_generation,
                RecoveryReasonV1::RecoveryFailure,
                cause.failures,
            );
        }
        self.recover_claimed(
            context.process,
            &request,
            outcome.blob_hash(),
            cause.failures,
        )
    }

    fn recover_claimed<P>(
        &mut self,
        process: &mut P,
        request: &RecoveryLaunchRequestV1,
        recovery_claim_hash: CanonicalHash,
        mut failures: Vec<LaunchFailureReceiptV1>,
    ) -> BootstrapReportV1
    where
        P: ProcessControl,
    {
        let source_generation = request.source_generation();
        let child = match process.acquire_recovery(request) {
            Ok(crate::RecoveryChildStatusV1::Acquired(child)) => child,
            Ok(crate::RecoveryChildStatusV1::Exited { exit_code }) => {
                push_failure(
                    &mut failures,
                    LaunchFailureReceiptV1::with_detail(
                        source_generation,
                        None,
                        None,
                        LaunchPhaseV1::RecoverySpawn,
                        LaunchFailureCodeV1::RecoveryLoopSuppressed,
                        LaunchFailureDetailV1::ProcessExit { exit_code },
                    ),
                );
                return self.halt(
                    source_generation,
                    RecoveryReasonV1::RecoveryFailure,
                    failures,
                );
            }
            Ok(crate::RecoveryChildStatusV1::Unknown) => {
                push_failure(
                    &mut failures,
                    LaunchFailureReceiptV1::new(
                        source_generation,
                        None,
                        None,
                        LaunchPhaseV1::RecoverySpawn,
                        LaunchFailureCodeV1::RecoveryLoopSuppressed,
                    ),
                );
                return self.halt(
                    source_generation,
                    RecoveryReasonV1::RecoveryFailure,
                    failures,
                );
            }
            Err(failure) => {
                push_failure(
                    &mut failures,
                    LaunchFailureReceiptV1::with_detail(
                        source_generation,
                        None,
                        None,
                        LaunchPhaseV1::RecoverySpawn,
                        LaunchFailureCodeV1::SpawnFailed,
                        LaunchFailureDetailV1::Spawn { failure },
                    ),
                );
                return self.halt(
                    source_generation,
                    RecoveryReasonV1::RecoveryFailure,
                    failures,
                );
            }
        };
        let process_request = ProcessLaunchRequestV1::RecoveryShell(request.clone());
        let observation =
            process.await_bootstrap(child, &process_request, MAX_BOOTSTRAP_ACK_WAIT_MS);
        self.finish_recovery_boot(
            process,
            request,
            recovery_claim_hash,
            child,
            observation,
            failures,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn finish_recovery_boot<P>(
        &mut self,
        process: &mut P,
        request: &RecoveryLaunchRequestV1,
        recovery_claim_hash: CanonicalHash,
        child: SpawnedProcess,
        observation: BootObservationV1,
        mut failures: Vec<LaunchFailureReceiptV1>,
    ) -> BootstrapReportV1
    where
        P: ProcessControl,
    {
        let source_generation = request.source_generation();
        match observation {
            BootObservationV1::RecoveryAcknowledged(ack) if ack.matches_request(request) => {
                let process_epoch = ack.process_epoch();
                self.state = Some(ClientRuntimeStateV1::Recovery {
                    generation: request.recovery_generation(),
                    process_epoch,
                    reason: request.reason(),
                    recovery_claim_hash,
                });
                BootstrapReportV1::new(
                    TransitionOutcomeV1::RecoveryShell {
                        source_generation,
                        process_epoch,
                        reason: request.reason(),
                    },
                    failures,
                )
            }
            BootObservationV1::RecoveryAcknowledged(_) | BootObservationV1::Acknowledged(_) => {
                push_failure(
                    &mut failures,
                    LaunchFailureReceiptV1::new(
                        source_generation,
                        None,
                        None,
                        LaunchPhaseV1::RecoveryAck,
                        LaunchFailureCodeV1::AckInvalid,
                    ),
                );
                self.terminate_recovery_and_halt(process, child, source_generation, failures)
            }
            BootObservationV1::BootFailed => {
                push_failure(
                    &mut failures,
                    LaunchFailureReceiptV1::new(
                        source_generation,
                        None,
                        None,
                        LaunchPhaseV1::RecoveryBoot,
                        LaunchFailureCodeV1::BootFailed,
                    ),
                );
                self.terminate_recovery_and_halt(process, child, source_generation, failures)
            }
            BootObservationV1::Crashed { exit_code } => {
                push_failure(
                    &mut failures,
                    LaunchFailureReceiptV1::with_detail(
                        source_generation,
                        None,
                        None,
                        LaunchPhaseV1::RecoveryBoot,
                        LaunchFailureCodeV1::ProcessCrashed,
                        LaunchFailureDetailV1::ProcessExit { exit_code },
                    ),
                );
                self.halt(
                    source_generation,
                    RecoveryReasonV1::RecoveryFailure,
                    failures,
                )
            }
            BootObservationV1::TimedOut => {
                push_failure(
                    &mut failures,
                    LaunchFailureReceiptV1::new(
                        source_generation,
                        None,
                        None,
                        LaunchPhaseV1::RecoveryAck,
                        LaunchFailureCodeV1::AckTimedOut,
                    ),
                );
                self.terminate_recovery_and_halt(process, child, source_generation, failures)
            }
        }
    }

    fn terminate_recovery_and_halt<P>(
        &mut self,
        process: &mut P,
        child: SpawnedProcess,
        generation: Option<LaunchGeneration>,
        mut failures: Vec<LaunchFailureReceiptV1>,
    ) -> BootstrapReportV1
    where
        P: ProcessControl,
    {
        if let Err(failure) = process.terminate(child) {
            push_failure(
                &mut failures,
                LaunchFailureReceiptV1::with_detail(
                    generation,
                    None,
                    None,
                    LaunchPhaseV1::Terminate,
                    LaunchFailureCodeV1::TerminationFailed,
                    LaunchFailureDetailV1::Termination { failure },
                ),
            );
        }
        self.halt(generation, RecoveryReasonV1::RecoveryFailure, failures)
    }

    fn halt(
        &mut self,
        generation: Option<LaunchGeneration>,
        reason: RecoveryReasonV1,
        failures: Vec<LaunchFailureReceiptV1>,
    ) -> BootstrapReportV1 {
        if let Some(generation) = generation {
            self.recovery_generation = self.recovery_generation.max(generation);
        }
        self.state = Some(ClientRuntimeStateV1::Halted {
            generation: Some(self.recovery_generation),
            reason,
        });
        BootstrapReportV1::new(TransitionOutcomeV1::Halted, failures)
    }
}

fn barrier_receipt_failure(
    state: ClientRuntimeStateV1,
    intent: &LaunchIntentV1,
    receipt: ShutdownBarrierReceiptV1,
) -> Option<ShutdownBarrierFailureV1> {
    let durability_matches = match state {
        ClientRuntimeStateV1::World { world_id, .. } => receipt
            .durable_world()
            .is_some_and(|durable| durable.world_id() == world_id),
        ClientRuntimeStateV1::Shell { .. } | ClientRuntimeStateV1::Recovery { .. } => {
            receipt.durable_world().is_none()
        }
        ClientRuntimeStateV1::RecoveryHandoffPublished { .. }
        | ClientRuntimeStateV1::HandoffPublished { .. }
        | ClientRuntimeStateV1::Halted { .. } => false,
    };
    if !durability_matches {
        return Some(ShutdownBarrierFailureV1::new(
            crate::ShutdownBarrierStageV1::ConfirmDurability,
            FailureDispositionV1::RecoveryRequired,
        ));
    }
    if intent.confirmed_setting_transaction_revision()
        != receipt.confirmed_setting_transaction_revision()
    {
        return Some(ShutdownBarrierFailureV1::new(
            crate::ShutdownBarrierStageV1::FlushSettings,
            FailureDispositionV1::RecoveryRequired,
        ));
    }
    None
}
fn current_terminal_blob_hash<S>(
    store: &mut S,
    current_generation: LaunchGeneration,
    state: ClientRuntimeStateV1,
) -> Result<Option<CanonicalHash>, IntentStoreError>
where
    S: AtomicLaunchIntentStore,
{
    let expected_blob_hash = state.boot_slot_blob_hash();
    let IntentSlot::Occupied {
        disposition,
        bytes,
        blob_hash,
        ..
    } = store.read()?
    else {
        return if expected_blob_hash.is_none() {
            Ok(None)
        } else {
            Err(IntentStoreError::UnexpectedState {
                expected: "exact active-process terminal",
                actual: "empty",
            })
        };
    };
    let Some(expected_blob_hash) = expected_blob_hash else {
        return Err(IntentStoreError::Occupied);
    };
    if blob_hash != expected_blob_hash {
        return Err(IntentStoreError::BlobMismatch);
    }
    match state {
        ClientRuntimeStateV1::Shell { .. } | ClientRuntimeStateV1::World { .. } => {
            if disposition != SlotDisposition::Consumed {
                return Err(IntentStoreError::UnexpectedState {
                    expected: SlotDisposition::Consumed.as_str(),
                    actual: disposition.as_str(),
                });
            }
            let intent = LaunchIntentV1::authenticate_at_rest(&bytes)
                .map_err(|_| IntentStoreError::BlobMismatch)?;
            let expected_target = match state {
                ClientRuntimeStateV1::Shell { .. } => LaunchTargetV1::Shell,
                ClientRuntimeStateV1::World { world_id, .. } => LaunchTargetV1::World { world_id },
                _ => return Err(IntentStoreError::BlobMismatch),
            };
            if intent.generation() != current_generation || intent.target() != expected_target {
                return Err(IntentStoreError::BlobMismatch);
            }
        }
        ClientRuntimeStateV1::Recovery { reason, .. } => {
            if disposition != SlotDisposition::RecoveryClaimed {
                return Err(IntentStoreError::UnexpectedState {
                    expected: SlotDisposition::RecoveryClaimed.as_str(),
                    actual: disposition.as_str(),
                });
            }
            let request = RecoveryLaunchRequestV1::authenticate_at_rest(&bytes)
                .map_err(|_| IntentStoreError::BlobMismatch)?;
            if request.recovery_generation() != current_generation || request.reason() != reason {
                return Err(IntentStoreError::BlobMismatch);
            }
        }
        ClientRuntimeStateV1::RecoveryHandoffPublished { .. }
        | ClientRuntimeStateV1::HandoffPublished { .. }
        | ClientRuntimeStateV1::Halted { .. } => {
            return Err(IntentStoreError::UnexpectedState {
                expected: "active process",
                actual: "inactive process",
            });
        }
    }
    Ok(Some(blob_hash))
}
fn expected_generation(
    predecessor: Option<&crate::TerminalPredecessor>,
) -> Result<LaunchGeneration, IntentStoreError> {
    let Some(predecessor) = predecessor else {
        return LaunchGeneration::FIRST
            .next()
            .map_err(|_| IntentStoreError::BlobMismatch);
    };
    let generation = match predecessor.disposition() {
        SlotDisposition::Consumed => LaunchIntentV1::authenticate_at_rest(predecessor.bytes())
            .map(|intent| intent.generation())
            .map_err(|_| IntentStoreError::BlobMismatch)?,
        SlotDisposition::RecoveryClaimed => {
            RecoveryLaunchRequestV1::authenticate_at_rest(predecessor.bytes())
                .map(|request| request.recovery_generation())
                .map_err(|_| IntentStoreError::BlobMismatch)?
        }
        SlotDisposition::Pending | SlotDisposition::Claimed | SlotDisposition::Quarantined => {
            return Err(IntentStoreError::UnexpectedState {
                expected: "authenticated consumed or recovery predecessor",
                actual: predecessor.disposition().as_str(),
            });
        }
    };
    generation
        .next()
        .map_err(|_| IntentStoreError::BlobMismatch)
}

fn push_recovery_claim_failure(
    failures: &mut Vec<LaunchFailureReceiptV1>,
    source_generation: Option<LaunchGeneration>,
) {
    push_failure(
        failures,
        LaunchFailureReceiptV1::new(
            source_generation,
            None,
            None,
            LaunchPhaseV1::RecoveryClaim,
            LaunchFailureCodeV1::RecoveryClaimFailed,
        ),
    );
}
fn target_failure(
    intent: &LaunchIntentV1,
    phase: LaunchPhaseV1,
    code: LaunchFailureCodeV1,
) -> LaunchFailureReceiptV1 {
    LaunchFailureReceiptV1::new(
        Some(intent.generation()),
        Some(intent.attempt()),
        Some(intent.target()),
        phase,
        code,
    )
}

fn target_failure_with_detail(
    intent: &LaunchIntentV1,
    phase: LaunchPhaseV1,
    code: LaunchFailureCodeV1,
    detail: LaunchFailureDetailV1,
) -> LaunchFailureReceiptV1 {
    LaunchFailureReceiptV1::with_detail(
        Some(intent.generation()),
        Some(intent.attempt()),
        Some(intent.target()),
        phase,
        code,
        detail,
    )
}

fn intent_failure_receipt(
    error: &LaunchIntentError,
    intent: Option<&LaunchIntentV1>,
) -> LaunchFailureReceiptV1 {
    let code = match error {
        LaunchIntentError::ChecksumMismatch { .. } => LaunchFailureCodeV1::IntentChecksumMismatch,
        LaunchIntentError::StaleGeneration { .. } | LaunchIntentError::GenerationGap { .. } => {
            LaunchFailureCodeV1::IntentStale
        }
        LaunchIntentError::Expired { .. } | LaunchIntentError::NotYetValid { .. } => {
            LaunchFailureCodeV1::IntentExpired
        }
        LaunchIntentError::ShellLockMismatch
        | LaunchIntentError::ConfirmedSettingsRevisionMismatch
        | LaunchIntentError::TargetMismatch
        | LaunchIntentError::WorldLockMismatch
        | LaunchIntentError::WorldOpenPlanMismatch => LaunchFailureCodeV1::IntentSelectionMismatch,
        LaunchIntentError::InputTooLarge { .. }
        | LaunchIntentError::InvalidJson { .. }
        | LaunchIntentError::NonCanonicalBytes
        | LaunchIntentError::UnsupportedSchema { .. }
        | LaunchIntentError::InvalidTargetHashes
        | LaunchIntentError::InvalidAttempt { .. }
        | LaunchIntentError::InvalidLifetime => LaunchFailureCodeV1::IntentCorrupt,
    };
    LaunchFailureReceiptV1::new(
        intent.map(LaunchIntentV1::generation),
        intent.map(LaunchIntentV1::attempt),
        intent.map(LaunchIntentV1::target),
        LaunchPhaseV1::IntentValidate,
        code,
    )
}

fn store_failure_code(error: &IntentStoreError) -> (LaunchPhaseV1, LaunchFailureCodeV1) {
    match error {
        IntentStoreError::Io { operation, .. } => match operation {
            StoreOperation::Read => (
                LaunchPhaseV1::IntentRead,
                LaunchFailureCodeV1::IntentReadFailed,
            ),
            StoreOperation::WriteTemporary => (
                LaunchPhaseV1::IntentWrite,
                LaunchFailureCodeV1::IntentWriteFailed,
            ),
            StoreOperation::SyncTemporary | StoreOperation::SyncDirectory => (
                LaunchPhaseV1::IntentSync,
                LaunchFailureCodeV1::IntentSyncFailed,
            ),
            StoreOperation::Replace | StoreOperation::Retire => (
                LaunchPhaseV1::IntentReplace,
                LaunchFailureCodeV1::IntentReplaceFailed,
            ),
        },
        IntentStoreError::Occupied | IntentStoreError::ConflictingSlots => (
            LaunchPhaseV1::IntentReplace,
            LaunchFailureCodeV1::IntentSlotConflict,
        ),
        IntentStoreError::UnexpectedState { actual, .. } if *actual == "claimed" => (
            LaunchPhaseV1::IntentClaim,
            LaunchFailureCodeV1::AttemptAlreadyClaimed,
        ),
        IntentStoreError::UnsafeRoot
        | IntentStoreError::UnsafeSlot { .. }
        | IntentStoreError::SlotTooLarge { .. }
        | IntentStoreError::UnexpectedState { .. }
        | IntentStoreError::BlobMismatch => (
            LaunchPhaseV1::IntentRead,
            LaunchFailureCodeV1::IntentCorrupt,
        ),
    }
}

fn push_failure(failures: &mut Vec<LaunchFailureReceiptV1>, failure: LaunchFailureReceiptV1) {
    if failures.len() < MAX_REPORT_FAILURES {
        failures.push(failure);
    }
}

#[cfg(test)]
mod tests {
    use std::{
        collections::VecDeque,
        fs, io,
        path::PathBuf,
        sync::atomic::{AtomicU64, Ordering},
    };

    use latticeaxiom_core::WorldId;

    use crate::model::RecoveryLaunchDraftV1;

    use crate::{
        BootstrapAckV1, DurableWorldRevisionV1, FileLaunchIntentStore, LaunchAttempt,
        MutationDurability, ProcessEpoch, ProcessSupervisorIdentityV1, RecoveryBootstrapAckV1,
        RecoveryChildStatusV1, RecoveryClaimOutcome, SettingTransactionRevision, SpawnFailureV1,
        SpawnedProcess, TerminalPredecessor, TerminationFailureV1, WorldRevision,
    };

    use super::*;

    static NEXT_MACHINE_TEMP: AtomicU64 = AtomicU64::new(1);

    #[derive(Debug)]
    struct MachineTestDirectory(PathBuf);

    impl MachineTestDirectory {
        fn create() -> Self {
            let temporary_root = fs::canonicalize(std::env::temp_dir())
                .unwrap_or_else(|error| panic!("temporary root did not canonicalize: {error}"));
            let serial = NEXT_MACHINE_TEMP.fetch_add(1, Ordering::Relaxed);
            let path = temporary_root.join(format!(
                "latticeaxiom-launcher-machine-{}-{serial}",
                std::process::id()
            ));
            fs::create_dir(&path)
                .unwrap_or_else(|error| panic!("machine test directory was not created: {error}"));
            let canonical = fs::canonicalize(path).unwrap_or_else(|error| {
                panic!("machine test directory did not canonicalize: {error}")
            });
            assert!(canonical.starts_with(&temporary_root));
            Self(canonical)
        }
    }

    impl Drop for MachineTestDirectory {
        fn drop(&mut self) {
            let temporary_root = fs::canonicalize(std::env::temp_dir())
                .unwrap_or_else(|error| panic!("temporary root did not canonicalize: {error}"));
            assert!(self.0.starts_with(&temporary_root));
            if let Err(error) = fs::remove_dir_all(&self.0)
                && error.kind() != io::ErrorKind::NotFound
            {
                panic!("machine test directory cleanup failed: {error}");
            }
        }
    }

    #[derive(Debug, Default)]
    struct MemoryStore {
        slot: IntentSlot,
        fail: Option<StoreOperation>,
        replace_calls: usize,
        fail_replace_call: Option<usize>,
    }

    impl MemoryStore {
        fn fail_next(&mut self, operation: StoreOperation) {
            self.fail = Some(operation);
        }

        fn fail_replace_after(&mut self, calls: usize) {
            self.fail_replace_call = self.replace_calls.checked_add(calls);
        }

        fn check(&mut self, operation: StoreOperation) -> Result<(), IntentStoreError> {
            if operation == StoreOperation::Replace {
                self.replace_calls = self.replace_calls.saturating_add(1);
                if self.fail_replace_call == Some(self.replace_calls) {
                    self.fail_replace_call = None;
                    return Err(IntentStoreError::Io {
                        operation,
                        source: io::Error::other("injected replace call"),
                    });
                }
            }
            if self.fail == Some(operation) {
                self.fail = None;
                return Err(IntentStoreError::Io {
                    operation,
                    source: io::Error::other("injected"),
                });
            }
            Ok(())
        }

        fn committed_sync(&mut self) -> MutationDurability {
            if self.check(StoreOperation::SyncDirectory).is_ok() {
                MutationDurability::Durable
            } else {
                MutationDurability::Indeterminate
            }
        }

        fn move_to(
            &mut self,
            expected: SlotDisposition,
            next: SlotDisposition,
            hash: CanonicalHash,
        ) -> Result<MutationDurability, IntentStoreError> {
            let IntentSlot::Occupied {
                disposition,
                bytes,
                blob_hash,
                predecessor,
            } = &self.slot
            else {
                return Err(IntentStoreError::UnexpectedState {
                    expected: expected.as_str(),
                    actual: "empty",
                });
            };
            if *disposition != expected {
                return Err(IntentStoreError::UnexpectedState {
                    expected: expected.as_str(),
                    actual: disposition.as_str(),
                });
            }
            if *blob_hash != hash {
                return Err(IntentStoreError::BlobMismatch);
            }
            let bytes = bytes.clone();
            let predecessor = predecessor.clone();
            self.check(StoreOperation::Replace)?;
            let retained = if matches!(
                next,
                SlotDisposition::Claimed | SlotDisposition::Quarantined
            ) {
                predecessor
            } else {
                None
            };
            self.slot = IntentSlot::occupied_with_predecessor(next, bytes.clone(), retained)?;
            let mut durability = self.committed_sync();
            if durability == MutationDurability::Durable
                && next == SlotDisposition::Claimed
                && self.slot.predecessor().is_some()
            {
                if self.check(StoreOperation::Retire).is_err() {
                    return Ok(MutationDurability::Indeterminate);
                }
                self.slot = IntentSlot::occupied(next, bytes)?;
                durability = self.committed_sync();
            }
            Ok(durability)
        }
    }

    impl AtomicLaunchIntentStore for MemoryStore {
        fn read(&mut self) -> Result<IntentSlot, IntentStoreError> {
            self.check(StoreOperation::Read)?;
            Ok(self.slot.clone())
        }

        fn publish(
            &mut self,
            canonical_bytes: &[u8],
        ) -> Result<PublishDisposition, IntentStoreError> {
            self.check(StoreOperation::WriteTemporary)?;
            self.check(StoreOperation::SyncTemporary)?;
            match &self.slot {
                IntentSlot::Empty => {
                    self.check(StoreOperation::Replace)?;
                    self.slot =
                        IntentSlot::occupied(SlotDisposition::Pending, canonical_bytes.to_vec())?;
                    Ok(match self.committed_sync() {
                        MutationDurability::Durable => PublishDisposition::Published,
                        MutationDurability::Indeterminate => {
                            PublishDisposition::PublishedIndeterminate
                        }
                    })
                }
                IntentSlot::Occupied {
                    disposition, bytes, ..
                } if bytes == canonical_bytes => Ok(match disposition {
                    SlotDisposition::Pending => PublishDisposition::AlreadyPublished,
                    handled => PublishDisposition::AlreadyHandled(*handled),
                }),
                IntentSlot::Occupied { .. } => Err(IntentStoreError::Occupied),
            }
        }

        fn publish_replacing_terminal(
            &mut self,
            expected_terminal_blob_hash: CanonicalHash,
            canonical_bytes: &[u8],
        ) -> Result<PublishDisposition, IntentStoreError> {
            if matches!(
                &self.slot,
                IntentSlot::Occupied {
                    disposition: SlotDisposition::Pending,
                    bytes,
                    ..
                } if bytes == canonical_bytes
            ) {
                return Ok(PublishDisposition::AlreadyPublished);
            }
            let IntentSlot::Occupied {
                disposition,
                bytes: terminal_bytes,
                blob_hash,
                ..
            } = &self.slot
            else {
                return Err(IntentStoreError::BlobMismatch);
            };
            if *blob_hash != expected_terminal_blob_hash || !disposition.is_terminal() {
                return Err(IntentStoreError::BlobMismatch);
            }
            let predecessor = TerminalPredecessor::occupied(*disposition, terminal_bytes.clone())?;
            self.check(StoreOperation::WriteTemporary)?;
            self.check(StoreOperation::SyncTemporary)?;
            self.check(StoreOperation::Replace)?;
            self.slot = IntentSlot::occupied_with_predecessor(
                SlotDisposition::Pending,
                canonical_bytes.to_vec(),
                Some(predecessor),
            )?;
            Ok(match self.committed_sync() {
                MutationDurability::Durable => PublishDisposition::Published,
                MutationDurability::Indeterminate => PublishDisposition::PublishedIndeterminate,
            })
        }

        fn claim(
            &mut self,
            expected_blob_hash: CanonicalHash,
        ) -> Result<MutationDurability, IntentStoreError> {
            self.move_to(
                SlotDisposition::Pending,
                SlotDisposition::Claimed,
                expected_blob_hash,
            )
        }

        fn consume(
            &mut self,
            expected_blob_hash: CanonicalHash,
        ) -> Result<MutationDurability, IntentStoreError> {
            self.move_to(
                SlotDisposition::Claimed,
                SlotDisposition::Consumed,
                expected_blob_hash,
            )
        }

        fn quarantine(
            &mut self,
            expected_blob_hash: CanonicalHash,
        ) -> Result<MutationDurability, IntentStoreError> {
            let disposition = self
                .slot
                .disposition()
                .ok_or(IntentStoreError::UnexpectedState {
                    expected: "pending, claimed, or consumed",
                    actual: "empty",
                })?;
            if !matches!(
                disposition,
                SlotDisposition::Pending | SlotDisposition::Claimed | SlotDisposition::Consumed
            ) {
                return Err(IntentStoreError::UnexpectedState {
                    expected: "pending, claimed, or consumed",
                    actual: disposition.as_str(),
                });
            }
            self.move_to(
                disposition,
                SlotDisposition::Quarantined,
                expected_blob_hash,
            )
        }

        fn claim_recovery(
            &mut self,
            expected_terminal_blob_hash: Option<CanonicalHash>,
            recovery_request_bytes: &[u8],
        ) -> Result<RecoveryClaimOutcome, IntentStoreError> {
            let request_hash = CanonicalHash::digest(recovery_request_bytes);
            if matches!(
                &self.slot,
                IntentSlot::Occupied {
                    disposition: SlotDisposition::RecoveryClaimed,
                    bytes,
                    ..
                } if bytes == recovery_request_bytes
            ) {
                return Ok(RecoveryClaimOutcome::new(
                    request_hash,
                    MutationDurability::Durable,
                ));
            }
            let predecessor = match expected_terminal_blob_hash {
                Some(expected_hash) => {
                    let IntentSlot::Occupied {
                        disposition,
                        bytes,
                        blob_hash,
                        ..
                    } = &self.slot
                    else {
                        return Err(IntentStoreError::BlobMismatch);
                    };
                    if *blob_hash != expected_hash || !disposition.is_terminal() {
                        return Err(IntentStoreError::BlobMismatch);
                    }
                    Some(TerminalPredecessor::occupied(*disposition, bytes.clone())?)
                }
                None if self.slot == IntentSlot::Empty => None,
                None => return Err(IntentStoreError::Occupied),
            };
            self.check(StoreOperation::WriteTemporary)?;
            self.check(StoreOperation::SyncTemporary)?;
            self.check(StoreOperation::Replace)?;
            self.slot = IntentSlot::occupied_with_predecessor(
                SlotDisposition::RecoveryClaimed,
                recovery_request_bytes.to_vec(),
                predecessor,
            )?;
            let mut durability = self.committed_sync();
            if durability == MutationDurability::Durable && self.slot.predecessor().is_some() {
                if self.check(StoreOperation::Retire).is_err() {
                    durability = MutationDurability::Indeterminate;
                } else {
                    self.slot = IntentSlot::occupied(
                        SlotDisposition::RecoveryClaimed,
                        recovery_request_bytes.to_vec(),
                    )?;
                    durability = self.committed_sync();
                }
            }
            Ok(RecoveryClaimOutcome::new(request_hash, durability))
        }
    }
    #[derive(Debug)]
    struct FixedBarrier(Result<ShutdownBarrierReceiptV1, ShutdownBarrierFailureV1>);

    impl TransitionBarrier for FixedBarrier {
        fn reach(
            &mut self,
            _current: ClientRuntimeStateV1,
            _target: LaunchTargetV1,
        ) -> Result<ShutdownBarrierReceiptV1, ShutdownBarrierFailureV1> {
            self.0
        }
    }

    #[derive(Debug, Default)]
    struct FakeProcess {
        reconciliations: VecDeque<PriorChildStatusV1>,
        recovery_reconciliations: VecDeque<RecoveryChildStatusV1>,
        spawn_failures: VecDeque<Option<SpawnFailureV1>>,
        observations: VecDeque<BootObservationV1>,
        termination_failures: VecDeque<Option<TerminationFailureV1>>,
        acknowledge_recovery: bool,
        recovery_status: Option<RecoveryChildStatusV1>,
        spawn_count: usize,
        terminated: usize,
    }

    impl ProcessControl for FakeProcess {
        fn supervisor_identity(&self) -> ProcessSupervisorIdentityV1 {
            supervisor_identity()
        }

        fn reconcile_prior_child(&mut self, _intent: &LaunchIntentV1) -> PriorChildStatusV1 {
            self.reconciliations
                .pop_front()
                .unwrap_or(PriorChildStatusV1::Unknown)
        }

        fn spawn(
            &mut self,
            _request: &ProcessLaunchRequestV1,
        ) -> Result<SpawnedProcess, SpawnFailureV1> {
            self.spawn_count = self.spawn_count.saturating_add(1);
            if let Some(Some(failure)) = self.spawn_failures.pop_front() {
                return Err(failure);
            }
            SpawnedProcess::new(u64::try_from(self.spawn_count).unwrap_or(u64::MAX))
                .map_err(|_| SpawnFailureV1::ResourceUnavailable)
        }

        fn acquire_recovery(
            &mut self,
            _request: &RecoveryLaunchRequestV1,
        ) -> Result<RecoveryChildStatusV1, SpawnFailureV1> {
            if let Some(status) = self.recovery_reconciliations.pop_front() {
                self.recovery_status = Some(status);
                return Ok(status);
            }
            if let Some(status) = self.recovery_status {
                return Ok(status);
            }
            self.spawn_count = self.spawn_count.saturating_add(1);
            if let Some(Some(failure)) = self.spawn_failures.pop_front() {
                return Err(failure);
            }
            let process = SpawnedProcess::new(u64::try_from(self.spawn_count).unwrap_or(u64::MAX))
                .map_err(|_| SpawnFailureV1::ResourceUnavailable)?;
            let status = RecoveryChildStatusV1::Acquired(process);
            self.recovery_status = Some(status);
            Ok(status)
        }

        fn await_bootstrap(
            &mut self,
            _process: SpawnedProcess,
            request: &ProcessLaunchRequestV1,
            deadline_ms: u64,
        ) -> BootObservationV1 {
            assert_eq!(deadline_ms, MAX_BOOTSTRAP_ACK_WAIT_MS);
            if self.acknowledge_recovery
                && let ProcessLaunchRequestV1::RecoveryShell(recovery_request) = request
            {
                let ack = RecoveryBootstrapAckV1::for_ready_request(
                    recovery_request,
                    crate::model::test_app_created_proof(epoch(99)),
                );
                return BootObservationV1::RecoveryAcknowledged(ack);
            }
            self.observations
                .pop_front()
                .unwrap_or(BootObservationV1::TimedOut)
        }

        fn terminate(&mut self, process: SpawnedProcess) -> Result<(), TerminationFailureV1> {
            self.terminated = self.terminated.saturating_add(1);
            if let Some(Some(failure)) = self.termination_failures.pop_front() {
                return Err(failure);
            }
            if matches!(
                self.recovery_status,
                Some(RecoveryChildStatusV1::Acquired(recovery)) if recovery == process
            ) {
                self.recovery_status = Some(RecoveryChildStatusV1::Exited { exit_code: None });
            }
            Ok(())
        }
    }
    fn generation(value: u64) -> LaunchGeneration {
        LaunchGeneration::new(value)
            .unwrap_or_else(|error| panic!("test generation must be non-zero: {error}"))
    }

    fn epoch(value: u64) -> ProcessEpoch {
        ProcessEpoch::new(value)
            .unwrap_or_else(|error| panic!("test epoch must be non-zero: {error}"))
    }

    fn supervisor_identity() -> ProcessSupervisorIdentityV1 {
        ProcessSupervisorIdentityV1::new(CanonicalHash::digest(b"test process supervisor"))
    }

    const fn setting_revision(value: u64) -> SettingTransactionRevision {
        SettingTransactionRevision::new(value)
    }

    const fn world_revision(value: u64) -> WorldRevision {
        WorldRevision::new(value)
    }

    fn world_draft(world_id: WorldId) -> LaunchIntentDraftV1 {
        LaunchIntentDraftV1 {
            generation: generation(2),
            attempt: LaunchAttempt::FIRST,
            issued_at_ms: 1_000,
            expires_at_ms: 2_000,
            target: LaunchTargetV1::World { world_id },
            shell_lock_hash: CanonicalHash::digest(b"shell"),
            world_lock_hash: Some(CanonicalHash::digest(b"world")),
            world_open_plan_hash: Some(CanonicalHash::digest(b"plan")),
            confirmed_setting_transaction_revision: setting_revision(9),
        }
    }

    fn initial_shell() -> ClientRuntimeStateV1 {
        ClientRuntimeStateV1::Shell {
            generation: generation(1),
            process_epoch: epoch(1),
            launch_blob_hash: None,
        }
    }

    fn publish_world(store: &mut MemoryStore, world_id: WorldId) -> LaunchIntentV1 {
        let mut machine = ClientTransitionMachine::from_active(initial_shell())
            .unwrap_or_else(|error| panic!("active state was rejected: {error}"));
        let mut barrier = FixedBarrier(Ok(ShutdownBarrierReceiptV1::complete(
            setting_revision(9),
            None,
        )));
        machine
            .publish_handoff(
                world_draft(world_id),
                store,
                &mut barrier,
                supervisor_identity(),
            )
            .unwrap_or_else(|error| panic!("handoff was rejected: {error}"))
            .intent
            .unwrap_or_else(|| panic!("handoff did not publish an intent"))
    }

    fn world_policy(world_id: WorldId) -> TransitionValidationPolicy {
        TransitionValidationPolicy::for_world(
            1_500,
            world_id,
            CanonicalHash::digest(b"shell"),
            CanonicalHash::digest(b"world"),
            CanonicalHash::digest(b"plan"),
            setting_revision(9),
        )
    }

    fn target_ack(intent: &LaunchIntentV1, process_epoch: ProcessEpoch) -> BootstrapAckV1 {
        BootstrapAckV1::for_ready_intent(
            intent,
            crate::model::test_app_created_proof(process_epoch),
        )
    }

    fn persist_recovery_claim(
        store: &mut MemoryStore,
        recovery_generation: LaunchGeneration,
    ) -> (RecoveryLaunchRequestV1, CanonicalHash) {
        let failure = LaunchFailureReceiptV1::new(
            None,
            None,
            None,
            LaunchPhaseV1::IntentRead,
            LaunchFailureCodeV1::IntentMissing,
        );
        let request = RecoveryLaunchRequestV1::seal(RecoveryLaunchDraftV1 {
            recovery_generation,
            source_generation: None,
            source_blob_hash: None,
            reason: RecoveryReasonV1::InvalidIntent,
            shell_lock_hash: CanonicalHash::digest(b"shell"),
            confirmed_setting_transaction_revision: setting_revision(0),
            supervisor_identity: supervisor_identity(),
            failure,
        })
        .unwrap_or_else(|error| panic!("recovery request did not seal: {error}"));
        let bytes = request
            .canonical_bytes()
            .unwrap_or_else(|error| panic!("recovery request did not encode: {error}"));
        let outcome = store
            .claim_recovery(None, &bytes)
            .unwrap_or_else(|error| panic!("recovery request did not persist: {error}"));
        (request, outcome.blob_hash())
    }
    #[derive(Debug)]
    struct MustNotRunBarrier;

    impl TransitionBarrier for MustNotRunBarrier {
        fn reach(
            &mut self,
            _current: ClientRuntimeStateV1,
            _target: LaunchTargetV1,
        ) -> Result<ShutdownBarrierReceiptV1, ShutdownBarrierFailureV1> {
            panic!("invalid launch drafts must be rejected before the shutdown barrier")
        }
    }

    fn shell_draft(
        generation: LaunchGeneration,
        revision: SettingTransactionRevision,
    ) -> LaunchIntentDraftV1 {
        LaunchIntentDraftV1 {
            generation,
            attempt: LaunchAttempt::FIRST,
            issued_at_ms: 1_000,
            expires_at_ms: 2_000,
            target: LaunchTargetV1::Shell,
            shell_lock_hash: CanonicalHash::digest(b"shell"),
            world_lock_hash: None,
            world_open_plan_hash: None,
            confirmed_setting_transaction_revision: revision,
        }
    }

    #[test]
    fn invalid_draft_and_inactive_reentry_never_run_the_barrier() {
        let mut store = MemoryStore::default();
        let mut machine = ClientTransitionMachine::from_active(initial_shell())
            .unwrap_or_else(|error| panic!("active state was rejected: {error}"));
        let mut draft = world_draft(WorldId::new_v4());
        draft.attempt = LaunchAttempt::new(2)
            .unwrap_or_else(|error| panic!("non-zero test attempt was rejected: {error}"));
        let result = machine.publish_handoff(
            draft,
            &mut store,
            &mut MustNotRunBarrier,
            supervisor_identity(),
        );
        assert!(matches!(
            result,
            Err(TransitionMachineError::Counter(
                LaunchModelError::InvalidAttempt { actual: 2 }
            ))
        ));
        assert_eq!(store.slot, IntentSlot::Empty);

        let world_id = WorldId::new_v4();
        let mut barrier = FixedBarrier(Ok(ShutdownBarrierReceiptV1::complete(
            setting_revision(9),
            None,
        )));
        let report = machine
            .publish_handoff(
                world_draft(world_id),
                &mut store,
                &mut barrier,
                supervisor_identity(),
            )
            .unwrap_or_else(|error| panic!("valid handoff was rejected: {error}"));
        assert!(matches!(
            report.outcome(),
            TransitionOutcomeV1::HandoffPublished { .. }
        ));
        assert!(matches!(
            machine.publish_handoff(
                shell_draft(generation(3), setting_revision(10)),
                &mut store,
                &mut MustNotRunBarrier,
                supervisor_identity(),
            ),
            Err(TransitionMachineError::InactiveState)
        ));
    }

    #[test]
    fn forged_active_provenance_is_rejected() {
        let forged_shell = ClientRuntimeStateV1::Shell {
            generation: generation(2),
            process_epoch: epoch(1),
            launch_blob_hash: None,
        };
        assert!(matches!(
            ClientTransitionMachine::from_active(forged_shell),
            Err(TransitionMachineError::InconsistentBootProvenance)
        ));
        let forged_world = ClientRuntimeStateV1::World {
            generation: generation(1),
            process_epoch: epoch(1),
            world_id: WorldId::new_v4(),
            launch_blob_hash: CanonicalHash::digest(b"forged"),
        };
        assert!(matches!(
            ClientTransitionMachine::from_active(forged_world),
            Err(TransitionMachineError::InconsistentBootProvenance)
        ));
    }

    #[test]
    fn barrier_receipt_is_bound_to_settings_and_exact_world_writer() {
        let world_id = WorldId::new_v4();
        let mut draft = world_draft(world_id);
        draft.generation = generation(3);
        let state = ClientRuntimeStateV1::World {
            generation: generation(2),
            process_epoch: epoch(1),
            world_id,
            launch_blob_hash: CanonicalHash::digest(b"boot-slot"),
        };
        for receipt in [
            ShutdownBarrierReceiptV1::complete(setting_revision(9), None),
            ShutdownBarrierReceiptV1::complete(
                setting_revision(9),
                Some(DurableWorldRevisionV1::new(
                    WorldId::new_v4(),
                    world_revision(4),
                )),
            ),
            ShutdownBarrierReceiptV1::complete(
                setting_revision(8),
                Some(DurableWorldRevisionV1::new(world_id, world_revision(4))),
            ),
        ] {
            let mut store = MemoryStore::default();
            let mut machine = ClientTransitionMachine::from_active(state)
                .unwrap_or_else(|error| panic!("world state was rejected: {error}"));
            let mut barrier = FixedBarrier(Ok(receipt));
            let report = machine
                .publish_handoff(draft, &mut store, &mut barrier, supervisor_identity())
                .unwrap_or_else(|error| panic!("barrier report failed: {error}"));
            assert_eq!(report.outcome(), TransitionOutcomeV1::Halted);
            assert!(report.barrier_failure().is_some());
            assert_eq!(store.slot, IntentSlot::Empty);
        }

        let mut store = MemoryStore::default();
        let mut shell = ClientTransitionMachine::from_active(initial_shell())
            .unwrap_or_else(|error| panic!("initial shell was rejected: {error}"));
        let mut spurious_world = FixedBarrier(Ok(ShutdownBarrierReceiptV1::complete(
            setting_revision(9),
            Some(DurableWorldRevisionV1::new(world_id, world_revision(1))),
        )));
        let report = shell
            .publish_handoff(
                world_draft(world_id),
                &mut store,
                &mut spurious_world,
                supervisor_identity(),
            )
            .unwrap_or_else(|error| panic!("shell barrier report failed: {error}"));
        assert!(matches!(
            report.outcome(),
            TransitionOutcomeV1::RecoveryHandoffPublished { .. }
        ));
        assert!(report.recovery_request().is_some());
        assert_eq!(
            store.slot.disposition(),
            Some(SlotDisposition::RecoveryClaimed)
        );
    }

    #[test]
    fn barrier_failure_never_publishes() {
        let mut store = MemoryStore::default();
        let mut machine = ClientTransitionMachine::from_active(initial_shell())
            .unwrap_or_else(|error| panic!("active state was rejected: {error}"));
        let mut barrier = FixedBarrier(Err(ShutdownBarrierFailureV1::new(
            crate::ShutdownBarrierStageV1::FlushSettings,
            FailureDispositionV1::CurrentProcessUsable,
        )));
        let report = machine
            .publish_handoff(
                world_draft(WorldId::new_v4()),
                &mut store,
                &mut barrier,
                supervisor_identity(),
            )
            .unwrap_or_else(|error| panic!("barrier report failed: {error}"));
        assert_eq!(
            report.outcome(),
            TransitionOutcomeV1::CurrentProcessRetained
        );
        assert_eq!(store.slot, IntentSlot::Empty);
    }

    #[test]
    fn initial_atomic_publish_faults_are_typed_and_post_commit_is_replayable() {
        for operation in [
            StoreOperation::WriteTemporary,
            StoreOperation::SyncTemporary,
            StoreOperation::Replace,
            StoreOperation::SyncDirectory,
        ] {
            let mut store = MemoryStore::default();
            store.fail_next(operation);
            let mut machine = ClientTransitionMachine::from_active(initial_shell())
                .unwrap_or_else(|error| panic!("active state was rejected: {error}"));
            let mut barrier = FixedBarrier(Ok(ShutdownBarrierReceiptV1::complete(
                setting_revision(9),
                None,
            )));
            let report = machine
                .publish_handoff(
                    world_draft(WorldId::new_v4()),
                    &mut store,
                    &mut barrier,
                    supervisor_identity(),
                )
                .unwrap_or_else(|error| panic!("fault report failed: {error}"));
            if operation == StoreOperation::SyncDirectory {
                assert!(matches!(
                    report.outcome(),
                    TransitionOutcomeV1::HandoffPublished { .. }
                ));
                assert_eq!(store.slot.disposition(), Some(SlotDisposition::Pending));
            } else {
                assert_eq!(report.outcome(), TransitionOutcomeV1::Halted);
                assert_eq!(store.slot, IntentSlot::Empty);
            }
        }
    }

    #[test]
    fn successful_ack_is_checksum_bound_consumed_and_machine_is_one_shot() {
        let world_id = WorldId::new_v4();
        let mut store = MemoryStore::default();
        let intent = publish_world(&mut store, world_id);
        let ack = target_ack(&intent, epoch(2));
        let mut process = FakeProcess {
            observations: VecDeque::from([BootObservationV1::Acknowledged(ack)]),
            ..FakeProcess::default()
        };
        let mut machine = BootstrapMachine::new();
        let report = machine.activate(&mut store, &world_policy(world_id), &mut process);
        assert!(matches!(
            report.outcome(),
            TransitionOutcomeV1::Activated {
                target: LaunchTargetV1::World { .. },
                ..
            }
        ));
        assert_eq!(store.slot.disposition(), Some(SlotDisposition::Consumed));
        assert_eq!(process.spawn_count, 1);
        let second = machine.activate(&mut store, &world_policy(world_id), &mut process);
        assert_eq!(second.outcome(), TransitionOutcomeV1::Halted);
        assert_eq!(process.spawn_count, 1);
    }

    #[test]
    fn consume_failure_terminates_quarantines_claims_recovery_and_recovers() {
        let world_id = WorldId::new_v4();
        let mut store = MemoryStore::default();
        let intent = publish_world(&mut store, world_id);
        store.fail_replace_after(2);
        let mut process = FakeProcess {
            observations: VecDeque::from([BootObservationV1::Acknowledged(target_ack(
                &intent,
                epoch(2),
            ))]),
            acknowledge_recovery: true,
            ..FakeProcess::default()
        };
        let mut machine = BootstrapMachine::new();
        let report = machine.activate(&mut store, &world_policy(world_id), &mut process);
        assert!(matches!(
            report.outcome(),
            TransitionOutcomeV1::RecoveryShell { .. }
        ));
        assert_eq!(
            report.failures()[0].code(),
            LaunchFailureCodeV1::ConsumeFailed
        );
        assert_eq!(process.terminated, 1);
        assert_eq!(
            store.slot.disposition(),
            Some(SlotDisposition::RecoveryClaimed)
        );
    }

    #[test]
    fn corrupt_intent_uses_initial_watermark_and_resumes_claimed_recovery() {
        let mut store = MemoryStore::default();
        assert!(store.publish(b"not-json").is_ok());
        let policy = TransitionValidationPolicy::for_shell(
            1_500,
            CanonicalHash::digest(b"shell"),
            setting_revision(0),
        );
        let mut process = FakeProcess {
            acknowledge_recovery: true,
            ..FakeProcess::default()
        };
        let mut machine = BootstrapMachine::new();
        let report = machine.activate(&mut store, &policy, &mut process);
        assert_eq!(
            report.failures()[0].code(),
            LaunchFailureCodeV1::IntentCorrupt
        );
        assert!(matches!(
            machine.state(),
            Some(ClientRuntimeStateV1::Recovery { generation: value, .. })
                if value == generation(2)
        ));
        assert_eq!(
            store.slot.disposition(),
            Some(SlotDisposition::RecoveryClaimed)
        );

        let recovery = SpawnedProcess::new(1)
            .unwrap_or_else(|error| panic!("test recovery handle was rejected: {error}"));
        let mut fresh_process = FakeProcess {
            recovery_reconciliations: VecDeque::from([RecoveryChildStatusV1::Acquired(recovery)]),
            acknowledge_recovery: true,
            ..FakeProcess::default()
        };
        let mut fresh = BootstrapMachine::new();
        let replay = fresh.activate(&mut store, &policy, &mut fresh_process);
        assert!(matches!(
            replay.outcome(),
            TransitionOutcomeV1::RecoveryShell { .. }
        ));
        assert_eq!(fresh_process.spawn_count, 0);
    }

    #[test]
    fn expired_generation_jump_cannot_poison_the_durable_watermark() {
        let intent = LaunchIntentV1::seal(LaunchIntentDraftV1 {
            generation: generation(8),
            attempt: LaunchAttempt::FIRST,
            issued_at_ms: 1_000,
            expires_at_ms: 1_200,
            target: LaunchTargetV1::Shell,
            shell_lock_hash: CanonicalHash::digest(b"shell"),
            world_lock_hash: None,
            world_open_plan_hash: None,
            confirmed_setting_transaction_revision: setting_revision(3),
        })
        .unwrap_or_else(|error| panic!("expired fixture did not seal: {error}"));
        let bytes = intent
            .canonical_bytes()
            .unwrap_or_else(|error| panic!("expired fixture did not encode: {error}"));
        let mut store = MemoryStore::default();
        assert!(store.publish(&bytes).is_ok());
        let policy = TransitionValidationPolicy::for_shell(
            1_500,
            CanonicalHash::digest(b"shell"),
            setting_revision(3),
        );
        let mut process = FakeProcess {
            acknowledge_recovery: true,
            ..FakeProcess::default()
        };
        let mut machine = BootstrapMachine::new();
        let report = machine.activate(&mut store, &policy, &mut process);
        assert_eq!(
            report.failures()[0].code(),
            LaunchFailureCodeV1::IntentExpired
        );
        assert!(matches!(
            machine.state(),
            Some(ClientRuntimeStateV1::Recovery { generation: value, .. })
                if value == generation(2)
        ));
    }

    #[test]
    fn supervisor_proof_controls_claimed_launcher_crash_recovery() {
        for status in [PriorChildStatusV1::Running, PriorChildStatusV1::Unknown] {
            let world_id = WorldId::new_v4();
            let mut store = MemoryStore::default();
            let _intent = publish_world(&mut store, world_id);
            let blob_hash = store
                .slot
                .blob_hash()
                .unwrap_or_else(|| panic!("published slot has no hash"));
            assert!(store.claim(blob_hash).is_ok());
            let mut process = FakeProcess {
                reconciliations: VecDeque::from([status]),
                acknowledge_recovery: true,
                ..FakeProcess::default()
            };
            let report =
                BootstrapMachine::new().activate(&mut store, &world_policy(world_id), &mut process);
            assert_eq!(report.outcome(), TransitionOutcomeV1::Halted);
            assert_eq!(process.spawn_count, 0);
            assert_eq!(store.slot.disposition(), Some(SlotDisposition::Claimed));
        }

        let world_id = WorldId::new_v4();
        let mut store = MemoryStore::default();
        let _intent = publish_world(&mut store, world_id);
        let blob_hash = store
            .slot
            .blob_hash()
            .unwrap_or_else(|| panic!("published slot has no hash"));
        assert!(store.claim(blob_hash).is_ok());
        let mut process = FakeProcess {
            reconciliations: VecDeque::from([PriorChildStatusV1::Exited { exit_code: Some(9) }]),
            acknowledge_recovery: true,
            ..FakeProcess::default()
        };
        let report =
            BootstrapMachine::new().activate(&mut store, &world_policy(world_id), &mut process);
        assert!(matches!(
            report.outcome(),
            TransitionOutcomeV1::RecoveryShell { .. }
        ));
        assert_eq!(process.spawn_count, 1, "only recovery may be spawned");
        assert_eq!(
            store.slot.disposition(),
            Some(SlotDisposition::RecoveryClaimed)
        );
    }

    #[test]
    fn supervisor_proven_client_crash_after_ack_recovers_once() {
        let world_id = WorldId::new_v4();
        let mut store = MemoryStore::default();
        let intent = publish_world(&mut store, world_id);
        let mut first_process = FakeProcess {
            observations: VecDeque::from([BootObservationV1::Acknowledged(target_ack(
                &intent,
                epoch(2),
            ))]),
            ..FakeProcess::default()
        };
        let _activated = BootstrapMachine::new().activate(
            &mut store,
            &world_policy(world_id),
            &mut first_process,
        );
        assert_eq!(store.slot.disposition(), Some(SlotDisposition::Consumed));

        let mut recovery_process = FakeProcess {
            reconciliations: VecDeque::from([PriorChildStatusV1::Exited { exit_code: Some(3) }]),
            acknowledge_recovery: true,
            ..FakeProcess::default()
        };
        let report = BootstrapMachine::new().activate(
            &mut store,
            &world_policy(world_id),
            &mut recovery_process,
        );
        assert!(matches!(
            report.outcome(),
            TransitionOutcomeV1::RecoveryShell { .. }
        ));
        assert_eq!(recovery_process.spawn_count, 1);
        assert_eq!(
            store.slot.disposition(),
            Some(SlotDisposition::RecoveryClaimed)
        );
    }

    #[test]
    fn spawn_boot_ack_timeout_and_crash_each_recover_once() {
        let cases = [
            (
                Some(SpawnFailureV1::ResourceUnavailable),
                None,
                LaunchFailureCodeV1::SpawnFailed,
            ),
            (
                None,
                Some(BootObservationV1::BootFailed),
                LaunchFailureCodeV1::BootFailed,
            ),
            (
                None,
                Some(BootObservationV1::TimedOut),
                LaunchFailureCodeV1::AckTimedOut,
            ),
            (
                None,
                Some(BootObservationV1::Crashed { exit_code: Some(9) }),
                LaunchFailureCodeV1::ProcessCrashed,
            ),
        ];
        for (spawn_failure, observation, expected_code) in cases {
            let world_id = WorldId::new_v4();
            let mut store = MemoryStore::default();
            let _intent = publish_world(&mut store, world_id);
            let mut process = FakeProcess {
                spawn_failures: VecDeque::from([spawn_failure]),
                acknowledge_recovery: true,
                ..FakeProcess::default()
            };
            if let Some(observation) = observation {
                process.observations.push_back(observation);
            }
            let report =
                BootstrapMachine::new().activate(&mut store, &world_policy(world_id), &mut process);
            assert!(matches!(
                report.outcome(),
                TransitionOutcomeV1::RecoveryShell { .. }
            ));
            assert_eq!(report.failures()[0].code(), expected_code);
            assert_eq!(
                store.slot.disposition(),
                Some(SlotDisposition::RecoveryClaimed)
            );
        }
    }

    #[test]
    fn ack_for_same_generation_and_target_but_different_checksum_is_rejected() {
        let world_id = WorldId::new_v4();
        let mut store = MemoryStore::default();
        let intent = publish_world(&mut store, world_id);
        let mut other_draft = world_draft(world_id);
        other_draft.world_open_plan_hash = Some(CanonicalHash::digest(b"other-plan"));
        let other = LaunchIntentV1::seal(other_draft)
            .unwrap_or_else(|error| panic!("other intent was rejected: {error}"));
        let invalid_ack = target_ack(&other, epoch(2));
        assert_ne!(invalid_ack.intent_checksum(), intent.checksum());
        let mut process = FakeProcess {
            observations: VecDeque::from([BootObservationV1::Acknowledged(invalid_ack)]),
            acknowledge_recovery: true,
            ..FakeProcess::default()
        };
        let report =
            BootstrapMachine::new().activate(&mut store, &world_policy(world_id), &mut process);
        assert_eq!(report.failures()[0].code(), LaunchFailureCodeV1::AckInvalid);
        assert_eq!(process.terminated, 1);
        assert!(matches!(
            report.outcome(),
            TransitionOutcomeV1::RecoveryShell { .. }
        ));
    }

    #[test]
    fn unproven_termination_fails_closed_without_recovery() {
        let world_id = WorldId::new_v4();
        let mut store = MemoryStore::default();
        let _intent = publish_world(&mut store, world_id);
        let mut process = FakeProcess {
            observations: VecDeque::from([BootObservationV1::BootFailed]),
            termination_failures: VecDeque::from([Some(TerminationFailureV1::PlatformRejected)]),
            acknowledge_recovery: true,
            ..FakeProcess::default()
        };
        let report =
            BootstrapMachine::new().activate(&mut store, &world_policy(world_id), &mut process);
        assert_eq!(report.outcome(), TransitionOutcomeV1::Halted);
        assert_eq!(process.spawn_count, 1);
        assert_eq!(store.slot.disposition(), Some(SlotDisposition::Claimed));
        assert!(report.failures().iter().any(|failure| {
            failure.code() == LaunchFailureCodeV1::TerminationFailed
                && matches!(
                    failure.detail(),
                    LaunchFailureDetailV1::Termination {
                        failure: TerminationFailureV1::PlatformRejected
                    }
                )
        }));
    }

    #[test]
    fn quarantine_and_recovery_claim_failures_never_spawn_recovery_early() {
        let world_id = WorldId::new_v4();
        let mut store = MemoryStore::default();
        let _intent = publish_world(&mut store, world_id);
        store.fail_replace_after(2);
        let mut process = FakeProcess {
            spawn_failures: VecDeque::from([Some(SpawnFailureV1::ResourceUnavailable)]),
            acknowledge_recovery: true,
            ..FakeProcess::default()
        };
        let report =
            BootstrapMachine::new().activate(&mut store, &world_policy(world_id), &mut process);
        assert_eq!(report.outcome(), TransitionOutcomeV1::Halted);
        assert_eq!(process.spawn_count, 1, "only the failed target spawn ran");
        assert_eq!(store.slot.disposition(), Some(SlotDisposition::Claimed));
        assert!(
            report
                .failures()
                .iter()
                .any(|failure| failure.code() == LaunchFailureCodeV1::QuarantineFailed)
        );

        let mut corrupt_store = MemoryStore::default();
        assert!(corrupt_store.publish(b"not-json").is_ok());
        corrupt_store.fail_replace_after(2);
        let policy = TransitionValidationPolicy::for_shell(
            1_500,
            CanonicalHash::digest(b"shell"),
            setting_revision(0),
        );
        let mut blocked_process = FakeProcess {
            acknowledge_recovery: true,
            ..FakeProcess::default()
        };
        let blocked =
            BootstrapMachine::new().activate(&mut corrupt_store, &policy, &mut blocked_process);
        assert_eq!(blocked.outcome(), TransitionOutcomeV1::Halted);
        assert_eq!(blocked_process.spawn_count, 0);
        assert_eq!(
            corrupt_store.slot.disposition(),
            Some(SlotDisposition::Quarantined)
        );
        assert!(
            blocked
                .failures()
                .iter()
                .any(|failure| failure.code() == LaunchFailureCodeV1::RecoveryClaimFailed)
        );

        let mut resumed_process = FakeProcess {
            acknowledge_recovery: true,
            ..FakeProcess::default()
        };
        let resumed =
            BootstrapMachine::new().activate(&mut corrupt_store, &policy, &mut resumed_process);
        assert!(matches!(
            resumed.outcome(),
            TransitionOutcomeV1::RecoveryShell { .. }
        ));
        assert_eq!(resumed_process.spawn_count, 1);
    }

    #[test]
    fn recovery_failure_is_durably_terminal_for_a_fresh_machine() {
        let mut store = MemoryStore::default();
        let policy = TransitionValidationPolicy::for_shell(
            1,
            CanonicalHash::digest(b"shell"),
            setting_revision(0),
        );
        let mut process = FakeProcess {
            observations: VecDeque::from([BootObservationV1::BootFailed]),
            ..FakeProcess::default()
        };
        let first = BootstrapMachine::new().activate(&mut store, &policy, &mut process);
        assert_eq!(first.outcome(), TransitionOutcomeV1::Halted);
        assert_eq!(process.spawn_count, 1);
        assert_eq!(
            store.slot.disposition(),
            Some(SlotDisposition::RecoveryClaimed)
        );

        let mut fresh_process = FakeProcess {
            recovery_reconciliations: VecDeque::from([RecoveryChildStatusV1::Exited {
                exit_code: None,
            }]),
            acknowledge_recovery: true,
            ..FakeProcess::default()
        };
        let second = BootstrapMachine::new().activate(&mut store, &policy, &mut fresh_process);
        assert_eq!(second.outcome(), TransitionOutcomeV1::Halted);
        assert_eq!(fresh_process.spawn_count, 0);
        assert!(
            second
                .failures()
                .iter()
                .any(|failure| failure.code() == LaunchFailureCodeV1::RecoveryLoopSuppressed)
        );
    }

    #[test]
    fn terminal_to_next_publish_has_no_empty_crash_window_and_reconciles_residue() {
        let world_id = WorldId::new_v4();
        let mut store = MemoryStore::default();
        let intent = publish_world(&mut store, world_id);
        let mut process = FakeProcess {
            observations: VecDeque::from([BootObservationV1::Acknowledged(target_ack(
                &intent,
                epoch(2),
            ))]),
            ..FakeProcess::default()
        };
        let mut bootstrap = BootstrapMachine::new();
        let _report = bootstrap.activate(&mut store, &world_policy(world_id), &mut process);
        let active_world = bootstrap
            .state()
            .unwrap_or_else(|| panic!("world bootstrap produced no active state"));
        assert_eq!(store.slot.disposition(), Some(SlotDisposition::Consumed));

        store.fail_next(StoreOperation::SyncDirectory);
        let mut publisher = ClientTransitionMachine::from_active(active_world)
            .unwrap_or_else(|error| panic!("bootstrapped world was rejected: {error}"));
        let mut barrier = FixedBarrier(Ok(ShutdownBarrierReceiptV1::complete(
            setting_revision(10),
            Some(DurableWorldRevisionV1::new(world_id, world_revision(5))),
        )));
        let publish = publisher
            .publish_handoff(
                shell_draft(generation(3), setting_revision(10)),
                &mut store,
                &mut barrier,
                supervisor_identity(),
            )
            .unwrap_or_else(|error| panic!("next handoff validation failed: {error}"));
        assert!(matches!(
            publish.outcome(),
            TransitionOutcomeV1::HandoffPublished { .. }
        ));
        assert_eq!(store.slot.disposition(), Some(SlotDisposition::Pending));
        assert!(store.slot.predecessor().is_some());

        let next_intent = LaunchIntentV1::authenticate_at_rest(
            store
                .slot
                .bytes()
                .unwrap_or_else(|| panic!("pending replacement has no bytes")),
        )
        .unwrap_or_else(|error| panic!("pending replacement did not authenticate: {error}"));
        let policy = TransitionValidationPolicy::for_shell(
            1_500,
            CanonicalHash::digest(b"shell"),
            setting_revision(10),
        );
        let mut next_process = FakeProcess {
            observations: VecDeque::from([BootObservationV1::Acknowledged(target_ack(
                &next_intent,
                epoch(3),
            ))]),
            ..FakeProcess::default()
        };
        let activated = BootstrapMachine::new().activate(&mut store, &policy, &mut next_process);
        assert!(matches!(
            activated.outcome(),
            TransitionOutcomeV1::Activated {
                target: LaunchTargetV1::Shell,
                ..
            }
        ));
        assert!(store.slot.predecessor().is_none());
        assert_eq!(store.slot.disposition(), Some(SlotDisposition::Consumed));
    }

    #[test]
    fn recovery_claim_crash_cuts_reconcile_without_a_second_physical_spawn() {
        let policy = TransitionValidationPolicy::for_shell(
            1,
            CanonicalHash::digest(b"shell"),
            setting_revision(0),
        );

        let mut before_spawn_store = MemoryStore::default();
        let (_request, _hash) =
            persist_recovery_claim(&mut before_spawn_store, LaunchGeneration::FIRST);
        let mut first_supervisor = FakeProcess {
            acknowledge_recovery: true,
            ..FakeProcess::default()
        };
        let first = BootstrapMachine::new().activate(
            &mut before_spawn_store,
            &policy,
            &mut first_supervisor,
        );
        assert!(matches!(
            first.outcome(),
            TransitionOutcomeV1::RecoveryShell { .. }
        ));
        assert_eq!(first_supervisor.spawn_count, 1);

        let second = BootstrapMachine::new().activate(
            &mut before_spawn_store,
            &policy,
            &mut first_supervisor,
        );
        assert!(matches!(
            second.outcome(),
            TransitionOutcomeV1::RecoveryShell { .. }
        ));
        assert_eq!(first_supervisor.spawn_count, 1);

        let mut after_spawn_store = MemoryStore::default();
        let (_request, _hash) =
            persist_recovery_claim(&mut after_spawn_store, LaunchGeneration::FIRST);
        let prior_child = SpawnedProcess::new(77)
            .unwrap_or_else(|error| panic!("prior recovery handle was rejected: {error}"));
        let mut restarted_supervisor = FakeProcess {
            recovery_reconciliations: VecDeque::from([RecoveryChildStatusV1::Acquired(
                prior_child,
            )]),
            acknowledge_recovery: true,
            ..FakeProcess::default()
        };
        let resumed = BootstrapMachine::new().activate(
            &mut after_spawn_store,
            &policy,
            &mut restarted_supervisor,
        );
        assert!(matches!(
            resumed.outcome(),
            TransitionOutcomeV1::RecoveryShell { .. }
        ));
        assert_eq!(restarted_supervisor.spawn_count, 0);
    }

    #[test]
    fn recovery_reconciliation_exited_and_unknown_halt_without_spawn() {
        let policy = TransitionValidationPolicy::for_shell(
            1,
            CanonicalHash::digest(b"shell"),
            setting_revision(0),
        );
        for status in [
            RecoveryChildStatusV1::Exited { exit_code: Some(4) },
            RecoveryChildStatusV1::Unknown,
        ] {
            let mut store = MemoryStore::default();
            let (_request, _hash) = persist_recovery_claim(&mut store, LaunchGeneration::FIRST);
            let mut process = FakeProcess {
                recovery_reconciliations: VecDeque::from([status]),
                acknowledge_recovery: true,
                ..FakeProcess::default()
            };
            let report = BootstrapMachine::new().activate(&mut store, &policy, &mut process);
            assert_eq!(report.outcome(), TransitionOutcomeV1::Halted);
            assert_eq!(process.spawn_count, 0);
            assert!(
                report.failures().iter().any(|failure| {
                    failure.code() == LaunchFailureCodeV1::RecoveryLoopSuppressed
                })
            );
        }
    }

    #[test]
    fn destructive_barrier_stages_publish_one_replayable_recovery_request() {
        for stage in [
            crate::ShutdownBarrierStageV1::FlushSettings,
            crate::ShutdownBarrierStageV1::ConfirmDurability,
            crate::ShutdownBarrierStageV1::StopPackages,
            crate::ShutdownBarrierStageV1::JoinTasks,
            crate::ShutdownBarrierStageV1::CloseWriter,
        ] {
            let mut store = MemoryStore::default();
            let mut publisher = ClientTransitionMachine::from_active(initial_shell())
                .unwrap_or_else(|error| panic!("initial shell was rejected: {error}"));
            let mut barrier = FixedBarrier(Err(ShutdownBarrierFailureV1::new(
                stage,
                FailureDispositionV1::RecoveryRequired,
            )));
            let report = publisher
                .publish_handoff(
                    world_draft(WorldId::new_v4()),
                    &mut store,
                    &mut barrier,
                    supervisor_identity(),
                )
                .unwrap_or_else(|error| panic!("barrier recovery failed: {error}"));
            assert!(matches!(
                report.outcome(),
                TransitionOutcomeV1::RecoveryHandoffPublished { .. }
            ));
            assert!(report.intent().is_none());
            assert!(report.recovery_request().is_some());
            assert_eq!(
                store.slot.disposition(),
                Some(SlotDisposition::RecoveryClaimed)
            );

            let policy = TransitionValidationPolicy::for_shell(
                1_500,
                CanonicalHash::digest(b"shell"),
                setting_revision(9),
            );
            let mut process = FakeProcess {
                acknowledge_recovery: true,
                ..FakeProcess::default()
            };
            let recovered = BootstrapMachine::new().activate(&mut store, &policy, &mut process);
            assert!(matches!(
                recovered.outcome(),
                TransitionOutcomeV1::RecoveryShell {
                    reason: RecoveryReasonV1::BarrierFailure,
                    ..
                }
            ));
            let replay = BootstrapMachine::new().activate(&mut store, &policy, &mut process);
            assert!(matches!(
                replay.outcome(),
                TransitionOutcomeV1::RecoveryShell { .. }
            ));
            assert_eq!(process.spawn_count, 1);
        }
    }

    #[test]
    fn durable_predecessor_accepts_only_exact_next_generation() {
        for (candidate_generation, expected_code) in [
            (generation(2), LaunchFailureCodeV1::IntentStale),
            (generation(4), LaunchFailureCodeV1::IntentStale),
        ] {
            let world_id = WorldId::new_v4();
            let mut store = MemoryStore::default();
            let _intent = publish_world(&mut store, world_id);
            let blob_hash = store
                .slot
                .blob_hash()
                .unwrap_or_else(|| panic!("published world had no hash"));
            assert!(store.claim(blob_hash).is_ok());
            assert!(store.consume(blob_hash).is_ok());

            let candidate =
                LaunchIntentV1::seal(shell_draft(candidate_generation, setting_revision(10)))
                    .unwrap_or_else(|error| panic!("candidate did not seal: {error}"));
            let bytes = candidate
                .canonical_bytes()
                .unwrap_or_else(|error| panic!("candidate did not encode: {error}"));
            assert!(store.publish_replacing_terminal(blob_hash, &bytes).is_ok());
            let policy = TransitionValidationPolicy::for_shell(
                1_500,
                CanonicalHash::digest(b"shell"),
                setting_revision(10),
            );
            let mut process = FakeProcess {
                acknowledge_recovery: true,
                ..FakeProcess::default()
            };
            let report = BootstrapMachine::new().activate(&mut store, &policy, &mut process);
            assert_eq!(report.failures()[0].code(), expected_code);
            assert!(matches!(
                report.outcome(),
                TransitionOutcomeV1::RecoveryShell { .. }
            ));
            assert!(matches!(
                process.recovery_status,
                Some(RecoveryChildStatusV1::Acquired(_))
            ));
        }
    }

    #[test]
    fn forged_recovery_generation_cannot_retire_the_real_claim() {
        let mut store = MemoryStore::default();
        let (request, blob_hash) = persist_recovery_claim(&mut store, LaunchGeneration::FIRST);
        let forged = ClientRuntimeStateV1::Recovery {
            generation: generation(2),
            process_epoch: epoch(4),
            reason: request.reason(),
            recovery_claim_hash: blob_hash,
        };
        let mut publisher = ClientTransitionMachine::from_active(forged)
            .unwrap_or_else(|error| panic!("well-shaped recovery was rejected: {error}"));
        let mut barrier = FixedBarrier(Ok(ShutdownBarrierReceiptV1::complete(
            setting_revision(0),
            None,
        )));
        let report = publisher
            .publish_handoff(
                shell_draft(generation(3), setting_revision(0)),
                &mut store,
                &mut barrier,
                supervisor_identity(),
            )
            .unwrap_or_else(|error| panic!("forged recovery report failed: {error}"));
        assert_eq!(report.outcome(), TransitionOutcomeV1::Halted);
        assert_eq!(store.slot.blob_hash(), Some(blob_hash));
        assert_eq!(
            store.slot.disposition(),
            Some(SlotDisposition::RecoveryClaimed)
        );
    }
    #[test]
    fn real_file_store_durably_boots_target_and_recovery() {
        let target_directory = MachineTestDirectory::create();
        let mut target_store = FileLaunchIntentStore::open(&target_directory.0)
            .unwrap_or_else(|error| panic!("target file store did not open: {error}"));
        let world_id = WorldId::new_v4();
        let mut publisher = ClientTransitionMachine::from_active(initial_shell())
            .unwrap_or_else(|error| panic!("active state was rejected: {error}"));
        let mut barrier = FixedBarrier(Ok(ShutdownBarrierReceiptV1::complete(
            setting_revision(9),
            None,
        )));
        let publication = publisher
            .publish_handoff(
                world_draft(world_id),
                &mut target_store,
                &mut barrier,
                supervisor_identity(),
            )
            .unwrap_or_else(|error| panic!("file handoff was rejected: {error}"));
        let intent = publication
            .intent()
            .unwrap_or_else(|| panic!("file handoff did not publish an intent"));
        let mut target_process = FakeProcess {
            observations: VecDeque::from([BootObservationV1::Acknowledged(target_ack(
                intent,
                epoch(2),
            ))]),
            ..FakeProcess::default()
        };
        let activated = BootstrapMachine::new().activate(
            &mut target_store,
            &world_policy(world_id),
            &mut target_process,
        );
        assert!(matches!(
            activated.outcome(),
            TransitionOutcomeV1::Activated {
                target: LaunchTargetV1::World { .. },
                ..
            }
        ));
        assert_eq!(target_process.spawn_count, 1);
        let target_slot = target_store
            .read()
            .unwrap_or_else(|error| panic!("target file store did not re-read: {error}"));
        assert_eq!(target_slot.disposition(), Some(SlotDisposition::Consumed));

        let recovery_directory = MachineTestDirectory::create();
        let mut recovery_store = FileLaunchIntentStore::open(&recovery_directory.0)
            .unwrap_or_else(|error| panic!("recovery file store did not open: {error}"));
        assert_eq!(
            recovery_store.publish(b"not-json").ok(),
            Some(PublishDisposition::Published)
        );
        let recovery_policy = TransitionValidationPolicy::for_shell(
            1_500,
            CanonicalHash::digest(b"shell"),
            setting_revision(0),
        );
        let mut recovery_process = FakeProcess {
            acknowledge_recovery: true,
            ..FakeProcess::default()
        };
        let recovered = BootstrapMachine::new().activate(
            &mut recovery_store,
            &recovery_policy,
            &mut recovery_process,
        );
        assert!(matches!(
            recovered.outcome(),
            TransitionOutcomeV1::RecoveryShell { .. }
        ));
        assert_eq!(recovery_process.spawn_count, 1);
        let recovery_slot = recovery_store
            .read()
            .unwrap_or_else(|error| panic!("recovery file store did not re-read: {error}"));
        assert_eq!(
            recovery_slot.disposition(),
            Some(SlotDisposition::RecoveryClaimed)
        );
    }

    #[test]
    fn exact_terminal_blob_is_required_before_next_publish() {
        let world_id = WorldId::new_v4();
        let mut store = MemoryStore::default();
        let intent = publish_world(&mut store, world_id);
        let blob_hash = store
            .slot
            .blob_hash()
            .unwrap_or_else(|| panic!("pending world has no blob hash"));
        assert!(store.claim(blob_hash).is_ok());
        assert!(store.consume(blob_hash).is_ok());
        let forged_state = ClientRuntimeStateV1::World {
            generation: intent.generation(),
            process_epoch: epoch(2),
            world_id,
            launch_blob_hash: CanonicalHash::digest(b"different-slot"),
        };
        let mut publisher = ClientTransitionMachine::from_active(forged_state)
            .unwrap_or_else(|error| panic!("well-shaped provenance was rejected: {error}"));
        let mut barrier = FixedBarrier(Ok(ShutdownBarrierReceiptV1::complete(
            setting_revision(10),
            Some(DurableWorldRevisionV1::new(world_id, world_revision(5))),
        )));
        let report = publisher
            .publish_handoff(
                shell_draft(generation(3), setting_revision(10)),
                &mut store,
                &mut barrier,
                supervisor_identity(),
            )
            .unwrap_or_else(|error| panic!("next handoff validation failed: {error}"));
        assert_eq!(report.outcome(), TransitionOutcomeV1::Halted);
        assert_eq!(store.slot.disposition(), Some(SlotDisposition::Consumed));
    }
}
