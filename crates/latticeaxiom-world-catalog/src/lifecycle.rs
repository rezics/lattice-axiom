//! Create, checkpoint, clone, and export plans that never open a writer.
//!
//! Hosts execute accepted plans through separately reviewed storage
//! boundaries. This module only validates identity, equivalence, disk
//! admission, and writer-drain evidence.

use std::collections::BTreeSet;

use latticeaxiom_core::{CanonicalHash, PackageName, StableId, WorldId};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{
    CheckpointId, DiskAdmission, DisplayName, LiveWorldLocation, StorageOperation, WorldRootId,
    WriterBarrier,
};

/// Catalog-visible crash marker derived from the bounded header clean flag.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum CrashMarker {
    /// Last shutdown reached the clean marker.
    #[default]
    Absent,
    /// Unclean shutdown or an explicit crash marker is present.
    Present,
}

impl CrashMarker {
    /// Derives the marker from the header clean-shutdown projection.
    #[must_use]
    pub const fn from_clean_shutdown(clean_shutdown: bool) -> Self {
        if clean_shutdown {
            Self::Absent
        } else {
            Self::Present
        }
    }
}

/// Process-local exclusive writer lease observed without opening a writer.
///
/// Clone, export, and restore never copy this identity. A held or stale lease
/// is not [`crate::WorldOpenStatus::ReadyExact`].
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum WriterLeaseState {
    /// No exclusive writer lease is recorded.
    #[default]
    Absent,
    /// Another live process still holds the exclusive writer.
    Held,
    /// A crash or abandoned process left a lease without a live writer.
    Stale,
}

/// Retention class used when planning a checkpoint.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum CheckpointClass {
    /// Initial, last-known-clean, pre-migration, pre-graph-change, or pinned.
    Protected,
    /// Automatically rotated checkpoint.
    RotatingAutomatic,
}

/// Why a catalog checkpoint is being planned.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum CheckpointReason {
    /// First durable point of a newly published world.
    Initial,
    /// Latest known clean shutdown.
    LatestKnownClean,
    /// Required before staged migration.
    PreMigration,
    /// Required before an explicit graph change.
    PreGraphChange,
    /// Player-pinned manual checkpoint.
    ManualPinned,
    /// Rotating automatic checkpoint.
    RotatingAutomatic,
}

impl CheckpointReason {
    /// Returns the retention class for this reason.
    #[must_use]
    pub const fn class(self) -> CheckpointClass {
        match self {
            Self::RotatingAutomatic => CheckpointClass::RotatingAutomatic,
            Self::Initial
            | Self::LatestKnownClean
            | Self::PreMigration
            | Self::PreGraphChange
            | Self::ManualPinned => CheckpointClass::Protected,
        }
    }
}

/// Fingerprint that makes two checkpoints equivalent under WORLD-15.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CheckpointFingerprint {
    /// Source world revision captured by the checkpoint.
    pub source_revision: u64,
    /// Exact frozen lock hash.
    pub exact_lock_hash: CanonicalHash,
    /// Authoritative metadata hash.
    pub metadata_hash: CanonicalHash,
}

/// One already-retained checkpoint used for equivalence and clone gates.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RecordedCheckpoint {
    /// Stable checkpoint identity.
    pub id: CheckpointId,
    /// Equivalence fingerprint.
    pub fingerprint: CheckpointFingerprint,
    /// Retention class.
    pub class: CheckpointClass,
    /// Whether an independent restore verification receipt exists.
    pub restore_verified: bool,
}

/// Read-only evidence required to plan checkpoint, clone, or export.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WorldLifecycleEvidence {
    /// Current source fingerprint.
    pub fingerprint: CheckpointFingerprint,
    /// Already retained checkpoints in stable identity order.
    pub checkpoints: Vec<RecordedCheckpoint>,
    /// Bounded header checksum.
    pub header_checksum: CanonicalHash,
    /// Authoritative metadata checksum.
    pub metadata_checksum: CanonicalHash,
    /// Crash marker derived from the clean-shutdown projection.
    pub crash_marker: CrashMarker,
    /// Exclusive writer lease observed from catalog/sidecar evidence.
    #[serde(default)]
    pub lease: WriterLeaseState,
}

/// Immutable create plan; publishing a sidecar or opening a writer is external.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CreateWorldPlan {
    /// Fresh `UUIDv4` identity.
    pub world_id: WorldId,
    /// Allowlisted structural location.
    pub location: LiveWorldLocation,
    /// Validated display name.
    pub display_name: DisplayName,
    /// Shell-visible template identity.
    pub template: StableId,
    /// Root game package resolved during create.
    pub root_game_package: PackageName,
    /// Profile lock whose safe defaults were summarized.
    pub profile_lock: CanonicalHash,
    /// Optional stable generation profile selected during creation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub generation_profile: Option<StableId>,
}

/// Validated inputs used to form an immutable world creation plan.
///
/// Grouping the values keeps the planning boundary explicit as creation gains
/// independently versioned options such as terrain profiles.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CreateWorldPlanInput {
    /// Fresh `UUIDv4` identity.
    pub world_id: WorldId,
    /// Allowlisted storage root.
    pub root: WorldRootId,
    /// Validated player-visible name.
    pub display_name: DisplayName,
    /// Shell-visible template identity.
    pub template: StableId,
    /// Root game package resolved during create.
    pub root_game_package: PackageName,
    /// Profile lock whose safe defaults were summarized.
    pub profile_lock: CanonicalHash,
    /// Optional stable generation profile selected during creation.
    pub generation_profile: Option<StableId>,
}

/// Failure to form a create plan.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum CreateWorldPlanError {
    /// The requested identity already exists in the live catalog.
    #[error("create world identity already exists")]
    DuplicateWorldId,
}

/// Plans a new world identity without opening a writer or copying a directory.
///
/// # Errors
///
/// Returns [`CreateWorldPlanError::DuplicateWorldId`] when `world_id` is live.
pub fn plan_create_world(
    input: CreateWorldPlanInput,
    live_world_ids: &BTreeSet<WorldId>,
) -> Result<CreateWorldPlan, CreateWorldPlanError> {
    if live_world_ids.contains(&input.world_id) {
        return Err(CreateWorldPlanError::DuplicateWorldId);
    }
    Ok(CreateWorldPlan {
        world_id: input.world_id,
        location: LiveWorldLocation::new(input.root, input.world_id),
        display_name: input.display_name,
        template: input.template,
        root_game_package: input.root_game_package,
        profile_lock: input.profile_lock,
        generation_profile: input.generation_profile,
    })
}

/// Immutable checkpoint plan; the storage crate creates the independent image.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CheckpointPlan {
    /// World being checkpointed.
    pub world_id: WorldId,
    /// Live catalog location.
    pub location: LiveWorldLocation,
    /// Newly allocated checkpoint identity.
    pub checkpoint_id: CheckpointId,
    /// Player- or system-visible reason.
    pub reason: CheckpointReason,
    /// Retention class derived from the reason.
    pub class: CheckpointClass,
    /// Fingerprint that must be unique among retained checkpoints.
    pub fingerprint: CheckpointFingerprint,
}

/// Failure to form a checkpoint plan.
#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum CheckpointPlanError {
    /// Unique writer or bounded tasks have not fully drained.
    #[error("checkpoint requires a closed writer and drained tasks")]
    WriterNotDrained,
    /// Disk admission rejects this checkpoint class.
    #[error("checkpoint is not admitted under the current disk policy")]
    DiskDenied,
    /// An equivalent checkpoint already exists.
    #[error("an equivalent checkpoint already exists as {existing}")]
    EquivalentExists {
        /// Existing equivalent checkpoint.
        existing: CheckpointId,
    },
    /// The requested identity is already retained.
    #[error("checkpoint identity already exists")]
    DuplicateCheckpointId,
}

/// Plans a checkpoint from catalog evidence without opening a writer.
///
/// # Errors
///
/// Returns [`CheckpointPlanError`] when the writer is open, disk admission
/// rejects the class, or an equivalent checkpoint already exists.
pub fn plan_checkpoint(
    location: LiveWorldLocation,
    checkpoint_id: CheckpointId,
    reason: CheckpointReason,
    evidence: &WorldLifecycleEvidence,
    existing: &[RecordedCheckpoint],
    writer: WriterBarrier,
    disk: DiskAdmission,
) -> Result<CheckpointPlan, CheckpointPlanError> {
    if writer != WriterBarrier::ClosedAndDrained {
        return Err(CheckpointPlanError::WriterNotDrained);
    }
    let class = reason.class();
    let operation = match class {
        CheckpointClass::Protected => StorageOperation::AuthoritativeMutation,
        CheckpointClass::RotatingAutomatic => StorageOperation::AutomaticCheckpoint,
    };
    if !disk.allows(operation) {
        return Err(CheckpointPlanError::DiskDenied);
    }
    if let Some(existing) = existing
        .iter()
        .find(|checkpoint| checkpoint.fingerprint == evidence.fingerprint)
    {
        return Err(CheckpointPlanError::EquivalentExists {
            existing: existing.id.clone(),
        });
    }
    if existing
        .iter()
        .any(|checkpoint| checkpoint.id == checkpoint_id)
    {
        return Err(CheckpointPlanError::DuplicateCheckpointId);
    }
    Ok(CheckpointPlan {
        world_id: location.world_id,
        location,
        checkpoint_id,
        reason,
        class,
        fingerprint: evidence.fingerprint,
    })
}

/// Provenance retained by a clone; process-local leases are never copied.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CloneProvenance {
    /// Source world identity.
    pub source_world_id: WorldId,
    /// Verified source checkpoint.
    pub source_checkpoint_id: CheckpointId,
    /// Source revision captured by that checkpoint.
    pub source_revision: u64,
    /// Source exact lock hash.
    pub source_lock_hash: CanonicalHash,
}

/// Immutable clone plan that assigns a fresh world identity.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ClonePlan {
    /// Source live location.
    pub source: LiveWorldLocation,
    /// Verified checkpoint used as the clone image.
    pub source_checkpoint: CheckpointId,
    /// Fresh `UUIDv4` identity.
    pub new_world_id: WorldId,
    /// Destination structural location.
    pub target: LiveWorldLocation,
    /// Clone provenance retained with the new world.
    pub provenance: CloneProvenance,
    /// `true` because storage keys contain `WorldId`.
    pub rekey_world_prefix: bool,
    /// `false` by contract: process-local leases must not be copied.
    pub copies_process_local_lease: bool,
}

/// Failure to form a clone plan.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum ClonePlanError {
    /// Unique writer or bounded tasks have not fully drained.
    #[error("clone requires a closed writer and drained tasks")]
    WriterNotDrained,
    /// Disk admission rejects clone/migration work.
    #[error("clone is not admitted under the current disk policy")]
    DiskDenied,
    /// The requested clone identity is not fresh.
    #[error("clone world identity is not unique")]
    DuplicateWorldId,
    /// No verified checkpoint exists for the source world.
    #[error("clone requires a restore-verified checkpoint")]
    UnverifiedCheckpoint,
}

/// Plans a clone from a verified checkpoint without opening a writer.
///
/// # Errors
///
/// Returns [`ClonePlanError`] when the writer is open, disk admission rejects
/// clone work, the new identity collides, or the checkpoint is unverified.
pub fn plan_clone_world(
    source: LiveWorldLocation,
    checkpoint: &RecordedCheckpoint,
    new_world_id: WorldId,
    target_root: WorldRootId,
    live_world_ids: &BTreeSet<WorldId>,
    writer: WriterBarrier,
    disk: DiskAdmission,
) -> Result<ClonePlan, ClonePlanError> {
    if writer != WriterBarrier::ClosedAndDrained {
        return Err(ClonePlanError::WriterNotDrained);
    }
    if !disk.allows(StorageOperation::MigrationOrClone) {
        return Err(ClonePlanError::DiskDenied);
    }
    if !checkpoint.restore_verified {
        return Err(ClonePlanError::UnverifiedCheckpoint);
    }
    if new_world_id == source.world_id || live_world_ids.contains(&new_world_id) {
        return Err(ClonePlanError::DuplicateWorldId);
    }
    Ok(ClonePlan {
        source,
        source_checkpoint: checkpoint.id.clone(),
        new_world_id,
        target: LiveWorldLocation::new(target_root, new_world_id),
        provenance: CloneProvenance {
            source_world_id: source.world_id,
            source_checkpoint_id: checkpoint.id.clone(),
            source_revision: checkpoint.fingerprint.source_revision,
            source_lock_hash: checkpoint.fingerprint.exact_lock_hash,
        },
        rekey_world_prefix: true,
        copies_process_local_lease: false,
    })
}

/// Read-only export plan; secrets are never included.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ExportPlan {
    /// Exported world identity.
    pub world_id: WorldId,
    /// Live catalog location.
    pub location: LiveWorldLocation,
    /// Bounded header checksum included in the manifest.
    pub header_checksum: CanonicalHash,
    /// Authoritative metadata checksum included in the manifest.
    pub metadata_checksum: CanonicalHash,
    /// Bounded physical size estimate.
    pub physical_bytes: u64,
    /// `false` by contract: export must not imply secret material.
    pub includes_secrets: bool,
}

/// Failure to form an export plan.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum ExportPlanError {
    /// Read-only export is not admitted.
    #[error("export is not admitted under the current disk policy")]
    DiskDenied,
}

/// Plans a bounded export manifest without opening a writer.
///
/// # Errors
///
/// Returns [`ExportPlanError::DiskDenied`] when read-only work is not admitted.
pub fn plan_export_world(
    location: LiveWorldLocation,
    evidence: &WorldLifecycleEvidence,
    physical_bytes: u64,
    disk: DiskAdmission,
) -> Result<ExportPlan, ExportPlanError> {
    if !disk.allows(StorageOperation::ReadOnly) {
        return Err(ExportPlanError::DiskDenied);
    }
    Ok(ExportPlan {
        world_id: location.world_id,
        location,
        header_checksum: evidence.header_checksum,
        metadata_checksum: evidence.metadata_checksum,
        physical_bytes,
        includes_secrets: false,
    })
}

/// Catalog plan that clears a stale exclusive lease without opening a writer.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct StaleLeaseRecoveryPlan {
    /// World whose abandoned lease may be cleared.
    pub world_id: WorldId,
    /// Live catalog location.
    pub location: LiveWorldLocation,
    /// `false` by contract: recovery never copies a process-local lease.
    pub copies_process_local_lease: bool,
}

/// Failure to form a stale-lease recovery plan.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum StaleLeaseRecoveryError {
    /// Unique writer or bounded tasks have not fully drained.
    #[error("stale-lease recovery requires a closed writer and drained tasks")]
    WriterNotDrained,
    /// No abandoned lease is recorded.
    #[error("writer lease is not stale")]
    LeaseNotStale,
    /// A live process still holds the exclusive writer.
    #[error("a live writer lease cannot be recovered from the catalog")]
    LeaseHeld,
}

/// Plans stale-lease recovery from catalog evidence without opening a writer.
///
/// # Errors
///
/// Returns [`StaleLeaseRecoveryError`] when the writer is not drained, the
/// lease is still held by a live process, or no stale lease exists.
pub fn plan_recover_stale_lease(
    location: LiveWorldLocation,
    evidence: &WorldLifecycleEvidence,
    writer: WriterBarrier,
) -> Result<StaleLeaseRecoveryPlan, StaleLeaseRecoveryError> {
    if writer != WriterBarrier::ClosedAndDrained {
        return Err(StaleLeaseRecoveryError::WriterNotDrained);
    }
    match evidence.lease {
        WriterLeaseState::Stale => Ok(StaleLeaseRecoveryPlan {
            world_id: location.world_id,
            location,
            copies_process_local_lease: false,
        }),
        WriterLeaseState::Held => Err(StaleLeaseRecoveryError::LeaseHeld),
        WriterLeaseState::Absent => Err(StaleLeaseRecoveryError::LeaseNotStale),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{DiskSample, GIB, HeadroomInputs, LowDiskMonitor, WorldRootId};

    #[test]
    fn create_checkpoint_clone_and_export_never_claim_a_writer() {
        let source_world = world("123e4567-e89b-42d3-a456-426614174000");
        let clone_world = world("223e4567-e89b-42d3-a456-426614174000");
        let location = LiveWorldLocation::new(WorldRootId(1), source_world);
        let evidence = evidence();
        let disk = normal_disk();
        let live = BTreeSet::from([source_world]);

        let created = plan_create_world(
            CreateWorldPlanInput {
                world_id: clone_world,
                root: WorldRootId(1),
                display_name: DisplayName::new("Clone").unwrap_or_else(|error| panic!("{error}")),
                template: template(),
                root_game_package: package(),
                profile_lock: CanonicalHash::digest(b"profile"),
                generation_profile: None,
            },
            &live,
        )
        .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(created.world_id, clone_world);
        assert_eq!(
            plan_create_world(
                CreateWorldPlanInput {
                    world_id: source_world,
                    root: WorldRootId(1),
                    display_name: created.display_name.clone(),
                    template: template(),
                    root_game_package: package(),
                    profile_lock: CanonicalHash::digest(b"profile"),
                    generation_profile: None,
                },
                &live,
            ),
            Err(CreateWorldPlanError::DuplicateWorldId)
        );

        let checkpoint = plan_checkpoint(
            location,
            CheckpointId::new("checkpoint-2").unwrap_or_else(|error| panic!("{error}")),
            CheckpointReason::ManualPinned,
            &evidence,
            &evidence.checkpoints,
            WriterBarrier::ClosedAndDrained,
            disk,
        )
        .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(checkpoint.class, CheckpointClass::Protected);
        assert!(matches!(
            plan_checkpoint(
                location,
                CheckpointId::new("checkpoint-3").unwrap_or_else(|error| panic!("{error}")),
                CheckpointReason::ManualPinned,
                &evidence,
                &evidence.checkpoints,
                WriterBarrier::Open,
                disk,
            ),
            Err(CheckpointPlanError::WriterNotDrained)
        ));
        assert!(matches!(
            plan_checkpoint(
                location,
                CheckpointId::new("checkpoint-3").unwrap_or_else(|error| panic!("{error}")),
                CheckpointReason::ManualPinned,
                &WorldLifecycleEvidence {
                    fingerprint: evidence.checkpoints[0].fingerprint,
                    ..evidence.clone()
                },
                &evidence.checkpoints,
                WriterBarrier::ClosedAndDrained,
                disk,
            ),
            Err(CheckpointPlanError::EquivalentExists { .. })
        ));

        let clone = plan_clone_world(
            location,
            &evidence.checkpoints[0],
            clone_world,
            WorldRootId(1),
            &live,
            WriterBarrier::ClosedAndDrained,
            disk,
        )
        .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(clone.new_world_id, clone_world);
        assert!(clone.rekey_world_prefix);
        assert!(!clone.copies_process_local_lease);
        assert_eq!(clone.provenance.source_world_id, source_world);

        let export = plan_export_world(location, &evidence, 4096, disk)
            .unwrap_or_else(|error| panic!("{error}"));
        assert!(!export.includes_secrets);
    }

    #[test]
    fn stale_lease_recovery_never_copies_a_live_lease() {
        let source_world = world("123e4567-e89b-42d3-a456-426614174000");
        let location = LiveWorldLocation::new(WorldRootId(1), source_world);
        let evidence = evidence();
        let stale = plan_recover_stale_lease(
            location,
            &WorldLifecycleEvidence {
                lease: WriterLeaseState::Stale,
                crash_marker: CrashMarker::Present,
                ..evidence.clone()
            },
            WriterBarrier::ClosedAndDrained,
        )
        .unwrap_or_else(|error| panic!("{error}"));
        assert!(!stale.copies_process_local_lease);
        assert_eq!(
            plan_recover_stale_lease(
                location,
                &WorldLifecycleEvidence {
                    lease: WriterLeaseState::Held,
                    ..evidence
                },
                WriterBarrier::ClosedAndDrained,
            ),
            Err(StaleLeaseRecoveryError::LeaseHeld)
        );
    }

    fn world(value: &str) -> WorldId {
        value
            .parse()
            .unwrap_or_else(|error| panic!("fixture world ID: {error}"))
    }

    fn template() -> StableId {
        "latticeaxiom:world-template/safe"
            .parse()
            .unwrap_or_else(|error| panic!("template fixture: {error}"))
    }

    fn package() -> PackageName {
        "@example/game"
            .parse()
            .unwrap_or_else(|error| panic!("package fixture: {error}"))
    }

    fn evidence() -> WorldLifecycleEvidence {
        let fingerprint = CheckpointFingerprint {
            source_revision: 11,
            exact_lock_hash: CanonicalHash::digest(b"lock"),
            metadata_hash: CanonicalHash::digest(b"metadata"),
        };
        WorldLifecycleEvidence {
            fingerprint: CheckpointFingerprint {
                source_revision: 12,
                exact_lock_hash: fingerprint.exact_lock_hash,
                metadata_hash: CanonicalHash::digest(b"metadata-next"),
            },
            checkpoints: vec![RecordedCheckpoint {
                id: CheckpointId::new("checkpoint-1").unwrap_or_else(|error| panic!("{error}")),
                fingerprint,
                class: CheckpointClass::Protected,
                restore_verified: true,
            }],
            header_checksum: CanonicalHash::digest(b"header"),
            metadata_checksum: CanonicalHash::digest(b"metadata"),
            crash_marker: CrashMarker::Absent,
            lease: WriterLeaseState::Absent,
        }
    }

    fn normal_disk() -> DiskAdmission {
        let mut monitor = LowDiskMonitor::new();
        monitor
            .evaluate(
                DiskSample {
                    usable_free_bytes: 100 * GIB,
                    capacity_bytes: 100 * GIB,
                },
                HeadroomInputs::default(),
                None,
                crate::DirtyDrainState::Clean,
            )
            .unwrap_or_else(|error| panic!("{error}"))
    }
}
