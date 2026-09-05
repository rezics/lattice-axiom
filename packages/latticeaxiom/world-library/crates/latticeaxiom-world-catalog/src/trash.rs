use std::{collections::BTreeSet, fmt};

use latticeaxiom_core::{CanonicalHash, WorldId};
use serde::{Deserialize, Deserializer, Serialize, Serializer, de};
use thiserror::Error;

use crate::{CheckpointId, DisplayName, LiveWorldLocation, WorldRootId};

/// First-version managed-trash policy never purges automatically.
pub const AUTO_PURGE_ENABLED: bool = false;
const MAX_TRASH_ENTRY_ID_BYTES: usize = 128;

/// Stable bounded identifier for one managed-trash entry.
#[repr(transparent)]
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct TrashEntryId(String);

impl TrashEntryId {
    /// Validates a path-independent trash entry token.
    ///
    /// # Errors
    ///
    /// Returns [`TrashEntryIdError`] for an empty, oversized, or non-token
    /// value. Tokens contain only ASCII alphanumeric characters, `-`, and `_`.
    pub fn new(value: &str) -> Result<Self, TrashEntryIdError> {
        if value.is_empty() {
            return Err(TrashEntryIdError::Empty);
        }
        if value.len() > MAX_TRASH_ENTRY_ID_BYTES {
            return Err(TrashEntryIdError::TooLong {
                actual: value.len(),
            });
        }
        if value
            .bytes()
            .any(|byte| !byte.is_ascii_alphanumeric() && !matches!(byte, b'-' | b'_'))
        {
            return Err(TrashEntryIdError::ForbiddenCharacter);
        }
        Ok(Self(value.to_owned()))
    }

    /// Returns the opaque token.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for TrashEntryId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl Serialize for TrashEntryId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for TrashEntryId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::new(&value).map_err(de::Error::custom)
    }
}

/// Failure to validate a managed-trash entry ID.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum TrashEntryIdError {
    /// The token is empty.
    #[error("a trash entry ID must not be empty")]
    Empty,
    /// The token exceeds the fixed bound.
    #[error("a trash entry ID has {actual} bytes; the limit is 128")]
    TooLong {
        /// Observed byte length.
        actual: usize,
    },
    /// The token could participate in path interpretation.
    #[error("a trash entry ID contains a forbidden character")]
    ForbiddenCharacter,
}

/// Structural locator for `<root>/.trash/<WorldId>/<trash-entry-id>`.
///
/// The host resolves this value beneath an allowlisted root without accepting
/// an unchecked literal path from package or UI code.
#[derive(Clone, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ManagedTrashLocation {
    /// Root whose same-filesystem managed trash contains the entry.
    pub root: WorldRootId,
    /// Original world identity represented by the first directory segment.
    pub world_id: WorldId,
    /// Unique deletion event represented by the second segment.
    pub entry_id: TrashEntryId,
}

/// Retention is visible metadata, never an implicit permission to purge.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum TrashRetentionPolicy {
    /// Retain until an explicit, disclosed permanent purge.
    ManualPurgeOnly,
}

/// Bounded tombstone stored with a managed-trash entry.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TrashTombstone {
    /// Original allowlisted root.
    pub original_root: WorldRootId,
    /// Immutable identity before deletion.
    pub world_id: WorldId,
    /// User-facing name at deletion time.
    pub display_name: DisplayName,
    /// Milliseconds since Unix epoch when the entry was moved.
    pub deleted_at_ms: u64,
    /// Verified sidecar projection hash.
    pub header_checksum: CanonicalHash,
    /// Verified authoritative metadata body hash.
    pub metadata_checksum: CanonicalHash,
    /// Bounded physical size estimate shown before purge.
    pub physical_bytes: u64,
    /// Last checkpoint available at deletion time.
    pub last_checkpoint: Option<CheckpointId>,
    /// Explicit retention contract.
    pub retention: TrashRetentionPolicy,
}

/// Writer lifecycle evidence required before moving a live world.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WriterBarrier {
    /// A writer or authoritative tasks still exist.
    Open,
    /// The writer is closed but bounded durability tasks have not drained.
    ClosedNotDrained,
    /// The writer is closed and all bounded tasks have joined and drained.
    ClosedAndDrained,
}

/// Pure-data transport selected for a move to managed trash.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum TrashMoveTransport {
    /// Same-volume atomic rename into the managed trash tree.
    SameVolumeAtomicRename,
    /// Temporary copy, fsync, checksum, preflight, publish, then source removal.
    VerifiedCopyThenRemove,
}

/// Non-executing plan for moving a live world into managed trash.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MoveToTrashPlan {
    /// Existing live-world location.
    pub source: LiveWorldLocation,
    /// Structural managed-trash destination.
    pub destination: ManagedTrashLocation,
    /// Required transport protocol.
    pub transport: TrashMoveTransport,
}

impl MoveToTrashPlan {
    /// Forms a plan only after the unique writer and tasks have drained.
    ///
    /// # Errors
    ///
    /// Returns [`TrashPlanError`] when the barrier is incomplete or the
    /// destination encodes a different world identity.
    pub fn new(
        source: LiveWorldLocation,
        destination: ManagedTrashLocation,
        writer: WriterBarrier,
        same_volume: bool,
    ) -> Result<Self, TrashPlanError> {
        if writer != WriterBarrier::ClosedAndDrained {
            return Err(TrashPlanError::WriterNotDrained);
        }
        if source.world_id != destination.world_id {
            return Err(TrashPlanError::IdentityMismatch);
        }
        Ok(Self {
            source,
            destination,
            transport: if same_volume {
                TrashMoveTransport::SameVolumeAtomicRename
            } else {
                TrashMoveTransport::VerifiedCopyThenRemove
            },
        })
    }
}

/// Restore identity choice presented explicitly to the player.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RestoreMode {
    /// Restore the immutable original identity; blocked on a live-ID conflict.
    OriginalIdentity,
    /// Restore as a real clone with a caller-generated fresh `UUIDv4`.
    AsClone {
        /// Fresh world identity used while re-keying every world key.
        new_world_id: WorldId,
    },
}

/// Non-executing managed-trash restore plan.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RestorePlan {
    /// Managed-trash source.
    pub source: ManagedTrashLocation,
    /// Target allowlisted root.
    pub target_root: WorldRootId,
    /// Identity of the published world.
    pub target_world_id: WorldId,
    /// Whether every storage key must be re-keyed for clone identity.
    pub rekey_world_prefix: bool,
    /// Original identity retained as clone provenance.
    pub source_world_id: WorldId,
    /// `false` by contract: restore never overwrites a live world.
    pub overwrite_existing: bool,
}

/// Result of pure restore conflict planning.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RestorePlanningOutcome {
    /// Restore may proceed through an external transactional executor.
    Ready(RestorePlan),
    /// Original identity already exists and must not be overwritten.
    BlockedWorldIdConflict {
        /// Conflicting live world identity.
        world_id: WorldId,
    },
    /// A requested clone ID is not fresh.
    BlockedCloneIdentity {
        /// Rejected clone identity.
        world_id: WorldId,
    },
    /// Tombstone and managed-trash path disagree about identity.
    BlockedTombstoneMismatch,
}

/// Plans restore without consulting display-name uniqueness.
///
/// Display-name conflicts are intentionally allowed. Only immutable live
/// `WorldId` conflicts block restoration.
#[must_use]
pub fn plan_restore(
    source: ManagedTrashLocation,
    tombstone: &TrashTombstone,
    target_root: WorldRootId,
    live_world_ids: &BTreeSet<WorldId>,
    mode: RestoreMode,
) -> RestorePlanningOutcome {
    if source.world_id != tombstone.world_id {
        return RestorePlanningOutcome::BlockedTombstoneMismatch;
    }
    match mode {
        RestoreMode::OriginalIdentity => {
            if live_world_ids.contains(&source.world_id) {
                RestorePlanningOutcome::BlockedWorldIdConflict {
                    world_id: source.world_id,
                }
            } else {
                RestorePlanningOutcome::Ready(RestorePlan {
                    source,
                    target_root,
                    target_world_id: tombstone.world_id,
                    rekey_world_prefix: false,
                    source_world_id: tombstone.world_id,
                    overwrite_existing: false,
                })
            }
        }
        RestoreMode::AsClone { new_world_id } => {
            if new_world_id == source.world_id || live_world_ids.contains(&new_world_id) {
                RestorePlanningOutcome::BlockedCloneIdentity {
                    world_id: new_world_id,
                }
            } else {
                RestorePlanningOutcome::Ready(RestorePlan {
                    source,
                    target_root,
                    target_world_id: new_world_id,
                    rekey_world_prefix: true,
                    source_world_id: tombstone.world_id,
                    overwrite_existing: false,
                })
            }
        }
    }
}

/// Failure to form a managed-trash move plan.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum TrashPlanError {
    /// Unique writer or bounded tasks have not fully drained.
    #[error("move to trash requires a closed writer and drained tasks")]
    WriterNotDrained,
    /// Destination path structure encodes another world identity.
    #[error("managed-trash destination identity does not match the source world")]
    IdentityMismatch,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trash_entry_id_is_a_literal_path_segment() {
        assert!(TrashEntryId::new("deletion_1-A").is_ok());
        for invalid in [".", "..", "a.b", "a:b", "a/b", "a\\b", "a b", "é"] {
            assert_eq!(
                TrashEntryId::new(invalid),
                Err(TrashEntryIdError::ForbiddenCharacter),
                "invalid token {invalid:?} unexpectedly passed"
            );
        }
    }

    #[test]
    fn restore_blocks_live_identity_but_clone_rekeys_without_overwrite() {
        let original = world("123e4567-e89b-42d3-a456-426614174000");
        let clone = world("123e4567-e89b-42d3-b456-426614174000");
        let source = ManagedTrashLocation {
            root: WorldRootId(1),
            world_id: original,
            entry_id: TrashEntryId::new("deletion-1").unwrap_or_else(|error| panic!("{error}")),
        };
        let tombstone = tombstone(original);
        let live = BTreeSet::from([original]);

        assert!(matches!(
            plan_restore(
                source.clone(),
                &tombstone,
                WorldRootId(1),
                &live,
                RestoreMode::OriginalIdentity
            ),
            RestorePlanningOutcome::BlockedWorldIdConflict { .. }
        ));
        let restored = plan_restore(
            source,
            &tombstone,
            WorldRootId(1),
            &live,
            RestoreMode::AsClone {
                new_world_id: clone,
            },
        );
        let RestorePlanningOutcome::Ready(plan) = restored else {
            panic!("restore-as-clone unexpectedly blocked");
        };
        assert!(plan.rekey_world_prefix);
        assert!(!plan.overwrite_existing);
        assert_eq!(plan.target_world_id, clone);
    }

    fn world(value: &str) -> WorldId {
        value
            .parse()
            .unwrap_or_else(|error| panic!("fixture world ID: {error}"))
    }

    fn tombstone(world_id: WorldId) -> TrashTombstone {
        TrashTombstone {
            original_root: WorldRootId(1),
            world_id,
            display_name: DisplayName::new("Same Name").unwrap_or_else(|error| panic!("{error}")),
            deleted_at_ms: 1,
            header_checksum: CanonicalHash::digest(b"header"),
            metadata_checksum: CanonicalHash::digest(b"metadata"),
            physical_bytes: 42,
            last_checkpoint: None,
            retention: TrashRetentionPolicy::ManualPurgeOnly,
        }
    }
}
