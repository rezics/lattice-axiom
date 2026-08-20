//! In-memory start-ui create/list/continue for the production host.
//!
//! Worlds published here live in [`crate::MemoryTransactionKernel`] for the
//! current process. Continue resumes that session; it does not open a durable
//! catalog writer, trash, restore, or checkpoint path.

use std::{collections::BTreeMap, time::Duration};

use bevy::prelude::Resource;
use latticeaxiom_compose::LockedGameGraph;
use latticeaxiom_core::{CapabilityId, IdentifierError, WorldId};
use latticeaxiom_start_ui::{
    ClientShellGraph, ClientShellGraphError, HomePrimaryAction, InMemoryWorldList,
    MemoryStartEffect, MemoryStartError, MemoryStartFlow, QuickCreateIntent, SemanticCommand,
    ShellCapability, ShellPackageProvider, WorldShellError, WorldSort, memory_session_template,
};
use thiserror::Error;

use super::{ProductionHostError, ProductionInspectSurface, ProductionSpine};
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
/// [`WorldId`].
#[derive(Debug)]
pub struct ProductionMemoryStart {
    images: LockVerifiedComposeImages,
    flow: MemoryStartFlow,
    spines: BTreeMap<WorldId, ProductionSpine>,
}

impl ProductionMemoryStart {
    /// Builds an empty memory-session start surface for lock-verified images.
    #[must_use]
    pub fn new(images: LockVerifiedComposeImages, graph: ClientShellGraph) -> Self {
        Self {
            images,
            flow: MemoryStartFlow::new(graph),
            spines: BTreeMap::new(),
        }
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
    /// # Errors
    ///
    /// Returns [`ProductionMemoryStartError`] when the session list rejects the
    /// new identity.
    pub fn create(
        &mut self,
        intent: &QuickCreateIntent,
        now_ms: u64,
    ) -> Result<WorldId, ProductionMemoryStartError> {
        Ok(self.flow.create(intent, WorldId::new_v4(), now_ms)?)
    }

    /// Returns the exact-ready Continue target, when one exists.
    #[must_use]
    pub fn continue_world_id(&self) -> Option<WorldId> {
        self.flow.continue_world_id()
    }

    /// Applies a start-ui semantic command to the in-memory session list.
    ///
    /// # Errors
    ///
    /// Returns [`ProductionMemoryStartError`] when the current tree rejects the
    /// command or quick-create cannot publish.
    pub fn inject(
        &mut self,
        command: &SemanticCommand,
    ) -> Result<MemoryStartEffect, ProductionMemoryStartError> {
        Ok(self.flow.inject(command)?)
    }

    /// Materializes the in-memory session into a GPU-free production host.
    ///
    /// A previously played world reuses the same spine so Continue stays on the
    /// same [`WorldId`] and memory kernel.
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
    /// # Errors
    ///
    /// Returns [`ProductionMemoryStartError::NoContinueWorld`] when Continue is
    /// not available, and otherwise the same failures as [`Self::play_headless`].
    pub fn play_continued_headless(
        &mut self,
        played_at_ms: u64,
        fixed_timestep: Duration,
    ) -> Result<(WorldId, EngineInstance), ProductionMemoryStartError> {
        let world_id = self
            .continue_world_id()
            .ok_or(ProductionMemoryStartError::NoContinueWorld)?;
        let instance = self.play_headless(world_id, played_at_ms, fixed_timestep)?;
        Ok((world_id, instance))
    }

    fn ensure_spine(
        &mut self,
        world_id: WorldId,
    ) -> Result<ProductionSpine, ProductionMemoryStartError> {
        if let Some(spine) = self.spines.get(&world_id) {
            return Ok(spine.clone());
        }
        let spine = ProductionSpine::materialize_world(&self.images, world_id)?;
        self.spines.insert(world_id, spine.clone());
        Ok(spine)
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
            super::install_production_host(app, product_lock_hash, spine, inspect_surface, true);
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
}

impl From<WorldShellError> for ProductionMemoryStartError {
    fn from(error: WorldShellError) -> Self {
        Self::Start(error.into())
    }
}

fn shell_graph_from_lock(
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
