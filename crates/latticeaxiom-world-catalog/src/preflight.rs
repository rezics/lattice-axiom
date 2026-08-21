use latticeaxiom_core::{
    CanonicalHash, CanonicalJsonError, PackageName, WorldId, canonical_json_hash,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

use crate::{
    AuthoritativeMetadataV1, CheckpointId, DiskAdmission, HeaderCodecError, LiveWorldLocation,
    MAX_WORLD_HEADER_BYTES, MigrationPlan, ReadOnlyWorldSource, SourceReadError, StorageOperation,
    StoragePressureState, StoreId, WorldHeaderV1,
};

/// Normative metadata-only world-open status from ADR 0027 `WORLD-13`.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum WorldOpenStatus {
    /// Exact frozen lock and every authoritative contract match.
    ReadyExact,
    /// A verified compatible closure exists with an explicit diff.
    ReadyCompatible,
    /// Artifacts must be acquired or built, then preflight rerun.
    NeedsDownloadOrBuild,
    /// A checkpointed staged migration is the next safe step.
    NeedsMigration,
    /// Bytes can be preserved, inspected, exported, or restored without a writer.
    RecoverableReadOnly,
    /// Even read or opaque preservation cannot safely continue.
    Blocked,
}

/// Risk attached to an immutable open plan.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum WorldOpenRisk {
    /// No compatibility choice or write-before-open work remains.
    None,
    /// Reversible preparation or compatible-closure choice remains.
    Low,
    /// Recovery, checkpoint, clone, or migration work remains.
    Elevated,
    /// Opening may lose or misinterpret authoritative data.
    Critical,
}

/// Contract domain represented by a compatibility difference.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum CompatibilityDomain {
    /// Frozen package graph or artifact set.
    ExactLock,
    /// Artifact trust receipt.
    Trust,
    /// Engine or external ABI requirement.
    Abi,
    /// Persistent schema read/write contract.
    Schema,
    /// Registration image.
    Registration,
    /// Semantic image, Role binding, or bundle receipt.
    Semantic,
    /// World-authoritative setting schema or value.
    Settings,
    /// Used-content requirement closure.
    ContentClosure,
}

/// Structured exact-versus-compatible difference.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CompatibilityDiff {
    /// Contract domain that differs.
    pub domain: CompatibilityDomain,
    /// Fingerprint frozen in the world.
    pub required: CanonicalHash,
    /// Verified compatible local fingerprint.
    pub available: CanonicalHash,
}

/// Declarative artifact preparation item.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PackagePreparation {
    /// Package whose artifact is unavailable.
    pub package: PackageName,
    /// Expected artifact hash, when already resolved.
    pub expected_artifact: Option<CanonicalHash>,
    /// Estimated download or build bytes.
    pub estimated_bytes: u64,
    /// Whether a build is required instead of acquisition alone.
    pub build_required: bool,
}

/// Reason authoritative bytes cannot be interpreted or preserved safely.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum BlockingReason {
    /// Immutable identity is conflicting or corrupt.
    Identity,
    /// Artifact or package trust cannot be established.
    Trust,
    /// Authoritative bytes or digest are corrupt.
    Corruption,
    /// No safe decoder or opaque envelope boundary exists.
    UnknownSchema,
    /// A concrete required content ID cannot be preserved safely.
    UnknownContent,
}

/// Pure-data compatibility result supplied to preflight.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case", tag = "assessment")]
pub enum CompatibilityAssessment {
    /// Exact frozen closure is locally verified.
    Exact,
    /// All authoritative contracts are compatible without world-byte migration.
    Compatible {
        /// Explicit differences shown before accepting the compatible closure.
        differences: Vec<CompatibilityDiff>,
    },
    /// A satisfiable graph exists after explicit artifact preparation.
    NeedsPreparation {
        /// Artifacts to acquire or build before rerunning preflight.
        packages: Vec<PackagePreparation>,
    },
    /// Target closure exists but requires a checkpointed staged migration.
    Migration {
        /// Immutable non-executing migration plan.
        plan: MigrationPlan,
    },
    /// Unknown authoritative envelopes can be preserved only without a writer.
    OpaquePreservable {
        /// Missing authoritative owners retained in diagnostics.
        missing_owners: Vec<PackageName>,
    },
    /// Read and opaque preservation are unsafe.
    Unsafe {
        /// Typed blocking reasons.
        reasons: Vec<BlockingReason>,
    },
}

/// Header state observed before DB-first reconciliation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum HeaderObservation {
    /// Canonical sidecar decoded and checksum-verified.
    Present(Box<WorldHeaderV1>),
    /// DB metadata exists but no published sidecar exists.
    Missing,
    /// Sidecar bytes were available but invalid.
    Invalid(HeaderCodecError),
    /// Sidecar could not be read.
    ReadFailed(SourceReadError),
}

/// Repairable DB-first projection divergence.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum HeaderRepairReason {
    /// No sidecar was published.
    Missing,
    /// Sidecar could not be read or decoded safely.
    InvalidOrUnreadable,
    /// DB committed a newer epoch before sidecar publication.
    DatabaseAhead,
    /// Sidecar claims an epoch newer than authoritative metadata.
    HeaderAhead,
    /// Epoch matches but projection hash or fields differ.
    ProjectionMismatch,
}

/// Non-repairable reconciliation conflict.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case", tag = "reason")]
pub enum ReconciliationBlock {
    /// Authoritative metadata's expected projection hash is internally invalid.
    InvalidMetadataProjection,
    /// Direct child directory and authoritative metadata disagree.
    DirectoryMetadataIdentityMismatch {
        /// `UUIDv4` encoded by the directory.
        directory: WorldId,
        /// `UUIDv4` read from authoritative metadata.
        metadata: WorldId,
    },
    /// Sidecar and authoritative metadata identify different worlds.
    HeaderMetadataIdentityMismatch {
        /// Sidecar world identity.
        header: WorldId,
        /// Authoritative metadata world identity.
        metadata: WorldId,
    },
    /// Sidecar and authoritative metadata identify different stores.
    StoreIdentityMismatch {
        /// Sidecar store identity.
        header: StoreId,
        /// Authoritative metadata store identity.
        metadata: StoreId,
    },
    /// Authoritative metadata could not be read.
    MetadataUnavailable {
        /// Stable source failure summary.
        detail: String,
    },
    /// The same immutable live identity appears more than once.
    DuplicateLiveWorldId,
}

/// State machine result for sidecar versus authoritative DB metadata.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case", tag = "state")]
pub enum ReconciliationState {
    /// World ID, store ID, epoch, metadata hash, and projection hash all match.
    InSync {
        /// Matched authoritative metadata epoch.
        metadata_epoch: u64,
    },
    /// DB authority can explicitly rebuild the sidecar before writable open.
    RepairRequired {
        /// Why the current sidecar cannot be used.
        reason: HeaderRepairReason,
        /// Authoritative replacement epoch.
        metadata_epoch: u64,
        /// Hash expected after rebuilding the projection.
        expected_projection_hash: CanonicalHash,
    },
    /// Identity or authoritative metadata integrity blocks repair and open.
    Blocked {
        /// Typed non-repairable conflict.
        reason: ReconciliationBlock,
    },
}

/// Reconciles one bounded sidecar observation with DB-first metadata authority.
#[must_use]
pub fn reconcile_header(
    location: LiveWorldLocation,
    observation: HeaderObservation,
    metadata: &AuthoritativeMetadataV1,
) -> ReconciliationState {
    if metadata.verify_projection_hash().is_err() {
        return ReconciliationState::Blocked {
            reason: ReconciliationBlock::InvalidMetadataProjection,
        };
    }
    let expected = &metadata.projected_header;
    if expected.world_id != location.world_id {
        return ReconciliationState::Blocked {
            reason: ReconciliationBlock::DirectoryMetadataIdentityMismatch {
                directory: location.world_id,
                metadata: expected.world_id,
            },
        };
    }

    let repair = |reason| ReconciliationState::RepairRequired {
        reason,
        metadata_epoch: expected.metadata_epoch,
        expected_projection_hash: metadata.expected_header_projection_hash,
    };
    let HeaderObservation::Present(header) = observation else {
        return match observation {
            HeaderObservation::Missing => repair(HeaderRepairReason::Missing),
            HeaderObservation::Invalid(_) | HeaderObservation::ReadFailed(_) => {
                repair(HeaderRepairReason::InvalidOrUnreadable)
            }
            HeaderObservation::Present(_) => {
                unreachable!("pattern already excluded present header")
            }
        };
    };

    if header.projection.world_id != expected.world_id {
        return ReconciliationState::Blocked {
            reason: ReconciliationBlock::HeaderMetadataIdentityMismatch {
                header: header.projection.world_id,
                metadata: expected.world_id,
            },
        };
    }
    if header.projection.store_id != expected.store_id {
        return ReconciliationState::Blocked {
            reason: ReconciliationBlock::StoreIdentityMismatch {
                header: header.projection.store_id,
                metadata: expected.store_id.clone(),
            },
        };
    }
    if header.projection.metadata_epoch < expected.metadata_epoch {
        return repair(HeaderRepairReason::DatabaseAhead);
    }
    if header.projection.metadata_epoch > expected.metadata_epoch {
        return repair(HeaderRepairReason::HeaderAhead);
    }
    if header.checksum != metadata.expected_header_projection_hash
        || header.projection != metadata.projected_header
    {
        return repair(HeaderRepairReason::ProjectionMismatch);
    }
    ReconciliationState::InSync {
        metadata_epoch: expected.metadata_epoch,
    }
}

/// Explicit action offered by a metadata-only open plan.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case", tag = "action")]
pub enum WorldOpenAction {
    /// Activate the exact frozen closure.
    UseFrozenLock,
    /// Accept a verified compatible closure and visible diff.
    ResolveCompatibleGraph,
    /// Acquire or build one required artifact, then rerun preflight.
    PreparePackage {
        /// Package whose artifact is prepared first.
        package: PackageName,
    },
    /// Open without an authoritative writer.
    OpenReadOnly,
    /// Export a read-only checkpoint bundle.
    Export,
    /// Restore a named verified checkpoint.
    RestoreCheckpoint {
        /// Checkpoint selected for restore.
        checkpoint: CheckpointId,
    },
    /// Rebuild `world-header.json` explicitly from DB metadata.
    RepairHeader {
        /// Authoritative epoch projected by the replacement.
        metadata_epoch: u64,
        /// Expected checksum of the replacement projection.
        expected_projection_hash: CanonicalHash,
    },
    /// Clone from a checkpoint and run the staged migration plan externally.
    CloneAndMigrate {
        /// Pure-data migration plan; no code has executed yet.
        plan: MigrationPlan,
    },
}

/// Stable diagnostic code carried by structured plan diagnostics.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum DiagnosticCode {
    /// DB and sidecar require explicit reconciliation.
    HeaderRepairRequired,
    /// Identity or metadata integrity blocks reconciliation.
    ReconciliationBlocked,
    /// Shutdown marker is not clean.
    UncleanShutdown,
    /// Latest authoritative revision is beyond the durable frontier.
    NonDurableFrontier,
    /// Compatible graph differs from the frozen exact lock.
    CompatibleDiff,
    /// Artifacts must be acquired or built.
    PackagePreparationRequired,
    /// World-byte migration is required.
    MigrationRequired,
    /// Authoritative bytes are opaque-preservable only.
    AuthoritativeDataReadOnly,
    /// Authoritative bytes cannot be read or preserved safely.
    AuthoritativeDataBlocked,
    /// Storage pressure restricts or pauses operations.
    LowDisk,
}

/// Typed diagnostic payload retained alongside the next safe step.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case", tag = "diagnostic")]
pub enum WorldDiagnostic {
    /// Rebuildable header divergence.
    HeaderRepairRequired {
        /// Repair classification.
        reason: HeaderRepairReason,
    },
    /// Non-repairable metadata or identity conflict.
    ReconciliationBlocked {
        /// Blocking reconciliation reason.
        reason: ReconciliationBlock,
    },
    /// Last shutdown was not clean.
    UncleanShutdown,
    /// Latest revision exceeds the contiguous durable frontier.
    NonDurableFrontier {
        /// Latest authoritative revision.
        authoritative_revision: u64,
        /// Contiguous durable frontier.
        durable_frontier: u64,
    },
    /// Explicit exact-versus-compatible differences.
    CompatibleDiff {
        /// Structured differences.
        differences: Vec<CompatibilityDiff>,
    },
    /// Artifact acquisition or build remains.
    PackagePreparationRequired {
        /// Ordered preparation items.
        packages: Vec<PackagePreparation>,
    },
    /// Staged world-byte migration remains.
    MigrationRequired,
    /// Missing owners whose bytes can only be preserved opaquely.
    AuthoritativeDataReadOnly {
        /// Missing authoritative owners.
        missing_owners: Vec<PackageName>,
    },
    /// Unsafe read or preservation reasons.
    AuthoritativeDataBlocked {
        /// Typed blocking reasons.
        reasons: Vec<BlockingReason>,
    },
    /// Storage-pressure restriction.
    LowDisk {
        /// Current hysteretic state.
        state: StoragePressureState,
        /// Usable free bytes in the sample.
        usable_free_bytes: u64,
        /// Mutation-pause threshold used for this plan.
        mutation_paused_below: u64,
    },
}

impl WorldDiagnostic {
    /// Returns the stable code for this structured diagnostic.
    #[must_use]
    pub const fn code(&self) -> DiagnosticCode {
        match self {
            Self::HeaderRepairRequired { .. } => DiagnosticCode::HeaderRepairRequired,
            Self::ReconciliationBlocked { .. } => DiagnosticCode::ReconciliationBlocked,
            Self::UncleanShutdown => DiagnosticCode::UncleanShutdown,
            Self::NonDurableFrontier { .. } => DiagnosticCode::NonDurableFrontier,
            Self::CompatibleDiff { .. } => DiagnosticCode::CompatibleDiff,
            Self::PackagePreparationRequired { .. } => DiagnosticCode::PackagePreparationRequired,
            Self::MigrationRequired => DiagnosticCode::MigrationRequired,
            Self::AuthoritativeDataReadOnly { .. } => DiagnosticCode::AuthoritativeDataReadOnly,
            Self::AuthoritativeDataBlocked { .. } => DiagnosticCode::AuthoritativeDataBlocked,
            Self::LowDisk { .. } => DiagnosticCode::LowDisk,
        }
    }
}

/// Pure-data inputs that do not require package code execution.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PreflightContext {
    /// Declarative compatibility evaluation.
    pub compatibility: CompatibilityAssessment,
    /// Current low-disk admission decision.
    pub disk: DiskAdmission,
    /// Whether another live catalog location has the same UUID.
    pub duplicate_live_world_id: bool,
}

/// Bound catalog evidence used to mint a sealed writer-activation receipt.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SealedActivationBindingV1 {
    /// Storage-generation identity.
    pub store_id: StoreId,
    /// Authoritative metadata epoch captured by preflight.
    pub metadata_epoch: u64,
    /// Hash of the authoritative metadata body.
    pub metadata_hash: CanonicalHash,
    /// Header projection hash committed with that epoch.
    pub projection_hash: CanonicalHash,
    /// Plan generation captured by this preflight.
    pub plan_generation: u64,
}

/// Non-forgeable catalog receipt required to open a world writer.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SealedWorldActivationReceiptV1 {
    /// World named by the receipt.
    pub world_id: WorldId,
    /// Storage-generation identity.
    pub store_id: StoreId,
    /// Authoritative metadata epoch.
    pub metadata_epoch: u64,
    /// Hash of the authoritative metadata body.
    pub metadata_hash: CanonicalHash,
    /// Header projection hash.
    pub projection_hash: CanonicalHash,
    /// Plan generation.
    pub plan_generation: u64,
    /// Canonical checksum of the bound fields.
    pub checksum: CanonicalHash,
}

impl SealedWorldActivationReceiptV1 {
    /// Seals catalog activation evidence for one writable accept.
    ///
    /// # Errors
    ///
    /// Returns [`CanonicalJsonError`] when the receipt cannot be hashed.
    pub fn seal(
        world_id: WorldId,
        binding: &SealedActivationBindingV1,
    ) -> Result<Self, CanonicalJsonError> {
        let mut receipt = Self {
            world_id,
            store_id: binding.store_id.clone(),
            metadata_epoch: binding.metadata_epoch,
            metadata_hash: binding.metadata_hash,
            projection_hash: binding.projection_hash,
            plan_generation: binding.plan_generation,
            checksum: CanonicalHash::digest(b""),
        };
        receipt.checksum = receipt.recompute_checksum()?;
        Ok(receipt)
    }

    /// Recomputes the checksum excluding the stored checksum field.
    ///
    /// # Errors
    ///
    /// Returns [`CanonicalJsonError`] when encoding fails.
    pub fn recompute_checksum(&self) -> Result<CanonicalHash, CanonicalJsonError> {
        let mut value = serde_json::to_value(self)?;
        if let Value::Object(fields) = &mut value {
            fields.remove("checksum");
        }
        canonical_json_hash(&value)
    }

    /// Verifies the stored checksum.
    ///
    /// # Errors
    ///
    /// Returns [`SealedReceiptError`] when encoding fails or the checksum
    /// does not match.
    pub fn verify(&self) -> Result<(), SealedReceiptError> {
        let actual = self.recompute_checksum()?;
        if actual == self.checksum {
            Ok(())
        } else {
            Err(SealedReceiptError::ChecksumMismatch)
        }
    }
}

/// Failure to verify a sealed writer-activation receipt.
#[derive(Debug, Error)]
pub enum SealedReceiptError {
    /// Canonical encoding failed.
    #[error(transparent)]
    Canonical(#[from] CanonicalJsonError),
    /// The stored checksum does not match the bound fields.
    #[error("sealed activation receipt checksum mismatch")]
    ChecksumMismatch,
}

/// Immutable result of read-only sidecar and metadata preflight.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WorldOpenPlan {
    /// World being evaluated.
    pub world_id: WorldId,
    /// Normative next-safe-step status.
    pub status: WorldOpenStatus,
    /// Highest risk exposed by the plan.
    pub risk: WorldOpenRisk,
    /// DB-first reconciliation result.
    pub reconciliation: ReconciliationState,
    /// Single default next safe action, or none when blocked.
    pub next_safe_step: Option<WorldOpenAction>,
    /// Other explicit safe actions available from this state.
    pub actions: Vec<WorldOpenAction>,
    /// Stable structured diagnostics; never an untyped string bag.
    pub diagnostics: Vec<WorldDiagnostic>,
    /// Bound catalog evidence for minting a writer-activation receipt.
    #[serde(default)]
    pub activation_binding: Option<SealedActivationBindingV1>,
}

impl WorldOpenPlan {
    /// Accepts one action that this immutable plan explicitly offered.
    ///
    /// # Errors
    ///
    /// Returns [`PlanAcceptanceError`] when the action is absent from this plan.
    pub fn accept(
        &self,
        action: WorldOpenAction,
    ) -> Result<AcceptedWorldOpenPlan, PlanAcceptanceError> {
        if !self.actions.contains(&action) {
            return Err(PlanAcceptanceError::ActionNotOffered);
        }
        let mut accepted = AcceptedWorldOpenPlan {
            status: self.status,
            action,
            activation_receipt: None,
        };
        if accepted.writable()
            && let Some(binding) = &self.activation_binding
        {
            accepted.activation_receipt = Some(
                SealedWorldActivationReceiptV1::seal(self.world_id, binding)
                    .map_err(|_| PlanAcceptanceError::Receipt)?,
            );
        }
        Ok(accepted)
    }
}

/// Accepted plan whose writer permission is derived, never stored as input.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AcceptedWorldOpenPlan {
    status: WorldOpenStatus,
    action: WorldOpenAction,
    activation_receipt: Option<SealedWorldActivationReceiptV1>,
}

impl AcceptedWorldOpenPlan {
    /// Returns whether activation may open the unique authoritative writer.
    ///
    /// Only status-compatible exact or compatible activation actions return
    /// `true`. Repair, preparation, migration, restore, export, and read-only
    /// actions must complete and rerun preflight first.
    #[must_use]
    pub const fn writable(&self) -> bool {
        matches!(
            (&self.status, &self.action),
            (WorldOpenStatus::ReadyExact, WorldOpenAction::UseFrozenLock)
                | (
                    WorldOpenStatus::ReadyCompatible,
                    WorldOpenAction::ResolveCompatibleGraph
                )
        )
    }

    /// Returns the explicitly accepted action.
    #[must_use]
    pub const fn action(&self) -> &WorldOpenAction {
        &self.action
    }

    /// Returns the sealed writer-activation receipt when this accept is writable
    /// and preflight supplied bound catalog evidence.
    #[must_use]
    pub const fn activation_receipt(&self) -> Option<&SealedWorldActivationReceiptV1> {
        self.activation_receipt.as_ref()
    }
}

/// Failure to accept an immutable preflight plan.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum PlanAcceptanceError {
    /// The action was not part of the immutable plan.
    #[error("world-open action was not offered by this preflight plan")]
    ActionNotOffered,
    /// Sealing the catalog activation receipt failed.
    #[error("failed to seal the catalog activation receipt")]
    Receipt,
}

/// Pure metadata-only preflight service.
#[derive(Debug, Default)]
pub struct WorldPreflight;

impl WorldPreflight {
    /// Reads the bounded sidecar and authoritative metadata, then produces the
    /// deterministic next-safe-step plan.
    pub fn evaluate<S>(
        source: &mut S,
        location: LiveWorldLocation,
        context: PreflightContext,
    ) -> WorldOpenPlan
    where
        S: ReadOnlyWorldSource,
    {
        let observation = match source.read_header_bounded(location, MAX_WORLD_HEADER_BYTES) {
            Ok(Some(bytes)) => match WorldHeaderV1::decode_canonical(&bytes) {
                Ok(header) => HeaderObservation::Present(Box::new(header)),
                Err(error) => HeaderObservation::Invalid(error),
            },
            Ok(None) => HeaderObservation::Missing,
            Err(error) => HeaderObservation::ReadFailed(error),
        };
        let metadata = match source.read_metadata_read_only(location) {
            Ok(metadata) => metadata,
            Err(error) => {
                return blocked_plan(
                    location.world_id,
                    ReconciliationBlock::MetadataUnavailable {
                        detail: error.to_string(),
                    },
                );
            }
        };
        let reconciliation = if context.duplicate_live_world_id {
            ReconciliationState::Blocked {
                reason: ReconciliationBlock::DuplicateLiveWorldId,
            }
        } else {
            reconcile_header(location, observation, &metadata)
        };
        Self::plan_from_metadata(&metadata, reconciliation, context)
    }

    #[allow(
        clippy::too_many_lines,
        reason = "the function implements the normative world-open status precedence table"
    )]
    fn plan_from_metadata(
        metadata: &AuthoritativeMetadataV1,
        reconciliation: ReconciliationState,
        context: PreflightContext,
    ) -> WorldOpenPlan {
        let world_id = metadata.projected_header.world_id;
        match reconciliation.clone() {
            ReconciliationState::Blocked { reason } => {
                return blocked_plan_with_state(world_id, reconciliation, reason);
            }
            ReconciliationState::RepairRequired {
                reason,
                metadata_epoch,
                expected_projection_hash,
            } => {
                let action = WorldOpenAction::RepairHeader {
                    metadata_epoch,
                    expected_projection_hash,
                };
                return WorldOpenPlan {
                    world_id,
                    status: WorldOpenStatus::RecoverableReadOnly,
                    risk: WorldOpenRisk::Elevated,
                    reconciliation,
                    next_safe_step: Some(action.clone()),
                    actions: vec![
                        action,
                        WorldOpenAction::OpenReadOnly,
                        WorldOpenAction::Export,
                    ],
                    diagnostics: vec![WorldDiagnostic::HeaderRepairRequired { reason }],
                    activation_binding: None,
                };
            }
            ReconciliationState::InSync { .. } => {}
        }

        let projection = &metadata.projected_header;
        let mut diagnostics = Vec::new();
        if !projection.clean_shutdown {
            diagnostics.push(WorldDiagnostic::UncleanShutdown);
        }
        if projection.durable_frontier < projection.authoritative_revision {
            diagnostics.push(WorldDiagnostic::NonDurableFrontier {
                authoritative_revision: projection.authoritative_revision,
                durable_frontier: projection.durable_frontier,
            });
        }
        if !diagnostics.is_empty() {
            let restore = projection
                .checkpoints
                .latest
                .clone()
                .map(|checkpoint| WorldOpenAction::RestoreCheckpoint { checkpoint });
            let next_safe_step = restore.clone().or(Some(WorldOpenAction::OpenReadOnly));
            let mut actions = vec![WorldOpenAction::OpenReadOnly, WorldOpenAction::Export];
            if let Some(action) = restore {
                actions.insert(0, action);
            }
            return WorldOpenPlan {
                world_id,
                status: WorldOpenStatus::RecoverableReadOnly,
                risk: WorldOpenRisk::Elevated,
                reconciliation,
                next_safe_step,
                actions,
                diagnostics,
                activation_binding: None,
            };
        }

        if !context.disk.allows(StorageOperation::AuthoritativeMutation) {
            let diagnostic = disk_diagnostic(context.disk);
            if context.disk.allows(StorageOperation::ReadOnly) {
                return WorldOpenPlan {
                    world_id,
                    status: WorldOpenStatus::RecoverableReadOnly,
                    risk: WorldOpenRisk::Elevated,
                    reconciliation,
                    next_safe_step: Some(WorldOpenAction::OpenReadOnly),
                    actions: vec![WorldOpenAction::OpenReadOnly, WorldOpenAction::Export],
                    diagnostics: vec![diagnostic],
                    activation_binding: None,
                };
            }
            return WorldOpenPlan {
                world_id,
                status: WorldOpenStatus::Blocked,
                risk: WorldOpenRisk::Critical,
                reconciliation,
                next_safe_step: None,
                actions: Vec::new(),
                diagnostics: vec![diagnostic],
                activation_binding: None,
            };
        }

        match context.compatibility {
            CompatibilityAssessment::Exact => ready_plan(
                world_id,
                reconciliation,
                WorldOpenStatus::ReadyExact,
                WorldOpenRisk::None,
                WorldOpenAction::UseFrozenLock,
                warning_diagnostics(context.disk),
                binding_from_metadata(metadata),
            ),
            CompatibilityAssessment::Compatible { differences } => ready_plan(
                world_id,
                reconciliation,
                WorldOpenStatus::ReadyCompatible,
                WorldOpenRisk::Low,
                WorldOpenAction::ResolveCompatibleGraph,
                vec![WorldDiagnostic::CompatibleDiff { differences }],
                binding_from_metadata(metadata),
            ),
            CompatibilityAssessment::NeedsPreparation { packages } => {
                let next_safe_step =
                    packages
                        .first()
                        .map(|package| WorldOpenAction::PreparePackage {
                            package: package.package.clone(),
                        });
                let actions = packages
                    .iter()
                    .map(|package| WorldOpenAction::PreparePackage {
                        package: package.package.clone(),
                    })
                    .chain([WorldOpenAction::OpenReadOnly, WorldOpenAction::Export])
                    .collect();
                WorldOpenPlan {
                    world_id,
                    status: WorldOpenStatus::NeedsDownloadOrBuild,
                    risk: WorldOpenRisk::Low,
                    reconciliation,
                    next_safe_step,
                    actions,
                    diagnostics: vec![WorldDiagnostic::PackagePreparationRequired { packages }],
                    activation_binding: None,
                }
            }
            CompatibilityAssessment::Migration { plan } => {
                if context.disk.allows_migration() {
                    let action = WorldOpenAction::CloneAndMigrate { plan };
                    WorldOpenPlan {
                        world_id,
                        status: WorldOpenStatus::NeedsMigration,
                        risk: WorldOpenRisk::Elevated,
                        reconciliation,
                        next_safe_step: Some(action.clone()),
                        actions: vec![
                            action,
                            WorldOpenAction::OpenReadOnly,
                            WorldOpenAction::Export,
                        ],
                        diagnostics: vec![WorldDiagnostic::MigrationRequired],
                        activation_binding: None,
                    }
                } else {
                    WorldOpenPlan {
                        world_id,
                        status: WorldOpenStatus::RecoverableReadOnly,
                        risk: WorldOpenRisk::Elevated,
                        reconciliation,
                        next_safe_step: Some(WorldOpenAction::OpenReadOnly),
                        actions: vec![WorldOpenAction::OpenReadOnly, WorldOpenAction::Export],
                        diagnostics: vec![
                            WorldDiagnostic::MigrationRequired,
                            disk_diagnostic(context.disk),
                        ],
                        activation_binding: None,
                    }
                }
            }
            CompatibilityAssessment::OpaquePreservable { missing_owners } => WorldOpenPlan {
                world_id,
                status: WorldOpenStatus::RecoverableReadOnly,
                risk: WorldOpenRisk::Elevated,
                reconciliation,
                next_safe_step: Some(WorldOpenAction::OpenReadOnly),
                actions: vec![WorldOpenAction::OpenReadOnly, WorldOpenAction::Export],
                diagnostics: vec![WorldDiagnostic::AuthoritativeDataReadOnly { missing_owners }],
                activation_binding: None,
            },
            CompatibilityAssessment::Unsafe { reasons } => WorldOpenPlan {
                world_id,
                status: WorldOpenStatus::Blocked,
                risk: WorldOpenRisk::Critical,
                reconciliation,
                next_safe_step: None,
                actions: Vec::new(),
                diagnostics: vec![WorldDiagnostic::AuthoritativeDataBlocked { reasons }],
                activation_binding: None,
            },
        }
    }
}

fn blocked_plan(world_id: WorldId, reason: ReconciliationBlock) -> WorldOpenPlan {
    blocked_plan_with_state(
        world_id,
        ReconciliationState::Blocked {
            reason: reason.clone(),
        },
        reason,
    )
}

fn blocked_plan_with_state(
    world_id: WorldId,
    reconciliation: ReconciliationState,
    reason: ReconciliationBlock,
) -> WorldOpenPlan {
    WorldOpenPlan {
        world_id,
        status: WorldOpenStatus::Blocked,
        risk: WorldOpenRisk::Critical,
        reconciliation,
        next_safe_step: None,
        actions: Vec::new(),
        diagnostics: vec![WorldDiagnostic::ReconciliationBlocked { reason }],
        activation_binding: None,
    }
}

fn ready_plan(
    world_id: WorldId,
    reconciliation: ReconciliationState,
    status: WorldOpenStatus,
    risk: WorldOpenRisk,
    action: WorldOpenAction,
    diagnostics: Vec<WorldDiagnostic>,
    binding: SealedActivationBindingV1,
) -> WorldOpenPlan {
    WorldOpenPlan {
        world_id,
        status,
        risk,
        reconciliation,
        next_safe_step: Some(action.clone()),
        actions: vec![action],
        diagnostics,
        activation_binding: Some(binding),
    }
}

fn binding_from_metadata(metadata: &AuthoritativeMetadataV1) -> SealedActivationBindingV1 {
    let projection = &metadata.projected_header;
    SealedActivationBindingV1 {
        store_id: projection.store_id.clone(),
        metadata_epoch: projection.metadata_epoch,
        metadata_hash: projection.authoritative_metadata_hash,
        projection_hash: metadata.expected_header_projection_hash,
        plan_generation: projection.metadata_epoch,
    }
}

fn warning_diagnostics(disk: DiskAdmission) -> Vec<WorldDiagnostic> {
    if disk.state == StoragePressureState::Warning {
        vec![disk_diagnostic(disk)]
    } else {
        Vec::new()
    }
}

fn disk_diagnostic(disk: DiskAdmission) -> WorldDiagnostic {
    WorldDiagnostic::LowDisk {
        state: disk.state,
        usable_free_bytes: disk.usable_free_bytes,
        mutation_paused_below: disk.thresholds.mutation_paused_below,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        DirtyDrainState, DiskSample, HeadroomInputs, LowDiskMonitor, MemoryWorldRecord,
        MemoryWorldSource, WorldRootId, header::tests::fixture_projection,
    };

    #[test]
    fn exact_plan_derives_writer_permission_only_after_matching_acceptance() {
        let (mut source, location, context) = fixture(CompatibilityAssessment::Exact);
        let plan = WorldPreflight::evaluate(&mut source, location, context);
        assert_eq!(plan.status, WorldOpenStatus::ReadyExact);
        assert!(!plan.actions.is_empty());
        let accepted = plan
            .accept(WorldOpenAction::UseFrozenLock)
            .unwrap_or_else(|error| panic!("{error}"));
        assert!(accepted.writable());
        let receipt = accepted
            .activation_receipt()
            .unwrap_or_else(|| panic!("exact accept must mint a sealed activation receipt"));
        receipt.verify().unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(receipt.world_id, plan.world_id);
        assert_eq!(source.audit().header_reads, 1);
        assert_eq!(source.audit().metadata_reads, 1);
    }

    #[test]
    fn sealed_activation_receipt_checksum_covers_bound_fields() {
        let binding = SealedActivationBindingV1 {
            store_id: StoreId::new("store-generation-1").unwrap_or_else(|error| panic!("{error}")),
            metadata_epoch: 1,
            metadata_hash: CanonicalHash::digest(b"metadata"),
            projection_hash: CanonicalHash::digest(b"projection"),
            plan_generation: 1,
        };
        let world = crate::header::tests::fixture_projection().world_id;
        let receipt = SealedWorldActivationReceiptV1::seal(world, &binding)
            .unwrap_or_else(|error| panic!("{error}"));
        receipt.verify().unwrap_or_else(|error| panic!("{error}"));
        let mut mutated = receipt.clone();
        mutated.metadata_epoch = 2;
        assert!(mutated.verify().is_err());
    }

    #[test]
    fn db_ahead_requires_header_repair_and_never_opens_writer() {
        let projection = fixture_projection();
        let mut stale = projection.clone();
        stale.metadata_epoch -= 1;
        let stale_header = WorldHeaderV1::seal(stale)
            .unwrap_or_else(|error| panic!("{error}"))
            .encode_canonical()
            .unwrap_or_else(|error| panic!("{error}"));
        let metadata = AuthoritativeMetadataV1::seal(projection.clone())
            .unwrap_or_else(|error| panic!("{error}"));
        let location = LiveWorldLocation::new(WorldRootId(1), projection.world_id);
        let mut source = MemoryWorldSource::default();
        source.insert(
            location,
            MemoryWorldRecord {
                header_bytes: Some(stale_header),
                metadata: Some(metadata),
            },
        );
        let context = PreflightContext {
            compatibility: CompatibilityAssessment::Exact,
            disk: normal_disk(),
            duplicate_live_world_id: false,
        };

        let plan = WorldPreflight::evaluate(&mut source, location, context);
        assert_eq!(plan.status, WorldOpenStatus::RecoverableReadOnly);
        assert!(matches!(
            plan.reconciliation,
            ReconciliationState::RepairRequired {
                reason: HeaderRepairReason::DatabaseAhead,
                ..
            }
        ));
        let repair = plan
            .next_safe_step
            .clone()
            .unwrap_or_else(|| panic!("missing repair"));
        assert!(
            !plan
                .accept(repair)
                .unwrap_or_else(|error| panic!("{error}"))
                .writable()
        );
    }

    #[test]
    fn low_disk_does_not_claim_migration_is_executable() {
        let projection = fixture_projection();
        let migration = crate::MigrationPlan::new(
            projection.world_id,
            CheckpointId::new("checkpoint-1").unwrap_or_else(|error| panic!("{error}")),
            CanonicalHash::digest(b"source"),
            CanonicalHash::digest(b"target"),
            vec![crate::MigrationStep {
                owner: "@example/game"
                    .parse()
                    .unwrap_or_else(|error| panic!("fixture package: {error}")),
                schema: "example:schema/state@1"
                    .parse()
                    .unwrap_or_else(|error| panic!("fixture schema: {error}")),
                from_version: 1,
                to_version: 2,
                changes_world_bytes: true,
                dry_run: crate::DryRunSupport::Supported,
            }],
            crate::MigrationEstimate {
                records: 1,
                uncompressed_bytes: 1,
                required_disk_bytes: 1,
            },
            crate::VolumeRelation::SameVolume,
            false,
        )
        .unwrap_or_else(|error| panic!("{error}"));
        let (mut source, location, mut context) =
            fixture(CompatibilityAssessment::Migration { plan: migration });
        let mut monitor = LowDiskMonitor::new();
        context.disk = monitor
            .evaluate(
                DiskSample {
                    usable_free_bytes: 1,
                    capacity_bytes: 100 * crate::GIB,
                },
                HeadroomInputs::default(),
                None,
                DirtyDrainState::Clean,
            )
            .unwrap_or_else(|error| panic!("{error}"));

        let plan = WorldPreflight::evaluate(&mut source, location, context);
        assert_eq!(plan.status, WorldOpenStatus::RecoverableReadOnly);
        assert!(
            !plan
                .actions
                .iter()
                .any(|action| matches!(action, WorldOpenAction::CloneAndMigrate { .. }))
        );
    }

    fn fixture(
        compatibility: CompatibilityAssessment,
    ) -> (MemoryWorldSource, LiveWorldLocation, PreflightContext) {
        let projection = fixture_projection();
        let header = WorldHeaderV1::seal(projection.clone())
            .unwrap_or_else(|error| panic!("{error}"))
            .encode_canonical()
            .unwrap_or_else(|error| panic!("{error}"));
        let metadata = AuthoritativeMetadataV1::seal(projection.clone())
            .unwrap_or_else(|error| panic!("{error}"));
        let location = LiveWorldLocation::new(WorldRootId(1), projection.world_id);
        let mut source = MemoryWorldSource::default();
        source.insert(
            location,
            MemoryWorldRecord {
                header_bytes: Some(header),
                metadata: Some(metadata),
            },
        );
        (
            source,
            location,
            PreflightContext {
                compatibility,
                disk: normal_disk(),
                duplicate_live_world_id: false,
            },
        )
    }

    fn normal_disk() -> DiskAdmission {
        let mut monitor = LowDiskMonitor::new();
        monitor
            .evaluate(
                DiskSample {
                    usable_free_bytes: 100 * crate::GIB,
                    capacity_bytes: 100 * crate::GIB,
                },
                HeadroomInputs::default(),
                None,
                DirtyDrainState::Clean,
            )
            .unwrap_or_else(|error| panic!("{error}"))
    }
}
