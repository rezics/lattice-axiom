//! Stable process-transition contracts for the first-demo client launcher.
//!
//! The crate is deliberately independent of Bevy. It validates and persists a
//! bounded [`LaunchIntentV1`], models shell/world/recovery transitions, and
//! issues a single-use process lease that a future Bevy host must consume
//! before creating its one fresh client `DefaultPlugins` application. The
//! non-Bevy [`SupervisorMachine`] consumes one-shot child-exit reports and
//! intents, launches at most one replacement child, and emits deterministic
//! semantic reports without creating a window or `App`.
//!
//! Ordinary launch reopens and fully verifies `latticeaxiom.lock` before a
//! host may construct [`latticeaxiom_compose::RuntimeImage`] or load native
//! modules. Client and headless hosts share that reopened lock and must not
//! re-resolve. This crate still does not create a Bevy `App`.

mod boot;
mod child_exit;
mod error;
mod filesystem;
mod machine;
mod model;
mod process;
mod store;
mod supervisor;

pub use boot::{
    HostBuildReceipts, ProductLockBootError, ReopenedFinalLockV1, VerifiedArtifactObjects,
};
pub use child_exit::{
    CHILD_EXIT_SCHEMA_VERSION, ChildExitKindV1, ChildExitReportDraftV1, ChildExitReportV1,
    ChildRoleV1,
};
pub use error::{
    ChildExitError, IntentStoreError, LaunchIntentError, LaunchModelError, StoreOperation,
};
pub use filesystem::{FileChildExitStore, FileLaunchIntentStore};
pub use machine::{
    BootstrapMachine, ClientTransitionMachine, TransitionBarrier, TransitionMachineError,
    TransitionPublishReport,
};
pub use model::{
    BOOTSTRAP_ACK_SCHEMA_VERSION, BootObservationV1, BootstrapAckV1, BootstrapReportV1,
    BootstrapSafeStateV1, ClientAppLeaseError, ClientRuntimeStateV1, DurableWorldRevisionV1,
    FailureDispositionV1, FreshClientAppLeaseProof, FreshClientAppLeaseToken,
    LAUNCH_INTENT_SCHEMA_VERSION, LAUNCH_RECEIPT_SCHEMA_VERSION, LaunchAttempt,
    LaunchFailureCodeV1, LaunchFailureDetailV1, LaunchFailureReceiptV1, LaunchGeneration,
    LaunchIntentDraftV1, LaunchIntentV1, LaunchPhaseV1, LaunchTargetV1, ProcessEpoch,
    ProcessLaunchRequestV1, ProcessSupervisorIdentityV1, RECOVERY_ACK_SCHEMA_VERSION,
    RECOVERY_REQUEST_SCHEMA_VERSION, RecoveryBootstrapAckV1, RecoveryLaunchRequestV1,
    RecoveryReasonV1, RecoveryRequestError, SettingTransactionRevision, ShutdownBarrierFailureV1,
    ShutdownBarrierReceiptV1, ShutdownBarrierStageV1, TransitionOutcomeV1,
    TransitionValidationPolicy, WorldRevision, claim_fresh_client_app_lease,
};
pub use process::{
    ChildObservationV1, PriorChildStatusV1, ProcessControl, RecoveryChildStatusV1, SpawnFailureV1,
    SpawnHandleError, SpawnedProcess, TerminationFailureV1,
};
pub use store::{
    AtomicChildExitStore, AtomicLaunchIntentStore, ChildExitDisposition, ChildExitSlot, IntentSlot,
    MutationDurability, PublishDisposition, RecoveryClaimOutcome, SlotDisposition,
    TerminalPredecessor,
};
pub use supervisor::{
    SUPERVISOR_REPORT_SCHEMA_VERSION, SupervisorConfigV1, SupervisorHopV1, SupervisorMachine,
    SupervisorOutcomeV1, SupervisorReportV1, SupervisorStateV1,
};

/// Maximum accepted canonical launch-intent envelope size.
pub const MAX_LAUNCH_INTENT_BYTES: usize = 16 * 1024;

/// Maximum lifetime of a launch intent in milliseconds.
pub const MAX_LAUNCH_INTENT_LIFETIME_MS: u64 = 5 * 60 * 1_000;

/// Maximum future clock skew tolerated when validating an intent.
pub const MAX_LAUNCH_CLOCK_SKEW_MS: u64 = 5_000;

/// Maximum wait for a child to acknowledge its safe bootstrap state.
pub const MAX_BOOTSTRAP_ACK_WAIT_MS: u64 = 30_000;

/// Maximum wait for a supervised child to exit after the product requests shutdown.
pub const MAX_CHILD_SHUTDOWN_WAIT_MS: u64 = 30_000;

/// Maximum replacement-process hops in one supervisor product loop.
pub const MAX_SUPERVISOR_HOPS: usize = 8;

/// Maximum consecutive child, spawn, or recovery failures before the product halts.
pub const MAX_CONSECUTIVE_CHILD_FAILURES: u8 = 2;
