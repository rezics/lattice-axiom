//! Process-local in-memory world list for create/list/continue.
//!
//! This is the production-client session catalog. Continue resumes the same
//! process-local [`WorldId`]. Pause opens an overlay without mutating world
//! state. Save and Exit are host effects: the host performs sealed-writer
//! Durable commits and checkpoints. An optional shared
//! [`DeterministicWorldStorage`] fills [`WorldOpenPlan::activation_binding`]
//! from storage preflight. When storage is absent, create stays memory-only.

use std::collections::BTreeMap;

use latticeaxiom_core::{CanonicalHash, StableId, WorldId};
use latticeaxiom_world_catalog::{
    CatalogEntry, CatalogEntryState, CatalogProjection, DisplayNameError, LiveWorldLocation,
    ReconciliationState, SealedActivationBindingV1, StoreId, WorldOpenAction, WorldOpenPlan,
    WorldOpenRisk, WorldOpenStatus, WorldRootId,
};
use latticeaxiom_world_db::{
    ActivationPermitV1, DeterministicWorldStorage, WorldDbError, WorldStorage,
};
use thiserror::Error;

use crate::{
    HomePrimaryAction, QuickCreateIntent, SemanticCommand, ShellCommandError, ShellEffect,
    ShellScreen, StartShellModel, WorldCardMetadata, WorldListModel, WorldShellError,
    WorldShellRecord, WorldSort, WorldgenProfileOption,
};

/// Catalog root ordinal reserved for process-local memory sessions.
///
/// The host never resolves this ordinal to a filesystem root or durable store.
pub const MEMORY_SESSION_WORLD_ROOT: WorldRootId = WorldRootId(0);

/// Process-local world list backed only by memory.
///
/// Entries are [`WorldOpenStatus::ReadyExact`] because they were created in this
/// process against the current lock. The list does not write a catalog.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct InMemoryWorldList {
    records: BTreeMap<WorldId, WorldShellRecord>,
}

impl InMemoryWorldList {
    /// Creates an empty process-local list.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            records: BTreeMap::new(),
        }
    }

    /// Returns whether the list has no session worlds.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }

    /// Returns the number of live in-memory session worlds.
    #[must_use]
    pub fn len(&self) -> usize {
        self.records.len()
    }

    /// Returns world identities in stable identifier order.
    #[must_use]
    pub fn world_ids(&self) -> Vec<WorldId> {
        self.records.keys().copied().collect()
    }

    /// Returns one session record by identity.
    #[must_use]
    pub fn get(&self, world_id: WorldId) -> Option<&WorldShellRecord> {
        self.records.get(&world_id)
    }

    /// Inserts a new in-memory session world without opening a writer.
    ///
    /// `activation_binding` is catalog evidence from storage preflight. Memory-only
    /// sessions pass [`None`] and remain unwritable.
    ///
    /// # Errors
    ///
    /// Returns [`WorldShellError::DuplicateWorldId`] when `world_id` is already
    /// present, or [`WorldShellError::PlanIdentityMismatch`] if the `ReadyExact`
    /// plan cannot be attached.
    pub fn create(
        &mut self,
        intent: &QuickCreateIntent,
        world_id: WorldId,
        now_ms: u64,
        activation_binding: Option<SealedActivationBindingV1>,
    ) -> Result<WorldId, WorldShellError> {
        if self.records.contains_key(&world_id) {
            return Err(WorldShellError::DuplicateWorldId);
        }
        let record = WorldShellRecord::new(
            CatalogEntry {
                location: LiveWorldLocation::new(MEMORY_SESSION_WORLD_ROOT, world_id),
                state: CatalogEntryState::Projected(CatalogProjection {
                    world_id,
                    display_name: intent.display_name.clone(),
                    metadata_epoch: 1,
                    clean_shutdown: true,
                    durable_frontier: 0,
                }),
            },
            WorldCardMetadata {
                created_at_ms: now_ms,
                last_played_at_ms: now_ms,
                physical_bytes: None,
                game_summary: Some(intent.root_game_package.to_string()),
                dimension_summary: None,
            },
            Some(memory_session_open_plan(world_id, activation_binding)),
        )?;
        self.records.insert(world_id, record);
        Ok(world_id)
    }

    /// Updates last-played so Continue targets this session world.
    ///
    /// # Errors
    ///
    /// Returns [`WorldShellError::MissingLiveWorld`] when the identity is absent.
    pub fn mark_played(
        &mut self,
        world_id: WorldId,
        played_at_ms: u64,
    ) -> Result<(), WorldShellError> {
        let record = self
            .records
            .get_mut(&world_id)
            .ok_or(WorldShellError::MissingLiveWorld)?;
        record.metadata.last_played_at_ms = played_at_ms;
        Ok(())
    }

    /// Records a host-proven durable frontier without opening a writer.
    ///
    /// # Errors
    ///
    /// Returns [`WorldShellError::MissingLiveWorld`] when the identity is absent.
    pub fn record_durable_frontier(
        &mut self,
        world_id: WorldId,
        durable_frontier: u64,
        clean_shutdown: bool,
    ) -> Result<(), WorldShellError> {
        let record = self
            .records
            .get_mut(&world_id)
            .ok_or(WorldShellError::MissingLiveWorld)?;
        if let CatalogEntryState::Projected(projection) = &mut record.entry.state {
            projection.durable_frontier = durable_frontier;
            projection.clean_shutdown = clean_shutdown;
        }
        Ok(())
    }

    /// Projects the live session worlds into a deterministically sorted list.
    #[must_use]
    pub fn list(&self, sort: WorldSort) -> WorldListModel {
        WorldListModel::new(self.records.values().cloned().collect(), sort)
    }
}

/// Semantic create/list/continue coordinator over [`InMemoryWorldList`].
///
/// [`SemanticActionId::QuickCreate`](crate::SemanticActionId::QuickCreate)
/// publishes a session world. Continue is exact-ready in-memory play, not a
/// replacement-process handoff.
#[derive(Clone, Debug)]
pub struct MemoryStartFlow {
    shell: StartShellModel,
    worlds: InMemoryWorldList,
    draft: Option<QuickCreateIntent>,
    now_ms: u64,
    storage: Option<DeterministicWorldStorage>,
}

impl MemoryStartFlow {
    /// Restores a catalog projection without reading voxel payloads or opening a writer.
    ///
    /// # Errors
    /// Returns [`WorldShellError`] if this identity is already in the list.
    pub fn restore_record(&mut self, record: WorldShellRecord) -> Result<(), WorldShellError> {
        let world = record.world_id();
        if self.worlds.records.contains_key(&world) {
            return Err(WorldShellError::DuplicateWorldId);
        }
        self.worlds.records.insert(world, record);
        self.sync_worlds();
        Ok(())
    }

    /// Refreshes a known catalog projection while retaining its world identity.
    ///
    /// # Errors
    /// Returns [`WorldShellError`] when the world is not already listed.
    pub fn replace_record(&mut self, record: WorldShellRecord) -> Result<(), WorldShellError> {
        let world = record.world_id();
        if !self.worlds.records.contains_key(&world) {
            return Err(WorldShellError::MissingLiveWorld);
        }
        self.worlds.records.insert(world, record);
        self.sync_worlds();
        Ok(())
    }

    /// Attaches a fresh read-only preflight to an existing catalog record.
    ///
    /// # Errors
    /// Returns [`WorldShellError`] for a missing or mismatched world.
    pub fn attach_open_plan(
        &mut self,
        world: WorldId,
        plan: WorldOpenPlan,
    ) -> Result<(), WorldShellError> {
        if plan.world_id != world {
            return Err(WorldShellError::PlanIdentityMismatch);
        }
        let record = self
            .worlds
            .records
            .get_mut(&world)
            .ok_or(WorldShellError::MissingLiveWorld)?;
        record.open_plan = Some(plan);
        self.sync_worlds();
        Ok(())
    }

    /// Builds an empty home shell over a validated package graph.
    #[must_use]
    pub fn new(graph: crate::ClientShellGraph) -> Self {
        let worlds = InMemoryWorldList::new();
        Self {
            shell: StartShellModel::new(graph, worlds.list(WorldSort::LastPlayed)),
            worlds,
            draft: None,
            now_ms: 1,
            storage: None,
        }
    }

    /// Returns the presentation-neutral shell.
    #[must_use]
    pub const fn shell(&self) -> &StartShellModel {
        &self.shell
    }

    /// Returns the mutable presentation-neutral shell.
    pub const fn shell_mut(&mut self) -> &mut StartShellModel {
        &mut self.shell
    }

    /// Returns the process-local world list.
    #[must_use]
    pub const fn worlds(&self) -> &InMemoryWorldList {
        &self.worlds
    }

    /// Returns the pending quick-create intent, when set.
    #[must_use]
    pub const fn draft(&self) -> Option<&QuickCreateIntent> {
        self.draft.as_ref()
    }

    /// Clock used when semantic quick-create publishes a session world.
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

    /// Clock used when semantic quick-create publishes a session world.
    #[must_use]
    pub const fn now_ms(&self) -> u64 {
        self.now_ms
    }

    /// Shares a deterministic world store used to bind create plans from preflight.
    ///
    /// When absent, create stays memory-only and
    /// [`WorldOpenPlan::activation_binding`] remains [`None`].
    pub fn set_storage(&mut self, storage: DeterministicWorldStorage) {
        self.storage = Some(storage);
    }

    /// Returns the shared world store, when one is attached.
    #[must_use]
    pub const fn storage(&self) -> Option<&DeterministicWorldStorage> {
        self.storage.as_ref()
    }

    /// Publishes an in-memory world and refreshes the shell list.
    ///
    /// When storage is attached, the open plan's activation binding is filled
    /// from read-only storage preflight. The [`WorldId`] is still published to
    /// the in-memory list.
    ///
    /// # Errors
    ///
    /// Returns [`WorldShellError`] when the identity already exists, or
    /// [`MemoryStartError`] when attached storage preflight is not ready.
    pub fn create(
        &mut self,
        intent: &QuickCreateIntent,
        world_id: WorldId,
        now_ms: u64,
    ) -> Result<WorldId, MemoryStartError> {
        let activation_binding = self.activation_binding_from_storage(world_id)?;
        let world_id = self
            .worlds
            .create(intent, world_id, now_ms, activation_binding)?;
        self.draft = None;
        self.sync_worlds();
        self.shell.screen = ShellScreen::Home;
        Ok(world_id)
    }

    /// Validates and applies a semantic command without publishing a world.
    ///
    /// Quick-create returns [`ShellEffect::RequestQuickCreate`] so a host that
    /// owns storage can provision before [`Self::create`].
    ///
    /// # Errors
    ///
    /// Returns [`MemoryStartError`] when the current tree rejects the command.
    pub fn apply_shell_command(
        &mut self,
        command: &SemanticCommand,
    ) -> Result<ShellEffect, MemoryStartError> {
        let effect = self.shell.inject(command)?;
        if effect == ShellEffect::WorldgenProfileSelected
            && let Some(profile) = self.shell.selected_worldgen_profile().cloned()
            && let Some(draft) = self.draft.as_mut()
        {
            draft.set_generation_profile(profile);
        }
        Ok(effect)
    }

    /// Routes Continue/Play onto the Loading surface in this process.
    pub fn enter_loading(&mut self) {
        self.shell.enter_world_loading();
    }

    /// Advertises the in-session pause overlay for a live world.
    ///
    /// Pause does not write, flush, or otherwise mutate world state.
    pub fn enter_playing(&mut self) {
        self.shell.loading = None;
        self.shell.screen = ShellScreen::Playing;
    }

    /// Advances the Continue/Play loading route by one honest stage.
    ///
    /// # Errors
    ///
    /// Returns [`crate::LoadingStateError`] when no loading state exists or the
    /// requested stage is not a valid successor.
    pub fn advance_loading(
        &mut self,
        stage: crate::LoadingStage,
        progress: crate::LoadingProgress,
        current_item: Option<String>,
    ) -> Result<(), crate::LoadingStateError> {
        let loading = self
            .shell
            .loading
            .as_mut()
            .ok_or(crate::LoadingStateError::MissingLoadingState)?;
        loading.advance(stage, progress, current_item)
    }

    /// Returns the shell to Home without opening a writer.
    ///
    /// Shutdown-timeout recovery uses this path so a timeout cannot masquerade
    /// as durable Save & Quit.
    pub fn enter_home(&mut self) {
        self.shell.screen = ShellScreen::Home;
    }

    /// Records that an in-memory session was entered.
    ///
    /// # Errors
    ///
    /// Returns [`WorldShellError::MissingLiveWorld`] when the identity is absent.
    pub fn mark_played(
        &mut self,
        world_id: WorldId,
        played_at_ms: u64,
    ) -> Result<(), WorldShellError> {
        self.worlds.mark_played(world_id, played_at_ms)?;
        self.sync_worlds();
        Ok(())
    }

    /// Records a host-proven durable frontier and refreshes the shell list.
    ///
    /// # Errors
    ///
    /// Returns [`WorldShellError::MissingLiveWorld`] when the identity is absent.
    pub fn record_durable_frontier(
        &mut self,
        world_id: WorldId,
        durable_frontier: u64,
        clean_shutdown: bool,
    ) -> Result<(), WorldShellError> {
        self.worlds
            .record_durable_frontier(world_id, durable_frontier, clean_shutdown)?;
        self.sync_worlds();
        Ok(())
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
    /// Quick-create publishes a session world from the current draft. Continue
    /// emits [`ShellEffect::RequestExactWorldLaunch`] so the host can load the
    /// world in this process after the Loading route.
    ///
    /// # Errors
    ///
    /// Returns [`MemoryStartError`] when the tree rejects the command, the
    /// quick-create draft is missing, or session publication fails.
    pub fn inject(
        &mut self,
        command: &SemanticCommand,
    ) -> Result<MemoryStartEffect, MemoryStartError> {
        match self.apply_shell_command(command)? {
            ShellEffect::RequestQuickCreate => {
                let intent = self
                    .draft
                    .clone()
                    .ok_or(MemoryStartError::MissingQuickCreateDraft)?;
                let world_id = self.create(&intent, WorldId::new_v4(), self.now_ms)?;
                Ok(MemoryStartEffect::Created(world_id))
            }
            effect => Ok(MemoryStartEffect::Shell(effect)),
        }
    }

    fn sync_worlds(&mut self) {
        self.shell.worlds = self.worlds.list(WorldSort::LastPlayed);
    }

    fn activation_binding_from_storage(
        &self,
        world_id: WorldId,
    ) -> Result<Option<SealedActivationBindingV1>, MemoryStartError> {
        let Some(storage) = &self.storage else {
            return Ok(None);
        };
        let preflight = storage
            .preflight(world_id)
            .map_err(|error| MemoryStartError::from_storage("preflight", error))?;
        let Some(permit) = preflight.activation_permit() else {
            return Err(MemoryStartError::StorageActivationUnavailable { world: world_id });
        };
        Ok(Some(sealed_activation_binding(permit)))
    }
}

/// Observable result of one accepted memory-session shell command.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MemoryStartEffect {
    /// Quick-create published a process-local world.
    Created(WorldId),
    /// Shell routing or launch effect that did not publish a world.
    Shell(ShellEffect),
}

/// Invalid memory-session command or publication.
#[derive(Clone, Debug, Error, PartialEq)]
pub enum MemoryStartError {
    /// Accessibility tree or current route rejected the command.
    #[error(transparent)]
    Shell(#[from] ShellCommandError),
    /// Session list publication failed.
    #[error(transparent)]
    World(#[from] WorldShellError),
    /// Quick-create display name failed validation.
    #[error(transparent)]
    DisplayName(#[from] DisplayNameError),
    /// Quick-create was activated without a typed intent.
    #[error("quick-create draft is missing")]
    MissingQuickCreateDraft,
    /// Attached world storage rejected a create-time preflight.
    #[error("world storage {operation} failed: {detail}")]
    Storage {
        /// Storage operation that failed.
        operation: &'static str,
        /// Typed storage diagnostic.
        detail: String,
    },
    /// Attached storage preflight did not yield ready activation evidence.
    #[error("world storage preflight did not yield activation evidence for {world}")]
    StorageActivationUnavailable {
        /// World whose preflight lacked a ready permit.
        world: WorldId,
    },
}

impl MemoryStartError {
    fn from_storage(operation: &'static str, error: WorldDbError) -> Self {
        match error {
            WorldDbError::ActivationEvidenceUnavailable { world } => {
                Self::StorageActivationUnavailable { world }
            }
            error => Self::Storage {
                operation,
                detail: error.to_string(),
            },
        }
    }
}

/// Template identity for a process-local memory session world.
#[must_use]
pub fn memory_session_template() -> StableId {
    match "latticeaxiom:world-template/memory-session".parse() {
        Ok(id) => id,
        Err(error) => unreachable!("validated memory-session template: {error}"),
    }
}

/// Store-generation identity used when a memory session provisions into shared storage.
#[must_use]
pub fn memory_session_store_id() -> StoreId {
    match StoreId::new("latticeaxiom-memory-session") {
        Ok(id) => id,
        Err(error) => unreachable!("validated memory-session store ID: {error}"),
    }
}

fn memory_session_open_plan(
    world_id: WorldId,
    activation_binding: Option<SealedActivationBindingV1>,
) -> WorldOpenPlan {
    WorldOpenPlan {
        world_id,
        status: WorldOpenStatus::ReadyExact,
        risk: WorldOpenRisk::None,
        reconciliation: ReconciliationState::InSync { metadata_epoch: 1 },
        next_safe_step: Some(WorldOpenAction::UseFrozenLock),
        actions: vec![WorldOpenAction::UseFrozenLock],
        diagnostics: Vec::new(),
        activation_binding,
    }
}

fn sealed_activation_binding(permit: &ActivationPermitV1) -> SealedActivationBindingV1 {
    SealedActivationBindingV1 {
        store_id: permit.store_id().clone(),
        metadata_epoch: permit.metadata_epoch().get(),
        metadata_hash: CanonicalHash::from_bytes(*permit.metadata_hash().as_bytes()),
        projection_hash: CanonicalHash::from_bytes(*permit.projection_hash().as_bytes()),
        plan_generation: permit.metadata_epoch().get(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use latticeaxiom_core::{CanonicalHash, PackageName};

    fn intent() -> QuickCreateIntent {
        QuickCreateIntent::new(
            "Session",
            memory_session_template(),
            package("@latticeaxiom/front-end"),
            CanonicalHash::digest(b"profile"),
        )
        .unwrap_or_else(|error| panic!("quick-create fixture: {error}"))
    }

    fn package(value: &str) -> PackageName {
        value
            .parse()
            .unwrap_or_else(|error| panic!("package fixture: {error}"))
    }

    #[test]
    fn create_list_and_continue_share_the_same_world_id() {
        let mut list = InMemoryWorldList::new();
        let world = WorldId::new_v4();
        list.create(&intent(), world, 10, None)
            .unwrap_or_else(|error| panic!("create: {error}"));
        let model = list.list(WorldSort::LastPlayed);
        assert_eq!(model.records().len(), 1);
        assert_eq!(model.records()[0].world_id(), world);
        assert!(matches!(
            model.home_primary_action(),
            HomePrimaryAction::Continue { world_id, .. } if world_id == world
        ));
        list.mark_played(world, 20)
            .unwrap_or_else(|error| panic!("mark played: {error}"));
        assert_eq!(
            list.get(world)
                .map(|record| record.metadata.last_played_at_ms),
            Some(20)
        );
    }

    #[test]
    fn duplicate_session_identity_is_rejected() {
        let mut list = InMemoryWorldList::new();
        let world = WorldId::new_v4();
        list.create(&intent(), world, 1, None)
            .unwrap_or_else(|error| panic!("first create: {error}"));
        assert_eq!(
            list.create(&intent(), world, 2, None),
            Err(WorldShellError::DuplicateWorldId)
        );
    }

    #[test]
    fn create_attaches_supplied_activation_binding() {
        let mut list = InMemoryWorldList::new();
        let world = WorldId::new_v4();
        let binding = SealedActivationBindingV1 {
            store_id: memory_session_store_id(),
            metadata_epoch: 1,
            metadata_hash: CanonicalHash::digest(b"metadata"),
            projection_hash: CanonicalHash::digest(b"projection"),
            plan_generation: 1,
        };
        list.create(&intent(), world, 1, Some(binding.clone()))
            .unwrap_or_else(|error| panic!("create: {error}"));
        assert_eq!(
            list.get(world)
                .and_then(|record| record.open_plan.as_ref())
                .and_then(|plan| plan.activation_binding.as_ref()),
            Some(&binding)
        );
    }
}
