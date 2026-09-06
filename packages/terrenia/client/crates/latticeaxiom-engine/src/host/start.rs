//! World-library create/list/pause/save/exit/continue for the production host.
//!
//! Worlds published here live in [`crate::MemoryTransactionKernel`] as the
//! session cache. Pause opens the start-ui overlay and does not mutate the
//! materialized-chunk world hash. Durable Save & Quit activates a sealed
//! writer only after exact lock, catalog, world-header, and lease receipts
//! match, publishes player/chunk state, checkpoints, and returns a validated
//! [`ChildResultV1`]. Continue on a storage-backed world reopens
//! [`DeterministicWorldStorage`] first. Recoverable read-only, low-disk, and
//! lease-conflict paths never open a second writer.

use std::{
    collections::{BTreeMap, BTreeSet},
    time::Duration,
};

use bevy::prelude::Resource;
use latticeaxiom_client_ui::{GameModalV1, SurfaceCommandV1};
use latticeaxiom_compose::LockedGameGraph;
use latticeaxiom_core::{
    CanonicalHash, CapabilityId, IdentifierError, PackageName, StableId, WorldId,
    canonical_json_hash,
};
use latticeaxiom_launcher::{
    ChildExitKindV1, ChildExitReportDraftV1, ChildExitReportV1, ChildRoleV1,
    DurableWorldRevisionV1, LaunchAttempt, LaunchGeneration, LaunchIntentDraftV1, LaunchIntentV1,
    LaunchTargetV1, MAX_LAUNCH_INTENT_LIFETIME_MS, ProcessEpoch, SettingTransactionRevision,
    WorldRevision as LauncherWorldRevision,
};
use latticeaxiom_start_ui::{
    ClientShellGraph, ClientShellGraphError, HomePrimaryAction, InMemoryWorldList, InputSource,
    LaunchHandoff, LaunchHandoffContext, LaunchHandoffError, MemoryStartEffect, MemoryStartError,
    MemoryStartFlow, QuickCreateIntent, SemanticActionId, SemanticCommand, SemanticNodeId,
    ShellCapability, ShellEffect, ShellPackageProvider, WorldShellError, WorldShellRecord,
    WorldSort, WorldgenProfileOption, memory_session_store_id, memory_session_template,
};
use latticeaxiom_terrenia_worldgen::TerrainPresetV2;
use latticeaxiom_world_catalog::{
    ReconciliationState, WorldOpenAction, WorldOpenPlan, WorldOpenRisk, WorldOpenStatus,
};
use latticeaxiom_world_db::{
    ActivationPermitV1, AuthoritativeMetadataInputV1, CheckpointId, CheckpointKindV1,
    CheckpointReceiptV1, CheckpointRequestV1, DeterministicWorldStorage, DigestV1,
    FrozenLockReceiptV1, StorageDurabilityCapabilityV1, StoragePreflightStatusV1,
    WorldCreateRequestV1, WorldDbError, WorldRequirementClosureV1, WorldStorage,
};
use latticeaxiom_worldgen::TerrainConfigV2;
use thiserror::Error;

use super::{
    ProductionHostError, ProductionInspectSurface, ProductionSessionPause, ProductionSpine,
    ProductionSurfaceRouter, SealedWorldWriterHost, SealedWriterHostError,
    sealed_activation_binding,
};
use crate::{EngineInstance, LockVerifiedComposeImages, VerifiedProductLockHash};

/// Validated world-child result published after a durable Save & Quit.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ChildResultV1 {
    report: ChildExitReportV1,
    intent: LaunchIntentV1,
    checkpoint: CheckpointReceiptV1,
}

impl ChildResultV1 {
    /// Returns the checksummed child-exit envelope.
    #[must_use]
    pub const fn report(&self) -> &ChildExitReportV1 {
        &self.report
    }

    /// Returns the one-shot shell intent that must accompany the report.
    #[must_use]
    pub const fn intent(&self) -> &LaunchIntentV1 {
        &self.intent
    }

    /// Returns the independently verified Save & Quit checkpoint.
    #[must_use]
    pub const fn checkpoint(&self) -> &CheckpointReceiptV1 {
        &self.checkpoint
    }
}

/// Process-local world list installed on a production host after Continue.
#[derive(Clone, Debug, Resource)]
pub struct ProductionWorldList {
    inner: InMemoryWorldList,
}

impl ProductionWorldList {
    /// Wraps a process-local list for the running host.
    #[must_use]
    pub const fn new(inner: InMemoryWorldList) -> Self {
        Self { inner }
    }

    /// Returns the process-local list.
    #[must_use]
    pub const fn inner(&self) -> &InMemoryWorldList {
        &self.inner
    }

    /// Returns the exact-ready Continue target, when one exists.
    #[must_use]
    pub fn continue_world_id(&self) -> Option<WorldId> {
        match self.inner.list(WorldSort::LastPlayed).home_primary_action() {
            HomePrimaryAction::Continue { world_id, .. } => Some(world_id),
            _ => None,
        }
    }
}

/// Production-client create/list/continue over a metadata-only world list.
///
/// Each created world materializes a [`ProductionSpine`] on first play. The
/// spine and list stay in process memory so Continue observes the same
/// [`WorldId`]. An optional shared [`DeterministicWorldStorage`] fills
/// [`latticeaxiom_world_catalog::WorldOpenPlan::activation_binding`] from
/// storage preflight. [`Self::from_disk_images`] additionally restores the
/// catalog and publishes new worlds physically before showing them in the list.
/// When storage is absent, the explicit memory fixture stays memory-only.
#[derive(Debug)]
pub struct ProductionMemoryStart {
    images: LockVerifiedComposeImages,
    flow: MemoryStartFlow,
    spines: BTreeMap<WorldId, ProductionSpine>,
    terrain_configs: BTreeMap<WorldId, TerrainConfigV2>,
    storage: Option<DeterministicWorldStorage>,
    disk: Option<latticeaxiom_world_db::DiskWorldStore>,
    saved_worlds: BTreeMap<WorldId, latticeaxiom_world_db::DiskWorldEntryV1>,
    shell_lock_hash: Option<CanonicalHash>,
    frozen_lock_catalog: Option<std::path::PathBuf>,
}

impl ProductionMemoryStart {
    /// Builds an empty memory-session start surface for lock-verified images.
    #[must_use]
    pub fn new(images: LockVerifiedComposeImages, graph: ClientShellGraph) -> Self {
        Self {
            images,
            flow: MemoryStartFlow::new(graph),
            spines: BTreeMap::new(),
            terrain_configs: BTreeMap::new(),
            storage: None,
            disk: None,
            saved_worlds: BTreeMap::new(),
            shell_lock_hash: None,
            frozen_lock_catalog: None,
        }
    }

    /// Opens a metadata-only disk library while keeping shell and gameplay locks distinct.
    ///
    /// # Errors
    /// Returns [`ProductionMemoryStartError`] for invalid locks or saved catalog records.
    pub fn from_disk_images(
        shell: &LockVerifiedComposeImages,
        game: LockVerifiedComposeImages,
        disk: latticeaxiom_world_db::DiskWorldStore,
    ) -> Result<Self, ProductionMemoryStartError> {
        let graph = shell_graph_from_lock(shell.images().graph())?;
        let mut start = Self::new(game, graph);
        start.shell_lock_hash = Some(shell.product_lock_hash());
        start.install_worldgen_profiles()?;
        for entry in disk.entries()? {
            let record =
                super::persistent::catalog_record(&entry, start.images.product_lock_hash())?;
            start.flow.restore_record(record)?;
            start.saved_worlds.insert(entry.world, entry);
        }
        start.disk = Some(disk);
        Ok(start)
    }

    /// Makes archived gameplay locks available to existing worlds without loading voxels.
    ///
    /// # Errors
    /// Returns a catalog projection error if an existing row cannot be refreshed.
    pub fn with_frozen_lock_catalog(
        mut self,
        catalog: std::path::PathBuf,
    ) -> Result<Self, ProductionMemoryStartError> {
        for entry in self.saved_worlds.values() {
            let path = latticeaxiom_compose::archived_product_lock_path(&catalog, entry.game_lock);
            if latticeaxiom_compose::reopen_product_lock(path)
                .is_ok_and(|lock| lock.product_lock_hash == entry.game_lock)
            {
                self.flow
                    .replace_record(super::persistent::catalog_record(entry, entry.game_lock)?)?;
            }
        }
        self.frozen_lock_catalog = Some(catalog);
        Ok(self)
    }

    fn world_game_lock(&self, world: WorldId) -> CanonicalHash {
        self.saved_worlds
            .get(&world)
            .map_or(self.images.product_lock_hash(), |entry| entry.game_lock)
    }

    fn images_for_world(
        &self,
        world: WorldId,
    ) -> Result<LockVerifiedComposeImages, ProductionMemoryStartError> {
        let hash = self.world_game_lock(world);
        if hash == self.images.product_lock_hash() {
            return Ok(self.images.clone());
        }
        let catalog = self
            .frozen_lock_catalog
            .as_ref()
            .ok_or(ProductionMemoryStartError::SavedWorldLockMismatch { world })?;
        latticeaxiom_host::load_archived_product_images(catalog, hash, self.images.target())
            .map_err(|error| ProductionMemoryStartError::FrozenGameLock {
                reason: error.to_string(),
            })
    }

    /// Shares a deterministic world store used to bind create plans from preflight.
    ///
    /// The same store is attached to the start-ui memory session. When absent,
    /// create stays memory-only and activation bindings remain unset.
    pub fn set_storage(&mut self, storage: DeterministicWorldStorage) {
        self.flow.set_storage(storage.clone());
        self.storage = Some(storage);
    }

    /// Shares a deterministic world store and returns the start surface.
    #[must_use]
    pub fn with_storage(mut self, storage: DeterministicWorldStorage) -> Self {
        self.set_storage(storage);
        self
    }

    /// Returns the shared world store, when one is attached.
    #[must_use]
    pub const fn storage(&self) -> Option<&DeterministicWorldStorage> {
        self.storage.as_ref()
    }

    /// Resolves the shell graph from lock-selected capability providers.
    ///
    /// # Errors
    ///
    /// Returns [`ProductionMemoryStartError`] when a required shell capability
    /// is missing or the provider closure is not exactly-one.
    pub fn from_lock_images(
        images: LockVerifiedComposeImages,
    ) -> Result<Self, ProductionMemoryStartError> {
        let graph = shell_graph_from_lock(images.images().graph())?;
        let mut start = Self::new(images, graph);
        start.install_worldgen_profiles()?;
        Ok(start)
    }

    /// Selects the client process role from reopened lock capability evidence.
    ///
    /// An absent `client-shell@1` entry selects the game process. A present
    /// entry selects the shell only when it contains exactly one provider; the
    /// provider package name and graph roots do not affect the decision.
    ///
    /// # Errors
    ///
    /// Returns [`ProductionMemoryStartError::InvalidClientShellProviderEvidence`]
    /// when a present provider vector is empty or contains multiple rows. Also
    /// returns an identity error if the platform capability constant is invalid.
    pub fn lock_graph_selects_shell(
        graph: &LockedGameGraph,
    ) -> Result<bool, ProductionMemoryStartError> {
        let capability = CLIENT_SHELL_CAPABILITY.parse::<CapabilityId>()?;
        let Some(providers) = graph.capability_providers.get(&capability) else {
            return Ok(false);
        };
        match providers.as_slice() {
            [_] => Ok(true),
            _ => Err(
                ProductionMemoryStartError::InvalidClientShellProviderEvidence {
                    providers: providers.clone(),
                },
            ),
        }
    }

    /// Seals a replacement-process handoff for an exact-ready session world.
    ///
    /// This does not create a Bevy game [`bevy::app::App`] and does not spawn
    /// [`super::ProductionSpine`]. An external supervisor must persist the
    /// intent and spawn the game process. The caller supplies the exact durable
    /// settings revision confirmed before the handoff.
    ///
    /// # Errors
    ///
    /// Returns [`ProductionMemoryStartError`] when the world is absent or is
    /// not exact-ready, or when launcher intent validation fails.
    pub fn launch_handoff_for_ready_exact(
        &mut self,
        world_id: WorldId,
        now_ms: u64,
        confirmed_setting_transaction_revision: SettingTransactionRevision,
    ) -> Result<LaunchHandoff, ProductionMemoryStartError> {
        if let Some(disk) = &self.disk {
            let entry = self
                .saved_worlds
                .get(&world_id)
                .ok_or(WorldShellError::MissingLiveWorld)?;
            // Verify the exact original lock/CAS only when the user selects the world.
            let _images = self.images_for_world(entry.world)?;
            let storage = super::persistent::load_disk_world(disk, world_id)?;
            let preflight = storage.preflight(world_id)?;
            let permit = preflight.activation_permit().ok_or(
                ProductionMemoryStartError::StorageActivationUnavailable { world: world_id },
            )?;
            self.flow
                .attach_open_plan(world_id, writable_open_plan(world_id, permit))?;
        }
        let record = self
            .flow
            .worlds()
            .get(world_id)
            .ok_or(WorldShellError::MissingLiveWorld)?;
        sealed_ready_exact_handoff(
            record,
            self.shell_lock_hash
                .unwrap_or(self.images.product_lock_hash()),
            self.world_game_lock(world_id),
            now_ms,
            confirmed_setting_transaction_revision,
        )
        .map_err(ProductionMemoryStartError::from)
    }

    /// Returns the presentation-neutral start flow.
    #[must_use]
    pub const fn flow(&self) -> &MemoryStartFlow {
        &self.flow
    }

    /// Stores the typed intent applied by the next quick-create command.
    pub fn set_draft(&mut self, intent: QuickCreateIntent) {
        self.flow.set_draft(intent);
    }

    /// Clock used when semantic quick-create publishes a session world.
    pub fn set_now_ms(&mut self, now_ms: u64) {
        self.flow.set_now_ms(now_ms);
    }

    /// Builds a quick-create intent from the locked graph root and product lock.
    ///
    /// Template identity is the process-local memory session template. The root
    /// package is the first locked graph root; this host does not embed a game
    /// catalog default.
    ///
    /// # Errors
    ///
    /// Returns [`ProductionMemoryStartError::NoGraphRoot`] when the lock graph
    /// has no roots, or a display-name validation error.
    pub fn quick_create_intent(
        &self,
        display_name: &str,
    ) -> Result<QuickCreateIntent, ProductionMemoryStartError> {
        let root = self
            .images
            .images()
            .graph()
            .roots
            .iter()
            .find(|name| {
                self.images
                    .images()
                    .graph()
                    .packages
                    .get(*name)
                    .is_some_and(|package| {
                        package
                            .domains
                            .contains(&latticeaxiom_compose::PackageDomain::Authoritative)
                    })
            })
            .cloned()
            .ok_or(ProductionMemoryStartError::NoGraphRoot)?;
        let mut intent = QuickCreateIntent::new(
            display_name,
            memory_session_template(),
            root,
            self.images.product_lock_hash(),
        )
        .map_err(MemoryStartError::from)?;
        if let Some(profile) = self.flow.shell().selected_worldgen_profile() {
            intent.set_generation_profile(profile.clone());
        }
        Ok(intent)
    }

    /// Builds a quick-create intent with one explicit built-in terrain preset.
    ///
    /// # Errors
    ///
    /// Returns the same errors as [`Self::quick_create_intent`] or an identity
    /// error if a built-in profile literal violates the stable-ID grammar.
    pub fn quick_create_intent_with_terrain(
        &self,
        display_name: &str,
        preset: TerrainPresetV2,
    ) -> Result<QuickCreateIntent, ProductionMemoryStartError> {
        Ok(self
            .quick_create_intent(display_name)?
            .with_generation_profile(preset.profile_id_str().parse::<StableId>()?))
    }

    /// Creates a world and publishes its identity after storage succeeds.
    ///
    /// When shared storage is attached, the world is provisioned and the open
    /// plan's activation binding is filled from storage preflight. The
    /// disk-backed constructor also commits the image and catalog atomically
    /// before publishing the [`WorldId`] to the process-local list. When storage
    /// is [`None`], create stays memory-only.
    ///
    /// # Errors
    ///
    /// Returns [`ProductionMemoryStartError`] when the session list rejects the
    /// new identity, provisioning fails, or attached storage preflight is not
    /// ready for activation.
    pub fn create(
        &mut self,
        intent: &QuickCreateIntent,
        now_ms: u64,
    ) -> Result<WorldId, ProductionMemoryStartError> {
        if self.disk.is_some() {
            self.set_storage(super::persistent::new_working_store()?);
        }
        let preset = terrain_preset_for_intent(intent)?;
        let terrain = preset.resolve();
        #[cfg(feature = "development")]
        let world_id = self
            .saved_worlds
            .is_empty()
            .then(crate::lifecycle_qa::requested_world_id)
            .flatten()
            .unwrap_or_else(WorldId::new_v4);
        #[cfg(not(feature = "development"))]
        let world_id = WorldId::new_v4();
        self.provision_created_world(world_id, intent, preset, &terrain)?;
        if let (Some(disk), Some(storage)) = (&self.disk, &self.storage) {
            let entry = latticeaxiom_world_db::DiskWorldEntryV1 {
                world: world_id,
                display_name: intent.display_name.clone(),
                created_at_ms: now_ms,
                last_played_at_ms: now_ms,
                game_lock: self.images.product_lock_hash(),
                generation_profile: intent.generation_profile.clone(),
                metadata_epoch: 1,
                durable_revision: 0,
            };
            disk.publish(storage, &entry)?;
            self.saved_worlds.insert(world_id, entry);
        }
        let world_id = self.flow.create(intent, world_id, now_ms)?;
        if let Some(entry) = self.saved_worlds.get(&world_id) {
            self.flow.replace_record(super::persistent::catalog_record(
                entry,
                self.images.product_lock_hash(),
            )?)?;
        }
        self.terrain_configs.insert(world_id, terrain);
        Ok(world_id)
    }

    /// Returns the exact-ready Continue target, when one exists.
    #[must_use]
    pub fn continue_world_id(&self) -> Option<WorldId> {
        self.flow.continue_world_id()
    }

    /// Applies a start-ui semantic command to the in-memory session list.
    ///
    /// Quick-create provisions shared storage before the session list publishes
    /// the same [`WorldId`].
    ///
    /// # Errors
    ///
    /// Returns [`ProductionMemoryStartError`] when the current tree rejects the
    /// command or quick-create cannot publish.
    pub fn inject(
        &mut self,
        command: &SemanticCommand,
    ) -> Result<MemoryStartEffect, ProductionMemoryStartError> {
        match self.flow.apply_shell_command(command)? {
            ShellEffect::RequestQuickCreate => {
                let intent = self
                    .flow
                    .draft()
                    .cloned()
                    .ok_or(MemoryStartError::MissingQuickCreateDraft)?;
                let world_id = self.create(&intent, self.flow.now_ms())?;
                Ok(MemoryStartEffect::Created(world_id))
            }
            effect => Ok(MemoryStartEffect::Shell(effect)),
        }
    }

    /// Materializes the in-memory session into a GPU-free production host.
    ///
    /// A previously played world reuses the same spine so Continue stays on the
    /// same [`WorldId`] and memory kernel. Storage-backed worlds still generate
    /// into that cache on first play; [`Self::play_reopened_headless`] reads
    /// world-db first after a sealed writer flush.
    ///
    /// # Errors
    ///
    /// Returns [`ProductionMemoryStartError`] when the world is absent, spine
    /// materialization fails, or Bevy construction fails.
    pub fn play_headless(
        &mut self,
        world_id: WorldId,
        played_at_ms: u64,
        fixed_timestep: Duration,
    ) -> Result<EngineInstance, ProductionMemoryStartError> {
        if self.flow.worlds().get(world_id).is_none() {
            return Err(WorldShellError::MissingLiveWorld.into());
        }
        let spine = self.ensure_spine(world_id)?;
        self.flow.mark_played(world_id, played_at_ms)?;
        let worlds = ProductionWorldList::new(self.flow.worlds().clone());
        let mut instance = EngineInstance::new_headless_host_from_lock_with_spine(
            self.images_for_world(world_id)?,
            fixed_timestep,
            spine,
        )?;
        instance.app.world_mut().insert_resource(worlds);
        Ok(instance)
    }

    /// Continues the exact-ready recent world into a GPU-free production host.
    ///
    /// Storage-backed worlds reopen [`DeterministicWorldStorage`] first so
    /// Continue cannot reuse a previous memory kernel. Memory-only sessions
    /// still reuse the process-local spine.
    ///
    /// # Errors
    ///
    /// Returns [`ProductionMemoryStartError::NoContinueWorld`] when Continue is
    /// not available, and otherwise the same failures as [`Self::play_headless`]
    /// or [`Self::play_reopened_headless`].
    pub fn play_continued_headless(
        &mut self,
        played_at_ms: u64,
        fixed_timestep: Duration,
    ) -> Result<(WorldId, EngineInstance), ProductionMemoryStartError> {
        let world_id = self
            .continue_world_id()
            .ok_or(ProductionMemoryStartError::NoContinueWorld)?;
        let instance = if self.storage.is_some() {
            self.play_reopened_headless(world_id, played_at_ms, fixed_timestep)?
        } else {
            self.play_headless(world_id, played_at_ms, fixed_timestep)?
        };
        Ok((world_id, instance))
    }

    /// Opens the in-session pause overlay without mutating world state.
    ///
    /// Chunk streaming is skipped while paused. The materialized-chunk hash is
    /// unchanged. This is not a checkpoint.
    ///
    /// # Errors
    ///
    /// Returns [`ProductionMemoryStartError`] when the pause command is
    /// rejected by the start-ui tree.
    pub fn pause_session(
        &mut self,
        instance: &mut EngineInstance,
    ) -> Result<MemoryStartEffect, ProductionMemoryStartError> {
        self.flow.enter_playing();
        let effect = self.inject(&session_command(
            "playing/pause",
            SemanticActionId::PauseWorld,
        ))?;
        apply_game_surface(instance, &SurfaceCommandV1::Pause)?;
        Ok(effect)
    }

    /// Resumes a paused world session without writing.
    ///
    /// # Errors
    ///
    /// Returns [`ProductionMemoryStartError`] when the pause overlay is not open.
    pub fn resume_session(
        &mut self,
        instance: &mut EngineInstance,
    ) -> Result<MemoryStartEffect, ProductionMemoryStartError> {
        let effect = self.inject(&session_command(
            "pause/resume",
            SemanticActionId::ResumeWorld,
        ))?;
        apply_game_surface(instance, &SurfaceCommandV1::Back)?;
        Ok(effect)
    }

    /// Save & Quit: flush dirty state, drop the host, and return Home.
    ///
    /// Durable oracles additionally checkpoint and return a validated
    /// [`ChildResultV1`] through [`Self::save_and_quit_durable`]. Volatile
    /// references keep the Written close used by V2 tests. The caller supplies
    /// the exact durable settings revision confirmed before exit.
    ///
    /// # Errors
    ///
    /// Returns [`ProductionMemoryStartError`] when pause/save/exit is rejected.
    pub fn save_and_quit(
        &mut self,
        world_id: WorldId,
        mut instance: EngineInstance,
        writer: &mut SealedWorldWriterHost,
        confirmed_setting_transaction_revision: SettingTransactionRevision,
    ) -> Result<MemoryStartEffect, ProductionMemoryStartError> {
        let _ = apply_game_surface(&mut instance, &SurfaceCommandV1::RequestSaveQuit);
        let _ = apply_game_surface(&mut instance, &SurfaceCommandV1::Confirm);
        if writer.durability_capability() == StorageDurabilityCapabilityV1::WalSyncCheckpoint {
            let _ = self.save_and_quit_durable(
                world_id,
                instance,
                writer,
                confirmed_setting_transaction_revision,
            )?;
            return Ok(MemoryStartEffect::Shell(ShellEffect::RequestExitWorld));
        }
        self.save_world(world_id, writer)?;
        self.exit_world(world_id, instance)
    }

    /// Durable Save & Quit: sealed writer, checkpoint, and child result.
    ///
    /// Exact lock/catalog/world-header/lease receipts are revalidated before
    /// the writer opens. Player pose, inventory, selected slot, tool
    /// durability, containers, scheduled work, and edited chunks are
    /// published at [`latticeaxiom_world_db::CommitDurabilityV1::Durable`]. A protected checkpoint
    /// is created at that frontier. The writer is closed before the child
    /// result is sealed with the caller's confirmed durable settings revision.
    ///
    /// # Errors
    ///
    /// Returns [`ProductionMemoryStartError`] when the world is absent,
    /// activation evidence is missing, durability is unsupported, or the
    /// child-result envelope is invalid.
    pub fn save_and_quit_durable(
        &mut self,
        world_id: WorldId,
        instance: EngineInstance,
        writer: &mut SealedWorldWriterHost,
        confirmed_setting_transaction_revision: SettingTransactionRevision,
    ) -> Result<ChildResultV1, ProductionMemoryStartError> {
        if writer.durability_capability() != StorageDurabilityCapabilityV1::WalSyncCheckpoint {
            return Err(ProductionMemoryStartError::DurableCapabilityRequired);
        }
        self.inject(&session_command("pause/save", SemanticActionId::SaveWorld))?;
        let outcome = self.flush_dirty_chunks(world_id, writer)?;
        if outcome.is_none() {
            writer.flush_durable()?;
        }
        let checkpoint = writer.create_checkpoint(CheckpointRequestV1::new(
            CheckpointId::from_u128(u128::from(self.flow.now_ms()).saturating_add(1)),
            CheckpointKindV1::Protected,
            "save-and-quit",
        ))?;
        let frontier = writer.begin_read(world_id)?.frontier();
        if frontier.durable() != frontier.current() || frontier.checkpointed() != frontier.durable()
        {
            return Err(ProductionMemoryStartError::DurableFrontierIncomplete { world: world_id });
        }
        writer.close()?;
        self.flow
            .record_durable_frontier(world_id, frontier.durable().get(), true)?;
        let result = self.seal_child_result(
            world_id,
            frontier.durable().get(),
            checkpoint.receipt().clone(),
            confirmed_setting_transaction_revision,
        )?;
        self.exit_world(world_id, instance)?;
        Ok(result)
    }

    /// Flushes dirty chunks through the sealed writer and then closes it.
    ///
    /// Missing sealed receipts still fail closed as
    /// [`WorldDbError::ActivationEvidenceUnavailable`]. This is not a physical
    /// checkpoint and does not call [`SealedWorldWriterHost::flush_durable`].
    ///
    /// # Errors
    ///
    /// Returns [`ProductionMemoryStartError`] when the pause overlay is not
    /// open, the world is absent, preflight/activation fails, or the commit is
    /// rejected.
    pub fn save_world(
        &mut self,
        world_id: WorldId,
        writer: &mut SealedWorldWriterHost,
    ) -> Result<MemoryStartEffect, ProductionMemoryStartError> {
        let effect = self.inject(&session_command("pause/save", SemanticActionId::SaveWorld))?;
        self.flush_dirty_chunks(world_id, writer)?;
        if writer.is_writer_active() {
            writer.close()?;
        }
        Ok(effect)
    }

    /// Drops the live host and returns to the start shell.
    ///
    /// Storage-backed worlds drop the cached spine so the next Continue reopens
    /// storage-first. Memory-only sessions keep the process-local spine.
    ///
    /// # Errors
    ///
    /// Returns [`ProductionMemoryStartError`] when the pause overlay is not
    /// open or the world is absent from the session list.
    pub fn exit_world(
        &mut self,
        world_id: WorldId,
        instance: EngineInstance,
    ) -> Result<MemoryStartEffect, ProductionMemoryStartError> {
        if self.flow.worlds().get(world_id).is_none() {
            return Err(WorldShellError::MissingLiveWorld.into());
        }
        let effect = self.inject(&session_command("pause/exit", SemanticActionId::ExitWorld))?;
        drop(instance);
        if self.storage.is_some() {
            self.spines.remove(&world_id);
        }
        Ok(effect)
    }

    /// Publishes edited working-set chunks through an activated sealed writer.
    ///
    /// The writer is activated from a fresh storage preflight and
    /// [`WorldOpenAction::UseFrozenLock`] is accepted only when that preflight
    /// yields a sealed [`crate::sealed_activation_binding`]. Missing catalog
    /// evidence still fails closed as
    /// [`WorldDbError::ActivationEvidenceUnavailable`]. The writer is closed
    /// after the Written commit. [`SealedWorldWriterHost::flush_durable`] is
    /// not called.
    ///
    /// # Errors
    ///
    /// Returns [`ProductionMemoryStartError`] when the world is absent from the
    /// session list or spine cache, preflight/activation fails, or the commit
    /// is rejected.
    pub fn flush_dirty_chunks(
        &mut self,
        world_id: WorldId,
        writer: &mut SealedWorldWriterHost,
    ) -> Result<Option<latticeaxiom_world_db::WorldCommitOutcomeV1>, ProductionMemoryStartError>
    {
        if self.flow.worlds().get(world_id).is_none() {
            return Err(WorldShellError::MissingLiveWorld.into());
        }
        let spine = self
            .spines
            .get(&world_id)
            .cloned()
            .ok_or(WorldShellError::MissingLiveWorld)?;
        let preflight = writer.preflight(world_id)?;
        let permit = preflight
            .activation_permit()
            .cloned()
            .ok_or(ProductionMemoryStartError::StorageActivationUnavailable { world: world_id })?;
        let plan = writable_open_plan(world_id, &permit);
        writer.reactivate(&plan, permit)?;
        let outcome = spine.flush_dirty_chunks(writer, preflight.metadata())?;
        if writer.durability_capability() != StorageDurabilityCapabilityV1::WalSyncCheckpoint {
            writer.close()?;
        }
        Ok(outcome)
    }

    /// Recovers a crash-abandoned durable world without opening a writer first.
    ///
    /// Canonical reopen discards written-but-not-durable mutations, header
    /// repair is a read-only recovery action, and
    /// [`SealedWorldWriterHost::verify_crash_recovery`] must succeed before a
    /// later sealed activation. Recoverable read-only status never yields a
    /// writer permit.
    ///
    /// # Errors
    ///
    /// Returns [`ProductionMemoryStartError`] when shared storage is absent,
    /// reopen fails, or recovery verification cannot complete.
    pub fn recover_after_crash(
        &mut self,
        world_id: WorldId,
        writer: &mut SealedWorldWriterHost,
    ) -> Result<(), ProductionMemoryStartError> {
        writer.canonical_reopen()?;
        self.set_storage(writer.storage().clone());
        let preflight = writer.preflight(world_id)?;
        if let Some(repair) = preflight.header_repair_permit().cloned() {
            writer.repair_header(repair)?;
        }
        writer.verify_crash_recovery(world_id)?;
        let ready = writer.preflight(world_id)?;
        if matches!(
            ready.status(),
            StoragePreflightStatusV1::RecoverableReadOnly { .. }
        ) {
            return Err(ProductionMemoryStartError::RecoverableReadOnly { world: world_id });
        }
        if ready.activation_permit().is_none() {
            return Err(ProductionMemoryStartError::StorageActivationUnavailable {
                world: world_id,
            });
        }
        Ok(())
    }

    /// Recovers the latest durable world after a shutdown timeout.
    ///
    /// Canonical reopen discards written-but-not-durable mutations. The sealed
    /// child report uses [`ChildExitKindV1::ShutdownTimeout`] and cannot
    /// masquerade as Save & Quit. No writer is left active. The caller supplies
    /// the last durable settings revision confirmed before recovery.
    ///
    /// # Errors
    ///
    /// Returns [`ProductionMemoryStartError`] when the world is absent, the
    /// store is not durable, reopen fails, or the timeout envelope is invalid.
    pub fn on_shutdown_timeout(
        &mut self,
        world_id: WorldId,
        writer: &mut SealedWorldWriterHost,
        confirmed_setting_transaction_revision: SettingTransactionRevision,
    ) -> Result<ChildExitReportV1, ProductionMemoryStartError> {
        if writer.durability_capability() != StorageDurabilityCapabilityV1::WalSyncCheckpoint {
            return Err(ProductionMemoryStartError::DurableCapabilityRequired);
        }
        writer.canonical_reopen()?;
        self.set_storage(writer.storage().clone());
        self.spines.remove(&world_id);
        if writer.is_writer_active() {
            writer.close()?;
        }
        writer.verify_crash_recovery(world_id)?;
        let ready = writer.preflight(world_id)?;
        if matches!(
            ready.status(),
            StoragePreflightStatusV1::RecoverableReadOnly { .. }
        ) {
            return Err(ProductionMemoryStartError::RecoverableReadOnly { world: world_id });
        }
        let plan_hash = {
            let record = self
                .flow
                .worlds()
                .get(world_id)
                .ok_or(WorldShellError::MissingLiveWorld)?;
            let plan = record.open_plan.as_ref().ok_or(
                ProductionMemoryStartError::StorageActivationUnavailable { world: world_id },
            )?;
            canonical_json_hash(plan)?
        };
        let frontier = writer.begin_read(world_id)?.frontier();
        let durable = DurableWorldRevisionV1::new(
            world_id,
            LauncherWorldRevision::new(frontier.durable().get()),
        );
        let report = ChildExitReportV1::seal(ChildExitReportDraftV1 {
            child_generation: current_launch_generation(),
            process_epoch: current_process_epoch(),
            role: ChildRoleV1::World { world_id },
            exit_kind: ChildExitKindV1::ShutdownTimeout,
            intent_generation: None,
            intent_checksum: None,
            confirmed_setting_transaction_revision,
            last_written_world: Some(durable),
            last_durable_world: Some(durable),
            shell_lock_hash: self
                .shell_lock_hash
                .unwrap_or(self.images.product_lock_hash()),
            world_lock_hash: Some(self.world_game_lock(world_id)),
            world_open_plan_hash: Some(plan_hash),
            diagnostic_ref: None,
        })?;
        self.flow
            .record_durable_frontier(world_id, frontier.durable().get(), false)?;
        self.flow.enter_home();
        Ok(report)
    }

    fn seal_child_result(
        &self,
        world_id: WorldId,
        durable_revision: u64,
        checkpoint: CheckpointReceiptV1,
        confirmed_setting_transaction_revision: SettingTransactionRevision,
    ) -> Result<ChildResultV1, ProductionMemoryStartError> {
        let record = self
            .flow
            .worlds()
            .get(world_id)
            .ok_or(WorldShellError::MissingLiveWorld)?;
        let plan = record
            .open_plan
            .as_ref()
            .ok_or(ProductionMemoryStartError::StorageActivationUnavailable { world: world_id })?;
        let plan_hash = canonical_json_hash(plan)?;
        let now_ms = self.flow.now_ms();
        let child_generation = current_launch_generation();
        let intent = LaunchIntentV1::seal(LaunchIntentDraftV1 {
            generation: child_generation.next()?,
            attempt: LaunchAttempt::FIRST,
            issued_at_ms: now_ms,
            expires_at_ms: now_ms.saturating_add(MAX_LAUNCH_INTENT_LIFETIME_MS),
            target: LaunchTargetV1::Shell,
            shell_lock_hash: self
                .shell_lock_hash
                .unwrap_or(self.images.product_lock_hash()),
            world_lock_hash: None,
            world_open_plan_hash: None,
            confirmed_setting_transaction_revision,
        })?;
        let durable =
            DurableWorldRevisionV1::new(world_id, LauncherWorldRevision::new(durable_revision));
        let report = ChildExitReportV1::seal(ChildExitReportDraftV1 {
            child_generation,
            process_epoch: current_process_epoch(),
            role: ChildRoleV1::World { world_id },
            exit_kind: ChildExitKindV1::SaveAndQuit,
            intent_generation: Some(intent.generation()),
            intent_checksum: Some(intent.checksum()),
            confirmed_setting_transaction_revision,
            last_written_world: Some(durable),
            last_durable_world: Some(durable),
            shell_lock_hash: self
                .shell_lock_hash
                .unwrap_or(self.images.product_lock_hash()),
            world_lock_hash: Some(self.world_game_lock(world_id)),
            world_open_plan_hash: Some(plan_hash),
            diagnostic_ref: None,
        })?;
        Ok(ChildResultV1 {
            report,
            intent,
            checkpoint,
        })
    }

    /// Constructs a new GPU-free host from shared world-db for `world_id`.
    ///
    /// The cached spine is dropped so the new host cannot reuse the previous
    /// memory kernel. Materialization reads [`DeterministicWorldStorage`] first
    /// and uses [`crate::MemoryTransactionKernel`] only as the working-set cache.
    ///
    /// # Errors
    ///
    /// Returns [`ProductionMemoryStartError::StorageRequired`] when no shared
    /// store is attached, [`WorldShellError::MissingLiveWorld`] when the
    /// identity is absent, or the same failures as [`Self::play_headless`].
    pub fn play_reopened_headless(
        &mut self,
        world_id: WorldId,
        played_at_ms: u64,
        fixed_timestep: Duration,
    ) -> Result<EngineInstance, ProductionMemoryStartError> {
        if self.storage.is_none() && self.disk.is_none() {
            return Err(ProductionMemoryStartError::StorageRequired);
        }
        self.spines.remove(&world_id);
        self.play_headless(world_id, played_at_ms, fixed_timestep)
    }

    fn ensure_spine(
        &mut self,
        world_id: WorldId,
    ) -> Result<ProductionSpine, ProductionMemoryStartError> {
        if let Some(spine) = self.spines.get(&world_id) {
            return Ok(spine.clone());
        }
        if let Some(disk) = &self.disk {
            if !self.saved_worlds.contains_key(&world_id) {
                return Err(WorldShellError::MissingLiveWorld.into());
            }
            self.set_storage(super::persistent::load_disk_world(disk, world_id)?);
        }
        let images = self.images_for_world(world_id)?;
        let spine = match &self.storage {
            Some(storage) => {
                ProductionSpine::materialize_world_from_storage(&images, world_id, storage.clone())?
            }
            None => ProductionSpine::materialize_world_with_terrain(
                &images,
                world_id,
                self.terrain_configs
                    .get(&world_id)
                    .copied()
                    .unwrap_or_else(|| TerrainPresetV2::Balanced.resolve()),
            )?,
        };
        self.spines.insert(world_id, spine.clone());
        Ok(spine)
    }

    fn provision_created_world(
        &self,
        world_id: WorldId,
        intent: &QuickCreateIntent,
        preset: TerrainPresetV2,
        terrain: &TerrainConfigV2,
    ) -> Result<(), ProductionMemoryStartError> {
        let Some(storage) = &self.storage else {
            return Ok(());
        };
        let metadata = memory_session_authoritative_metadata(&self.images, preset, terrain)?;
        storage.provision_world(WorldCreateRequestV1::new(
            world_id,
            intent.display_name.clone(),
            memory_session_store_id(),
            metadata,
        ))?;
        Ok(())
    }

    fn install_worldgen_profiles(&mut self) -> Result<(), ProductionMemoryStartError> {
        let selected = TerrainPresetV2::Balanced
            .profile_id_str()
            .parse::<StableId>()?;
        self.flow
            .set_worldgen_profiles(worldgen_profile_options()?, &selected)
            .map_err(MemoryStartError::from)?;
        Ok(())
    }
}

impl EngineInstance {
    /// Builds a GPU-free production host around an already materialized spine.
    ///
    /// The host inserts [`crate::MemoryTransactionKernel`] as the production
    /// storage implementation and does not open a world writer.
    ///
    /// # Errors
    ///
    /// Returns [`ProductionHostError`] when Bevy construction fails or the
    /// fixed timestep is zero.
    pub fn new_headless_host_from_lock_with_spine(
        images: LockVerifiedComposeImages,
        fixed_timestep: Duration,
        spine: ProductionSpine,
    ) -> Result<Self, ProductionHostError> {
        let product_lock_hash = VerifiedProductLockHash::new(images.product_lock_hash());
        let inspect_surface = ProductionInspectSurface::from_lock_images(&images);
        Self::new_headless_with_setup(images.into_images(), fixed_timestep, move |app| {
            super::install_production_host(
                app,
                product_lock_hash,
                spine,
                inspect_surface,
                true,
                #[cfg(feature = "client")]
                None,
            );
        })
        .map_err(ProductionHostError::from)
    }
}

/// Failure to drive the in-memory production start surface.
#[derive(Debug, Error)]
pub enum ProductionMemoryStartError {
    /// A world's archived gameplay closure could not be verified.
    #[error("saved gameplay lock failed: {reason}")]
    FrozenGameLock {
        /// The underlying verification failure.
        reason: String,
    },
    /// Physical world publication or catalog read failed.
    #[error(transparent)]
    Disk(#[from] latticeaxiom_world_db::DiskWorldError),
    /// The current gameplay closure is not the world's frozen closure.
    #[error("saved world {world} requires its original gameplay lock")]
    SavedWorldLockMismatch {
        /// Saved world requiring an exact compatible build.
        world: WorldId,
    },
    /// Start-ui semantic flow or session list failed.
    #[error(transparent)]
    Start(#[from] MemoryStartError),
    /// Production spine or Bevy host construction failed.
    #[error(transparent)]
    Host(#[from] ProductionHostError),
    /// Lock graph did not resolve a valid shell-package closure.
    #[error(transparent)]
    ShellGraph(#[from] ClientShellGraphError),
    /// A shell capability identity could not be parsed.
    #[error(transparent)]
    Identity(#[from] IdentifierError),
    /// The lock graph did not select a required shell capability.
    #[error("shell capability `{capability:?}` is not provided by the lock graph")]
    MissingShellCapability {
        /// Missing shell capability.
        capability: ShellCapability,
    },
    /// Present client-shell capability evidence did not identify exactly one provider.
    #[error(
        "client-shell capability provider evidence must contain exactly one package when present; found {providers:?}"
    )]
    InvalidClientShellProviderEvidence {
        /// Ordered provider rows retained from the locked graph as failure evidence.
        providers: Vec<PackageName>,
    },
    /// The lock graph has no root package to bind to a create intent.
    #[error("lock graph has no root package")]
    NoGraphRoot,
    /// Create intent named a generation profile not offered by this host.
    #[error("world-generation profile `{profile}` is not available")]
    UnknownWorldgenProfile {
        /// Unrecognized profile identity.
        profile: StableId,
    },
    /// Continue was requested without an exact-ready in-memory world.
    #[error("no exact-ready in-memory world is available for Continue")]
    NoContinueWorld,
    /// Shared world storage rejected provisioning or metadata construction.
    #[error(transparent)]
    WorldDb(#[from] WorldDbError),
    /// Sealed writer activation, commit, or close failed.
    #[error(transparent)]
    Writer(#[from] SealedWriterHostError),
    /// Reopen was requested without a shared [`DeterministicWorldStorage`].
    #[error("shared world storage is required to reopen from world-db")]
    StorageRequired,
    /// Attached storage preflight did not yield ready activation evidence.
    #[error("world storage preflight did not yield activation evidence for {world}")]
    StorageActivationUnavailable {
        /// World whose preflight lacked a ready permit.
        world: WorldId,
    },
    /// `ReadyExact` replacement-process handoff could not be sealed.
    #[error(transparent)]
    LaunchHandoff(#[from] LaunchHandoffError),
    /// Durable Save & Quit was requested on a volatile reference store.
    #[error("durable Save & Quit requires a WAL/sync/checkpoint storage capability")]
    DurableCapabilityRequired,
    /// The game-process surface router rejected a typed command.
    #[error(transparent)]
    Surface(#[from] latticeaxiom_client_ui::SurfaceRouterError),
    /// Crash recovery proved the world readable but not writable.
    #[error("world {world} is recoverable read-only and must not open a writer")]
    RecoverableReadOnly {
        /// World that remains read-only.
        world: WorldId,
    },
    /// Save & Quit closed before the durable and checkpointed frontiers matched.
    #[error("world {world} durable Save & Quit frontier is incomplete")]
    DurableFrontierIncomplete {
        /// World whose frontier lagged the checkpoint.
        world: WorldId,
    },
}

impl From<latticeaxiom_core::CanonicalJsonError> for ProductionMemoryStartError {
    fn from(error: latticeaxiom_core::CanonicalJsonError) -> Self {
        Self::LaunchHandoff(error.into())
    }
}

impl From<latticeaxiom_launcher::LaunchModelError> for ProductionMemoryStartError {
    fn from(error: latticeaxiom_launcher::LaunchModelError) -> Self {
        Self::LaunchHandoff(error.into())
    }
}

impl From<WorldShellError> for ProductionMemoryStartError {
    fn from(error: WorldShellError) -> Self {
        Self::Start(error.into())
    }
}

/// Capability whose exactly-one provider selects the package-driven start shell.
const CLIENT_SHELL_CAPABILITY: &str = "latticeaxiom:capability/client-shell@1";
/// Seals [`LaunchHandoff::for_ready_exact`] from a catalog record and lock hashes.
pub(crate) fn sealed_ready_exact_handoff(
    record: &WorldShellRecord,
    shell_lock_hash: CanonicalHash,
    world_lock_hash: CanonicalHash,
    now_ms: u64,
    confirmed_setting_transaction_revision: SettingTransactionRevision,
) -> Result<LaunchHandoff, LaunchHandoffError> {
    LaunchHandoff::for_ready_exact(
        record,
        LaunchHandoffContext {
            generation: next_launch_generation(),
            issued_at_ms: now_ms,
            expires_at_ms: now_ms.saturating_add(MAX_LAUNCH_INTENT_LIFETIME_MS),
            shell_lock_hash,
            world_lock_hash,
            confirmed_setting_transaction_revision,
        },
    )
}

fn next_launch_generation() -> LaunchGeneration {
    std::env::var("LATTICEAXIOM_GENERATION")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .and_then(|current| current.checked_add(1))
        .and_then(|next| LaunchGeneration::new(next).ok())
        .unwrap_or(LaunchGeneration::FIRST)
}

fn current_launch_generation() -> LaunchGeneration {
    std::env::var("LATTICEAXIOM_GENERATION")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .and_then(|current| LaunchGeneration::new(current).ok())
        .unwrap_or(LaunchGeneration::FIRST)
}

fn current_process_epoch() -> ProcessEpoch {
    std::env::var("LATTICEAXIOM_PROCESS_EPOCH")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .and_then(|value| ProcessEpoch::new(value).ok())
        .unwrap_or(ProcessEpoch::FIRST)
}

/// Milliseconds since Unix epoch; `0` when the system clock is unavailable.
#[cfg(feature = "client")]
pub(crate) fn unix_now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .and_then(|duration| u64::try_from(duration.as_millis()).ok())
        .unwrap_or(0)
}

fn session_command(target: &'static str, action: SemanticActionId) -> SemanticCommand {
    SemanticCommand {
        target: match SemanticNodeId::new(target) {
            Ok(id) => id,
            Err(error) => unreachable!("validated static semantic ID `{target}`: {error}"),
        },
        action,
        source: InputSource::Headless,
    }
}

fn apply_game_surface(
    instance: &mut EngineInstance,
    command: &SurfaceCommandV1,
) -> Result<(), ProductionMemoryStartError> {
    let paused = if let Some(mut router) = instance
        .app
        .world_mut()
        .get_resource_mut::<ProductionSurfaceRouter>()
    {
        let receipt = router.apply(command)?;
        matches!(
            receipt.route.modal(),
            GameModalV1::Pause
                | GameModalV1::Settings
                | GameModalV1::ConfirmSaveQuit
                | GameModalV1::BindingCapture
        ) || receipt.route.transition() != latticeaxiom_client_ui::GameTransitionV1::None
    } else {
        matches!(
            command,
            SurfaceCommandV1::Pause | SurfaceCommandV1::OpenSettings
        )
    };
    set_session_paused(instance, paused);
    Ok(())
}

fn set_session_paused(instance: &mut EngineInstance, paused: bool) {
    if let Some(mut latch) = instance
        .app
        .world_mut()
        .get_resource_mut::<ProductionSessionPause>()
    {
        latch.set(paused);
        return;
    }
    instance
        .app
        .world_mut()
        .insert_resource(ProductionSessionPause::new(paused));
}

pub(super) fn writable_open_plan(world_id: WorldId, permit: &ActivationPermitV1) -> WorldOpenPlan {
    let action = WorldOpenAction::UseFrozenLock;
    WorldOpenPlan {
        world_id,
        status: WorldOpenStatus::ReadyExact,
        risk: WorldOpenRisk::None,
        reconciliation: ReconciliationState::InSync {
            metadata_epoch: permit.metadata_epoch().get(),
        },
        next_safe_step: Some(action.clone()),
        actions: vec![action],
        diagnostics: Vec::new(),
        activation_binding: Some(sealed_activation_binding(permit)),
    }
}

fn memory_session_authoritative_metadata(
    images: &LockVerifiedComposeImages,
    preset: TerrainPresetV2,
    terrain: &TerrainConfigV2,
) -> Result<AuthoritativeMetadataInputV1, ProductionMemoryStartError> {
    let lock_hash = DigestV1::from_bytes(*images.product_lock_hash().as_bytes());
    let lock = FrozenLockReceiptV1::new(
        images.product_lock_hash().to_string().into_bytes(),
        BTreeMap::new(),
        lock_hash,
        lock_hash,
        lock_hash,
        lock_hash,
        lock_hash,
    )?;
    let profile = preset.profile_id_str().parse::<StableId>()?;
    let terrain_hash = terrain
        .canonical_hash()
        .map_err(ProductionHostError::from)?;
    let closure = WorldRequirementClosureV1::new(
        BTreeMap::new(),
        BTreeMap::new(),
        BTreeSet::new(),
        BTreeMap::new(),
        lock_hash,
        lock_hash,
        BTreeMap::from([(profile, DigestV1::from_bytes(*terrain_hash.as_bytes()))]),
    )?;
    Ok(AuthoritativeMetadataInputV1::new(lock, closure))
}

fn terrain_preset_for_intent(
    intent: &QuickCreateIntent,
) -> Result<TerrainPresetV2, ProductionMemoryStartError> {
    let Some(profile) = intent.generation_profile.as_ref() else {
        return Ok(TerrainPresetV2::Balanced);
    };
    TerrainPresetV2::from_profile_id(profile).ok_or_else(|| {
        ProductionMemoryStartError::UnknownWorldgenProfile {
            profile: profile.clone(),
        }
    })
}

fn worldgen_profile_options() -> Result<Vec<WorldgenProfileOption>, IdentifierError> {
    TerrainPresetV2::ALL
        .into_iter()
        .map(|preset| {
            let (label, description) = match preset {
                TerrainPresetV2::Balanced => (
                    "Balanced",
                    "Continents, island chains, rivers, lakes, wetlands, plateaus, and mountain ranges.",
                ),
                TerrainPresetV2::Continental => (
                    "Continental",
                    "Large landmasses, long river systems, broad interiors, and inland ranges.",
                ),
                TerrainPresetV2::Archipelago => (
                    "Archipelago",
                    "Deep oceans, dense island chains, rugged coasts, and compact watersheds.",
                ),
                TerrainPresetV2::Alpine => (
                    "Alpine",
                    "Continuous high mountain systems, deep valleys, and strong altitude climate.",
                ),
                TerrainPresetV2::Eroded => (
                    "Eroded",
                    "Old low-relief terrain with broad valleys, lakes, and extensive wetlands.",
                ),
                TerrainPresetV2::Wild => (
                    "Wild",
                    "A 1024-voxel world with extreme relief and uncommon volcanic landforms.",
                ),
            };
            Ok(WorldgenProfileOption::new(
                preset.profile_id_str().parse::<StableId>()?,
                label,
                description,
            ))
        })
        .collect()
}

/// Resolves the exactly-one start-ui capability providers from a locked graph.
pub(crate) fn shell_graph_from_lock(
    graph: &LockedGameGraph,
) -> Result<ClientShellGraph, ProductionMemoryStartError> {
    const CAPABILITIES: [(ShellCapability, &str); 5] = [
        (ShellCapability::ClientShell, CLIENT_SHELL_CAPABILITY),
        (
            ShellCapability::WorldCatalog,
            "latticeaxiom:capability/world-catalog@1",
        ),
        (
            ShellCapability::SettingsSurface,
            "latticeaxiom:capability/settings-surface@1",
        ),
        (
            ShellCapability::SettingsRegistry,
            "latticeaxiom:capability/settings-registry@1",
        ),
        (
            ShellCapability::DiagnosticRegistry,
            "latticeaxiom:capability/diagnostic-registry@1",
        ),
    ];
    let mut providers = Vec::new();
    for (capability, identity) in CAPABILITIES {
        let capability_id = identity.parse::<CapabilityId>()?;
        let Some(packages) = graph.capability_providers.get(&capability_id) else {
            return Err(ProductionMemoryStartError::MissingShellCapability { capability });
        };
        if packages.is_empty() {
            return Err(ProductionMemoryStartError::MissingShellCapability { capability });
        }
        for package in packages {
            providers.push(ShellPackageProvider {
                package: package.clone(),
                capability,
            });
        }
    }
    Ok(ClientShellGraph::resolve(providers)?)
}

#[cfg(test)]
mod tests {
    use super::sealed_ready_exact_handoff;
    use latticeaxiom_core::{CanonicalHash, WorldId};
    use latticeaxiom_launcher::{LaunchTargetV1, SettingTransactionRevision};
    use latticeaxiom_start_ui::{
        ClientProcessDisposition, LaunchHandoffError, WorldCardMetadata, WorldShellRecord,
    };
    use latticeaxiom_world_catalog::{
        CatalogEntry, CatalogEntryState, CatalogProjection, DisplayName, LiveWorldLocation,
        ReconciliationState, WorldOpenAction, WorldOpenPlan, WorldOpenRisk, WorldOpenStatus,
        WorldRootId,
    };

    fn world_id(value: &str) -> WorldId {
        value
            .parse()
            .unwrap_or_else(|error| panic!("world fixture: {error}"))
    }

    fn ready_exact_record() -> WorldShellRecord {
        let world_id = world_id("123e4567-e89b-42d3-a456-426614174000");
        let action = WorldOpenAction::UseFrozenLock;
        WorldShellRecord::new(
            CatalogEntry {
                location: LiveWorldLocation::new(WorldRootId(0), world_id),
                state: CatalogEntryState::Projected(CatalogProjection {
                    world_id,
                    display_name: DisplayName::new("Exact")
                        .unwrap_or_else(|error| panic!("display fixture: {error}")),
                    metadata_epoch: 1,
                    clean_shutdown: true,
                    durable_frontier: 0,
                }),
            },
            WorldCardMetadata {
                created_at_ms: 1,
                last_played_at_ms: 2,
                physical_bytes: None,
                game_summary: None,
                dimension_summary: None,
            },
            Some(WorldOpenPlan {
                world_id,
                status: WorldOpenStatus::ReadyExact,
                risk: WorldOpenRisk::None,
                reconciliation: ReconciliationState::InSync { metadata_epoch: 1 },
                next_safe_step: Some(action.clone()),
                actions: vec![action],
                diagnostics: Vec::new(),
                activation_binding: None,
            }),
        )
        .unwrap_or_else(|error| panic!("record fixture: {error}"))
    }

    #[test]
    fn continue_ready_exact_seals_replacement_process_handoff() {
        let record = ready_exact_record();
        let lock = CanonicalHash::digest(b"shell-lock");
        let settings_revision = SettingTransactionRevision::new(7);
        let handoff = sealed_ready_exact_handoff(&record, lock, lock, 1_000, settings_revision)
            .unwrap_or_else(|error| panic!("ReadyExact handoff: {error}"));
        assert_eq!(
            handoff.intent.target(),
            LaunchTargetV1::World {
                world_id: record.world_id()
            }
        );
        assert_eq!(
            handoff.disposition,
            ClientProcessDisposition::ExitAfterAtomicIntentPublish
        );
        assert_eq!(
            handoff.intent.confirmed_setting_transaction_revision(),
            settings_revision
        );

        let mut compatible = ready_exact_record();
        if let Some(plan) = compatible.open_plan.as_mut() {
            plan.status = WorldOpenStatus::ReadyCompatible;
            plan.actions = vec![WorldOpenAction::ResolveCompatibleGraph];
        }
        assert!(matches!(
            sealed_ready_exact_handoff(&compatible, lock, lock, 1_000, settings_revision),
            Err(LaunchHandoffError::NotReadyExact)
        ));
    }
}
