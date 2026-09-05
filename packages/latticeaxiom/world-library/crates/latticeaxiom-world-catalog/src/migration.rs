use latticeaxiom_core::{CanonicalHash, PackageName, SchemaId, WorldId};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::CheckpointId;

/// Maximum uncompressed staged payload for in-store migration staging.
pub const IN_STORE_MIGRATION_MAX_BYTES: u64 = 64 * 1024 * 1024;
/// Maximum record count for in-store migration staging.
pub const IN_STORE_MIGRATION_MAX_RECORDS: u64 = 4_096;

/// Whether staging and the active store reside on the same filesystem volume.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum VolumeRelation {
    /// Same-volume publish primitives are available.
    SameVolume,
    /// Staging first occurs on a different volume.
    CrossVolume,
}

/// Bounded migration work estimate produced from declarative descriptors.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct MigrationEstimate {
    /// Records expected to be read and validated.
    pub records: u64,
    /// Uncompressed bytes expected in the staged result.
    pub uncompressed_bytes: u64,
    /// Additional physical bytes reserved by low-disk admission.
    pub required_disk_bytes: u64,
}

/// Normative staging mechanism selected from size and volume evidence.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum MigrationStrategy {
    /// Versioned staging keys in the same DB, published by one metadata switch.
    InStoreEpoch,
    /// Sibling store on the same volume, published after all handles close.
    SiblingStore,
    /// Complete staging on the target volume before a same-volume publish there.
    CrossVolumeStagedStore,
}

impl MigrationStrategy {
    /// Selects the ADR 0027 `WORLD-16` staging strategy.
    #[must_use]
    pub const fn select(estimate: MigrationEstimate, relation: VolumeRelation) -> Self {
        if matches!(relation, VolumeRelation::CrossVolume) {
            return Self::CrossVolumeStagedStore;
        }
        if estimate.uncompressed_bytes <= IN_STORE_MIGRATION_MAX_BYTES
            && estimate.records <= IN_STORE_MIGRATION_MAX_RECORDS
        {
            Self::InStoreEpoch
        } else {
            Self::SiblingStore
        }
    }
}

/// Whether a descriptor can provide a meaningful pure-data dry run.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum DryRunSupport {
    /// Declarative inspection can predict scope and errors.
    Supported,
    /// The owner explicitly declares that a reliable dry run is unavailable.
    Unavailable,
}

/// One owner-controlled schema migration step.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct MigrationStep {
    /// Package that owns the migration implementation and schema meaning.
    pub owner: PackageName,
    /// Persistent schema being transformed.
    pub schema: SchemaId,
    /// Source schema version.
    pub from_version: u32,
    /// Target schema version.
    pub to_version: u32,
    /// Whether authoritative world bytes change.
    pub changes_world_bytes: bool,
    /// Honest dry-run capability declaration.
    pub dry_run: DryRunSupport,
}

/// Immutable pure-data plan produced before any migration code executes.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct MigrationPlan {
    /// World whose checkpoint will be used as the source.
    pub world_id: WorldId,
    /// Verified checkpoint required before staging begins.
    pub source_checkpoint: CheckpointId,
    /// Exact source closure fingerprint.
    pub source_lock: CanonicalHash,
    /// Verified target closure fingerprint.
    pub target_lock: CanonicalHash,
    /// Ordered owner-controlled migration steps.
    pub steps: Vec<MigrationStep>,
    /// Bounded work and disk estimate.
    pub estimate: MigrationEstimate,
    /// Selected staging strategy.
    pub strategy: MigrationStrategy,
    /// `true` when the plan moves to a lower schema or package version.
    pub downgrade: bool,
}

impl MigrationPlan {
    /// Validates and constructs a migration plan.
    ///
    /// # Errors
    ///
    /// Returns [`MigrationPlanError`] for an empty plan, an identity plan, or
    /// an in-place downgrade request.
    #[allow(
        clippy::too_many_arguments,
        reason = "the constructor validates one complete immutable plan DTO"
    )]
    pub fn new(
        world_id: WorldId,
        source_checkpoint: CheckpointId,
        source_lock: CanonicalHash,
        target_lock: CanonicalHash,
        steps: Vec<MigrationStep>,
        estimate: MigrationEstimate,
        relation: VolumeRelation,
        downgrade: bool,
    ) -> Result<Self, MigrationPlanError> {
        if steps.is_empty() {
            return Err(MigrationPlanError::NoSteps);
        }
        if steps
            .iter()
            .all(|step| step.from_version == step.to_version && !step.changes_world_bytes)
        {
            return Err(MigrationPlanError::NoEffectiveChange);
        }
        if downgrade {
            return Err(MigrationPlanError::DowngradeRequiresClone);
        }
        Ok(Self {
            world_id,
            source_checkpoint,
            source_lock,
            target_lock,
            steps,
            estimate,
            strategy: MigrationStrategy::select(estimate, relation),
            downgrade,
        })
    }
}

/// Failure to form a safe first-version migration plan.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum MigrationPlanError {
    /// No owner-controlled descriptor was provided.
    #[error("a migration plan requires at least one versioned step")]
    NoSteps,
    /// Every step is an identity operation.
    #[error("a migration plan must describe an effective version or world-byte change")]
    NoEffectiveChange,
    /// Downgrade may not write the original world.
    #[error("downgrade requires an explicit clone or export conversion flow")]
    DowngradeRequiresClone,
}

/// Evidence required before staged migration publication.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "the DTO mirrors the fixed normative publication-gate receipt matrix"
)]
pub struct MigrationPublishEvidence {
    /// Source checkpoint was created and independently verified.
    pub checkpoint_verified: bool,
    /// Target lock and artifact trust were revalidated.
    pub target_lock_and_trust_verified: bool,
    /// Target schema closure was verified.
    pub schema_closure_verified: bool,
    /// Required stable content closure was verified.
    pub content_closure_verified: bool,
    /// Staged record count matches the receipt.
    pub record_count_verified: bool,
    /// Staged digest matches the receipt.
    pub digest_verified: bool,
    /// World and record revisions satisfy the plan.
    pub revision_verified: bool,
    /// Reserved disk headroom remains admitted.
    pub disk_headroom_admitted: bool,
    /// Header projection and expected hash were rebuilt and verified.
    pub header_projection_verified: bool,
    /// A fresh headless read-only reopen passed.
    pub headless_reopen_verified: bool,
}

impl MigrationPublishEvidence {
    /// Validates every mandatory publication gate.
    ///
    /// # Errors
    ///
    /// Returns all failed [`MigrationGate`] values in deterministic order.
    pub fn validate(self) -> Result<(), MigrationPublishGateError> {
        let checks = [
            (MigrationGate::Checkpoint, self.checkpoint_verified),
            (
                MigrationGate::TargetLockAndTrust,
                self.target_lock_and_trust_verified,
            ),
            (MigrationGate::SchemaClosure, self.schema_closure_verified),
            (MigrationGate::ContentClosure, self.content_closure_verified),
            (MigrationGate::RecordCount, self.record_count_verified),
            (MigrationGate::Digest, self.digest_verified),
            (MigrationGate::Revision, self.revision_verified),
            (MigrationGate::DiskHeadroom, self.disk_headroom_admitted),
            (
                MigrationGate::HeaderProjection,
                self.header_projection_verified,
            ),
            (MigrationGate::HeadlessReopen, self.headless_reopen_verified),
        ];
        let failed = checks
            .into_iter()
            .filter_map(|(gate, passed)| (!passed).then_some(gate))
            .collect::<Vec<_>>();
        if failed.is_empty() {
            Ok(())
        } else {
            Err(MigrationPublishGateError { failed })
        }
    }
}

/// Named migration publication gate.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum MigrationGate {
    /// Verified source checkpoint.
    Checkpoint,
    /// Verified target lock and trust.
    TargetLockAndTrust,
    /// Verified schema closure.
    SchemaClosure,
    /// Verified content closure.
    ContentClosure,
    /// Record-count receipt.
    RecordCount,
    /// Digest receipt.
    Digest,
    /// Revision receipt.
    Revision,
    /// Low-disk admission.
    DiskHeadroom,
    /// Header projection receipt.
    HeaderProjection,
    /// Headless reopen receipt.
    HeadlessReopen,
}

/// Failed staged-migration publication gate.
#[derive(Clone, Debug, Eq, Error, PartialEq)]
#[error("migration publish gate failed: {failed:?}")]
pub struct MigrationPublishGateError {
    /// Failed gates in deterministic normative order.
    pub failed: Vec<MigrationGate>,
}

/// Rollback semantics after a staged generation has been published.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum MigrationRollback {
    /// No new authoritative mutation exists; switch back to the prior generation.
    GenerationSwitchAvailable,
    /// New data exists; recovery must restore or clone instead of downgrading.
    RestoreOrCloneRequired,
}

impl MigrationRollback {
    /// Derives rollback semantics from post-publish authoritative mutation.
    #[must_use]
    pub const fn after_publish(new_authoritative_mutation: bool) -> Self {
        if new_authoritative_mutation {
            Self::RestoreOrCloneRequired
        } else {
            Self::GenerationSwitchAvailable
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn staging_thresholds_select_the_normative_strategy() {
        let at_limit = MigrationEstimate {
            records: IN_STORE_MIGRATION_MAX_RECORDS,
            uncompressed_bytes: IN_STORE_MIGRATION_MAX_BYTES,
            required_disk_bytes: 1,
        };
        assert_eq!(
            MigrationStrategy::select(at_limit, VolumeRelation::SameVolume),
            MigrationStrategy::InStoreEpoch
        );
        assert_eq!(
            MigrationStrategy::select(
                MigrationEstimate {
                    records: at_limit.records + 1,
                    ..at_limit
                },
                VolumeRelation::SameVolume
            ),
            MigrationStrategy::SiblingStore
        );
        assert_eq!(
            MigrationStrategy::select(at_limit, VolumeRelation::CrossVolume),
            MigrationStrategy::CrossVolumeStagedStore
        );
    }

    #[test]
    fn publish_gate_reports_every_missing_receipt() {
        let evidence = MigrationPublishEvidence {
            checkpoint_verified: true,
            target_lock_and_trust_verified: false,
            schema_closure_verified: true,
            content_closure_verified: true,
            record_count_verified: true,
            digest_verified: false,
            revision_verified: true,
            disk_headroom_admitted: true,
            header_projection_verified: true,
            headless_reopen_verified: true,
        };
        assert_eq!(
            evidence.validate().err().map(|error| error.failed),
            Some(vec![
                MigrationGate::TargetLockAndTrust,
                MigrationGate::Digest
            ])
        );
    }
}
