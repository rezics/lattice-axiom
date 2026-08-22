//! In-memory start-ui create/list/pause/save/exit/continue for the production host.
//!
//! Worlds published here live in [`crate::MemoryTransactionKernel`] for the
//! current process. Pause opens the start-ui overlay and does not mutate the
//! materialized-chunk world hash. Save flushes dirty chunks through
//! [`SealedWorldWriterHost`] as Written commits and closes the writer; it does
//! not call [`SealedWorldWriterHost::flush_durable`]. Exit drops the live host
//! so Continue on a storage-backed world reopens [`DeterministicWorldStorage`]
//! first. This is not a physical checkpoint, trash, or restore path.

use std::{
    collections::{BTreeMap, BTreeSet},
    time::Duration,
};

use bevy::prelude::Resource;
use latticeaxiom_compose::LockedGameGraph;
use latticeaxiom_core::{CanonicalHash, CapabilityId, IdentifierError, PackageName, WorldId};
use latticeaxiom_launcher::{
    LaunchGeneration, MAX_LAUNCH_INTENT_LIFETIME_MS, SettingTransactionRevision,
};
use latticeaxiom_start_ui::{
    ClientShellGraph, ClientShellGraphError, HomePrimaryAction, InMemoryWorldList, InputSource,
    LaunchHandoff, LaunchHandoffContext, LaunchHandoffError, MemoryStartEffect, MemoryStartError,
    MemoryStartFlow, QuickCreateIntent, SemanticActionId, SemanticCommand, SemanticNodeId,
    ShellCapability, ShellEffect, ShellPackageProvider, WorldShellError, WorldShellRecord,
    WorldSort, memory_session_store_id, memory_session_template,
};
use latticeaxiom_world_catalog::{
    ReconciliationState, WorldOpenAction, WorldOpenPlan, WorldOpenRisk, WorldOpenStatus,
};
use latticeaxiom_world_db::{
    ActivationPermitV1, AuthoritativeMetadataInputV1, DeterministicWorldStorage, DigestV1,
    FrozenLockReceiptV1, WorldCreateRequestV1, WorldDbError, WorldRequirementClosureV1,
    WorldStorage,
};
use thiserror::Error;

use super::{
    ProductionHostError, ProductionInspectSurface, ProductionSessionPause, ProductionSpine,
    SealedWorldWriterHost, SealedWriterHostError, sealed_activation_binding,
};
use crate::{EngineInstance, LockVerifiedComposeImages, VerifiedProductLockHash};

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

/// Production-client create/list/continue over an in-memory world list.
///
/// Each created world materializes a [`ProductionSpine`] on first play. The
/// spine and list stay in process memory so Continue observes the same
/// [`WorldId`]. An optional shared [`DeterministicWorldStorage`] fills
/// [`latticeaxiom_world_catalog::WorldOpenPlan::activation_binding`] from
/// storage preflight; create still publishes the identity to the in-memory
/// list. When storage is absent, create stays memory-only.
#[derive(Debug)]
pub struct ProductionMemoryStart {
    images: LockVerifiedComposeImages,
    flow: MemoryStartFlow,
    spines: BTreeMap<WorldId, ProductionSpine>,
    storage: Option<DeterministicWorldStorage>,
}

impl ProductionMemoryStart {
    /// Builds an empty memory-session start surface for lock-verified images.
    #[must_use]
    pub fn new(images: LockVerifiedComposeImages, graph: ClientShellGraph) -> Self {
        Self {
            images,
            flow: MemoryStartFlow::new(graph),
            spines: BTreeMap::new(),
            storage: None,
        }
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
        Ok(Self::new(images, graph))
    }

    /// Selects the start-shell process from reopened lock graph roots.
    ///
    /// True only when `roots` contains `@latticeaxiom/front-end` and does not
    /// contain `terrenia`. A `terrenia` root, including `profiles/dev.toml`
    /// client-world, boots the production game host instead.
    #[must_use]
    pub fn lock_roots_select_shell<I, S>(roots: I) -> bool
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let mut has_front_end = false;
        let mut has_terrenia = false;
        for root in roots {
            match root.as_ref() {
                FRONT_END_PACKAGE => has_front_end = true,
                TERRENIA_PACKAGE => has_terrenia = true,
                _ => {}
            }
        }
        has_front_end && !has_terrenia
    }

    /// Selects the start-shell process from a reopened locked graph.
    #[must_use]
    pub fn lock_graph_selects_shell(graph: &LockedGameGraph) -> bool {
        Self::lock_roots_select_shell(graph.roots.iter().map(PackageName::as_str))
    }

    /// Seals a replacement-process handoff for an exact-ready session world.
    ///
    /// This does not create a Bevy game [`bevy::app::App`] and does not spawn
    /// [`super::ProductionSpine`]. An external supervisor must persist the
    /// intent and spawn the game process.
    ///
    /// # Errors
    ///
    /// Returns [`ProductionMemoryStartError`] when the world is absent or is
    /// not exact-ready, or when launcher intent validation fails.
    pub fn launch_handoff_for_ready_exact(
        &self,
        world_id: WorldId,
        now_ms: u64,
    ) -> Result<LaunchHandoff, ProductionMemoryStartError> {
        let record = self
            .flow
            .worlds()
            .get(world_id)
            .ok_or(WorldShellError::MissingLiveWorld)?;
        sealed_ready_exact_handoff(
            record,
            self.images.product_lock_hash(),
            self.images.product_lock_hash(),
            now_ms,
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
            .next()
            .cloned()
            .ok_or(ProductionMemoryStartError::NoGraphRoot)?;
        Ok(QuickCreateIntent::new(
            display_name,
            memory_session_template(),
            root,
            self.images.product_lock_hash(),
        )
        .map_err(MemoryStartError::from)?)
    }

    /// Publishes an in-memory world without opening a catalog writer.
    ///
    /// When shared storage is attached, the world is provisioned and the open
    /// plan's activation binding is filled from storage preflight. The
    /// [`WorldId`] is still published to the in-memory list. When storage is
    /// [`None`], create stays memory-only.
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
        let world_id = WorldId::new_v4();
        self.provision_created_world(world_id, intent)?;
        Ok(self.flow.create(intent, world_id, now_ms)?)
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
            self.images.clone(),
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
        set_session_paused(instance, true);
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
        set_session_paused(instance, false);
        Ok(effect)
    }

    /// Save & Quit: flush Written chunks, drop the host, and return Home.
    ///
    /// V2 uses the sealed-writer Written close as the replacement-process
    /// barrier. This is not a `RocksDB` Durable checkpoint.
    ///
    /// # Errors
    ///
    /// Returns [`ProductionMemoryStartError`] when pause/save/exit is rejected.
    pub fn save_and_quit(
        &mut self,
        world_id: WorldId,
        instance: EngineInstance,
        writer: &mut SealedWorldWriterHost,
    ) -> Result<MemoryStartEffect, ProductionMemoryStartError> {
        self.save_world(world_id, writer)?;
        self.exit_world(world_id, instance)
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
    ) -> Result<(), ProductionMemoryStartError> {
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
        spine.flush_dirty_chunks(writer, preflight.metadata())?;
        writer.close()?;
        Ok(())
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
        if self.storage.is_none() {
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
        let spine = match &self.storage {
            Some(storage) => ProductionSpine::materialize_world_from_storage(
                &self.images,
                world_id,
                storage.clone(),
            )?,
            None => ProductionSpine::materialize_world(&self.images, world_id)?,
        };
        self.spines.insert(world_id, spine.clone());
        Ok(spine)
    }

    fn provision_created_world(
        &self,
        world_id: WorldId,
        intent: &QuickCreateIntent,
    ) -> Result<(), ProductionMemoryStartError> {
        let Some(storage) = &self.storage else {
            return Ok(());
        };
        let metadata = memory_session_authoritative_metadata(&self.images)?;
        storage.provision_world(WorldCreateRequestV1::new(
            world_id,
            intent.display_name.clone(),
            memory_session_store_id(),
            metadata,
        ))?;
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
    /// The lock graph has no root package to bind to a create intent.
    #[error("lock graph has no root package")]
    NoGraphRoot,
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
}

impl From<WorldShellError> for ProductionMemoryStartError {
    fn from(error: WorldShellError) -> Self {
        Self::Start(error.into())
    }
}

/// Logical package name of the package-driven start shell.
const FRONT_END_PACKAGE: &str = "@latticeaxiom/front-end";
/// Logical package name of the current demo game world.
const TERRENIA_PACKAGE: &str = "terrenia";

/// Seals [`LaunchHandoff::for_ready_exact`] from a catalog record and lock hashes.
pub(crate) fn sealed_ready_exact_handoff(
    record: &WorldShellRecord,
    shell_lock_hash: CanonicalHash,
    world_lock_hash: CanonicalHash,
    now_ms: u64,
) -> Result<LaunchHandoff, LaunchHandoffError> {
    LaunchHandoff::for_ready_exact(
        record,
        LaunchHandoffContext {
            generation: next_launch_generation(),
            issued_at_ms: now_ms,
            expires_at_ms: now_ms.saturating_add(MAX_LAUNCH_INTENT_LIFETIME_MS),
            shell_lock_hash,
            world_lock_hash,
            confirmed_setting_transaction_revision: SettingTransactionRevision::new(0),
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

fn writable_open_plan(world_id: WorldId, permit: &ActivationPermitV1) -> WorldOpenPlan {
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
    let closure = WorldRequirementClosureV1::new(
        BTreeMap::new(),
        BTreeMap::new(),
        BTreeSet::new(),
        BTreeMap::new(),
        lock_hash,
        lock_hash,
        BTreeMap::new(),
    )?;
    Ok(AuthoritativeMetadataInputV1::new(lock, closure))
}

/// Resolves the exactly-one start-ui capability providers from a locked graph.
pub(crate) fn shell_graph_from_lock(
    graph: &LockedGameGraph,
) -> Result<ClientShellGraph, ProductionMemoryStartError> {
    const CAPABILITIES: [(ShellCapability, &str); 5] = [
        (
            ShellCapability::ClientShell,
            "latticeaxiom:capability/client-shell@1",
        ),
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
    use super::{
        FRONT_END_PACKAGE, ProductionMemoryStart, TERRENIA_PACKAGE, sealed_ready_exact_handoff,
    };
    use latticeaxiom_core::{CanonicalHash, WorldId};
    use latticeaxiom_launcher::LaunchTargetV1;
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
    fn lock_roots_select_shell_only_for_front_end_without_terrenia() {
        assert!(ProductionMemoryStart::lock_roots_select_shell([
            FRONT_END_PACKAGE
        ]));
        assert!(!ProductionMemoryStart::lock_roots_select_shell([
            TERRENIA_PACKAGE
        ]));
        assert!(!ProductionMemoryStart::lock_roots_select_shell([
            FRONT_END_PACKAGE,
            TERRENIA_PACKAGE,
        ]));
        assert!(!ProductionMemoryStart::lock_roots_select_shell([
            "@latticeaxiom/settings"
        ]));
        assert!(!ProductionMemoryStart::lock_roots_select_shell(
            None::<&str>
        ));
    }

    #[test]
    fn continue_ready_exact_seals_replacement_process_handoff() {
        let record = ready_exact_record();
        let lock = CanonicalHash::digest(b"shell-lock");
        let handoff = sealed_ready_exact_handoff(&record, lock, lock, 1_000)
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

        let mut compatible = ready_exact_record();
        if let Some(plan) = compatible.open_plan.as_mut() {
            plan.status = WorldOpenStatus::ReadyCompatible;
            plan.actions = vec![WorldOpenAction::ResolveCompatibleGraph];
        }
        assert!(matches!(
            sealed_ready_exact_handoff(&compatible, lock, lock, 1_000),
            Err(LaunchHandoffError::NotReadyExact)
        ));
    }
}
