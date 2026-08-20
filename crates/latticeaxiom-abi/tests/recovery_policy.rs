//! Recovery-policy conformance tests.

use latticeaxiom_abi::{
    CallbackFault, CallbackPolicy, CallbackRecoveryInput, FailureScope, RecoveryAction,
    RecoveryPolicyError, StagedOutputState, WriteCommitMode, decide_recovery,
};

const STAGING_VALID: StagedOutputState = StagedOutputState {
    writable_column_bytes: 64,
    command_count: 2,
    message_count: 1,
    validation_passed: true,
};

#[test]
fn recoverable_domain_status_discards_all_staging() {
    let policy = CallbackPolicy::new(
        WriteCommitMode::Staged,
        FailureScope::CallbackRecoverable,
        true,
    );
    assert!(policy.is_ok());
    let Some(policy) = policy.ok() else {
        return;
    };
    assert_eq!(
        decide_recovery(CallbackRecoveryInput {
            policy,
            staging: STAGING_VALID,
            fault: CallbackFault::DeclaredDomainStatus(7),
            deadline_overrun_is_fatal: true,
        }),
        RecoveryAction::DiscardStagingAndContinue
    );
}

#[test]
fn direct_failure_invalidates_authoritative_tick() {
    let policy = CallbackPolicy::new(WriteCommitMode::Direct, FailureScope::InstanceFatal, true);
    assert!(policy.is_ok());
    let Some(policy) = policy.ok() else {
        return;
    };
    assert_eq!(
        decide_recovery(CallbackRecoveryInput {
            policy,
            staging: STAGING_VALID,
            fault: CallbackFault::InvalidOutput,
            deadline_overrun_is_fatal: true,
        }),
        RecoveryAction::FailInstanceAndInvalidateTick
    );
}

#[test]
fn invalid_staged_success_fails_instance_and_process_fault_terminates() {
    let policy = CallbackPolicy::new(WriteCommitMode::Staged, FailureScope::InstanceFatal, true);
    assert!(policy.is_ok());
    let Some(policy) = policy.ok() else {
        return;
    };
    let invalid = StagedOutputState {
        validation_passed: false,
        ..STAGING_VALID
    };
    assert_eq!(
        decide_recovery(CallbackRecoveryInput {
            policy,
            staging: invalid,
            fault: CallbackFault::Success,
            deadline_overrun_is_fatal: true,
        }),
        RecoveryAction::FailInstanceAndDiscardStaging
    );
    assert_eq!(
        decide_recovery(CallbackRecoveryInput {
            policy,
            staging: STAGING_VALID,
            fault: CallbackFault::ProcessFault,
            deadline_overrun_is_fatal: true,
        }),
        RecoveryAction::TerminateProcess
    );
}

#[test]
fn direct_recoverable_policy_is_rejected() {
    assert_eq!(
        CallbackPolicy::new(
            WriteCommitMode::Direct,
            FailureScope::CallbackRecoverable,
            true
        ),
        Err(RecoveryPolicyError::RecoverableDirectWrite)
    );
}
#[test]
fn soft_deadline_policy_can_report_or_fail_the_instance() {
    let policy = CallbackPolicy::new(WriteCommitMode::Staged, FailureScope::InstanceFatal, true);
    assert!(policy.is_ok());
    let Some(policy) = policy.ok() else {
        return;
    };
    let report = CallbackRecoveryInput {
        policy,
        staging: STAGING_VALID,
        fault: CallbackFault::DeadlineOverrun,
        deadline_overrun_is_fatal: false,
    };
    assert_eq!(
        decide_recovery(report),
        RecoveryAction::CommitValidatedStagingAndReportOverrun
    );
    assert_eq!(
        decide_recovery(CallbackRecoveryInput {
            deadline_overrun_is_fatal: true,
            ..report
        }),
        RecoveryAction::FailInstanceAndDiscardStaging
    );
}
