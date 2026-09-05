use std::cmp::max;

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Number of bytes in the binary GiB used by ADR 0027 thresholds.
pub const GIB: u64 = 1024 * 1024 * 1024;
/// Fixed reserve included in every required-headroom calculation.
pub const REQUIRED_HEADROOM_RESERVE_BYTES: u64 = GIB;

/// Inputs that bound bytes needed to finish or safely abandon an operation.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HeadroomInputs {
    /// Bounded bytes already queued or in flight.
    pub bounded_in_flight_bytes: u64,
    /// Worst-case WAL plus orderly shutdown drain.
    pub worst_case_wal_shutdown_bytes: u64,
    /// Estimated bytes for the current checkpoint, clone, or migration.
    pub estimated_operation_bytes: u64,
}

impl HeadroomInputs {
    /// Computes the ADR 0027 required headroom with checked arithmetic.
    ///
    /// # Errors
    ///
    /// Returns [`DiskPolicyError::ArithmeticOverflow`] if the bounded values
    /// cannot be represented by `u64`.
    pub fn required_headroom(self) -> Result<u64, DiskPolicyError> {
        REQUIRED_HEADROOM_RESERVE_BYTES
            .checked_add(self.bounded_in_flight_bytes)
            .and_then(|value| value.checked_add(self.worst_case_wal_shutdown_bytes))
            .and_then(|value| value.checked_add(self.estimated_operation_bytes))
            .ok_or(DiskPolicyError::ArithmeticOverflow)
    }
}

/// One filesystem capacity observation.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DiskSample {
    /// Usable free bytes after host quota semantics.
    pub usable_free_bytes: u64,
    /// Total filesystem or quota capacity.
    pub capacity_bytes: u64,
}

/// Computed watermarks for one sample and operation estimate.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DiskThresholds {
    /// Required bytes used by both watermarks.
    pub required_headroom: u64,
    /// Below this value the host enters `Warning`.
    pub warning_below: u64,
    /// Below this value the host pauses authoritative mutation.
    pub mutation_paused_below: u64,
}

impl DiskThresholds {
    /// Computes the bootstrap thresholds from ADR 0027 `WORLD-17`.
    ///
    /// # Errors
    ///
    /// Returns [`DiskPolicyError::ArithmeticOverflow`] when bounded input
    /// arithmetic overflows.
    pub fn compute(capacity_bytes: u64, inputs: HeadroomInputs) -> Result<Self, DiskPolicyError> {
        let required_headroom = inputs.required_headroom()?;
        let twice_required = required_headroom
            .checked_mul(2)
            .ok_or(DiskPolicyError::ArithmeticOverflow)?;
        Ok(Self {
            required_headroom,
            warning_below: max(max(5 * GIB, capacity_bytes / 10), twice_required),
            mutation_paused_below: max(max(2 * GIB, capacity_bytes / 20), required_headroom),
        })
    }
}

/// Storage failure that immediately pauses authoritative mutation.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum StorageFailure {
    /// Host reported no space left on device.
    NoSpace,
    /// A device or user quota rejected a write.
    Quota,
    /// WAL creation, append, or synchronization failed.
    Wal,
    /// Checkpoint creation or verification failed.
    Checkpoint,
}

/// Ability to make already-dirty authoritative state durable.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum DirtyDrainState {
    /// There is no dirty authoritative state.
    Clean,
    /// Dirty state exists and the host is still draining it.
    Drainable,
    /// Dirty state cannot become durable, but a safe read-only recovery path exists.
    FailedReadOnlyAvailable,
    /// Dirty state cannot become durable and shutdown recovery is required.
    FailedShutdownRequired,
}

/// Stateful storage-pressure level.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum StoragePressureState {
    /// All admitted operation classes may proceed.
    #[default]
    Normal,
    /// Expensive background work is suspended and a persistent warning is shown.
    Warning,
    /// New authoritative mutation is rejected before command application.
    MutationPaused,
    /// Only read-only recovery operations remain safe.
    RecoverableReadOnly,
    /// The host must complete a bounded recovery shutdown.
    ShutdownRequired,
}

/// Operation class evaluated by low-disk admission.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum StorageOperation {
    /// Break, place, craft, or any other new authoritative command.
    AuthoritativeMutation,
    /// Drain already-dirty authoritative state toward durability.
    DurabilityDrain,
    /// Read-only catalog, inspect, or export work.
    ReadOnly,
    /// Remote chunk prefetch.
    RemotePrefetch,
    /// Automatic checkpoint rotation.
    AutomaticCheckpoint,
    /// Nonessential generation work.
    NonessentialWorldgen,
    /// Staged migration or clone requiring a checkpoint and reserved headroom.
    MigrationOrClone,
}

/// One storage-pressure decision and its exact thresholds.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DiskAdmission {
    /// Resulting state after hysteresis and failure escalation.
    pub state: StoragePressureState,
    /// Thresholds used for this decision.
    pub thresholds: DiskThresholds,
    /// Observed usable free bytes.
    pub usable_free_bytes: u64,
}

impl DiskAdmission {
    /// Returns whether an operation may begin under this decision.
    #[must_use]
    pub const fn allows(self, operation: StorageOperation) -> bool {
        match self.state {
            StoragePressureState::Normal => true,
            StoragePressureState::Warning => matches!(
                operation,
                StorageOperation::AuthoritativeMutation
                    | StorageOperation::DurabilityDrain
                    | StorageOperation::ReadOnly
            ),
            StoragePressureState::MutationPaused => matches!(
                operation,
                StorageOperation::DurabilityDrain | StorageOperation::ReadOnly
            ),
            StoragePressureState::RecoverableReadOnly => {
                matches!(operation, StorageOperation::ReadOnly)
            }
            StoragePressureState::ShutdownRequired => false,
        }
    }

    /// Returns whether a migration plan may truthfully be advertised as
    /// executable now.
    #[must_use]
    pub const fn allows_migration(self) -> bool {
        self.allows(StorageOperation::MigrationOrClone)
    }
}

/// Hysteretic evaluator for periodic filesystem samples.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct LowDiskMonitor {
    state: StoragePressureState,
    successful_resume_polls: u8,
}

impl LowDiskMonitor {
    /// Creates a monitor in the normal state.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            state: StoragePressureState::Normal,
            successful_resume_polls: 0,
        }
    }

    /// Evaluates one poll or immediate write failure.
    ///
    /// Recovery from `MutationPaused` requires usable free space strictly above
    /// the warning threshold for two consecutive successful polls.
    ///
    /// # Errors
    ///
    /// Returns [`DiskPolicyError`] if threshold arithmetic overflows.
    pub fn evaluate(
        &mut self,
        sample: DiskSample,
        inputs: HeadroomInputs,
        failure: Option<StorageFailure>,
        dirty: DirtyDrainState,
    ) -> Result<DiskAdmission, DiskPolicyError> {
        let thresholds = DiskThresholds::compute(sample.capacity_bytes, inputs)?;

        if matches!(dirty, DirtyDrainState::FailedShutdownRequired) {
            self.state = StoragePressureState::ShutdownRequired;
            self.successful_resume_polls = 0;
        } else if matches!(dirty, DirtyDrainState::FailedReadOnlyAvailable) {
            self.state = StoragePressureState::RecoverableReadOnly;
            self.successful_resume_polls = 0;
        } else if failure.is_some() || sample.usable_free_bytes < thresholds.mutation_paused_below {
            self.state = StoragePressureState::MutationPaused;
            self.successful_resume_polls = 0;
        } else if self.state == StoragePressureState::MutationPaused {
            if sample.usable_free_bytes > thresholds.warning_below {
                self.successful_resume_polls = self.successful_resume_polls.saturating_add(1);
                if self.successful_resume_polls >= 2 {
                    self.state = StoragePressureState::Normal;
                    self.successful_resume_polls = 0;
                }
            } else {
                self.successful_resume_polls = 0;
            }
        } else if matches!(
            self.state,
            StoragePressureState::RecoverableReadOnly | StoragePressureState::ShutdownRequired
        ) {
            self.successful_resume_polls = 0;
        } else if sample.usable_free_bytes < thresholds.warning_below {
            self.state = StoragePressureState::Warning;
            self.successful_resume_polls = 0;
        } else {
            self.state = StoragePressureState::Normal;
            self.successful_resume_polls = 0;
        }

        Ok(DiskAdmission {
            state: self.state,
            thresholds,
            usable_free_bytes: sample.usable_free_bytes,
        })
    }
}

/// Failure to compute a bounded storage-pressure decision.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum DiskPolicyError {
    /// Required-headroom arithmetic overflowed `u64`.
    #[error("low-disk threshold arithmetic overflowed")]
    ArithmeticOverflow,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pause_boundary_is_strict_and_blocks_before_mutation() {
        let inputs = HeadroomInputs::default();
        let thresholds =
            DiskThresholds::compute(100 * GIB, inputs).unwrap_or_else(|error| panic!("{error}"));
        let mut at_boundary = LowDiskMonitor::new();
        let admitted = at_boundary
            .evaluate(
                DiskSample {
                    usable_free_bytes: thresholds.mutation_paused_below,
                    capacity_bytes: 100 * GIB,
                },
                inputs,
                None,
                DirtyDrainState::Clean,
            )
            .unwrap_or_else(|error| panic!("{error}"));
        assert!(admitted.allows(StorageOperation::AuthoritativeMutation));

        let mut below_boundary = LowDiskMonitor::new();
        let paused = below_boundary
            .evaluate(
                DiskSample {
                    usable_free_bytes: thresholds.mutation_paused_below - 1,
                    capacity_bytes: 100 * GIB,
                },
                inputs,
                None,
                DirtyDrainState::Drainable,
            )
            .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(paused.state, StoragePressureState::MutationPaused);
        assert!(!paused.allows(StorageOperation::AuthoritativeMutation));
        assert!(paused.allows(StorageOperation::DurabilityDrain));
    }

    #[test]
    fn resume_requires_two_strictly_safe_polls_and_write_failure_pauses() {
        let inputs = HeadroomInputs::default();
        let sample = DiskSample {
            usable_free_bytes: 100 * GIB,
            capacity_bytes: 100 * GIB,
        };
        let mut monitor = LowDiskMonitor::new();
        let failed = monitor
            .evaluate(
                sample,
                inputs,
                Some(StorageFailure::Wal),
                DirtyDrainState::Drainable,
            )
            .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(failed.state, StoragePressureState::MutationPaused);

        let first = monitor
            .evaluate(sample, inputs, None, DirtyDrainState::Drainable)
            .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(first.state, StoragePressureState::MutationPaused);
        let second = monitor
            .evaluate(sample, inputs, None, DirtyDrainState::Drainable)
            .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(second.state, StoragePressureState::Normal);
    }
}
