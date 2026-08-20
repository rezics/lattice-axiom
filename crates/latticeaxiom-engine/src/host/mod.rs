//! Production playable host spine for the V2 package-driven slice.
//!
//! This module is the production Bevy world session. It starts from a
//! reopened [`LockVerifiedComposeImages`], stores voxels through
//! [`MemoryTransactionKernel`], and presents one Avian collider plus CPU mesh
//! per chunk. Chunk interest streams around the local player. It does not open
//! a durable writer and must not be confused with the `playable` development
//! fixture.

#[cfg(feature = "client")]
mod client;
mod gameplay;
#[cfg(feature = "client")]
mod hud;
mod spine;
mod start;
mod stream;
mod worldgen;

use std::fmt;

use avian3d::PhysicsPlugins;
use bevy::{
    app::{App, Plugin},
    ecs::schedule::IntoScheduleConfigs,
    prelude::{
        Commands, Component, Entity, FixedPostUpdate, MessageWriter, Query, Res, ResMut, Resource,
        Transform, With, Without,
    },
    transform::TransformPlugin,
};
#[cfg(feature = "client")]
use bevy::{
    app::{Startup, Update},
    prelude::{ClearColor, Color},
};
use latticeaxiom_compose::LockedGameGraph;
use latticeaxiom_core::IdentifierError;
use latticeaxiom_gameplay::{GameplayIdError, GameplayReject};
use latticeaxiom_player::{
    ActionFrameInbox, ActionFrameInboxError, AuthoritativeTargetInspectRequestV1,
    BlockEditAuthorityResource, CurrentPlayerActionFrame, D2Player, D2PlayerBundle,
    LocalPlayerInput, PlayerActionV1, PlayerFixedTick, PlayerMovementProfileV1, PlayerPlugin,
    PlayerSystemSet, PlayerViewV1, TargetEyePoseV1, TargetInspectReceiptV1,
};
#[cfg(feature = "client")]
use latticeaxiom_player::{LeafwingInputAdapterPlugin, LocalPlayerClientInputBundle};
use latticeaxiom_registration::CompiledRegistration;
use latticeaxiom_storage::{ChunkCoordinate, ChunkRevision, StorageError};
use latticeaxiom_voxel_mesh::{MeshError, MeshReceipt};
use latticeaxiom_voxel_runtime::RuntimeError;
use latticeaxiom_worldgen::WorldgenError;
use thiserror::Error;

pub use gameplay::{HOTBAR_SLOTS, INVENTORY_SLOTS, ProductionInventoryView};
pub use spine::{ProductionSpine, ProductionWorldStorage, WorkingSetDiagnosticsV1};
pub use start::{ProductionMemoryStart, ProductionMemoryStartError, ProductionWorldList};
pub use stream::ChunkLifecycle;

#[cfg(feature = "client")]
use crate::EngineProfile;
use crate::{
    EngineInstance, EngineInstanceError, LockVerifiedComposeImages, VerifiedProductLockHash,
};

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
/// When the lock image does not select `@latticeaxiom/inspect` and
/// `@latticeaxiom/observability`, the host uses
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
        Self {
            graph_selected_inspect: package_present(graph, "@latticeaxiom/inspect")
                || capability_present(graph, "latticeaxiom:capability/target-inspect-surface@1")
                || !registration.image.observability.inspect.is_empty(),
            graph_selected_observability: package_present(graph, "@latticeaxiom/observability")
                || capability_present(graph, "latticeaxiom:capability/diagnostic-registry@1"),
        }
    }

    /// Returns whether the lock graph selected the inspect package or catalog.
    #[must_use]
    pub const fn graph_selected_inspect(self) -> bool {
        self.graph_selected_inspect
    }

    /// Returns whether the lock graph selected the observability package.
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

fn package_present(graph: &LockedGameGraph, name: &str) -> bool {
    graph
        .packages
        .keys()
        .any(|package| package.as_str() == name)
}

fn capability_present(graph: &LockedGameGraph, capability: &str) -> bool {
    graph
        .capability_providers
        .keys()
        .any(|id| id.as_str() == capability)
}

/// Bevy plugin that spawns chunk colliders and the local player.
#[derive(Clone, Copy, Debug, Default)]
pub struct ProductionHostPlugin;

impl Plugin for ProductionHostPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<TargetInspectReceiptV1>().add_systems(
            FixedPostUpdate,
            (
                evaluate_target_inspect.after(PlayerSystemSet::EvaluateEdit),
                sync_player_pose.after(PlayerSystemSet::EvaluateEdit),
                sync_chunk_stream.after(sync_player_pose),
                sync_chunk_colliders.after(sync_chunk_stream),
                sync_working_set_diagnostics.after(sync_chunk_stream),
                refresh_crosshair_target
                    .after(sync_player_pose)
                    .after(evaluate_target_inspect),
            ),
        );
        #[cfg(feature = "client")]
        app.add_systems(
            Startup,
            (
                spawn_production_hud_if_client,
                client::spawn_production_client_view,
            ),
        )
        .add_systems(
            Update,
            (client::sync_production_camera, client::exit_on_pause),
        )
        .add_systems(
            FixedPostUpdate,
            (
                hud::sync_production_inspect_hud.after(refresh_crosshair_target),
                hud::sync_production_working_set_hud.after(sync_working_set_diagnostics),
                hud::sync_production_hotbar_hud.after(refresh_crosshair_target),
            ),
        );
    }

    fn finish(&self, app: &mut App) {
        spawn_host_entities(app.world_mut());
    }
}

impl EngineInstance {
    /// Builds a GPU-free production spine from a reopened final product lock.
    ///
    /// The host inserts [`MemoryTransactionKernel`] as the production storage
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
        let spine = ProductionSpine::materialize(&images)?;
        Self::new_headless_with_setup(images.into_images(), fixed_timestep, move |app| {
            install_production_host(app, product_lock_hash, spine, inspect_surface, true);
        })
        .map_err(ProductionHostError::from)
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
        let spine = ProductionSpine::materialize_with_catalog(&images, catalog)?;
        Self::new_headless_with_setup(images.into_images(), fixed_timestep, move |app| {
            install_production_host(app, product_lock_hash, spine, inspect_surface, true);
        })
        .map_err(ProductionHostError::from)
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
        let product_lock_hash = VerifiedProductLockHash::new(images.product_lock_hash());
        let inspect_surface = ProductionInspectSurface::from_lock_images(&images);
        let spine = ProductionSpine::materialize(&images)?;
        let instance = Self::new_client_with_setup(images.into_images(), move |app| {
            install_production_host(app, product_lock_hash, spine, inspect_surface, false);
        })?;
        Ok((instance, lease.into_app_created_proof()))
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

fn install_production_host(
    app: &mut App,
    product_lock_hash: VerifiedProductLockHash,
    spine: ProductionSpine,
    inspect_surface: ProductionInspectSurface,
    include_transform: bool,
) {
    if include_transform {
        app.add_plugins(TransformPlugin);
    }
    let working_set = spine.working_set_diagnostics();
    app.insert_resource(product_lock_hash)
        .insert_resource(inspect_surface)
        .insert_resource(spine.storage())
        .insert_resource(BlockEditAuthorityResource::new(spine.clone()))
        .insert_resource(working_set)
        .insert_resource(spine)
        .add_plugins(PhysicsPlugins::default())
        .add_plugins(PlayerPlugin)
        .add_plugins(ProductionHostPlugin);
    #[cfg(feature = "client")]
    if !include_transform {
        app.insert_resource(ClearColor(Color::srgb(0.48, 0.70, 0.91)))
            .add_plugins(LeafwingInputAdapterPlugin);
    }
}

fn spawn_host_entities(world: &mut bevy::prelude::World) {
    let Some(spine) = world.get_resource::<ProductionSpine>().cloned() else {
        return;
    };
    let spawn = spine.spawn_center();
    #[cfg(feature = "client")]
    {
        let client = world.get_resource::<EngineProfile>() == Some(&EngineProfile::Client);
        let mut player = world.spawn(D2PlayerBundle::new(
            spine::local_player_id(),
            Transform::from_translation(spawn),
        ));
        if client {
            player.insert(LocalPlayerClientInputBundle::default());
        }
    }
    #[cfg(not(feature = "client"))]
    world.spawn(D2PlayerBundle::new(
        spine::local_player_id(),
        Transform::from_translation(spawn),
    ));
    if let Ok(delta) = spine.take_presentation() {
        for (coordinate, collider, origin) in delta.upserts {
            world.spawn((
                ChunkPresentation { coordinate },
                avian3d::prelude::RigidBody::Static,
                collider,
                Transform::from_translation(origin),
            ));
        }
    }
}

#[allow(clippy::needless_pass_by_value)] // Bevy systems receive SystemParams by value.
fn sync_chunk_stream(tick: Res<'_, PlayerFixedTick>, spine: Res<'_, ProductionSpine>) {
    let _ = spine.sync_interest(tick.get());
}

#[allow(clippy::needless_pass_by_value)] // Bevy systems receive SystemParams by value.
fn sync_working_set_diagnostics(
    spine: Res<'_, ProductionSpine>,
    mut snapshot: ResMut<'_, WorkingSetDiagnosticsV1>,
) {
    *snapshot = spine.working_set_diagnostics();
}

#[cfg(feature = "client")]
#[allow(clippy::needless_pass_by_value)] // Bevy systems receive SystemParams by value.
fn spawn_production_hud_if_client(commands: Commands<'_, '_>, profile: Res<'_, EngineProfile>) {
    if *profile == EngineProfile::Client {
        hud::spawn_production_hud(commands);
    }
}

#[allow(clippy::needless_pass_by_value)] // Bevy systems receive SystemParams by value.
#[allow(clippy::type_complexity)] // Chunk collider query is one presentation mapping.
fn sync_chunk_colliders(
    mut commands: Commands<'_, '_>,
    spine: Res<'_, ProductionSpine>,
    mut chunks: Query<
        '_,
        '_,
        (
            Entity,
            &ChunkPresentation,
            &mut avian3d::prelude::Collider,
            &mut Transform,
        ),
    >,
) {
    let Ok(delta) = spine.take_presentation() else {
        return;
    };
    for coordinate in delta.removals {
        if let Some((entity, _, _, _)) = chunks
            .iter()
            .find(|(_, presentation, _, _)| presentation.coordinate == coordinate)
        {
            commands.entity(entity).despawn();
        }
    }
    for (coordinate, collider, origin) in delta.upserts {
        if let Some((_, _, mut existing, mut transform)) = chunks
            .iter_mut()
            .find(|(_, presentation, _, _)| presentation.coordinate == coordinate)
        {
            *existing = collider;
            transform.translation = origin;
            continue;
        }
        commands.spawn((
            ChunkPresentation { coordinate },
            avian3d::prelude::RigidBody::Static,
            collider,
            Transform::from_translation(origin),
        ));
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
    /// A stable identity could not be parsed.
    #[error(transparent)]
    Identity(#[from] IdentifierError),
    /// A gameplay identifier could not be parsed.
    #[error(transparent)]
    GameplayId(#[from] GameplayIdError),
    /// D4 generation or plan compilation failed.
    #[error(transparent)]
    Worldgen(#[from] WorldgenError),
    /// The production memory kernel rejected a transaction.
    #[error(transparent)]
    Storage(#[from] StorageError),
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
    /// No safe surface column exists in the generated spawn neighborhood.
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
    /// The gameplay kernel rejected a catalog, inventory, or command.
    #[error(transparent)]
    Gameplay(#[from] GameplayReject),
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
