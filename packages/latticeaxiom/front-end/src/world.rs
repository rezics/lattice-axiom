//! World-list projection, health, actions, quick-create, and managed trash.

use std::{cmp::Ordering, collections::BTreeMap};

use latticeaxiom_core::{CanonicalHash, PackageName, StableId, WorldId};
use latticeaxiom_world_catalog::{
    CatalogCardState, CatalogDiagnosticCode, CatalogEntry, CatalogEntryFailure, CatalogEntryState,
    CrashMarker, DiagnosticCode, DisplayName, LiveWorldLocation, ManagedTrashLocation,
    MoveToTrashPlan, RestoreMode, RestorePlanningOutcome, TrashTombstone, WorldDiagnostic,
    WorldLifecycleEvidence, WorldOpenAction, WorldOpenPlan, WorldOpenStatus, WorldRootId,
    WriterBarrier, WriterLeaseState, classify_catalog_card, plan_restore,
};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{SemanticActionId, SemanticNodeId};

/// Optional presentation metadata loaded independently from the bounded header.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WorldCardMetadata {
    /// Creation time in milliseconds since Unix epoch.
    pub created_at_ms: u64,
    /// Last-played time in milliseconds since Unix epoch.
    pub last_played_at_ms: u64,
    /// Lazily computed physical size, when available.
    pub physical_bytes: Option<u64>,
    /// Package/game summary.
    pub game_summary: Option<String>,
    /// Dimension summary.
    pub dimension_summary: Option<String>,
}

/// One visible world record including failed headers.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorldShellRecord {
    /// Bounded catalog entry.
    pub entry: CatalogEntry,
    /// Non-authoritative presentation metadata.
    pub metadata: WorldCardMetadata,
    /// Read-only preflight result, when available.
    pub open_plan: Option<WorldOpenPlan>,
    /// Checkpoint/clone/export evidence captured without a writer.
    pub lifecycle: Option<WorldLifecycleEvidence>,
}

impl WorldShellRecord {
    /// Builds a record while preventing a plan from being attached to another world.
    ///
    /// # Errors
    ///
    /// Returns [`WorldShellError::PlanIdentityMismatch`] for mismatched IDs.
    pub fn new(
        entry: CatalogEntry,
        metadata: WorldCardMetadata,
        open_plan: Option<WorldOpenPlan>,
    ) -> Result<Self, WorldShellError> {
        if open_plan
            .as_ref()
            .is_some_and(|plan| plan.world_id != entry.location.world_id)
        {
            return Err(WorldShellError::PlanIdentityMismatch);
        }
        Ok(Self {
            entry,
            metadata,
            open_plan,
            lifecycle: None,
        })
    }

    /// Immutable world identity retained even for a corrupt header.
    #[must_use]
    pub const fn world_id(&self) -> WorldId {
        self.entry.location.world_id
    }

    /// Stable accessibility/focus key independent of row order.
    #[must_use]
    pub fn semantic_id(&self) -> SemanticNodeId {
        match SemanticNodeId::new(format!(
            "world:{}/{}",
            self.entry.location.root.0, self.entry.location.world_id
        )) {
            Ok(id) => id,
            Err(error) => unreachable!("validated world row semantic ID: {error}"),
        }
    }

    /// Display label; corrupt entries remain visible under a recovery label.
    #[must_use]
    pub fn display_label(&self) -> String {
        match &self.entry.state {
            CatalogEntryState::Projected(projection) => projection.display_name.as_str().to_owned(),
            CatalogEntryState::Failed(_) => {
                let canonical = self.world_id().to_string();
                format!("Recovery {}", &canonical[..8])
            }
        }
    }

    /// Health derived from catalog failure or normative preflight status.
    #[must_use]
    pub fn health(&self) -> WorldHealth {
        match (&self.entry.state, &self.open_plan) {
            (CatalogEntryState::Failed(failure), _) => {
                WorldHealth::CatalogFailure(catalog_failure_code(failure))
            }
            (_, Some(plan)) => WorldHealth::OpenStatus(plan.status),
            (CatalogEntryState::Projected(projection), None) if !projection.clean_shutdown => {
                WorldHealth::NeedsPreflight
            }
            _ => WorldHealth::NeedsPreflight,
        }
    }

    /// Player-facing catalog card state, including scan-only recovery.
    #[must_use]
    pub fn card_state(&self) -> CatalogCardState {
        classify_catalog_card(&self.entry.state, self.open_plan.as_ref())
    }

    /// Actions mechanically derived from health and immutable plan actions.
    #[must_use]
    pub fn actions(&self) -> Vec<WorldCardAction> {
        let mut actions = Vec::new();
        match (&self.entry.state, &self.open_plan) {
            (CatalogEntryState::Failed(_), _) => {
                actions.push(WorldCardAction::InspectRecovery);
            }
            (_, Some(plan)) if plan.status == WorldOpenStatus::ReadyExact => {
                actions.push(WorldCardAction::PlayExact);
                actions.push(WorldCardAction::CreateCheckpoint);
                actions.push(WorldCardAction::Duplicate);
                actions.push(WorldCardAction::Export);
                actions.push(WorldCardAction::Rename);
                actions.push(WorldCardAction::MoveToTrash);
            }
            (_, Some(plan)) => {
                actions.extend(plan.actions.iter().cloned().map(WorldCardAction::Preflight));
                if plan.diagnostics.iter().any(|diagnostic| {
                    diagnostic.code() == latticeaxiom_world_catalog::DiagnosticCode::LowDisk
                }) {
                    actions.push(WorldCardAction::OpenStorageLocation);
                }
                actions.push(WorldCardAction::MoveToTrash);
            }
            _ => actions.push(WorldCardAction::RunPreflight),
        }
        actions.push(WorldCardAction::Details);
        actions
    }

    /// Stable child semantic ID for one card action.
    #[must_use]
    pub fn action_semantic_id(&self, action: &WorldCardAction) -> Option<SemanticNodeId> {
        let suffix = action.semantic_suffix()?;
        SemanticNodeId::new(format!("{}/{suffix}", self.semantic_id().as_str())).ok()
    }

    /// Actionable recovery cues derived from preflight and lifecycle evidence.
    ///
    /// Continue remains unavailable unless health is `ReadyExact`. These cues
    /// never collapse crash, lease, and low-disk into a generic cannot-open
    /// string.
    #[must_use]
    pub fn recovery_cues(&self) -> Vec<RecoveryCue> {
        let mut cues = Vec::new();
        if let Some(plan) = &self.open_plan {
            cues.extend(plan.diagnostics.iter().map(recovery_cue_from_diagnostic));
        } else {
            match &self.lifecycle {
                Some(evidence) if evidence.crash_marker == CrashMarker::Present => {
                    cues.push(RecoveryCue {
                        code: DiagnosticCode::UncleanShutdown,
                        title: "Unclean shutdown".to_owned(),
                        description: "Run metadata-only preflight. A writer will not open."
                            .to_owned(),
                        next_action: Some(WorldCardAction::RunPreflight),
                    });
                }
                Some(evidence) if evidence.lease == WriterLeaseState::Stale => {
                    cues.push(RecoveryCue {
                        code: DiagnosticCode::StaleWriterLease,
                        title: "Stale writer lease".to_owned(),
                        description:
                            "Run metadata-only preflight. The abandoned lease is not copied."
                                .to_owned(),
                        next_action: Some(WorldCardAction::RunPreflight),
                    });
                }
                Some(evidence) if evidence.lease == WriterLeaseState::Held => {
                    cues.push(RecoveryCue {
                        code: DiagnosticCode::WriterLeaseHeld,
                        title: "Writer lease held".to_owned(),
                        description:
                            "Another process holds the exclusive writer. Continue is unavailable."
                                .to_owned(),
                        next_action: Some(WorldCardAction::RunPreflight),
                    });
                }
                _ => {}
            }
        }
        cues
    }
}

/// Player-facing recovery diagnostic that names the next safe action.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RecoveryCue {
    /// Stable diagnostic code.
    pub code: DiagnosticCode,
    /// Accessible title.
    pub title: String,
    /// Actionable description; never a generic cannot-open label.
    pub description: String,
    /// Explicit next card action, when one exists.
    pub next_action: Option<WorldCardAction>,
}

fn recovery_cue_from_diagnostic(diagnostic: &WorldDiagnostic) -> RecoveryCue {
    let (title, description, next_action) = match diagnostic {
        WorldDiagnostic::UncleanShutdown => (
            "Unclean shutdown",
            "Restore a verified checkpoint or open read-only. A writer will not open.",
            Some(WorldCardAction::Preflight(WorldOpenAction::OpenReadOnly)),
        ),
        WorldDiagnostic::NonDurableFrontier { .. } => (
            "Non-durable frontier",
            "Restore a verified checkpoint or open read-only. Continue is unavailable.",
            Some(WorldCardAction::Preflight(WorldOpenAction::OpenReadOnly)),
        ),
        WorldDiagnostic::StaleWriterLease => (
            "Stale writer lease",
            "Clear the abandoned exclusive lease. The catalog will not copy a process-local lease.",
            Some(WorldCardAction::Preflight(
                WorldOpenAction::RecoverStaleLease,
            )),
        ),
        WorldDiagnostic::WriterLeaseHeld => (
            "Writer lease held",
            "Another process holds the exclusive writer. Open read-only or wait; Continue is unavailable.",
            Some(WorldCardAction::Preflight(WorldOpenAction::OpenReadOnly)),
        ),
        WorldDiagnostic::LowDisk { state, .. } => (
            "Storage pressure",
            match state {
                latticeaxiom_world_catalog::StoragePressureState::Warning => {
                    "Free disk space. Automatic checkpoints and remote prefetch are paused."
                }
                latticeaxiom_world_catalog::StoragePressureState::MutationPaused => {
                    "Free disk space. New authoritative writes are paused until drain succeeds."
                }
                _ => "Free disk space before any writable open. A writer will not open.",
            },
            Some(WorldCardAction::OpenStorageLocation),
        ),
        WorldDiagnostic::HeaderRepairRequired { .. } => (
            "Header repair required",
            "Rebuild the bounded sidecar from authoritative metadata before Continue.",
            Some(WorldCardAction::RunPreflight),
        ),
        WorldDiagnostic::PackagePreparationRequired { .. } => (
            "Missing package",
            "Acquire or build the required artifact, then rerun preflight.",
            Some(WorldCardAction::RunPreflight),
        ),
        WorldDiagnostic::MigrationRequired => (
            "Migration required",
            "Checkpoint and clone before staged migration. The original world stays unmodified.",
            Some(WorldCardAction::Duplicate),
        ),
        WorldDiagnostic::AuthoritativeDataReadOnly { .. } => (
            "Read-only recovery",
            "Inspect or export opaque bytes. A writer will not open.",
            Some(WorldCardAction::Preflight(WorldOpenAction::OpenReadOnly)),
        ),
        WorldDiagnostic::AuthoritativeDataBlocked { .. }
        | WorldDiagnostic::ReconciliationBlocked { .. } => (
            "World blocked",
            "Identity, trust, or corruption prevents safe preservation. Continue is unavailable.",
            Some(WorldCardAction::InspectRecovery),
        ),
        WorldDiagnostic::CompatibleDiff { .. } => (
            "Compatible lock differs",
            "Review the explicit graph diff. Compatible reopen does not rewrite the frozen lock.",
            Some(WorldCardAction::RunPreflight),
        ),
    };
    RecoveryCue {
        code: diagnostic.code(),
        title: title.to_owned(),
        description: description.to_owned(),
        next_action,
    }
}

fn catalog_failure_code(failure: &CatalogEntryFailure) -> CatalogDiagnosticCode {
    match failure {
        CatalogEntryFailure::Unreadable(_) | CatalogEntryFailure::MalformedHeader(_) => {
            CatalogDiagnosticCode::Unreadable
        }
        CatalogEntryFailure::PermissionDenied => CatalogDiagnosticCode::PermissionDenied,
        CatalogEntryFailure::HeaderTooLarge { .. } => CatalogDiagnosticCode::HeaderTooLarge,
        CatalogEntryFailure::BadChecksum => CatalogDiagnosticCode::BadChecksum,
        CatalogEntryFailure::UnsupportedHeader { .. } => CatalogDiagnosticCode::UnsupportedHeader,
        CatalogEntryFailure::DirectoryIdentityMismatch { .. }
        | CatalogEntryFailure::DuplicateWorldId { .. } => CatalogDiagnosticCode::DuplicateWorldId,
        CatalogEntryFailure::IncompleteTemporary => CatalogDiagnosticCode::IncompleteTemporary,
        CatalogEntryFailure::SymlinkEscape => CatalogDiagnosticCode::SymlinkEscape,
        CatalogEntryFailure::TrashTombstoneError(_) => CatalogDiagnosticCode::TrashTombstoneError,
    }
}

/// User-visible world health without untyped error strings.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case", tag = "health")]
pub enum WorldHealth {
    /// Catalog entry failed bounded scanning but remains visible.
    CatalogFailure(CatalogDiagnosticCode),
    /// Header exists but preflight has not completed or shutdown is suspect.
    NeedsPreflight,
    /// Normative metadata-only open status.
    OpenStatus(WorldOpenStatus),
}

impl WorldHealth {
    const fn rank(self) -> u8 {
        match self {
            Self::OpenStatus(WorldOpenStatus::ReadyExact) => 0,
            Self::OpenStatus(WorldOpenStatus::ReadyCompatible) => 1,
            Self::OpenStatus(WorldOpenStatus::NeedsDownloadOrBuild) => 2,
            Self::OpenStatus(WorldOpenStatus::NeedsMigration) => 3,
            Self::NeedsPreflight => 4,
            Self::OpenStatus(WorldOpenStatus::RecoverableReadOnly) => 5,
            Self::OpenStatus(WorldOpenStatus::Blocked) => 6,
            Self::CatalogFailure(_) => 7,
        }
    }
}

/// Explicit world-card action; dangerous actions are never implicit modifiers.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case", tag = "action")]
pub enum WorldCardAction {
    /// Play only an exact-ready world.
    PlayExact,
    /// Run metadata-only preflight.
    RunPreflight,
    /// One explicit action offered by preflight.
    Preflight(WorldOpenAction),
    /// Inspect a corrupt/unreadable entry and recovery diagnostics.
    InspectRecovery,
    /// Create a checkpoint.
    CreateCheckpoint,
    /// Clone into a new immutable world identity.
    Duplicate,
    /// Export a read-only bundle.
    Export,
    /// Rename display metadata without changing identity.
    Rename,
    /// Move to managed trash, never permanent-delete directly.
    MoveToTrash,
    /// Open a storage-management location for low-disk recovery.
    OpenStorageLocation,
    /// Open world/package/schema details.
    Details,
}

impl WorldCardAction {
    /// Path suffix used under the parent world row.
    #[must_use]
    pub fn semantic_suffix(&self) -> Option<&'static str> {
        match self {
            Self::PlayExact | Self::Preflight(WorldOpenAction::UseFrozenLock) => Some("play"),
            Self::RunPreflight => Some("preflight"),
            Self::Preflight(WorldOpenAction::ResolveCompatibleGraph) => Some("compatible"),
            Self::Preflight(WorldOpenAction::PreparePackage { .. }) => Some("prepare"),
            Self::Preflight(WorldOpenAction::OpenReadOnly) => Some("read-only"),
            Self::Preflight(WorldOpenAction::Export) | Self::Export => Some("export"),
            Self::Preflight(WorldOpenAction::RestoreCheckpoint { .. }) => {
                Some("restore-checkpoint")
            }
            Self::Preflight(WorldOpenAction::RepairHeader { .. }) => Some("repair"),
            Self::Preflight(WorldOpenAction::CloneAndMigrate { .. }) => Some("migrate"),
            Self::Preflight(WorldOpenAction::RecoverStaleLease) => Some("lease"),
            Self::InspectRecovery => Some("inspect"),
            Self::CreateCheckpoint => Some("checkpoint"),
            Self::Duplicate => Some("clone"),
            Self::Rename => Some("rename"),
            Self::MoveToTrash => Some("trash"),
            Self::OpenStorageLocation => Some("storage"),
            Self::Details => Some("details"),
        }
    }

    /// Logical action advertised on the corresponding semantic child.
    #[must_use]
    pub const fn semantic_action(&self) -> SemanticActionId {
        match self {
            Self::PlayExact | Self::Preflight(WorldOpenAction::UseFrozenLock) => {
                SemanticActionId::PlayExact
            }
            Self::RunPreflight
            | Self::Preflight(
                WorldOpenAction::OpenReadOnly
                | WorldOpenAction::PreparePackage { .. }
                | WorldOpenAction::ResolveCompatibleGraph
                | WorldOpenAction::RepairHeader { .. },
            ) => SemanticActionId::RunPreflight,
            Self::CreateCheckpoint => SemanticActionId::CreateCheckpoint,
            Self::Duplicate | Self::Preflight(WorldOpenAction::CloneAndMigrate { .. }) => {
                SemanticActionId::CloneWorld
            }
            Self::Export | Self::Preflight(WorldOpenAction::Export) => {
                SemanticActionId::ExportWorld
            }
            Self::MoveToTrash => SemanticActionId::MoveToTrash,
            Self::InspectRecovery => SemanticActionId::InspectRecovery,
            Self::Preflight(WorldOpenAction::RestoreCheckpoint { .. }) => {
                SemanticActionId::RestoreCheckpoint
            }
            Self::Preflight(WorldOpenAction::RecoverStaleLease) => {
                SemanticActionId::RecoverStaleLease
            }
            Self::Rename | Self::OpenStorageLocation | Self::Details => SemanticActionId::Activate,
        }
    }
}

/// Stable world-list sort modes.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum WorldSort {
    /// Most recently played first.
    LastPlayed,
    /// Most recently created first.
    Created,
    /// Unicode display label ascending.
    Name,
    /// Largest known world first; unknown last.
    Size,
    /// Healthiest/openable first.
    Health,
}

/// Deterministically sorted visible world list with stable focus restoration.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorldListModel {
    records: Vec<WorldShellRecord>,
    sort: WorldSort,
    focused: Option<SemanticNodeId>,
}

impl WorldListModel {
    /// Sorts records with identity as a deterministic final tie breaker.
    #[must_use]
    pub fn new(records: Vec<WorldShellRecord>, sort: WorldSort) -> Self {
        let mut model = Self {
            records,
            sort,
            focused: None,
        };
        model.sort_records();
        model.focused = model.records.first().map(WorldShellRecord::semantic_id);
        model
    }

    /// Returns rows in visible order, including failed entries.
    #[must_use]
    pub fn records(&self) -> &[WorldShellRecord] {
        &self.records
    }

    /// Returns the stable focused key.
    #[must_use]
    pub const fn focused(&self) -> Option<&SemanticNodeId> {
        self.focused.as_ref()
    }

    /// Sets focus by stable key rather than row index.
    ///
    /// # Errors
    ///
    /// Returns [`WorldShellError::UnknownFocusKey`] when the key is absent.
    pub fn focus(&mut self, key: SemanticNodeId) -> Result<(), WorldShellError> {
        if self
            .records
            .iter()
            .any(|record| record.semantic_id() == key)
        {
            self.focused = Some(key);
            Ok(())
        } else {
            Err(WorldShellError::UnknownFocusKey)
        }
    }

    /// Replaces asynchronous scan results while restoring the same stable key.
    pub fn refresh(&mut self, records: Vec<WorldShellRecord>) {
        let previous = self.focused.clone();
        self.records = records;
        self.sort_records();
        self.focused = previous
            .filter(|key| self.records.iter().any(|row| row.semantic_id() == *key))
            .or_else(|| self.records.first().map(WorldShellRecord::semantic_id));
    }

    /// Returns the home action based on the most recent world, not the first
    /// exact-ready world hidden behind a newer unhealthy one.
    #[must_use]
    pub fn home_primary_action(&self) -> HomePrimaryAction {
        let recent = self
            .records
            .iter()
            .max_by(|left, right| compare_recent(left, right));
        match recent {
            Some(record)
                if matches!(
                    record.health(),
                    WorldHealth::OpenStatus(WorldOpenStatus::ReadyExact)
                ) =>
            {
                HomePrimaryAction::Continue {
                    world_id: record.world_id(),
                    label: record.display_label(),
                }
            }
            Some(record) if record.metadata.last_played_at_ms > 0 => HomePrimaryAction::Review {
                world_id: record.world_id(),
                label: record.display_label(),
                health: record.health(),
            },
            _ => HomePrimaryAction::Worlds,
        }
    }

    fn sort_records(&mut self) {
        let sort = self.sort;
        self.records.sort_by(|left, right| {
            let primary = match sort {
                WorldSort::LastPlayed => right
                    .metadata
                    .last_played_at_ms
                    .cmp(&left.metadata.last_played_at_ms),
                WorldSort::Created => right
                    .metadata
                    .created_at_ms
                    .cmp(&left.metadata.created_at_ms),
                WorldSort::Name => left.display_label().cmp(&right.display_label()),
                WorldSort::Size => right
                    .metadata
                    .physical_bytes
                    .cmp(&left.metadata.physical_bytes),
                WorldSort::Health => left.health().rank().cmp(&right.health().rank()),
            };
            primary.then_with(|| left.entry.location.cmp(&right.entry.location))
        });
    }
}

fn compare_recent(left: &WorldShellRecord, right: &WorldShellRecord) -> Ordering {
    left.metadata
        .last_played_at_ms
        .cmp(&right.metadata.last_played_at_ms)
        .then_with(|| right.entry.location.cmp(&left.entry.location))
}

/// Home-page primary world action.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum HomePrimaryAction {
    /// No recent world exists; open the library.
    Worlds,
    /// Exact-ready recent world may be continued directly.
    Continue {
        /// Exact-ready world.
        world_id: WorldId,
        /// Current display label.
        label: String,
    },
    /// Recent world needs explicit review before any write.
    Review {
        /// World requiring review.
        world_id: WorldId,
        /// Current or recovery label.
        label: String,
        /// Reason category shown beside the action.
        health: WorldHealth,
    },
}

/// Minimal user intent for safe-default world creation.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct QuickCreateIntent {
    /// Validated user-facing world name.
    pub display_name: DisplayName,
    /// Template contributed by a shell-visible package.
    pub template: StableId,
    /// Root game package resolved during the create transaction.
    pub root_game_package: PackageName,
    /// Current profile lock whose safe defaults are summarized in the UI.
    pub profile_lock: CanonicalHash,
}

impl QuickCreateIntent {
    /// Validates the display name while retaining template/profile intent only.
    ///
    /// This does not copy a directory or create a half-published world.
    ///
    /// # Errors
    ///
    /// Returns the world-catalog display-name validation error.
    pub fn new(
        display_name: &str,
        template: StableId,
        root_game_package: PackageName,
        profile_lock: CanonicalHash,
    ) -> Result<Self, latticeaxiom_world_catalog::DisplayNameError> {
        Ok(Self {
            display_name: DisplayName::new(display_name)?,
            template,
            root_game_package,
            profile_lock,
        })
    }
}

/// Catalog state including recoverable managed-trash entries.
#[derive(Clone, Debug, Default)]
pub struct WorldLibraryState {
    live: BTreeMap<LiveWorldLocation, WorldShellRecord>,
    trash: BTreeMap<ManagedTrashLocation, TrashedWorldRecord>,
}

impl WorldLibraryState {
    /// Creates a library from visible live records.
    #[must_use]
    pub fn new(records: impl IntoIterator<Item = WorldShellRecord>) -> Self {
        Self {
            live: records
                .into_iter()
                .map(|record| (record.entry.location, record))
                .collect(),
            trash: BTreeMap::new(),
        }
    }

    /// Inserts a published live record without opening a writer.
    ///
    /// # Errors
    ///
    /// Returns [`WorldShellError::LiveLocationConflict`] when the structural
    /// location is already occupied.
    pub fn insert_live(&mut self, record: WorldShellRecord) -> Result<(), WorldShellError> {
        if self.live.contains_key(&record.entry.location) {
            return Err(WorldShellError::LiveLocationConflict);
        }
        self.live.insert(record.entry.location, record);
        Ok(())
    }

    /// Attaches a metadata-only preflight plan to a live world.
    ///
    /// # Errors
    ///
    /// Returns [`WorldShellError`] when the world is absent or the plan belongs
    /// to another identity.
    pub fn attach_preflight(
        &mut self,
        world_id: WorldId,
        plan: WorldOpenPlan,
    ) -> Result<(), WorldShellError> {
        if plan.world_id != world_id {
            return Err(WorldShellError::PlanIdentityMismatch);
        }
        let record = self
            .live
            .values_mut()
            .find(|record| record.world_id() == world_id)
            .ok_or(WorldShellError::MissingLiveWorld)?;
        record.open_plan = Some(plan);
        Ok(())
    }

    /// Attaches checkpoint/clone/export evidence without opening a writer.
    ///
    /// # Errors
    ///
    /// Returns [`WorldShellError::MissingLiveWorld`] when the identity is absent.
    pub fn attach_lifecycle(
        &mut self,
        world_id: WorldId,
        evidence: WorldLifecycleEvidence,
    ) -> Result<(), WorldShellError> {
        let record = self
            .live
            .values_mut()
            .find(|record| record.world_id() == world_id)
            .ok_or(WorldShellError::MissingLiveWorld)?;
        record.lifecycle = Some(evidence);
        Ok(())
    }

    /// Returns one live record by immutable world identity.
    #[must_use]
    pub fn live_by_id(&self, world_id: WorldId) -> Option<&WorldShellRecord> {
        self.live
            .values()
            .find(|record| record.world_id() == world_id)
    }

    /// Returns live entries in structural stable order.
    #[must_use]
    pub const fn live(&self) -> &BTreeMap<LiveWorldLocation, WorldShellRecord> {
        &self.live
    }

    /// Returns managed-trash entries in structural stable order.
    #[must_use]
    pub const fn trash(&self) -> &BTreeMap<ManagedTrashLocation, TrashedWorldRecord> {
        &self.trash
    }

    /// Plans a move only after writer shutdown/drain evidence.
    ///
    /// # Errors
    ///
    /// Returns [`WorldShellError`] if the source is absent or the underlying
    /// managed-trash contract rejects the destination/barrier.
    pub fn plan_move_to_trash(
        &self,
        source: LiveWorldLocation,
        destination: ManagedTrashLocation,
        writer: WriterBarrier,
        same_volume: bool,
    ) -> Result<MoveToTrashPlan, WorldShellError> {
        if !self.live.contains_key(&source) {
            return Err(WorldShellError::MissingLiveWorld);
        }
        MoveToTrashPlan::new(source, destination, writer, same_volume)
            .map_err(WorldShellError::TrashPlan)
    }

    /// Applies executor success: the source disappears from live catalog and
    /// appears under managed trash with its tombstone.
    ///
    /// # Errors
    ///
    /// Returns [`WorldShellError`] for identity mismatch or stale completion.
    pub fn complete_move_to_trash(
        &mut self,
        plan: MoveToTrashPlan,
        tombstone: TrashTombstone,
    ) -> Result<(), WorldShellError> {
        if plan.source.world_id != tombstone.world_id
            || plan.destination.world_id != tombstone.world_id
        {
            return Err(WorldShellError::TrashIdentityMismatch);
        }
        let record = self
            .live
            .remove(&plan.source)
            .ok_or(WorldShellError::MissingLiveWorld)?;
        self.trash.insert(
            plan.destination.clone(),
            TrashedWorldRecord {
                location: plan.destination,
                tombstone,
                metadata: record.metadata,
            },
        );
        Ok(())
    }

    /// Forms a non-overwriting restore or restore-as-clone plan.
    ///
    /// # Errors
    ///
    /// Returns [`WorldShellError::MissingTrashEntry`] for an unknown entry.
    pub fn plan_restore(
        &self,
        source: &ManagedTrashLocation,
        target_root: WorldRootId,
        mode: RestoreMode,
    ) -> Result<RestorePlanningOutcome, WorldShellError> {
        let entry = self
            .trash
            .get(source)
            .ok_or(WorldShellError::MissingTrashEntry)?;
        let live_ids = self.live.values().map(WorldShellRecord::world_id).collect();
        Ok(plan_restore(
            source.clone(),
            &entry.tombstone,
            target_root,
            &live_ids,
            mode,
        ))
    }

    /// Applies a successful external restore and removes the trash entry.
    ///
    /// # Errors
    ///
    /// Returns [`WorldShellError`] for missing source, duplicate location, or
    /// an executor result whose identity does not match the restore plan.
    pub fn complete_restore(
        &mut self,
        source: &ManagedTrashLocation,
        restored: WorldShellRecord,
    ) -> Result<(), WorldShellError> {
        if !self.trash.contains_key(source) {
            return Err(WorldShellError::MissingTrashEntry);
        }
        if source.world_id != restored.world_id() {
            return Err(WorldShellError::TrashIdentityMismatch);
        }
        if self.live.contains_key(&restored.entry.location) {
            return Err(WorldShellError::LiveLocationConflict);
        }
        self.trash.remove(source);
        self.live.insert(restored.entry.location, restored);
        Ok(())
    }
}

/// Visible managed-trash record.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TrashedWorldRecord {
    /// Structural managed-trash location.
    pub location: ManagedTrashLocation,
    /// Bounded deletion tombstone.
    pub tombstone: TrashTombstone,
    /// Retained card metadata.
    pub metadata: WorldCardMetadata,
}

/// Invalid world-shell state or lifecycle operation.
#[derive(Clone, Debug, Error, PartialEq)]
pub enum WorldShellError {
    /// Attached plan belongs to another world.
    #[error("world-open plan identity does not match catalog entry")]
    PlanIdentityMismatch,
    /// Async focus key is absent.
    #[error("world focus key does not exist")]
    UnknownFocusKey,
    /// Source live world is absent.
    #[error("live world does not exist")]
    MissingLiveWorld,
    /// Managed-trash source is absent.
    #[error("managed-trash entry does not exist")]
    MissingTrashEntry,
    /// Executor completion mixed identities.
    #[error("managed-trash identity does not match world/tombstone")]
    TrashIdentityMismatch,
    /// Restored structural location already exists.
    #[error("restored live-world location already exists")]
    LiveLocationConflict,
    /// An in-memory session with this identity already exists.
    #[error("in-memory world identity already exists")]
    DuplicateWorldId,
    /// Underlying managed-trash policy rejected the plan.
    #[error(transparent)]
    TrashPlan(latticeaxiom_world_catalog::TrashPlanError),
}
