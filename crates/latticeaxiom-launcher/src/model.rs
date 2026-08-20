//! Versioned launcher DTOs and bounded policy types.

use std::{
    fmt,
    sync::atomic::{AtomicBool, Ordering},
};

use latticeaxiom_core::{CanonicalHash, WorldId, canonical_json_bytes, canonical_json_hash};
use serde::{Deserialize, Deserializer, Serialize, de};
use serde_json::Value;
use thiserror::Error;

use crate::process::{PriorChildStatusV1, SpawnFailureV1, TerminationFailureV1};

use crate::{
    LaunchIntentError, LaunchModelError, MAX_LAUNCH_CLOCK_SKEW_MS, MAX_LAUNCH_INTENT_BYTES,
    MAX_LAUNCH_INTENT_LIFETIME_MS,
};

/// Stable schema version for [`LaunchIntentV1`].
pub const LAUNCH_INTENT_SCHEMA_VERSION: u32 = 1;

/// Stable schema version for launch failure and barrier receipts.
pub const LAUNCH_RECEIPT_SCHEMA_VERSION: u32 = 1;

/// Stable schema version for [`BootstrapAckV1`].
pub const BOOTSTRAP_ACK_SCHEMA_VERSION: u32 = 1;

/// Stable schema version for [`RecoveryLaunchRequestV1`].
pub const RECOVERY_REQUEST_SCHEMA_VERSION: u32 = 1;

/// Stable schema version for [`RecoveryBootstrapAckV1`].
pub const RECOVERY_ACK_SCHEMA_VERSION: u32 = 1;

/// Monotonically increasing launcher transition generation.
#[repr(transparent)]
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct LaunchGeneration(u64);

impl LaunchGeneration {
    /// First valid launch generation.
    pub const FIRST: Self = Self(1);

    /// Creates a non-zero transition generation.
    ///
    /// # Errors
    ///
    /// Returns [`LaunchModelError::Zero`] when `value` is zero.
    pub const fn new(value: u64) -> Result<Self, LaunchModelError> {
        if value == 0 {
            Err(LaunchModelError::Zero {
                field: "launch generation",
            })
        } else {
            Ok(Self(value))
        }
    }

    /// Returns the integer representation.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }

    /// Returns the next monotonic generation.
    ///
    /// # Errors
    ///
    /// Returns [`LaunchModelError::CounterExhausted`] at `u64::MAX`.
    pub const fn next(self) -> Result<Self, LaunchModelError> {
        match self.0.checked_add(1) {
            Some(next) => Ok(Self(next)),
            None => Err(LaunchModelError::CounterExhausted {
                field: "launch generation",
            }),
        }
    }
}

impl<'de> Deserialize<'de> for LaunchGeneration {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Self::new(u64::deserialize(deserializer)?).map_err(de::Error::custom)
    }
}

impl fmt::Display for LaunchGeneration {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

/// One-based immutable boot-attempt counter carried by a launch intent.
#[repr(transparent)]
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct LaunchAttempt(u8);

impl LaunchAttempt {
    /// The only target boot attempt allowed by the V1 recovery-loop policy.
    pub const FIRST: Self = Self(1);

    /// Creates a non-zero attempt count.
    ///
    /// # Errors
    ///
    /// Returns [`LaunchModelError::Zero`] when `value` is zero.
    pub const fn new(value: u8) -> Result<Self, LaunchModelError> {
        if value == 0 {
            Err(LaunchModelError::Zero {
                field: "launch attempt",
            })
        } else {
            Ok(Self(value))
        }
    }

    /// Returns the integer representation.
    #[must_use]
    pub const fn get(self) -> u8 {
        self.0
    }
}

impl<'de> Deserialize<'de> for LaunchAttempt {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Self::new(u8::deserialize(deserializer)?).map_err(de::Error::custom)
    }
}

/// Non-zero epoch identifying one operating-system client process.
#[repr(transparent)]
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct ProcessEpoch(u64);

impl ProcessEpoch {
    /// First valid process epoch for a directly launched client.
    pub const FIRST: Self = Self(1);

    /// Creates a non-zero process epoch supplied by the external launcher.
    ///
    /// # Errors
    ///
    /// Returns [`LaunchModelError::Zero`] when `value` is zero.
    pub const fn new(value: u64) -> Result<Self, LaunchModelError> {
        if value == 0 {
            Err(LaunchModelError::Zero {
                field: "process epoch",
            })
        } else {
            Ok(Self(value))
        }
    }

    /// Returns the integer representation.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

impl<'de> Deserialize<'de> for ProcessEpoch {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Self::new(u64::deserialize(deserializer)?).map_err(de::Error::custom)
    }
}

/// Stable identity of the durable external process supervisor.
///
/// Recovery requests bind to this identity so another adapter cannot acquire
/// or acknowledge a previously claimed recovery attempt.
#[repr(transparent)]
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct ProcessSupervisorIdentityV1(CanonicalHash);

impl ProcessSupervisorIdentityV1 {
    /// Creates an identity from the supervisor's stable public digest.
    #[must_use]
    pub const fn new(digest: CanonicalHash) -> Self {
        Self(digest)
    }

    /// Returns the stable public digest.
    #[must_use]
    pub const fn digest(self) -> CanonicalHash {
        self.0
    }
}

/// Monotonic revision of the durable settings transaction journal.
#[repr(transparent)]
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct SettingTransactionRevision(u64);

impl SettingTransactionRevision {
    /// Creates a settings revision. Zero denotes an empty journal.
    #[must_use]
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    /// Returns the integer representation.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

/// Monotonic revision of one authoritative world writer.
#[repr(transparent)]
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct WorldRevision(u64);

impl WorldRevision {
    /// Creates a world revision. Zero denotes the initial durable image.
    #[must_use]
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    /// Returns the integer representation.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

/// Client process destination authenticated by a launch intent.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case", tag = "kind")]
pub enum LaunchTargetV1 {
    /// Start the normal package-driven shell.
    Shell,
    /// Start one exact preflighted world.
    World {
        /// Immutable world identity.
        world_id: WorldId,
    },
}

/// World identity and revision proven durable by a shutdown barrier.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DurableWorldRevisionV1 {
    world_id: WorldId,
    revision: WorldRevision,
}

impl DurableWorldRevisionV1 {
    /// Creates a durable writer receipt for one exact world.
    #[must_use]
    pub const fn new(world_id: WorldId, revision: WorldRevision) -> Self {
        Self { world_id, revision }
    }

    /// Returns the world whose writer reached durability.
    #[must_use]
    pub const fn world_id(self) -> WorldId {
        self.world_id
    }

    /// Returns the durable authoritative revision.
    #[must_use]
    pub const fn revision(self) -> WorldRevision {
        self.revision
    }
}

/// Unsealed values used to construct one validated launch intent.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LaunchIntentDraftV1 {
    /// Monotonic transition generation.
    pub generation: LaunchGeneration,
    /// Immutable one-based attempt count. V1 accepts exactly one.
    pub attempt: LaunchAttempt,
    /// Launcher-provided wall-clock issue instant in milliseconds.
    pub issued_at_ms: u64,
    /// Launcher-provided expiry instant in milliseconds.
    pub expires_at_ms: u64,
    /// Destination process role.
    pub target: LaunchTargetV1,
    /// Exact shell lock used by every client role.
    pub shell_lock_hash: CanonicalHash,
    /// Exact world lock for a world target; absent for shell.
    pub world_lock_hash: Option<CanonicalHash>,
    /// Read-only preflight plan hash for a world target; absent for shell.
    pub world_open_plan_hash: Option<CanonicalHash>,
    /// Last settings transaction confirmed before the shutdown barrier.
    pub confirmed_setting_transaction_revision: SettingTransactionRevision,
}

/// Canonical, checksummed cross-process launch envelope.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct LaunchIntentV1 {
    schema_version: u32,
    generation: LaunchGeneration,
    attempt: LaunchAttempt,
    issued_at_ms: u64,
    expires_at_ms: u64,
    target: LaunchTargetV1,
    shell_lock_hash: CanonicalHash,
    world_lock_hash: Option<CanonicalHash>,
    world_open_plan_hash: Option<CanonicalHash>,
    confirmed_setting_transaction_revision: SettingTransactionRevision,
    checksum: CanonicalHash,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LaunchIntentWireV1 {
    schema_version: u32,
    generation: LaunchGeneration,
    attempt: LaunchAttempt,
    issued_at_ms: u64,
    expires_at_ms: u64,
    target: LaunchTargetV1,
    shell_lock_hash: CanonicalHash,
    world_lock_hash: Option<CanonicalHash>,
    world_open_plan_hash: Option<CanonicalHash>,
    confirmed_setting_transaction_revision: SettingTransactionRevision,
    checksum: CanonicalHash,
}

impl From<LaunchIntentWireV1> for LaunchIntentV1 {
    fn from(wire: LaunchIntentWireV1) -> Self {
        Self {
            schema_version: wire.schema_version,
            generation: wire.generation,
            attempt: wire.attempt,
            issued_at_ms: wire.issued_at_ms,
            expires_at_ms: wire.expires_at_ms,
            target: wire.target,
            shell_lock_hash: wire.shell_lock_hash,
            world_lock_hash: wire.world_lock_hash,
            world_open_plan_hash: wire.world_open_plan_hash,
            confirmed_setting_transaction_revision: wire.confirmed_setting_transaction_revision,
            checksum: wire.checksum,
        }
    }
}

impl LaunchIntentV1 {
    /// Validates, checksums, and seals a V1 launch intent.
    ///
    /// # Errors
    ///
    /// Returns [`LaunchModelError`] for an invalid lifetime, attempt, target
    /// shape, or canonical encoding failure.
    pub fn seal(draft: LaunchIntentDraftV1) -> Result<Self, LaunchModelError> {
        validate_draft(&draft)?;
        let mut intent = Self {
            schema_version: LAUNCH_INTENT_SCHEMA_VERSION,
            generation: draft.generation,
            attempt: draft.attempt,
            issued_at_ms: draft.issued_at_ms,
            expires_at_ms: draft.expires_at_ms,
            target: draft.target,
            shell_lock_hash: draft.shell_lock_hash,
            world_lock_hash: draft.world_lock_hash,
            world_open_plan_hash: draft.world_open_plan_hash,
            confirmed_setting_transaction_revision: draft.confirmed_setting_transaction_revision,
            checksum: CanonicalHash::digest(b"unsealed launch intent"),
        };
        intent.checksum =
            intent
                .recompute_checksum()
                .map_err(|error| LaunchModelError::CanonicalEncoding {
                    reason: error.to_string(),
                })?;
        Ok(intent)
    }

    /// Decodes exact canonical JSON bytes and applies launch-time policy.
    ///
    /// # Errors
    ///
    /// Returns [`LaunchIntentError`] if the input is oversized, malformed,
    /// non-canonical, corrupt, stale, expired, or differs from the selected
    /// target and lock receipts.
    pub fn decode_and_validate(
        bytes: &[u8],
        expected_generation: LaunchGeneration,
        policy: &TransitionValidationPolicy,
    ) -> Result<Self, LaunchIntentError> {
        if bytes.len() > MAX_LAUNCH_INTENT_BYTES {
            return Err(LaunchIntentError::InputTooLarge {
                actual_bytes: bytes.len(),
                maximum_bytes: MAX_LAUNCH_INTENT_BYTES,
            });
        }
        let intent = serde_json::from_slice::<LaunchIntentWireV1>(bytes)
            .map(Self::from)
            .map_err(|error| LaunchIntentError::InvalidJson {
                reason: bounded_parser_reason(&error.to_string()),
            })?;
        let canonical =
            canonical_json_bytes(&intent).map_err(|error| LaunchIntentError::InvalidJson {
                reason: bounded_parser_reason(&error.to_string()),
            })?;
        if canonical != bytes {
            return Err(LaunchIntentError::NonCanonicalBytes);
        }
        intent.validate(expected_generation, policy)?;
        Ok(intent)
    }

    pub(crate) fn authenticate_at_rest(bytes: &[u8]) -> Result<Self, LaunchIntentError> {
        if bytes.len() > MAX_LAUNCH_INTENT_BYTES {
            return Err(LaunchIntentError::InputTooLarge {
                actual_bytes: bytes.len(),
                maximum_bytes: MAX_LAUNCH_INTENT_BYTES,
            });
        }
        let intent = serde_json::from_slice::<LaunchIntentWireV1>(bytes)
            .map(Self::from)
            .map_err(|error| LaunchIntentError::InvalidJson {
                reason: bounded_parser_reason(&error.to_string()),
            })?;
        let canonical =
            canonical_json_bytes(&intent).map_err(|error| LaunchIntentError::InvalidJson {
                reason: bounded_parser_reason(&error.to_string()),
            })?;
        if canonical != bytes {
            return Err(LaunchIntentError::NonCanonicalBytes);
        }
        if intent.schema_version != LAUNCH_INTENT_SCHEMA_VERSION {
            return Err(LaunchIntentError::UnsupportedSchema {
                actual: intent.schema_version,
                expected: LAUNCH_INTENT_SCHEMA_VERSION,
            });
        }
        if intent.attempt != LaunchAttempt::FIRST {
            return Err(LaunchIntentError::InvalidAttempt {
                actual: intent.attempt.get(),
            });
        }
        if !target_hashes_are_valid(
            intent.target,
            intent.world_lock_hash,
            intent.world_open_plan_hash,
        ) {
            return Err(LaunchIntentError::InvalidTargetHashes);
        }
        let lifetime = intent
            .expires_at_ms
            .checked_sub(intent.issued_at_ms)
            .ok_or(LaunchIntentError::InvalidLifetime)?;
        if lifetime > MAX_LAUNCH_INTENT_LIFETIME_MS {
            return Err(LaunchIntentError::InvalidLifetime);
        }
        let actual =
            intent
                .recompute_checksum()
                .map_err(|error| LaunchIntentError::InvalidJson {
                    reason: bounded_parser_reason(&error.to_string()),
                })?;
        if actual != intent.checksum {
            return Err(LaunchIntentError::ChecksumMismatch {
                expected: intent.checksum,
                actual,
            });
        }
        Ok(intent)
    }

    /// Encodes the sealed envelope as exact canonical JSON bytes.
    ///
    /// # Errors
    ///
    /// Returns [`LaunchModelError::CanonicalEncoding`] if serialization fails.
    pub fn canonical_bytes(&self) -> Result<Vec<u8>, LaunchModelError> {
        canonical_json_bytes(self).map_err(|error| LaunchModelError::CanonicalEncoding {
            reason: error.to_string(),
        })
    }

    /// Returns the schema version.
    #[must_use]
    pub const fn schema_version(&self) -> u32 {
        self.schema_version
    }

    /// Returns the monotonic transition generation.
    #[must_use]
    pub const fn generation(&self) -> LaunchGeneration {
        self.generation
    }

    /// Returns the immutable boot-attempt count.
    #[must_use]
    pub const fn attempt(&self) -> LaunchAttempt {
        self.attempt
    }

    /// Returns the issue instant in milliseconds.
    #[must_use]
    pub const fn issued_at_ms(&self) -> u64 {
        self.issued_at_ms
    }

    /// Returns the expiry instant in milliseconds.
    #[must_use]
    pub const fn expires_at_ms(&self) -> u64 {
        self.expires_at_ms
    }

    /// Returns the destination role.
    #[must_use]
    pub const fn target(&self) -> LaunchTargetV1 {
        self.target
    }

    /// Returns the exact shell lock hash.
    #[must_use]
    pub const fn shell_lock_hash(&self) -> CanonicalHash {
        self.shell_lock_hash
    }

    /// Returns the optional frozen world lock hash.
    #[must_use]
    pub const fn world_lock_hash(&self) -> Option<CanonicalHash> {
        self.world_lock_hash
    }

    /// Returns the optional read-only world-open plan hash.
    #[must_use]
    pub const fn world_open_plan_hash(&self) -> Option<CanonicalHash> {
        self.world_open_plan_hash
    }

    /// Returns the last settings revision confirmed by the shutdown barrier.
    #[must_use]
    pub const fn confirmed_setting_transaction_revision(&self) -> SettingTransactionRevision {
        self.confirmed_setting_transaction_revision
    }

    /// Returns the body checksum.
    #[must_use]
    pub const fn checksum(&self) -> CanonicalHash {
        self.checksum
    }

    fn validate(
        &self,
        expected_generation: LaunchGeneration,
        policy: &TransitionValidationPolicy,
    ) -> Result<(), LaunchIntentError> {
        if self.schema_version != LAUNCH_INTENT_SCHEMA_VERSION {
            return Err(LaunchIntentError::UnsupportedSchema {
                actual: self.schema_version,
                expected: LAUNCH_INTENT_SCHEMA_VERSION,
            });
        }
        if self.attempt != LaunchAttempt::FIRST {
            return Err(LaunchIntentError::InvalidAttempt {
                actual: self.attempt.get(),
            });
        }
        if !target_hashes_are_valid(self.target, self.world_lock_hash, self.world_open_plan_hash) {
            return Err(LaunchIntentError::InvalidTargetHashes);
        }
        let actual = self
            .recompute_checksum()
            .map_err(|error| LaunchIntentError::InvalidJson {
                reason: bounded_parser_reason(&error.to_string()),
            })?;
        if actual != self.checksum {
            return Err(LaunchIntentError::ChecksumMismatch {
                expected: self.checksum,
                actual,
            });
        }
        validate_lifetime(self.issued_at_ms, self.expires_at_ms, policy.now_ms)?;
        if self.generation < expected_generation {
            let last_handled = LaunchGeneration(expected_generation.get() - 1);
            return Err(LaunchIntentError::StaleGeneration {
                actual: self.generation,
                last_handled,
            });
        }
        if self.generation != expected_generation {
            return Err(LaunchIntentError::GenerationGap {
                actual: self.generation,
                expected: expected_generation,
            });
        }
        if self.shell_lock_hash != policy.expected_shell_lock_hash {
            return Err(LaunchIntentError::ShellLockMismatch);
        }
        if self.confirmed_setting_transaction_revision
            != policy.expected_confirmed_setting_transaction_revision
        {
            return Err(LaunchIntentError::ConfirmedSettingsRevisionMismatch);
        }
        if self.target != policy.expected_target {
            return Err(LaunchIntentError::TargetMismatch);
        }
        if self.world_lock_hash != policy.expected_world_lock_hash {
            return Err(LaunchIntentError::WorldLockMismatch);
        }
        if self.world_open_plan_hash != policy.expected_world_open_plan_hash {
            return Err(LaunchIntentError::WorldOpenPlanMismatch);
        }
        Ok(())
    }

    fn recompute_checksum(&self) -> Result<CanonicalHash, latticeaxiom_core::CanonicalJsonError> {
        let mut value = serde_json::to_value(self)?;
        if let Value::Object(fields) = &mut value {
            fields.remove("checksum");
        }
        canonical_json_hash(&value)
    }
}

/// Read-only expectations used when accepting a cross-process intent.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TransitionValidationPolicy {
    now_ms: u64,
    expected_target: LaunchTargetV1,
    expected_shell_lock_hash: CanonicalHash,
    expected_confirmed_setting_transaction_revision: SettingTransactionRevision,
    expected_world_lock_hash: Option<CanonicalHash>,
    expected_world_open_plan_hash: Option<CanonicalHash>,
}

impl TransitionValidationPolicy {
    /// Creates policy for a normal shell destination.
    #[must_use]
    pub const fn for_shell(
        now_ms: u64,
        shell_lock_hash: CanonicalHash,
        confirmed_setting_transaction_revision: SettingTransactionRevision,
    ) -> Self {
        Self {
            now_ms,
            expected_target: LaunchTargetV1::Shell,
            expected_shell_lock_hash: shell_lock_hash,
            expected_confirmed_setting_transaction_revision: confirmed_setting_transaction_revision,
            expected_world_lock_hash: None,
            expected_world_open_plan_hash: None,
        }
    }

    /// Creates policy for an exact preflighted world destination.
    #[must_use]
    pub const fn for_world(
        now_ms: u64,
        world_id: WorldId,
        shell_lock_hash: CanonicalHash,
        world_lock_hash: CanonicalHash,
        world_open_plan_hash: CanonicalHash,
        confirmed_setting_transaction_revision: SettingTransactionRevision,
    ) -> Self {
        Self {
            now_ms,
            expected_target: LaunchTargetV1::World { world_id },
            expected_shell_lock_hash: shell_lock_hash,
            expected_confirmed_setting_transaction_revision: confirmed_setting_transaction_revision,
            expected_world_lock_hash: Some(world_lock_hash),
            expected_world_open_plan_hash: Some(world_open_plan_hash),
        }
    }

    /// Returns the validation instant.
    #[must_use]
    pub const fn now_ms(&self) -> u64 {
        self.now_ms
    }

    /// Returns the expected target.
    #[must_use]
    pub const fn expected_target(&self) -> LaunchTargetV1 {
        self.expected_target
    }

    /// Returns the exact shell lock used by normal and recovery bootstrap.
    #[must_use]
    pub const fn expected_shell_lock_hash(&self) -> CanonicalHash {
        self.expected_shell_lock_hash
    }

    /// Returns the exact confirmed settings revision used by bootstrap.
    #[must_use]
    pub const fn expected_confirmed_setting_transaction_revision(
        &self,
    ) -> SettingTransactionRevision {
        self.expected_confirmed_setting_transaction_revision
    }
}

/// Stable current-process role tracked by the transition publisher.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case", tag = "state")]
pub enum ClientRuntimeStateV1 {
    /// A normal client shell process is active.
    Shell {
        /// Generation that booted this process.
        generation: LaunchGeneration,
        /// Process epoch bound to the single-client-App lease.
        process_epoch: ProcessEpoch,
        /// Exact consumed launch-slot payload, absent only for initial shell boot.
        launch_blob_hash: Option<CanonicalHash>,
    },
    /// A client world process is active.
    World {
        /// Generation that booted this process.
        generation: LaunchGeneration,
        /// Process epoch bound to the single-client-App lease.
        process_epoch: ProcessEpoch,
        /// Active world identity.
        world_id: WorldId,
        /// Exact consumed launch-slot payload that booted this world.
        launch_blob_hash: CanonicalHash,
    },
    /// A package-minimal recovery shell is active.
    Recovery {
        /// Generation whose failure led to recovery.
        generation: LaunchGeneration,
        /// Process epoch bound to the single-client-App lease.
        process_epoch: ProcessEpoch,
        /// Stable recovery reason.
        reason: RecoveryReasonV1,
        /// Exact durable recovery-claim payload that authorized this shell.
        recovery_claim_hash: CanonicalHash,
    },
    /// A durable recovery request exists and the current process must exit.
    RecoveryHandoffPublished {
        /// Generation retained by the recovery claim.
        generation: LaunchGeneration,
        /// Authenticated recovery-request checksum.
        checksum: CanonicalHash,
    },
    /// A durable normal handoff exists and the current process must exit.
    HandoffPublished {
        /// Published generation.
        generation: LaunchGeneration,
        /// Published target.
        target: LaunchTargetV1,
        /// Authenticated intent checksum.
        checksum: CanonicalHash,
    },
    /// No further process restart is allowed automatically.
    Halted {
        /// Last known generation, if an intent was decoded.
        generation: Option<LaunchGeneration>,
        /// Stable reason for halting.
        reason: RecoveryReasonV1,
    },
}

impl ClientRuntimeStateV1 {
    /// Returns the generation associated with this state, when known.
    #[must_use]
    pub const fn generation(self) -> Option<LaunchGeneration> {
        match self {
            Self::Shell { generation, .. }
            | Self::World { generation, .. }
            | Self::Recovery { generation, .. }
            | Self::RecoveryHandoffPublished { generation, .. }
            | Self::HandoffPublished { generation, .. } => Some(generation),
            Self::Halted { generation, .. } => generation,
        }
    }

    /// Returns the exact durable slot payload that booted an active process.
    #[must_use]
    pub const fn boot_slot_blob_hash(self) -> Option<CanonicalHash> {
        match self {
            Self::Shell {
                launch_blob_hash, ..
            } => launch_blob_hash,
            Self::World {
                launch_blob_hash, ..
            } => Some(launch_blob_hash),
            Self::Recovery {
                recovery_claim_hash,
                ..
            } => Some(recovery_claim_hash),
            Self::RecoveryHandoffPublished { .. }
            | Self::HandoffPublished { .. }
            | Self::Halted { .. } => None,
        }
    }
}

/// Stage reached by a normal shutdown barrier.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ShutdownBarrierStageV1 {
    /// Settings were not flushed.
    FlushSettings,
    /// A world revision was not confirmed durable.
    ConfirmDurability,
    /// Package instances did not stop.
    StopPackages,
    /// Bounded tasks did not cancel and join.
    JoinTasks,
    /// The world writer did not close.
    CloseWriter,
}

/// Successful all-or-nothing shutdown barrier receipt.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ShutdownBarrierReceiptV1 {
    schema_version: u32,
    confirmed_setting_transaction_revision: SettingTransactionRevision,
    durable_world: Option<DurableWorldRevisionV1>,
}

impl ShutdownBarrierReceiptV1 {
    /// Creates a completed barrier receipt.
    #[must_use]
    pub const fn complete(
        confirmed_setting_transaction_revision: SettingTransactionRevision,
        durable_world: Option<DurableWorldRevisionV1>,
    ) -> Self {
        Self {
            schema_version: LAUNCH_RECEIPT_SCHEMA_VERSION,
            confirmed_setting_transaction_revision,
            durable_world,
        }
    }

    /// Returns the confirmed settings revision.
    #[must_use]
    pub const fn confirmed_setting_transaction_revision(self) -> SettingTransactionRevision {
        self.confirmed_setting_transaction_revision
    }

    /// Returns the optional exact world durability receipt.
    #[must_use]
    pub const fn durable_world(self) -> Option<DurableWorldRevisionV1> {
        self.durable_world
    }
}

/// Whether a barrier failure leaves the current App safe to keep running.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum FailureDispositionV1 {
    /// The barrier failed before destructive shutdown work.
    CurrentProcessUsable,
    /// Quiescence is unknown and the current process must terminate safely.
    RecoveryRequired,
}

/// Stable failure emitted by a shutdown barrier implementation.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ShutdownBarrierFailureV1 {
    schema_version: u32,
    stage: ShutdownBarrierStageV1,
    disposition: FailureDispositionV1,
}

impl ShutdownBarrierFailureV1 {
    /// Creates a typed barrier failure.
    #[must_use]
    pub const fn new(stage: ShutdownBarrierStageV1, disposition: FailureDispositionV1) -> Self {
        Self {
            schema_version: LAUNCH_RECEIPT_SCHEMA_VERSION,
            stage,
            disposition,
        }
    }

    /// Returns the failed stage.
    #[must_use]
    pub const fn stage(self) -> ShutdownBarrierStageV1 {
        self.stage
    }

    /// Returns whether the current process is reusable.
    #[must_use]
    pub const fn disposition(self) -> FailureDispositionV1 {
        self.disposition
    }
}

/// Stable launcher phase used by deterministic failure receipts.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum LaunchPhaseV1 {
    /// Normal shutdown barrier.
    Barrier,
    /// Temporary intent write.
    IntentWrite,
    /// Temporary or directory synchronization.
    IntentSync,
    /// Atomic slot move.
    IntentReplace,
    /// Bounded slot read.
    IntentRead,
    /// Canonical/checksum/policy validation.
    IntentValidate,
    /// Atomic one-attempt claim.
    IntentClaim,
    /// Replacement process creation.
    Spawn,
    /// Replacement process initialization.
    Boot,
    /// Safe-bootstrap acknowledgement.
    Ack,
    /// Proof that a rejected child exited before recovery.
    Terminate,
    /// Atomic consume after acknowledgement.
    Consume,
    /// Failed intent quarantine.
    Quarantine,
    /// Durable one-attempt recovery claim.
    RecoveryClaim,
    /// Recovery shell process creation.
    RecoverySpawn,
    /// Recovery shell initialization.
    RecoveryBoot,
    /// Recovery shell safe-bootstrap acknowledgement.
    RecoveryAck,
}

/// Stable failure code used without unbounded process or filesystem strings.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum LaunchFailureCodeV1 {
    /// The shutdown barrier rejected the transition.
    BarrierRejected,
    /// The intent temporary file could not be written.
    IntentWriteFailed,
    /// Intent durability synchronization failed.
    IntentSyncFailed,
    /// Atomic intent publication or state change failed.
    IntentReplaceFailed,
    /// The bounded slot could not be read.
    IntentReadFailed,
    /// No pending intent existed.
    IntentMissing,
    /// Intent bytes were malformed or non-canonical.
    IntentCorrupt,
    /// Intent checksum validation failed.
    IntentChecksumMismatch,
    /// Intent generation was stale or replayed.
    IntentStale,
    /// Intent lifetime expired.
    IntentExpired,
    /// Intent did not match selected locks or target.
    IntentSelectionMismatch,
    /// A prior process claimed the one allowed attempt.
    AttemptAlreadyClaimed,
    /// A different blob occupied the slot.
    IntentSlotConflict,
    /// The replacement process could not be spawned.
    SpawnFailed,
    /// The replacement process reported a boot failure.
    BootFailed,
    /// The replacement process crashed before acknowledgement.
    ProcessCrashed,
    /// No acknowledgement arrived within the bounded deadline.
    AckTimedOut,
    /// An acknowledgement did not match the claimed intent.
    AckInvalid,
    /// Child termination could not be proven.
    TerminationFailed,
    /// The acknowledged intent could not be consumed atomically.
    ConsumeFailed,
    /// Quarantine could not be published.
    QuarantineFailed,
    /// The durable one-attempt recovery claim could not be published.
    RecoveryClaimFailed,
    /// Recovery was already attempted and was suppressed.
    RecoveryLoopSuppressed,
}

/// Stable reason presented by the recovery shell.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum RecoveryReasonV1 {
    /// Current-process shutdown could not prove a safe handoff.
    BarrierFailure,
    /// Pending launch data was absent, invalid, stale, or expired.
    InvalidIntent,
    /// The target child could not be created.
    SpawnFailure,
    /// The target failed before reaching a safe bootstrap state.
    BootFailure,
    /// The target acknowledgement was absent or invalid.
    AckFailure,
    /// The acknowledged intent could not be consumed safely.
    ConsumeFailure,
    /// The recovery shell itself failed, so automatic retries stopped.
    RecoveryFailure,
}

/// Bounded typed detail retained for process-control failures.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case", tag = "detail")]
pub enum LaunchFailureDetailV1 {
    /// No additional typed argument applies.
    None,
    /// Durable supervisor status observed after launcher restart.
    PriorChild {
        /// Exact-intent reconciliation result.
        status: PriorChildStatusV1,
    },
    /// Stable process creation rejection.
    Spawn {
        /// Adapter-supplied bounded failure category.
        failure: SpawnFailureV1,
    },
    /// Stable failure to prove child termination.
    Termination {
        /// Adapter-supplied bounded failure category.
        failure: TerminationFailureV1,
    },
    /// Child exit code observed before acknowledgement.
    ProcessExit {
        /// Platform exit code when one was supplied.
        exit_code: Option<i32>,
    },
}

/// Deterministic bounded failure receipt for one launcher phase.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct LaunchFailureReceiptV1 {
    schema_version: u32,
    generation: Option<LaunchGeneration>,
    attempt: Option<LaunchAttempt>,
    target: Option<LaunchTargetV1>,
    phase: LaunchPhaseV1,
    code: LaunchFailureCodeV1,
    detail: LaunchFailureDetailV1,
}

impl LaunchFailureReceiptV1 {
    /// Creates a typed failure receipt.
    #[must_use]
    pub const fn new(
        generation: Option<LaunchGeneration>,
        attempt: Option<LaunchAttempt>,
        target: Option<LaunchTargetV1>,
        phase: LaunchPhaseV1,
        code: LaunchFailureCodeV1,
    ) -> Self {
        Self {
            schema_version: LAUNCH_RECEIPT_SCHEMA_VERSION,
            generation,
            attempt,
            target,
            phase,
            code,
            detail: LaunchFailureDetailV1::None,
        }
    }

    pub(crate) const fn with_detail(
        generation: Option<LaunchGeneration>,
        attempt: Option<LaunchAttempt>,
        target: Option<LaunchTargetV1>,
        phase: LaunchPhaseV1,
        code: LaunchFailureCodeV1,
        detail: LaunchFailureDetailV1,
    ) -> Self {
        Self {
            schema_version: LAUNCH_RECEIPT_SCHEMA_VERSION,
            generation,
            attempt,
            target,
            phase,
            code,
            detail,
        }
    }

    /// Returns the failed phase.
    #[must_use]
    pub const fn phase(self) -> LaunchPhaseV1 {
        self.phase
    }

    /// Returns the stable failure code.
    #[must_use]
    pub const fn code(self) -> LaunchFailureCodeV1 {
        self.code
    }

    /// Returns the bounded typed process-control detail.
    #[must_use]
    pub const fn detail(self) -> LaunchFailureDetailV1 {
        self.detail
    }
}

/// Target-specific state that proves a child is safe to hand control to.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum BootstrapSafeStateV1 {
    /// Normal shell graph and read-only catalog are ready.
    ShellReady,
    /// Exact world graph is validated and the world is safe to enter.
    WorldReady,
    /// Minimal recovery shell is ready without retrying the failed target.
    RecoveryReady,
}

/// Versioned acknowledgement of one exact normal target intent.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BootstrapAckV1 {
    schema_version: u32,
    generation: LaunchGeneration,
    attempt: LaunchAttempt,
    target: LaunchTargetV1,
    intent_checksum: CanonicalHash,
    process_epoch: ProcessEpoch,
    safe_state: BootstrapSafeStateV1,
}

impl BootstrapAckV1 {
    /// Creates an acknowledgement bound to a sealed intent and consumed App lease.
    #[must_use]
    #[allow(clippy::needless_pass_by_value)]
    pub const fn for_ready_intent(
        intent: &LaunchIntentV1,
        app_proof: FreshClientAppLeaseProof,
    ) -> Self {
        let safe_state = match intent.target() {
            LaunchTargetV1::Shell => BootstrapSafeStateV1::ShellReady,
            LaunchTargetV1::World { .. } => BootstrapSafeStateV1::WorldReady,
        };
        Self {
            schema_version: BOOTSTRAP_ACK_SCHEMA_VERSION,
            generation: intent.generation(),
            attempt: intent.attempt(),
            target: intent.target(),
            intent_checksum: intent.checksum(),
            process_epoch: app_proof.process_epoch,
            safe_state,
        }
    }

    /// Returns the exact intent checksum acknowledged by the child.
    #[must_use]
    pub const fn intent_checksum(self) -> CanonicalHash {
        self.intent_checksum
    }

    /// Returns the acknowledged process epoch.
    #[must_use]
    pub const fn process_epoch(self) -> ProcessEpoch {
        self.process_epoch
    }

    pub(crate) fn matches_intent(self, intent: &LaunchIntentV1) -> bool {
        let expected_safe_state = match intent.target() {
            LaunchTargetV1::Shell => BootstrapSafeStateV1::ShellReady,
            LaunchTargetV1::World { .. } => BootstrapSafeStateV1::WorldReady,
        };
        self.schema_version == BOOTSTRAP_ACK_SCHEMA_VERSION
            && self.generation == intent.generation()
            && self.attempt == intent.attempt()
            && self.target == intent.target()
            && self.intent_checksum == intent.checksum()
            && self.safe_state == expected_safe_state
    }
}

/// Rejection while authenticating a durable recovery request.
#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum RecoveryRequestError {
    /// The envelope exceeded the fixed input ceiling.
    #[error("recovery request has {actual_bytes} bytes; maximum is {maximum_bytes}")]
    InputTooLarge {
        /// Actual byte length.
        actual_bytes: usize,
        /// Maximum accepted byte length.
        maximum_bytes: usize,
    },
    /// The bytes were not a strict V1 JSON envelope.
    #[error("recovery request JSON is invalid: {reason}")]
    InvalidJson {
        /// Bounded parser diagnostic.
        reason: String,
    },
    /// The bytes were not the canonical encoding.
    #[error("recovery request bytes are not canonical JSON")]
    NonCanonicalBytes,
    /// The schema version is not supported.
    #[error("unsupported recovery request schema version {actual}; expected {expected}")]
    UnsupportedSchema {
        /// Encountered schema version.
        actual: u32,
        /// Supported schema version.
        expected: u32,
    },
    /// The body checksum did not authenticate.
    #[error("recovery request checksum mismatch")]
    ChecksumMismatch,
    /// The durable recovery generation did not match the authoritative slot chain.
    #[error("recovery request generation does not match the durable slot chain")]
    GenerationMismatch,
    /// The request selected another shell lock.
    #[error("recovery request shell lock does not match the selected shell lock")]
    ShellLockMismatch,
    /// The request selected another confirmed settings revision.
    #[error("recovery request settings revision does not match the boot journal")]
    ConfirmedSettingsRevisionMismatch,
    /// The request belongs to another durable supervisor.
    #[error("recovery request belongs to another process supervisor")]
    SupervisorMismatch,
}

/// Trusted fields used to seal a durable recovery request.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct RecoveryLaunchDraftV1 {
    pub(crate) recovery_generation: LaunchGeneration,
    pub(crate) source_generation: Option<LaunchGeneration>,
    pub(crate) source_blob_hash: Option<CanonicalHash>,
    pub(crate) reason: RecoveryReasonV1,
    pub(crate) shell_lock_hash: CanonicalHash,
    pub(crate) confirmed_setting_transaction_revision: SettingTransactionRevision,
    pub(crate) supervisor_identity: ProcessSupervisorIdentityV1,
    pub(crate) failure: LaunchFailureReceiptV1,
}
/// Versioned, self-authenticating minimal-shell request stored as the durable
/// recovery claim itself.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RecoveryLaunchRequestV1 {
    schema_version: u32,
    recovery_generation: LaunchGeneration,
    source_generation: Option<LaunchGeneration>,
    source_blob_hash: Option<CanonicalHash>,
    reason: RecoveryReasonV1,
    shell_lock_hash: CanonicalHash,
    confirmed_setting_transaction_revision: SettingTransactionRevision,
    supervisor_identity: ProcessSupervisorIdentityV1,
    failure: LaunchFailureReceiptV1,
    checksum: CanonicalHash,
}

impl RecoveryLaunchRequestV1 {
    pub(crate) fn seal(draft: RecoveryLaunchDraftV1) -> Result<Self, LaunchModelError> {
        if draft
            .source_generation
            .is_some_and(|source| source > draft.recovery_generation)
        {
            return Err(LaunchModelError::InvalidRecoveryGeneration {
                source_generation: draft.source_generation,
                recovery_generation: draft.recovery_generation,
            });
        }
        let mut request = Self {
            schema_version: RECOVERY_REQUEST_SCHEMA_VERSION,
            recovery_generation: draft.recovery_generation,
            source_generation: draft.source_generation,
            source_blob_hash: draft.source_blob_hash,
            reason: draft.reason,
            shell_lock_hash: draft.shell_lock_hash,
            confirmed_setting_transaction_revision: draft.confirmed_setting_transaction_revision,
            supervisor_identity: draft.supervisor_identity,
            failure: draft.failure,
            checksum: CanonicalHash::digest(b"unsealed recovery request"),
        };
        request.checksum =
            request
                .recompute_checksum()
                .map_err(|error| LaunchModelError::CanonicalEncoding {
                    reason: error.to_string(),
                })?;
        Ok(request)
    }

    /// Authenticates bounded canonical recovery-claim bytes at rest.
    ///
    /// # Errors
    ///
    /// Returns [`RecoveryRequestError`] for oversized, malformed,
    /// non-canonical, unsupported, or checksum-invalid bytes.
    pub fn authenticate_at_rest(bytes: &[u8]) -> Result<Self, RecoveryRequestError> {
        if bytes.len() > MAX_LAUNCH_INTENT_BYTES {
            return Err(RecoveryRequestError::InputTooLarge {
                actual_bytes: bytes.len(),
                maximum_bytes: MAX_LAUNCH_INTENT_BYTES,
            });
        }
        let request = serde_json::from_slice::<Self>(bytes).map_err(|error| {
            RecoveryRequestError::InvalidJson {
                reason: bounded_parser_reason(&error.to_string()),
            }
        })?;
        let canonical =
            canonical_json_bytes(&request).map_err(|error| RecoveryRequestError::InvalidJson {
                reason: bounded_parser_reason(&error.to_string()),
            })?;
        if canonical != bytes {
            return Err(RecoveryRequestError::NonCanonicalBytes);
        }
        if request.schema_version != RECOVERY_REQUEST_SCHEMA_VERSION {
            return Err(RecoveryRequestError::UnsupportedSchema {
                actual: request.schema_version,
                expected: RECOVERY_REQUEST_SCHEMA_VERSION,
            });
        }
        if request
            .recompute_checksum()
            .map_or(true, |actual| actual != request.checksum)
        {
            return Err(RecoveryRequestError::ChecksumMismatch);
        }
        if request
            .source_generation
            .is_some_and(|source| source > request.recovery_generation)
        {
            return Err(RecoveryRequestError::GenerationMismatch);
        }
        Ok(request)
    }

    /// Encodes the sealed recovery claim as canonical JSON bytes.
    ///
    /// # Errors
    ///
    /// Returns [`LaunchModelError::CanonicalEncoding`] if serialization fails.
    pub fn canonical_bytes(&self) -> Result<Vec<u8>, LaunchModelError> {
        canonical_json_bytes(self).map_err(|error| LaunchModelError::CanonicalEncoding {
            reason: error.to_string(),
        })
    }

    /// Validates this claim against trusted boot inputs.
    ///
    /// # Errors
    ///
    /// Returns [`RecoveryRequestError`] when generation, lock, journal, or
    /// supervisor identity differs from the trusted launcher context.
    pub fn validate_context(
        &self,
        recovery_generation: LaunchGeneration,
        policy: &TransitionValidationPolicy,
        supervisor_identity: ProcessSupervisorIdentityV1,
    ) -> Result<(), RecoveryRequestError> {
        if self.recovery_generation != recovery_generation {
            return Err(RecoveryRequestError::GenerationMismatch);
        }
        if self.shell_lock_hash != policy.expected_shell_lock_hash {
            return Err(RecoveryRequestError::ShellLockMismatch);
        }
        if self.confirmed_setting_transaction_revision
            != policy.expected_confirmed_setting_transaction_revision
        {
            return Err(RecoveryRequestError::ConfirmedSettingsRevisionMismatch);
        }
        if self.supervisor_identity != supervisor_identity {
            return Err(RecoveryRequestError::SupervisorMismatch);
        }
        Ok(())
    }

    /// Returns the authoritative generation carried by the recovery claim.
    #[must_use]
    pub const fn recovery_generation(&self) -> LaunchGeneration {
        self.recovery_generation
    }

    /// Returns the failed generation, when an intent authenticated far enough.
    #[must_use]
    pub const fn source_generation(&self) -> Option<LaunchGeneration> {
        self.source_generation
    }

    /// Returns the exact source blob, when one existed.
    #[must_use]
    pub const fn source_blob_hash(&self) -> Option<CanonicalHash> {
        self.source_blob_hash
    }

    /// Returns the stable recovery reason.
    #[must_use]
    pub const fn reason(&self) -> RecoveryReasonV1 {
        self.reason
    }

    /// Returns the exact selected shell lock.
    #[must_use]
    pub const fn shell_lock_hash(&self) -> CanonicalHash {
        self.shell_lock_hash
    }

    /// Returns the confirmed settings journal revision.
    #[must_use]
    pub const fn confirmed_setting_transaction_revision(&self) -> SettingTransactionRevision {
        self.confirmed_setting_transaction_revision
    }

    /// Returns the durable supervisor identity.
    #[must_use]
    pub const fn supervisor_identity(&self) -> ProcessSupervisorIdentityV1 {
        self.supervisor_identity
    }

    /// Returns the primary typed failure shown by recovery.
    #[must_use]
    pub const fn failure(&self) -> LaunchFailureReceiptV1 {
        self.failure
    }

    /// Returns the self-authenticating body checksum acknowledged by the child.
    #[must_use]
    pub const fn checksum(&self) -> CanonicalHash {
        self.checksum
    }

    fn recompute_checksum(&self) -> Result<CanonicalHash, latticeaxiom_core::CanonicalJsonError> {
        let mut value = serde_json::to_value(self)?;
        if let Value::Object(fields) = &mut value {
            fields.remove("checksum");
        }
        canonical_json_hash(&value)
    }
}

/// Versioned acknowledgement of one exact recovery request.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RecoveryBootstrapAckV1 {
    schema_version: u32,
    request_checksum: CanonicalHash,
    process_epoch: ProcessEpoch,
    safe_state: BootstrapSafeStateV1,
}

impl RecoveryBootstrapAckV1 {
    /// Creates an acknowledgement bound to a request and consumed App lease.
    #[must_use]
    #[allow(clippy::needless_pass_by_value)]
    pub const fn for_ready_request(
        request: &RecoveryLaunchRequestV1,
        app_proof: FreshClientAppLeaseProof,
    ) -> Self {
        Self {
            schema_version: RECOVERY_ACK_SCHEMA_VERSION,
            request_checksum: request.checksum(),
            process_epoch: app_proof.process_epoch,
            safe_state: BootstrapSafeStateV1::RecoveryReady,
        }
    }

    /// Returns the acknowledged process epoch.
    #[must_use]
    pub const fn process_epoch(self) -> ProcessEpoch {
        self.process_epoch
    }

    pub(crate) fn matches_request(self, request: &RecoveryLaunchRequestV1) -> bool {
        self.schema_version == RECOVERY_ACK_SCHEMA_VERSION
            && self.safe_state == BootstrapSafeStateV1::RecoveryReady
            && request.checksum() == self.request_checksum
    }
}

/// External process request. Recovery requests cannot carry or retry a world intent.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ProcessLaunchRequestV1 {
    /// Boot the exact claimed target intent.
    Intent(LaunchIntentV1),
    /// Boot one exact package-minimal recovery request.
    RecoveryShell(RecoveryLaunchRequestV1),
}

/// Result observed while waiting for a child bootstrap acknowledgement.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BootObservationV1 {
    /// The child reached the safe state for its exact intent.
    Acknowledged(BootstrapAckV1),
    /// A recovery child acknowledged its exact versioned request.
    RecoveryAcknowledged(RecoveryBootstrapAckV1),
    /// The child rejected its locks or bootstrap inputs.
    BootFailed,
    /// The child exited before acknowledgement.
    Crashed {
        /// Stable numeric exit code when the platform supplied one.
        exit_code: Option<i32>,
    },
    /// The bounded acknowledgement deadline elapsed.
    TimedOut,
}

/// Terminal outcome of one bounded bootstrap attempt.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case", tag = "outcome")]
pub enum TransitionOutcomeV1 {
    /// The intended shell or world reached safe bootstrap and was consumed.
    Activated {
        /// Activated generation.
        generation: LaunchGeneration,
        /// Activated role.
        target: LaunchTargetV1,
        /// Child process epoch.
        process_epoch: ProcessEpoch,
    },
    /// One recovery shell reached its safe bootstrap state.
    RecoveryShell {
        /// Failed generation, if known.
        source_generation: Option<LaunchGeneration>,
        /// Recovery process epoch.
        process_epoch: ProcessEpoch,
        /// Stable reason shown by recovery.
        reason: RecoveryReasonV1,
    },
    /// Barrier failure occurred before destructive work; current App remains.
    CurrentProcessRetained,
    /// A recovery request was durably published and the current process must exit.
    RecoveryHandoffPublished {
        /// Generation retained by the recovery claim.
        generation: LaunchGeneration,
        /// Authenticated recovery-request checksum.
        checksum: CanonicalHash,
    },
    /// The intent was published and the current process must exit.
    HandoffPublished {
        /// Published generation.
        generation: LaunchGeneration,
        /// Published checksum.
        checksum: CanonicalHash,
    },
    /// Automatic restart stopped to prevent a recovery loop.
    Halted,
}

/// Bounded report containing at most four ordered phase failures.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BootstrapReportV1 {
    schema_version: u32,
    outcome: TransitionOutcomeV1,
    failures: Vec<LaunchFailureReceiptV1>,
}

impl BootstrapReportV1 {
    pub(crate) fn new(
        outcome: TransitionOutcomeV1,
        mut failures: Vec<LaunchFailureReceiptV1>,
    ) -> Self {
        failures.truncate(4);
        Self {
            schema_version: LAUNCH_RECEIPT_SCHEMA_VERSION,
            outcome,
            failures,
        }
    }

    /// Returns the terminal outcome.
    #[must_use]
    pub const fn outcome(&self) -> TransitionOutcomeV1 {
        self.outcome
    }

    /// Returns deterministic failures in occurrence order.
    #[must_use]
    pub fn failures(&self) -> &[LaunchFailureReceiptV1] {
        &self.failures
    }
}

/// Error claiming the process-global fresh client-App lease.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum ClientAppLeaseError {
    /// This operating-system process already claimed its one client App.
    #[error("this process already claimed its fresh client DefaultPlugins/EventLoop lease")]
    AlreadyClaimed,
}

/// Linear token authorizing one fresh client `DefaultPlugins`/`EventLoop` App.
///
/// The token is intentionally neither `Clone` nor serializable. The Bevy host
/// must consume it when constructing the client App. Headless `MinimalPlugins`
/// instances do not use this token.
#[derive(Debug)]
pub struct FreshClientAppLeaseToken {
    process_epoch: ProcessEpoch,
}

impl FreshClientAppLeaseToken {
    /// Returns the external process epoch bound to this token.
    #[must_use]
    pub const fn process_epoch(&self) -> ProcessEpoch {
        self.process_epoch
    }

    /// Consumes the lease after the host created its only fresh client App.
    ///
    /// The resulting linear proof is required to construct either bootstrap
    /// acknowledgement and cannot be cloned or deserialized.
    #[must_use]
    pub const fn into_app_created_proof(self) -> FreshClientAppLeaseProof {
        FreshClientAppLeaseProof {
            process_epoch: self.process_epoch,
        }
    }
}

/// Linear evidence that the process-global fresh client-App lease was consumed.
#[derive(Debug)]
pub struct FreshClientAppLeaseProof {
    process_epoch: ProcessEpoch,
}

impl FreshClientAppLeaseProof {
    /// Returns the process epoch bound to the consumed lease.
    #[must_use]
    pub const fn process_epoch(&self) -> ProcessEpoch {
        self.process_epoch
    }
}

#[cfg(test)]
pub(crate) const fn test_app_created_proof(
    process_epoch: ProcessEpoch,
) -> FreshClientAppLeaseProof {
    FreshClientAppLeaseProof { process_epoch }
}

static CLIENT_APP_LEASE_CLAIMED: AtomicBool = AtomicBool::new(false);

/// Claims this operating-system process's one fresh client App lease.
///
/// # Errors
///
/// Returns [`ClientAppLeaseError::AlreadyClaimed`] after the first successful
/// claim in this process. The claim is never reset; switching shell/world
/// therefore requires process replacement.
pub fn claim_fresh_client_app_lease(
    process_epoch: ProcessEpoch,
) -> Result<FreshClientAppLeaseToken, ClientAppLeaseError> {
    CLIENT_APP_LEASE_CLAIMED
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .map_err(|_| ClientAppLeaseError::AlreadyClaimed)?;
    Ok(FreshClientAppLeaseToken { process_epoch })
}

fn validate_draft(draft: &LaunchIntentDraftV1) -> Result<(), LaunchModelError> {
    if draft.attempt != LaunchAttempt::FIRST {
        return Err(LaunchModelError::InvalidAttempt {
            actual: draft.attempt.get(),
        });
    }
    let lifetime = draft.expires_at_ms.checked_sub(draft.issued_at_ms).ok_or(
        LaunchModelError::InvalidLifetime {
            issued_at_ms: draft.issued_at_ms,
            expires_at_ms: draft.expires_at_ms,
        },
    )?;
    if lifetime > MAX_LAUNCH_INTENT_LIFETIME_MS {
        return Err(LaunchModelError::LifetimeExceeded {
            actual_ms: lifetime,
            maximum_ms: MAX_LAUNCH_INTENT_LIFETIME_MS,
        });
    }
    if !target_hashes_are_valid(
        draft.target,
        draft.world_lock_hash,
        draft.world_open_plan_hash,
    ) {
        return Err(LaunchModelError::InvalidTargetHashes {
            target: draft.target,
        });
    }
    Ok(())
}

fn target_hashes_are_valid(
    target: LaunchTargetV1,
    world_lock_hash: Option<CanonicalHash>,
    world_open_plan_hash: Option<CanonicalHash>,
) -> bool {
    match target {
        LaunchTargetV1::Shell => world_lock_hash.is_none() && world_open_plan_hash.is_none(),
        LaunchTargetV1::World { .. } => world_lock_hash.is_some() && world_open_plan_hash.is_some(),
    }
}

fn validate_lifetime(
    issued_at_ms: u64,
    expires_at_ms: u64,
    now_ms: u64,
) -> Result<(), LaunchIntentError> {
    let Some(lifetime) = expires_at_ms.checked_sub(issued_at_ms) else {
        return Err(LaunchIntentError::InvalidLifetime);
    };
    if lifetime > MAX_LAUNCH_INTENT_LIFETIME_MS {
        return Err(LaunchIntentError::InvalidLifetime);
    }
    let maximum_issue = now_ms.saturating_add(MAX_LAUNCH_CLOCK_SKEW_MS);
    if issued_at_ms > maximum_issue {
        return Err(LaunchIntentError::NotYetValid {
            issued_at_ms,
            now_ms,
            allowed_skew_ms: MAX_LAUNCH_CLOCK_SKEW_MS,
        });
    }
    if now_ms > expires_at_ms {
        return Err(LaunchIntentError::Expired {
            expires_at_ms,
            now_ms,
        });
    }
    Ok(())
}

fn bounded_parser_reason(reason: &str) -> String {
    const MAX_CHARS: usize = 256;
    reason.chars().take(MAX_CHARS).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn generation(value: u64) -> LaunchGeneration {
        LaunchGeneration::new(value)
            .unwrap_or_else(|error| panic!("test generation must be non-zero: {error}"))
    }

    fn epoch(value: u64) -> ProcessEpoch {
        ProcessEpoch::new(value)
            .unwrap_or_else(|error| panic!("test process epoch must be non-zero: {error}"))
    }

    const fn setting_revision(value: u64) -> SettingTransactionRevision {
        SettingTransactionRevision::new(value)
    }

    fn shell_intent() -> LaunchIntentV1 {
        LaunchIntentV1::seal(LaunchIntentDraftV1 {
            generation: generation(2),
            attempt: LaunchAttempt::FIRST,
            issued_at_ms: 1_000,
            expires_at_ms: 2_000,
            target: LaunchTargetV1::Shell,
            shell_lock_hash: CanonicalHash::digest(b"shell"),
            world_lock_hash: None,
            world_open_plan_hash: None,
            confirmed_setting_transaction_revision: setting_revision(7),
        })
        .unwrap_or_else(|error| panic!("valid intent was rejected: {error}"))
    }

    #[test]
    fn intent_round_trips_exact_canonical_bytes() {
        let intent = shell_intent();
        let bytes = intent
            .canonical_bytes()
            .unwrap_or_else(|error| panic!("intent did not encode: {error}"));
        let policy = TransitionValidationPolicy::for_shell(
            1_500,
            CanonicalHash::digest(b"shell"),
            setting_revision(7),
        );
        assert_eq!(
            LaunchIntentV1::decode_and_validate(&bytes, generation(2), &policy).ok(),
            Some(intent)
        );
    }

    #[test]
    fn canonical_shell_intent_matches_the_v1_golden_vector() {
        let bytes = shell_intent()
            .canonical_bytes()
            .unwrap_or_else(|error| panic!("intent did not encode: {error}"));
        let expected = br#"{"attempt":1,"checksum":"d0422e20de5cb464cd46991d062d02ac221bcf78cd1db916c50fc8077c1864aa","confirmed_setting_transaction_revision":7,"expires_at_ms":2000,"generation":2,"issued_at_ms":1000,"schema_version":1,"shell_lock_hash":"ce635c4eabff5e4f56dba8fb1e39ca235530aa2b6b18533eef1af3862016c577","target":{"kind":"shell"},"world_lock_hash":null,"world_open_plan_hash":null}"#;
        assert_eq!(bytes.as_slice(), expected);
    }

    #[test]
    fn whitespace_and_unknown_fields_are_rejected() {
        let intent = shell_intent();
        let mut whitespace = intent
            .canonical_bytes()
            .unwrap_or_else(|error| panic!("intent did not encode: {error}"));
        whitespace.push(b'\n');
        let policy = TransitionValidationPolicy::for_shell(
            1_500,
            CanonicalHash::digest(b"shell"),
            setting_revision(7),
        );
        assert_eq!(
            LaunchIntentV1::decode_and_validate(&whitespace, generation(2), &policy),
            Err(LaunchIntentError::NonCanonicalBytes)
        );

        let unknown = br#"{"attempt":1,"checksum":"e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855","confirmed_setting_transaction_revision":7,"expires_at_ms":2000,"generation":2,"issued_at_ms":1000,"schema_version":1,"shell_lock_hash":"e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855","target":{"kind":"shell"},"unknown":true,"world_lock_hash":null,"world_open_plan_hash":null}"#;
        assert!(matches!(
            LaunchIntentV1::decode_and_validate(unknown, generation(2), &policy),
            Err(LaunchIntentError::InvalidJson { .. })
        ));
    }

    #[test]
    fn checksum_staleness_expiry_and_selection_are_independent_rejections() {
        let intent = shell_intent();
        let bytes = intent
            .canonical_bytes()
            .unwrap_or_else(|error| panic!("intent did not encode: {error}"));
        let stale = TransitionValidationPolicy::for_shell(
            1_500,
            CanonicalHash::digest(b"shell"),
            setting_revision(7),
        );
        assert!(matches!(
            LaunchIntentV1::decode_and_validate(&bytes, generation(3), &stale),
            Err(LaunchIntentError::StaleGeneration { .. })
        ));

        let expired = TransitionValidationPolicy::for_shell(
            2_001,
            CanonicalHash::digest(b"shell"),
            setting_revision(7),
        );
        assert!(matches!(
            LaunchIntentV1::decode_and_validate(&bytes, generation(2), &expired),
            Err(LaunchIntentError::Expired { .. })
        ));

        let wrong_lock = TransitionValidationPolicy::for_shell(
            1_500,
            CanonicalHash::digest(b"other"),
            setting_revision(7),
        );
        assert_eq!(
            LaunchIntentV1::decode_and_validate(&bytes, generation(2), &wrong_lock),
            Err(LaunchIntentError::ShellLockMismatch)
        );

        let wrong_revision = TransitionValidationPolicy::for_shell(
            1_500,
            CanonicalHash::digest(b"shell"),
            setting_revision(8),
        );
        assert_eq!(
            LaunchIntentV1::decode_and_validate(&bytes, generation(2), &wrong_revision),
            Err(LaunchIntentError::ConfirmedSettingsRevisionMismatch)
        );

        let generation_gap = TransitionValidationPolicy::for_shell(
            1_500,
            CanonicalHash::digest(b"shell"),
            setting_revision(7),
        );
        assert_eq!(
            LaunchIntentV1::decode_and_validate(&bytes, generation(1), &generation_gap),
            Err(LaunchIntentError::GenerationGap {
                actual: generation(2),
                expected: generation(1),
            })
        );

        let mut value: Value = serde_json::from_slice(&bytes)
            .unwrap_or_else(|error| panic!("canonical fixture did not parse: {error}"));
        if let Value::Object(fields) = &mut value {
            fields.insert(
                "confirmed_setting_transaction_revision".to_owned(),
                8.into(),
            );
        }
        let corrupt = canonical_json_bytes(&value)
            .unwrap_or_else(|error| panic!("mutated fixture did not encode: {error}"));
        let valid = TransitionValidationPolicy::for_shell(
            1_500,
            CanonicalHash::digest(b"shell"),
            setting_revision(7),
        );
        assert!(matches!(
            LaunchIntentV1::decode_and_validate(&corrupt, generation(2), &valid),
            Err(LaunchIntentError::ChecksumMismatch { .. })
        ));
    }

    #[test]
    fn world_target_requires_both_preflight_hashes() {
        let world_id = WorldId::new_v4();
        let invalid = LaunchIntentV1::seal(LaunchIntentDraftV1 {
            generation: generation(1),
            attempt: LaunchAttempt::FIRST,
            issued_at_ms: 1,
            expires_at_ms: 2,
            target: LaunchTargetV1::World { world_id },
            shell_lock_hash: CanonicalHash::digest(b"shell"),
            world_lock_hash: Some(CanonicalHash::digest(b"world")),
            world_open_plan_hash: None,
            confirmed_setting_transaction_revision: setting_revision(0),
        });
        assert!(matches!(
            invalid,
            Err(LaunchModelError::InvalidTargetHashes { .. })
        ));
    }

    #[test]
    fn typed_counters_reject_zero_and_overflow() {
        assert!(LaunchGeneration::new(0).is_err());
        assert!(ProcessEpoch::new(0).is_err());
        assert!(LaunchAttempt::new(0).is_err());
        assert!(generation(u64::MAX).next().is_err());
        assert_eq!(epoch(9).get(), 9);
    }

    #[test]
    fn oversized_intent_is_rejected_before_parsing() {
        let bytes = vec![b' '; MAX_LAUNCH_INTENT_BYTES.saturating_add(1)];
        let policy = TransitionValidationPolicy::for_shell(
            1,
            CanonicalHash::digest(b"shell"),
            setting_revision(0),
        );
        assert!(matches!(
            LaunchIntentV1::decode_and_validate(&bytes, generation(2), &policy),
            Err(LaunchIntentError::InputTooLarge { .. })
        ));
    }

    #[test]
    fn client_app_lease_is_process_global_and_single_use() {
        let first = claim_fresh_client_app_lease(epoch(50));
        assert!(first.is_ok());
        assert_eq!(
            claim_fresh_client_app_lease(epoch(51)).err(),
            Some(ClientAppLeaseError::AlreadyClaimed)
        );
    }
}
