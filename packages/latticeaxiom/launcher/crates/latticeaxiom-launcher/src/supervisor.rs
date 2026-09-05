//! Non-Bevy product supervisor for the shell→world→shell replacement loop.

use latticeaxiom_core::CanonicalHash;

use crate::child_exit::{ChildExitKindV1, ChildExitReportV1, ChildRoleV1};
use crate::machine::BootstrapMachine;
use crate::store::ChildExitSlot;
use crate::{
    AtomicChildExitStore, AtomicLaunchIntentStore, BootObservationV1, ChildObservationV1,
    DurableWorldRevisionV1, IntentSlot, IntentStoreError, LaunchFailureCodeV1,
    LaunchFailureDetailV1, LaunchFailureReceiptV1, LaunchGeneration, LaunchIntentV1, LaunchPhaseV1,
    LaunchTargetV1, MAX_BOOTSTRAP_ACK_WAIT_MS, MAX_CHILD_SHUTDOWN_WAIT_MS,
    MAX_CONSECUTIVE_CHILD_FAILURES, MAX_SUPERVISOR_HOPS, PriorChildStatusV1, ProcessControl,
    ProcessEpoch, ProcessLaunchRequestV1, RecoveryReasonV1, SettingTransactionRevision,
    SlotDisposition, SpawnedProcess, TransitionOutcomeV1, TransitionValidationPolicy,
};

const MAX_REPORT_FAILURES: usize = 4;

/// Stable schema version for [`SupervisorReportV1`].
pub const SUPERVISOR_REPORT_SCHEMA_VERSION: u32 = 1;

/// Trusted product-loop inputs that do not come from child-published bytes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SupervisorConfigV1 {
    shell_lock_hash: CanonicalHash,
    now_ms: u64,
    confirmed_setting_transaction_revision: SettingTransactionRevision,
}

impl SupervisorConfigV1 {
    /// Creates a supervisor configuration from the reopened shell lock.
    #[must_use]
    pub const fn new(
        shell_lock_hash: CanonicalHash,
        now_ms: u64,
        confirmed_setting_transaction_revision: SettingTransactionRevision,
    ) -> Self {
        Self {
            shell_lock_hash,
            now_ms,
            confirmed_setting_transaction_revision,
        }
    }

    /// Returns the exact reopened shell lock.
    #[must_use]
    pub const fn shell_lock_hash(self) -> CanonicalHash {
        self.shell_lock_hash
    }
}

/// Durable supervisor position in the product loop.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SupervisorStateV1 {
    /// No child is supervised; the next [`SupervisorMachine::advance`] boots or resumes.
    Idle,
    /// Exactly one replacement-process child is live.
    Supervising {
        /// Role the child acknowledged.
        role: ChildRoleV1,
        /// Generation that booted the child.
        generation: LaunchGeneration,
        /// Process epoch bound to the child's App lease.
        process_epoch: ProcessEpoch,
        /// Adapter handle for the live child.
        child: SpawnedProcess,
    },
    /// The product ended without another spawn.
    ProductExited,
    /// Automatic restart stopped.
    Halted,
}

/// Terminal or in-progress supervisor outcome.
#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case", tag = "outcome")]
pub enum SupervisorOutcomeV1 {
    /// The loop still has a live or idle child to drive.
    Running,
    /// Shell or recovery exited without a next intent.
    ProductExited {
        /// Last supervised role.
        last_role: ChildRoleV1,
        /// Product-ending exit kind.
        exit_kind: ChildExitKindV1,
    },
    /// Automatic restart stopped to prevent a recovery loop.
    Halted {
        /// Stable halt reason.
        reason: RecoveryReasonV1,
    },
}

/// One ordered hop in a deterministic supervisor report.
#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct SupervisorHopV1 {
    generation: LaunchGeneration,
    role: ChildRoleV1,
    activation: TransitionOutcomeV1,
    exit_kind: Option<ChildExitKindV1>,
    needs_recovery: bool,
}

impl SupervisorHopV1 {
    /// Returns the hop generation.
    #[must_use]
    pub const fn generation(self) -> LaunchGeneration {
        self.generation
    }

    /// Returns the hop role.
    #[must_use]
    pub const fn role(self) -> ChildRoleV1 {
        self.role
    }

    /// Returns the activation outcome that started the hop.
    #[must_use]
    pub const fn activation(self) -> TransitionOutcomeV1 {
        self.activation
    }

    /// Returns the child-exit kind after the hop was reaped.
    #[must_use]
    pub const fn exit_kind(self) -> Option<ChildExitKindV1> {
        self.exit_kind
    }

    /// Returns whether this hop marked the world `NeedsRecovery`.
    #[must_use]
    pub const fn needs_recovery(self) -> bool {
        self.needs_recovery
    }
}

/// Deterministic semantic report for one product-loop drive.
#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct SupervisorReportV1 {
    schema_version: u32,
    outcome: SupervisorOutcomeV1,
    hops: Vec<SupervisorHopV1>,
    failures: Vec<LaunchFailureReceiptV1>,
    last_durable_world: Option<DurableWorldRevisionV1>,
    last_written_world: Option<DurableWorldRevisionV1>,
    needs_recovery: bool,
}

impl SupervisorReportV1 {
    /// Returns the current or terminal outcome.
    #[must_use]
    pub const fn outcome(&self) -> SupervisorOutcomeV1 {
        self.outcome
    }

    /// Returns hops in occurrence order.
    #[must_use]
    pub fn hops(&self) -> &[SupervisorHopV1] {
        &self.hops
    }

    /// Returns deterministic failures in occurrence order.
    #[must_use]
    pub fn failures(&self) -> &[LaunchFailureReceiptV1] {
        &self.failures
    }

    /// Returns the latest durable world revision retained by the supervisor.
    #[must_use]
    pub const fn last_durable_world(&self) -> Option<DurableWorldRevisionV1> {
        self.last_durable_world
    }

    /// Returns the latest written world revision retained by the supervisor.
    #[must_use]
    pub const fn last_written_world(&self) -> Option<DurableWorldRevisionV1> {
        self.last_written_world
    }

    /// Returns whether the latest world must be presented as `NeedsRecovery`.
    #[must_use]
    pub const fn needs_recovery(&self) -> bool {
        self.needs_recovery
    }
}

/// Non-Bevy replacement-process product loop.
///
/// The supervisor never creates a Bevy `App` or window. It reuses the sealed
/// intent store, one-shot child-exit store, and fail-closed bootstrap machine.
#[derive(Debug)]
pub struct SupervisorMachine {
    config: SupervisorConfigV1,
    state: SupervisorStateV1,
    consecutive_failures: u8,
    recovery_used_for: Option<LaunchGeneration>,
    hops: Vec<SupervisorHopV1>,
    failures: Vec<LaunchFailureReceiptV1>,
    last_durable_world: Option<DurableWorldRevisionV1>,
    last_written_world: Option<DurableWorldRevisionV1>,
    needs_recovery: bool,
    last_role: Option<ChildRoleV1>,
    last_exit_kind: Option<ChildExitKindV1>,
    halt_reason: RecoveryReasonV1,
}

impl SupervisorMachine {
    /// Creates an idle product supervisor.
    #[must_use]
    pub const fn new(config: SupervisorConfigV1) -> Self {
        Self {
            config,
            state: SupervisorStateV1::Idle,
            consecutive_failures: 0,
            recovery_used_for: None,
            hops: Vec::new(),
            failures: Vec::new(),
            last_durable_world: None,
            last_written_world: None,
            needs_recovery: false,
            last_role: None,
            last_exit_kind: None,
            halt_reason: RecoveryReasonV1::RecoveryFailure,
        }
    }

    /// Returns the current supervisor state.
    #[must_use]
    pub const fn state(&self) -> SupervisorStateV1 {
        self.state
    }

    /// Drives the product loop until it exits or halts.
    ///
    /// Child adapters must persist a [`ChildExitReportV1`] and optional
    /// [`LaunchIntentV1`] before [`ProcessControl::await_exit`] returns.
    pub fn run<S, E, P>(
        &mut self,
        intent_store: &mut S,
        exit_store: &mut E,
        process: &mut P,
    ) -> SupervisorReportV1
    where
        S: AtomicLaunchIntentStore,
        E: AtomicChildExitStore,
        P: ProcessControl,
    {
        while matches!(
            self.state,
            SupervisorStateV1::Idle | SupervisorStateV1::Supervising { .. }
        ) {
            let _ = self.advance(intent_store, exit_store, process);
        }
        self.report()
    }

    /// Advances one idle boot/resume or one live-child reap (and next boot).
    pub fn advance<S, E, P>(
        &mut self,
        intent_store: &mut S,
        exit_store: &mut E,
        process: &mut P,
    ) -> SupervisorReportV1
    where
        S: AtomicLaunchIntentStore,
        E: AtomicChildExitStore,
        P: ProcessControl,
    {
        if self.hops.len() >= MAX_SUPERVISOR_HOPS {
            return self.halt(
                None,
                None,
                RecoveryReasonV1::RecoveryFailure,
                LaunchPhaseV1::RecoverySpawn,
                LaunchFailureCodeV1::RecoveryLoopSuppressed,
            );
        }
        self.refresh_clock(process);
        match self.state {
            SupervisorStateV1::Idle => self.boot_or_resume(intent_store, exit_store, process),
            SupervisorStateV1::Supervising { .. } => {
                self.reap_child(intent_store, exit_store, process)
            }
            SupervisorStateV1::ProductExited | SupervisorStateV1::Halted => self.report(),
        }
    }

    fn refresh_clock(&mut self, process: &impl ProcessControl) {
        if let Some(now_ms) = process.current_time_ms() {
            self.config.now_ms = self.config.now_ms.max(now_ms);
        }
    }

    fn boot_or_resume<S, E, P>(
        &mut self,
        intent_store: &mut S,
        exit_store: &mut E,
        process: &mut P,
    ) -> SupervisorReportV1
    where
        S: AtomicLaunchIntentStore,
        E: AtomicChildExitStore,
        P: ProcessControl,
    {
        let exit_slot = match exit_store.read() {
            Ok(slot) => slot,
            Err(error) => return self.halt_store(None, None, &error),
        };
        if matches!(
            exit_slot.disposition(),
            Some(crate::ChildExitDisposition::Pending)
        ) {
            return self.consume_published_exit(intent_store, exit_store, process, None);
        }
        let intent_slot = match intent_store.read() {
            Ok(slot) => slot,
            Err(error) => return self.halt_store(None, None, &error),
        };
        match intent_slot {
            IntentSlot::Empty => self.spawn_initial_shell(intent_store, process),
            IntentSlot::Occupied {
                disposition: SlotDisposition::Pending,
                ..
            } if exit_slot.disposition() == Some(crate::ChildExitDisposition::Consumed) => {
                self.bootstrap_pending(intent_store, process)
            }
            IntentSlot::Occupied {
                disposition: SlotDisposition::Pending,
                bytes,
                blob_hash,
                ..
            } => {
                self.push_failure(LaunchFailureReceiptV1::new(
                    LaunchIntentV1::authenticate_at_rest(&bytes)
                        .ok()
                        .map(|intent| intent.generation()),
                    None,
                    None,
                    LaunchPhaseV1::ChildExit,
                    LaunchFailureCodeV1::ChildReportMissing,
                ));
                self.recover_from_blob(
                    intent_store,
                    process,
                    Some(blob_hash),
                    RecoveryReasonV1::InvalidIntent,
                )
            }
            IntentSlot::Occupied {
                disposition: SlotDisposition::Claimed | SlotDisposition::Consumed,
                bytes,
                ..
            } => self.resume_claimed_or_consumed(intent_store, process, &bytes),
            IntentSlot::Occupied { .. } => self.apply_bootstrap(&BootstrapMachine::new().activate(
                intent_store,
                &self.shell_policy(),
                process,
            )),
        }
    }

    fn spawn_initial_shell<S, P>(
        &mut self,
        intent_store: &mut S,
        process: &mut P,
    ) -> SupervisorReportV1
    where
        S: AtomicLaunchIntentStore,
        P: ProcessControl,
    {
        if self.consecutive_failures >= MAX_CONSECUTIVE_CHILD_FAILURES {
            return self.halt(
                None,
                None,
                RecoveryReasonV1::SpawnFailure,
                LaunchPhaseV1::Spawn,
                LaunchFailureCodeV1::RecoveryLoopSuppressed,
            );
        }
        let request = ProcessLaunchRequestV1::InitialShell {
            generation: LaunchGeneration::FIRST,
            shell_lock_hash: self.config.shell_lock_hash,
        };
        let child = match process.spawn(&request) {
            Ok(child) => child,
            Err(failure) => {
                self.consecutive_failures = self.consecutive_failures.saturating_add(1);
                self.push_failure(LaunchFailureReceiptV1::with_detail(
                    Some(LaunchGeneration::FIRST),
                    None,
                    Some(LaunchTargetV1::Shell),
                    LaunchPhaseV1::Spawn,
                    LaunchFailureCodeV1::SpawnFailed,
                    LaunchFailureDetailV1::Spawn { failure },
                ));
                return self.halt(
                    Some(LaunchGeneration::FIRST),
                    Some(LaunchTargetV1::Shell),
                    RecoveryReasonV1::SpawnFailure,
                    LaunchPhaseV1::Spawn,
                    LaunchFailureCodeV1::SpawnFailed,
                );
            }
        };
        match process.await_bootstrap(child, &request, MAX_BOOTSTRAP_ACK_WAIT_MS) {
            BootObservationV1::InitialShellAcknowledged { process_epoch } => self
                .enter_supervising(
                    ChildRoleV1::Shell,
                    LaunchGeneration::FIRST,
                    process_epoch,
                    child,
                    TransitionOutcomeV1::Activated {
                        generation: LaunchGeneration::FIRST,
                        target: LaunchTargetV1::Shell,
                        process_epoch,
                    },
                    false,
                ),
            BootObservationV1::Crashed { .. }
            | BootObservationV1::BootFailed
            | BootObservationV1::TimedOut => {
                if process.terminate(child).is_err() {
                    return self.halt(
                        Some(LaunchGeneration::FIRST),
                        Some(LaunchTargetV1::Shell),
                        RecoveryReasonV1::RecoveryFailure,
                        LaunchPhaseV1::Terminate,
                        LaunchFailureCodeV1::TerminationFailed,
                    );
                }
                self.consecutive_failures = self.consecutive_failures.saturating_add(1);
                self.push_failure(LaunchFailureReceiptV1::new(
                    Some(LaunchGeneration::FIRST),
                    None,
                    Some(LaunchTargetV1::Shell),
                    LaunchPhaseV1::Boot,
                    LaunchFailureCodeV1::BootFailed,
                ));
                self.apply_bootstrap(&BootstrapMachine::new().activate(
                    intent_store,
                    &self.shell_policy(),
                    process,
                ))
            }
            BootObservationV1::Acknowledged(_) | BootObservationV1::RecoveryAcknowledged(_) => {
                let _ = process.terminate(child);
                self.halt(
                    Some(LaunchGeneration::FIRST),
                    Some(LaunchTargetV1::Shell),
                    RecoveryReasonV1::AckFailure,
                    LaunchPhaseV1::Ack,
                    LaunchFailureCodeV1::AckInvalid,
                )
            }
        }
    }

    fn resume_claimed_or_consumed<S, P>(
        &mut self,
        intent_store: &mut S,
        process: &mut P,
        bytes: &[u8],
    ) -> SupervisorReportV1
    where
        S: AtomicLaunchIntentStore,
        P: ProcessControl,
    {
        let Ok(intent) = LaunchIntentV1::authenticate_at_rest(bytes) else {
            return self.apply_bootstrap(&BootstrapMachine::new().activate(
                intent_store,
                &self.shell_policy(),
                process,
            ));
        };
        match process.reconcile_prior_child(&intent) {
            PriorChildStatusV1::Running {
                process: child,
                process_epoch,
            } => {
                let role = match intent.target() {
                    LaunchTargetV1::Shell => ChildRoleV1::Shell,
                    LaunchTargetV1::World { world_id } => ChildRoleV1::World { world_id },
                };
                self.enter_supervising(
                    role,
                    intent.generation(),
                    process_epoch,
                    child,
                    TransitionOutcomeV1::Activated {
                        generation: intent.generation(),
                        target: intent.target(),
                        process_epoch,
                    },
                    false,
                )
            }
            PriorChildStatusV1::Unknown => self.halt(
                Some(intent.generation()),
                Some(intent.target()),
                RecoveryReasonV1::RecoveryFailure,
                LaunchPhaseV1::IntentClaim,
                LaunchFailureCodeV1::AttemptAlreadyClaimed,
            ),
            PriorChildStatusV1::Exited { .. } => {
                let policy = match TransitionValidationPolicy::for_authenticated_intent(
                    self.config.now_ms,
                    self.config.shell_lock_hash,
                    &intent,
                ) {
                    Ok(policy) => policy,
                    Err(_) => self.shell_policy(),
                };
                self.apply_bootstrap(&BootstrapMachine::new().activate(
                    intent_store,
                    &policy,
                    process,
                ))
            }
        }
    }

    fn bootstrap_pending<S, P>(
        &mut self,
        intent_store: &mut S,
        process: &mut P,
    ) -> SupervisorReportV1
    where
        S: AtomicLaunchIntentStore,
        P: ProcessControl,
    {
        let Ok(IntentSlot::Occupied { bytes, .. }) = intent_store.read() else {
            return self.halt(
                None,
                None,
                RecoveryReasonV1::InvalidIntent,
                LaunchPhaseV1::IntentRead,
                LaunchFailureCodeV1::IntentMissing,
            );
        };
        let Ok(intent) = LaunchIntentV1::authenticate_at_rest(&bytes) else {
            return self.apply_bootstrap(&BootstrapMachine::new().activate(
                intent_store,
                &self.shell_policy(),
                process,
            ));
        };
        let Ok(policy) = TransitionValidationPolicy::for_authenticated_intent(
            self.config.now_ms,
            self.config.shell_lock_hash,
            &intent,
        ) else {
            return self.apply_bootstrap(&BootstrapMachine::new().activate(
                intent_store,
                &self.shell_policy(),
                process,
            ));
        };
        self.apply_bootstrap(&BootstrapMachine::new().activate(intent_store, &policy, process))
    }

    fn reap_child<S, E, P>(
        &mut self,
        intent_store: &mut S,
        exit_store: &mut E,
        process: &mut P,
    ) -> SupervisorReportV1
    where
        S: AtomicLaunchIntentStore,
        E: AtomicChildExitStore,
        P: ProcessControl,
    {
        let SupervisorStateV1::Supervising {
            role,
            generation,
            process_epoch,
            child,
        } = self.state
        else {
            return self.report();
        };
        let observation = process.await_exit(child, MAX_CHILD_SHUTDOWN_WAIT_MS);
        self.refresh_clock(process);
        match observation {
            ChildObservationV1::Running => self.report(),
            ChildObservationV1::Unknown => self.halt(
                Some(generation),
                Some(role_target(role)),
                RecoveryReasonV1::RecoveryFailure,
                LaunchPhaseV1::ChildShutdown,
                LaunchFailureCodeV1::AttemptAlreadyClaimed,
            ),
            ChildObservationV1::TimedOut => {
                if process.terminate(child).is_err() {
                    return self.halt(
                        Some(generation),
                        Some(role_target(role)),
                        RecoveryReasonV1::RecoveryFailure,
                        LaunchPhaseV1::Terminate,
                        LaunchFailureCodeV1::TerminationFailed,
                    );
                }
                if matches!(role, ChildRoleV1::World { .. }) {
                    self.needs_recovery = true;
                }
                self.push_failure(LaunchFailureReceiptV1::new(
                    Some(generation),
                    None,
                    Some(role_target(role)),
                    LaunchPhaseV1::ChildShutdown,
                    LaunchFailureCodeV1::ShutdownTimedOut,
                ));
                self.fail_supervised(
                    intent_store,
                    process,
                    generation,
                    role,
                    RecoveryReasonV1::AckFailure,
                )
            }
            ChildObservationV1::Exited { .. } => {
                self.consume_published_exit(intent_store, exit_store, process, Some(process_epoch))
            }
        }
    }

    fn consume_published_exit<S, E, P>(
        &mut self,
        intent_store: &mut S,
        exit_store: &mut E,
        process: &mut P,
        expected_epoch: Option<ProcessEpoch>,
    ) -> SupervisorReportV1
    where
        S: AtomicLaunchIntentStore,
        E: AtomicChildExitStore,
        P: ProcessControl,
    {
        let supervised = match self.state {
            SupervisorStateV1::Supervising {
                role,
                generation,
                process_epoch,
                ..
            } => Some((role, generation, expected_epoch.or(Some(process_epoch)))),
            SupervisorStateV1::Idle => None,
            SupervisorStateV1::ProductExited | SupervisorStateV1::Halted => return self.report(),
        };
        let fallback_generation = supervised.map(|(_, generation, _)| generation);
        let fallback_role = supervised.map(|(role, _, _)| role);
        let report = match self.take_child_exit_report(
            exit_store,
            fallback_generation,
            fallback_role.map(role_target),
        ) {
            Ok(report) => report,
            Err(TakeExitError::ConsumeFailed) => {
                return self.halt(
                    fallback_generation,
                    fallback_role.map(role_target),
                    RecoveryReasonV1::ConsumeFailure,
                    LaunchPhaseV1::ChildExit,
                    LaunchFailureCodeV1::ConsumeFailed,
                );
            }
            Err(TakeExitError::Corrupt) => {
                return self.fail_supervised(
                    intent_store,
                    process,
                    fallback_generation.unwrap_or(LaunchGeneration::FIRST),
                    fallback_role.unwrap_or(ChildRoleV1::Shell),
                    RecoveryReasonV1::InvalidIntent,
                );
            }
            Err(TakeExitError::Store(error)) => {
                return self.halt_store(
                    fallback_generation,
                    fallback_role.map(role_target),
                    &error,
                );
            }
        };
        let (role, generation) = match self.bind_exit_identity(supervised, report.as_ref()) {
            Ok(identity) => identity,
            Err((generation, role)) => {
                return self.fail_supervised(
                    intent_store,
                    process,
                    generation,
                    role,
                    RecoveryReasonV1::InvalidIntent,
                );
            }
        };
        let pending_intent = match intent_store.read() {
            Ok(IntentSlot::Occupied {
                disposition: SlotDisposition::Pending,
                bytes,
                ..
            }) => LaunchIntentV1::authenticate_at_rest(&bytes).ok(),
            Ok(_) => None,
            Err(error) => {
                return self.halt_store(Some(generation), Some(role_target(role)), &error);
            }
        };
        if let Some(report) = report.as_ref() {
            self.record_world_revisions(report);
        }
        self.apply_next_action(
            intent_store,
            process,
            role,
            generation,
            report.as_ref(),
            pending_intent.as_ref(),
        )
    }

    fn apply_next_action<S, P>(
        &mut self,
        intent_store: &mut S,
        process: &mut P,
        role: ChildRoleV1,
        generation: LaunchGeneration,
        report: Option<&ChildExitReportV1>,
        pending_intent: Option<&LaunchIntentV1>,
    ) -> SupervisorReportV1
    where
        S: AtomicLaunchIntentStore,
        P: ProcessControl,
    {
        match decide_next(role, report, pending_intent) {
            NextAction::ProductExit { kind } => self.product_exit(role, kind),
            NextAction::ActivatePending => self.bootstrap_pending(intent_store, process),
            NextAction::Recover {
                reason,
                code,
                needs_recovery,
            } => {
                if needs_recovery {
                    self.needs_recovery = true;
                }
                self.push_failure(LaunchFailureReceiptV1::new(
                    Some(generation),
                    pending_intent.map(LaunchIntentV1::attempt),
                    pending_intent
                        .map(LaunchIntentV1::target)
                        .or(Some(role_target(role))),
                    LaunchPhaseV1::ChildExit,
                    code,
                ));
                self.fail_supervised(intent_store, process, generation, role, reason)
            }
            NextAction::Halt { reason, code } => self.halt(
                Some(generation),
                Some(role_target(role)),
                reason,
                LaunchPhaseV1::RecoverySpawn,
                code,
            ),
        }
    }

    fn take_child_exit_report<E>(
        &mut self,
        exit_store: &mut E,
        fallback_generation: Option<LaunchGeneration>,
        fallback_target: Option<LaunchTargetV1>,
    ) -> Result<Option<ChildExitReportV1>, TakeExitError>
    where
        E: AtomicChildExitStore,
    {
        let exit_slot = exit_store.read().map_err(TakeExitError::Store)?;
        let ChildExitSlot::Occupied {
            disposition: crate::ChildExitDisposition::Pending,
            bytes,
            blob_hash,
        } = exit_slot
        else {
            return Ok(None);
        };
        let Ok(report) = ChildExitReportV1::authenticate_at_rest(&bytes) else {
            let _ = exit_store.quarantine(blob_hash);
            self.push_failure(LaunchFailureReceiptV1::new(
                fallback_generation,
                None,
                fallback_target,
                LaunchPhaseV1::ChildExit,
                LaunchFailureCodeV1::ChildReportCorrupt,
            ));
            return Err(TakeExitError::Corrupt);
        };
        if exit_store.consume(blob_hash).is_err() {
            return Err(TakeExitError::ConsumeFailed);
        }
        Ok(Some(report))
    }

    fn bind_exit_identity(
        &mut self,
        supervised: Option<(ChildRoleV1, LaunchGeneration, Option<ProcessEpoch>)>,
        report: Option<&ChildExitReportV1>,
    ) -> Result<(ChildRoleV1, LaunchGeneration), (LaunchGeneration, ChildRoleV1)> {
        if let Some((role, generation, epoch)) = supervised {
            if let Some(report) = report
                && let Some(epoch) = epoch
                && report
                    .validate_supervised_child(generation, epoch, role, self.config.shell_lock_hash)
                    .is_err()
            {
                self.record_world_revisions(report);
                self.push_failure(LaunchFailureReceiptV1::new(
                    Some(generation),
                    None,
                    Some(role_target(role)),
                    LaunchPhaseV1::ChildExit,
                    LaunchFailureCodeV1::ChildReportMismatch,
                ));
                return Err((generation, role));
            }
            return Ok((role, generation));
        }
        let Some(report) = report else {
            return Ok((ChildRoleV1::Shell, LaunchGeneration::FIRST));
        };
        if report.shell_lock_hash() != self.config.shell_lock_hash {
            self.push_failure(LaunchFailureReceiptV1::new(
                Some(report.child_generation()),
                None,
                Some(role_target(report.role())),
                LaunchPhaseV1::ChildExit,
                LaunchFailureCodeV1::ChildReportMismatch,
            ));
            return Err((report.child_generation(), report.role()));
        }
        Ok((report.role(), report.child_generation()))
    }

    fn fail_supervised<S, P>(
        &mut self,
        intent_store: &mut S,
        process: &mut P,
        generation: LaunchGeneration,
        role: ChildRoleV1,
        reason: RecoveryReasonV1,
    ) -> SupervisorReportV1
    where
        S: AtomicLaunchIntentStore,
        P: ProcessControl,
    {
        if self.recovery_used_for == Some(generation) || matches!(role, ChildRoleV1::Recovery) {
            return self.halt(
                Some(generation),
                Some(role_target(role)),
                RecoveryReasonV1::RecoveryFailure,
                LaunchPhaseV1::RecoverySpawn,
                LaunchFailureCodeV1::RecoveryLoopSuppressed,
            );
        }
        let blob_hash = match intent_store.read() {
            Ok(IntentSlot::Occupied {
                disposition:
                    SlotDisposition::Pending | SlotDisposition::Claimed | SlotDisposition::Consumed,
                blob_hash,
                ..
            }) => Some(blob_hash),
            Ok(_) => None,
            Err(error) => {
                return self.halt_store(Some(generation), Some(role_target(role)), &error);
            }
        };
        if let Some(blob_hash) = blob_hash
            && intent_store.quarantine(blob_hash).is_err()
        {
            return self.halt(
                Some(generation),
                Some(role_target(role)),
                RecoveryReasonV1::RecoveryFailure,
                LaunchPhaseV1::Quarantine,
                LaunchFailureCodeV1::QuarantineFailed,
            );
        }
        self.recover_from_blob(intent_store, process, blob_hash, reason)
    }

    fn recover_from_blob<S, P>(
        &mut self,
        intent_store: &mut S,
        process: &mut P,
        _blob_hash: Option<CanonicalHash>,
        _reason: RecoveryReasonV1,
    ) -> SupervisorReportV1
    where
        S: AtomicLaunchIntentStore,
        P: ProcessControl,
    {
        self.consecutive_failures = self.consecutive_failures.saturating_add(1);
        if self.consecutive_failures > MAX_CONSECUTIVE_CHILD_FAILURES {
            return self.halt(
                None,
                None,
                RecoveryReasonV1::RecoveryFailure,
                LaunchPhaseV1::RecoverySpawn,
                LaunchFailureCodeV1::RecoveryLoopSuppressed,
            );
        }
        self.apply_bootstrap(&BootstrapMachine::new().activate(
            intent_store,
            &self.shell_policy(),
            process,
        ))
    }

    fn apply_bootstrap(&mut self, report: &crate::BootstrapReportV1) -> SupervisorReportV1 {
        for failure in report.failures() {
            self.push_failure(*failure);
        }
        match (report.outcome(), report.child()) {
            (
                TransitionOutcomeV1::Activated {
                    generation,
                    target,
                    process_epoch,
                },
                Some(child),
            ) => {
                self.consecutive_failures = 0;
                self.recovery_used_for = None;
                let role = match target {
                    LaunchTargetV1::Shell => ChildRoleV1::Shell,
                    LaunchTargetV1::World { world_id } => ChildRoleV1::World { world_id },
                };
                self.enter_supervising(
                    role,
                    generation,
                    process_epoch,
                    child,
                    TransitionOutcomeV1::Activated {
                        generation,
                        target,
                        process_epoch,
                    },
                    false,
                )
            }
            (
                TransitionOutcomeV1::RecoveryShell {
                    source_generation,
                    process_epoch,
                    reason,
                },
                Some(child),
            ) => {
                let generation = source_generation.unwrap_or(LaunchGeneration::FIRST);
                self.recovery_used_for = Some(generation);
                self.needs_recovery = true;
                self.enter_supervising(
                    ChildRoleV1::Recovery,
                    generation,
                    process_epoch,
                    child,
                    TransitionOutcomeV1::RecoveryShell {
                        source_generation,
                        process_epoch,
                        reason,
                    },
                    true,
                )
            }
            (TransitionOutcomeV1::Halted, _) => {
                self.state = SupervisorStateV1::Halted;
                self.halt_reason = RecoveryReasonV1::RecoveryFailure;
                self.report()
            }
            _ => self.halt(
                None,
                None,
                RecoveryReasonV1::RecoveryFailure,
                LaunchPhaseV1::RecoverySpawn,
                LaunchFailureCodeV1::SpawnFailed,
            ),
        }
    }

    fn enter_supervising(
        &mut self,
        role: ChildRoleV1,
        generation: LaunchGeneration,
        process_epoch: ProcessEpoch,
        child: SpawnedProcess,
        activation: TransitionOutcomeV1,
        needs_recovery: bool,
    ) -> SupervisorReportV1 {
        self.state = SupervisorStateV1::Supervising {
            role,
            generation,
            process_epoch,
            child,
        };
        self.last_role = Some(role);
        self.push_hop(SupervisorHopV1 {
            generation,
            role,
            activation,
            exit_kind: None,
            needs_recovery,
        });
        self.report()
    }

    fn product_exit(&mut self, role: ChildRoleV1, kind: ChildExitKindV1) -> SupervisorReportV1 {
        if let Some(hop) = self.hops.last_mut() {
            hop.exit_kind = Some(kind);
        }
        self.state = SupervisorStateV1::ProductExited;
        self.last_role = Some(role);
        self.last_exit_kind = Some(kind);
        self.report()
    }

    fn record_world_revisions(&mut self, report: &ChildExitReportV1) {
        if let Some(durable) = report.last_durable_world() {
            self.last_durable_world = Some(durable);
        }
        if let Some(written) = report.last_written_world() {
            self.last_written_world = Some(written);
        }
        if matches!(
            report.exit_kind(),
            ChildExitKindV1::WrittenOnly | ChildExitKindV1::ShutdownTimeout
        ) {
            self.needs_recovery = true;
        }
    }

    fn shell_policy(&self) -> TransitionValidationPolicy {
        TransitionValidationPolicy::for_shell(
            self.config.now_ms,
            self.config.shell_lock_hash,
            self.config.confirmed_setting_transaction_revision,
        )
    }

    fn halt(
        &mut self,
        generation: Option<LaunchGeneration>,
        target: Option<LaunchTargetV1>,
        reason: RecoveryReasonV1,
        phase: LaunchPhaseV1,
        code: LaunchFailureCodeV1,
    ) -> SupervisorReportV1 {
        self.push_failure(LaunchFailureReceiptV1::new(
            generation, None, target, phase, code,
        ));
        self.state = SupervisorStateV1::Halted;
        self.halt_reason = reason;
        self.report()
    }

    fn halt_store(
        &mut self,
        generation: Option<LaunchGeneration>,
        target: Option<LaunchTargetV1>,
        error: &IntentStoreError,
    ) -> SupervisorReportV1 {
        let (phase, code) = store_failure_code(error);
        self.halt(
            generation,
            target,
            RecoveryReasonV1::RecoveryFailure,
            phase,
            code,
        )
    }

    fn push_failure(&mut self, failure: LaunchFailureReceiptV1) {
        if self.failures.len() < MAX_REPORT_FAILURES {
            self.failures.push(failure);
        }
    }

    fn push_hop(&mut self, hop: SupervisorHopV1) {
        if self.hops.len() < MAX_SUPERVISOR_HOPS {
            self.hops.push(hop);
        }
    }

    fn report(&self) -> SupervisorReportV1 {
        let outcome = match self.state {
            SupervisorStateV1::Idle | SupervisorStateV1::Supervising { .. } => {
                SupervisorOutcomeV1::Running
            }
            SupervisorStateV1::ProductExited => SupervisorOutcomeV1::ProductExited {
                last_role: self.last_role.unwrap_or(ChildRoleV1::Shell),
                exit_kind: self.last_exit_kind.unwrap_or(ChildExitKindV1::ShellQuit),
            },
            SupervisorStateV1::Halted => SupervisorOutcomeV1::Halted {
                reason: self.halt_reason,
            },
        };
        SupervisorReportV1 {
            schema_version: SUPERVISOR_REPORT_SCHEMA_VERSION,
            outcome,
            hops: self.hops.clone(),
            failures: self.failures.clone(),
            last_durable_world: self.last_durable_world,
            last_written_world: self.last_written_world,
            needs_recovery: self.needs_recovery,
        }
    }
}

enum TakeExitError {
    ConsumeFailed,
    Corrupt,
    Store(IntentStoreError),
}

enum NextAction {
    ProductExit {
        kind: ChildExitKindV1,
    },
    ActivatePending,
    Recover {
        reason: RecoveryReasonV1,
        code: LaunchFailureCodeV1,
        needs_recovery: bool,
    },
    Halt {
        reason: RecoveryReasonV1,
        code: LaunchFailureCodeV1,
    },
}

fn decide_next(
    role: ChildRoleV1,
    report: Option<&ChildExitReportV1>,
    pending_intent: Option<&LaunchIntentV1>,
) -> NextAction {
    let Some(report) = report else {
        return NextAction::Recover {
            reason: RecoveryReasonV1::InvalidIntent,
            code: LaunchFailureCodeV1::ChildReportMissing,
            needs_recovery: matches!(role, ChildRoleV1::World { .. }),
        };
    };
    if matches!(role, ChildRoleV1::Recovery)
        && matches!(
            report.exit_kind(),
            ChildExitKindV1::Crash | ChildExitKindV1::ShutdownTimeout
        )
    {
        return NextAction::Halt {
            reason: RecoveryReasonV1::RecoveryFailure,
            code: LaunchFailureCodeV1::RecoveryLoopSuppressed,
        };
    }
    if let Some(intent) = pending_intent {
        if intent.generation().get() <= report.child_generation().get() {
            return NextAction::Recover {
                reason: RecoveryReasonV1::InvalidIntent,
                code: LaunchFailureCodeV1::IntentStale,
                needs_recovery: matches!(role, ChildRoleV1::World { .. }),
            };
        }
        if report.matches_intent(intent) && report.exit_kind().is_normal_handoff() {
            return NextAction::ActivatePending;
        }
    }
    if report.exit_kind().is_product_exit()
        && pending_intent.is_none()
        && matches!(role, ChildRoleV1::Shell | ChildRoleV1::Recovery)
    {
        return NextAction::ProductExit {
            kind: report.exit_kind(),
        };
    }
    let (code, needs_recovery) = match report.exit_kind() {
        ChildExitKindV1::WrittenOnly => (LaunchFailureCodeV1::WrittenOnly, true),
        ChildExitKindV1::ShutdownTimeout => (LaunchFailureCodeV1::ShutdownTimedOut, true),
        ChildExitKindV1::Crash => (LaunchFailureCodeV1::ProcessCrashed, true),
        ChildExitKindV1::SaveAndQuit
        | ChildExitKindV1::Handoff
        | ChildExitKindV1::OsClose
        | ChildExitKindV1::ShellQuit => (LaunchFailureCodeV1::ChildReportMismatch, true),
    };
    NextAction::Recover {
        reason: RecoveryReasonV1::InvalidIntent,
        code,
        needs_recovery: needs_recovery && matches!(role, ChildRoleV1::World { .. }),
    }
}

const fn role_target(role: ChildRoleV1) -> LaunchTargetV1 {
    match role {
        ChildRoleV1::Shell | ChildRoleV1::Recovery => LaunchTargetV1::Shell,
        ChildRoleV1::World { world_id } => LaunchTargetV1::World { world_id },
    }
}

fn store_failure_code(error: &IntentStoreError) -> (LaunchPhaseV1, LaunchFailureCodeV1) {
    match error {
        IntentStoreError::Io {
            operation: crate::StoreOperation::Read,
            ..
        } => (
            LaunchPhaseV1::IntentRead,
            LaunchFailureCodeV1::IntentReadFailed,
        ),
        _ => (
            LaunchPhaseV1::IntentRead,
            LaunchFailureCodeV1::IntentCorrupt,
        ),
    }
}

#[cfg(test)]
mod tests {
    use std::{collections::VecDeque, fs, path::PathBuf};

    use latticeaxiom_core::WorldId;

    use crate::machine::{ClientTransitionMachine, TransitionBarrier};
    use crate::{
        BootstrapAckV1, ChildExitReportDraftV1, ClientRuntimeStateV1, DurableWorldRevisionV1,
        FileChildExitStore, FileLaunchIntentStore, LaunchAttempt, LaunchIntentDraftV1,
        ProcessSupervisorIdentityV1, RecoveryBootstrapAckV1, RecoveryChildStatusV1,
        RecoveryLaunchRequestV1, ShutdownBarrierFailureV1, ShutdownBarrierReceiptV1,
        SpawnFailureV1, TerminationFailureV1, WorldRevision,
    };

    use super::*;

    #[derive(Debug)]
    struct TestDirectory {
        path: PathBuf,
        _directory: tempfile::TempDir,
    }

    impl TestDirectory {
        fn create() -> Self {
            let directory = tempfile::Builder::new()
                .prefix("latticeaxiom-launcher-supervisor-")
                .tempdir()
                .unwrap_or_else(|error| {
                    panic!("supervisor test directory was not created: {error}")
                });
            let path = fs::canonicalize(directory.path()).unwrap_or_else(|error| {
                panic!("supervisor test directory did not canonicalize: {error}")
            });
            Self {
                path,
                _directory: directory,
            }
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
        now_ms: Option<u64>,
        exit_time_ms: Option<u64>,
        reconciliations: VecDeque<PriorChildStatusV1>,
        spawn_failures: VecDeque<Option<SpawnFailureV1>>,
        observations: VecDeque<BootObservationV1>,
        exits: VecDeque<ChildObservationV1>,
        acknowledge_recovery: bool,
        recovery_status: Option<RecoveryChildStatusV1>,
        spawn_count: usize,
        terminated: usize,
    }

    impl ProcessControl for FakeProcess {
        fn current_time_ms(&self) -> Option<u64> {
            self.now_ms
        }

        fn supervisor_identity(&self) -> ProcessSupervisorIdentityV1 {
            ProcessSupervisorIdentityV1::new(CanonicalHash::digest(b"test process supervisor"))
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
            if let Some(status) = self.recovery_status {
                return Ok(status);
            }
            self.spawn_count = self.spawn_count.saturating_add(1);
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
                return BootObservationV1::RecoveryAcknowledged(
                    RecoveryBootstrapAckV1::for_ready_request(
                        recovery_request,
                        crate::model::test_app_created_proof(epoch(99)),
                    ),
                );
            }
            self.observations
                .pop_front()
                .unwrap_or(BootObservationV1::TimedOut)
        }

        fn terminate(&mut self, process: SpawnedProcess) -> Result<(), TerminationFailureV1> {
            self.terminated = self.terminated.saturating_add(1);
            if matches!(
                self.recovery_status,
                Some(RecoveryChildStatusV1::Acquired(recovery)) if recovery == process
            ) {
                self.recovery_status = Some(RecoveryChildStatusV1::Exited { exit_code: None });
            }
            Ok(())
        }

        fn await_exit(&mut self, _process: SpawnedProcess, deadline_ms: u64) -> ChildObservationV1 {
            assert_eq!(deadline_ms, MAX_CHILD_SHUTDOWN_WAIT_MS);
            if let Some(now_ms) = self.exit_time_ms.take() {
                self.now_ms = Some(now_ms);
            }
            self.exits
                .pop_front()
                .unwrap_or(ChildObservationV1::TimedOut)
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

    const fn setting_revision(value: u64) -> SettingTransactionRevision {
        SettingTransactionRevision::new(value)
    }

    fn supervisor_identity() -> ProcessSupervisorIdentityV1 {
        ProcessSupervisorIdentityV1::new(CanonicalHash::digest(b"test process supervisor"))
    }

    fn config() -> SupervisorConfigV1 {
        SupervisorConfigV1::new(CanonicalHash::digest(b"shell"), 1_500, setting_revision(0))
    }

    fn open_stores(directory: &TestDirectory) -> (FileLaunchIntentStore, FileChildExitStore) {
        let intent = FileLaunchIntentStore::open(&directory.path)
            .unwrap_or_else(|error| panic!("intent store did not open: {error}"));
        let exit = FileChildExitStore::open(&directory.path)
            .unwrap_or_else(|error| panic!("child-exit store did not open: {error}"));
        (intent, exit)
    }

    fn initial_shell_state() -> ClientRuntimeStateV1 {
        ClientRuntimeStateV1::Shell {
            generation: generation(1),
            process_epoch: epoch(1),
            launch_blob_hash: None,
        }
    }

    fn publish_world_intent(
        intent_store: &mut FileLaunchIntentStore,
        world_id: WorldId,
    ) -> LaunchIntentV1 {
        publish_world_intent_at(intent_store, world_id, 1_000)
    }

    fn publish_world_intent_at(
        intent_store: &mut FileLaunchIntentStore,
        world_id: WorldId,
        issued_at_ms: u64,
    ) -> LaunchIntentV1 {
        let mut machine = ClientTransitionMachine::from_active(initial_shell_state())
            .unwrap_or_else(|error| panic!("active state was rejected: {error}"));
        let mut barrier = FixedBarrier(Ok(ShutdownBarrierReceiptV1::complete(
            setting_revision(9),
            None,
        )));
        machine
            .publish_handoff(
                LaunchIntentDraftV1 {
                    generation: generation(2),
                    attempt: LaunchAttempt::FIRST,
                    issued_at_ms,
                    expires_at_ms: issued_at_ms + 1_000,
                    target: LaunchTargetV1::World { world_id },
                    shell_lock_hash: CanonicalHash::digest(b"shell"),
                    world_lock_hash: Some(CanonicalHash::digest(b"world")),
                    world_open_plan_hash: Some(CanonicalHash::digest(b"plan")),
                    confirmed_setting_transaction_revision: setting_revision(9),
                },
                intent_store,
                &mut barrier,
                supervisor_identity(),
            )
            .unwrap_or_else(|error| panic!("handoff was rejected: {error}"))
            .intent()
            .cloned()
            .unwrap_or_else(|| panic!("handoff did not publish an intent"))
    }

    fn publish_report(exit_store: &mut FileChildExitStore, report: &ChildExitReportV1) {
        let bytes = report
            .canonical_bytes()
            .unwrap_or_else(|error| panic!("report did not encode: {error}"));
        exit_store
            .publish(&bytes)
            .unwrap_or_else(|error| panic!("child-exit report did not publish: {error}"));
    }

    fn shell_quit_report() -> ChildExitReportV1 {
        ChildExitReportV1::seal(ChildExitReportDraftV1 {
            child_generation: generation(1),
            process_epoch: epoch(1),
            role: ChildRoleV1::Shell,
            exit_kind: ChildExitKindV1::ShellQuit,
            intent_generation: None,
            intent_checksum: None,
            confirmed_setting_transaction_revision: setting_revision(0),
            last_written_world: None,
            last_durable_world: None,
            shell_lock_hash: CanonicalHash::digest(b"shell"),
            world_lock_hash: None,
            world_open_plan_hash: None,
            diagnostic_ref: None,
        })
        .unwrap_or_else(|error| panic!("shell-quit report was rejected: {error}"))
    }

    fn shell_handoff_report(intent: &LaunchIntentV1) -> ChildExitReportV1 {
        ChildExitReportV1::seal(ChildExitReportDraftV1 {
            child_generation: generation(1),
            process_epoch: epoch(1),
            role: ChildRoleV1::Shell,
            exit_kind: ChildExitKindV1::Handoff,
            intent_generation: Some(intent.generation()),
            intent_checksum: Some(intent.checksum()),
            confirmed_setting_transaction_revision: intent.confirmed_setting_transaction_revision(),
            last_written_world: None,
            last_durable_world: None,
            shell_lock_hash: CanonicalHash::digest(b"shell"),
            world_lock_hash: None,
            world_open_plan_hash: None,
            diagnostic_ref: None,
        })
        .unwrap_or_else(|error| panic!("handoff report was rejected: {error}"))
    }

    fn world_save_report(intent: &LaunchIntentV1, world_id: WorldId) -> ChildExitReportV1 {
        ChildExitReportV1::seal(ChildExitReportDraftV1 {
            child_generation: generation(2),
            process_epoch: epoch(2),
            role: ChildRoleV1::World { world_id },
            exit_kind: ChildExitKindV1::SaveAndQuit,
            intent_generation: Some(intent.generation()),
            intent_checksum: Some(intent.checksum()),
            confirmed_setting_transaction_revision: intent.confirmed_setting_transaction_revision(),
            last_written_world: Some(DurableWorldRevisionV1::new(world_id, WorldRevision::new(4))),
            last_durable_world: Some(DurableWorldRevisionV1::new(world_id, WorldRevision::new(4))),
            shell_lock_hash: CanonicalHash::digest(b"shell"),
            world_lock_hash: Some(CanonicalHash::digest(b"world")),
            world_open_plan_hash: Some(CanonicalHash::digest(b"plan")),
            diagnostic_ref: None,
        })
        .unwrap_or_else(|error| panic!("save-and-quit report was rejected: {error}"))
    }

    fn queue_exit_and_ack(
        process: &mut FakeProcess,
        intent: &LaunchIntentV1,
        process_epoch: ProcessEpoch,
    ) {
        process
            .exits
            .push_back(ChildObservationV1::Exited { exit_code: Some(0) });
        process
            .observations
            .push_back(BootObservationV1::Acknowledged(
                BootstrapAckV1::for_ready_intent(
                    intent,
                    crate::model::test_app_created_proof(process_epoch),
                ),
            ));
    }

    fn publish_shell_return(
        intent_store: &mut FileLaunchIntentStore,
        world_id: WorldId,
    ) -> LaunchIntentV1 {
        let launch_blob_hash = intent_store
            .read()
            .ok()
            .and_then(|slot| slot.blob_hash())
            .unwrap_or_else(|| panic!("consumed world slot has no hash"));
        let mut shell_machine = ClientTransitionMachine::from_active(ClientRuntimeStateV1::World {
            generation: generation(2),
            process_epoch: epoch(2),
            world_id,
            launch_blob_hash,
        })
        .unwrap_or_else(|error| panic!("world state was rejected: {error}"));
        let mut barrier = FixedBarrier(Ok(ShutdownBarrierReceiptV1::complete(
            setting_revision(9),
            Some(DurableWorldRevisionV1::new(world_id, WorldRevision::new(4))),
        )));
        shell_machine
            .publish_handoff(
                LaunchIntentDraftV1 {
                    generation: generation(3),
                    attempt: LaunchAttempt::FIRST,
                    issued_at_ms: 1_000,
                    expires_at_ms: 2_000,
                    target: LaunchTargetV1::Shell,
                    shell_lock_hash: CanonicalHash::digest(b"shell"),
                    world_lock_hash: None,
                    world_open_plan_hash: None,
                    confirmed_setting_transaction_revision: setting_revision(9),
                },
                intent_store,
                &mut barrier,
                supervisor_identity(),
            )
            .unwrap_or_else(|error| panic!("shell handoff was rejected: {error}"))
            .intent()
            .cloned()
            .unwrap_or_else(|| panic!("shell handoff did not publish"))
    }

    fn later_shell_quit(
        generation: LaunchGeneration,
        process_epoch: ProcessEpoch,
    ) -> ChildExitReportV1 {
        ChildExitReportV1::seal(ChildExitReportDraftV1 {
            child_generation: generation,
            process_epoch,
            role: ChildRoleV1::Shell,
            exit_kind: ChildExitKindV1::ShellQuit,
            intent_generation: None,
            intent_checksum: None,
            confirmed_setting_transaction_revision: setting_revision(9),
            last_written_world: None,
            last_durable_world: None,
            shell_lock_hash: CanonicalHash::digest(b"shell"),
            world_lock_hash: None,
            world_open_plan_hash: None,
            diagnostic_ref: None,
        })
        .unwrap_or_else(|error| panic!("later shell-quit report was rejected: {error}"))
    }

    fn boot_initial_shell(
        supervisor: &mut SupervisorMachine,
        intent_store: &mut FileLaunchIntentStore,
        exit_store: &mut FileChildExitStore,
        process: &mut FakeProcess,
    ) {
        process
            .observations
            .push_back(BootObservationV1::InitialShellAcknowledged {
                process_epoch: epoch(1),
            });
        let report = supervisor.advance(intent_store, exit_store, process);
        assert!(matches!(report.outcome(), SupervisorOutcomeV1::Running));
        assert!(matches!(
            supervisor.state(),
            SupervisorStateV1::Supervising {
                role: ChildRoleV1::Shell,
                ..
            }
        ));
    }

    #[test]
    fn active_interaction_survives_multiple_observation_windows() {
        let directory = TestDirectory::create();
        let (mut intents, mut exits) = open_stores(&directory);
        let mut supervisor = SupervisorMachine::new(config());
        let mut process = FakeProcess::default();
        boot_initial_shell(&mut supervisor, &mut intents, &mut exits, &mut process);
        for _ in 0..3 {
            process.exits.push_back(ChildObservationV1::Running);
            let report = supervisor.advance(&mut intents, &mut exits, &mut process);
            assert!(matches!(report.outcome(), SupervisorOutcomeV1::Running));
            assert!(report.failures().is_empty());
        }
        assert_eq!(process.spawn_count, 1);
        assert_eq!(process.terminated, 0);
    }

    #[test]
    fn handoff_after_long_interaction_uses_the_clock_after_child_exit() {
        let directory = TestDirectory::create();
        let (mut intents, mut exits) = open_stores(&directory);
        let mut supervisor = SupervisorMachine::new(config());
        let mut process = FakeProcess {
            now_ms: Some(1_500),
            ..FakeProcess::default()
        };
        boot_initial_shell(&mut supervisor, &mut intents, &mut exits, &mut process);
        let intent = publish_world_intent_at(&mut intents, WorldId::new_v4(), 100_000);
        publish_report(&mut exits, &shell_handoff_report(&intent));
        process.exit_time_ms = Some(100_100);
        queue_exit_and_ack(&mut process, &intent, epoch(2));
        let report = supervisor.advance(&mut intents, &mut exits, &mut process);
        assert!(report.failures().is_empty());
        assert!(matches!(
            supervisor.state(),
            SupervisorStateV1::Supervising {
                role: ChildRoleV1::World { .. },
                ..
            }
        ));
    }

    #[test]
    fn shell_quit_without_intent_exits_the_product() {
        let directory = TestDirectory::create();
        let (mut intent_store, mut exit_store) = open_stores(&directory);
        let mut supervisor = SupervisorMachine::new(config());
        let mut process = FakeProcess::default();
        boot_initial_shell(
            &mut supervisor,
            &mut intent_store,
            &mut exit_store,
            &mut process,
        );
        publish_report(&mut exit_store, &shell_quit_report());
        process
            .exits
            .push_back(ChildObservationV1::Exited { exit_code: Some(0) });
        let report = supervisor.advance(&mut intent_store, &mut exit_store, &mut process);
        assert_eq!(
            report.outcome(),
            SupervisorOutcomeV1::ProductExited {
                last_role: ChildRoleV1::Shell,
                exit_kind: ChildExitKindV1::ShellQuit,
            }
        );
        assert_eq!(process.spawn_count, 1);
        assert!(!report.needs_recovery());
    }

    #[test]
    fn home_world_save_and_quit_home_completes_without_replay() {
        let directory = TestDirectory::create();
        let (mut intent_store, mut exit_store) = open_stores(&directory);
        let mut supervisor = SupervisorMachine::new(SupervisorConfigV1::new(
            CanonicalHash::digest(b"shell"),
            1_500,
            setting_revision(9),
        ));
        let mut process = FakeProcess::default();
        boot_initial_shell(
            &mut supervisor,
            &mut intent_store,
            &mut exit_store,
            &mut process,
        );

        let world_id = WorldId::new_v4();
        let world_intent = publish_world_intent(&mut intent_store, world_id);
        publish_report(&mut exit_store, &shell_handoff_report(&world_intent));
        queue_exit_and_ack(&mut process, &world_intent, epoch(2));
        let report = supervisor.advance(&mut intent_store, &mut exit_store, &mut process);
        assert!(matches!(
            supervisor.state(),
            SupervisorStateV1::Supervising {
                role: ChildRoleV1::World { .. },
                ..
            }
        ));
        assert!(matches!(report.outcome(), SupervisorOutcomeV1::Running));

        let shell_intent = publish_shell_return(&mut intent_store, world_id);
        publish_report(&mut exit_store, &world_save_report(&shell_intent, world_id));
        queue_exit_and_ack(&mut process, &shell_intent, epoch(3));
        let report = supervisor.advance(&mut intent_store, &mut exit_store, &mut process);
        assert!(matches!(
            supervisor.state(),
            SupervisorStateV1::Supervising {
                role: ChildRoleV1::Shell,
                ..
            }
        ));
        assert!(!report.needs_recovery());
        assert_eq!(
            report.last_durable_world(),
            Some(DurableWorldRevisionV1::new(world_id, WorldRevision::new(4)))
        );

        publish_report(&mut exit_store, &later_shell_quit(generation(3), epoch(3)));
        process
            .exits
            .push_back(ChildObservationV1::Exited { exit_code: Some(0) });
        let report = supervisor.advance(&mut intent_store, &mut exit_store, &mut process);
        assert!(matches!(
            report.outcome(),
            SupervisorOutcomeV1::ProductExited {
                exit_kind: ChildExitKindV1::ShellQuit,
                ..
            }
        ));
        assert_eq!(process.spawn_count, 3);
    }

    #[test]
    fn missing_report_with_pending_world_intent_does_not_start_the_world() {
        let directory = TestDirectory::create();
        let (mut intent_store, mut exit_store) = open_stores(&directory);
        let mut supervisor = SupervisorMachine::new(SupervisorConfigV1::new(
            CanonicalHash::digest(b"shell"),
            1_500,
            setting_revision(9),
        ));
        let mut process = FakeProcess {
            acknowledge_recovery: true,
            ..FakeProcess::default()
        };
        boot_initial_shell(
            &mut supervisor,
            &mut intent_store,
            &mut exit_store,
            &mut process,
        );
        let world_id = WorldId::new_v4();
        let _intent = publish_world_intent(&mut intent_store, world_id);
        process
            .exits
            .push_back(ChildObservationV1::Exited { exit_code: Some(1) });
        let report = supervisor.advance(&mut intent_store, &mut exit_store, &mut process);
        assert_eq!(
            report.failures()[0].code(),
            LaunchFailureCodeV1::ChildReportMissing
        );
        assert!(matches!(
            supervisor.state(),
            SupervisorStateV1::Supervising {
                role: ChildRoleV1::Recovery,
                ..
            }
        ));
        assert_eq!(
            intent_store.read().ok().and_then(|slot| slot.disposition()),
            Some(SlotDisposition::RecoveryClaimed)
        );
    }

    #[test]
    fn written_only_and_timeout_mark_needs_recovery() {
        let directory = TestDirectory::create();
        let (mut intent_store, mut exit_store) = open_stores(&directory);
        let mut supervisor = SupervisorMachine::new(config());
        let mut process = FakeProcess {
            acknowledge_recovery: true,
            ..FakeProcess::default()
        };
        boot_initial_shell(
            &mut supervisor,
            &mut intent_store,
            &mut exit_store,
            &mut process,
        );
        process.exits.push_back(ChildObservationV1::TimedOut);
        let report = supervisor.advance(&mut intent_store, &mut exit_store, &mut process);
        assert!(matches!(
            supervisor.state(),
            SupervisorStateV1::Supervising {
                role: ChildRoleV1::Recovery,
                ..
            } | SupervisorStateV1::Halted
        ));
        assert_eq!(
            report.failures()[0].code(),
            LaunchFailureCodeV1::ShutdownTimedOut
        );
        assert_eq!(process.terminated, 1);
    }

    #[test]
    fn lease_conflict_does_not_kill_or_spawn_a_second_window() {
        let directory = TestDirectory::create();
        let (mut intent_store, mut exit_store) = open_stores(&directory);
        let mut supervisor = SupervisorMachine::new(SupervisorConfigV1::new(
            CanonicalHash::digest(b"shell"),
            1_500,
            setting_revision(9),
        ));
        let world_id = WorldId::new_v4();
        let intent = publish_world_intent(&mut intent_store, world_id);
        let hash = intent_store
            .read()
            .ok()
            .and_then(|slot| slot.blob_hash())
            .unwrap_or_else(|| panic!("pending intent has no hash"));
        assert!(intent_store.claim(hash).is_ok());
        let mut process = FakeProcess {
            reconciliations: VecDeque::from([PriorChildStatusV1::Unknown]),
            ..FakeProcess::default()
        };
        let report = supervisor.advance(&mut intent_store, &mut exit_store, &mut process);
        assert!(matches!(
            report.outcome(),
            SupervisorOutcomeV1::Halted { .. }
        ));
        assert_eq!(process.spawn_count, 0);
        assert_eq!(process.terminated, 0);
        assert_eq!(
            report.failures()[0].code(),
            LaunchFailureCodeV1::AttemptAlreadyClaimed
        );
        let _ = intent;
    }

    #[test]
    fn running_consumed_child_is_reattached_without_a_second_spawn() {
        let directory = TestDirectory::create();
        let (mut intent_store, mut exit_store) = open_stores(&directory);
        let mut supervisor = SupervisorMachine::new(SupervisorConfigV1::new(
            CanonicalHash::digest(b"shell"),
            1_500,
            setting_revision(9),
        ));
        let world_id = WorldId::new_v4();
        let intent = publish_world_intent(&mut intent_store, world_id);
        let hash = intent_store
            .read()
            .ok()
            .and_then(|slot| slot.blob_hash())
            .unwrap_or_else(|| panic!("pending intent has no hash"));
        assert!(intent_store.claim(hash).is_ok());
        assert!(intent_store.consume(hash).is_ok());
        let child = SpawnedProcess::new(11)
            .unwrap_or_else(|error| panic!("test process handle was zero: {error}"));
        let mut process = FakeProcess {
            reconciliations: VecDeque::from([PriorChildStatusV1::Running {
                process: child,
                process_epoch: epoch(2),
            }]),
            ..FakeProcess::default()
        };
        let report = supervisor.advance(&mut intent_store, &mut exit_store, &mut process);
        assert!(matches!(report.outcome(), SupervisorOutcomeV1::Running));
        assert_eq!(process.spawn_count, 0);
        assert!(matches!(
            supervisor.state(),
            SupervisorStateV1::Supervising {
                role: ChildRoleV1::World { .. },
                child: attached,
                ..
            } if attached == child
        ));
        let _ = intent;
    }
}
