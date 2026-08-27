//! Catalog-owned create/continue/preflight/recovery/checkpoint/clone/trash flow.
//!
//! The shell prepares one-shot [`LaunchHandoff`] values without opening a
//! world writer. Hosts execute accepted plans through storage boundaries.

use std::collections::BTreeMap;

use latticeaxiom_core::{CanonicalHash, StableId, WorldId};
use latticeaxiom_world_catalog::{
    CatalogEntry, CatalogEntryState, CatalogProjection, CheckpointId, CheckpointPlan,
    CheckpointReason, ClonePlan, CreateWorldPlan, CreateWorldPlanInput, DiskAdmission, DiskSample,
    DisplayName, DisplayNameError, GIB, HeadroomInputs, LowDiskMonitor, ManagedTrashLocation,
    MoveToTrashPlan, RecordedCheckpoint, RestoreMode, RestorePlanningOutcome,
    SealedActivationBindingV1, StaleLeaseRecoveryPlan, TrashEntryId, TrashTombstone, WorldOpenPlan,
    WorldOpenStatus, WorldRootId, WriterBarrier, WriterLeaseState, plan_checkpoint,
    plan_clone_world, plan_create_world, plan_export_world, plan_recover_stale_lease,
};
use latticeaxiom_world_db::{
    ActivationPermitV1, StoragePreflightStatusV1, WorldStoragePreflightV1,
};
use thiserror::Error;

use crate::{
    ClientShellGraph, HomePrimaryAction, LaunchHandoff, LaunchHandoffContext, LaunchHandoffError,
    LoadingState, QuickCreateIntent, SemanticCommand, ShellCommandError, ShellEffect, ShellScreen,
    StartShellModel, WorldCardMetadata, WorldLibraryState, WorldListModel, WorldShellError,
    WorldShellRecord, WorldSort, WorldgenProfileOption,
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

    /// Installs generation profiles exposed by the active game/template.
    ///
    /// # Errors
    ///
    /// Returns [`ShellCommandError`] when the catalog is ambiguous or invalid.
    pub fn set_worldgen_profiles(
        &mut self,
        profiles: Vec<WorldgenProfileOption>,
        selected: &StableId,
    ) -> Result<(), ShellCommandError> {
        self.shell.set_worldgen_profiles(profiles, selected)
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
            CreateWorldPlanInput {
                world_id,
                root: self.root,
                display_name: intent.display_name.clone(),
                template: intent.template.clone(),
                root_game_package: intent.root_game_package.clone(),
                profile_lock: intent.profile_lock,
                generation_profile: intent.generation_profile.clone(),
            },
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

    /// Binds sealed storage-preflight evidence onto a `ReadyExact` plan.
    ///
    /// Missing, impure, or not-ready storage evidence fail closed. This never
    /// opens a writer.
    ///
    /// # Errors
    ///
    /// Returns [`WorldLibraryError`] when the world is absent, preflight is not
    /// pure, identities disagree, or `ReadyExact` lacks an activation permit.
    pub fn attach_storage_preflight(
        &mut self,
        world_id: WorldId,
        preflight: &WorldStoragePreflightV1,
    ) -> Result<(), WorldLibraryError> {
        let instrumentation = preflight.instrumentation();
        if instrumentation.writer_activations != 0
            || instrumentation.module_callbacks != 0
            || instrumentation.bevy_worlds_created != 0
            || instrumentation.authoritative_commands != 0
        {
            return Err(WorldLibraryError::PreflightSideEffect);
        }
        if preflight.world() != world_id {
            return Err(WorldLibraryError::StorageIdentityMismatch);
        }
        let record = self
            .library
            .live_by_id(world_id)
            .ok_or(WorldShellError::MissingLiveWorld)?;
        let Some(plan) = record.open_plan.clone() else {
            return Err(WorldLibraryError::MissingActivationEvidence);
        };
        if plan.status != WorldOpenStatus::ReadyExact {
            return Ok(());
        }
        if !matches!(
            preflight.status(),
            StoragePreflightStatusV1::ReadyForActivation
        ) {
            return Err(WorldLibraryError::MissingActivationEvidence);
        }
        let permit = preflight
            .activation_permit()
            .ok_or(WorldLibraryError::MissingActivationEvidence)?;
        let mut plan = plan;
        plan.activation_binding = Some(binding_from_permit(permit));
        self.library.attach_preflight(world_id, plan)?;
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
        let plan = record
            .open_plan
            .as_ref()
            .ok_or(WorldLibraryError::MissingActivationEvidence)?;
        if plan.activation_binding.is_none() {
            return Err(WorldLibraryError::MissingActivationEvidence);
        }
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
        let effect = self.shell.inject(command)?;
        if effect == ShellEffect::WorldgenProfileSelected
            && let Some(profile) = self.shell.selected_worldgen_profile().cloned()
            && let Some(draft) = self.draft.as_mut()
        {
            draft.set_generation_profile(profile);
        }
        match effect {
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
                self.plan_restore_trash(world_id, RestoreMode::OriginalIdentity)?,
            )),
            ShellEffect::RequestRestoreTrashAsClone(world_id) => {
                Ok(WorldLibraryEffect::Restore(self.plan_restore_trash(
                    world_id,
                    RestoreMode::AsClone {
                        new_world_id: WorldId::new_v4(),
                    },
                )?))
            }
            ShellEffect::RequestRecoverStaleLease(world_id) => Ok(WorldLibraryEffect::StaleLease(
                self.plan_stale_lease(world_id)?,
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
        mode: RestoreMode,
    ) -> Result<RestorePlanningOutcome, WorldLibraryError> {
        let (source, _) = self
            .library
            .trash()
            .iter()
            .find(|(location, _)| location.world_id == world_id)
            .ok_or(WorldShellError::MissingTrashEntry)?;
        Ok(self.library.plan_restore(source, self.root, mode)?)
    }

    fn plan_stale_lease(
        &self,
        world_id: WorldId,
    ) -> Result<StaleLeaseRecoveryPlan, WorldLibraryError> {
        let record = self
            .library
            .live_by_id(world_id)
            .ok_or(WorldShellError::MissingLiveWorld)?;
        let evidence = record
            .lifecycle
            .as_ref()
            .ok_or(WorldLibraryError::MissingLifecycleEvidence)?;
        Ok(plan_recover_stale_lease(
            record.entry.location,
            evidence,
            self.writer,
        )?)
    }

    /// Records a completed checkpoint without opening a writer.
    ///
    /// # Errors
    ///
    /// Returns [`WorldLibraryError`] when the world or lifecycle evidence is
    /// absent.
    pub fn complete_checkpoint(&mut self, plan: &CheckpointPlan) -> Result<(), WorldLibraryError> {
        let record = self
            .library
            .live_by_id(plan.world_id)
            .ok_or(WorldShellError::MissingLiveWorld)?;
        let mut evidence = record
            .lifecycle
            .clone()
            .ok_or(WorldLibraryError::MissingLifecycleEvidence)?;
        evidence.checkpoints.push(RecordedCheckpoint {
            id: plan.checkpoint_id.clone(),
            fingerprint: plan.fingerprint,
            class: plan.class,
            restore_verified: true,
        });
        self.library.attach_lifecycle(plan.world_id, evidence)?;
        self.sync_worlds();
        Ok(())
    }

    /// Records a completed clone as a scan-only catalog row.
    ///
    /// The clone is not `ReadyExact` until metadata-only preflight is attached.
    /// Process-local leases are never copied.
    ///
    /// # Errors
    ///
    /// Returns [`WorldLibraryError`] when the source is absent or the new
    /// identity already exists.
    pub fn complete_clone(
        &mut self,
        plan: &ClonePlan,
        display_name: DisplayName,
    ) -> Result<(), WorldLibraryError> {
        if plan.copies_process_local_lease {
            return Err(WorldLibraryError::LeaseCopied);
        }
        let source = self
            .library
            .live_by_id(plan.source.world_id)
            .ok_or(WorldShellError::MissingLiveWorld)?;
        let record = unpublished_clone_record(plan, source, display_name, self.now_ms)?;
        self.library.insert_live(record)?;
        self.sync_worlds();
        Ok(())
    }

    /// Applies a completed managed-trash move without opening a writer.
    ///
    /// # Errors
    ///
    /// Returns [`WorldLibraryError`] for identity mismatch or a missing source.
    pub fn complete_trash(
        &mut self,
        plan: MoveToTrashPlan,
        tombstone: TrashTombstone,
    ) -> Result<(), WorldLibraryError> {
        self.library.complete_move_to_trash(plan, tombstone)?;
        self.sync_worlds();
        self.shell.screen = ShellScreen::Trash;
        Ok(())
    }

    /// Applies a completed restore without opening a writer.
    ///
    /// # Errors
    ///
    /// Returns [`WorldLibraryError`] for missing trash, identity mismatch, or
    /// a live location conflict.
    pub fn complete_restore(
        &mut self,
        source: &ManagedTrashLocation,
        restored: WorldShellRecord,
    ) -> Result<(), WorldLibraryError> {
        self.library.complete_restore(source, restored)?;
        self.sync_worlds();
        self.shell.screen = ShellScreen::Worlds;
        Ok(())
    }

    /// Marks a stale exclusive lease recovered without opening a writer.
    ///
    /// Crash markers remain until a later clean preflight. Continue stays
    /// unavailable until a `ReadyExact` plan is attached.
    ///
    /// # Errors
    ///
    /// Returns [`WorldLibraryError`] when the world or lifecycle evidence is
    /// absent, or the plan copies a process-local lease.
    pub fn complete_stale_lease(
        &mut self,
        plan: &StaleLeaseRecoveryPlan,
    ) -> Result<(), WorldLibraryError> {
        if plan.copies_process_local_lease {
            return Err(WorldLibraryError::LeaseCopied);
        }
        let record = self
            .library
            .live_by_id(plan.world_id)
            .ok_or(WorldShellError::MissingLiveWorld)?;
        let mut evidence = record
            .lifecycle
            .clone()
            .ok_or(WorldLibraryError::MissingLifecycleEvidence)?;
        evidence.lease = WriterLeaseState::Absent;
        self.library.attach_lifecycle(plan.world_id, evidence)?;
        self.sync_worlds();
        Ok(())
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
    /// Stale exclusive-lease recovery plan; no writer is opened.
    StaleLease(StaleLeaseRecoveryPlan),
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
    /// `ReadyExact` launch requires sealed catalog activation evidence.
    #[error("sealed activation evidence is missing")]
    MissingActivationEvidence,
    /// Storage preflight reported a writer, module, or Bevy side effect.
    #[error("storage preflight reported a forbidden side effect")]
    PreflightSideEffect,
    /// Storage preflight named a different world identity.
    #[error("storage preflight identity does not match the catalog world")]
    StorageIdentityMismatch,
    /// Clone or lease recovery attempted to copy a process-local lease.
    #[error("process-local writer leases must not be copied")]
    LeaseCopied,
    /// Stale-lease recovery planning failed.
    #[error(transparent)]
    StaleLease(#[from] latticeaxiom_world_catalog::StaleLeaseRecoveryError),
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

fn unpublished_clone_record(
    plan: &ClonePlan,
    source: &WorldShellRecord,
    display_name: DisplayName,
    now_ms: u64,
) -> Result<WorldShellRecord, WorldShellError> {
    WorldShellRecord::new(
        CatalogEntry {
            location: plan.target,
            state: CatalogEntryState::Projected(CatalogProjection {
                world_id: plan.new_world_id,
                display_name,
                metadata_epoch: 1,
                clean_shutdown: true,
                durable_frontier: 0,
            }),
        },
        WorldCardMetadata {
            created_at_ms: now_ms,
            last_played_at_ms: 0,
            physical_bytes: source.metadata.physical_bytes,
            game_summary: source.metadata.game_summary.clone(),
            dimension_summary: source.metadata.dimension_summary.clone(),
        },
        None,
    )
}

fn binding_from_permit(permit: &ActivationPermitV1) -> SealedActivationBindingV1 {
    SealedActivationBindingV1 {
        store_id: permit.store_id().clone(),
        metadata_epoch: permit.metadata_epoch().get(),
        metadata_hash: CanonicalHash::from_bytes(*permit.metadata_hash().as_bytes()),
        projection_hash: CanonicalHash::from_bytes(*permit.projection_hash().as_bytes()),
        plan_generation: permit.metadata_epoch().get(),
    }
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
        RecordedCheckpoint, StoreId, WorldLifecycleEvidence, WorldOpenAction, WorldOpenRisk,
        WorldOpenStatus, WriterLeaseState,
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
            activation_binding: Some(SealedActivationBindingV1 {
                store_id: StoreId::new("store-1").unwrap_or_else(|error| panic!("{error}")),
                metadata_epoch: 1,
                metadata_hash: CanonicalHash::digest(b"metadata"),
                projection_hash: CanonicalHash::digest(b"projection"),
                plan_generation: 1,
            }),
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
            lease: WriterLeaseState::Absent,
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

    #[test]
    fn continue_without_activation_binding_fails_closed() {
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
        flow.create(&intent(), world)
            .unwrap_or_else(|error| panic!("{error}"));
        let mut plan = exact_plan(world);
        plan.activation_binding = None;
        flow.attach_preflight(world, plan)
            .unwrap_or_else(|error| panic!("{error}"));
        let error = flow
            .inject(&SemanticCommand {
                target: SemanticNodeId::new("home/continue")
                    .unwrap_or_else(|error| panic!("{error}")),
                action: SemanticActionId::ContinueWorld,
                source: InputSource::Headless,
            })
            .expect_err("ReadyExact without sealed evidence must fail closed");
        assert!(matches!(
            error,
            WorldLibraryError::MissingActivationEvidence
        ));
        assert!(flow.prepared_launch().is_none());
    }

    #[test]
    fn complete_clone_and_trash_never_copy_a_lease() {
        let mut flow = WorldLibraryFlow::new(graph());
        flow.set_now_ms(8);
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

        let clone_target = flow.shell().worlds.records()[0]
            .action_semantic_id(&crate::WorldCardAction::Duplicate)
            .unwrap_or_else(|| panic!("clone control"));
        let WorldLibraryEffect::Clone(plan) = flow
            .inject(&SemanticCommand {
                target: clone_target,
                action: SemanticActionId::CloneWorld,
                source: InputSource::Headless,
            })
            .unwrap_or_else(|error| panic!("{error}"))
        else {
            panic!("expected clone plan");
        };
        flow.complete_clone(
            &plan,
            DisplayName::new("Clone").unwrap_or_else(|error| panic!("{error}")),
        )
        .unwrap_or_else(|error| panic!("{error}"));
        let cloned = flow
            .library()
            .live_by_id(plan.new_world_id)
            .unwrap_or_else(|| panic!("clone row"));
        assert!(cloned.open_plan.is_none());
        assert!(
            cloned
                .lifecycle
                .as_ref()
                .is_none_or(|evidence| { evidence.lease == WriterLeaseState::Absent })
        );

        let trash_target = flow.shell().worlds.records()[0]
            .action_semantic_id(&crate::WorldCardAction::MoveToTrash)
            .unwrap_or_else(|| panic!("trash control"));
        let WorldLibraryEffect::Trash(trash) = flow
            .inject(&SemanticCommand {
                target: trash_target,
                action: SemanticActionId::MoveToTrash,
                source: InputSource::Headless,
            })
            .unwrap_or_else(|error| panic!("{error}"))
        else {
            panic!("expected trash plan");
        };
        flow.complete_trash(
            trash,
            latticeaxiom_world_catalog::TrashTombstone {
                original_root: WORLD_LIBRARY_ROOT,
                world_id: world,
                display_name: DisplayName::new("Library World")
                    .unwrap_or_else(|error| panic!("{error}")),
                deleted_at_ms: 8,
                header_checksum: CanonicalHash::digest(b"header"),
                metadata_checksum: CanonicalHash::digest(b"metadata"),
                physical_bytes: 0,
                last_checkpoint: None,
                retention: latticeaxiom_world_catalog::TrashRetentionPolicy::ManualPurgeOnly,
            },
        )
        .unwrap_or_else(|error| panic!("{error}"));
        assert!(flow.library().live_by_id(world).is_none());
        assert_eq!(flow.library().trash().len(), 1);
        assert_eq!(flow.shell().screen, ShellScreen::Trash);
    }
}
