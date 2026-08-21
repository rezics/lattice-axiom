//! Process-local in-memory world list for create/list/continue.
//!
//! This is the production-client session catalog until a durable writer exists.
//! Continue resumes the same process-local [`WorldId`]; it does not load a
//! checkpoint, restore trash, or publish a catalog sidecar.

use std::collections::BTreeMap;

use latticeaxiom_core::{StableId, WorldId};
use latticeaxiom_world_catalog::{
    CatalogEntry, CatalogEntryState, CatalogProjection, DisplayNameError, LiveWorldLocation,
    ReconciliationState, WorldOpenAction, WorldOpenPlan, WorldOpenRisk, WorldOpenStatus,
    WorldRootId,
};
use thiserror::Error;

use crate::{
    HomePrimaryAction, QuickCreateIntent, SemanticCommand, ShellCommandError, ShellEffect,
    ShellScreen, StartShellModel, WorldCardMetadata, WorldListModel, WorldShellError,
    WorldShellRecord, WorldSort,
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
            Some(memory_session_open_plan(world_id)),
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
}

impl MemoryStartFlow {
    /// Builds an empty home shell over a validated package graph.
    #[must_use]
    pub fn new(graph: crate::ClientShellGraph) -> Self {
        let worlds = InMemoryWorldList::new();
        Self {
            shell: StartShellModel::new(graph, worlds.list(WorldSort::LastPlayed)),
            worlds,
            draft: None,
            now_ms: 1,
        }
    }

    /// Returns the presentation-neutral shell.
    #[must_use]
    pub const fn shell(&self) -> &StartShellModel {
        &self.shell
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

    /// Publishes an in-memory world and refreshes the shell list.
    ///
    /// # Errors
    ///
    /// Returns [`WorldShellError`] when the identity already exists.
    pub fn create(
        &mut self,
        intent: &QuickCreateIntent,
        world_id: WorldId,
        now_ms: u64,
    ) -> Result<WorldId, WorldShellError> {
        let world_id = self.worlds.create(intent, world_id, now_ms)?;
        self.draft = None;
        self.sync_worlds();
        self.shell.screen = ShellScreen::Home;
        Ok(world_id)
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
    /// still emits [`ShellEffect::RequestExactWorldLaunch`] for the host.
    ///
    /// # Errors
    ///
    /// Returns [`MemoryStartError`] when the tree rejects the command, the
    /// quick-create draft is missing, or session publication fails.
    pub fn inject(
        &mut self,
        command: &SemanticCommand,
    ) -> Result<MemoryStartEffect, MemoryStartError> {
        match self.shell.inject(command)? {
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
}

/// Template identity for a process-local memory session world.
#[must_use]
pub fn memory_session_template() -> StableId {
    match "latticeaxiom:world-template/memory-session".parse() {
        Ok(id) => id,
        Err(error) => unreachable!("validated memory-session template: {error}"),
    }
}

fn memory_session_open_plan(world_id: WorldId) -> WorldOpenPlan {
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
        list.create(&intent(), world, 10)
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
        list.create(&intent(), world, 1)
            .unwrap_or_else(|error| panic!("first create: {error}"));
        assert_eq!(
            list.create(&intent(), world, 2),
            Err(WorldShellError::DuplicateWorldId)
        );
    }
}
