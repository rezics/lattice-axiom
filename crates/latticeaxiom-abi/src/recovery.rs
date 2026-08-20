//! Pure staged-write and native-fault recovery decisions.
//!
//! This module models host policy only. It does not claim that access
//! violations, illegal instructions, or indefinitely blocked native calls can
//! be recovered inside the process.

use thiserror::Error;

use crate::{LAX_STATUS_OK, LaxStatus};

/// How authoritative writable columns become visible.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WriteCommitMode {
    /// Foreign writes target staging storage and copy back only after validation.
    Staged,
    /// Foreign writes modify authoritative storage in place.
    Direct,
}

/// The allowed failure scope declared by a system manifest.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FailureScope {
    /// A declared domain rejection may discard staged output and continue.
    CallbackRecoverable,
    /// Any failure marks the engine instance failed.
    InstanceFatal,
}

/// Validated callback write/failure policy.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CallbackPolicy {
    write_commit_mode: WriteCommitMode,
    failure_scope: FailureScope,
    authoritative_write: bool,
}

impl CallbackPolicy {
    /// Validates a manifest callback policy.
    ///
    /// # Errors
    ///
    /// Direct writes cannot be callback-recoverable. A caller must instead use
    /// `Direct + InstanceFatal`, or stage the writes.
    pub fn new(
        write_commit_mode: WriteCommitMode,
        failure_scope: FailureScope,
        authoritative_write: bool,
    ) -> Result<Self, RecoveryPolicyError> {
        if write_commit_mode == WriteCommitMode::Direct
            && failure_scope == FailureScope::CallbackRecoverable
        {
            return Err(RecoveryPolicyError::RecoverableDirectWrite);
        }
        Ok(Self {
            write_commit_mode,
            failure_scope,
            authoritative_write,
        })
    }

    /// Returns the write commit mode.
    #[must_use]
    pub const fn write_commit_mode(self) -> WriteCommitMode {
        self.write_commit_mode
    }

    /// Returns the declared failure scope.
    #[must_use]
    pub const fn failure_scope(self) -> FailureScope {
        self.failure_scope
    }

    /// Returns whether this callback can alter authoritative state.
    #[must_use]
    pub const fn authoritative_write(self) -> bool {
        self.authoritative_write
    }
}

/// Host-observed state of all callback-local staged output.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StagedOutputState {
    /// Writable column bytes held outside authoritative storage.
    pub writable_column_bytes: u64,
    /// Staged structural command count.
    pub command_count: u64,
    /// Staged message count.
    pub message_count: u64,
    /// Whether column, command, and message validation completed successfully.
    pub validation_passed: bool,
}

impl StagedOutputState {
    /// Returns whether any callback-local output exists.
    #[must_use]
    pub const fn has_output(self) -> bool {
        self.writable_column_bytes != 0 || self.command_count != 0 || self.message_count != 0
    }
}

/// A callback outcome or native fault classified at the SDK/host boundary.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CallbackFault {
    /// Callback returned the successful status.
    Success,
    /// Callback returned a declared recoverable domain status.
    DeclaredDomainStatus(LaxStatus),
    /// SDK boundary caught a Rust panic or C++ exception.
    PanicOrException,
    /// Host rejected callback output after return.
    InvalidOutput,
    /// ABI ownership, permission, layout, or lifecycle violation.
    ContractViolation,
    /// Callback returned after its soft monotonic deadline.
    DeadlineOverrun,
    /// Access violation, illegal instruction, or indefinitely blocked native code.
    ProcessFault,
}

/// Complete input to the deterministic recovery decision.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CallbackRecoveryInput {
    /// Validated callback policy.
    pub policy: CallbackPolicy,
    /// Callback-local staging state.
    pub staging: StagedOutputState,
    /// Classified callback outcome.
    pub fault: CallbackFault,
    /// Whether a returned soft-deadline overrun is instance-fatal.
    pub deadline_overrun_is_fatal: bool,
}

/// Required host action after one callback boundary.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RecoveryAction {
    /// Atomically copy staged columns and apply validated commands/messages.
    CommitValidatedStaging,
    /// Commit validated staging and emit an overrun diagnostic.
    CommitValidatedStagingAndReportOverrun,
    /// Discard every staged output and continue later callbacks.
    DiscardStagingAndContinue,
    /// Fail the instance, discard staging, and exit the writable world safely.
    FailInstanceAndDiscardStaging,
    /// Fail the instance and prohibit durable capture of the partially written tick.
    FailInstanceAndInvalidateTick,
    /// Terminate/restart the process; in-process recovery is not promised.
    TerminateProcess,
}

/// Errors in callback policy declarations.
#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum RecoveryPolicyError {
    /// Direct authoritative writes cannot be rolled back at callback scope.
    #[error("direct write mode requires instance-fatal failure scope")]
    RecoverableDirectWrite,
}

/// Computes the mandatory staged-output and failure action.
///
/// A success commits only after validation. Declared domain rejection may
/// continue only for a validated callback-recoverable staged policy. Panics,
/// invalid output, and contract violations always fail the instance. A direct
/// failure invalidates the authoritative tick. Process faults require process
/// termination because the ABI is trusted in-process code, not a sandbox.
#[must_use]
pub fn decide_recovery(input: CallbackRecoveryInput) -> RecoveryAction {
    if matches!(input.fault, CallbackFault::ProcessFault) {
        return RecoveryAction::TerminateProcess;
    }
    if matches!(input.fault, CallbackFault::DeadlineOverrun) && !input.deadline_overrun_is_fatal {
        if input.staging.validation_passed {
            return RecoveryAction::CommitValidatedStagingAndReportOverrun;
        }
        return failure_action(input.policy);
    }
    if matches!(input.fault, CallbackFault::Success) {
        if input.staging.validation_passed {
            return RecoveryAction::CommitValidatedStaging;
        }
        return failure_action(input.policy);
    }
    if let CallbackFault::DeclaredDomainStatus(status) = input.fault
        && status != LAX_STATUS_OK
        && input.policy.failure_scope == FailureScope::CallbackRecoverable
        && input.policy.write_commit_mode == WriteCommitMode::Staged
    {
        return RecoveryAction::DiscardStagingAndContinue;
    }
    failure_action(input.policy)
}

fn failure_action(policy: CallbackPolicy) -> RecoveryAction {
    if policy.write_commit_mode == WriteCommitMode::Direct && policy.authoritative_write {
        RecoveryAction::FailInstanceAndInvalidateTick
    } else {
        RecoveryAction::FailInstanceAndDiscardStaging
    }
}
