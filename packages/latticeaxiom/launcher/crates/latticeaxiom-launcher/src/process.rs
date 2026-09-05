//! External process-control boundary used by the pure launcher state machine.

use serde::{Deserialize, Deserializer, Serialize, de};
use thiserror::Error;

use crate::{
    BootObservationV1, LaunchIntentV1, ProcessEpoch, ProcessLaunchRequestV1,
    ProcessSupervisorIdentityV1, RecoveryLaunchRequestV1,
};

/// Opaque non-zero identifier for a process owned by a [`ProcessControl`].
#[repr(transparent)]
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct SpawnedProcess(u64);

impl SpawnedProcess {
    /// Creates a non-zero process-control handle.
    ///
    /// # Errors
    ///
    /// Returns [`SpawnHandleError`] when `value` is zero.
    pub const fn new(value: u64) -> Result<Self, SpawnHandleError> {
        if value == 0 {
            Err(SpawnHandleError)
        } else {
            Ok(Self(value))
        }
    }

    /// Returns the adapter-local numeric handle.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

impl<'de> Deserialize<'de> for SpawnedProcess {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Self::new(u64::deserialize(deserializer)?).map_err(de::Error::custom)
    }
}

/// Rejection of a zero process-control handle.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
#[error("spawned process handle must be non-zero")]
pub struct SpawnHandleError;

/// Stable process creation failure category.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum SpawnFailureV1 {
    /// Executable or artifact selection was rejected.
    ExecutableRejected,
    /// The operating system could not allocate the process.
    ResourceUnavailable,
    /// Local policy refused the requested role.
    PolicyDenied,
}

/// Stable best-effort process termination failure category.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum TerminationFailureV1 {
    /// The adapter no longer knew the process.
    UnknownProcess,
    /// The platform refused termination.
    PlatformRejected,
}

/// Supervisor reconciliation of a child from a prior launcher instance.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case", tag = "status")]
pub enum PriorChildStatusV1 {
    /// The exact intent child is still supervised and may remain active.
    Running {
        /// Adapter handle for the still-owned child.
        process: SpawnedProcess,
        /// Process epoch bound to the child's single-client-App lease.
        process_epoch: ProcessEpoch,
    },
    /// The supervisor proves the exact intent child exited.
    Exited {
        /// Platform exit code when one was supplied.
        exit_code: Option<i32>,
    },
    /// The adapter cannot prove whether the exact intent child still exists.
    Unknown,
}

/// Result observed while waiting for a supervised child to exit.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ChildObservationV1 {
    /// The adapter proved the exact child exited.
    Exited {
        /// Platform exit code when one was supplied.
        exit_code: Option<i32>,
    },
    /// The bounded shutdown wait elapsed while the child still existed.
    TimedOut,
    /// The adapter cannot prove whether the exact child still exists.
    Unknown,
}

/// Result of atomically acquiring the one supervised recovery child.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RecoveryChildStatusV1 {
    /// The exact request owns a running child and may await its durable ack.
    Acquired(SpawnedProcess),
    /// The exact request previously owned a child that has exited.
    Exited {
        /// Platform exit code when one was supplied.
        exit_code: Option<i32>,
    },
    /// The supervisor cannot prove whether the exact child exists.
    Unknown,
}

/// Process creation and bounded bootstrap observation supplied by an external
/// launcher adapter.
///
/// The trait does not create a task runtime. Implementations may block only for
/// the explicit `deadline_ms` passed to [`Self::await_bootstrap`]. Tests use a
/// deterministic fake; production code can adapt the operating system's
/// existing process supervisor.
pub trait ProcessControl {
    /// Returns the stable identity of the durable process supervisor.
    ///
    /// The identity must remain stable across launcher restarts for the same
    /// private launch slot and must change when supervisor ownership changes.
    fn supervisor_identity(&self) -> ProcessSupervisorIdentityV1;

    /// Reconciles an exact claimed or consumed intent after launcher restart.
    ///
    /// Implementations must use durable supervisor ownership keyed by the
    /// intent checksum, not process-name or PID guessing. Recovery is allowed
    /// only after [`PriorChildStatusV1::Exited`].
    fn reconcile_prior_child(&mut self, intent: &LaunchIntentV1) -> PriorChildStatusV1;

    /// Creates one normal target child for the exact claimed intent.
    ///
    /// # Errors
    ///
    /// Returns a stable [`SpawnFailureV1`] without leaking platform strings
    /// into deterministic launch receipts.
    fn spawn(&mut self, request: &ProcessLaunchRequestV1)
    -> Result<SpawnedProcess, SpawnFailureV1>;

    /// Atomically acquires the one recovery child keyed by the exact request.
    ///
    /// Implementations must durably enforce at-most-once physical creation by
    /// `(supervisor identity, request checksum)`. Repeated calls after launcher
    /// crash return the same running handle or a terminal/unknown status; they
    /// must never create a second child for that request.
    ///
    /// # Errors
    ///
    /// Returns [`SpawnFailureV1`] only when the first durable acquire could not
    /// create a child. An already-recorded exited attempt returns
    /// [`RecoveryChildStatusV1::Exited`].
    fn acquire_recovery(
        &mut self,
        request: &RecoveryLaunchRequestV1,
    ) -> Result<RecoveryChildStatusV1, SpawnFailureV1>;

    /// Waits until the exact supervised child acknowledges safe bootstrap or the deadline.
    ///
    /// The acknowledgement channel must be authenticated and integrity-bound
    /// to `process` and `request`; adapters may not accept an epoch or receipt
    /// delivered by another process. Repeated observation after launcher crash
    /// must return the same durable acknowledgement when one already exists.
    fn await_bootstrap(
        &mut self,
        process: SpawnedProcess,
        request: &ProcessLaunchRequestV1,
        deadline_ms: u64,
    ) -> BootObservationV1;

    /// Terminates a rejected child and proves it exited before recovery.
    ///
    /// # Errors
    ///
    /// Returns a stable [`TerminationFailureV1`] when the adapter cannot prove
    /// termination. The bootstrap machine then fails closed and does not spawn
    /// recovery beside a possibly live target process.
    fn terminate(&mut self, process: SpawnedProcess) -> Result<(), TerminationFailureV1>;

    /// Waits until the exact supervised child exits or the deadline elapses.
    ///
    /// Implementations must not kill an unknown owner. [`ChildObservationV1::Unknown`]
    /// suppresses another window App. A timeout is not proof of crash.
    fn await_exit(&mut self, process: SpawnedProcess, deadline_ms: u64) -> ChildObservationV1;
}
