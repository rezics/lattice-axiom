//! Production playable host spine for the V2/V4 package-driven slice.
//!
//! This module is the production Bevy world session. It starts from a
//! reopened [`LockVerifiedComposeImages`], streams a bounded working set
//! around the local player, presents CPU meshes at render distance, and keeps
//! Avian colliders local to the player capsule. [`MemoryTransactionKernel`]
//! remains the session cache. Durable Save
//! & Quit activates a sealed [`SealedWorldWriterHost`] only after exact lock,
//! catalog, world-header, and lease receipts match. It must not be confused
//! with the `playable` development fixture.

mod catalog;
#[cfg(feature = "client")]
mod chunk_mesh;
#[cfg(feature = "client")]
mod client;
mod display;
#[cfg(feature = "client")]
mod far_mesh;
mod far_stream;
mod fluid;
mod gameplay;
#[cfg(feature = "client")]
mod hud;
mod layers;
#[cfg(feature = "client")]
mod mining_ring;
#[cfg(feature = "client")]
mod pause;
pub(crate) mod persistent;
mod profile;
mod session;
#[cfg(feature = "client")]
mod settings_view;
#[cfg(feature = "client")]
mod shell_view;
#[cfg(feature = "client")]
pub(crate) use shell_view::ShellHandoffState;
mod spine;
mod start;
mod stream;
mod surface;
#[cfg(feature = "client")]
mod voxel_icon;
#[cfg(feature = "client")]
mod water_material;
#[cfg(feature = "client")]
pub(crate) use water_material::install_resource_water_shader;
mod worldgen;
mod writer;

use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
};

use avian3d::{
    PhysicsPlugins,
    prelude::{ColliderTreeOptimization, LinearVelocity},
};
use bevy::{
    app::{App, Plugin},
    ecs::schedule::IntoScheduleConfigs,
    prelude::{
        Commands, Component, Entity, FixedPostUpdate, FixedUpdate, MessageWriter, Query, Res,
        ResMut, Resource, Transform, Update, With, Without,
    },
    transform::TransformPlugin,
};
#[cfg(feature = "client")]
use bevy::{
    app::{FixedFirst, PreUpdate, Startup},
    asset::Assets,
    input::InputSystems,
    prelude::{ClearColor, Color, Mesh},
};
use latticeaxiom_compose::LockedGameGraph;
use latticeaxiom_core::{CapabilityId, IdentifierError, PackageName, StableId};
use latticeaxiom_gameplay::{GameplayIdError, GameplayReject};
use latticeaxiom_player::{
    ActionFrameInbox, ActionFrameInboxError, AuthoritativeTargetInspectRequestV1,
    BlockEditAuthorityResource, CurrentPlayerActionFrame, D2Player, D2PlayerBundle,
    LocalPlayerInput, PlayerActionV1, PlayerFixedTick, PlayerMovementProfileV1, PlayerPlugin,
    PlayerSystemSet, PlayerViewV1, TargetEyePoseV1, TargetInspectReceiptV1,
};
#[cfg(feature = "client")]
use latticeaxiom_player::{
    ClientInputOwnership, ClientInputSystemSet, CompiledClientInputMaps,
    LeafwingInputAdapterPlugin, LocalPlayerClientInputBundle,
};
use latticeaxiom_registration::CompiledRegistration;
use latticeaxiom_runtime_contracts::WorldgenInspectError;
use latticeaxiom_storage::{ChunkCoordinate, ChunkRevision, StorageError};
use latticeaxiom_voxel_mesh::{MeshError, MeshReceipt};
use latticeaxiom_voxel_runtime::RuntimeError;
use latticeaxiom_world_db::WorldDbError;
use latticeaxiom_worldgen::WorldgenError;
use thiserror::Error;

const TARGET_INSPECT_SURFACE_CAPABILITY: &str = "latticeaxiom:capability/target-inspect-surface@1";
const DIAGNOSTIC_REGISTRY_CAPABILITY: &str = "latticeaxiom:capability/diagnostic-registry@1";
const DEBUG_WORKBENCH_CAPABILITY: &str = "latticeaxiom:capability/debug-workbench@1";

pub use catalog::{
    AuthoredContentCatalogSourcesV1, AuthoredGameplayCatalogSourcesV1,
    compile_authored_content_catalog, compile_authored_gameplay_catalog, empty_gameplay_catalog,
    lock_selected_content_catalog, lock_selected_gameplay_catalog,
};
pub use display::{
    AuthoredContentDisplayCatalogSourcesV1, AuthoredPresentationCatalogSourcesV1,
    ContentDisplayCatalogV1, ContentDisplayLabelV1, compile_authored_content_display_catalog,
    lock_selected_content_display_catalog,
};
pub use far_stream::{
    FAR_TERRAIN_IN_FLIGHT_CAP, FAR_TERRAIN_PENDING_CAP, FAR_TERRAIN_READY_CAP,
    FarTerrainQueueSnapshotV1,
};
pub use fluid::HostFluidTickV1;
pub use gameplay::{HOTBAR_SLOTS, INVENTORY_SLOTS, ProductionInventoryView};
pub use latticeaxiom_runtime_contracts::{
    FarTerrainQualityV1, FullDetailDistanceChunksV1, PresentedRenderDistanceChunksV1,
    RequestedFullDetailDistanceChunksV1, RequestedRenderDistanceChunksV1,
    RequestedSimulationDistanceChunksV1, SimulationDistanceChunksV1, TargetRenderDistanceChunksV1,
    TerrainDistanceRequestsV1, WorldgenInspectReportV1,
};
pub use latticeaxiom_worldgen::{CaveOccupancyArbitrationV1, ChunkFaceV1};
pub use profile::{
    ADR_0026_ACTIVE_COVERAGE_M, ADR_0026_CHUNK_EDGE_VOXELS, ADR_0026_RESIDENT_COVERAGE_M,
    STREAMING_PROFILE_EVIDENCE_SCHEMA_V1, StreamingProfileCountsV1, StreamingProfileEvidenceV1,
    coverage_m, radius_for_coverage,
};
pub use spine::{
    CellOccupancyV1, DerivedQueueSnapshotV1, ProductionSpine, ProductionWorldStorage,
    WorkingSetDiagnosticsV1, WorldgenQueueSnapshotV1,
};
pub use start::{
    ChildResultV1, ProductionMemoryStart, ProductionMemoryStartError, ProductionWorldList,
};
pub use stream::{
    ChunkLifecycle, FullDetailClampReasonV1, PrefetchDistanceChunksV1,
    PresentedRenderDistanceMetersV1, ResidentDistanceChunksV1, TerrainDistanceStatusV1,
};
pub use surface::ProductionSurfaceRouter;
pub use worldgen::RequiredCaveEntranceV1;
pub use writer::{SealedWorldWriterHost, SealedWriterHostError, sealed_activation_binding};

#[cfg(feature = "client")]
use crate::EngineProfile;
use crate::{
    EngineInstance, EngineInstanceError, LockVerifiedComposeImages, VerifiedProductLockHash,
};

use self::spine::{ColliderGeneration, ColliderPresentation};

/// Cursor capturing derived mesh identity before an authoritative edit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChunkMeshCursor {
    pub(crate) coordinate: ChunkCoordinate,
    pub(crate) receipt: MeshReceipt,
    pub(crate) revision: ChunkRevision,
}

impl ChunkMeshCursor {
    /// Returns the chunk whose mesh was captured.
    #[must_use]
    pub const fn coordinate(self) -> ChunkCoordinate {
        self.coordinate
    }

    /// Returns the captured mesh receipt.
    #[must_use]
    pub const fn receipt(self) -> MeshReceipt {
        self.receipt
    }

    /// Returns the captured chunk revision.
    #[must_use]
    pub const fn revision(self) -> ChunkRevision {
        self.revision
    }
}

/// Marker on the single ECS entity that presents one committed chunk.
#[derive(Clone, Copy, Component, Debug, Eq, PartialEq)]
pub struct ChunkPresentation {
    /// Canonical chunk coordinate presented by this entity.
    pub coordinate: ChunkCoordinate,
}

/// Last host collider generation applied to one chunk presentation entity.
#[derive(Clone, Copy, Component, Debug, Eq, PartialEq)]
struct ChunkColliderGeneration(ColliderGeneration);

/// Movement gate held closed while the current capsule lacks matching
/// revision-checked chunk colliders.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Resource)]
struct PlayerColliderSafetyGate {
    blocked: bool,
}

/// In-session pause latch for a production host.
///
/// Pause does not flush, commit, or otherwise mutate the materialized-chunk
/// world hash. Chunk streaming is skipped while paused.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Resource)]
pub struct ProductionSessionPause {
    paused: bool,
}

/// Presentation-only GPU/device-loss latch.
///
/// Recording device loss must never change the materialized-chunk world hash.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Resource)]
pub struct RenderDeviceLost {
    lost: bool,
}

impl RenderDeviceLost {
    /// Marks that the render device was lost.
    #[must_use]
    pub const fn reported() -> Self {
        Self { lost: true }
    }

    /// Returns whether device loss has been reported.
    #[must_use]
    pub const fn is_lost(self) -> bool {
        self.lost
    }
}

impl ProductionSessionPause {
    /// Creates a latch in the requested state.
    #[must_use]
    pub const fn new(paused: bool) -> Self {
        Self { paused }
    }

    /// Returns whether the session is paused.
    #[must_use]
    pub const fn is_paused(self) -> bool {
        self.paused
    }

    /// Sets the pause latch.
    pub const fn set(&mut self, paused: bool) {
        self.paused = paused;
    }
}

/// Latest local-player pose copied from the authoritative capsule.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ProductionPlayerPose {
    /// Capsule center in meters.
    pub translation: bevy::prelude::Vec3,
    /// Yaw around world `+Y`.
    pub yaw_radians: f32,
    /// Whether the last controller probe found walkable ground.
    pub grounded: bool,
}

/// Inspect/observability providers selected by the frozen package graph.
///
/// When neither capability evidence nor a compiled inspect catalog selects
/// both surfaces, the host uses
/// [`latticeaxiom_player::HeadlessTargetInspectV1`] from authoritative DDA.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Resource)]
pub struct ProductionInspectSurface {
    graph_selected_inspect: bool,
    graph_selected_observability: bool,
}

impl ProductionInspectSurface {
    /// Derives inspect-surface selection from a reopened lock image.
    #[must_use]
    pub fn from_lock_images(images: &LockVerifiedComposeImages) -> Self {
        Self::from_graph(images.images().graph(), images.images().registration())
    }

    fn from_graph(graph: &LockedGameGraph, registration: &CompiledRegistration) -> Self {
        Self::from_capability_evidence(
            &graph.capability_providers,
            !registration.image.observability.inspect.is_empty(),
        )
    }

    fn from_capability_evidence(
        providers: &BTreeMap<CapabilityId, Vec<PackageName>>,
        compiled_inspect: bool,
    ) -> Self {
        Self {
            graph_selected_inspect: capability_present(
                providers,
                TARGET_INSPECT_SURFACE_CAPABILITY,
            ) || compiled_inspect,
            graph_selected_observability: capability_present(
                providers,
                DIAGNOSTIC_REGISTRY_CAPABILITY,
            ),
        }
    }

    /// Returns whether a lock capability or compiled catalog selected inspect.
    #[must_use]
    pub const fn graph_selected_inspect(self) -> bool {
        self.graph_selected_inspect
    }

    /// Returns whether the lock graph selected the diagnostic capability.
    #[must_use]
    pub const fn graph_selected_observability(self) -> bool {
        self.graph_selected_observability
    }

    /// Returns whether inspect uses the headless DDA DTO fallback.
    #[must_use]
    pub const fn uses_headless_dto(self) -> bool {
        !(self.graph_selected_inspect && self.graph_selected_observability)
    }
}

pub(super) fn capability_present(
    providers: &BTreeMap<CapabilityId, Vec<PackageName>>,
    capability: &str,
) -> bool {
    providers.keys().any(|id| id.as_str() == capability)
}

#[cfg(test)]
mod inspect_surface_tests {
    use super::*;

    fn capabilities(ids: &[&str]) -> BTreeMap<CapabilityId, Vec<PackageName>> {
        let provider = "@example/provider"
            .parse::<PackageName>()
            .expect("test package is canonical");
        ids.iter()
            .map(|id| {
                (
                    id.parse::<CapabilityId>()
                        .expect("test capability is canonical"),
                    vec![provider.clone()],
                )
            })
            .collect()
    }

    #[test]
    fn inspect_surface_requires_capability_or_compiled_catalog_evidence() {
        let absent = ProductionInspectSurface::from_capability_evidence(&BTreeMap::new(), false);
        assert!(!absent.graph_selected_inspect());
        assert!(!absent.graph_selected_observability());
        assert!(absent.uses_headless_dto());

        let inspect_only = ProductionInspectSurface::from_capability_evidence(
            &capabilities(&[TARGET_INSPECT_SURFACE_CAPABILITY]),
            false,
        );
        assert!(inspect_only.graph_selected_inspect());
        assert!(!inspect_only.graph_selected_observability());
        assert!(inspect_only.uses_headless_dto());

        let selected = ProductionInspectSurface::from_capability_evidence(
            &capabilities(&[
                TARGET_INSPECT_SURFACE_CAPABILITY,
                DIAGNOSTIC_REGISTRY_CAPABILITY,
            ]),
            false,
        );
        assert!(!selected.uses_headless_dto());

        let compiled = ProductionInspectSurface::from_capability_evidence(
            &capabilities(&[DIAGNOSTIC_REGISTRY_CAPABILITY]),
            true,
        );
        assert!(!compiled.uses_headless_dto());
    }
}

/// Bevy plugin that spawns chunk colliders and the local player.
///
/// `FixedUpdate` only applies the current capsule's collider safety gate.
/// Interest is reconciled once in `FixedPostUpdate` from the final pose after
/// movement, Avian writeback, and edit evaluation. Camera `Update` copies the
/// eye transform and does not write chunk interest.
#[derive(Clone, Copy, Debug, Default)]
pub struct ProductionHostPlugin;

impl Plugin for ProductionHostPlugin {
    #[allow(clippy::too_many_lines)] // This method declares the host scheduler contract in one place.
    fn build(&self, app: &mut App) {
        app.add_message::<TargetInspectReceiptV1>()
            .add_systems(Update, pump_chunk_background)
            .add_systems(
                FixedUpdate,
                (
                    sync_collider_safety.before(PlayerSystemSet::ProbeGround),
                    freeze_player_while_collider_unready
                        .after(sync_collider_safety)
                        .after(PlayerSystemSet::PrepareMovement)
                        .before(PlayerSystemSet::MoveCapsule),
                    evaluate_pick_block,
                ),
            )
            .add_systems(
                FixedPostUpdate,
                (
                    evaluate_target_inspect.after(PlayerSystemSet::EvaluateEdit),
                    sync_player_pose.after(PlayerSystemSet::EvaluateEdit),
                    sync_chunk_stream.after(sync_player_pose),
                    sync_chunk_colliders.after(sync_chunk_stream),
                    sync_working_set_diagnostics.after(sync_chunk_colliders),
                    refresh_crosshair_target
                        .after(sync_player_pose)
                        .after(evaluate_target_inspect),
                ),
            );
        #[cfg(feature = "client")]
        app.init_resource::<far_mesh::FarTerrainPresentationStatusV1>()
            .add_observer(pause::render_distance_slider_changed)
            .add_observer(pause::pause_menu_activated)
            .add_observer(settings_view::settings_page_activated)
            .add_observer(settings_view::settings_integer_slider_changed)
            .add_observer(hud::inventory_slot_activated)
            .add_observer(hud::recipe_activated)
            .add_observer(hud::item_browser_activated)
            .add_systems(
                Startup,
                (
                    voxel_icon::build_production_voxel_icons,
                    spawn_production_hud_if_client.after(voxel_icon::build_production_voxel_icons),
                    client::spawn_production_client_view,
                    pause::spawn_pause_overlay_if_client,
                    attach_initial_chunk_meshes.after(client::spawn_production_client_view),
                )
                    .run_if(is_interactive_client),
            )
            .add_systems(
                PreUpdate,
                (
                    crate::cursor_capture::observe_primary_window_focus,
                    pause::update_cursor_capture,
                )
                    .chain()
                    .after(InputSystems)
                    .before(ClientInputSystemSet::Sample)
                    .run_if(is_interactive_client),
            )
            .add_systems(
                Update,
                far_mesh::sync_far_terrain_presentation
                    .after(pump_chunk_background)
                    .before(client::sync_production_camera)
                    .run_if(is_interactive_client),
            )
            .add_systems(
                Update,
                (
                    client::sync_production_camera,
                    client::sync_water_material_medium,
                    pause::toggle_pause,
                    pause::sync_pause_overlay,
                    surface::apply_surface_actions,
                    pause::apply_settings_surface_actions,
                    pause::sync_cursor_capture,
                    pause::sync_pause_menu_page,
                    settings_view::sync_settings_page,
                    settings_view::sync_settings_page_visibility,
                    pause::sync_settings_control_focus_visuals,
                    hud::activate_workbench_from_target,
                    surface::select_hotbar_from_surface,
                    hud::capture_item_browser_search,
                    hud::sync_inventory_overlay,
                    hud::sync_workbench_overlay,
                    hud::sync_slot_pickable,
                    hud::sync_hand_recipe_list,
                    hud::sync_workbench_recipe_list,
                    hud::sync_item_browser,
                )
                    .chain()
                    .run_if(is_interactive_client),
            )
            .add_systems(
                Update,
                (
                    hud::sync_production_inspect_hud,
                    hud::sync_production_working_set_hud,
                    mining_ring::sync_mining_ring,
                    hud::sync_production_status_hud,
                    hud::sync_production_hotbar_hud,
                    hud::sync_production_inventory_hud,
                )
                    .chain()
                    .after(hud::sync_item_browser)
                    .run_if(is_interactive_client),
            )
            .add_systems(
                FixedFirst,
                pause::suppress_gameplay_while_paused.before(PlayerSystemSet::SampleInput),
            )
            .add_systems(
                FixedUpdate,
                pause::freeze_player_while_paused
                    .after(PlayerSystemSet::PrepareMovement)
                    .before(PlayerSystemSet::MoveCapsule),
            );
    }

    fn finish(&self, app: &mut App) {
        spawn_host_entities(app.world_mut());
    }
}

impl EngineInstance {
    /// Records GPU/device loss without mutating authoritative world state.
    ///
    /// Presentation and device failures never change the materialized-chunk
    /// world hash. Missing spine resources still latch the loss.
    ///
    /// # Errors
    ///
    /// Returns [`ProductionHostError`] when the spine lock is poisoned, a
    /// snapshot cannot be taken, or device-loss handling mutates world state.
    pub fn report_render_device_lost(&mut self) -> Result<(), ProductionHostError> {
        let before = self
            .app
            .world()
            .get_resource::<ProductionSpine>()
            .map(ProductionSpine::materialized_chunk_state_hash)
            .transpose()?;
        self.app
            .world_mut()
            .insert_resource(RenderDeviceLost::reported());
        if let Some(spine) = self.app.world().get_resource::<ProductionSpine>() {
            let after = spine.materialized_chunk_state_hash()?;
            if before.is_some_and(|hash| hash != after) {
                return Err(ProductionHostError::PresentationFailureMutatedWorld);
            }
        }
        Ok(())
    }

    /// Builds a GPU-free production spine from a reopened final product lock.
    ///
    /// The host inserts [`latticeaxiom_storage::MemoryTransactionKernel`] as the production storage
    /// implementation, streams a bounded working set around the player, and
    /// installs [`PlayerPlugin`]. It does not open a world writer.
    ///
    /// # Errors
    ///
    /// Returns [`ProductionHostError`] when the lock-verified images cannot
    /// materialize, Bevy construction fails, or the fixed timestep is zero.
    pub fn new_headless_host_from_lock(
        images: LockVerifiedComposeImages,
        fixed_timestep: std::time::Duration,
    ) -> Result<Self, ProductionHostError> {
        let product_lock_hash = VerifiedProductLockHash::new(images.product_lock_hash());
        let inspect_surface = ProductionInspectSurface::from_lock_images(&images);
        let images_for_spine = images.clone();
        let mut setup_error = None;
        let instance =
            Self::new_headless_with_setup(images.into_images(), fixed_timestep, |app| {
                match ProductionSpine::materialize(&images_for_spine) {
                    Ok(spine) => install_production_host(
                        app,
                        product_lock_hash,
                        spine,
                        inspect_surface,
                        true,
                        #[cfg(feature = "client")]
                        None,
                    ),
                    Err(error) => setup_error = Some(error),
                }
            })?;
        setup_error.map_or(Ok(instance), Err)
    }

    /// Builds a GPU-free production spine with a caller-supplied gameplay catalog.
    ///
    /// The catalog is compiled from package data by the caller. This path does
    /// not embed Terrenia identifiers and does not open a world writer.
    ///
    /// # Errors
    ///
    /// Returns [`ProductionHostError`] when materialization or Bevy construction
    /// fails.
    pub fn new_headless_host_from_lock_with_catalog(
        images: LockVerifiedComposeImages,
        fixed_timestep: std::time::Duration,
        catalog: latticeaxiom_gameplay::GameplayCatalog,
    ) -> Result<Self, ProductionHostError> {
        let product_lock_hash = VerifiedProductLockHash::new(images.product_lock_hash());
        let inspect_surface = ProductionInspectSurface::from_lock_images(&images);
        let images_for_spine = images.clone();
        let mut setup_error = None;
        let instance =
            Self::new_headless_with_setup(images.into_images(), fixed_timestep, |app| {
                match ProductionSpine::materialize_with_catalog(&images_for_spine, catalog) {
                    Ok(spine) => install_production_host(
                        app,
                        product_lock_hash,
                        spine,
                        inspect_surface,
                        true,
                        #[cfg(feature = "client")]
                        None,
                    ),
                    Err(error) => setup_error = Some(error),
                }
            })?;
        setup_error.map_or(Ok(instance), Err)
    }

    /// Builds the process's sole interactive client with the production spine.
    ///
    /// # Errors
    ///
    /// Returns [`ProductionHostError`] when the event loop is reserved or spine
    /// materialization fails.
    #[cfg(feature = "client")]
    pub fn new_client_host_from_lock(
        images: LockVerifiedComposeImages,
        lease: latticeaxiom_launcher::FreshClientAppLeaseToken,
    ) -> Result<(Self, latticeaxiom_launcher::FreshClientAppLeaseProof), ProductionHostError> {
        Self::new_client_host_from_lock_with_maps(images, lease, None)
    }

    /// Builds the process's sole interactive client with lock-compiled input maps.
    ///
    /// # Errors
    ///
    /// Returns [`ProductionHostError`] when the event loop is reserved or spine
    /// materialization fails.
    #[cfg(feature = "client")]
    pub fn new_client_host_from_lock_with_maps(
        images: LockVerifiedComposeImages,
        lease: latticeaxiom_launcher::FreshClientAppLeaseToken,
        maps: Option<latticeaxiom_player::CompiledClientInputMaps>,
    ) -> Result<(Self, latticeaxiom_launcher::FreshClientAppLeaseProof), ProductionHostError> {
        Self::new_client_host_with_source(images, lease, maps, None)
    }

    /// Reopens a specific disk-backed world in the sole client App.
    ///
    /// # Errors
    /// Returns a host error for invalid stored state or client initialization.
    #[cfg(feature = "client")]
    pub fn new_client_host_from_world(
        images: LockVerifiedComposeImages,
        lease: latticeaxiom_launcher::FreshClientAppLeaseToken,
        maps: Option<latticeaxiom_player::CompiledClientInputMaps>,
        world: latticeaxiom_core::WorldId,
        storage: latticeaxiom_world_db::DeterministicWorldStorage,
    ) -> Result<(Self, latticeaxiom_launcher::FreshClientAppLeaseProof), ProductionHostError> {
        Self::new_client_host_with_source(images, lease, maps, Some((world, storage)))
    }

    #[cfg(feature = "client")]
    fn new_client_host_with_source(
        images: LockVerifiedComposeImages,
        lease: latticeaxiom_launcher::FreshClientAppLeaseToken,
        maps: Option<latticeaxiom_player::CompiledClientInputMaps>,
        source: Option<(
            latticeaxiom_core::WorldId,
            latticeaxiom_world_db::DeterministicWorldStorage,
        )>,
    ) -> Result<(Self, latticeaxiom_launcher::FreshClientAppLeaseProof), ProductionHostError> {
        let product_lock_hash = VerifiedProductLockHash::new(images.product_lock_hash());
        let inspect_surface = ProductionInspectSurface::from_lock_images(&images);
        let images_for_spine = images.clone();
        let mut setup_error = None;
        let instance = Self::new_client_with_setup(images.into_images(), |app| {
            let spine = match source {
                Some((world, storage)) => ProductionSpine::materialize_world_from_storage(
                    &images_for_spine,
                    world,
                    storage,
                ),
                None => ProductionSpine::materialize(&images_for_spine),
            };
            match spine {
                Ok(spine) => install_production_host(
                    app,
                    product_lock_hash,
                    spine,
                    inspect_surface,
                    false,
                    maps,
                ),
                Err(error) => setup_error = Some(error),
            }
        })?;
        if let Some(error) = setup_error {
            return Err(error);
        }
        Ok((instance, lease.into_app_created_proof()))
    }

    /// Installs lock-selected user settings before the first client update.
    ///
    /// # Errors
    ///
    /// Returns [`crate::settings::HostSettingsError`] when the production spine
    /// is unavailable or initial runtime admission fails.
    #[cfg(feature = "client")]
    pub(crate) fn install_user_settings(
        &mut self,
        root: std::path::PathBuf,
        user: crate::settings::HostUserSettings,
        catalog: crate::settings::HostSettingsCatalog,
        active_lock: latticeaxiom_core::CanonicalHash,
    ) -> Result<(), crate::settings::HostSettingsError> {
        let spine = self
            .app
            .world()
            .get_resource::<ProductionSpine>()
            .cloned()
            .ok_or_else(|| crate::settings::HostSettingsError::Runtime {
                reason: "production spine is unavailable".to_owned(),
            })?;
        let state = pause::ProductionSettingsState::new(root, user, catalog, active_lock)?;
        spine
            .set_terrain_presentation(
                state.applied_terrain_distances(),
                state.applied_far_terrain_quality(),
            )
            .map_err(|error| crate::settings::HostSettingsError::Runtime {
                reason: error.to_string(),
            })?;
        let world = self.app.world_mut();
        if let Some(mut video) = world.get_resource_mut::<crate::VideoRuntimeSettings>() {
            let requested = state.applied_video();
            video.replace(
                requested.vsync(),
                requested.foreground_limit(),
                requested.background_limit(),
            );
        }
        let tick_rate = state.applied_tick_rate();
        world.insert_resource(latticeaxiom_player::SimulationClock::new(tick_rate));
        if let Some(mut fixed) = world.get_resource_mut::<bevy::time::Time<bevy::time::Fixed>>() {
            fixed.set_timestep(tick_rate.timestep());
        }
        world.insert_resource(state);
        Ok(())
    }
    /// Enqueues exact headless action frames on the production player inbox.
    ///
    /// # Errors
    ///
    /// Returns [`ActionFrameInboxError`] when the bounded queue is full or a
    /// generation is not strictly newer than the last queued frame.
    pub fn enqueue_headless_actions(
        &mut self,
        frames: impl IntoIterator<Item = latticeaxiom_player::PlayerActionFrameV1>,
    ) -> Result<(), ActionFrameInboxError> {
        let mut inbox = self.app.world_mut().resource_mut::<ActionFrameInbox>();
        for frame in frames {
            inbox.push_headless(frame)?;
        }
        Ok(())
    }

    /// Returns the number of chunk presentation entities spawned by the spine.
    #[must_use]
    pub fn production_chunk_entity_count(&self) -> usize {
        self.app
            .world()
            .iter_entities()
            .filter(bevy::ecs::world::EntityRef::contains::<ChunkPresentation>)
            .count()
    }
}

pub(super) fn install_production_host(
    app: &mut App,
    product_lock_hash: VerifiedProductLockHash,
    spine: ProductionSpine,
    inspect_surface: ProductionInspectSurface,
    include_transform: bool,
    #[cfg(feature = "client")] input_maps: Option<latticeaxiom_player::CompiledClientInputMaps>,
) {
    if include_transform {
        app.add_plugins(TransformPlugin);
    }
    let working_set = spine.working_set_diagnostics();
    #[cfg(feature = "client")]
    let terrain_palette = (!include_transform).then(|| {
        spine.terrain_layer_table().map_or_else(
            || chunk_mesh::ProductionTerrainPalette::from_ids(&spine.palette_ids()),
            chunk_mesh::ProductionTerrainPalette::from_layer_table,
        )
    });
    app.insert_resource(product_lock_hash)
        .insert_resource(inspect_surface)
        .insert_resource(spine.storage())
        .insert_resource(BlockEditAuthorityResource::new(spine.clone()))
        .insert_resource(working_set)
        .insert_resource(ProductionSessionPause::default())
        .insert_resource(PlayerColliderSafetyGate::default())
        // Player shape casts run before PhysicsSchedule. Inline optimization
        // prevents Avian's end-of-step join from queueing behind worldgen and
        // derived work on Bevy's shared AsyncComputeTaskPool.
        .insert_resource(ColliderTreeOptimization {
            optimize_in_place: true,
            use_async_tasks: false,
            ..Default::default()
        })
        .insert_resource(spine)
        .add_plugins(PhysicsPlugins::default())
        .add_plugins(PlayerPlugin)
        .add_plugins(ProductionHostPlugin);
    if let Ok(router) = ProductionSurfaceRouter::playing() {
        app.insert_resource(router);
    }
    #[cfg(feature = "client")]
    if let Some(maps) = input_maps {
        app.insert_resource(maps);
    }
    #[cfg(feature = "client")]
    {
        app.insert_resource(hud::ProductionHudSurfaces::default())
            .init_resource::<mining_ring::ProductionMiningRingState>()
            .init_resource::<crate::cursor_capture::ConfirmedPrimaryWindowFocus>()
            .init_resource::<crate::cursor_capture::CursorCaptureState>()
            .init_resource::<ClientInputOwnership>();
    }
    #[cfg(feature = "client")]
    if let Some(terrain_palette) = terrain_palette {
        water_material::install_water_material(app);
        app.insert_resource(ClearColor(Color::srgb(0.48, 0.70, 0.91)))
            .insert_resource(terrain_palette)
            .add_plugins(LeafwingInputAdapterPlugin);
    }
}

fn spawn_host_entities(world: &mut bevy::prelude::World) {
    let Some(spine) = world.get_resource::<ProductionSpine>().cloned() else {
        return;
    };
    let spawn = spine.restored_spawn_translation();
    #[cfg(feature = "client")]
    {
        let client = world.get_resource::<EngineProfile>() == Some(&EngineProfile::Client);
        let maps = world.get_resource::<CompiledClientInputMaps>().cloned();
        let mut player = world.spawn(D2PlayerBundle::new(
            spine::local_player_id(),
            Transform::from_translation(spawn),
        ));
        if client {
            if let Some(maps) = maps {
                player.insert(LocalPlayerClientInputBundle::from_compiled(&maps));
            } else {
                player.insert(LocalPlayerClientInputBundle::default());
            }
        }
    }
    #[cfg(not(feature = "client"))]
    world.spawn(D2PlayerBundle::new(
        spine::local_player_id(),
        Transform::from_translation(spawn),
    ));
    if let Ok(delta) = spine.take_presentation() {
        for update in delta.collider_update {
            world.spawn((
                ChunkPresentation {
                    coordinate: update.coordinate,
                },
                ChunkColliderGeneration(update.generation),
                avian3d::prelude::RigidBody::Static,
                update.collider,
                Transform::from_translation(update.origin),
            ));
        }
    }
}

#[allow(clippy::needless_pass_by_value)] // Bevy systems receive SystemParams by value.
fn sync_chunk_stream(
    tick: Res<'_, PlayerFixedTick>,
    spine: Res<'_, ProductionSpine>,
    pause: Option<Res<'_, ProductionSessionPause>>,
) {
    if pause.is_some_and(|pause| pause.is_paused()) {
        return;
    }
    let _ = spine.sync_interest(tick.get());
}

#[allow(clippy::needless_pass_by_value)] // Bevy systems receive SystemParams by value.
fn pump_chunk_background(
    tick: Res<'_, PlayerFixedTick>,
    spine: Res<'_, ProductionSpine>,
    pause: Option<Res<'_, ProductionSessionPause>>,
) {
    if pause.is_some_and(|pause| pause.is_paused()) {
        return;
    }
    let _ = spine.pump_background_work(tick.get());
}

#[allow(clippy::needless_pass_by_value)] // Bevy systems receive SystemParams by value.
#[allow(clippy::too_many_arguments)] // Collider identity is a separate read-only ECS query.
#[allow(clippy::type_complexity)] // Player-capsule collider mapping is one safety query.
fn sync_collider_safety(
    tick: Res<'_, PlayerFixedTick>,
    mut commands: Commands<'_, '_>,
    spine: Res<'_, ProductionSpine>,
    mut gate: ResMut<'_, PlayerColliderSafetyGate>,
    pause: Option<Res<'_, ProductionSessionPause>>,
    chunks: Query<'_, '_, (Entity, &ChunkPresentation)>,
    mut transforms: Query<'_, '_, &mut Transform>,
    mut colliders: Query<'_, '_, &mut avian3d::prelude::Collider>,
    collider_generations: Query<'_, '_, &ChunkColliderGeneration>,
) {
    if pause.is_some_and(|pause| pause.is_paused()) {
        return;
    }
    let Ok(safety) = spine.ensure_collider_safety(tick.get()) else {
        gate.blocked = true;
        return;
    };
    gate.blocked = !safety.ready;
    let mut by_coordinate = chunks
        .iter()
        .map(|(entity, presentation)| (presentation.coordinate, entity))
        .collect::<BTreeMap<_, _>>();
    let active = safety
        .updates
        .iter()
        .map(|update| update.coordinate)
        .collect::<BTreeSet<_>>();
    for (entity, presentation) in &chunks {
        if !active.contains(&presentation.coordinate) && collider_generations.get(entity).is_ok() {
            commands.entity(entity).remove::<(
                avian3d::prelude::RigidBody,
                avian3d::prelude::Collider,
                ChunkColliderGeneration,
            )>();
        }
    }
    for update in safety.updates {
        apply_collider_update(
            &mut commands,
            &mut transforms,
            &mut colliders,
            &collider_generations,
            &mut by_coordinate,
            update,
        );
    }
}

#[cfg(all(test, feature = "client"))]
mod production_schedule_tests {
    use bevy::{app::App, ecs::schedule::Schedules, prelude::Update};

    use super::ProductionHostPlugin;

    #[test]
    fn production_update_schedule_builds_without_ordering_cycles() {
        let mut app = App::new();
        app.add_plugins(ProductionHostPlugin);
        let mut schedule = app
            .world_mut()
            .resource_mut::<Schedules>()
            .remove(Update)
            .expect("the production plugin registers the Update schedule");

        schedule
            .initialize(app.world_mut())
            .expect("the production Update schedule must be acyclic");
    }
}

#[allow(clippy::needless_pass_by_value)] // Bevy systems receive SystemParams by value.
fn freeze_player_while_collider_unready(
    gate: Res<'_, PlayerColliderSafetyGate>,
    mut players: Query<'_, '_, &mut LinearVelocity, With<D2Player>>,
) {
    if !gate.blocked {
        return;
    }
    for mut velocity in &mut players {
        *velocity = LinearVelocity::ZERO;
    }
}

#[allow(clippy::needless_pass_by_value)] // Bevy systems receive SystemParams by value.
fn sync_working_set_diagnostics(
    spine: Res<'_, ProductionSpine>,
    mut snapshot: ResMut<'_, WorkingSetDiagnosticsV1>,
) {
    *snapshot = spine.working_set_diagnostics();
}

#[cfg(feature = "client")]
#[allow(clippy::needless_pass_by_value)] // Bevy run conditions receive SystemParams by value.
fn is_interactive_client(profile: Res<'_, EngineProfile>) -> bool {
    *profile == EngineProfile::Client
}

#[cfg(feature = "client")]
#[allow(clippy::needless_pass_by_value)] // Bevy systems receive SystemParams by value.
fn spawn_production_hud_if_client(commands: Commands<'_, '_>, profile: Res<'_, EngineProfile>) {
    if *profile == EngineProfile::Client {
        hud::spawn_production_hud(commands);
    }
}

#[allow(clippy::needless_pass_by_value)] // Bevy systems receive SystemParams by value.
#[allow(clippy::too_many_arguments)] // Client mesh params are optional presentation resources.
#[allow(clippy::type_complexity)] // Chunk collider query is one presentation mapping.
fn sync_chunk_colliders(
    mut commands: Commands<'_, '_>,
    spine: Res<'_, ProductionSpine>,
    chunks: Query<'_, '_, (Entity, &ChunkPresentation)>,
    mut transforms: Query<'_, '_, &mut Transform>,
    mut colliders: Query<'_, '_, &mut avian3d::prelude::Collider>,
    collider_generations: Query<'_, '_, &ChunkColliderGeneration>,
    #[cfg(feature = "client")] mut meshes: Option<ResMut<'_, Assets<Mesh>>>,
    #[cfg(feature = "client")] material: Option<Res<'_, chunk_mesh::ProductionTerrainMaterials>>,
    #[cfg(feature = "client")] palette: Option<Res<'_, chunk_mesh::ProductionTerrainPalette>>,
    #[cfg(feature = "client")] gpu_meshes: Query<'_, '_, &chunk_mesh::ChunkGpuMesh>,
) {
    let Ok(delta) = spine.take_presentation() else {
        return;
    };
    let mut by_coordinate = chunks
        .iter()
        .map(|(entity, presentation)| (presentation.coordinate, entity))
        .collect::<BTreeMap<_, _>>();
    for coordinate in delta.removals {
        if let Some(entity) = by_coordinate.remove(&coordinate) {
            commands.entity(entity).despawn();
        }
    }
    for update in delta.collider_update {
        apply_collider_update(
            &mut commands,
            &mut transforms,
            &mut colliders,
            &collider_generations,
            &mut by_coordinate,
            update,
        );
    }
    #[cfg(not(feature = "client"))]
    {
        let _ = delta.mesh_update;
    }
    #[cfg(feature = "client")]
    for update in delta.mesh_update {
        let entity = if let Some(&entity) = by_coordinate.get(&update.coordinate) {
            if let Ok(mut transform) = transforms.get_mut(entity) {
                transform.translation = update.origin;
            }
            entity
        } else {
            let entity = commands
                .spawn((
                    ChunkPresentation {
                        coordinate: update.coordinate,
                    },
                    Transform::from_translation(update.origin),
                ))
                .id();
            by_coordinate.insert(update.coordinate, entity);
            entity
        };
        attach_chunk_mesh(
            &mut commands,
            meshes.as_mut(),
            material.as_ref(),
            palette.as_ref(),
            entity,
            &update.geometry,
            update.bounds,
            gpu_meshes.get(entity).ok(),
        );
    }
}

fn apply_collider_update(
    commands: &mut Commands<'_, '_>,
    transforms: &mut Query<'_, '_, &mut Transform>,
    colliders: &mut Query<'_, '_, &mut avian3d::prelude::Collider>,
    collider_generations: &Query<'_, '_, &ChunkColliderGeneration>,
    by_coordinate: &mut BTreeMap<ChunkCoordinate, Entity>,
    update: ColliderPresentation,
) {
    if let Some(&entity) = by_coordinate.get(&update.coordinate) {
        if collider_generations
            .get(entity)
            .is_ok_and(|generation| generation.0 == update.generation)
        {
            return;
        }
        if let Ok(mut transform) = transforms.get_mut(entity) {
            transform.translation = update.origin;
        }
        if let Ok(mut existing) = colliders.get_mut(entity) {
            *existing = update.collider;
            commands
                .entity(entity)
                .insert(ChunkColliderGeneration(update.generation));
        } else {
            commands.entity(entity).insert((
                ChunkColliderGeneration(update.generation),
                avian3d::prelude::RigidBody::Static,
                update.collider,
            ));
        }
        return;
    }
    let entity = commands
        .spawn((
            ChunkPresentation {
                coordinate: update.coordinate,
            },
            ChunkColliderGeneration(update.generation),
            avian3d::prelude::RigidBody::Static,
            update.collider,
            Transform::from_translation(update.origin),
        ))
        .id();
    by_coordinate.insert(update.coordinate, entity);
}

#[cfg(feature = "client")]
#[allow(clippy::too_many_arguments)] // Geometry, bounds, and previous GPU mesh are presentation inputs.
fn attach_chunk_mesh(
    commands: &mut Commands<'_, '_>,
    meshes: Option<&mut ResMut<'_, Assets<Mesh>>>,
    material: Option<&Res<'_, chunk_mesh::ProductionTerrainMaterials>>,
    palette: Option<&Res<'_, chunk_mesh::ProductionTerrainPalette>>,
    entity: Entity,
    geometry: &latticeaxiom_voxel_mesh::MeshBuffer<latticeaxiom_voxel_mesh::LayerMergeKey>,
    bounds: Option<latticeaxiom_voxel_mesh::Aabb>,
    existing: Option<&chunk_mesh::ChunkGpuMesh>,
) {
    let Some(meshes) = meshes else {
        return;
    };
    let Some(material) = material else {
        return;
    };
    let Some(palette) = palette else {
        return;
    };
    chunk_mesh::apply_chunk_mesh(
        commands, meshes, material, palette, entity, geometry, bounds, existing,
    );
}

#[cfg(feature = "client")]
#[allow(clippy::needless_pass_by_value)] // Bevy systems receive SystemParams by value.
fn attach_initial_chunk_meshes(
    mut commands: Commands<'_, '_>,
    spine: Res<'_, ProductionSpine>,
    chunks: Query<'_, '_, (Entity, &ChunkPresentation), Without<chunk_mesh::ChunkGpuMesh>>,
    mut meshes: Option<ResMut<'_, Assets<Mesh>>>,
    material: Option<Res<'_, chunk_mesh::ProductionTerrainMaterials>>,
    palette: Option<Res<'_, chunk_mesh::ProductionTerrainPalette>>,
) {
    for (entity, presentation) in &chunks {
        let Some(geometry) = spine.derived_geometry(presentation.coordinate) else {
            continue;
        };
        attach_chunk_mesh(
            &mut commands,
            meshes.as_mut(),
            material.as_ref(),
            palette.as_ref(),
            entity,
            &geometry,
            geometry.bounds(),
            None,
        );
    }
}

#[allow(clippy::needless_pass_by_value)] // Bevy systems receive SystemParams by value.
#[allow(clippy::type_complexity)] // Local player pose query is one bounded origin.
fn sync_player_pose(
    spine: Res<'_, ProductionSpine>,
    players: Query<
        '_,
        '_,
        (
            &Transform,
            &PlayerViewV1,
            &latticeaxiom_player::PlayerControllerState,
        ),
        (
            With<D2Player>,
            With<LocalPlayerInput>,
            Without<ChunkPresentation>,
        ),
    >,
) {
    let Some((transform, view, controller)) = players.iter().next() else {
        return;
    };
    spine.record_player_pose(ProductionPlayerPose {
        translation: transform.translation,
        yaw_radians: view.yaw_radians(),
        grounded: controller.grounded(),
    });
}

#[allow(clippy::needless_pass_by_value)] // Bevy systems receive SystemParams by value.
#[allow(clippy::type_complexity)] // Local inspect query is one bounded origin.
fn evaluate_target_inspect(
    tick: Res<'_, PlayerFixedTick>,
    spine: Res<'_, ProductionSpine>,
    mut receipts: MessageWriter<'_, TargetInspectReceiptV1>,
    players: Query<
        '_,
        '_,
        (
            &D2Player,
            &CurrentPlayerActionFrame,
            &PlayerMovementProfileV1,
            &PlayerViewV1,
            &Transform,
        ),
        (With<LocalPlayerInput>, Without<ChunkPresentation>),
    >,
) {
    let mut local_players = players.iter();
    let Some((player, frame, profile, view, transform)) = local_players.next() else {
        return;
    };
    if local_players.next().is_some() {
        return;
    }
    if !frame.0.started.contains(PlayerActionV1::Inspect) {
        return;
    }

    let result = spine.inspect(&AuthoritativeTargetInspectRequestV1 {
        player: player.player_id,
        fixed_tick: tick.get(),
        eye_pose: local_eye_pose(transform, profile, *view),
        input_generation: frame.0.generation,
        client_observation: frame.0.client_observation.clone(),
    });
    receipts.write(TargetInspectReceiptV1 {
        player: player.player_id,
        fixed_tick: tick.get(),
        input_generation: frame.0.generation,
        result,
    });
}

#[allow(clippy::needless_pass_by_value)] // Bevy systems receive SystemParams by value.
#[allow(clippy::type_complexity)] // Live crosshair query is one bounded origin.
fn refresh_crosshair_target(
    spine: Res<'_, ProductionSpine>,
    players: Query<
        '_,
        '_,
        (&PlayerMovementProfileV1, &PlayerViewV1, &Transform),
        (
            With<D2Player>,
            With<LocalPlayerInput>,
            Without<ChunkPresentation>,
        ),
    >,
) {
    let Some((profile, view, transform)) = players.iter().next() else {
        return;
    };
    spine.refresh_target(local_eye_pose(transform, profile, *view));
}

#[allow(clippy::needless_pass_by_value)] // Bevy systems receive SystemParams by value.
#[allow(clippy::type_complexity)] // Local pick-block query is one bounded origin.
fn evaluate_pick_block(
    spine: Res<'_, ProductionSpine>,
    pause: Option<Res<'_, ProductionSessionPause>>,
    players: Query<
        '_,
        '_,
        &CurrentPlayerActionFrame,
        (
            With<D2Player>,
            With<LocalPlayerInput>,
            Without<ChunkPresentation>,
        ),
    >,
) {
    if pause.is_some_and(|pause| pause.is_paused()) {
        return;
    }
    let mut local_players = players.iter();
    let Some(frame) = local_players.next() else {
        return;
    };
    if local_players.next().is_some() {
        return;
    }
    if !frame.0.started.contains(PlayerActionV1::PickBlock) {
        return;
    }
    let _ = spine.pick_aimed_block();
}

fn local_eye_pose(
    transform: &Transform,
    profile: &PlayerMovementProfileV1,
    view: PlayerViewV1,
) -> TargetEyePoseV1 {
    let origin = transform.translation
        + bevy::prelude::Vec3::Y
            * (profile.eye_height_m() - profile.capsule_total_height_m() * 0.5);
    let forward = view.forward();
    TargetEyePoseV1 {
        origin_m: origin.to_array(),
        forward: forward.to_array(),
    }
}

/// Failure to construct or run the production playable host.
#[derive(Debug, Error)]
pub enum ProductionHostError {
    /// Bevy instance construction failed.
    #[error(transparent)]
    Engine(#[from] EngineInstanceError),
    /// A lock-selected package artifact failed typed verification or decoding.
    #[error(transparent)]
    LockedArtifact(#[from] crate::LockedPackageArtifactError),
    /// A stable identity could not be parsed.
    #[error(transparent)]
    Identity(#[from] IdentifierError),
    /// A gameplay identifier could not be parsed.
    #[error(transparent)]
    GameplayId(#[from] GameplayIdError),
    /// D4 generation or plan compilation failed.
    #[error(transparent)]
    Worldgen(#[from] WorldgenError),
    /// Bounded worldgen inspect compilation failed.
    #[error(transparent)]
    WorldgenInspect(#[from] WorldgenInspectError),
    /// Authored D9 content catalog compilation failed.
    #[error(transparent)]
    Content(Box<latticeaxiom_content::ContentError>),
    /// The production memory kernel rejected a transaction.
    #[error(transparent)]
    Storage(#[from] StorageError),
    /// Authoritative world-db rejected a read, hydration, or commit.
    #[error(transparent)]
    WorldDb(#[from] WorldDbError),
    /// Sealed writer activation, commit, or close failed.
    #[error(transparent)]
    Writer(#[from] SealedWriterHostError),
    /// Voxel projection or derived dispatch failed.
    #[error(transparent)]
    Runtime(#[from] RuntimeError),
    /// CPU mesh derivation failed.
    #[error(transparent)]
    Mesh(#[from] MeshError),
    /// Shared spine state was poisoned.
    #[error("production spine lock was poisoned")]
    Poisoned,
    /// Storage published no chunk for a generated coordinate.
    #[error("storage snapshot is missing generated chunk {coordinate:?}")]
    MissingStoredChunk {
        /// Missing chunk.
        coordinate: ChunkCoordinate,
    },
    /// A successful bounded generation result omitted its requested chunk.
    #[error("generated region is missing requested chunk {coordinate:?}")]
    MissingGeneratedChunk {
        /// Missing chunk.
        coordinate: ChunkCoordinate,
    },
    /// Voxel payload length did not match the cubic chunk contract.
    #[error("chunk voxel payload length is invalid")]
    PayloadLength,
    /// A D4 draft voxel was outside the padded interior.
    #[error("D4 draft is missing a voxel")]
    DraftVoxelMissing,
    /// A generated block was absent from the D4 catalog palette.
    #[error("generated block is absent from the D4 catalog palette")]
    UnknownDraftBlock,
    /// Derived work was backpressured on the finite V2 region.
    #[error("derived mesh or collider work was backpressured")]
    DerivedBackpressure,
    /// A derived job violated its retained-byte contract.
    #[error("derived mesh or collider work violated its memory contract")]
    DerivedMemory,
    /// Bevy's worker pool was unavailable for frame-independent CPU work.
    #[error("Bevy AsyncComputeTaskPool is unavailable")]
    AsyncComputeTaskPoolUnavailable,
    /// A mesh job lacked a mesh source identity.
    #[error("derived mesh job for {coordinate:?} has no mesh source")]
    MissingMeshSource {
        /// Affected chunk.
        coordinate: ChunkCoordinate,
    },
    /// Derived apply panicked while remaining contained.
    #[error("derived apply panicked")]
    DerivedApplyPanicked,
    /// Derived completion was cancelled or unknown.
    #[error("derived completion was rejected")]
    DerivedRejected,
    /// Padded mesh indexing left the interior.
    #[error("padded voxel index is outside the captured halo")]
    PaddedIndex,
    /// No safe surface column exists in the validated V5 spawn search.
    #[error("no safe spawn column exists in the generated region")]
    NoSafeSpawn,
    /// Generation reused storage evidence on a vacant request.
    #[error("D4 generation reused existing snapshot evidence for a vacant chunk")]
    UnexpectedExistingSnapshot,
    /// Playable host clamps were zero or could not bound the working set.
    #[error("playable host hard limits are invalid")]
    InvalidHostLimits,
    /// The local player pose is outside the canonical chunk domain.
    #[error("player pose is outside the canonical chunk domain")]
    InvalidPlayerPose,
    /// Durable player-session bytes failed schema, decode, or bound checks.
    #[error("durable player session payload is invalid")]
    InvalidPlayerSession,
    /// The gameplay kernel rejected a catalog, inventory, or command.
    #[error(transparent)]
    Gameplay(#[from] GameplayReject),
    /// Authored package JSON could not be decoded.
    #[error("authored catalog `{name}` is invalid JSON: {source}")]
    InvalidAuthoredCatalog {
        /// Catalog artifact name.
        name: &'static str,
        /// Decoder diagnostic.
        source: serde_json::Error,
    },
    /// An authored catalog field was missing or the wrong JSON type.
    #[error("authored catalog field `{field}` is invalid")]
    InvalidCatalogField {
        /// Field name.
        field: &'static str,
    },
    /// A required catalog identity was absent.
    #[error("authored catalog is missing {kind} `{id}`")]
    MissingCatalogDefinition {
        /// Catalog row kind.
        kind: &'static str,
        /// Missing identity or path.
        id: String,
    },
    /// A presentation or device-loss path mutated the materialized-chunk hash.
    #[error("presentation or device failure mutated the authoritative world hash")]
    PresentationFailureMutatedWorld,
    /// The registration image named more than one dimension.
    #[error("registration image names more than one dimension")]
    AmbiguousDimension,
    /// The reopened lock omitted a required exactly-one capability provider.
    #[error("lock graph is missing exactly-one provider for `{capability}`")]
    MissingLockProvider {
        /// Capability identity that must select one package.
        capability: String,
    },
    /// The reopened lock named more than one provider for an exclusive slot.
    #[error("lock graph lists more than one provider for `{capability}`")]
    AmbiguousLockProvider {
        /// Capability identity that must select one package.
        capability: String,
    },
    /// A compiled V5 plan omitted a required natural-layer sample.
    #[error("compiled V5 plan is missing a `{kind}` sample")]
    MissingNaturalSample {
        /// Missing sample kind.
        kind: &'static str,
    },
    /// Compiled occupancy palettes rejected an authored content row.
    #[error("authored content catalog is invalid: {reason}")]
    InvalidContentCatalog {
        /// Compiler diagnostic.
        reason: String,
    },
    /// Locked terrain layer-table compilation failed.
    #[error("terrain layer table failed to compile: {reason}")]
    TerrainLayerTable {
        /// Compiler diagnostic.
        reason: String,
    },
    /// A bounded derived readiness barrier was exhausted before the required
    /// mesh or collider jobs were applied.
    #[error("bounded derived readiness barrier was exhausted")]
    DerivedReadinessBarrier,
    /// More than one built-in terrain profile was persisted for one world.
    #[error("world metadata selects more than one terrain profile")]
    AmbiguousTerrainProfile,
    /// Persisted terrain profile digest disagrees with its resolved V2 config.
    #[error("terrain profile `{profile}` has a mismatched resolved-config digest")]
    TerrainProfileDigestMismatch {
        /// Profile whose resolved configuration no longer matches metadata.
        profile: StableId,
    },
}

impl From<latticeaxiom_content::ContentError> for ProductionHostError {
    fn from(error: latticeaxiom_content::ContentError) -> Self {
        Self::Content(Box::new(error))
    }
}

impl fmt::Debug for ProductionSpine {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ProductionSpine")
            .field("world", &self.world_id())
            .field("chunk_edge", &self.chunk_edge())
            .finish_non_exhaustive()
    }
}
