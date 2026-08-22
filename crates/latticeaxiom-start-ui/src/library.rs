//! Catalog-owned create/continue/preflight/recovery/checkpoint/clone/trash flow.
//!
//! The shell prepares one-shot [`LaunchHandoff`] values without opening a
//! world writer. Hosts execute accepted plans through storage boundaries.

use std::collections::BTreeMap;

use latticeaxiom_core::WorldId;
use latticeaxiom_world_catalog::{
    CatalogEntry, CatalogEntryState, CatalogProjection, CheckpointId, CheckpointPlan,
    CheckpointReason, ClonePlan, CreateWorldPlan, DiskAdmission, DiskSample, DisplayNameError, GIB,
    HeadroomInputs, LowDiskMonitor, ManagedTrashLocation, MoveToTrashPlan, RestoreMode,
    RestorePlanningOutcome, TrashEntryId, WorldOpenPlan, WorldRootId, WriterBarrier,
    plan_checkpoint, plan_clone_world, plan_create_world, plan_export_world,
};
use thiserror::Error;

use crate::{
    ClientShellGraph, HomePrimaryAction, LaunchHandoff, LaunchHandoffContext, LaunchHandoffError,
    LoadingState, QuickCreateIntent, SemanticCommand, ShellCommandError, ShellEffect, ShellScreen,
    StartShellModel, WorldCardMetadata, WorldLibraryState, WorldListModel, WorldShellError,
    WorldShellRecord, WorldSort,
};

/// Default allowlisted root used by the catalog shell until a host binds one.
pub const WORLD_LIBRARY_ROOT: WorldRootId = WorldRootId(1);

/// Semantic world-library coordinator over [`WorldLibraryState`].
///
/// This flow never opens a writer and never enters Playing in the shell
/// process. Continue of a `ReadyExact` world prepares [`LaunchHandoff`].
#[derive(Clone, Debug)]
pub struct WorldLibraryFlow {
    shell: StartShellModel,
    library: WorldLibraryState,
    draft: Option<QuickCreateIntent>,
    now_ms: u64,
    root: WorldRootId,
    writer: WriterBarrier,
    disk: DiskAdmission,
    launch_context: Option<LaunchHandoffContext>,
    pending_creates: BTreeMap<WorldId, CreateWorldPlan>,
    prepared_launch: Option<LaunchHandoff>,
}

impl WorldLibraryFlow {
    /// Builds an empty home library over a validated package graph.
    #[must_use]
    pub fn new(graph: ClientShellGraph) -> Self {
        let library = WorldLibraryState::new([]);
        Self {
            shell: StartShellModel::new(graph, library_list(&library)),
            library,
            draft: None,
            now_ms: 1,
            root: WORLD_LIBRARY_ROOT,
            writer: WriterBarrier::ClosedAndDrained,
            disk: shell_disk_admission(),
            launch_context: None,
            pending_creates: BTreeMap::new(),
            prepared_launch: None,
        }
    }

    /// Returns the presentation-neutral shell.
    #[must_use]
    pub const fn shell(&self) -> &StartShellModel {
        &self.shell
    }

    /// Returns the catalog live/trash state.
    #[must_use]
    pub const fn library(&self) -> &WorldLibraryState {
        &self.library
    }

    /// Returns the pending quick-create intent, when set.
    #[must_use]
    pub const fn draft(&self) -> Option<&QuickCreateIntent> {
        self.draft.as_ref()
    }

    /// Returns the sealed replacement-process handoff, when prepared.
    #[must_use]
    pub const fn prepared_launch(&self) -> Option<&LaunchHandoff> {
        self.prepared_launch.as_ref()
    }

    /// Clock used when planning create/checkpoint/trash identities.
    pub const fn set_now_ms(&mut self, now_ms: u64) {
        self.now_ms = now_ms;
    }

    /// Stores the typed intent applied by the next quick-create command.
    pub fn set_draft(&mut self, intent: QuickCreateIntent) {
        self.draft = Some(intent);
    }

    /// Supplies lock fingerprints required to seal [`LaunchHandoff`].
    pub const fn set_launch_context(&mut self, context: LaunchHandoffContext) {
        self.launch_context = Some(context);
    }

    /// Host disk admission used by checkpoint/clone/export/trash planning.
    pub const fn set_disk(&mut self, disk: DiskAdmission) {
        self.disk = disk;
    }

    /// Plans a new world identity and records a scan-only catalog row.
    ///
    /// The row is not `ReadyExact` and does not open a writer. Continue stays
    /// unavailable until metadata-only preflight is attached.
    ///
    /// # Errors
    ///
    /// Returns [`WorldLibraryError`] when the identity already exists.
    pub fn create(
        &mut self,
        intent: &QuickCreateIntent,
        world_id: WorldId,
    ) -> Result<CreateWorldPlan, WorldLibraryError> {
        let live_ids = self
            .library
            .live()
            .values()
            .map(WorldShellRecord::world_id)
            .collect();
        let plan = plan_create_world(
            world_id,
            self.root,
            intent.display_name.clone(),
            intent.template.clone(),
            intent.root_game_package.clone(),
            intent.profile_lock,
            &live_ids,
        )?;
        let record = unpublished_record(&plan, self.now_ms)?;
        self.library.insert_live(record)?;
        self.pending_creates.insert(world_id, plan.clone());
        self.draft = None;
        self.sync_worlds();
        self.shell.screen = ShellScreen::Home;
        Ok(plan)
    }

    /// Attaches a metadata-only preflight plan without opening a writer.
    ///
    /// # Errors
    ///
    /// Returns [`WorldLibraryError`] when the world is absent or the plan
    /// belongs to another identity.
    pub fn attach_preflight(
        &mut self,
        world_id: WorldId,
        plan: WorldOpenPlan,
    ) -> Result<(), WorldLibraryError> {
        self.library.attach_preflight(world_id, plan)?;
        self.sync_worlds();
        Ok(())
    }

    /// Attaches checkpoint/clone/export evidence without opening a writer.
    ///
    /// # Errors
    ///
    /// Returns [`WorldLibraryError`] when the world is absent.
    pub fn attach_lifecycle(
        &mut self,
        world_id: WorldId,
        evidence: latticeaxiom_world_catalog::WorldLifecycleEvidence,
    ) -> Result<(), WorldLibraryError> {
        self.library.attach_lifecycle(world_id, evidence)?;
        self.sync_worlds();
        Ok(())
    }

    /// Seals a one-shot replacement-process launch without opening a writer.
    ///
    /// # Errors
    ///
    /// Returns [`WorldLibraryError`] when the world is not exact-ready or the
    /// host has not supplied launch-handoff context.
    pub fn prepare_launch(
        &mut self,
        world_id: WorldId,
    ) -> Result<LaunchHandoff, WorldLibraryError> {
        let record = self
            .library
            .live_by_id(world_id)
            .ok_or(WorldShellError::MissingLiveWorld)?;
        let context = self
            .launch_context
            .ok_or(WorldLibraryError::MissingLaunchContext)?;
        let handoff = LaunchHandoff::for_ready_exact(record, context)?;
        self.shell.loading = Some(LoadingState::new());
        self.shell.screen = ShellScreen::Loading;
        self.prepared_launch = Some(handoff.clone());
        Ok(handoff)
    }

    /// Returns the exact-ready Continue target, when one exists.
    #[must_use]
    pub fn continue_world_id(&self) -> Option<WorldId> {
        match self.shell.worlds.home_primary_action() {
            HomePrimaryAction::Continue { world_id, .. } => Some(world_id),
            _ => None,
        }
    }

    /// Validates and applies a semantic command against the current tree.
    ///
    /// # Errors
    ///
    /// Returns [`WorldLibraryError`] when the tree rejects the command or a
    /// catalog plan cannot be formed without a writer.
    pub fn inject(
        &mut self,
        command: &SemanticCommand,
    ) -> Result<WorldLibraryEffect, WorldLibraryError> {
        match self.shell.inject(command)? {
            ShellEffect::RequestQuickCreate => {
                let intent = self
                    .draft
                    .clone()
                    .ok_or(WorldLibraryError::MissingQuickCreateDraft)?;
                let plan = self.create(&intent, WorldId::new_v4())?;
                Ok(WorldLibraryEffect::Created(plan))
            }
            ShellEffect::RequestExactWorldLaunch(world_id) => {
                let handoff = self.prepare_launch(world_id)?;
                Ok(WorldLibraryEffect::PreparedLaunch(handoff))
            }
            ShellEffect::RequestCheckpoint(world_id) => Ok(WorldLibraryEffect::Checkpoint(
                self.plan_checkpoint(world_id)?,
            )),
            ShellEffect::RequestClone(world_id) => {
                Ok(WorldLibraryEffect::Clone(self.plan_clone(world_id)?))
            }
            ShellEffect::RequestExport(world_id) => {
                Ok(WorldLibraryEffect::Export(self.plan_export(world_id)?))
            }
            ShellEffect::RequestMoveToTrash(world_id) => {
                Ok(WorldLibraryEffect::Trash(self.plan_trash(world_id)?))
            }
            ShellEffect::RequestRestoreTrash(world_id) => Ok(WorldLibraryEffect::Restore(
                self.plan_restore_trash(world_id)?,
            )),
            ShellEffect::RequestRunPreflight(world_id)
            | ShellEffect::RequestInspectRecovery(world_id)
            | ShellEffect::RequestRestoreCheckpoint(world_id) => {
                self.shell.selected = Some(world_id);
                self.shell.screen = ShellScreen::Worlds;
                Ok(WorldLibraryEffect::Review(world_id))
            }
            effect => Ok(WorldLibraryEffect::Shell(effect)),
        }
    }

    fn plan_checkpoint(&self, world_id: WorldId) -> Result<CheckpointPlan, WorldLibraryError> {
        let record = self
            .library
            .live_by_id(world_id)
            .ok_or(WorldShellError::MissingLiveWorld)?;
        let evidence = record
            .lifecycle
            .as_ref()
            .ok_or(WorldLibraryError::MissingLifecycleEvidence)?;
        let checkpoint_id = CheckpointId::new(&format!("checkpoint-{}", self.now_ms))
            .map_err(|_| WorldLibraryError::InvalidCheckpointId)?;
        Ok(plan_checkpoint(
            record.entry.location,
            checkpoint_id,
            CheckpointReason::ManualPinned,
            evidence,
            &evidence.checkpoints,
            self.writer,
            self.disk,
        )?)
    }

    fn plan_clone(&self, world_id: WorldId) -> Result<ClonePlan, WorldLibraryError> {
        let record = self
            .library
            .live_by_id(world_id)
            .ok_or(WorldShellError::MissingLiveWorld)?;
        let evidence = record
            .lifecycle
            .as_ref()
            .ok_or(WorldLibraryError::MissingLifecycleEvidence)?;
        let checkpoint = evidence
            .checkpoints
            .iter()
            .rev()
            .find(|checkpoint| checkpoint.restore_verified)
            .ok_or(WorldLibraryError::MissingVerifiedCheckpoint)?;
        let live_ids = self
            .library
            .live()
            .values()
            .map(WorldShellRecord::world_id)
            .collect();
        Ok(plan_clone_world(
            record.entry.location,
            checkpoint,
            WorldId::new_v4(),
            self.root,
            &live_ids,
            self.writer,
            self.disk,
        )?)
    }

    fn plan_export(
        &self,
        world_id: WorldId,
    ) -> Result<latticeaxiom_world_catalog::ExportPlan, WorldLibraryError> {
        let record = self
            .library
            .live_by_id(world_id)
            .ok_or(WorldShellError::MissingLiveWorld)?;
        let evidence = record
            .lifecycle
            .as_ref()
            .ok_or(WorldLibraryError::MissingLifecycleEvidence)?;
        Ok(plan_export_world(
            record.entry.location,
            evidence,
            record.metadata.physical_bytes.unwrap_or(0),
            self.disk,
        )?)
    }

    fn plan_trash(&self, world_id: WorldId) -> Result<MoveToTrashPlan, WorldLibraryError> {
        let record = self
            .library
            .live_by_id(world_id)
            .ok_or(WorldShellError::MissingLiveWorld)?;
        let entry_id = TrashEntryId::new(&format!("delete-{}", self.now_ms))
            .map_err(|_| WorldLibraryError::InvalidTrashEntryId)?;
        let destination = ManagedTrashLocation {
            root: record.entry.location.root,
            world_id,
            entry_id,
        };
        Ok(self.library.plan_move_to_trash(
            record.entry.location,
            destination,
            self.writer,
            true,
        )?)
    }

    fn plan_restore_trash(
        &self,
        world_id: WorldId,
    ) -> Result<RestorePlanningOutcome, WorldLibraryError> {
        let (source, _) = self
            .library
            .trash()
            .iter()
            .find(|(location, _)| location.world_id == world_id)
            .ok_or(WorldShellError::MissingTrashEntry)?;
        Ok(self
            .library
            .plan_restore(source, self.root, RestoreMode::OriginalIdentity)?)
    }

    fn sync_worlds(&mut self) {
        let records = self.library.live().values().cloned().collect();
        if self.shell.worlds.records().is_empty() {
            self.shell.worlds = library_list(&self.library);
        } else {
            self.shell.worlds.refresh(records);
        }
        self.shell.trash = self.library.trash().values().cloned().collect();
    }
}

/// Observable result of one accepted world-library command.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum WorldLibraryEffect {
    /// Quick-create planned a new identity without opening a writer.
    Created(CreateWorldPlan),
    /// One-shot replacement-process launch is sealed.
    PreparedLaunch(LaunchHandoff),
    /// Checkpoint plan ready for an external executor.
    Checkpoint(CheckpointPlan),
    /// Clone plan ready for an external executor.
    Clone(ClonePlan),
    /// Export plan ready for an external executor.
    Export(latticeaxiom_world_catalog::ExportPlan),
    /// Managed-trash move plan ready for an external executor.
    Trash(MoveToTrashPlan),
    /// Managed-trash restore plan or identity-conflict outcome.
    Restore(RestorePlanningOutcome),
    /// Recovery/preflight review selected a world.
    Review(WorldId),
    /// Shell routing that did not produce a catalog plan.
    Shell(ShellEffect),
}

/// Invalid world-library command or plan.
#[derive(Debug, Error)]
pub enum WorldLibraryError {
    /// Accessibility tree or current route rejected the command.
    #[error(transparent)]
    Shell(#[from] ShellCommandError),
    /// Live/trash catalog mutation failed.
    #[error(transparent)]
    World(#[from] WorldShellError),
    /// Create identity was not unique.
    #[error(transparent)]
    Create(#[from] latticeaxiom_world_catalog::CreateWorldPlanError),
    /// Checkpoint planning failed.
    #[error(transparent)]
    Checkpoint(#[from] latticeaxiom_world_catalog::CheckpointPlanError),
    /// Clone planning failed.
    #[error(transparent)]
    Clone(#[from] latticeaxiom_world_catalog::ClonePlanError),
    /// Export planning failed.
    #[error(transparent)]
    Export(#[from] latticeaxiom_world_catalog::ExportPlanError),
    /// Replacement-process handoff could not be sealed.
    #[error(transparent)]
    Launch(#[from] LaunchHandoffError),
    /// Quick-create display name failed validation.
    #[error(transparent)]
    DisplayName(#[from] DisplayNameError),
    /// Quick-create was activated without a typed intent.
    #[error("quick-create draft is missing")]
    MissingQuickCreateDraft,
    /// Checkpoint/clone/export requires catalog lifecycle evidence.
    #[error("world lifecycle evidence is required")]
    MissingLifecycleEvidence,
    /// Launch handoff requires host lock fingerprints.
    #[error("launch handoff context is missing")]
    MissingLaunchContext,
    /// Clone requires a restore-verified checkpoint.
    #[error("clone requires a restore-verified checkpoint")]
    MissingVerifiedCheckpoint,
    /// Checkpoint identity token was not a bounded opaque ID.
    #[error("checkpoint identity is invalid")]
    InvalidCheckpointId,
    /// Managed-trash entry token was not a bounded opaque ID.
    #[error("managed-trash entry identity is invalid")]
    InvalidTrashEntryId,
}

fn library_list(library: &WorldLibraryState) -> WorldListModel {
    WorldListModel::new(
        library.live().values().cloned().collect(),
        WorldSort::LastPlayed,
    )
}

fn unpublished_record(
    plan: &CreateWorldPlan,
    now_ms: u64,
) -> Result<WorldShellRecord, WorldShellError> {
    WorldShellRecord::new(
        CatalogEntry {
            location: plan.location,
            state: CatalogEntryState::Projected(CatalogProjection {
                world_id: plan.world_id,
                display_name: plan.display_name.clone(),
                metadata_epoch: 1,
                clean_shutdown: true,
                durable_frontier: 0,
            }),
        },
        WorldCardMetadata {
            created_at_ms: now_ms,
            last_played_at_ms: now_ms,
            physical_bytes: None,
            game_summary: Some(plan.root_game_package.to_string()),
            dimension_summary: None,
        },
        None,
    )
}

fn shell_disk_admission() -> DiskAdmission {
    let mut monitor = LowDiskMonitor::new();
    match monitor.evaluate(
        DiskSample {
            usable_free_bytes: 100 * GIB,
            capacity_bytes: 100 * GIB,
        },
        HeadroomInputs::default(),
        None,
        latticeaxiom_world_catalog::DirtyDrainState::Clean,
    ) {
        Ok(admission) => admission,
        Err(error) => unreachable!("default shell disk thresholds are bounded: {error}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use latticeaxiom_core::{CanonicalHash, PackageName};
    use latticeaxiom_launcher::{LaunchGeneration, SettingTransactionRevision};
    use latticeaxiom_world_catalog::{
        CheckpointClass, CheckpointFingerprint, CrashMarker, ReconciliationState,
        RecordedCheckpoint, WorldLifecycleEvidence, WorldOpenAction, WorldOpenRisk,
        WorldOpenStatus,
    };

    use crate::{
        InputSource, SemanticActionId, SemanticCommand, SemanticNodeId, ShellCapability,
        ShellPackageProvider, memory_session_template,
    };

    fn graph() -> ClientShellGraph {
        ClientShellGraph::resolve([
            provider("@latticeaxiom/front-end", ShellCapability::ClientShell),
            provider("@latticeaxiom/world-library", ShellCapability::WorldCatalog),
            provider(
                "@latticeaxiom/settings-ui",
                ShellCapability::SettingsSurface,
            ),
            provider("@latticeaxiom/settings", ShellCapability::SettingsRegistry),
            provider(
                "@latticeaxiom/observability",
                ShellCapability::DiagnosticRegistry,
            ),
        ])
        .unwrap_or_else(|error| panic!("{error}"))
    }

    fn provider(package: &str, capability: ShellCapability) -> ShellPackageProvider {
        ShellPackageProvider {
            package: package
                .parse()
                .unwrap_or_else(|error| panic!("package fixture: {error}")),
            capability,
        }
    }

    fn intent() -> QuickCreateIntent {
        QuickCreateIntent::new(
            "Library World",
            memory_session_template(),
            package("@example/game"),
            CanonicalHash::digest(b"profile"),
        )
        .unwrap_or_else(|error| panic!("{error}"))
    }

    fn package(value: &str) -> PackageName {
        value
            .parse()
            .unwrap_or_else(|error| panic!("package fixture: {error}"))
    }

    fn exact_plan(world_id: WorldId) -> WorldOpenPlan {
        WorldOpenPlan {
            world_id,
            status: WorldOpenStatus::ReadyExact,
            risk: WorldOpenRisk::None,
            reconciliation: ReconciliationState::InSync { metadata_epoch: 1 },
            next_safe_step: Some(WorldOpenAction::UseFrozenLock),
            actions: vec![WorldOpenAction::UseFrozenLock],
            diagnostics: Vec::new(),
            activation_binding: None,
        }
    }

    fn lifecycle() -> WorldLifecycleEvidence {
        WorldLifecycleEvidence {
            fingerprint: CheckpointFingerprint {
                source_revision: 2,
                exact_lock_hash: CanonicalHash::digest(b"lock"),
                metadata_hash: CanonicalHash::digest(b"metadata-2"),
            },
            checkpoints: vec![RecordedCheckpoint {
                id: CheckpointId::new("checkpoint-1").unwrap_or_else(|error| panic!("{error}")),
                fingerprint: CheckpointFingerprint {
                    source_revision: 1,
                    exact_lock_hash: CanonicalHash::digest(b"lock"),
                    metadata_hash: CanonicalHash::digest(b"metadata"),
                },
                class: CheckpointClass::Protected,
                restore_verified: true,
            }],
            header_checksum: CanonicalHash::digest(b"header"),
            metadata_checksum: CanonicalHash::digest(b"metadata"),
            crash_marker: CrashMarker::Absent,
        }
    }

    #[test]
    fn create_stays_review_until_preflight_then_continue_prepares_launch() {
        let mut flow = WorldLibraryFlow::new(graph());
        flow.set_now_ms(10);
        flow.set_launch_context(LaunchHandoffContext {
            generation: LaunchGeneration::FIRST,
            issued_at_ms: 10,
            expires_at_ms: 70_000,
            shell_lock_hash: CanonicalHash::digest(b"shell"),
            world_lock_hash: CanonicalHash::digest(b"world"),
            confirmed_setting_transaction_revision: SettingTransactionRevision::new(1),
        });
        let world = WorldId::new_v4();
        let created = flow
            .create(&intent(), world)
            .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(created.world_id, world);
        assert!(flow.continue_world_id().is_none());
        assert!(matches!(
            flow.library()
                .live_by_id(world)
                .map(WorldShellRecord::card_state),
            Some(latticeaxiom_world_catalog::CatalogCardState::Recoverable)
        ));

        flow.attach_preflight(world, exact_plan(world))
            .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(flow.continue_world_id(), Some(world));

        let effect = flow
            .inject(&SemanticCommand {
                target: SemanticNodeId::new("home/continue")
                    .unwrap_or_else(|error| panic!("{error}")),
                action: SemanticActionId::ContinueWorld,
                source: InputSource::Headless,
            })
            .unwrap_or_else(|error| panic!("{error}"));
        assert!(matches!(effect, WorldLibraryEffect::PreparedLaunch(_)));
        assert_eq!(flow.shell().screen, ShellScreen::Loading);
        assert_eq!(
            flow.shell()
                .loading
                .as_ref()
                .map(crate::LoadingState::cancel_disposition),
            Some(crate::LoadingCancelDisposition::CancelBeforeWriter)
        );
        assert!(flow.prepared_launch().is_some());
    }

    #[test]
    fn checkpoint_clone_and_trash_are_plans_without_a_writer() {
        let mut flow = WorldLibraryFlow::new(graph());
        flow.set_now_ms(4);
        let world = WorldId::new_v4();
        flow.create(&intent(), world)
            .unwrap_or_else(|error| panic!("{error}"));
        flow.attach_preflight(world, exact_plan(world))
            .unwrap_or_else(|error| panic!("{error}"));
        flow.attach_lifecycle(world, lifecycle())
            .unwrap_or_else(|error| panic!("{error}"));
        flow.inject(&SemanticCommand {
            target: SemanticNodeId::new("home/worlds").unwrap_or_else(|error| panic!("{error}")),
            action: SemanticActionId::OpenWorlds,
            source: InputSource::Headless,
        })
        .unwrap_or_else(|error| panic!("{error}"));

        let play_target = flow.shell().worlds.records()[0]
            .action_semantic_id(&crate::WorldCardAction::CreateCheckpoint)
            .unwrap_or_else(|| panic!("checkpoint control"));
        let checkpoint = flow
            .inject(&SemanticCommand {
                target: play_target,
                action: SemanticActionId::CreateCheckpoint,
                source: InputSource::Headless,
            })
            .unwrap_or_else(|error| panic!("{error}"));
        assert!(matches!(checkpoint, WorldLibraryEffect::Checkpoint(_)));

        let clone_target = flow.shell().worlds.records()[0]
            .action_semantic_id(&crate::WorldCardAction::Duplicate)
            .unwrap_or_else(|| panic!("clone control"));
        let clone = flow
            .inject(&SemanticCommand {
                target: clone_target,
                action: SemanticActionId::CloneWorld,
                source: InputSource::Headless,
            })
            .unwrap_or_else(|error| panic!("{error}"));
        let WorldLibraryEffect::Clone(plan) = clone else {
            panic!("expected clone plan");
        };
        assert_ne!(plan.new_world_id, world);
        assert!(!plan.copies_process_local_lease);

        let trash_target = flow.shell().worlds.records()[0]
            .action_semantic_id(&crate::WorldCardAction::MoveToTrash)
            .unwrap_or_else(|| panic!("trash control"));
        let trash = flow
            .inject(&SemanticCommand {
                target: trash_target,
                action: SemanticActionId::MoveToTrash,
                source: InputSource::Headless,
            })
            .unwrap_or_else(|error| panic!("{error}"));
        assert!(matches!(trash, WorldLibraryEffect::Trash(_)));
    }
}
