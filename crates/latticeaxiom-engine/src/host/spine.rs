//! Package-driven playable spine: storage, projection, meshing, and edits.

use std::{
    cell::RefCell,
    collections::{BTreeMap, BTreeSet, VecDeque},
    mem,
    num::NonZeroU16,
    sync::{Arc, Mutex},
    time::Instant,
};

use avian3d::prelude::Collider;
use bevy::prelude::{Quat, Resource, Vec3};
use bevy::tasks::{AsyncComputeTaskPool, Task, block_on, tick_global_task_pools_on_main_thread};
use latticeaxiom_compose::PlayableWorldHardLimitsV1;
use latticeaxiom_content::{
    CompiledFluidPaletteV1, CompiledSolidPaletteEntryV1, CompiledSolidPaletteV1, ContentCatalogV1,
    FluidFlowV1, FluidLevelV1, FluidOccupancyKindV1, FluidOccupancyPolicyV1, FluidPaletteEntryV1,
    FluidStateV1, OccupancyArbitrationContextV1, PaletteLimitsV1, SolidFluidArbitrationV1,
    SolidPaletteEntryV1, arbitrate_cell,
};
use latticeaxiom_core::{SchemaId, StableId, WorldId};
use latticeaxiom_gameplay::{
    AuthorityTick, BlockId, BlockPosition, CommandOutcomeV1, ContainerId, ContainerStateV1,
    DimensionChunkKey, DropEntityId, GameplayCatalog, GameplayReject, InventoryInspectV1,
    ItemStackV1, PlayerId, ProcessId, RecipeId, RecipeInspectV1, SlotIndex, TransferCommandV1,
    WorkstationId,
};
use latticeaxiom_player::{
    AuthoritativeBlockEditRequestV1, AuthoritativeTargetInspectRequestV1, BlockEditActionV1,
    BlockEditAuthority, BlockEditRejectV1, BlockEditSuccessV1, BlockFaceV1,
    ClientTargetObservationV1, HeadlessTargetInspectV1, MAX_BLOCK_EDIT_REACH_M,
    PlayerMovementProfileV1, TargetEyePoseV1, TargetInspectRejectV1, occupancy_line,
};
use latticeaxiom_runtime_contracts::{
    EngineEpoch, WorldEpoch as InspectWorldEpoch, WorldgenInspectReportV1,
};
use latticeaxiom_storage::{
    AuthoritativeTransactionKernel, ChangedDomains, ChunkCoordinate, ChunkData, ChunkKey,
    ChunkMutation, ChunkRevision, ChunkRevisionExpectation, ContinuationId, DimensionId,
    MaterializedChunkStateHash, MemoryTransactionKernel, PayloadSchemaVersion, PersistentEntityId,
    StoredChunk, TransactionId, VersionedPayload, WorldRevision, WorldTransaction,
};
use latticeaxiom_voxel_mesh::{
    Aabb, Face, FaceDescriptor, GreedyMesher, LayerMergeKey, MeshBuffer, MeshReceipt, MeshSource,
    PaddedChunk, Voxel,
};
use latticeaxiom_voxel_runtime::{
    ApplyAdmission, ApplyByteDeclaration, CellSelection, ColliderSafetyState,
    ColliderSemanticFingerprint, CollisionSemantics, CommittedChunkProjection, CompletionOutcome,
    DdaOrigin, DdaOutcome, DdaQuery, DerivedApplyBudget, DerivedApplySlice, DerivedInput,
    DerivedKind, DerivedMemoryBudget, DerivedOwner, DerivedPriority, DerivedQueueLimits,
    DerivedRequest, DerivedRequestSet, DispatchOutcome, EvictionLeaseGeneration, ExecutorFinish,
    ExecutorOutcome, FixedTick, MeshSemanticFingerprint, RetainedBytes, RuntimeDiagnostics,
    RuntimeGeneration, RuntimeLimits, VoxelCoordinate, VoxelRuntime, WallClockNanos,
    WorkerAbortOutcome, WorkingSetScope, WorldEpoch, cpu_heavy_concurrency, host_parallelism,
};
use latticeaxiom_world_db::{
    AuthoritativeMetadataInputV1, CommitDurabilityV1, DeterministicWorldStorage, PersistedChunkV1,
    StorageDurabilityCapabilityV1, WorldCommitOutcomeV1, WorldCommitRequestV1, WorldStorage,
};
use latticeaxiom_worldgen::{
    AuthoredWorldgenBindingsV1, BoundedGeneratedRegionV1, CaveOccupancyArbitrationV1,
    GenerationPlanV1, HydrologyFlowV1, MAX_BOUNDED_REGION_CHUNKS, SpawnLocationV1,
};

use super::{
    ChunkLifecycle, ChunkMeshCursor, ProductionHostError, ProductionPlayerPose,
    SealedWorldWriterHost,
    catalog::{
        host_worldgen_catalog, lock_selected_content_catalog, lock_selected_gameplay_catalog,
    },
    display::{
        ContentDisplayCatalogV1, ContentDisplayLabelV1, lock_selected_content_display_catalog,
    },
    fluid::{HostFluidTickV1, tick_simulated as tick_simulated_fluids},
    gameplay::{ProductionGameplay, ProductionInventoryView, block_edit_reject, remaining_work},
    layers::{HostFaceStyle, HostPresentationIndex},
    profile::StreamingProfileEvidenceV1,
    session::{DurablePlayerSessionV1, PLAYER_SESSION_ENTITY},
    stream::{
        InterestClass, LOOK_AHEAD_EXPIRY_TICKS, StreamClamps, ViewDistanceStatusV1, chebyshev_xz,
        desired_chunks, interest_class, prioritize_chunks, retain_protected, sticky_look_ahead,
    },
    worldgen::{
        RequiredCaveEntranceV1, compile_host_worldgen_inspect, compile_plan, generate_plan_chunks,
        host_hard_limits, occupancy_candidate_is_current, required_cave_entrance,
        spawn_center as player_spawn_center, spine_config, validated_spawn,
    },
};
use crate::LockVerifiedComposeImages;

const MESH_SEMANTICS: MeshSemanticFingerprint = MeshSemanticFingerprint::new([0x51; 32]);
const COLLIDER_SEMANTICS: ColliderSemanticFingerprint =
    ColliderSemanticFingerprint::new([0xC1; 32]);
const VOXEL_SCHEMA: &str = "latticeaxiom:schema/chunk-voxels@1";
const VOXEL_SCHEMA_VERSION: u32 = 2;
const CELL_OCCUPANCY_BYTES: usize = 4;
/// One generated chunk is published per fixed slice so commits and projection stay bounded.
const WORLDGEN_APPLY_JOB_CAP: usize = 1;
const WORLD_ID: &str = "00000000-0000-4000-8000-0000000000b1";
const REACH_MM: u16 = 5_000;
/// Portal SDF is doubled-voxel Chebyshev minus radius and is biased one unit
/// outside the aperture voxel. Hydrology must not occupy that entrance
/// neighborhood; cave-interior aquifers farther from portals remain.
const HYDROLOGY_PORTAL_EXCLUSION_SDF: i32 = 8;

/// Production [`MemoryTransactionKernel`] installed behind the storage trait.
#[derive(Clone, Debug, Resource)]
pub struct ProductionWorldStorage {
    kernel: Arc<MemoryTransactionKernel>,
}

impl ProductionWorldStorage {
    /// Returns the non-durable production kernel.
    #[must_use]
    pub fn kernel(&self) -> &MemoryTransactionKernel {
        &self.kernel
    }
}

/// Shared V2 playable-spine state consumed by authority and presentation.
#[derive(Clone, Resource)]
pub struct ProductionSpine {
    inner: Arc<Mutex<ProductionSpineInner>>,
    storage: ProductionWorldStorage,
}

/// Occupancy snapshot copied from [`VoxelRuntime`] diagnostics.
///
/// Counts are working-set occupancy, not frame-time budgets. `saving` stays
/// zero because this host does not open a world writer.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Resource)]
pub struct WorkingSetDiagnosticsV1 {
    resident: u32,
    active: u32,
    visible: u32,
    in_flight: u32,
    dirty: u32,
    saving: u32,
    reserved_bytes: u64,
    byte_budget: u64,
}

impl WorkingSetDiagnosticsV1 {
    /// Copies occupancy counters from a runtime diagnostics snapshot.
    #[must_use]
    pub fn from_runtime(diagnostics: RuntimeDiagnostics) -> Self {
        Self {
            resident: count_u32(diagnostics.resident_chunks()),
            active: count_u32(diagnostics.active_chunks()),
            visible: count_u32(diagnostics.visible_chunks()),
            in_flight: count_u32(diagnostics.in_flight_chunks()),
            dirty: count_u32(diagnostics.dirty_chunks()),
            saving: count_u32(diagnostics.saving_chunks()),
            reserved_bytes: diagnostics.combined_reserved_bytes(),
            byte_budget: diagnostics.byte_budget(),
        }
    }

    /// Resident committed projections.
    #[must_use]
    pub const fn resident(self) -> u32 {
        self.resident
    }

    /// Projections with both mesh and collider last-applied keys.
    #[must_use]
    pub const fn active(self) -> u32 {
        self.active
    }

    /// Projections with a last-applied mesh key.
    #[must_use]
    pub const fn visible(self) -> u32 {
        self.visible
    }

    /// Combined mesh and collider jobs currently in flight.
    #[must_use]
    pub const fn in_flight(self) -> u32 {
        self.in_flight
    }

    /// Resident projections pinned because they were edited.
    #[must_use]
    pub const fn dirty(self) -> u32 {
        self.dirty
    }

    /// Chunks currently being written to durable storage.
    ///
    /// Always zero on this host: no world writer is opened.
    #[must_use]
    pub const fn saving(self) -> u32 {
        self.saving
    }

    /// Combined mesh and collider reservations currently held.
    #[must_use]
    pub const fn reserved_bytes(self) -> u64 {
        self.reserved_bytes
    }

    /// Cross-kind combined reservation hard limit.
    #[must_use]
    pub const fn byte_budget(self) -> u64 {
        self.byte_budget
    }

    /// Compact one-line overlay for the production HUD.
    #[must_use]
    pub fn overlay_line(self) -> String {
        format!(
            "r{} a{} v{} i{} d{} s{} {}/{}",
            self.resident,
            self.active,
            self.visible,
            self.in_flight,
            self.dirty,
            self.saving,
            self.reserved_bytes,
            self.byte_budget
        )
    }

    /// Occupancy fragment used by the F3 inspect overlay.
    #[must_use]
    pub fn inspect_occupancy_line(self) -> String {
        occupancy_line(self.resident, self.active, self.in_flight, self.dirty)
    }
}

fn count_u32(value: usize) -> u32 {
    u32::try_from(value).unwrap_or(u32::MAX)
}

/// Derived-queue occupancy used by V4 backpressure and cancellation tests.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct DerivedQueueSnapshotV1 {
    /// Pending mesh jobs.
    pub mesh_pending: usize,
    /// In-flight mesh jobs.
    pub mesh_in_flight: usize,
    /// Pending collider jobs.
    pub collider_pending: usize,
    /// In-flight collider jobs.
    pub collider_in_flight: usize,
    /// Combined cancellation requests.
    pub cancel_requests: u64,
    /// Receipt-checked results waiting for host apply.
    pub waiting_to_apply: usize,
    /// Combined reserved derived bytes.
    pub reserved_bytes: u64,
    /// Apply slices stopped by the job cap.
    pub apply_stopped_jobs: u64,
    /// Apply slices stopped by the byte cap.
    pub apply_stopped_bytes: u64,
    /// Apply slices stopped by the wall-clock cap.
    pub apply_stopped_wall_clock: u64,
    /// Host-owned Bevy tasks not yet polled to completion.
    pub spawned_tasks: usize,
}

/// Host-owned world-generation work waiting across the Bevy task boundary.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct WorldgenQueueSnapshotV1 {
    /// Inputs admitted but not yet spawned on the Bevy compute pool.
    pub pending: usize,
    /// Bevy compute tasks not yet polled to completion.
    pub in_flight: usize,
    /// Completed results waiting for stable sequence-order publication.
    pub waiting_to_apply: usize,
}

/// Versioned solid and orthogonal fluid occupancy of one committed cell.
///
/// Collision and selection identities are catalog-owned policy references. This
/// host does not apply them to physics or DDA until a consumer is ready.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CellOccupancyV1 {
    /// Inspected voxel in world cells.
    pub position: BlockPosition,
    /// Solid palette identity, including empty air.
    pub solid: Option<BlockId>,
    /// Versioned solid-occupancy policy from the solid's default state.
    pub solid_occupancy: StableId,
    /// Versioned orthogonal fluid-occupancy policy from the solid's default state.
    pub fluid_occupancy: StableId,
    /// Fluid palette identity when the cell is not canonically empty.
    pub fluid: Option<StableId>,
    /// Authoritative per-cell fluid state when a fluid occupies the cell.
    pub fluid_state: Option<FluidStateV1>,
    /// Versioned collision policy; unused by this host's physics.
    pub collision_policy: StableId,
    /// Versioned selection policy; unused by this host's DDA.
    pub selection_policy: StableId,
}

pub(super) struct ProductionSpineInner {
    pub(super) runtime: VoxelRuntime<HostVoxel>,
    plan: Arc<GenerationPlanV1>,
    worldgen_materialization: Arc<WorldgenMaterialization>,
    worldgen_bindings: AuthoredWorldgenBindingsV1,
    spawn: SpawnLocationV1,
    clamps: StreamClamps,
    pub(super) content: Arc<ContentCatalogV1>,
    pub(super) palette: Arc<[BlockId]>,
    solid_palette: CompiledSolidPaletteV1,
    pub(super) fluid_palette: Arc<CompiledFluidPaletteV1>,
    empty: HostVoxel,
    pub(super) chunk_edge: u16,
    world: WorldId,
    dimension: DimensionId,
    next_transaction: u128,
    voxel_schema: SchemaId,
    voxel_schema_version: PayloadSchemaVersion,
    derived: BTreeMap<ChunkCoordinate, ChunkDerived>,
    mesh_dirty: BTreeSet<ChunkCoordinate>,
    collider_dirty: BTreeSet<ChunkCoordinate>,
    removed: BTreeSet<ChunkCoordinate>,
    waiting_derived: VecDeque<ComputedDerived>,
    in_flight_tasks: Vec<Task<ComputedDerived>>,
    pending_worldgen: VecDeque<WorldgenInput>,
    in_flight_worldgen_tasks: Vec<Task<ComputedWorldgen>>,
    waiting_worldgen: BTreeMap<WorldgenTicket, ComputedWorldgen>,
    worldgen_tickets: BTreeMap<ChunkCoordinate, WorldgenTicket>,
    next_worldgen_sequence: u64,
    next_worldgen_apply_sequence: u64,
    pub(super) presentation: Arc<HostPresentationIndex>,
    edited: BTreeSet<ChunkCoordinate>,
    lifecycle: BTreeMap<ChunkCoordinate, ChunkLifecycle>,
    eviction_lease: u64,
    residency: BTreeMap<ChunkCoordinate, ChunkResidency>,
    stream_anchor_xz: [f32; 2],
    look_ahead_axis: [i32; 2],
    look_ahead_tick: u64,
    last_reconciled_tick: Option<u64>,
    last_desired: BTreeSet<ChunkCoordinate>,
    stream_admissions: u64,
    stream_evictions: u64,
    spawn_center: Vec3,
    placement_content: BlockId,
    last_success: Option<BlockEditSuccessV1>,
    last_reject: Option<BlockEditRejectV1>,
    current_target: Option<HeadlessTargetInspectV1>,
    last_inspect: Option<Result<HeadlessTargetInspectV1, TargetInspectRejectV1>>,
    player_pose: ProductionPlayerPose,
    last_stream_error: Option<String>,
    gameplay: Option<ProductionGameplay>,
    world_store: Option<DeterministicWorldStorage>,
    display: ContentDisplayCatalogV1,
    /// Full `sync_interest` calls; schedule tests lock one per fixed tick.
    interest_reconciliations: u64,
}

/// Admission and core clocks used by retain/grace eviction.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ChunkResidency {
    admitted_tick: u64,
    last_core_tick: Option<u64>,
}

/// Mesh, collider, and eviction events consumed by production presentation.
#[derive(Debug)]
pub(super) struct PresentationDelta {
    pub(super) mesh_update: Vec<MeshPresentation>,
    pub(super) collider_update: Vec<ColliderPresentation>,
    pub(super) removals: Vec<ChunkCoordinate>,
}

/// Accepted halo-aware geometry ready to replace a chunk GPU mesh.
#[derive(Clone, Debug)]
#[cfg_attr(not(feature = "client"), allow(dead_code))]
pub(super) struct MeshPresentation {
    pub(super) coordinate: ChunkCoordinate,
    pub(super) origin: Vec3,
    pub(super) geometry: Arc<MeshBuffer<LayerMergeKey>>,
    pub(super) bounds: Option<Aabb>,
}

/// Accepted collider ready to replace a chunk physics shape.
#[derive(Clone, Debug)]
pub(super) struct ColliderPresentation {
    pub(super) coordinate: ChunkCoordinate,
    pub(super) origin: Vec3,
    pub(super) collider: Collider,
}

/// Occupied interior cell used only to build a chunk collider.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct OccupiedCell {
    pub(super) local: [u16; 3],
    pub(super) palette_index: u16,
}

#[derive(Clone, Debug)]
struct ChunkDerived {
    mesh_receipt: Option<MeshReceipt>,
    mesh_source: Option<MeshSource>,
    geometry: Option<Arc<MeshBuffer<LayerMergeKey>>>,
    bounds: Option<Aabb>,
    collider: Option<Collider>,
    occupied: Vec<OccupiedCell>,
    revision: ChunkRevision,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) struct HostVoxel {
    pub(super) palette_index: u16,
    pub(super) fluid_palette_index: u16,
    collision_occupied: bool,
    solid_style: Option<HostFaceStyle>,
    fluid_style: Option<HostFaceStyle>,
}

impl HostVoxel {
    pub(super) fn occupancy(
        palette_index: u16,
        fluid_palette_index: u16,
        presentation: &HostPresentationIndex,
    ) -> Self {
        Self {
            palette_index,
            fluid_palette_index,
            collision_occupied: presentation.solid_collision(palette_index),
            solid_style: (palette_index != 0)
                .then(|| presentation.solid(palette_index))
                .flatten(),
            fluid_style: (fluid_palette_index != 0)
                .then(|| presentation.fluid(fluid_palette_index))
                .flatten(),
        }
    }

    fn from_solid(palette_index: u16, presentation: &HostPresentationIndex) -> Self {
        Self::occupancy(palette_index, 0, presentation)
    }
}

#[derive(Clone, Debug)]
struct HostDerivedMesh {
    receipt: MeshReceipt,
    source: MeshSource,
    geometry: MeshBuffer<LayerMergeKey>,
}

#[derive(Clone, Debug)]
struct HostDerivedCollider {
    occupied: Vec<OccupiedCell>,
}

#[derive(Debug)]
#[allow(
    clippy::large_enum_variant,
    reason = "boxing mesh geometry adds a hot-path allocation"
)]
enum DerivedPayload {
    Mesh(HostDerivedMesh),
    Collider(HostDerivedCollider),
}

impl RetainedBytes for DerivedPayload {
    fn retained_bytes(&self) -> u64 {
        match self {
            Self::Mesh(value) => value.retained_bytes(),
            Self::Collider(value) => value.retained_bytes(),
        }
    }
}

#[derive(Debug)]
struct ComputedDerived {
    input: DerivedInput<HostVoxel>,
    payload: Result<DerivedPayload, ProductionHostError>,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct WorldgenTicket(u64);

#[derive(Clone)]
struct WorldgenInput {
    ticket: WorldgenTicket,
    coordinate: ChunkCoordinate,
    materialization: Arc<WorldgenMaterialization>,
}

struct ComputedWorldgen {
    ticket: WorldgenTicket,
    coordinate: ChunkCoordinate,
    result: Result<Vec<HostVoxel>, ProductionHostError>,
}

struct WorldgenMaterialization {
    plan: Arc<GenerationPlanV1>,
    content: Arc<ContentCatalogV1>,
    palette: Arc<[BlockId]>,
    fluid_palette: Arc<CompiledFluidPaletteV1>,
    presentation: Arc<HostPresentationIndex>,
    spawn: SpawnLocationV1,
    empty: HostVoxel,
    chunk_edge: u16,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum WorldgenExecution {
    Blocking,
    Deferred,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct WorldgenAdmission {
    limit: usize,
    execution: WorldgenExecution,
}

thread_local! {
    static MESH_LANE: RefCell<GreedyMesher<LayerMergeKey>> =
        const { RefCell::new(GreedyMesher::new()) };
}

impl ProductionSpine {
    /// Materializes validated-spawn interest into memory storage and projection.
    ///
    /// The host does not pre-generate a large finite map. Chunks stream from the
    /// compiled V5 plan around player interest. This path does not open a
    /// durable writer.
    ///
    /// # Errors
    ///
    /// Returns [`ProductionHostError`] when lock-bound generation, storage
    /// publication, voxel projection, or derived meshing fails.
    pub fn materialize(images: &LockVerifiedComposeImages) -> Result<Self, ProductionHostError> {
        Self::materialize_world(images, WORLD_ID.parse()?)
    }

    /// Materializes the production spine with a caller-supplied gameplay catalog.
    ///
    /// # Errors
    ///
    /// Returns [`ProductionHostError`] when materialization or catalog binding fails.
    pub fn materialize_with_catalog(
        images: &LockVerifiedComposeImages,
        catalog: GameplayCatalog,
    ) -> Result<Self, ProductionHostError> {
        Self::materialize_world_with_catalog(images, WORLD_ID.parse()?, catalog)
    }

    /// Materializes one process-local world identity into the memory kernel.
    ///
    /// The host does not pre-generate a large finite map. Chunks stream from the
    /// compiled V5 plan around player interest. This path does not open a
    /// durable writer.
    ///
    /// # Errors
    ///
    /// Returns [`ProductionHostError`] when lock-bound generation, storage
    /// publication, voxel projection, or derived meshing fails.
    pub fn materialize_world(
        images: &LockVerifiedComposeImages,
        world: WorldId,
    ) -> Result<Self, ProductionHostError> {
        Self::materialize_world_with_catalog(images, world, lock_selected_gameplay_catalog(images)?)
    }

    /// Materializes one process-local world with a caller-supplied catalog.
    ///
    /// # Errors
    ///
    /// Returns [`ProductionHostError`] when lock-bound generation, storage
    /// publication, voxel projection, derived meshing, or gameplay binding fails.
    #[allow(clippy::too_many_lines)]
    pub fn materialize_world_with_catalog(
        images: &LockVerifiedComposeImages,
        world: WorldId,
        catalog: GameplayCatalog,
    ) -> Result<Self, ProductionHostError> {
        Self::materialize_world_with_catalog_and_store(images, world, catalog, None)
    }

    /// Materializes one world, hydrating the memory kernel from world-db first.
    ///
    /// [`MemoryTransactionKernel`] remains the working-set cache. Chunks already
    /// committed to [`DeterministicWorldStorage`] are loaded before D4 generation.
    ///
    /// # Errors
    ///
    /// Returns [`ProductionHostError`] when lock-bound generation, storage
    /// publication, world-db hydration, voxel projection, derived meshing, or
    /// gameplay binding fails.
    pub fn materialize_world_from_storage(
        images: &LockVerifiedComposeImages,
        world: WorldId,
        storage: DeterministicWorldStorage,
    ) -> Result<Self, ProductionHostError> {
        Self::materialize_world_with_catalog_from_storage(
            images,
            world,
            lock_selected_gameplay_catalog(images)?,
            storage,
        )
    }

    /// Materializes one world from world-db with a caller-supplied catalog.
    ///
    /// # Errors
    ///
    /// Returns [`ProductionHostError`] when materialization or catalog binding fails.
    pub fn materialize_world_with_catalog_from_storage(
        images: &LockVerifiedComposeImages,
        world: WorldId,
        catalog: GameplayCatalog,
        storage: DeterministicWorldStorage,
    ) -> Result<Self, ProductionHostError> {
        Self::materialize_world_with_catalog_and_store(images, world, catalog, Some(storage))
    }

    #[allow(clippy::too_many_lines)]
    fn materialize_world_with_catalog_and_store(
        images: &LockVerifiedComposeImages,
        world: WorldId,
        catalog: GameplayCatalog,
        world_store: Option<DeterministicWorldStorage>,
    ) -> Result<Self, ProductionHostError> {
        let config = spine_config();
        let chunk_edge = config.chunk_edge_voxels;
        let worldgen = host_worldgen_catalog(images)?;
        let plan = Arc::new(compile_plan(
            images.product_lock_hash(),
            images.images().registration().image.image_hash,
            &worldgen,
        )?);
        let dimension = worldgen.dimension.clone();
        let kernel = Arc::new(MemoryTransactionKernel::new());
        let content = Arc::new(lock_selected_content_catalog(images)?);
        let display = lock_selected_content_display_catalog(images)?;
        let palette = Arc::<[BlockId]>::from(worldgen.palette.clone());
        let solid_palette = compile_host_solid_palette(&content, &palette)?;
        let fluid_palette = Arc::new(compile_host_fluid_palette(&content)?);
        let presentation = Arc::new(HostPresentationIndex::compile(
            images,
            &content,
            &palette,
            &fluid_palette,
        )?);
        let empty = HostVoxel::from_solid(
            palette_index(&palette, &worldgen.empty)
                .ok_or(ProductionHostError::UnknownDraftBlock)?,
            &presentation,
        );
        let placement_content = worldgen.placement_content.clone();
        let voxel_schema: SchemaId = VOXEL_SCHEMA.parse()?;
        let voxel_schema_version =
            PayloadSchemaVersion::new(VOXEL_SCHEMA_VERSION).map_err(ProductionHostError::from)?;
        let clamps = StreamClamps::new(host_hard_limits()?, &config)?;
        let scope = WorkingSetScope::new(world, dimension.clone(), WorldEpoch::new(1));
        let limits = runtime_limits(clamps.hard_limits)?;
        let runtime =
            VoxelRuntime::new(scope, RuntimeGeneration::new(1), chunk_edge, empty, limits)?;
        let spawn = validated_spawn(&plan, &worldgen.bindings)?;
        let spawn_center = player_spawn_center(spawn)?;
        let spawn_chunk = translation_chunk(spawn_center, chunk_edge)
            .ok_or(ProductionHostError::InvalidPlayerPose)?;
        let restored_session = match &world_store {
            Some(store) => peek_player_session(store, world, &dimension, spawn_chunk)?,
            None => None,
        };
        let mut player_pose = ProductionPlayerPose {
            translation: spawn_center,
            yaw_radians: 0.0,
            grounded: false,
        };
        if let Some(session) = &restored_session {
            player_pose = session.pose();
        }
        let worldgen_materialization = Arc::new(WorldgenMaterialization {
            plan: Arc::clone(&plan),
            content: Arc::clone(&content),
            palette: Arc::clone(&palette),
            fluid_palette: Arc::clone(&fluid_palette),
            presentation: Arc::clone(&presentation),
            spawn,
            empty,
            chunk_edge,
        });

        let mut inner = ProductionSpineInner {
            runtime,
            plan,
            worldgen_materialization,
            worldgen_bindings: worldgen.bindings,
            spawn,
            clamps,
            content,
            palette,
            solid_palette,
            fluid_palette,
            empty,
            chunk_edge,
            world,
            dimension,
            next_transaction: 1,
            voxel_schema,
            voxel_schema_version,
            derived: BTreeMap::new(),
            mesh_dirty: BTreeSet::new(),
            collider_dirty: BTreeSet::new(),
            removed: BTreeSet::new(),
            waiting_derived: VecDeque::new(),
            in_flight_tasks: Vec::new(),
            pending_worldgen: VecDeque::new(),
            in_flight_worldgen_tasks: Vec::new(),
            waiting_worldgen: BTreeMap::new(),
            worldgen_tickets: BTreeMap::new(),
            next_worldgen_sequence: 0,
            next_worldgen_apply_sequence: 0,
            presentation,
            edited: BTreeSet::new(),
            lifecycle: BTreeMap::new(),
            eviction_lease: 0,
            residency: BTreeMap::new(),
            stream_anchor_xz: [0.0, 0.0],
            look_ahead_axis: [0, 0],
            look_ahead_tick: 0,
            last_reconciled_tick: None,
            last_desired: BTreeSet::new(),
            stream_admissions: 0,
            stream_evictions: 0,
            spawn_center,
            placement_content,
            last_success: None,
            last_reject: None,
            current_target: None,
            last_inspect: None,
            player_pose,
            last_stream_error: None,
            gameplay: None,
            world_store,
            display,
            interest_reconciliations: 0,
        };
        inner.stream_anchor_xz = [
            inner.player_pose.translation.x,
            inner.player_pose.translation.z,
        ];
        let origin = translation_chunk(inner.player_pose.translation, inner.chunk_edge)
            .ok_or(ProductionHostError::InvalidPlayerPose)?;
        fill_working_set(&mut inner, &kernel, origin, [0, 0], FixedTick::new(0))?;
        inner.last_success = None;
        bind_gameplay_session(&mut inner, &kernel, catalog, spawn_chunk)?;
        if let Some(session) = restored_session {
            restore_player_session(&mut inner, &session)?;
        }

        let spine = Self {
            inner: Arc::new(Mutex::new(inner)),
            storage: ProductionWorldStorage { kernel },
        };
        await_derived_ready(
            &spine,
            FixedTick::new(0),
            DerivedReadiness::Startup,
            &[],
            STARTUP_BARRIER_SLICES,
        )?;
        Ok(spine)
    }

    pub(super) fn storage(&self) -> ProductionWorldStorage {
        self.storage.clone()
    }

    /// Returns the cubic chunk edge in voxels.
    #[must_use]
    pub fn chunk_edge(&self) -> u16 {
        self.lock_inner().map_or(0, |inner| inner.chunk_edge)
    }

    /// Returns the persistent world identity owned by this spine.
    #[must_use]
    pub fn world_id(&self) -> Option<WorldId> {
        self.lock_inner().ok().map(|inner| inner.world)
    }

    /// Returns the non-durable production kernel.
    #[must_use]
    pub fn kernel(&self) -> &MemoryTransactionKernel {
        self.storage.kernel()
    }

    /// Returns the memory-kernel materialized-chunk world hash.
    ///
    /// Pause must leave this digest unchanged. It is not a durable catalog hash
    /// and is not a physical checkpoint.
    ///
    /// # Errors
    ///
    /// Returns [`ProductionHostError`] when the spine lock is poisoned or the
    /// memory kernel cannot snapshot.
    pub fn materialized_chunk_state_hash(
        &self,
    ) -> Result<MaterializedChunkStateHash, ProductionHostError> {
        let world = self.lock_inner()?.world;
        Ok(self
            .storage
            .kernel()
            .reference_snapshot(world)?
            .materialized_chunk_state_hash())
    }

    /// Commits edited working-set chunks through an already activated host writer.
    ///
    /// [`MemoryTransactionKernel`] stays the session cache. Player pose,
    /// inventory, selected slot, tool durability, dropped items, containers,
    /// and scheduled work are captured onto the spawn chunk before the
    /// world-db commit.
    /// Durable oracles publish [`CommitDurabilityV1::Durable`]; volatile
    /// references stay [`CommitDurabilityV1::Written`]. After a durable
    /// commit, previously dirty chunks may be evicted and later hydrated from
    /// storage instead of being regenerated.
    ///
    /// # Errors
    ///
    /// Returns [`ProductionHostError`] when the spine lock is poisoned, the
    /// memory kernel cannot snapshot, world-db cannot be read, or the writer
    /// rejects the commit.
    pub fn flush_dirty_chunks(
        &self,
        writer: &mut SealedWorldWriterHost,
        metadata: &AuthoritativeMetadataInputV1,
    ) -> Result<Option<WorldCommitOutcomeV1>, ProductionHostError> {
        self.persist_player_session()?;
        let inner = self.lock_inner()?;
        let world = inner.world;
        let dimension = inner.dimension.clone();
        let edited = inner.edited.clone();
        drop(inner);
        let snapshot = self.storage.kernel().reference_snapshot(world)?;
        let view = writer.begin_read(world)?;
        let mut mutations = Vec::new();
        for coordinate in edited {
            let key = ChunkKey::new(world, dimension.clone(), coordinate);
            let Some(stored) = snapshot.chunk(&key) else {
                continue;
            };
            let persisted = view.load_chunk(&key)?;
            let changed = chunk_changed_domains(
                persisted.as_ref().map(PersistedChunkV1::data),
                stored.data(),
            );
            if changed.is_empty() {
                continue;
            }
            let expectation = match persisted.as_ref() {
                Some(current) => ChunkRevisionExpectation::Exact(current.chunk_revision()),
                None => ChunkRevisionExpectation::Absent,
            };
            mutations.push(ChunkMutation::new(
                key,
                expectation,
                changed,
                stored.data().clone(),
            ));
        }
        mutations.sort_by(|left, right| left.key().cmp(right.key()));
        let durability = match writer.durability_capability() {
            StorageDurabilityCapabilityV1::WalSyncCheckpoint => CommitDurabilityV1::Durable,
            StorageDurabilityCapabilityV1::VolatileReference => CommitDurabilityV1::Written,
        };
        let outcome = commit_mutations(
            writer,
            world,
            view.frontier().current(),
            metadata,
            &mutations,
            durability,
        )?;
        if durability == CommitDurabilityV1::Durable {
            self.lock_inner()?.edited.clear();
        }
        Ok(outcome)
    }

    /// Returns the pose used to spawn the local player capsule.
    #[must_use]
    pub fn restored_spawn_translation(&self) -> Vec3 {
        self.lock_inner()
            .map_or(Vec3::ZERO, |inner| inner.player_pose.translation)
    }

    fn persist_player_session(&self) -> Result<(), ProductionHostError> {
        let kernel = self.storage.kernel();
        let mut inner = self.lock_inner()?;
        let spawn_chunk = translation_chunk(inner.spawn_center, inner.chunk_edge)
            .ok_or(ProductionHostError::InvalidPlayerPose)?;
        let pose = inner.player_pose;
        let session = {
            let gameplay = inner
                .gameplay
                .as_ref()
                .ok_or(ProductionHostError::InvalidPlayerSession)?;
            let inventory = gameplay.inventory_view();
            let containers = gameplay
                .containers()
                .iter()
                .map(|(id, container)| (*id, container))
                .collect::<Vec<_>>();
            let scheduled = gameplay
                .continuations()
                .iter()
                .map(|(id, continuation)| (*id, continuation))
                .collect::<Vec<_>>();
            let drops = gameplay
                .dropped_items()
                .iter()
                .map(|(id, drop)| (*id, drop))
                .collect::<Vec<_>>();
            DurablePlayerSessionV1::capture(
                pose,
                inventory
                    .as_ref()
                    .map_or(0, ProductionInventoryView::hotbar_slot),
                inventory
                    .as_ref()
                    .map_or(&[], ProductionInventoryView::slots),
                &containers,
                &scheduled,
                &drops,
                gameplay.next_drop(),
            )
        };
        let payload = session.encode_payload()?;
        let snapshot = kernel.reference_snapshot(inner.world)?;
        let key = ChunkKey::new(inner.world, inner.dimension.clone(), spawn_chunk);
        let stored = snapshot
            .chunk(&key)
            .ok_or(ProductionHostError::MissingStoredChunk {
                coordinate: spawn_chunk,
            })?;
        let mut entities = stored.data().persistent_entities().clone();
        entities.insert(PLAYER_SESSION_ENTITY, payload);
        let replacement = ChunkData::new(
            stored.data().voxels().clone(),
            entities,
            stored.data().continuations().clone(),
            stored.data().provenance().clone(),
        );
        let changed = chunk_changed_domains(Some(stored.data()), &replacement);
        if changed.is_empty() {
            return Ok(());
        }
        kernel.commit(WorldTransaction::new(
            next_transaction_id(&mut inner),
            inner.world,
            snapshot.revision(),
            vec![ChunkMutation::new(
                key,
                ChunkRevisionExpectation::Exact(stored.revision()),
                changed,
                replacement,
            )],
        ))?;
        inner.edited.insert(spawn_chunk);
        Ok(())
    }

    /// Returns the local player spawn center in meters.
    #[must_use]
    pub fn spawn_center(&self) -> Vec3 {
        self.lock_inner()
            .map_or(Vec3::ZERO, |inner| inner.spawn_center)
    }

    /// Returns default placement content for the place action.
    #[must_use]
    pub fn placement_content(&self) -> Option<BlockId> {
        self.lock_inner()
            .ok()
            .map(|inner| inner.placement_content.clone())
    }

    /// Returns the locked solid palette in index order.
    #[must_use]
    #[cfg(feature = "client")]
    pub(super) fn palette_ids(&self) -> Vec<BlockId> {
        self.lock_inner()
            .map(|inner| inner.palette.to_vec())
            .unwrap_or_default()
    }

    /// Locked terrain layer table used by the client material adapter.
    #[must_use]
    #[cfg(feature = "client")]
    pub(super) fn terrain_layer_table(
        &self,
    ) -> Option<latticeaxiom_render_contracts::CompiledTerrainLayerTableV1> {
        self.lock_inner()
            .ok()
            .map(|inner| inner.presentation.table().clone())
    }

    /// Returns accepted derived geometry retained for a presented chunk.
    #[must_use]
    #[cfg(feature = "client")]
    pub(super) fn derived_geometry(
        &self,
        coordinate: ChunkCoordinate,
    ) -> Option<Arc<MeshBuffer<LayerMergeKey>>> {
        self.lock_inner().ok().and_then(|inner| {
            inner
                .derived
                .get(&coordinate)
                .and_then(|derived| derived.geometry.as_ref().map(Arc::clone))
        })
    }

    /// Returns the union of derived mesh faces currently retained.
    #[must_use]
    pub fn visible_faces(&self) -> BTreeSet<Face> {
        match self.lock_inner() {
            Ok(inner) => {
                let mut faces = BTreeSet::new();
                for derived in inner.derived.values() {
                    if let Some(geometry) = &derived.geometry {
                        faces.extend(geometry.iter().map(|(_, face, _)| face));
                    }
                }
                faces
            }
            Err(_) => BTreeSet::new(),
        }
    }

    /// Returns whether derived meshes currently emit all six faces.
    #[must_use]
    pub fn has_all_six_faces(&self) -> bool {
        let faces = self.visible_faces();
        Face::ALL.iter().all(|face| faces.contains(face))
    }

    /// Returns how many chunks currently have derived mesh state.
    #[must_use]
    pub fn derived_chunk_count(&self) -> usize {
        self.lock_inner().map_or(0, |inner| inner.derived.len())
    }

    /// Returns chunk coordinates currently resident in the voxel working set.
    #[must_use]
    pub fn resident_chunks(&self) -> BTreeSet<ChunkCoordinate> {
        self.lock_inner().map_or_else(
            |_| BTreeSet::new(),
            |inner| {
                inner
                    .lifecycle
                    .iter()
                    .filter_map(|(coordinate, state)| {
                        matches!(
                            state,
                            ChunkLifecycle::Resident
                                | ChunkLifecycle::MeshCollider
                                | ChunkLifecycle::Active
                        )
                        .then_some(*coordinate)
                    })
                    .collect()
            },
        )
    }

    /// Returns chunks pinned because they carry player edits.
    #[must_use]
    pub fn edited_chunks(&self) -> BTreeSet<ChunkCoordinate> {
        self.lock_inner()
            .map_or_else(|_| BTreeSet::new(), |inner| inner.edited.clone())
    }

    /// Returns the lifecycle of one streamed chunk.
    #[must_use]
    pub fn chunk_lifecycle(&self, coordinate: ChunkCoordinate) -> ChunkLifecycle {
        self.lock_inner().map_or(ChunkLifecycle::Absent, |inner| {
            inner
                .lifecycle
                .get(&coordinate)
                .copied()
                .unwrap_or(ChunkLifecycle::Absent)
        })
    }

    /// Returns whether mesh and collider derivation allow entry into cave voids.
    #[must_use]
    pub fn cave_entry_ready(&self, coordinate: ChunkCoordinate) -> bool {
        self.lock_inner()
            .is_ok_and(|inner| cave_entry_ready_inner(&inner, coordinate))
    }

    /// Returns whether the compiled V5 plan includes the natural layer.
    #[must_use]
    pub fn has_natural_layer(&self) -> bool {
        self.lock_inner()
            .is_ok_and(|inner| inner.plan.has_natural_layer())
    }

    /// Returns whether the compiled plan includes V6 cave topology.
    #[must_use]
    pub fn has_cave_topology_layer(&self) -> bool {
        self.lock_inner()
            .is_ok_and(|inner| inner.plan.has_cave_topology_layer())
    }

    /// Returns whether the compiled plan includes V6 hydrology occupancy.
    #[must_use]
    pub fn has_hydrology_occupancy(&self) -> bool {
        self.lock_inner()
            .is_ok_and(|inner| inner.plan.has_hydrology_occupancy())
    }

    /// Returns the topology ownership domain at world `(x, y, z)`.
    #[must_use]
    pub fn cave_topology_domain(&self, x: i64, y: i64, z: i64) -> Option<StableId> {
        self.lock_inner()
            .ok()
            .and_then(|inner| inner.plan.cave_topology_domain(x, y, z).cloned())
    }

    /// Returns canonically ordered underground-owned topology domains.
    #[must_use]
    pub fn cave_owned_domains(&self) -> Vec<StableId> {
        self.lock_inner().map_or_else(
            |_| Vec::new(),
            |inner| {
                inner
                    .plan
                    .cave_topology_owned_domains()
                    .unwrap_or(&[])
                    .iter()
                    .map(|domain| domain.domain().clone())
                    .collect()
            },
        )
    }

    /// Returns must-connect destination voxels and their topology domains.
    #[must_use]
    pub fn cave_destinations(&self) -> Vec<([i64; 3], StableId)> {
        self.lock_inner().map_or_else(
            |_| Vec::new(),
            |inner| {
                let Some(entrances) = inner.plan.cave_topology_entrances() else {
                    return Vec::new();
                };
                let mut destinations = Vec::new();
                for entrance in entrances {
                    let cell = entrance.destination_cell();
                    let voxel = latticeaxiom_worldgen::cell_center_voxels(
                        cell[0],
                        cell[1],
                        entrance.y_voxel().saturating_mul(1_000),
                        inner.plan.config(),
                    );
                    let Some(domain) = inner
                        .plan
                        .cave_topology_domain(voxel[0], voxel[1], voxel[2])
                        .cloned()
                    else {
                        continue;
                    };
                    destinations.push((voxel, domain));
                }
                destinations.sort_by(|left, right| left.1.cmp(&right.1).then(left.0.cmp(&right.0)));
                destinations.dedup();
                destinations
            },
        )
    }

    /// Projects bounded worldgen inspect rows from the compiled V5 plan.
    ///
    /// Collection does not mutate chunks, spawn, or overlay state.
    ///
    /// # Errors
    ///
    /// Returns [`ProductionHostError`] when the spine lock is poisoned or inspect
    /// compilation fails closed.
    pub fn worldgen_inspect_report(&self) -> Result<WorldgenInspectReportV1, ProductionHostError> {
        let inner = self.lock_inner()?;
        compile_host_worldgen_inspect(
            &inner.plan,
            &inner.worldgen_bindings,
            inner.spawn,
            EngineEpoch::new(1),
            InspectWorldEpoch::new(1),
        )
    }

    /// Returns the required `CaveTopology` field portal nearest the spawn column.
    #[must_use]
    pub fn required_cave_entrance(&self) -> Option<RequiredCaveEntranceV1> {
        let inner = self.lock_inner().ok()?;
        let origin = translation_chunk(inner.spawn_center, inner.chunk_edge)?;
        required_cave_entrance(&inner.plan, origin)
    }

    /// Returns local/branch/portal occupancy from the compiled `CaveTopology` owner.
    #[must_use]
    pub fn cave_occupancy_arbitration(
        &self,
        x: i64,
        y: i64,
        z: i64,
    ) -> Option<CaveOccupancyArbitrationV1> {
        self.lock_inner()
            .ok()
            .map(|inner| inner.plan.cave_occupancy_arbitration(x, y, z))
    }

    /// Returns whether the local player currently occupies an unready cave void.
    #[must_use]
    pub fn occupies_unready_cave_void(&self) -> bool {
        let Ok(inner) = self.lock_inner() else {
            return false;
        };
        player_sample_cells(inner.player_pose.translation)
            .iter()
            .any(|&[x, y, z]| {
                inner
                    .plan
                    .cave_occupancy_arbitration(x, y, z)
                    .is_finally_void()
                    && !cave_entry_ready_inner(&inner, world_chunk(x, y, z, inner.chunk_edge))
            })
    }

    /// Returns the structural host clamps for this session.
    #[must_use]
    pub fn hard_limits(&self) -> Option<PlayableWorldHardLimitsV1> {
        self.lock_inner().ok().map(|inner| inner.clamps.hard_limits)
    }

    /// Returns the player request admitted after the host cap.
    #[must_use]
    pub fn admitted_view_distance(&self) -> u32 {
        self.lock_inner()
            .map_or(1, |inner| inner.clamps.admitted_render_distance())
    }

    /// Returns the interest radius actually admitted after resident-budget clamping.
    #[must_use]
    pub fn effective_view_distance(&self) -> u32 {
        self.lock_inner()
            .map_or(1, |inner| inner.clamps.effective_render_distance())
    }

    /// Returns the accepted request and its effective host clamp.
    #[must_use]
    pub fn view_distance_status(&self) -> Option<ViewDistanceStatusV1> {
        self.lock_inner()
            .ok()
            .map(|inner| inner.clamps.view_distance_status())
    }

    /// Requests an authored render radius in `2..=32` chunks.
    ///
    /// Host admission, generation, and resident budgets may lower the effective
    /// render radius without mutating the authored request.
    ///
    /// # Errors
    ///
    /// Returns [`ProductionHostError::Poisoned`] when the spine lock is poisoned.
    pub fn set_requested_view_distance(&self, chunks: u32) -> Result<u32, ProductionHostError> {
        let mut inner = self.lock_inner()?;
        inner.clamps.set_requested_view_distance(chunks)?;
        Ok(inner.clamps.effective_render_distance())
    }

    /// Returns occupancy copied from [`VoxelRuntime`] diagnostics.
    #[must_use]
    pub fn working_set_diagnostics(&self) -> WorkingSetDiagnosticsV1 {
        self.lock_inner().map_or_else(
            |_| WorkingSetDiagnosticsV1::default(),
            |inner| WorkingSetDiagnosticsV1::from_runtime(inner.runtime.diagnostics()),
        )
    }

    /// Machine-readable V4/P4 streaming coverage evidence.
    ///
    /// The live 8³ fixture does not claim the ADR 0026 D2/D10 working-set gate.
    #[must_use]
    pub fn streaming_profile_evidence(&self) -> Option<StreamingProfileEvidenceV1> {
        let inner = self.lock_inner().ok()?;
        let requested = u32::try_from(inner.last_desired.len()).unwrap_or(u32::MAX);
        let diagnostics = WorkingSetDiagnosticsV1::from_runtime(inner.runtime.diagnostics());
        Some(StreamingProfileEvidenceV1::capture(
            inner.clamps,
            inner.plan.config(),
            super::profile::StreamingProfileCountsV1 {
                resident: diagnostics.resident(),
                active: diagnostics.active(),
                visible: diagnostics.visible(),
                in_flight: diagnostics.in_flight(),
                requested,
                dirty: diagnostics.dirty(),
                reserved_bytes: diagnostics.reserved_bytes(),
                byte_budget: diagnostics.byte_budget(),
            },
        ))
    }

    /// Completes one derived job as an executor panic without applying results.
    ///
    /// Reservations are released. Authoritative voxel state and the
    /// materialized-chunk world hash are unchanged.
    ///
    /// # Errors
    ///
    /// Returns [`ProductionHostError`] when the spine lock is poisoned, no job
    /// of `kind` is dispatchable, or completion is rejected.
    pub fn complete_panicked_derived_job(
        &self,
        kind: DerivedKind,
    ) -> Result<WorkerAbortOutcome<()>, ProductionHostError> {
        let mut inner = self.lock_inner()?;
        match inner.runtime.dispatch_next(kind)? {
            DispatchOutcome::Started(input) => {
                match inner.runtime.complete_executor::<u8, (), ()>(
                    ExecutorOutcome::Panicked { input },
                    ApplyByteDeclaration::new(0),
                    FixedTick::new(0),
                    |_| Ok(()),
                ) {
                    ExecutorFinish::Aborted(WorkerAbortOutcome::Panicked(receipt)) => {
                        Ok(WorkerAbortOutcome::Panicked(receipt))
                    }
                    ExecutorFinish::Aborted(WorkerAbortOutcome::Lost(receipt)) => {
                        Ok(WorkerAbortOutcome::Lost(receipt))
                    }
                    ExecutorFinish::Aborted(WorkerAbortOutcome::UnknownTicket { job, ticket }) => {
                        Ok(WorkerAbortOutcome::UnknownTicket { job, ticket })
                    }
                    ExecutorFinish::Aborted(WorkerAbortOutcome::UnknownInput { .. })
                    | ExecutorFinish::Completed(_) => Err(ProductionHostError::DerivedRejected),
                }
            }
            DispatchOutcome::Empty => Err(ProductionHostError::DerivedRejected),
            DispatchOutcome::Backpressured { .. } => Err(ProductionHostError::DerivedBackpressure),
            DispatchOutcome::MemoryContractViolation { .. } => {
                Err(ProductionHostError::DerivedMemory)
            }
        }
    }

    /// Combined derived-queue occupancy used by backpressure tests.
    #[must_use]
    pub fn derived_queue_snapshot(&self) -> DerivedQueueSnapshotV1 {
        self.lock_inner().map_or_else(
            |_| DerivedQueueSnapshotV1::default(),
            |inner| {
                let diagnostics = inner.runtime.diagnostics();
                DerivedQueueSnapshotV1 {
                    mesh_pending: diagnostics.mesh().pending(),
                    mesh_in_flight: diagnostics.mesh().in_flight(),
                    collider_pending: diagnostics.collider().pending(),
                    collider_in_flight: diagnostics.collider().in_flight(),
                    cancel_requests: diagnostics
                        .mesh()
                        .cancel_requests()
                        .saturating_add(diagnostics.collider().cancel_requests()),
                    waiting_to_apply: diagnostics.waiting_to_apply_jobs(),
                    reserved_bytes: diagnostics.combined_reserved_bytes(),
                    apply_stopped_jobs: diagnostics.apply_stopped_jobs(),
                    apply_stopped_bytes: diagnostics.apply_stopped_bytes(),
                    apply_stopped_wall_clock: diagnostics.apply_stopped_wall_clock(),
                    spawned_tasks: inner.in_flight_tasks.len(),
                }
            },
        )
    }

    /// Returns bounded world-generation occupancy across the Bevy task boundary.
    #[must_use]
    pub fn worldgen_queue_snapshot(&self) -> WorldgenQueueSnapshotV1 {
        self.lock_inner().map_or_else(
            |_| WorldgenQueueSnapshotV1::default(),
            |inner| WorldgenQueueSnapshotV1 {
                pending: inner.pending_worldgen.len(),
                in_flight: inner.in_flight_worldgen_tasks.len(),
                waiting_to_apply: inner.waiting_worldgen.len(),
            },
        )
    }

    /// Streams interest, generation, and eviction from the latest player pose.
    ///
    /// # Errors
    ///
    /// Returns [`ProductionHostError`] when generation, storage publication,
    /// projection, eviction, or derived apply fails.
    pub fn sync_interest(&self, fixed_tick: u64) -> Result<(), ProductionHostError> {
        let result = self.sync_interest_inner(fixed_tick);
        if let Ok(mut inner) = self.lock_inner() {
            inner.last_stream_error = result.as_ref().err().map(ToString::to_string);
        }
        result
    }

    /// Ensures colliders for the current player capsule without reconciling interest.
    ///
    /// This is the movement-time safety gate: it may complete pending collider
    /// jobs and emit conservative shapes for occupied chunks, but it does not
    /// generate, evict, or consume a presentation delta.
    ///
    /// # Errors
    ///
    /// Returns [`ProductionHostError`] when the player pose is outside the
    /// chunk domain or collider derivation fails.
    pub(super) fn ensure_collider_safety(
        &self,
        fixed_tick: u64,
    ) -> Result<Vec<ColliderPresentation>, ProductionHostError> {
        let result = self.ensure_collider_safety_inner(fixed_tick);
        if result.is_err()
            && let Ok(mut inner) = self.lock_inner()
        {
            inner.last_stream_error = result.as_ref().err().map(ToString::to_string);
        }
        result
    }

    /// Number of full interest reconciliations performed by [`Self::sync_interest`].
    #[must_use]
    pub fn interest_reconciliation_count(&self) -> u64 {
        self.lock_inner()
            .map_or(0, |inner| inner.interest_reconciliations)
    }

    /// Chunks admitted into the working set since materialization.
    #[must_use]
    pub fn stream_admission_count(&self) -> u64 {
        self.lock_inner().map_or(0, |inner| inner.stream_admissions)
    }

    /// Chunks evicted from the working set since materialization.
    #[must_use]
    pub fn stream_eviction_count(&self) -> u64 {
        self.lock_inner().map_or(0, |inner| inner.stream_evictions)
    }

    /// Sticky look-ahead axis used by the latest interest reconciliation.
    #[must_use]
    pub fn interest_look_ahead(&self) -> [i32; 2] {
        self.lock_inner()
            .map_or([0, 0], |inner| inner.look_ahead_axis)
    }

    /// Returns the last chunk-stream failure, if any.
    #[must_use]
    pub fn last_stream_error(&self) -> Option<String> {
        self.lock_inner()
            .ok()
            .and_then(|inner| inner.last_stream_error.clone())
    }

    fn sync_interest_inner(&self, fixed_tick: u64) -> Result<(), ProductionHostError> {
        poll_worldgen_tasks(self)?;
        let mut inner = self.lock_inner()?;
        if inner.last_reconciled_tick == Some(fixed_tick) {
            return Ok(());
        }
        inner.last_reconciled_tick = Some(fixed_tick);
        inner.interest_reconciliations = inner.interest_reconciliations.saturating_add(1);
        let translation = inner.player_pose.translation;
        let chunk = translation_chunk(translation, inner.chunk_edge)
            .ok_or(ProductionHostError::InvalidPlayerPose)?;
        let (look_ahead, look_ahead_tick) = sticky_look_ahead(
            [
                translation.x - inner.stream_anchor_xz[0],
                translation.z - inner.stream_anchor_xz[1],
            ],
            inner.look_ahead_axis,
            inner.look_ahead_tick,
            fixed_tick,
            LOOK_AHEAD_EXPIRY_TICKS,
        );
        inner.look_ahead_axis = look_ahead;
        inner.look_ahead_tick = look_ahead_tick;
        inner.stream_anchor_xz = [translation.x, translation.z];
        let admit_limit = inner.clamps.max_in_flight();
        let tick = FixedTick::new(fixed_tick);
        apply_ready_worldgen(&mut inner, self.storage.kernel(), tick, chunk, look_ahead)?;
        sync_working_set(
            &mut inner,
            self.storage.kernel(),
            chunk,
            look_ahead,
            tick,
            WorldgenAdmission {
                limit: admit_limit,
                execution: WorldgenExecution::Deferred,
            },
        )?;
        drop(inner);
        spawn_worldgen_jobs(self)?;
        drain_derived(self, tick)
    }

    fn ensure_collider_safety_inner(
        &self,
        fixed_tick: u64,
    ) -> Result<Vec<ColliderPresentation>, ProductionHostError> {
        let tick = FixedTick::new(fixed_tick);
        let (occupied, edge) = {
            let inner = self.lock_inner()?;
            let translation = inner.player_pose.translation;
            let mut occupied = player_occupied_chunks(translation, inner.chunk_edge)?;
            if let Some(chunk) = translation_chunk(translation, inner.chunk_edge) {
                occupied.insert(chunk);
            }
            (occupied, f32::from(inner.chunk_edge))
        };
        await_collider_safety(self, &occupied, tick)?;
        let mut inner = self.lock_inner()?;
        let mut updates = Vec::new();
        for coordinate in occupied {
            if !inner.runtime.is_resident(coordinate) {
                continue;
            }
            seal_unready_cave_voids(&mut inner, coordinate);
            let Some(derived) = inner.derived.get(&coordinate) else {
                continue;
            };
            let Some(collider) = derived.collider.clone() else {
                continue;
            };
            updates.push(ColliderPresentation {
                coordinate,
                origin: chunk_origin(coordinate, edge),
                collider,
            });
        }
        refresh_lifecycle(&mut inner);
        Ok(updates)
    }

    /// Returns a cursor used to detect mesh invalidation after an edit.
    #[must_use]
    pub fn mesh_cursor(&self, coordinate: ChunkCoordinate) -> Option<ChunkMeshCursor> {
        let inner = self.lock_inner().ok()?;
        let derived = inner.derived.get(&coordinate)?;
        Some(ChunkMeshCursor {
            coordinate,
            receipt: derived.mesh_receipt?,
            revision: derived.revision,
        })
    }

    /// Returns whether `cursor` is stale against current derived state.
    #[must_use]
    pub fn mesh_invalidated(&self, cursor: &ChunkMeshCursor) -> bool {
        self.lock_inner().is_ok_and(|inner| {
            inner.derived.get(&cursor.coordinate).is_none_or(|derived| {
                derived.mesh_source.is_none_or(|source| {
                    !cursor.receipt.is_current_for(source) || derived.revision != cursor.revision
                })
            })
        })
    }

    /// Returns the current chunk revision projected from storage.
    #[must_use]
    pub fn chunk_revision(&self, coordinate: ChunkCoordinate) -> Option<ChunkRevision> {
        self.lock_inner().ok().and_then(|inner| {
            inner
                .runtime
                .chunk_revisions(coordinate)
                .map(|(_, revision, _)| revision)
        })
    }

    /// Returns the last committed block-edit success.
    #[must_use]
    pub fn last_success(&self) -> Option<BlockEditSuccessV1> {
        self.lock_inner()
            .ok()
            .and_then(|inner| inner.last_success.clone())
    }

    /// Returns the last authoritative block-edit rejection.
    #[must_use]
    pub fn last_reject(&self) -> Option<BlockEditRejectV1> {
        self.lock_inner()
            .ok()
            .and_then(|inner| inner.last_reject.clone())
    }

    /// Returns the compiled gameplay catalog bound to this session.
    #[must_use]
    pub fn gameplay_catalog(&self) -> Option<GameplayCatalog> {
        self.lock_inner().ok().and_then(|inner| {
            inner
                .gameplay
                .as_ref()
                .map(|session| session.catalog().clone())
        })
    }

    /// Resolves a HUD display name and icon for locked content.
    ///
    /// Omitting the presentation package still returns the deterministic
    /// missing-presentation fallback.
    #[must_use]
    pub fn content_display(&self, content_id: &str) -> ContentDisplayLabelV1 {
        self.lock_inner().map_or_else(
            |_| ContentDisplayLabelV1::missing_presentation(content_id),
            |inner| inner.display.lookup(content_id),
        )
    }

    /// Returns the local inventory and selected hotbar slot.
    #[must_use]
    pub fn inventory_view(&self) -> Option<ProductionInventoryView> {
        self.lock_inner().ok().and_then(|inner| {
            inner
                .gameplay
                .as_ref()
                .and_then(ProductionGameplay::inventory_view)
        })
    }

    /// Selects a hotbar slot in `0..HOTBAR_SLOTS`.
    ///
    /// # Errors
    ///
    /// Returns [`GameplayReject::SlotOutOfRange`] for an invalid index.
    pub fn select_hotbar_slot(&self, slot: u16) -> Result<(), GameplayReject> {
        let mut inner = self
            .lock_inner()
            .map_err(|_| GameplayReject::StorageCommitMismatch {
                resource: "spine_lock",
            })?;
        inner
            .gameplay
            .as_mut()
            .ok_or(GameplayReject::UnknownPlayer {
                player: local_player_id().as_bytes(),
            })?
            .select_hotbar_slot(slot)
    }

    /// Returns dropped items still waiting for pickup.
    #[must_use]
    pub fn dropped_items(&self) -> BTreeMap<DropEntityId, latticeaxiom_gameplay::DroppedItemV1> {
        self.lock_inner().map_or_else(
            |_| BTreeMap::new(),
            |inner| {
                inner
                    .gameplay
                    .as_ref()
                    .map(|session| session.dropped_items().clone())
                    .unwrap_or_default()
            },
        )
    }

    /// Returns the last gameplay command outcome.
    #[must_use]
    pub fn last_gameplay_outcome(&self) -> Option<CommandOutcomeV1> {
        self.lock_inner().ok().and_then(|inner| {
            inner
                .gameplay
                .as_ref()
                .and_then(ProductionGameplay::last_outcome)
                .cloned()
        })
    }

    /// Returns the last gameplay kernel rejection.
    #[must_use]
    pub fn last_gameplay_reject(&self) -> Option<GameplayReject> {
        self.lock_inner().ok().and_then(|inner| {
            inner
                .gameplay
                .as_ref()
                .and_then(ProductionGameplay::last_reject)
                .cloned()
        })
    }

    /// Mines one cell through the gameplay kernel without DDA.
    ///
    /// # Errors
    ///
    /// Returns a block-edit rejection when targeting, planning, or commit fails.
    pub fn mine_cell(
        &self,
        position: BlockPosition,
    ) -> Result<BlockEditSuccessV1, BlockEditRejectV1> {
        let result = {
            let mut inner = self
                .inner
                .lock()
                .map_err(|_| BlockEditRejectV1::StorageUnavailable)?;
            let result = inner.mine_cell(self.storage.kernel(), position, 0);
            record_edit_result(&mut inner, &result);
            result
        };
        finish_edit_readiness(self, FixedTick::new(0), [position])?;
        result
    }

    /// Places from the selected hotbar slot beside `anchor` using `face`.
    ///
    /// # Errors
    ///
    /// Returns a block-edit rejection when placement validation or commit fails.
    pub fn place_from_hotbar(
        &self,
        anchor: BlockPosition,
        face: BlockFaceV1,
    ) -> Result<BlockEditSuccessV1, BlockEditRejectV1> {
        let result = {
            let mut inner = self
                .inner
                .lock()
                .map_err(|_| BlockEditRejectV1::StorageUnavailable)?;
            let adjacent = face
                .adjacent(anchor)
                .ok_or(BlockEditRejectV1::PermissionDenied)?;
            let result =
                inner.place_cell(self.storage.kernel(), adjacent, None, [0.0, 0.0, 0.0], 0);
            record_edit_result(&mut inner, &result);
            result
        };
        let placed = face
            .adjacent(anchor)
            .ok_or(BlockEditRejectV1::PermissionDenied)?;
        finish_edit_readiness(self, FixedTick::new(0), [placed])?;
        result
    }

    /// Picks up one dropped stack.
    ///
    /// # Errors
    ///
    /// Returns [`GameplayReject`] when the drop is missing or inventory is full.
    pub fn pickup_drop(&self, drop: DropEntityId) -> Result<CommandOutcomeV1, GameplayReject> {
        let mut inner = self
            .lock_inner()
            .map_err(|_| GameplayReject::StorageCommitMismatch {
                resource: "spine_lock",
            })?;
        inner.pickup_drop(self.storage.kernel(), drop)
    }

    /// Crafts `recipe`, optionally at a bound workstation container.
    ///
    /// # Errors
    ///
    /// Returns [`GameplayReject`] when inputs, workstation, or output fail closed.
    pub fn craft_recipe(
        &self,
        recipe: &RecipeId,
        workstation: Option<ContainerId>,
    ) -> Result<CommandOutcomeV1, GameplayReject> {
        let mut inner = self
            .lock_inner()
            .map_err(|_| GameplayReject::StorageCommitMismatch {
                resource: "spine_lock",
            })?;
        inner.craft_recipe(self.storage.kernel(), recipe, workstation)
    }

    /// Moves the full `from` stack onto `to` through the gameplay kernel.
    ///
    /// # Errors
    ///
    /// Returns [`GameplayReject`] when the spine lock is poisoned, the source
    /// is empty, or the inventory revision is stale.
    pub fn move_stack(
        &self,
        from: SlotIndex,
        to: SlotIndex,
    ) -> Result<CommandOutcomeV1, GameplayReject> {
        let mut inner = self
            .lock_inner()
            .map_err(|_| GameplayReject::StorageCommitMismatch {
                resource: "spine_lock",
            })?;
        inner.move_stack(self.storage.kernel(), from, to)
    }

    /// Starts a catalog furnace process on a bound container.
    ///
    /// Input, fuel, and output use slots `0`, `1`, and `2`.
    ///
    /// # Errors
    ///
    /// Returns [`GameplayReject`] when the container, process, fuel, or input
    /// fail closed.
    pub fn start_process(
        &self,
        process: &ProcessId,
        container: ContainerId,
    ) -> Result<CommandOutcomeV1, GameplayReject> {
        let mut inner = self
            .lock_inner()
            .map_err(|_| GameplayReject::StorageCommitMismatch {
                resource: "spine_lock",
            })?;
        inner.start_process(self.storage.kernel(), process, container)
    }

    /// Completes due scheduled processes through a large authoritative tick.
    ///
    /// # Errors
    ///
    /// Returns [`GameplayReject`] when the scheduler rejects the advance.
    pub fn advance_scheduled(&self) -> Result<CommandOutcomeV1, GameplayReject> {
        let mut inner = self
            .lock_inner()
            .map_err(|_| GameplayReject::StorageCommitMismatch {
                resource: "spine_lock",
            })?;
        inner.advance_scheduled(self.storage.kernel())
    }

    /// Transfers a quantity between the local player inventory and a container.
    ///
    /// # Errors
    ///
    /// Returns [`GameplayReject`] when slots, revisions, or quantities fail closed.
    pub fn transfer(&self, command: TransferCommandV1) -> Result<CommandOutcomeV1, GameplayReject> {
        let mut inner = self
            .lock_inner()
            .map_err(|_| GameplayReject::StorageCommitMismatch {
                resource: "spine_lock",
            })?;
        inner.transfer(self.storage.kernel(), command)
    }

    /// Returns a bound container snapshot, when present.
    #[must_use]
    pub fn container(&self, id: ContainerId) -> Option<ContainerStateV1> {
        self.lock_inner().ok().and_then(|inner| {
            inner
                .gameplay
                .as_ref()
                .and_then(|session| session.containers().get(&id).cloned())
        })
    }

    /// Selects or swaps the stack that places the live DDA target block.
    ///
    /// Reuses the latest crosshair hit. Missing stacks fail closed as
    /// [`GameplayReject::EmptySlot`]. This is survival pick, not creative give.
    ///
    /// # Errors
    ///
    /// Returns [`GameplayReject`] when no target is aimed, no matching stack
    /// exists, or the inventory move fails.
    pub fn pick_aimed_block(&self) -> Result<(), GameplayReject> {
        let mut inner = self
            .lock_inner()
            .map_err(|_| GameplayReject::StorageCommitMismatch {
                resource: "spine_lock",
            })?;
        inner.pick_aimed_block(self.storage.kernel())
    }

    /// Typed inventory inspect fragment. UI must not invent a second inventory.
    #[must_use]
    pub fn inventory_inspect(&self) -> Option<InventoryInspectV1> {
        self.lock_inner().ok().and_then(|inner| {
            inner
                .gameplay
                .as_ref()
                .and_then(|session| session.inventory_inspect().ok())
        })
    }

    /// Typed recipe inspect fragments in catalog identity order.
    #[must_use]
    pub fn recipe_inspect(&self, workstation: Option<&WorkstationId>) -> Vec<RecipeInspectV1> {
        self.lock_inner().map_or_else(
            |_| Vec::new(),
            |inner| {
                inner
                    .gameplay
                    .as_ref()
                    .and_then(|session| session.recipe_inspect(workstation).ok())
                    .unwrap_or_default()
            },
        )
    }

    /// Recipe identities currently craftable at `workstation`.
    ///
    /// `None` is hand crafting. Workstation recipes stay empty until that
    /// contract is bound. Order follows the catalog identity map.
    #[must_use]
    pub fn craftable_recipe_ids(&self, workstation: Option<&WorkstationId>) -> Vec<RecipeId> {
        self.lock_inner().map_or_else(
            |_| Vec::new(),
            |inner| {
                inner
                    .gameplay
                    .as_ref()
                    .map(|session| session.craftable_recipe_ids(workstation))
                    .unwrap_or_default()
            },
        )
    }

    /// Catalog workstation contract realized by `block`, when bound.
    #[must_use]
    pub fn block_workstation(&self, block: &BlockId) -> Option<WorkstationId> {
        self.lock_inner().ok().and_then(|inner| {
            inner
                .gameplay
                .as_ref()?
                .catalog()
                .block_schema_binding(block)?
                .workstation
                .clone()
        })
    }

    /// Binds a workstation container on the player inventory chunk.
    ///
    /// # Errors
    ///
    /// Returns [`GameplayReject`] when the workstation is unknown or the
    /// container cannot be seeded.
    pub fn bind_workstation(
        &self,
        workstation: WorkstationId,
        container: ContainerId,
    ) -> Result<(), GameplayReject> {
        let mut inner = self
            .lock_inner()
            .map_err(|_| GameplayReject::StorageCommitMismatch {
                resource: "spine_lock",
            })?;
        let spawn = translation_chunk(inner.spawn_center, inner.chunk_edge).ok_or(
            GameplayReject::ChunkNotLoaded {
                dimension: inner.dimension.as_str().to_owned(),
                x: 0,
                y: 0,
                z: 0,
            },
        )?;
        let chunk = DimensionChunkKey::new(inner.dimension.clone(), spawn);
        inner
            .gameplay
            .as_mut()
            .ok_or(GameplayReject::UnknownPlayer {
                player: local_player_id().as_bytes(),
            })?
            .bind_workstation(workstation, &chunk, container)
    }

    /// Binds a persistent container from a catalog block schema binding.
    ///
    /// # Errors
    ///
    /// Returns [`GameplayReject`] when the block has no container schema or the
    /// container cannot be seeded.
    pub fn bind_block_container(
        &self,
        block: &BlockId,
        container: ContainerId,
    ) -> Result<(), GameplayReject> {
        let mut inner = self
            .lock_inner()
            .map_err(|_| GameplayReject::StorageCommitMismatch {
                resource: "spine_lock",
            })?;
        let spawn = translation_chunk(inner.spawn_center, inner.chunk_edge).ok_or(
            GameplayReject::ChunkNotLoaded {
                dimension: inner.dimension.as_str().to_owned(),
                x: 0,
                y: 0,
                z: 0,
            },
        )?;
        let chunk = DimensionChunkKey::new(inner.dimension.clone(), spawn);
        inner
            .gameplay
            .as_mut()
            .ok_or(GameplayReject::UnknownPlayer {
                player: local_player_id().as_bytes(),
            })?
            .bind_block_container(block, &chunk, container)
    }

    /// Seeds one local inventory slot for fixture setup.
    ///
    /// # Errors
    ///
    /// Returns [`GameplayReject`] when the slot or stack is invalid.
    pub fn seed_inventory_slot(
        &self,
        slot: SlotIndex,
        stack: Option<ItemStackV1>,
    ) -> Result<(), GameplayReject> {
        let mut inner = self
            .lock_inner()
            .map_err(|_| GameplayReject::StorageCommitMismatch {
                resource: "spine_lock",
            })?;
        inner
            .gameplay
            .as_mut()
            .ok_or(GameplayReject::UnknownPlayer {
                player: local_player_id().as_bytes(),
            })?
            .seed_slot(slot, stack)
    }

    /// Returns the first resident cell matching `block`, if any.
    #[must_use]
    pub fn first_resident_block(&self, block: &BlockId) -> Option<BlockPosition> {
        let inner = self.lock_inner().ok()?;
        let wanted = palette_index(&inner.palette, block)?;
        let edge = i32::from(inner.chunk_edge);
        for coordinate in inner.resident_coordinates() {
            for ly in 0..edge {
                for lz in 0..edge {
                    for lx in 0..edge {
                        let position = BlockPosition {
                            x: coordinate.x.checked_mul(edge)?.checked_add(lx)?,
                            y: coordinate.y.checked_mul(edge)?.checked_add(ly)?,
                            z: coordinate.z.checked_mul(edge)?.checked_add(lz)?,
                        };
                        let voxel = VoxelCoordinate::new(
                            i64::from(position.x),
                            i64::from(position.y),
                            i64::from(position.z),
                        );
                        if inner
                            .runtime
                            .cell(voxel)
                            .ok()
                            .is_some_and(|cell| cell.palette_index == wanted)
                        {
                            return Some(position);
                        }
                    }
                }
            }
        }
        None
    }

    /// Returns the first resident `block` that shares a face with a cave void.
    #[must_use]
    pub fn first_cave_adjacent_block(&self, block: &BlockId) -> Option<BlockPosition> {
        let inner = self.lock_inner().ok()?;
        let wanted = palette_index(&inner.palette, block)?;
        let edge = i32::from(inner.chunk_edge);
        for coordinate in inner.resident_coordinates() {
            for ly in 0..edge {
                for lz in 0..edge {
                    for lx in 0..edge {
                        let position = BlockPosition {
                            x: coordinate.x.checked_mul(edge)?.checked_add(lx)?,
                            y: coordinate.y.checked_mul(edge)?.checked_add(ly)?,
                            z: coordinate.z.checked_mul(edge)?.checked_add(lz)?,
                        };
                        let voxel = VoxelCoordinate::new(
                            i64::from(position.x),
                            i64::from(position.y),
                            i64::from(position.z),
                        );
                        if inner
                            .runtime
                            .cell(voxel)
                            .ok()
                            .is_none_or(|cell| cell.palette_index != wanted)
                        {
                            continue;
                        }
                        if cell_faces_cave_void(&inner, position) {
                            return Some(position);
                        }
                    }
                }
            }
        }
        None
    }

    /// Returns the latest crosshair DDA target.
    #[must_use]
    pub fn current_target(&self) -> Option<HeadlessTargetInspectV1> {
        self.lock_inner()
            .ok()
            .and_then(|inner| inner.current_target.clone())
    }

    /// Returns the last inspect-action result.
    #[must_use]
    pub fn last_inspect(&self) -> Option<Result<HeadlessTargetInspectV1, TargetInspectRejectV1>> {
        self.lock_inner()
            .ok()
            .and_then(|inner| inner.last_inspect.clone())
    }

    /// Places one water or lava occupancy cell without running fluid simulation.
    ///
    /// The solid layer is preserved. Placement is rejected when the solid's
    /// authored `fluid_occupancy` policy is `reject`. This path does not open a
    /// world writer.
    ///
    /// # Errors
    ///
    /// Returns [`BlockEditRejectV1`] when the cell is not resident, the fluid is
    /// unknown, occupancy is rejected, or memory publication fails.
    pub fn place_fluid_occupancy(
        &self,
        position: BlockPosition,
        fluid: &StableId,
        state: FluidStateV1,
    ) -> Result<CellOccupancyV1, BlockEditRejectV1> {
        let result = {
            let mut inner = self
                .inner
                .lock()
                .map_err(|_| BlockEditRejectV1::StorageUnavailable)?;
            inner.place_fluid_occupancy(self.storage.kernel(), position, fluid, state)
        };
        finish_edit_readiness(self, FixedTick::new(0), [position])?;
        result
    }

    /// Inspects versioned solid and fluid occupancy at an exact cell.
    ///
    /// # Errors
    ///
    /// Returns [`TargetInspectRejectV1`] when the cell is not resident or the
    /// catalog occupancy row is missing.
    pub fn inspect_occupancy(
        &self,
        position: BlockPosition,
    ) -> Result<CellOccupancyV1, TargetInspectRejectV1> {
        let inner = self
            .inner
            .lock()
            .map_err(|_| TargetInspectRejectV1::StorageUnavailable)?;
        inner.occupancy_at(position)
    }

    /// Plans and applies one bounded water/lava tick inside simulation distance.
    ///
    /// Completions that no longer match the captured chunk revision are
    /// rejected and never applied.
    ///
    /// # Errors
    ///
    /// Returns [`BlockEditRejectV1`] when a plan exceeds its hard bound, mixing
    /// is detected, storage is unavailable, or a completion is stale.
    pub fn tick_bounded_fluids(&self) -> Result<Vec<HostFluidTickV1>, BlockEditRejectV1> {
        let mut inner = self
            .inner
            .lock()
            .map_err(|_| BlockEditRejectV1::StorageUnavailable)?;
        let origin = translation_chunk(inner.player_pose.translation, inner.chunk_edge)
            .ok_or(BlockEditRejectV1::StorageUnavailable)?;
        let simulation_distance = inner.clamps.simulation_distance();
        tick_simulated_fluids(
            &mut inner,
            self.storage.kernel(),
            origin,
            simulation_distance,
        )
    }

    /// Returns the captured fluid revision stamp for a resident chunk.
    #[must_use]
    pub fn fluid_revision_stamp(
        &self,
        coordinate: ChunkCoordinate,
    ) -> Option<latticeaxiom_voxel_runtime::FluidRevisionStamp> {
        let inner = self.lock_inner().ok()?;
        let (world, chunk, voxel) = inner.runtime.chunk_revisions(coordinate)?;
        Some(latticeaxiom_voxel_runtime::FluidRevisionStamp::new(
            world, chunk, voxel,
        ))
    }

    /// Reruns authoritative Y-up DDA and records the inspect result.
    ///
    /// Client observations are ignored as hits. The selected voxel identity
    /// comes from the committed projection.
    ///
    /// # Errors
    ///
    /// Returns [`TargetInspectRejectV1`] when DDA finds no selectable voxel,
    /// the hit is out of reach or occluded, or committed content is missing.
    pub fn inspect(
        &self,
        request: &AuthoritativeTargetInspectRequestV1,
    ) -> Result<HeadlessTargetInspectV1, TargetInspectRejectV1> {
        let mut inner = self
            .inner
            .lock()
            .map_err(|_| TargetInspectRejectV1::StorageUnavailable)?;
        let result = inner.inspect_from_eye(request.eye_pose);
        inner.last_inspect = Some(result.clone());
        inner.current_target = result.as_ref().ok().cloned();
        result
    }

    /// Refreshes the live crosshair target without recording an inspect edge.
    pub fn refresh_target(&self, eye_pose: TargetEyePoseV1) {
        if let Ok(mut inner) = self.lock_inner() {
            inner.current_target = inner.inspect_from_eye(eye_pose).ok();
        }
    }

    /// Returns the latest copied local-player pose.
    #[must_use]
    pub fn player_pose(&self) -> ProductionPlayerPose {
        self.lock_inner().map_or_else(
            |_| ProductionPlayerPose::default(),
            |inner| inner.player_pose,
        )
    }

    /// Maps a voxel to its host chunk using the spine edge.
    #[must_use]
    pub fn chunk_of(&self, position: BlockPosition) -> Option<ChunkCoordinate> {
        let inner = self.lock_inner().ok()?;
        Some(chunk_of(position, inner.chunk_edge))
    }

    pub(super) fn take_presentation(&self) -> Result<PresentationDelta, ProductionHostError> {
        let mut inner = self.lock_inner()?;
        let mesh_dirty = mem::take(&mut inner.mesh_dirty);
        let collider_dirty = mem::take(&mut inner.collider_dirty);
        let removals: Vec<ChunkCoordinate> = mem::take(&mut inner.removed).into_iter().collect();
        let edge = f32::from(inner.chunk_edge);
        let mut mesh_update = Vec::with_capacity(mesh_dirty.len());
        let mut collider_update = Vec::with_capacity(collider_dirty.len());
        for coordinate in mesh_dirty {
            if removals.binary_search(&coordinate).is_ok() {
                continue;
            }
            let Some(derived) = inner.derived.get(&coordinate) else {
                continue;
            };
            let Some(geometry) = derived.geometry.as_ref().map(Arc::clone) else {
                continue;
            };
            mesh_update.push(MeshPresentation {
                coordinate,
                origin: chunk_origin(coordinate, edge),
                geometry,
                bounds: derived.bounds,
            });
        }
        for coordinate in collider_dirty {
            if removals.binary_search(&coordinate).is_ok() {
                continue;
            }
            let Some(derived) = inner.derived.get(&coordinate) else {
                continue;
            };
            let Some(collider) = derived.collider.clone() else {
                continue;
            };
            collider_update.push(ColliderPresentation {
                coordinate,
                origin: chunk_origin(coordinate, edge),
                collider,
            });
        }
        Ok(PresentationDelta {
            mesh_update,
            collider_update,
            removals,
        })
    }

    pub(super) fn record_player_pose(&self, pose: ProductionPlayerPose) {
        if let Ok(mut inner) = self.lock_inner() {
            inner.player_pose = pose;
        }
    }

    fn lock_inner(
        &self,
    ) -> Result<std::sync::MutexGuard<'_, ProductionSpineInner>, ProductionHostError> {
        self.inner.lock().map_err(|_| ProductionHostError::Poisoned)
    }
}

impl BlockEditAuthority for ProductionSpine {
    fn apply(
        &mut self,
        request: AuthoritativeBlockEditRequestV1,
    ) -> Result<BlockEditSuccessV1, BlockEditRejectV1> {
        let tick = FixedTick::new(request.fixed_tick);
        let result = {
            let mut inner = self
                .inner
                .lock()
                .map_err(|_| BlockEditRejectV1::StorageUnavailable)?;
            let result = inner.apply(request, self.storage.kernel());
            match &result {
                Ok(success) => {
                    inner.last_success = Some(success.clone());
                    inner.last_reject = None;
                }
                Err(reject) => inner.last_reject = Some(reject.clone()),
            }
            result
        };
        let target = result.as_ref().ok().map(|success| success.position);
        finish_edit_readiness(self, tick, target)?;
        result
    }
}

fn finish_edit_readiness(
    spine: &ProductionSpine,
    tick: FixedTick,
    positions: impl IntoIterator<Item = BlockPosition>,
) -> Result<(), BlockEditRejectV1> {
    match await_edit_readiness(spine, tick, positions) {
        Ok(()) | Err(ProductionHostError::DerivedReadinessBarrier) => Ok(()),
        Err(_) => Err(BlockEditRejectV1::StorageUnavailable),
    }
}

struct SelectableHit {
    position: BlockPosition,
    face: BlockFaceV1,
    distance: f64,
    revision: ChunkRevision,
    voxel: HostVoxel,
}

impl ProductionSpineInner {
    fn resident_coordinates(&self) -> Vec<ChunkCoordinate> {
        self.lifecycle
            .iter()
            .filter_map(|(coordinate, state)| {
                matches!(
                    state,
                    ChunkLifecycle::Resident
                        | ChunkLifecycle::MeshCollider
                        | ChunkLifecycle::Active
                )
                .then_some(*coordinate)
            })
            .collect()
    }

    #[allow(clippy::too_many_lines)]
    #[allow(clippy::needless_pass_by_value)] // Trait-shaped request is owned by the authority call.
    fn apply(
        &mut self,
        request: AuthoritativeBlockEditRequestV1,
        kernel: &MemoryTransactionKernel,
    ) -> Result<BlockEditSuccessV1, BlockEditRejectV1> {
        let hit = self.selectable_hit(request.eye_pose)?;
        validate_client_observation(
            request.intent.client_observation.as_ref(),
            hit.position,
            hit.face,
            hit.revision,
            hit.distance,
        )?;
        match request.intent.action {
            BlockEditActionV1::Break => self.mine_cell(kernel, hit.position, request.fixed_tick),
            BlockEditActionV1::Place => {
                let adjacent = hit
                    .face
                    .adjacent(hit.position)
                    .ok_or(BlockEditRejectV1::PermissionDenied)?;
                self.place_cell(
                    kernel,
                    adjacent,
                    request.intent.placement_content.as_ref(),
                    request.eye_pose.origin_m,
                    request.fixed_tick,
                )
            }
        }
    }

    fn mine_cell(
        &mut self,
        kernel: &MemoryTransactionKernel,
        position: BlockPosition,
        fixed_tick: u64,
    ) -> Result<BlockEditSuccessV1, BlockEditRejectV1> {
        let voxel = *self
            .runtime
            .cell(VoxelCoordinate::new(
                i64::from(position.x),
                i64::from(position.y),
                i64::from(position.z),
            ))
            .map_err(|_| BlockEditRejectV1::PermissionDenied)?;
        if voxel.palette_index == self.empty.palette_index {
            return Err(BlockEditRejectV1::NotBreakable);
        }
        let Some(block) = self.block_id(voxel) else {
            return Err(BlockEditRejectV1::ContentUnavailable);
        };
        if self
            .gameplay
            .as_ref()
            .is_none_or(|session| session.catalog().block(&block).is_none())
        {
            return self.commit_cell(kernel, fixed_tick, position, voxel, self.empty);
        }
        self.sync_gameplay_world(kernel)
            .map_err(|_| BlockEditRejectV1::StorageUnavailable)?;
        let required_tool = self.gameplay.as_ref().and_then(|session| {
            session.catalog().block(&block).and_then(|definition| {
                definition
                    .mining
                    .tool
                    .as_ref()
                    .map(|tool| tool.class.clone())
            })
        });
        let revision = self
            .runtime
            .chunk_revisions(chunk_of(position, self.chunk_edge))
            .map(|(_, revision, _)| revision)
            .ok_or(BlockEditRejectV1::PermissionDenied)?;
        let command = self
            .gameplay
            .as_mut()
            .ok_or(BlockEditRejectV1::ContentUnavailable)?
            .prepare_mine(position, block.clone(), revision)
            .map_err(|error| block_edit_reject(&error, required_tool.as_ref()))?;
        let transaction = next_transaction_id(self);
        let receipt = self
            .gameplay
            .as_mut()
            .ok_or(BlockEditRejectV1::ContentUnavailable)?
            .execute(transaction, command)
            .map_err(|error| block_edit_reject(&error, required_tool.as_ref()))?;
        let voxel_write = matches!(receipt.outcome, CommandOutcomeV1::BlockBroken { .. })
            .then_some((position, self.empty));
        commit_gameplay_storage(self, kernel, transaction, voxel_write, fixed_tick)?;
        match receipt.outcome {
            CommandOutcomeV1::MiningProgress {
                accumulated,
                required,
            } => Err(BlockEditRejectV1::RequiresProgress {
                remaining_work: remaining_work(accumulated, required),
            }),
            CommandOutcomeV1::BlockBroken { drop, .. } => {
                let _ = self.pickup_drop(kernel, drop);
                let published = kernel
                    .reference_snapshot(self.world)
                    .map_err(|_| BlockEditRejectV1::StorageUnavailable)?;
                let coordinate = chunk_of(position, self.chunk_edge);
                let stored = published
                    .chunk(&ChunkKey::new(
                        self.world,
                        self.dimension.clone(),
                        coordinate,
                    ))
                    .ok_or(BlockEditRejectV1::StorageUnavailable)?;
                Ok(BlockEditSuccessV1 {
                    position,
                    old_content: Some(block),
                    new_content: None,
                    committed_chunk_revision: stored.revision(),
                })
            }
            _ => Err(BlockEditRejectV1::StorageUnavailable),
        }
    }

    fn place_cell(
        &mut self,
        kernel: &MemoryTransactionKernel,
        target: BlockPosition,
        requested_block: Option<&BlockId>,
        eye_origin: [f32; 3],
        fixed_tick: u64,
    ) -> Result<BlockEditSuccessV1, BlockEditRejectV1> {
        if cell_intersects_player(target, eye_origin) {
            return Err(BlockEditRejectV1::WouldIntersectActor);
        }
        let old = *self
            .runtime
            .cell(VoxelCoordinate::new(
                i64::from(target.x),
                i64::from(target.y),
                i64::from(target.z),
            ))
            .map_err(|_| BlockEditRejectV1::PermissionDenied)?;
        if old != self.empty {
            return Err(BlockEditRejectV1::NotReplaceable);
        }
        let has_catalog = self.gameplay.as_ref().is_some_and(|session| {
            session.inventory_view().is_some_and(|view| {
                view.selected()
                    .is_some_and(|stack| session.catalog().item(stack.item()).is_some())
                    || requested_block.is_some_and(|block| {
                        session
                            .catalog()
                            .items()
                            .values()
                            .any(|item| item.placement_block.as_ref() == Some(block))
                    })
            })
        });
        if !has_catalog {
            let placement = requested_block
                .cloned()
                .or_else(|| Some(self.placement_content.clone()))
                .ok_or(BlockEditRejectV1::NoPlacementContent)?;
            let palette_index = palette_index(&self.palette, &placement)
                .ok_or(BlockEditRejectV1::ContentUnavailable)?;
            return self.commit_cell(
                kernel,
                fixed_tick,
                target,
                old,
                HostVoxel::from_solid(palette_index, &self.presentation),
            );
        }
        self.sync_gameplay_world(kernel)
            .map_err(|_| BlockEditRejectV1::StorageUnavailable)?;
        let revision = self
            .runtime
            .chunk_revisions(chunk_of(target, self.chunk_edge))
            .map(|(_, revision, _)| revision)
            .ok_or(BlockEditRejectV1::PermissionDenied)?;
        let (command, block) = self
            .gameplay
            .as_mut()
            .ok_or(BlockEditRejectV1::ContentUnavailable)?
            .prepare_place(target, revision, requested_block)
            .map_err(|error| block_edit_reject(&error, None))?;
        let palette_index =
            palette_index(&self.palette, &block).ok_or(BlockEditRejectV1::ContentUnavailable)?;
        let transaction = next_transaction_id(self);
        let _receipt = self
            .gameplay
            .as_mut()
            .ok_or(BlockEditRejectV1::ContentUnavailable)?
            .execute(transaction, command)
            .map_err(|error| block_edit_reject(&error, None))?;
        commit_gameplay_storage(
            self,
            kernel,
            transaction,
            Some((
                target,
                HostVoxel::from_solid(palette_index, &self.presentation),
            )),
            fixed_tick,
        )?;
        let published = kernel
            .reference_snapshot(self.world)
            .map_err(|_| BlockEditRejectV1::StorageUnavailable)?;
        let coordinate = chunk_of(target, self.chunk_edge);
        let stored = published
            .chunk(&ChunkKey::new(
                self.world,
                self.dimension.clone(),
                coordinate,
            ))
            .ok_or(BlockEditRejectV1::StorageUnavailable)?;
        Ok(BlockEditSuccessV1 {
            position: target,
            old_content: None,
            new_content: Some(block),
            committed_chunk_revision: stored.revision(),
        })
    }

    fn pickup_drop(
        &mut self,
        kernel: &MemoryTransactionKernel,
        drop: DropEntityId,
    ) -> Result<CommandOutcomeV1, GameplayReject> {
        self.sync_gameplay_world(kernel)?;
        let transaction = next_transaction_id(self);
        let receipt = self
            .gameplay
            .as_mut()
            .ok_or(GameplayReject::UnknownPlayer {
                player: local_player_id().as_bytes(),
            })?
            .pickup(transaction, drop)?;
        commit_gameplay_storage(self, kernel, transaction, None, 0)
            .map_err(|_| GameplayReject::StorageCommitMismatch { resource: "pickup" })?;
        Ok(receipt.outcome)
    }

    fn craft_recipe(
        &mut self,
        kernel: &MemoryTransactionKernel,
        recipe: &RecipeId,
        workstation: Option<ContainerId>,
    ) -> Result<CommandOutcomeV1, GameplayReject> {
        self.sync_gameplay_world(kernel)?;
        let transaction = next_transaction_id(self);
        let receipt = self
            .gameplay
            .as_mut()
            .ok_or(GameplayReject::UnknownPlayer {
                player: local_player_id().as_bytes(),
            })?
            .craft(transaction, recipe, workstation)?;
        commit_gameplay_storage(self, kernel, transaction, None, 0)
            .map_err(|_| GameplayReject::StorageCommitMismatch { resource: "craft" })?;
        Ok(receipt.outcome)
    }

    fn move_stack(
        &mut self,
        kernel: &MemoryTransactionKernel,
        from: SlotIndex,
        to: SlotIndex,
    ) -> Result<CommandOutcomeV1, GameplayReject> {
        self.sync_gameplay_world(kernel)?;
        let transaction = next_transaction_id(self);
        let receipt = self
            .gameplay
            .as_mut()
            .ok_or(GameplayReject::UnknownPlayer {
                player: local_player_id().as_bytes(),
            })?
            .move_stack(transaction, from, to)?;
        commit_gameplay_storage(self, kernel, transaction, None, 0).map_err(|_| {
            GameplayReject::StorageCommitMismatch {
                resource: "move_stack",
            }
        })?;
        Ok(receipt.outcome)
    }

    fn start_process(
        &mut self,
        kernel: &MemoryTransactionKernel,
        process: &ProcessId,
        container: ContainerId,
    ) -> Result<CommandOutcomeV1, GameplayReject> {
        self.sync_gameplay_world(kernel)?;
        let revision = self
            .gameplay
            .as_ref()
            .ok_or(GameplayReject::UnknownPlayer {
                player: local_player_id().as_bytes(),
            })?
            .containers()
            .get(&container)
            .ok_or(GameplayReject::UnknownContainer)?
            .revision();
        let transaction = next_transaction_id(self);
        let receipt = self
            .gameplay
            .as_mut()
            .ok_or(GameplayReject::UnknownPlayer {
                player: local_player_id().as_bytes(),
            })?
            .start_process(
                transaction,
                process,
                container,
                revision,
                AuthorityTick::new(0),
            )?;
        commit_gameplay_storage(self, kernel, transaction, None, 0).map_err(|_| {
            GameplayReject::StorageCommitMismatch {
                resource: "start_process",
            }
        })?;
        Ok(receipt.outcome)
    }

    fn advance_scheduled(
        &mut self,
        kernel: &MemoryTransactionKernel,
    ) -> Result<CommandOutcomeV1, GameplayReject> {
        self.sync_gameplay_world(kernel)?;
        let transaction = next_transaction_id(self);
        let max_completions = NonZeroU16::new(16).unwrap_or(NonZeroU16::MIN);
        let receipt = self
            .gameplay
            .as_mut()
            .ok_or(GameplayReject::UnknownPlayer {
                player: local_player_id().as_bytes(),
            })?
            .advance_scheduled(transaction, AuthorityTick::new(10_000), max_completions)?;
        commit_gameplay_storage(self, kernel, transaction, None, 0).map_err(|_| {
            GameplayReject::StorageCommitMismatch {
                resource: "advance_scheduled",
            }
        })?;
        Ok(receipt.outcome)
    }

    fn transfer(
        &mut self,
        kernel: &MemoryTransactionKernel,
        command: TransferCommandV1,
    ) -> Result<CommandOutcomeV1, GameplayReject> {
        self.sync_gameplay_world(kernel)?;
        let transaction = next_transaction_id(self);
        let receipt = self
            .gameplay
            .as_mut()
            .ok_or(GameplayReject::UnknownPlayer {
                player: local_player_id().as_bytes(),
            })?
            .transfer(transaction, command)?;
        commit_gameplay_storage(self, kernel, transaction, None, 0).map_err(|_| {
            GameplayReject::StorageCommitMismatch {
                resource: "transfer",
            }
        })?;
        Ok(receipt.outcome)
    }

    fn pick_aimed_block(&mut self, kernel: &MemoryTransactionKernel) -> Result<(), GameplayReject> {
        let aimed = self
            .current_target
            .as_ref()
            .map(|target| target.block_id.clone())
            .ok_or(GameplayReject::EmptySlot)?;
        self.sync_gameplay_world(kernel)?;
        let transaction = next_transaction_id(self);
        let moved = self
            .gameplay
            .as_mut()
            .ok_or(GameplayReject::UnknownPlayer {
                player: local_player_id().as_bytes(),
            })?
            .pick_aimed_block(transaction, &aimed)?;
        if moved.is_some() {
            commit_gameplay_storage(self, kernel, transaction, None, 0).map_err(|_| {
                GameplayReject::StorageCommitMismatch {
                    resource: "pick_block",
                }
            })?;
        }
        Ok(())
    }

    fn sync_gameplay_world(
        &mut self,
        kernel: &MemoryTransactionKernel,
    ) -> Result<(), GameplayReject> {
        let snapshot = kernel.reference_snapshot(self.world).map_err(|_| {
            GameplayReject::StorageCommitMismatch {
                resource: "snapshot",
            }
        })?;
        let mut loaded = BTreeMap::new();
        for (key, stored) in snapshot.chunks() {
            loaded.insert(
                DimensionChunkKey::new(key.dimension.clone(), key.coordinate),
                stored.revision(),
            );
        }
        if let Some(gameplay) = self.gameplay.as_mut() {
            gameplay.sync_loaded_world(snapshot.revision(), loaded)?;
        }
        Ok(())
    }

    pub(super) fn commit_cell(
        &mut self,
        kernel: &MemoryTransactionKernel,
        fixed_tick: u64,
        position: BlockPosition,
        old_voxel: HostVoxel,
        new_voxel: HostVoxel,
    ) -> Result<BlockEditSuccessV1, BlockEditRejectV1> {
        let coordinate = chunk_of(position, self.chunk_edge);
        let key = ChunkKey::new(self.world, self.dimension.clone(), coordinate);
        let snapshot = kernel
            .reference_snapshot(self.world)
            .map_err(|_| BlockEditRejectV1::StorageUnavailable)?;
        let stored = snapshot
            .chunk(&key)
            .ok_or(BlockEditRejectV1::PermissionDenied)?;
        let mut cells = decode_cells(
            stored.data().voxels().bytes(),
            self.chunk_edge,
            &self.presentation,
        )
        .map_err(|_| BlockEditRejectV1::StorageUnavailable)?;
        let local = local_index(position, self.chunk_edge);
        let index = canonical_index(usize::from(self.chunk_edge), local[0], local[1], local[2]);
        let Some(cell) = cells.get_mut(index) else {
            return Err(BlockEditRejectV1::StorageUnavailable);
        };
        *cell = new_voxel;
        let transaction = WorldTransaction::new(
            TransactionId::from_u128(self.next_transaction),
            self.world,
            snapshot.revision(),
            vec![ChunkMutation::new(
                key.clone(),
                ChunkRevisionExpectation::Exact(stored.revision()),
                ChangedDomains::VOXELS,
                chunk_data(&self.voxel_schema, self.voxel_schema_version, &cells),
            )],
        );
        kernel
            .commit(transaction)
            .map_err(|_| BlockEditRejectV1::StorageUnavailable)?;
        self.next_transaction = self.next_transaction.saturating_add(1);
        let published = kernel
            .reference_snapshot(self.world)
            .map_err(|_| BlockEditRejectV1::StorageUnavailable)?;
        let stored = published
            .chunk(&key)
            .ok_or(BlockEditRejectV1::StorageUnavailable)?;
        self.edited.insert(coordinate);
        project_stored(
            &mut self.runtime,
            stored,
            self.chunk_edge,
            FixedTick::new(fixed_tick),
            &self.presentation,
            derived_requests(priority_with_distance(DerivedPriority::EDIT_TO_VISIBLE, 0)),
        )
        .map_err(|_| BlockEditRejectV1::StorageUnavailable)?;
        seal_unready_cave_voids(self, coordinate);
        self.lifecycle
            .entry(coordinate)
            .and_modify(|state| {
                if *state == ChunkLifecycle::Active {
                    *state = ChunkLifecycle::MeshCollider;
                }
            })
            .or_insert(ChunkLifecycle::Resident);
        Ok(BlockEditSuccessV1 {
            position,
            old_content: self.block_id(old_voxel),
            new_content: self.block_id(new_voxel),
            committed_chunk_revision: stored.revision(),
        })
    }

    pub(super) fn commit_chunk_voxels(
        &mut self,
        kernel: &MemoryTransactionKernel,
        coordinate: ChunkCoordinate,
        cells: &[HostVoxel],
    ) -> Result<(), BlockEditRejectV1> {
        let key = ChunkKey::new(self.world, self.dimension.clone(), coordinate);
        let snapshot = kernel
            .reference_snapshot(self.world)
            .map_err(|_| BlockEditRejectV1::StorageUnavailable)?;
        let stored = snapshot
            .chunk(&key)
            .ok_or(BlockEditRejectV1::PermissionDenied)?;
        let transaction = WorldTransaction::new(
            TransactionId::from_u128(self.next_transaction),
            self.world,
            snapshot.revision(),
            vec![ChunkMutation::new(
                key.clone(),
                ChunkRevisionExpectation::Exact(stored.revision()),
                ChangedDomains::VOXELS,
                chunk_data(&self.voxel_schema, self.voxel_schema_version, cells),
            )],
        );
        kernel
            .commit(transaction)
            .map_err(|_| BlockEditRejectV1::StorageUnavailable)?;
        self.next_transaction = self.next_transaction.saturating_add(1);
        let published = kernel
            .reference_snapshot(self.world)
            .map_err(|_| BlockEditRejectV1::StorageUnavailable)?;
        let stored = published
            .chunk(&key)
            .ok_or(BlockEditRejectV1::StorageUnavailable)?;
        self.edited.insert(coordinate);
        project_stored(
            &mut self.runtime,
            stored,
            self.chunk_edge,
            FixedTick::new(0),
            &self.presentation,
            derived_requests(priority_with_distance(DerivedPriority::EDIT_TO_VISIBLE, 0)),
        )
        .map_err(|_| BlockEditRejectV1::StorageUnavailable)?;
        seal_unready_cave_voids(self, coordinate);
        self.lifecycle
            .entry(coordinate)
            .and_modify(|state| {
                if *state == ChunkLifecycle::Active {
                    *state = ChunkLifecycle::MeshCollider;
                }
            })
            .or_insert(ChunkLifecycle::Resident);
        Ok(())
    }

    fn block_id(&self, voxel: HostVoxel) -> Option<BlockId> {
        if voxel == self.empty {
            None
        } else {
            self.palette.get(usize::from(voxel.palette_index)).cloned()
        }
    }

    fn place_fluid_occupancy(
        &mut self,
        kernel: &MemoryTransactionKernel,
        position: BlockPosition,
        fluid: &StableId,
        state: FluidStateV1,
    ) -> Result<CellOccupancyV1, BlockEditRejectV1> {
        let current = *self
            .runtime
            .cell(VoxelCoordinate::new(
                i64::from(position.x),
                i64::from(position.y),
                i64::from(position.z),
            ))
            .map_err(|_| BlockEditRejectV1::PermissionDenied)?;
        let occupancy = self
            .occupancy_from_voxel(position, current)
            .map_err(|_| BlockEditRejectV1::ContentUnavailable)?;
        let fluid_policy = FluidOccupancyPolicyV1::new(occupancy.fluid_occupancy.clone())
            .map_err(|_| BlockEditRejectV1::ContentUnavailable)?;
        let fluid_kind = FluidOccupancyKindV1::classify(&fluid_policy)
            .map_err(|_| BlockEditRejectV1::ContentUnavailable)?;
        if matches!(fluid_kind, FluidOccupancyKindV1::Reject) {
            return Err(BlockEditRejectV1::NotReplaceable);
        }
        let fluid_palette_index = fluid_palette_index(&self.fluid_palette, fluid, state)
            .ok_or(BlockEditRejectV1::ContentUnavailable)?;
        let placed = HostVoxel::occupancy(
            current.palette_index,
            fluid_palette_index,
            &self.presentation,
        );
        self.commit_cell(kernel, 0, position, current, placed)?;
        self.occupancy_from_voxel(position, placed)
            .map_err(|_| BlockEditRejectV1::ContentUnavailable)
    }

    fn occupancy_at(
        &self,
        position: BlockPosition,
    ) -> Result<CellOccupancyV1, TargetInspectRejectV1> {
        let voxel = *self
            .runtime
            .cell(VoxelCoordinate::new(
                i64::from(position.x),
                i64::from(position.y),
                i64::from(position.z),
            ))
            .map_err(|_| TargetInspectRejectV1::StorageUnavailable)?;
        self.occupancy_from_voxel(position, voxel)
    }

    fn occupancy_from_voxel(
        &self,
        position: BlockPosition,
        voxel: HostVoxel,
    ) -> Result<CellOccupancyV1, TargetInspectRejectV1> {
        let solid = self.palette.get(usize::from(voxel.palette_index)).cloned();
        let solid_id = solid
            .as_ref()
            .map(|block| block.as_str().parse::<StableId>())
            .transpose()
            .map_err(|_| TargetInspectRejectV1::ContentUnavailable)?;
        let semantics = solid_id
            .as_ref()
            .and_then(|id| self.block_semantics(id))
            .ok_or(TargetInspectRejectV1::ContentUnavailable)?;
        let fluid_entry = self
            .fluid_palette
            .entries()
            .get(usize::from(voxel.fluid_palette_index));
        let (fluid, fluid_state) = match fluid_entry {
            Some(entry) if !entry.is_empty() => (entry.fluid().cloned(), entry.state().copied()),
            _ => (None, None),
        };
        let (collision_policy, selection_policy) = if let Some(fluid_id) = fluid.as_ref() {
            let definition = self
                .content
                .fluid(fluid_id)
                .ok_or(TargetInspectRejectV1::ContentUnavailable)?;
            (
                definition.collision_policy.as_stable_id().clone(),
                definition.selection_policy.as_stable_id().clone(),
            )
        } else {
            (
                semantics.collision.as_stable_id().clone(),
                semantics.selection.as_stable_id().clone(),
            )
        };
        Ok(CellOccupancyV1 {
            position,
            solid,
            solid_occupancy: semantics.solid_occupancy.as_stable_id().clone(),
            fluid_occupancy: semantics.fluid_occupancy.as_stable_id().clone(),
            fluid,
            fluid_state,
            collision_policy,
            selection_policy,
        })
    }

    pub(super) fn block_semantics(
        &self,
        block: &StableId,
    ) -> Option<&latticeaxiom_content::BlockStateSemanticsV1> {
        let definition = self.content.block(block)?;
        let state = self
            .solid_palette
            .entries()
            .iter()
            .find(|entry| entry.block() == block)
            .map_or(
                &definition.definition().default_state,
                CompiledSolidPaletteEntryV1::state,
            );
        definition.states().iter().find(|row| &row.state == state)
    }

    fn inspect_from_eye(
        &self,
        eye_pose: TargetEyePoseV1,
    ) -> Result<HeadlessTargetInspectV1, TargetInspectRejectV1> {
        let hit = self
            .selectable_hit(eye_pose)
            .map_err(|reject| map_inspect_reject(&reject))?;
        let block_id = self
            .block_id(hit.voxel)
            .ok_or(TargetInspectRejectV1::ContentUnavailable)?;
        let occupancy = WorkingSetDiagnosticsV1::from_runtime(self.runtime.diagnostics());
        let label = self.display.lookup(block_id.as_str());
        let mut inspect = HeadlessTargetInspectV1::new(
            ClientTargetObservationV1 {
                position: hit.position,
                face: hit.face,
                chunk_revision: hit.revision,
                distance_mm: quantized_distance_mm(hit.distance),
            },
            block_id,
            chunk_of(hit.position, self.chunk_edge),
            occupancy.resident(),
            occupancy.active(),
            occupancy.in_flight(),
            occupancy.dirty(),
        );
        inspect.block_display_name = label.name;
        inspect.block_display_icon = label.icon;
        inspect.declared_by = inspect.block_id.namespace().to_owned();
        if let Some(gameplay) = &self.gameplay
            && let Some(harvest) = gameplay.catalog().mining_inspect(&inspect.block_id)
        {
            inspect.apply_mining_inspect(&harvest);
        }
        Ok(inspect)
    }

    fn selectable_hit(
        &self,
        eye_pose: TargetEyePoseV1,
    ) -> Result<SelectableHit, BlockEditRejectV1> {
        let origin =
            dda_origin(eye_pose.origin_m, self.chunk_edge).ok_or(BlockEditRejectV1::NoTarget)?;
        let query = DdaQuery::new(
            origin,
            eye_pose.forward.map(f64::from),
            f64::from(MAX_BLOCK_EDIT_REACH_M),
        );
        let outcome = self
            .runtime
            .raycast_committed(query, |voxel| {
                if voxel.collision_occupied() {
                    CellSelection::Target
                } else {
                    CellSelection::PassThrough
                }
            })
            .map_err(|_| BlockEditRejectV1::StorageUnavailable)?;
        let hit = match outcome {
            DdaOutcome::Hit(cell) => cell,
            DdaOutcome::Occluded(_) => return Err(BlockEditRejectV1::Occluded),
            DdaOutcome::Unavailable(_) | DdaOutcome::NoTarget => {
                return Err(BlockEditRejectV1::NoTarget);
            }
        };
        if hit.distance() > f64::from(MAX_BLOCK_EDIT_REACH_M) {
            return Err(BlockEditRejectV1::OutOfReach {
                maximum_mm: REACH_MM,
            });
        }
        let position = block_position(hit.coordinate()).ok_or(BlockEditRejectV1::NoTarget)?;
        let face = hit
            .entered_face()
            .map(block_face)
            .ok_or(BlockEditRejectV1::NoTarget)?;
        let voxel = *self
            .runtime
            .cell(hit.coordinate())
            .map_err(|_| BlockEditRejectV1::NoTarget)?;
        Ok(SelectableHit {
            position,
            face,
            distance: hit.distance(),
            revision: hit.revision(),
            voxel,
        })
    }
}

fn map_inspect_reject(reject: &BlockEditRejectV1) -> TargetInspectRejectV1 {
    match reject {
        BlockEditRejectV1::NoTarget => TargetInspectRejectV1::NoTarget,
        BlockEditRejectV1::OutOfReach { maximum_mm } => TargetInspectRejectV1::OutOfReach {
            maximum_mm: *maximum_mm,
        },
        BlockEditRejectV1::Occluded => TargetInspectRejectV1::Occluded,
        BlockEditRejectV1::ContentUnavailable => TargetInspectRejectV1::ContentUnavailable,
        BlockEditRejectV1::StorageUnavailable
        | BlockEditRejectV1::StaleRevision { .. }
        | BlockEditRejectV1::NotBreakable
        | BlockEditRejectV1::NotReplaceable
        | BlockEditRejectV1::WouldIntersectActor
        | BlockEditRejectV1::RequiresTool { .. }
        | BlockEditRejectV1::ToolBroken
        | BlockEditRejectV1::RequiresProgress { .. }
        | BlockEditRejectV1::NoPlacementContent
        | BlockEditRejectV1::PermissionDenied
        | BlockEditRejectV1::Cooldown { .. } => TargetInspectRejectV1::StorageUnavailable,
    }
}

#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn quantized_distance_mm(distance_m: f64) -> u16 {
    let millimeters = (distance_m * 1_000.0).round();
    if !millimeters.is_finite() || millimeters <= 0.0 {
        0
    } else if millimeters >= f64::from(u16::MAX) {
        u16::MAX
    } else {
        millimeters as u16
    }
}

impl RetainedBytes for HostVoxel {
    fn retained_bytes(&self) -> u64 {
        u64::try_from(mem::size_of::<Self>()).unwrap_or(u64::MAX)
    }
}

impl CollisionSemantics for HostVoxel {
    fn collision_occupied(&self) -> bool {
        self.collision_occupied
    }
}

impl Voxel for HostVoxel {
    type MergeKey = LayerMergeKey;

    fn face(&self, face: Face) -> Option<FaceDescriptor<LayerMergeKey>> {
        if let Some(style) = self.solid_style {
            return Some(style.descriptor(face));
        }
        self.fluid_style.map(|style| style.descriptor(face))
    }
}

impl RetainedBytes for HostDerivedMesh {
    fn retained_bytes(&self) -> u64 {
        4_096_u64.saturating_add(
            u64::try_from(self.geometry.quad_count().saturating_mul(48)).unwrap_or(u64::MAX),
        )
    }
}

impl RetainedBytes for HostDerivedCollider {
    fn retained_bytes(&self) -> u64 {
        4_096_u64.saturating_add(
            u64::try_from(self.occupied.len().saturating_mul(16)).unwrap_or(u64::MAX),
        )
    }
}

fn fill_working_set(
    inner: &mut ProductionSpineInner,
    kernel: &MemoryTransactionKernel,
    origin: ChunkCoordinate,
    look_ahead: [i32; 2],
    tick: FixedTick,
) -> Result<(), ProductionHostError> {
    let limit = inner.clamps.max_resident();
    for _ in 0..limit {
        let desired = desired_chunks(origin, inner.clamps, look_ahead, &inner.edited);
        if desired
            .iter()
            .all(|coordinate| inner.runtime.is_resident(*coordinate))
        {
            sync_working_set(
                inner,
                kernel,
                origin,
                look_ahead,
                tick,
                WorldgenAdmission {
                    limit,
                    execution: WorldgenExecution::Blocking,
                },
            )?;
            return Ok(());
        }
        let before = inner.runtime.diagnostics().resident_chunks();
        sync_working_set(
            inner,
            kernel,
            origin,
            look_ahead,
            tick,
            WorldgenAdmission {
                limit,
                execution: WorldgenExecution::Blocking,
            },
        )?;
        if inner.runtime.diagnostics().resident_chunks() == before {
            break;
        }
    }
    Ok(())
}

fn sync_working_set(
    inner: &mut ProductionSpineInner,
    kernel: &MemoryTransactionKernel,
    origin: ChunkCoordinate,
    look_ahead: [i32; 2],
    tick: FixedTick,
    admission: WorldgenAdmission,
) -> Result<(), ProductionHostError> {
    let desired = desired_chunks(origin, inner.clamps, look_ahead, &inner.edited);
    refresh_residency(inner, origin, look_ahead, tick);
    let needs_mutate = inner.last_desired != desired
        || desired
            .iter()
            .any(|coordinate| !inner.runtime.is_resident(*coordinate));
    if needs_mutate {
        evict_unwanted(inner, origin, look_ahead, &desired, tick, false)?;
        let needs_capacity = desired
            .iter()
            .any(|coordinate| !inner.runtime.is_resident(*coordinate))
            && inner.runtime.diagnostics().resident_chunks() >= inner.clamps.max_resident();
        if needs_capacity {
            // Retain is a latency optimization, not permission to deadlock
            // admission at the hard resident cap. Dirty and pinned chunks stay
            // protected; only clean retained chunks may be released here.
            evict_unwanted(inner, origin, look_ahead, &desired, tick, true)?;
        }
        let ordered = prioritize_chunks(&desired, origin, look_ahead);
        admit_desired(inner, kernel, origin, look_ahead, &ordered, tick, admission)?;
        inner.last_desired.clone_from(&desired);
    }
    Ok(())
}

fn refresh_residency(
    inner: &mut ProductionSpineInner,
    origin: ChunkCoordinate,
    look_ahead: [i32; 2],
    tick: FixedTick,
) {
    let now = tick.get();
    let coordinates = inner.lifecycle.keys().copied().collect::<Vec<_>>();
    for coordinate in coordinates {
        let class = interest_class(coordinate, origin, inner.clamps, look_ahead, &inner.edited);
        let residency = inner.residency.entry(coordinate).or_insert(ChunkResidency {
            admitted_tick: now,
            last_core_tick: None,
        });
        match class {
            InterestClass::Pin | InterestClass::Core => {
                residency.last_core_tick = Some(now);
            }
            InterestClass::Retain | InterestClass::Prefetch => {}
        }
    }
}

fn evict_unwanted(
    inner: &mut ProductionSpineInner,
    origin: ChunkCoordinate,
    look_ahead: [i32; 2],
    desired: &BTreeSet<ChunkCoordinate>,
    tick: FixedTick,
    force_retain_release: bool,
) -> Result<(), ProductionHostError> {
    let now = tick.get();
    let mut victims = inner
        .lifecycle
        .keys()
        .copied()
        .filter(|coordinate| {
            if desired.contains(coordinate)
                || inner.edited.contains(coordinate)
                || inner.runtime.is_dirty(*coordinate)
            {
                return false;
            }
            if matches!(
                interest_class(*coordinate, origin, inner.clamps, look_ahead, &inner.edited),
                InterestClass::Pin | InterestClass::Core
            ) {
                return false;
            }
            force_retain_release
                || inner.residency.get(coordinate).is_none_or(|residency| {
                    !retain_protected(now, residency.admitted_tick, residency.last_core_tick)
                })
        })
        .collect::<Vec<_>>();
    victims.sort_by_key(|coordinate| {
        let was_core = inner
            .residency
            .get(coordinate)
            .and_then(|residency| residency.last_core_tick)
            .is_some();
        (
            u8::from(was_core),
            std::cmp::Reverse(chebyshev_xz(*coordinate, origin)),
            coordinate.x,
            coordinate.y,
            coordinate.z,
        )
    });
    for coordinate in victims {
        if !inner.runtime.is_resident(coordinate) {
            forget_chunk(inner, coordinate);
            inner.stream_evictions = inner.stream_evictions.saturating_add(1);
            continue;
        }
        inner.eviction_lease = inner.eviction_lease.saturating_add(1);
        let permit = inner.runtime.prepare_eviction(
            coordinate,
            EvictionLeaseGeneration::new(inner.eviction_lease),
        )?;
        inner.runtime.evict_committed(
            permit,
            tick,
            derived_requests(priority_with_distance(DerivedPriority::RETAIN, 0)),
        )?;
        forget_chunk(inner, coordinate);
        inner.stream_evictions = inner.stream_evictions.saturating_add(1);
    }
    Ok(())
}

fn admit_desired(
    inner: &mut ProductionSpineInner,
    kernel: &MemoryTransactionKernel,
    origin: ChunkCoordinate,
    look_ahead: [i32; 2],
    ordered: &[ChunkCoordinate],
    tick: FixedTick,
    admission: WorldgenAdmission,
) -> Result<(), ProductionHostError> {
    let mut generate = Vec::new();
    let mut hydrate = Vec::new();
    let mut admitted = 0_usize;
    let snapshot = kernel.reference_snapshot(inner.world)?;
    let world_view = inner
        .world_store
        .as_ref()
        .map(|storage| storage.begin_read(inner.world))
        .transpose()?;
    let admit_limit = admission
        .limit
        .max(1)
        .saturating_sub(worldgen_work_count(inner));
    let high_water = inner.clamps.prefetch_high_water();
    for coordinate in ordered {
        if inner.runtime.is_resident(*coordinate) || inner.worldgen_tickets.contains_key(coordinate)
        {
            continue;
        }
        let upcoming = inner
            .runtime
            .diagnostics()
            .resident_chunks()
            .saturating_add(worldgen_work_count(inner))
            .saturating_add(generate.len())
            .saturating_add(hydrate.len());
        let class = interest_class(*coordinate, origin, inner.clamps, look_ahead, &inner.edited);
        if class == InterestClass::Prefetch && upcoming >= high_water {
            continue;
        }
        if upcoming >= inner.clamps.max_resident() || admitted >= admit_limit {
            break;
        }
        let key = ChunkKey::new(inner.world, inner.dimension.clone(), *coordinate);
        if let Some(stored) = snapshot.chunk(&key) {
            inner.lifecycle.insert(*coordinate, ChunkLifecycle::Load);
            project_stored(
                &mut inner.runtime,
                stored,
                inner.chunk_edge,
                tick,
                &inner.presentation,
                stream_derived_requests(class, chebyshev_xz(*coordinate, origin)),
            )?;
            inner
                .lifecycle
                .insert(*coordinate, ChunkLifecycle::Resident);
            seal_unready_cave_voids(inner, *coordinate);
            remember_admission(inner, *coordinate, class, tick);
            admitted = admitted.saturating_add(1);
            continue;
        }
        if let Some(persisted) = world_view
            .as_ref()
            .map(|view| view.load_chunk(&key))
            .transpose()?
            .flatten()
        {
            inner.lifecycle.insert(*coordinate, ChunkLifecycle::Load);
            hydrate.push((*coordinate, persisted));
            remember_admission(inner, *coordinate, class, tick);
            admitted = admitted.saturating_add(1);
            continue;
        }
        if generate.len() >= MAX_BOUNDED_REGION_CHUNKS {
            break;
        }
        inner
            .lifecycle
            .insert(*coordinate, ChunkLifecycle::Generate);
        generate.push(*coordinate);
        remember_admission(inner, *coordinate, class, tick);
        admitted = admitted.saturating_add(1);
    }
    if !hydrate.is_empty() {
        publish_hydrated(inner, kernel, &hydrate, tick, origin, look_ahead)?;
    }
    if generate.is_empty() {
        return Ok(());
    }
    match admission.execution {
        WorldgenExecution::Blocking => {
            publish_generated(inner, kernel, &generate, tick, origin, look_ahead)
        }
        WorldgenExecution::Deferred => {
            queue_worldgen(inner, generate);
            Ok(())
        }
    }
}

fn worldgen_work_count(inner: &ProductionSpineInner) -> usize {
    inner
        .pending_worldgen
        .len()
        .saturating_add(inner.in_flight_worldgen_tasks.len())
        .saturating_add(inner.waiting_worldgen.len())
}

fn queue_worldgen(inner: &mut ProductionSpineInner, coordinates: Vec<ChunkCoordinate>) {
    for coordinate in coordinates {
        if inner.worldgen_tickets.contains_key(&coordinate) {
            continue;
        }
        let ticket = WorldgenTicket(inner.next_worldgen_sequence);
        inner.next_worldgen_sequence = inner.next_worldgen_sequence.saturating_add(1);
        inner.worldgen_tickets.insert(coordinate, ticket);
        inner.pending_worldgen.push_back(WorldgenInput {
            ticket,
            coordinate,
            materialization: Arc::clone(&inner.worldgen_materialization),
        });
    }
}

fn compute_worldgen(input: &WorldgenInput) -> ComputedWorldgen {
    let result =
        generate_plan_chunks(&input.materialization.plan, [input.coordinate]).and_then(|region| {
            let candidate = region.candidate(input.coordinate).ok_or(
                ProductionHostError::MissingGeneratedChunk {
                    coordinate: input.coordinate,
                },
            )?;
            let mut cells = draft_cells(
                candidate.draft(),
                &input.materialization.palette,
                &input.materialization.presentation,
            )?;
            apply_hydrology_occupancy(&input.materialization, input.coordinate, &mut cells)?;
            Ok(cells)
        });
    ComputedWorldgen {
        ticket: input.ticket,
        coordinate: input.coordinate,
        result,
    }
}

fn spawn_worldgen_jobs(spine: &ProductionSpine) -> Result<(), ProductionHostError> {
    let inputs = {
        let mut inner = spine.lock_inner()?;
        inner.pending_worldgen.drain(..).collect::<Vec<_>>()
    };
    if inputs.is_empty() {
        return Ok(());
    }
    if let Some(pool) = AsyncComputeTaskPool::try_get() {
        let mut tasks = inputs
            .into_iter()
            .map(|input| pool.spawn(async move { compute_worldgen(&input) }))
            .collect::<Vec<_>>();
        let mut inner = spine.lock_inner()?;
        inner.in_flight_worldgen_tasks.append(&mut tasks);
    } else {
        let completed = inputs.iter().map(compute_worldgen).collect::<Vec<_>>();
        let mut inner = spine.lock_inner()?;
        enqueue_waiting_worldgen(&mut inner, completed);
    }
    Ok(())
}

fn poll_worldgen_tasks(spine: &ProductionSpine) -> Result<(), ProductionHostError> {
    let tasks = {
        let mut inner = spine.lock_inner()?;
        mem::take(&mut inner.in_flight_worldgen_tasks)
    };
    if tasks.iter().any(|task| !task.is_finished()) {
        tick_compute_pool();
    }
    let mut remaining = Vec::new();
    let mut completed = Vec::new();
    for task in tasks {
        if task.is_finished() {
            completed.push(block_on(task));
        } else {
            remaining.push(task);
        }
    }
    let mut inner = spine.lock_inner()?;
    inner.in_flight_worldgen_tasks = remaining;
    enqueue_waiting_worldgen(&mut inner, completed);
    Ok(())
}

fn enqueue_waiting_worldgen(inner: &mut ProductionSpineInner, completed: Vec<ComputedWorldgen>) {
    for job in completed {
        inner.waiting_worldgen.insert(job.ticket, job);
    }
}

fn apply_ready_worldgen(
    inner: &mut ProductionSpineInner,
    kernel: &MemoryTransactionKernel,
    tick: FixedTick,
    origin: ChunkCoordinate,
    look_ahead: [i32; 2],
) -> Result<(), ProductionHostError> {
    let mut published = 0_usize;
    while published < WORLDGEN_APPLY_JOB_CAP {
        let ticket = WorldgenTicket(inner.next_worldgen_apply_sequence);
        let Some(job) = inner.waiting_worldgen.remove(&ticket) else {
            break;
        };
        inner.next_worldgen_apply_sequence = inner.next_worldgen_apply_sequence.saturating_add(1);
        if inner.worldgen_tickets.get(&job.coordinate) != Some(&ticket) {
            continue;
        }
        let result = match job.result {
            Ok(cells) => publish_generated_cells(
                inner,
                kernel,
                &[job.coordinate],
                &BTreeMap::from([(job.coordinate, cells)]),
                tick,
                origin,
                look_ahead,
            ),
            Err(error) => Err(error),
        };
        inner.worldgen_tickets.remove(&job.coordinate);
        if let Err(error) = result {
            if inner.lifecycle.get(&job.coordinate) == Some(&ChunkLifecycle::Generate) {
                inner.lifecycle.remove(&job.coordinate);
                inner.residency.remove(&job.coordinate);
            }
            return Err(error);
        }
        published = published.saturating_add(1);
    }
    Ok(())
}

fn publish_hydrated(
    inner: &mut ProductionSpineInner,
    kernel: &MemoryTransactionKernel,
    chunks: &[(ChunkCoordinate, PersistedChunkV1)],
    tick: FixedTick,
    origin: ChunkCoordinate,
    look_ahead: [i32; 2],
) -> Result<(), ProductionHostError> {
    let mut remaining = chunks.iter().collect::<Vec<_>>();
    remaining.sort_by_key(|(coordinate, _)| *coordinate);
    let max_chunks = usize::try_from(kernel.limits().max_chunks_per_transaction())
        .unwrap_or(1)
        .max(1);
    while !remaining.is_empty() {
        let snapshot = kernel.reference_snapshot(inner.world)?;
        let mut mutations = Vec::new();
        let mut take = 0_usize;
        for (coordinate, persisted) in remaining.iter().take(max_chunks) {
            take = take.saturating_add(1);
            let key = ChunkKey::new(inner.world, inner.dimension.clone(), *coordinate);
            if snapshot.chunk(&key).is_some() {
                continue;
            }
            mutations.push(ChunkMutation::new(
                key,
                ChunkRevisionExpectation::Absent,
                ChangedDomains::ALL,
                persisted.data().clone(),
            ));
        }
        remaining.drain(..take);
        if mutations.is_empty() {
            continue;
        }
        kernel.commit(WorldTransaction::new(
            next_transaction_id(inner),
            inner.world,
            snapshot.revision(),
            mutations,
        ))?;
    }
    let published = kernel.reference_snapshot(inner.world)?;
    for (coordinate, _) in chunks {
        let key = ChunkKey::new(inner.world, inner.dimension.clone(), *coordinate);
        let stored = published
            .chunk(&key)
            .ok_or(ProductionHostError::MissingStoredChunk {
                coordinate: *coordinate,
            })?;
        inner.lifecycle.insert(*coordinate, ChunkLifecycle::Load);
        let class = interest_class(*coordinate, origin, inner.clamps, look_ahead, &inner.edited);
        let requests = stream_derived_requests(class, chebyshev_xz(*coordinate, origin));
        project_stored(
            &mut inner.runtime,
            stored,
            inner.chunk_edge,
            tick,
            &inner.presentation,
            requests,
        )?;
        inner
            .lifecycle
            .insert(*coordinate, ChunkLifecycle::Resident);
        seal_unready_cave_voids(inner, *coordinate);
    }
    Ok(())
}

fn publish_generated(
    inner: &mut ProductionSpineInner,
    kernel: &MemoryTransactionKernel,
    coordinates: &[ChunkCoordinate],
    tick: FixedTick,
    origin: ChunkCoordinate,
    look_ahead: [i32; 2],
) -> Result<(), ProductionHostError> {
    let region = generate_plan_chunks(&inner.plan, coordinates.iter().copied())?;
    publish_generated_region(
        inner,
        kernel,
        coordinates,
        &region,
        tick,
        origin,
        look_ahead,
    )
}

fn publish_generated_region(
    inner: &mut ProductionSpineInner,
    kernel: &MemoryTransactionKernel,
    coordinates: &[ChunkCoordinate],
    region: &BoundedGeneratedRegionV1,
    tick: FixedTick,
    origin: ChunkCoordinate,
    look_ahead: [i32; 2],
) -> Result<(), ProductionHostError> {
    let mut generated_cells = BTreeMap::new();
    for (coordinate, candidate) in region.candidates() {
        if !inner
            .lifecycle
            .get(&coordinate)
            .is_some_and(|state| *state == ChunkLifecycle::Generate)
        {
            continue;
        }
        let mut cells = draft_cells(candidate.draft(), &inner.palette, &inner.presentation)?;
        apply_hydrology_occupancy(&inner.worldgen_materialization, coordinate, &mut cells)?;
        generated_cells.insert(coordinate, cells);
    }
    publish_generated_cells(
        inner,
        kernel,
        coordinates,
        &generated_cells,
        tick,
        origin,
        look_ahead,
    )
}

fn publish_generated_cells(
    inner: &mut ProductionSpineInner,
    kernel: &MemoryTransactionKernel,
    coordinates: &[ChunkCoordinate],
    generated_cells: &BTreeMap<ChunkCoordinate, Vec<HostVoxel>>,
    tick: FixedTick,
    origin: ChunkCoordinate,
    look_ahead: [i32; 2],
) -> Result<(), ProductionHostError> {
    let mut mutations = Vec::with_capacity(generated_cells.len());
    for (coordinate, cells) in generated_cells {
        if inner.lifecycle.get(coordinate) != Some(&ChunkLifecycle::Generate) {
            continue;
        }
        mutations.push(ChunkMutation::new(
            ChunkKey::new(inner.world, inner.dimension.clone(), *coordinate),
            ChunkRevisionExpectation::Absent,
            ChangedDomains::ALL,
            chunk_data(&inner.voxel_schema, inner.voxel_schema_version, cells),
        ));
    }
    if mutations.is_empty() {
        for coordinate in coordinates {
            if inner.lifecycle.get(coordinate) == Some(&ChunkLifecycle::Generate) {
                inner.lifecycle.remove(coordinate);
                inner.residency.remove(coordinate);
            }
        }
        return Ok(());
    }
    let snapshot = kernel.reference_snapshot(inner.world)?;
    kernel.commit(WorldTransaction::new(
        TransactionId::from_u128(inner.next_transaction),
        inner.world,
        snapshot.revision(),
        mutations,
    ))?;
    inner.next_transaction = inner.next_transaction.saturating_add(1);
    let published = kernel.reference_snapshot(inner.world)?;
    for coordinate in coordinates {
        if inner.lifecycle.get(coordinate) != Some(&ChunkLifecycle::Generate) {
            continue;
        }
        let key = ChunkKey::new(inner.world, inner.dimension.clone(), *coordinate);
        let stored = published
            .chunk(&key)
            .ok_or(ProductionHostError::MissingStoredChunk {
                coordinate: *coordinate,
            })?;
        let class = interest_class(*coordinate, origin, inner.clamps, look_ahead, &inner.edited);
        let requests = stream_derived_requests(class, chebyshev_xz(*coordinate, origin));
        project_stored(
            &mut inner.runtime,
            stored,
            inner.chunk_edge,
            tick,
            &inner.presentation,
            requests,
        )?;
        inner
            .lifecycle
            .insert(*coordinate, ChunkLifecycle::Resident);
        seal_unready_cave_voids(inner, *coordinate);
    }
    Ok(())
}

fn remember_admission(
    inner: &mut ProductionSpineInner,
    coordinate: ChunkCoordinate,
    class: InterestClass,
    tick: FixedTick,
) {
    let now = tick.get();
    inner.residency.insert(
        coordinate,
        ChunkResidency {
            admitted_tick: now,
            last_core_tick: matches!(class, InterestClass::Pin | InterestClass::Core)
                .then_some(now),
        },
    );
    inner.stream_admissions = inner.stream_admissions.saturating_add(1);
}

fn forget_chunk(inner: &mut ProductionSpineInner, coordinate: ChunkCoordinate) {
    inner.lifecycle.remove(&coordinate);
    inner.worldgen_tickets.remove(&coordinate);
    inner.derived.remove(&coordinate);
    inner.mesh_dirty.remove(&coordinate);
    inner.collider_dirty.remove(&coordinate);
    inner.residency.remove(&coordinate);
    inner.last_desired.remove(&coordinate);
    inner.removed.insert(coordinate);
}

fn refresh_lifecycle(inner: &mut ProductionSpineInner) {
    let coordinates = inner.lifecycle.keys().copied().collect::<Vec<_>>();
    for coordinate in coordinates {
        if !matches!(
            inner.lifecycle.get(&coordinate),
            Some(ChunkLifecycle::Resident | ChunkLifecycle::MeshCollider | ChunkLifecycle::Active)
        ) {
            continue;
        }
        let next = if !inner.derived.contains_key(&coordinate) {
            ChunkLifecycle::Resident
        } else if cave_entry_ready_inner(inner, coordinate) {
            ChunkLifecycle::Active
        } else {
            ChunkLifecycle::MeshCollider
        };
        if let Some(state) = inner.lifecycle.get_mut(&coordinate) {
            *state = next;
        }
    }
}

fn project_stored(
    runtime: &mut VoxelRuntime<HostVoxel>,
    stored: &StoredChunk,
    edge: u16,
    tick: FixedTick,
    presentation: &HostPresentationIndex,
    requests: DerivedRequestSet,
) -> Result<(), ProductionHostError> {
    let cells = decode_cells(stored.data().voxels().bytes(), edge, presentation)?;
    let projection = CommittedChunkProjection::from_stored_chunk(
        stored,
        edge,
        cells,
        MESH_SEMANTICS,
        COLLIDER_SEMANTICS,
    )?;
    runtime.project_committed(projection, tick, requests)?;
    Ok(())
}

/// ADR 0026 main-world completion apply job cap per interest slice.
#[cfg(test)]
const MAIN_WORLD_APPLY_JOB_CAP: usize = RuntimeLimits::MAIN_WORLD_APPLY_JOB_CAP;
/// Startup may drain the initial working set without joining a dispatched slice.
const STARTUP_BARRIER_SLICES: usize = 8_192;
/// Edit readiness waits only for the touched chunks' current revisions.
const EDIT_BARRIER_SLICES: usize = 1_024;
/// Movement-time collider safety waits only for player-occupied chunks.
const COLLIDER_SAFETY_BARRIER_SLICES: usize = 32;

/// Why a bounded readiness barrier is pumping derived work.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DerivedReadiness {
    /// Materialize until pending, in-flight, waiting, and spawned work is idle.
    Startup,
    /// Wait until these resident chunks have matching mesh and collider applies.
    Chunks,
}

/// One per-tick derived slice: poll finished Bevy tasks, apply under the
/// ADR 0026 16 job / 16 MiB / 2 ms budget, then dispatch without joining.
fn drain_derived(spine: &ProductionSpine, tick: FixedTick) -> Result<(), ProductionHostError> {
    poll_derived_tasks(spine)?;
    {
        let mut inner = spine.lock_inner()?;
        apply_ready_waiting(&mut inner, tick, ApplyBudgetMode::Frame)?;
        refresh_lifecycle(&mut inner);
    }
    let inputs = {
        let mut inner = spine.lock_inner()?;
        dispatch_derived_batch(&mut inner, DerivedKind::ALL)?
    };
    spawn_derived_jobs(spine, inputs)?;
    poll_derived_tasks(spine)?;
    let mut inner = spine.lock_inner()?;
    apply_ready_waiting(&mut inner, tick, ApplyBudgetMode::Frame)?;
    refresh_lifecycle(&mut inner);
    Ok(())
}

fn await_edit_readiness(
    spine: &ProductionSpine,
    tick: FixedTick,
    positions: impl IntoIterator<Item = BlockPosition>,
) -> Result<(), ProductionHostError> {
    let chunks = positions
        .into_iter()
        .filter_map(|position| spine.chunk_of(position))
        .collect::<Vec<_>>();
    await_derived_ready(
        spine,
        tick,
        DerivedReadiness::Chunks,
        &chunks,
        EDIT_BARRIER_SLICES,
    )
}

fn await_collider_safety(
    spine: &ProductionSpine,
    occupied: &BTreeSet<ChunkCoordinate>,
    tick: FixedTick,
) -> Result<(), ProductionHostError> {
    for _ in 0..COLLIDER_SAFETY_BARRIER_SLICES {
        poll_derived_tasks(spine)?;
        {
            let mut inner = spine.lock_inner()?;
            apply_ready_waiting(&mut inner, tick, ApplyBudgetMode::Barrier)?;
            if collider_safety_ready(&inner, occupied) {
                refresh_lifecycle(&mut inner);
                return Ok(());
            }
        }
        let inputs = {
            let mut inner = spine.lock_inner()?;
            dispatch_derived_batch(&mut inner, [DerivedKind::Collider])?
        };
        spawn_derived_jobs(spine, inputs)?;
        tick_compute_pool();
    }
    poll_derived_tasks(spine)?;
    let mut inner = spine.lock_inner()?;
    apply_ready_waiting(&mut inner, tick, ApplyBudgetMode::Barrier)?;
    Ok(())
}

fn await_derived_ready(
    spine: &ProductionSpine,
    tick: FixedTick,
    readiness: DerivedReadiness,
    chunks: &[ChunkCoordinate],
    max_slices: usize,
) -> Result<(), ProductionHostError> {
    for _ in 0..max_slices {
        poll_derived_tasks(spine)?;
        let idle = {
            let mut inner = spine.lock_inner()?;
            apply_ready_waiting(&mut inner, tick, ApplyBudgetMode::Barrier)?;
            let ready = match readiness {
                DerivedReadiness::Startup => derived_work_idle(&inner),
                DerivedReadiness::Chunks => chunks_derived_ready(&inner, chunks),
            };
            if ready {
                refresh_lifecycle(&mut inner);
                return Ok(());
            }
            derived_work_idle(&inner)
        };
        let inputs = {
            let mut inner = spine.lock_inner()?;
            dispatch_derived_batch(&mut inner, DerivedKind::ALL)?
        };
        if inputs.is_empty() && idle {
            let mut inner = spine.lock_inner()?;
            refresh_lifecycle(&mut inner);
            if match readiness {
                DerivedReadiness::Startup => true,
                DerivedReadiness::Chunks => chunks_derived_ready(&inner, chunks),
            } {
                return Ok(());
            }
            return Err(ProductionHostError::DerivedReadinessBarrier);
        }
        spawn_derived_jobs(spine, inputs)?;
        for _ in 0..8 {
            tick_compute_pool();
            poll_derived_tasks(spine)?;
            let mut inner = spine.lock_inner()?;
            apply_ready_waiting(&mut inner, tick, ApplyBudgetMode::Barrier)?;
            let ready = match readiness {
                DerivedReadiness::Startup => derived_work_idle(&inner),
                DerivedReadiness::Chunks => chunks_derived_ready(&inner, chunks),
            };
            if ready {
                refresh_lifecycle(&mut inner);
                return Ok(());
            }
        }
    }
    Err(ProductionHostError::DerivedReadinessBarrier)
}

fn derived_work_idle(inner: &ProductionSpineInner) -> bool {
    inner.in_flight_tasks.is_empty()
        && inner.waiting_derived.is_empty()
        && inner.runtime.pending_jobs() == 0
        && inner.runtime.in_flight_jobs() == 0
}

fn chunks_derived_ready(inner: &ProductionSpineInner, chunks: &[ChunkCoordinate]) -> bool {
    chunks.iter().all(|coordinate| {
        !inner.runtime.is_resident(*coordinate)
            || (inner
                .runtime
                .last_applied_key(*coordinate, DerivedKind::Mesh)
                .is_some()
                && matches!(
                    inner.runtime.collider_safety(*coordinate),
                    Some(ColliderSafetyState::Ready { .. })
                ))
    })
}

fn collider_safety_ready(
    inner: &ProductionSpineInner,
    occupied: &BTreeSet<ChunkCoordinate>,
) -> bool {
    occupied.iter().all(|coordinate| {
        !inner.runtime.is_resident(*coordinate)
            || matches!(
                inner.runtime.collider_safety(*coordinate),
                Some(ColliderSafetyState::Ready { .. })
            )
    })
}

fn dispatch_derived_batch(
    inner: &mut ProductionSpineInner,
    kinds: impl IntoIterator<Item = DerivedKind>,
) -> Result<Vec<DerivedInput<HostVoxel>>, ProductionHostError> {
    let mut mesh = false;
    let mut collider = false;
    for kind in kinds {
        match kind {
            DerivedKind::Mesh => mesh = true,
            DerivedKind::Collider => collider = true,
        }
    }
    let capacity = inner
        .runtime
        .admission_snapshot()
        .cpu_heavy_slots_remaining();
    let mut inputs = Vec::with_capacity(capacity);
    loop {
        let outcome = match (mesh, collider) {
            (true, true) => inner.runtime.dispatch_next_any()?,
            (true, false) => inner.runtime.dispatch_next(DerivedKind::Mesh)?,
            (false, true) => inner.runtime.dispatch_next(DerivedKind::Collider)?,
            (false, false) => break,
        };
        match outcome {
            DispatchOutcome::Started(input) => inputs.push(input),
            DispatchOutcome::Empty | DispatchOutcome::Backpressured { .. } => break,
            DispatchOutcome::MemoryContractViolation { .. } => {
                return Err(ProductionHostError::DerivedMemory);
            }
        }
    }
    Ok(inputs)
}

fn spawn_derived_jobs(
    spine: &ProductionSpine,
    inputs: Vec<DerivedInput<HostVoxel>>,
) -> Result<(), ProductionHostError> {
    if inputs.is_empty() {
        return Ok(());
    }
    if let Some(pool) = AsyncComputeTaskPool::try_get() {
        let mut tasks = inputs
            .into_iter()
            .map(|input| pool.spawn(async move { compute_derived(input) }))
            .collect::<Vec<_>>();
        let mut inner = spine.lock_inner()?;
        inner.in_flight_tasks.append(&mut tasks);
    } else {
        let completed = inputs.into_iter().map(compute_derived).collect();
        let mut inner = spine.lock_inner()?;
        enqueue_waiting_derived(&mut inner, completed);
    }
    Ok(())
}

fn poll_derived_tasks(spine: &ProductionSpine) -> Result<(), ProductionHostError> {
    let tasks = {
        let mut inner = spine.lock_inner()?;
        mem::take(&mut inner.in_flight_tasks)
    };
    if tasks.iter().any(|task| !task.is_finished()) {
        tick_compute_pool();
    }
    let mut remaining = Vec::new();
    let mut completed = Vec::new();
    for task in tasks {
        if task.is_finished() {
            completed.push(block_on(task));
        } else {
            remaining.push(task);
        }
    }
    let mut inner = spine.lock_inner()?;
    inner.in_flight_tasks = remaining;
    enqueue_waiting_derived(&mut inner, completed);
    Ok(())
}

fn tick_compute_pool() {
    if AsyncComputeTaskPool::try_get().is_some() {
        tick_global_task_pools_on_main_thread();
    }
}

fn enqueue_waiting_derived(inner: &mut ProductionSpineInner, completed: Vec<ComputedDerived>) {
    for job in completed {
        let bytes = job
            .payload
            .as_ref()
            .map_or(0, RetainedBytes::retained_bytes);
        inner.runtime.record_waiting_to_apply(bytes);
        inner.waiting_derived.push_back(job);
    }
}

fn waiting_derived_bytes(job: &ComputedDerived) -> u64 {
    job.payload
        .as_ref()
        .map_or(0, RetainedBytes::retained_bytes)
}

/// Whether an apply slice uses the frame budget or drains waiting work.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ApplyBudgetMode {
    /// ADR 0026 16 job / 16 MiB / 2 ms main-world apply.
    Frame,
    /// Explicit readiness barrier; still last-write-wins and stale-rejects.
    Barrier,
}

fn apply_ready_waiting(
    inner: &mut ProductionSpineInner,
    tick: FixedTick,
    mode: ApplyBudgetMode,
) -> Result<(), ProductionHostError> {
    let started = Instant::now();
    let budget = match mode {
        ApplyBudgetMode::Frame => RuntimeLimits::main_world_apply_budget(),
        ApplyBudgetMode::Barrier => {
            DerivedApplyBudget::new(usize::MAX / 4, u64::MAX / 4, u64::MAX / 4)?
        }
    };
    let mut slice = DerivedApplySlice::new();
    let mut waiting = mem::take(&mut inner.waiting_derived);
    let result = apply_waiting_derived(
        &mut waiting,
        waiting_derived_bytes,
        |job, bytes| {
            inner.runtime.consume_waiting_to_apply(bytes);
            apply_computed_derived(inner, job, tick)
        },
        &mut slice,
        budget,
        || elapsed_nanos(started),
    );
    if let Ok(stop) = result.as_ref()
        && matches!(mode, ApplyBudgetMode::Frame)
        && !stop.is_admit()
    {
        inner.runtime.record_apply_budget_stop(*stop);
    }
    inner.waiting_derived = waiting;
    result.map(|_| ())
}

fn elapsed_nanos(started: Instant) -> WallClockNanos {
    let nanos = u64::try_from(started.elapsed().as_nanos()).unwrap_or(u64::MAX);
    WallClockNanos::new(nanos)
}

/// Applies waiting derived jobs until the supplied budget is exhausted.
///
/// The first job of a slice is always applied so an oversized payload cannot
/// stall the queue. Remaining jobs stay in `waiting`.
///
/// # Errors
///
/// Returns [`ProductionHostError`] when `apply` rejects a popped job.
pub(super) fn apply_waiting_derived<T>(
    waiting: &mut VecDeque<T>,
    job_bytes: impl Fn(&T) -> u64,
    mut apply: impl FnMut(T, u64) -> Result<(), ProductionHostError>,
    slice: &mut DerivedApplySlice,
    budget: DerivedApplyBudget,
    mut elapsed: impl FnMut() -> WallClockNanos,
) -> Result<ApplyAdmission, ProductionHostError> {
    let mut last = ApplyAdmission::Admit;
    while let Some(job) = waiting.pop_front() {
        let bytes = job_bytes(&job);
        slice.set_elapsed(elapsed());
        last = slice.admission(bytes, budget);
        if !last.is_admit() {
            waiting.push_front(job);
            return Ok(last);
        }
        apply(job, bytes)?;
        slice.commit_applied(bytes, elapsed());
    }
    Ok(last)
}

fn compute_derived(input: DerivedInput<HostVoxel>) -> ComputedDerived {
    let payload = match input.ticket().key().kind() {
        DerivedKind::Mesh => compute_mesh_payload(&input),
        DerivedKind::Collider => compute_collider_payload(&input),
    };
    ComputedDerived { input, payload }
}

fn compute_mesh_payload(
    input: &DerivedInput<HostVoxel>,
) -> Result<DerivedPayload, ProductionHostError> {
    let coordinate = input.ticket().key().coordinate();
    let source = input
        .ticket()
        .key()
        .mesh_source()
        .ok_or(ProductionHostError::MissingMeshSource { coordinate })?;
    MESH_LANE.with(|lane| {
        let mut geometry = MeshBuffer::default();
        let receipt = lane.borrow_mut().mesh_into(
            input.samples(),
            input.dimensions(),
            source,
            &mut geometry,
        )?;
        Ok(DerivedPayload::Mesh(HostDerivedMesh {
            receipt,
            source,
            geometry,
        }))
    })
}

fn compute_collider_payload(
    input: &DerivedInput<HostVoxel>,
) -> Result<DerivedPayload, ProductionHostError> {
    Ok(DerivedPayload::Collider(HostDerivedCollider {
        occupied: occupied_voxels(input.samples(), input.dimensions())?,
    }))
}

fn apply_computed_derived(
    inner: &mut ProductionSpineInner,
    job: ComputedDerived,
    tick: FixedTick,
) -> Result<(), ProductionHostError> {
    match job.payload {
        Ok(DerivedPayload::Mesh(derived)) => {
            let coordinate = job.input.ticket().key().coordinate();
            let revision = job.input.ticket().key().revision();
            finish_derived(inner, job.input, derived, tick, |inner, value| {
                apply_mesh_derived(inner, coordinate, revision, value);
            })
        }
        Ok(DerivedPayload::Collider(derived)) => {
            let coordinate = job.input.ticket().key().coordinate();
            let revision = job.input.ticket().key().revision();
            finish_derived(inner, job.input, derived, tick, |inner, value| {
                apply_collider_derived(inner, coordinate, revision, value);
            })
        }
        Err(error) => match inner.runtime.complete_derived(
            job.input,
            0_u8,
            ApplyByteDeclaration::new(0),
            tick,
            |_| Err::<(), ProductionHostError>(error),
        ) {
            CompletionOutcome::ApplyFailed { error, .. } => Err(error),
            CompletionOutcome::StaleRejected { .. } | CompletionOutcome::Cancelled { .. } => Ok(()),
            CompletionOutcome::ApplyPanicked { .. } => {
                Err(ProductionHostError::DerivedApplyPanicked)
            }
            CompletionOutcome::Applied { .. }
            | CompletionOutcome::MemoryContractViolation { .. }
            | CompletionOutcome::UnknownJob { .. } => Err(ProductionHostError::DerivedRejected),
        },
    }
}

fn finish_derived<R: RetainedBytes>(
    inner: &mut ProductionSpineInner,
    input: DerivedInput<HostVoxel>,
    derived: R,
    tick: FixedTick,
    apply: impl FnOnce(&mut ProductionSpineInner, R),
) -> Result<(), ProductionHostError> {
    let apply_bytes = ApplyByteDeclaration::new(derived.retained_bytes());
    let outcome = inner
        .runtime
        .complete_derived(input, derived, apply_bytes, tick, |value| {
            Ok::<R, ProductionHostError>(value)
        });
    match outcome {
        CompletionOutcome::Applied { value, .. } => {
            apply(inner, value);
            Ok(())
        }
        CompletionOutcome::StaleRejected { .. } | CompletionOutcome::Cancelled { .. } => Ok(()),
        CompletionOutcome::ApplyFailed { error, .. } => Err(error),
        CompletionOutcome::ApplyPanicked { .. } => Err(ProductionHostError::DerivedApplyPanicked),
        CompletionOutcome::MemoryContractViolation { .. }
        | CompletionOutcome::UnknownJob { .. } => Err(ProductionHostError::DerivedRejected),
    }
}

fn apply_mesh_derived(
    inner: &mut ProductionSpineInner,
    coordinate: ChunkCoordinate,
    revision: ChunkRevision,
    value: HostDerivedMesh,
) {
    let entry = inner
        .derived
        .entry(coordinate)
        .or_insert_with(|| empty_derived(revision));
    entry.revision = revision;
    entry.mesh_receipt = Some(value.receipt);
    entry.mesh_source = Some(value.source);
    entry.bounds = value.geometry.bounds();
    entry.geometry = Some(Arc::new(value.geometry));
    inner.mesh_dirty.insert(coordinate);
}

fn apply_collider_derived(
    inner: &mut ProductionSpineInner,
    coordinate: ChunkCoordinate,
    revision: ChunkRevision,
    value: HostDerivedCollider,
) {
    let entry = inner
        .derived
        .entry(coordinate)
        .or_insert_with(|| empty_derived(revision));
    entry.revision = revision;
    entry.occupied = value.occupied;
    entry.collider = compound_collider(&entry.occupied);
    inner.collider_dirty.insert(coordinate);
}

fn empty_derived(revision: ChunkRevision) -> ChunkDerived {
    ChunkDerived {
        mesh_receipt: None,
        mesh_source: None,
        geometry: None,
        bounds: None,
        collider: None,
        occupied: Vec::new(),
        revision,
    }
}

fn occupied_voxels(
    samples: &[HostVoxel],
    dimensions: PaddedChunk,
) -> Result<Vec<OccupiedCell>, ProductionHostError> {
    let interior = dimensions.interior_size();
    let mut occupied = Vec::new();
    for z in 0..interior[2] {
        for y in 0..interior[1] {
            for x in 0..interior[0] {
                let padded = [x + 1, y + 1, z + 1];
                let index = dimensions
                    .linearize(padded)
                    .ok_or(ProductionHostError::PaddedIndex)?;
                let Some(sample) = samples.get(index) else {
                    continue;
                };
                if !sample.collision_occupied() {
                    continue;
                }
                occupied.push(OccupiedCell {
                    local: [
                        u16::try_from(x).map_err(|_| ProductionHostError::PaddedIndex)?,
                        u16::try_from(y).map_err(|_| ProductionHostError::PaddedIndex)?,
                        u16::try_from(z).map_err(|_| ProductionHostError::PaddedIndex)?,
                    ],
                    palette_index: sample.palette_index,
                });
            }
        }
    }
    Ok(occupied)
}

/// Inclusive local origin and voxel extents of one merged collision cuboid.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct OccupiedBox {
    origin: [u16; 3],
    size: [u16; 3],
}

/// Deterministic greedy merge of occupied cells into axis-aligned cuboids.
///
/// Cells are visited in local `(x, y, z)` order via [`BTreeSet`]. Each unused
/// cell grows along `+X`, then `+Y` while every x-run in the rectangle stays
/// occupied, then `+Z` while every xy-slab stays occupied. The cuboids never
/// overlap and cover exactly the input occupancy set.
fn merge_occupied_boxes(occupied: &[OccupiedCell]) -> Vec<OccupiedBox> {
    let mut remaining: BTreeSet<[u16; 3]> = occupied.iter().map(|cell| cell.local).collect();
    let mut boxes = Vec::new();
    while let Some(origin) = remaining.first().copied() {
        let merged = grow_occupied_box(&remaining, origin);
        remove_occupied_box(&mut remaining, merged);
        boxes.push(merged);
    }
    boxes
}

fn grow_occupied_box(remaining: &BTreeSet<[u16; 3]>, origin: [u16; 3]) -> OccupiedBox {
    let [x0, y0, z0] = origin;
    let mut size_x = 1_u16;
    while let Some(x) = x0.checked_add(size_x) {
        if !remaining.contains(&[x, y0, z0]) {
            break;
        }
        let Some(next) = size_x.checked_add(1) else {
            break;
        };
        size_x = next;
    }

    let mut size_y = 1_u16;
    while let Some(y) = y0.checked_add(size_y) {
        if !(0..size_x).all(|dx| remaining.contains(&[x0 + dx, y, z0])) {
            break;
        }
        let Some(next) = size_y.checked_add(1) else {
            break;
        };
        size_y = next;
    }

    let mut size_z = 1_u16;
    while let Some(z) = z0.checked_add(size_z) {
        let slab_occupied =
            (0..size_y).all(|dy| (0..size_x).all(|dx| remaining.contains(&[x0 + dx, y0 + dy, z])));
        if !slab_occupied {
            break;
        }
        let Some(next) = size_z.checked_add(1) else {
            break;
        };
        size_z = next;
    }

    OccupiedBox {
        origin,
        size: [size_x, size_y, size_z],
    }
}

fn remove_occupied_box(remaining: &mut BTreeSet<[u16; 3]>, merged: OccupiedBox) {
    let [x0, y0, z0] = merged.origin;
    let [size_x, size_y, size_z] = merged.size;
    for dz in 0..size_z {
        for dy in 0..size_y {
            for dx in 0..size_x {
                remaining.remove(&[x0 + dx, y0 + dy, z0 + dz]);
            }
        }
    }
}

/// Builds a finite Avian compound from greedily merged occupancy cuboids.
fn compound_collider(occupied: &[OccupiedCell]) -> Option<Collider> {
    let boxes = merge_occupied_boxes(occupied);
    if boxes.is_empty() {
        return None;
    }
    let shapes = boxes
        .iter()
        .map(|merged| {
            let [x, y, z] = merged.origin;
            let [sx, sy, sz] = merged.size;
            (
                Vec3::new(
                    f32::from(x) + f32::from(sx) * 0.5,
                    f32::from(y) + f32::from(sy) * 0.5,
                    f32::from(z) + f32::from(sz) * 0.5,
                ),
                Quat::IDENTITY,
                Collider::cuboid(f32::from(sx), f32::from(sy), f32::from(sz)),
            )
        })
        .collect();
    Some(Collider::compound(shapes))
}

fn draft_cells(
    draft: &latticeaxiom_worldgen::ChunkDraftV1,
    palette: &[BlockId],
    presentation: &HostPresentationIndex,
) -> Result<Vec<HostVoxel>, ProductionHostError> {
    let edge = draft.edge_voxels();
    let mut cells = Vec::with_capacity(usize::from(edge).pow(3));
    for y in 0..edge {
        for z in 0..edge {
            for x in 0..edge {
                let block = draft
                    .block_at(x, y, z)
                    .ok_or(ProductionHostError::DraftVoxelMissing)?;
                let block_id = BlockId::parse(block.as_str())?;
                let palette_index = palette_index(palette, &block_id)
                    .ok_or(ProductionHostError::UnknownDraftBlock)?;
                cells.push(HostVoxel::from_solid(palette_index, presentation));
            }
        }
    }
    Ok(cells)
}

fn apply_hydrology_occupancy(
    materialization: &WorldgenMaterialization,
    coordinate: ChunkCoordinate,
    cells: &mut [HostVoxel],
) -> Result<(), ProductionHostError> {
    if !materialization.plan.has_hydrology_occupancy() {
        return Ok(());
    }
    let candidate = materialization
        .plan
        .hydrology_occupancy_candidate(coordinate)?;
    if !occupancy_candidate_is_current(&materialization.plan, &candidate) {
        return Ok(());
    }
    let empty_block = materialization
        .palette
        .get(usize::from(materialization.empty.palette_index))
        .ok_or(ProductionHostError::UnknownDraftBlock)?;
    let empty_id: StableId = empty_block.as_str().parse()?;
    let empty_definition = materialization.content.block(&empty_id).ok_or(
        ProductionHostError::MissingCatalogDefinition {
            kind: "block",
            id: empty_id.to_string(),
        },
    )?;
    let context = OccupancyArbitrationContextV1::new(
        empty_id,
        empty_definition.definition().default_state.clone(),
    )?;
    let edge = materialization.chunk_edge;
    let stride = usize::from(edge).saturating_mul(usize::from(edge));
    let entrance = player_spawn_center(materialization.spawn)
        .ok()
        .and_then(|center| translation_chunk(center, materialization.chunk_edge))
        .and_then(|chunk| required_cave_entrance(&materialization.plan, chunk));
    for occupied in candidate.cells() {
        let index = usize::from(occupied.y())
            .saturating_mul(stride)
            .saturating_add(usize::from(occupied.z()).saturating_mul(usize::from(edge)))
            .saturating_add(usize::from(occupied.x()));
        let Some(cell) = cells.get_mut(index) else {
            continue;
        };
        if cell.palette_index != materialization.empty.palette_index {
            continue;
        }
        let world_x = i64::from(coordinate.x)
            .saturating_mul(i64::from(edge))
            .saturating_add(i64::from(occupied.x()));
        let world_y = i64::from(coordinate.y)
            .saturating_mul(i64::from(edge))
            .saturating_add(i64::from(occupied.y()));
        let world_z = i64::from(coordinate.z)
            .saturating_mul(i64::from(edge))
            .saturating_add(i64::from(occupied.z()));
        // Initial occupancy is standing source water. Directional flow is a
        // drainage hint for the bounded runtime planner, not a D9 snapshot.
        if occupied.flow() != HydrologyFlowV1::Still
            || hydrology_occupancy_forbidden(
                materialization,
                entrance.as_ref(),
                world_x,
                world_y,
                world_z,
            )
        {
            continue;
        }
        let solid = SolidPaletteEntryV1 {
            block: context.empty_block().clone(),
            state: context.empty_state().clone(),
        };
        let level = FluidLevelV1::new(occupied.level()).unwrap_or(FluidLevelV1::SOURCE);
        let requested = FluidPaletteEntryV1::Fluid {
            fluid: occupied.fluid().clone(),
            state: FluidStateV1 {
                level,
                flow: hydrology_flow(occupied.flow()),
            },
        };
        match arbitrate_cell(
            &materialization.content,
            &context,
            &solid,
            &FluidPaletteEntryV1::Empty,
            &requested,
        )? {
            SolidFluidArbitrationV1::Occupied {
                fluid, fluid_state, ..
            } => {
                let Some(fluid_index) =
                    fluid_palette_index(&materialization.fluid_palette, &fluid, fluid_state)
                else {
                    continue;
                };
                *cell = HostVoxel::occupancy(
                    cell.palette_index,
                    fluid_index,
                    &materialization.presentation,
                );
            }
            SolidFluidArbitrationV1::ReplaceThenOccupy { .. }
            | SolidFluidArbitrationV1::Rejected { .. }
            | SolidFluidArbitrationV1::Unchanged { .. } => {}
        }
    }
    Ok(())
}

fn hydrology_occupancy_forbidden(
    materialization: &WorldgenMaterialization,
    entrance: Option<&RequiredCaveEntranceV1>,
    world_x: i64,
    world_y: i64,
    world_z: i64,
) -> bool {
    let occupancy = materialization
        .plan
        .cave_occupancy_arbitration(world_x, world_y, world_z);
    if occupancy.portal_signed_distance() <= 0 {
        return true;
    }
    if occupancy.is_finally_void()
        && occupancy.portal_signed_distance() <= HYDROLOGY_PORTAL_EXCLUSION_SDF
    {
        return true;
    }
    entrance.is_some_and(|entrance| {
        hydrology_forbidden_for_entrance(*entrance, world_x, world_y, world_z)
    })
}

fn hydrology_forbidden_for_entrance(
    entrance: RequiredCaveEntranceV1,
    world_x: i64,
    world_y: i64,
    world_z: i64,
) -> bool {
    let [aperture_x, aperture_y, aperture_z] = entrance.aperture();
    let aperture = [
        i64::from(aperture_x),
        i64::from(aperture_y),
        i64::from(aperture_z),
    ];
    let radius = i64::from(entrance.clearance_radius_voxels()).max(1);
    let dx = (world_x - aperture[0]).abs();
    let dy = (world_y - aperture[1]).abs();
    let dz = (world_z - aperture[2]).abs();
    if dx.max(dy).max(dz) <= radius {
        return true;
    }
    let [destination_x, destination_y, destination_z] = entrance.destination();
    if world_x == i64::from(destination_x)
        && world_y == i64::from(destination_y)
        && world_z == i64::from(destination_z)
    {
        return true;
    }
    let [_, surface_y, _] = entrance.surface_footing();
    world_x == aperture[0]
        && world_z == aperture[2]
        && world_y >= aperture[1]
        && world_y <= i64::from(surface_y)
}

const fn hydrology_flow(flow: HydrologyFlowV1) -> FluidFlowV1 {
    match flow {
        HydrologyFlowV1::Still => FluidFlowV1::Still,
        HydrologyFlowV1::Down => FluidFlowV1::Down,
        HydrologyFlowV1::East => FluidFlowV1::East,
        HydrologyFlowV1::West => FluidFlowV1::West,
        HydrologyFlowV1::South => FluidFlowV1::South,
        HydrologyFlowV1::North => FluidFlowV1::North,
    }
}

fn chunk_data(
    schema: &SchemaId,
    schema_version: PayloadSchemaVersion,
    cells: &[HostVoxel],
) -> ChunkData {
    ChunkData::new(
        voxel_payload(schema, schema_version, cells),
        BTreeMap::new(),
        BTreeMap::new(),
        BTreeMap::new(),
    )
}

pub(super) fn runtime_chunk_cells(
    inner: &ProductionSpineInner,
    coordinate: ChunkCoordinate,
) -> Vec<HostVoxel> {
    let edge = inner.chunk_edge;
    let mut cells = Vec::with_capacity(usize::from(edge).pow(3));
    for y in 0..edge {
        for z in 0..edge {
            for x in 0..edge {
                let voxel = VoxelCoordinate::new(
                    i64::from(coordinate.x) * i64::from(edge) + i64::from(x),
                    i64::from(coordinate.y) * i64::from(edge) + i64::from(y),
                    i64::from(coordinate.z) * i64::from(edge) + i64::from(z),
                );
                cells.push(inner.runtime.cell(voxel).map_or(inner.empty, |cell| *cell));
            }
        }
    }
    cells
}

fn voxel_payload(
    schema: &SchemaId,
    schema_version: PayloadSchemaVersion,
    cells: &[HostVoxel],
) -> VersionedPayload {
    let mut bytes = Vec::with_capacity(cells.len().saturating_mul(CELL_OCCUPANCY_BYTES));
    for cell in cells {
        bytes.extend_from_slice(&cell.palette_index.to_le_bytes());
        bytes.extend_from_slice(&cell.fluid_palette_index.to_le_bytes());
    }
    VersionedPayload::new(schema.clone(), schema_version, bytes)
}

fn runtime_capture_payload(
    schema: &SchemaId,
    schema_version: PayloadSchemaVersion,
    transaction_id: TransactionId,
    discriminator: u8,
) -> VersionedPayload {
    let mut bytes = transaction_id.as_bytes().to_vec();
    bytes.push(discriminator);
    VersionedPayload::new(schema.clone(), schema_version, bytes)
}

fn capture_entity(coordinate: ChunkCoordinate) -> PersistentEntityId {
    let mut bytes = [0xCA; 16];
    bytes[0..4].copy_from_slice(&coordinate.x.to_le_bytes());
    bytes[4..8].copy_from_slice(&coordinate.y.to_le_bytes());
    bytes[8..12].copy_from_slice(&coordinate.z.to_le_bytes());
    PersistentEntityId::from_bytes(bytes)
}

fn capture_continuation(coordinate: ChunkCoordinate) -> ContinuationId {
    let mut bytes = [0xC0; 16];
    bytes[0..4].copy_from_slice(&coordinate.x.to_le_bytes());
    bytes[4..8].copy_from_slice(&coordinate.y.to_le_bytes());
    bytes[8..12].copy_from_slice(&coordinate.z.to_le_bytes());
    ContinuationId::from_bytes(bytes)
}

fn decode_cells(
    bytes: &[u8],
    edge: u16,
    presentation: &HostPresentationIndex,
) -> Result<Vec<HostVoxel>, ProductionHostError> {
    let expected = usize::from(edge)
        .checked_pow(3)
        .and_then(|count| count.checked_mul(CELL_OCCUPANCY_BYTES))
        .ok_or(ProductionHostError::PayloadLength)?;
    if bytes.len() != expected {
        return Err(ProductionHostError::PayloadLength);
    }
    Ok(bytes
        .chunks_exact(CELL_OCCUPANCY_BYTES)
        .map(|cell| {
            HostVoxel::occupancy(
                u16::from_le_bytes([cell[0], cell[1]]),
                u16::from_le_bytes([cell[2], cell[3]]),
                presentation,
            )
        })
        .collect())
}

fn compile_host_solid_palette(
    catalog: &ContentCatalogV1,
    palette: &[BlockId],
) -> Result<CompiledSolidPaletteV1, ProductionHostError> {
    let mut entries = Vec::with_capacity(palette.len());
    for block in palette {
        let id: StableId = block.as_str().parse()?;
        let definition =
            catalog
                .block(&id)
                .ok_or_else(|| ProductionHostError::MissingCatalogDefinition {
                    kind: "block",
                    id: id.to_string(),
                })?;
        entries.push(SolidPaletteEntryV1 {
            block: id,
            state: definition.definition().default_state.clone(),
        });
    }
    CompiledSolidPaletteV1::compile(catalog, entries, PaletteLimitsV1::default())
        .map_err(ProductionHostError::from)
}

fn compile_host_fluid_palette(
    catalog: &ContentCatalogV1,
) -> Result<CompiledFluidPaletteV1, ProductionHostError> {
    if catalog.fluids().is_empty() {
        return Err(ProductionHostError::MissingCatalogDefinition {
            kind: "fluid",
            id: "catalog".to_owned(),
        });
    }
    let mut entries = Vec::new();
    for definition in catalog.fluids() {
        for raw_level in 0..=FluidLevelV1::MAX {
            let Ok(level) = FluidLevelV1::new(raw_level) else {
                continue;
            };
            for flow in FluidFlowV1::ALL {
                entries.push(FluidPaletteEntryV1::Fluid {
                    fluid: definition.header.stable_id.clone(),
                    state: FluidStateV1 { level, flow },
                });
            }
        }
    }
    CompiledFluidPaletteV1::compile(catalog, entries, PaletteLimitsV1::default())
        .map_err(ProductionHostError::from)
}

fn fluid_palette_index(
    palette: &CompiledFluidPaletteV1,
    fluid: &StableId,
    state: FluidStateV1,
) -> Option<u16> {
    palette
        .entries()
        .iter()
        .position(|entry| entry.fluid() == Some(fluid) && entry.state() == Some(&state))
        .and_then(|index| u16::try_from(index).ok())
}

fn palette_index(palette: &[BlockId], block: &BlockId) -> Option<u16> {
    palette
        .iter()
        .position(|candidate| candidate == block)
        .and_then(|index| u16::try_from(index).ok())
}

fn runtime_limits(
    hard_limits: PlayableWorldHardLimitsV1,
) -> Result<RuntimeLimits, ProductionHostError> {
    let max_resident = usize::try_from(hard_limits.max_resident_chunks).unwrap_or(64);
    let mesh = DerivedQueueLimits::mesh_desktop_reference_v1()?;
    let collider = DerivedQueueLimits::collider_desktop_reference_v1()?;
    Ok(
        RuntimeLimits::new(max_resident.max(1), 32 * 1024 * 1024, mesh, collider)?
            .with_cpu_heavy_concurrency(cpu_heavy_concurrency(host_parallelism()))?,
    )
}

const DERIVED_PRIORITY_BUCKET_WIDTH: u16 = 8_192;

fn stream_derived_priority(class: InterestClass, distance: u32) -> DerivedPriority {
    let priority = match class {
        InterestClass::Core => DerivedPriority::CORE,
        InterestClass::Pin => DerivedPriority::EDIT_TO_VISIBLE,
        InterestClass::Retain => DerivedPriority::RETAIN,
        InterestClass::Prefetch => DerivedPriority::PREFETCH,
    };
    priority_with_distance(priority, distance)
}

fn priority_with_distance(priority: DerivedPriority, distance: u32) -> DerivedPriority {
    let maximum_offset = DERIVED_PRIORITY_BUCKET_WIDTH.saturating_sub(1);
    let offset = u16::try_from(distance.min(u32::from(maximum_offset))).unwrap_or(maximum_offset);
    DerivedPriority::new(
        priority
            .get()
            .saturating_mul(DERIVED_PRIORITY_BUCKET_WIDTH)
            .saturating_add(offset),
    )
}

fn stream_derived_requests(class: InterestClass, distance: u32) -> DerivedRequestSet {
    derived_requests(stream_derived_priority(class, distance))
}

fn derived_requests(priority: DerivedPriority) -> DerivedRequestSet {
    let request = DerivedRequest::new(
        priority,
        DerivedOwner::new(1),
        DerivedMemoryBudget::new(64 * 1024, 64 * 1024),
    );
    DerivedRequestSet::new(request, request)
}
fn chunk_changed_domains(current: Option<&ChunkData>, replacement: &ChunkData) -> ChangedDomains {
    let Some(current) = current else {
        return ChangedDomains::ALL;
    };
    let mut changed = ChangedDomains::NONE;
    if current.voxels() != replacement.voxels() {
        changed = changed.union(ChangedDomains::VOXELS);
    }
    if current.persistent_entities() != replacement.persistent_entities() {
        changed = changed.union(ChangedDomains::PERSISTENT_ENTITIES);
    }
    if current.continuations() != replacement.continuations() {
        changed = changed.union(ChangedDomains::CONTINUATIONS);
    }
    if current.provenance() != replacement.provenance() {
        changed = changed.union(ChangedDomains::PROVENANCE);
    }
    changed
}

fn commit_mutations(
    writer: &mut SealedWorldWriterHost,
    world: WorldId,
    mut base: WorldRevision,
    metadata: &AuthoritativeMetadataInputV1,
    mutations: &[ChunkMutation],
    durability: CommitDurabilityV1,
) -> Result<Option<WorldCommitOutcomeV1>, ProductionHostError> {
    if mutations.is_empty() {
        return Ok(None);
    }
    let max_chunks = usize::try_from(writer.storage().limits().max_chunks_per_commit())
        .unwrap_or(1)
        .max(1);
    let mut transaction = base.get().saturating_add(1);
    let mut last = None;
    for batch in mutations.chunks(max_chunks) {
        let outcome = writer.commit(WorldCommitRequestV1::new(
            WorldTransaction::new(
                TransactionId::from_u128(u128::from(transaction)),
                world,
                base,
                batch.to_vec(),
            ),
            metadata.clone(),
            durability,
        ))?;
        base = outcome.receipt().frontier().current();
        transaction = transaction.saturating_add(1);
        last = Some(outcome);
    }
    Ok(last)
}

fn peek_player_session(
    store: &DeterministicWorldStorage,
    world: WorldId,
    dimension: &DimensionId,
    spawn_chunk: ChunkCoordinate,
) -> Result<Option<DurablePlayerSessionV1>, ProductionHostError> {
    let view = store.begin_read(world)?;
    let key = ChunkKey::new(world, dimension.clone(), spawn_chunk);
    let Some(persisted) = view.load_chunk(&key)? else {
        return Ok(None);
    };
    let Some(payload) = persisted
        .data()
        .persistent_entities()
        .get(&PLAYER_SESSION_ENTITY)
    else {
        return Ok(None);
    };
    Ok(Some(DurablePlayerSessionV1::decode_payload(payload)?))
}

fn restore_player_session(
    inner: &mut ProductionSpineInner,
    session: &DurablePlayerSessionV1,
) -> Result<(), ProductionHostError> {
    inner.player_pose = session.pose();
    inner.stream_anchor_xz = [
        inner.player_pose.translation.x,
        inner.player_pose.translation.z,
    ];
    let gameplay = inner
        .gameplay
        .as_mut()
        .ok_or(ProductionHostError::InvalidPlayerSession)?;
    gameplay.restore_inventory(&session.inventory_stacks()?)?;
    gameplay.select_hotbar_slot(session.hotbar_slot())?;
    for (id, container) in session.container_states()? {
        gameplay.restore_container(id, container)?;
    }
    for (id, continuation) in session.scheduled_states()? {
        gameplay.restore_continuation(id, continuation)?;
    }
    for (id, drop) in session.dropped_states()? {
        gameplay.restore_drop(id, drop)?;
    }
    gameplay.set_next_drop(session.next_drop());
    Ok(())
}
#[allow(clippy::cast_possible_truncation)] // Origins are rejected unless they fit `i32` chunks.
fn dda_origin(origin_m: [f32; 3], edge: u16) -> Option<DdaOrigin> {
    let edge_f = f64::from(edge);
    let origin = origin_m.map(f64::from);
    if !origin.iter().copied().all(f64::is_finite) {
        return None;
    }
    let chunks = origin.map(|value| (value / edge_f).floor());
    if chunks
        .iter()
        .any(|value| !(f64::from(i32::MIN)..=f64::from(i32::MAX)).contains(value))
    {
        return None;
    }
    let chunk = ChunkCoordinate::new(chunks[0] as i32, chunks[1] as i32, chunks[2] as i32);
    Some(DdaOrigin::new(
        chunk,
        [
            origin[0] - f64::from(chunk.x) * edge_f,
            origin[1] - f64::from(chunk.y) * edge_f,
            origin[2] - f64::from(chunk.z) * edge_f,
        ],
    ))
}

fn block_position(coordinate: VoxelCoordinate) -> Option<BlockPosition> {
    Some(BlockPosition {
        x: i32::try_from(coordinate.x).ok()?,
        y: i32::try_from(coordinate.y).ok()?,
        z: i32::try_from(coordinate.z).ok()?,
    })
}

fn translation_chunk(translation: Vec3, edge: u16) -> Option<ChunkCoordinate> {
    dda_origin(translation.to_array(), edge).map(DdaOrigin::chunk)
}

fn cave_entry_ready_inner(inner: &ProductionSpineInner, coordinate: ChunkCoordinate) -> bool {
    inner
        .derived
        .get(&coordinate)
        .is_some_and(|derived| derived.mesh_receipt.is_some())
        && matches!(
            inner.runtime.collider_safety(coordinate),
            Some(ColliderSafetyState::Ready { .. })
        )
}

fn seal_unready_cave_voids(inner: &mut ProductionSpineInner, coordinate: ChunkCoordinate) {
    if cave_entry_ready_inner(inner, coordinate) {
        return;
    }
    let Some((_, revision, _)) = inner.runtime.chunk_revisions(coordinate) else {
        return;
    };
    let occupied = conservative_occupied_voxels(inner, coordinate);
    let entry = inner
        .derived
        .entry(coordinate)
        .or_insert_with(|| empty_derived(revision));
    entry.revision = revision;
    if entry.mesh_receipt.is_none() {
        entry.occupied = occupied;
        entry.collider = compound_collider(&entry.occupied);
        inner.collider_dirty.insert(coordinate);
    }
}

fn conservative_occupied_voxels(
    inner: &ProductionSpineInner,
    coordinate: ChunkCoordinate,
) -> Vec<OccupiedCell> {
    let edge = inner.chunk_edge;
    let mut occupied = BTreeMap::new();
    for z in 0..edge {
        for y in 0..edge {
            for x in 0..edge {
                let world_x = i64::from(coordinate.x) * i64::from(edge) + i64::from(x);
                let world_y = i64::from(coordinate.y) * i64::from(edge) + i64::from(y);
                let world_z = i64::from(coordinate.z) * i64::from(edge) + i64::from(z);
                let voxel = VoxelCoordinate::new(world_x, world_y, world_z);
                let sample = inner.runtime.cell(voxel).ok();
                let solid = sample.is_some_and(CollisionSemantics::collision_occupied);
                let cave_void = inner
                    .plan
                    .cave_occupancy_arbitration(world_x, world_y, world_z)
                    .is_finally_void();
                if solid || cave_void {
                    occupied.insert(
                        [x, y, z],
                        OccupiedCell {
                            local: [x, y, z],
                            palette_index: sample.map_or(1, |voxel| voxel.palette_index.max(1)),
                        },
                    );
                }
            }
        }
    }
    occupied.into_values().collect()
}

fn cell_faces_cave_void(inner: &ProductionSpineInner, position: BlockPosition) -> bool {
    let neighbors = [
        [position.x.saturating_sub(1), position.y, position.z],
        [position.x.saturating_add(1), position.y, position.z],
        [position.x, position.y.saturating_sub(1), position.z],
        [position.x, position.y.saturating_add(1), position.z],
        [position.x, position.y, position.z.saturating_sub(1)],
        [position.x, position.y, position.z.saturating_add(1)],
    ];
    neighbors.iter().any(|&[x, y, z]| {
        inner
            .plan
            .cave_occupancy_arbitration(i64::from(x), i64::from(y), i64::from(z))
            .is_finally_void()
    })
}

fn player_sample_cells(translation: Vec3) -> [[i64; 3]; 2] {
    let profile = PlayerMovementProfileV1::default();
    let feet_y = translation.y - profile.capsule_total_height_m() * 0.5;
    [
        [
            f32_floor_i64(translation.x),
            f32_floor_i64(feet_y),
            f32_floor_i64(translation.z),
        ],
        [
            f32_floor_i64(translation.x),
            f32_floor_i64(translation.y),
            f32_floor_i64(translation.z),
        ],
    ]
}

fn player_occupied_chunks(
    translation: Vec3,
    edge: u16,
) -> Result<BTreeSet<ChunkCoordinate>, ProductionHostError> {
    let _ = translation_chunk(translation, edge).ok_or(ProductionHostError::InvalidPlayerPose)?;
    Ok(player_sample_cells(translation)
        .iter()
        .map(|&[x, y, z]| world_chunk(x, y, z, edge))
        .collect())
}

fn world_chunk(x: i64, y: i64, z: i64, edge: u16) -> ChunkCoordinate {
    let edge = i64::from(edge).max(1);
    ChunkCoordinate::new(
        i32::try_from(x.div_euclid(edge)).unwrap_or(0),
        i32::try_from(y.div_euclid(edge)).unwrap_or(0),
        i32::try_from(z.div_euclid(edge)).unwrap_or(0),
    )
}

#[allow(clippy::cast_possible_truncation)]
fn f32_floor_i64(value: f32) -> i64 {
    if value.is_finite() {
        value.floor() as i64
    } else {
        0
    }
}

fn chunk_of(position: BlockPosition, edge: u16) -> ChunkCoordinate {
    let edge = i32::from(edge);
    ChunkCoordinate::new(
        position.x.div_euclid(edge),
        position.y.div_euclid(edge),
        position.z.div_euclid(edge),
    )
}

fn local_index(position: BlockPosition, edge: u16) -> [usize; 3] {
    let edge = i32::from(edge);
    [
        usize::try_from(position.x.rem_euclid(edge)).unwrap_or(0),
        usize::try_from(position.y.rem_euclid(edge)).unwrap_or(0),
        usize::try_from(position.z.rem_euclid(edge)).unwrap_or(0),
    ]
}

pub(super) const fn canonical_index(edge: usize, x: usize, y: usize, z: usize) -> usize {
    x + edge * (z + edge * y)
}

fn block_face(face: Face) -> BlockFaceV1 {
    match face {
        Face::PosX => BlockFaceV1::PositiveX,
        Face::NegX => BlockFaceV1::NegativeX,
        Face::PosY => BlockFaceV1::PositiveY,
        Face::NegY => BlockFaceV1::NegativeY,
        Face::PosZ => BlockFaceV1::PositiveZ,
        Face::NegZ => BlockFaceV1::NegativeZ,
    }
}

fn validate_client_observation(
    observation: Option<&ClientTargetObservationV1>,
    position: BlockPosition,
    face: BlockFaceV1,
    actual_revision: ChunkRevision,
    distance_m: f64,
) -> Result<(), BlockEditRejectV1> {
    let Some(observation) = observation else {
        return Ok(());
    };
    if observation.distance_mm > REACH_MM || distance_m > f64::from(MAX_BLOCK_EDIT_REACH_M) {
        return Err(BlockEditRejectV1::OutOfReach {
            maximum_mm: REACH_MM,
        });
    }
    if observation.position != position || observation.face != face {
        return Err(BlockEditRejectV1::Occluded);
    }
    if observation.chunk_revision != actual_revision {
        return Err(BlockEditRejectV1::StaleRevision {
            expected: observation.chunk_revision.get(),
            actual: actual_revision.get(),
        });
    }
    Ok(())
}

#[allow(clippy::cast_precision_loss)] // Edit overlap uses meter-space AABBs near the player.
fn cell_intersects_player(position: BlockPosition, eye_origin: [f32; 3]) -> bool {
    const PLAYER_RADIUS_M: f32 = 0.42;
    const PLAYER_HEIGHT_BELOW_EYE_M: f32 = 1.7;
    const PLAYER_HEIGHT_ABOVE_EYE_M: f32 = 0.15;
    let cell_min = [position.x as f32, position.y as f32, position.z as f32];
    let cell_max = [cell_min[0] + 1.0, cell_min[1] + 1.0, cell_min[2] + 1.0];
    let actor_min_y = eye_origin[1] - PLAYER_HEIGHT_BELOW_EYE_M;
    let actor_max_y = eye_origin[1] + PLAYER_HEIGHT_ABOVE_EYE_M;
    if cell_max[1] <= actor_min_y || cell_min[1] >= actor_max_y {
        return false;
    }
    let nearest_x = eye_origin[0].clamp(cell_min[0], cell_max[0]);
    let nearest_z = eye_origin[2].clamp(cell_min[2], cell_max[2]);
    let offset_x = eye_origin[0] - nearest_x;
    let offset_z = eye_origin[2] - nearest_z;
    offset_x.mul_add(offset_x, offset_z * offset_z) < PLAYER_RADIUS_M * PLAYER_RADIUS_M
}

#[allow(clippy::cast_precision_loss)] // V2 region stays near the origin.
fn chunk_origin(coordinate: ChunkCoordinate, edge: f32) -> Vec3 {
    Vec3::new(
        coordinate.x as f32 * edge,
        coordinate.y as f32 * edge,
        coordinate.z as f32 * edge,
    )
}

/// Identity of the local command origin spawned by the production host.
#[must_use]
pub(super) const fn local_player_id() -> PlayerId {
    PlayerId::new(1)
}

fn bind_gameplay_session(
    inner: &mut ProductionSpineInner,
    kernel: &MemoryTransactionKernel,
    catalog: GameplayCatalog,
    spawn_chunk: ChunkCoordinate,
) -> Result<(), ProductionHostError> {
    let snapshot = kernel.reference_snapshot(inner.world)?;
    let mut loaded = BTreeMap::new();
    for (key, stored) in snapshot.chunks() {
        loaded.insert(
            DimensionChunkKey::new(key.dimension.clone(), key.coordinate),
            stored.revision(),
        );
    }
    inner.gameplay = Some(ProductionGameplay::new(
        inner.world,
        catalog,
        local_player_id(),
        inner.dimension.clone(),
        DimensionChunkKey::new(inner.dimension.clone(), spawn_chunk),
        loaded,
        snapshot.revision(),
        inner.chunk_edge,
    )?);
    Ok(())
}

fn next_transaction_id(inner: &mut ProductionSpineInner) -> TransactionId {
    let transaction = TransactionId::from_u128(inner.next_transaction);
    inner.next_transaction = inner.next_transaction.saturating_add(1);
    transaction
}

fn record_edit_result(
    inner: &mut ProductionSpineInner,
    result: &Result<BlockEditSuccessV1, BlockEditRejectV1>,
) {
    match result {
        Ok(success) => {
            inner.last_success = Some(success.clone());
            inner.last_reject = None;
        }
        Err(reject) => inner.last_reject = Some(reject.clone()),
    }
}

#[allow(clippy::too_many_lines)]
fn commit_gameplay_storage(
    inner: &mut ProductionSpineInner,
    kernel: &MemoryTransactionKernel,
    transaction_id: TransactionId,
    voxel_write: Option<(BlockPosition, HostVoxel)>,
    fixed_tick: u64,
) -> Result<(), BlockEditRejectV1> {
    let pending = match inner
        .gameplay
        .as_ref()
        .and_then(ProductionGameplay::pending_storage_chunks)
    {
        Some(chunks) if !chunks.is_empty() => chunks.clone(),
        _ => return Ok(()),
    };
    let snapshot = kernel
        .reference_snapshot(inner.world)
        .map_err(|_| BlockEditRejectV1::StorageUnavailable)?;
    let mut mutations = Vec::new();
    for (chunk, domains) in &pending {
        let key = ChunkKey::new(inner.world, chunk.dimension.clone(), chunk.coordinate);
        let stored = snapshot
            .chunk(&key)
            .ok_or(BlockEditRejectV1::StorageUnavailable)?;
        let mut cells = if inner.runtime.is_resident(chunk.coordinate) {
            runtime_chunk_cells(inner, chunk.coordinate)
        } else {
            decode_cells(
                stored.data().voxels().bytes(),
                inner.chunk_edge,
                &inner.presentation,
            )
            .map_err(|_| BlockEditRejectV1::StorageUnavailable)?
        };
        if let Some((position, voxel)) = voxel_write
            && chunk_of(position, inner.chunk_edge) == chunk.coordinate
        {
            let local = local_index(position, inner.chunk_edge);
            let index =
                canonical_index(usize::from(inner.chunk_edge), local[0], local[1], local[2]);
            if let Some(cell) = cells.get_mut(index) {
                *cell = voxel;
            }
        }
        let mut entities = stored.data().persistent_entities().clone();
        if domains.contains(ChangedDomains::PERSISTENT_ENTITIES) {
            entities.insert(
                capture_entity(chunk.coordinate),
                runtime_capture_payload(
                    &inner.voxel_schema,
                    inner.voxel_schema_version,
                    transaction_id,
                    1,
                ),
            );
        }
        let mut continuations = stored.data().continuations().clone();
        if domains.contains(ChangedDomains::CONTINUATIONS) {
            continuations.insert(
                capture_continuation(chunk.coordinate),
                runtime_capture_payload(
                    &inner.voxel_schema,
                    inner.voxel_schema_version,
                    transaction_id,
                    2,
                ),
            );
        }
        let replacement = ChunkData::new(
            voxel_payload(&inner.voxel_schema, inner.voxel_schema_version, &cells),
            entities,
            continuations,
            stored.data().provenance().clone(),
        );
        let mut declared = ChangedDomains::NONE;
        if replacement.voxels() != stored.data().voxels() {
            declared = declared.union(ChangedDomains::VOXELS);
        }
        if replacement.persistent_entities() != stored.data().persistent_entities() {
            declared = declared.union(ChangedDomains::PERSISTENT_ENTITIES);
        }
        if replacement.continuations() != stored.data().continuations() {
            declared = declared.union(ChangedDomains::CONTINUATIONS);
        }
        if replacement.provenance() != stored.data().provenance() {
            declared = declared.union(ChangedDomains::PROVENANCE);
        }
        if declared.is_empty() {
            return Err(BlockEditRejectV1::StorageUnavailable);
        }
        mutations.push(ChunkMutation::new(
            key,
            ChunkRevisionExpectation::Exact(stored.revision()),
            declared,
            replacement,
        ));
    }
    let receipt = kernel
        .commit(WorldTransaction::new(
            transaction_id,
            inner.world,
            snapshot.revision(),
            mutations,
        ))
        .map_err(|error| {
            inner.last_stream_error = Some(error.to_string());
            BlockEditRejectV1::StorageUnavailable
        })?;
    inner
        .gameplay
        .as_mut()
        .ok_or(BlockEditRejectV1::ContentUnavailable)?
        .observe_storage_commit(&receipt)
        .map_err(|error| {
            inner.last_stream_error = Some(error.to_string());
            BlockEditRejectV1::StorageUnavailable
        })?;
    let published = kernel
        .reference_snapshot(inner.world)
        .map_err(|_| BlockEditRejectV1::StorageUnavailable)?;
    for chunk in pending.keys() {
        inner.edited.insert(chunk.coordinate);
        let key = ChunkKey::new(inner.world, chunk.dimension.clone(), chunk.coordinate);
        let stored = published
            .chunk(&key)
            .ok_or(BlockEditRejectV1::StorageUnavailable)?;
        project_stored(
            &mut inner.runtime,
            stored,
            inner.chunk_edge,
            FixedTick::new(fixed_tick),
            &inner.presentation,
            derived_requests(priority_with_distance(DerivedPriority::EDIT_TO_VISIBLE, 0)),
        )
        .map_err(|error| {
            inner.last_stream_error = Some(error.to_string());
            BlockEditRejectV1::StorageUnavailable
        })?;
        inner
            .lifecycle
            .entry(chunk.coordinate)
            .and_modify(|state| {
                if *state == ChunkLifecycle::Active {
                    *state = ChunkLifecycle::MeshCollider;
                }
            })
            .or_insert(ChunkLifecycle::Resident);
    }
    Ok(())
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::{
        CollisionSemantics, HostVoxel, InterestClass, MAIN_WORLD_APPLY_JOB_CAP, MeshPresentation,
        OccupiedBox, OccupiedCell, apply_waiting_derived, compound_collider, merge_occupied_boxes,
        stream_derived_priority,
    };
    use bevy::prelude::Vec3;
    use latticeaxiom_storage::ChunkCoordinate;
    use latticeaxiom_voxel_mesh::{LayerMergeKey, MeshBuffer};
    use latticeaxiom_voxel_runtime::{
        ApplyAdmission, DerivedApplyBudget, DerivedApplySlice, RuntimeLimits, WallClockNanos,
    };
    use std::{
        collections::{BTreeSet, VecDeque},
        sync::Arc,
    };

    #[test]
    fn stream_priority_is_class_first_then_near_to_far() {
        let core_far = stream_derived_priority(InterestClass::Core, u32::MAX);
        let pin_near = stream_derived_priority(InterestClass::Pin, 0);
        let retain_near = stream_derived_priority(InterestClass::Retain, 0);
        let prefetch_near = stream_derived_priority(InterestClass::Prefetch, 0);
        assert!(core_far < pin_near);
        assert!(pin_near < retain_near);
        assert!(retain_near < prefetch_near);
        assert!(
            stream_derived_priority(InterestClass::Core, 3)
                < stream_derived_priority(InterestClass::Core, 4)
        );
    }

    #[test]
    fn cloned_mesh_presentations_share_geometry_storage() {
        let geometry: Arc<MeshBuffer<LayerMergeKey>> = Arc::new(MeshBuffer::default());
        let presentation = MeshPresentation {
            coordinate: ChunkCoordinate::new(1, 2, 3),
            origin: Vec3::ZERO,
            geometry,
            bounds: None,
        };
        let cloned = presentation.clone();
        assert!(Arc::ptr_eq(&presentation.geometry, &cloned.geometry));
    }

    fn cell(x: u16, y: u16, z: u16) -> OccupiedCell {
        OccupiedCell {
            local: [x, y, z],
            palette_index: 1,
        }
    }

    fn rasterize(boxes: &[OccupiedBox]) -> BTreeSet<[u16; 3]> {
        let mut cells = BTreeSet::new();
        for merged in boxes {
            for dz in 0..merged.size[2] {
                for dy in 0..merged.size[1] {
                    for dx in 0..merged.size[0] {
                        assert!(
                            cells.insert([
                                merged.origin[0] + dx,
                                merged.origin[1] + dy,
                                merged.origin[2] + dz,
                            ]),
                            "merged boxes must not overlap at {:?}",
                            [
                                merged.origin[0] + dx,
                                merged.origin[1] + dy,
                                merged.origin[2] + dz,
                            ]
                        );
                    }
                }
            }
        }
        cells
    }

    fn assert_occupancy_equivalent(occupied: &[OccupiedCell]) {
        let expected: BTreeSet<[u16; 3]> = occupied.iter().map(|cell| cell.local).collect();
        let merged = merge_occupied_boxes(occupied);
        assert_eq!(rasterize(&merged), expected);
    }

    #[test]
    fn collision_occupancy_comes_from_content_semantics() {
        let passable = HostVoxel {
            palette_index: 7,
            fluid_palette_index: 0,
            collision_occupied: false,
            solid_style: None,
            fluid_style: None,
        };
        let solid = HostVoxel {
            collision_occupied: true,
            ..passable
        };
        assert!(!passable.collision_occupied());
        assert!(solid.collision_occupied());
    }

    #[test]
    fn empty_occupancy_has_no_collider() {
        assert!(merge_occupied_boxes(&[]).is_empty());
        assert!(compound_collider(&[]).is_none());
    }

    #[test]
    fn single_voxel_is_one_unit_box() {
        let occupied = [cell(3, 4, 5)];
        let merged = merge_occupied_boxes(&occupied);
        assert_eq!(
            merged,
            [OccupiedBox {
                origin: [3, 4, 5],
                size: [1, 1, 1],
            }]
        );
        assert_occupancy_equivalent(&occupied);
        assert!(compound_collider(&occupied).is_some());
    }

    #[test]
    fn rectangular_prism_merges_to_one_box() {
        let mut occupied = Vec::new();
        for z in 2..4u16 {
            for y in 1..4u16 {
                for x in 0..4u16 {
                    occupied.push(cell(x, y, z));
                }
            }
        }
        let merged = merge_occupied_boxes(&occupied);
        assert_eq!(
            merged,
            [OccupiedBox {
                origin: [0, 1, 2],
                size: [4, 3, 2],
            }]
        );
        assert_occupancy_equivalent(&occupied);
    }

    #[test]
    fn l_shape_keeps_exact_occupancy() {
        let occupied = [cell(0, 0, 0), cell(1, 0, 0), cell(0, 1, 0)];
        assert_occupancy_equivalent(&occupied);
        assert_eq!(merge_occupied_boxes(&occupied).len(), 2);
    }

    #[test]
    fn merge_order_is_stable_for_shuffled_input() {
        let occupied = [
            cell(2, 2, 2),
            cell(0, 0, 0),
            cell(1, 0, 0),
            cell(0, 1, 0),
            cell(5, 1, 3),
        ];
        let mut reversed = occupied;
        reversed.reverse();
        assert_eq!(
            merge_occupied_boxes(&occupied),
            merge_occupied_boxes(&reversed)
        );
        assert_occupancy_equivalent(&occupied);
    }

    #[test]
    fn solid_chunk_shape_count_is_far_below_occupied_count() {
        let mut occupied = Vec::with_capacity(512);
        for z in 0..8u16 {
            for y in 0..8u16 {
                for x in 0..8u16 {
                    occupied.push(cell(x, y, z));
                }
            }
        }
        let merged = merge_occupied_boxes(&occupied);
        assert_occupancy_equivalent(&occupied);
        assert_eq!(merged.len(), 1);
        assert_eq!(merged.first().map(|merged| merged.size), Some([8, 8, 8]));
        assert!(
            merged.len() * 8 < occupied.len(),
            "solid chunk shape count must be significantly below occupied count (shapes={}, occupied={})",
            merged.len(),
            occupied.len()
        );
        assert!(compound_collider(&occupied).is_some());
    }

    /// Exhaustive small-volume property test: merged boxes occupy exactly the
    /// same cells as the original occupancy set.
    #[test]
    fn property_merged_boxes_match_all_two_by_two_by_two_occupancy() {
        for occupancy in 0_u16..=u8::MAX.into() {
            let mut occupied = Vec::new();
            for z in 0..2u16 {
                for y in 0..2u16 {
                    for x in 0..2u16 {
                        let bit = x + 2 * (y + 2 * z);
                        if occupancy & (1 << bit) != 0 {
                            occupied.push(cell(x, y, z));
                        }
                    }
                }
            }
            assert_occupancy_equivalent(&occupied);
        }
    }

    /// Deterministic generated property corpus uses asymmetric extents so axis
    /// swaps and visit-order bugs cannot cancel out.
    #[test]
    fn property_merged_boxes_match_generated_asymmetric_occupancy() {
        for seed in 0_u64..96 {
            let mut state = seed.wrapping_add(0x9e37_79b9_7f4a_7c15);
            let mut occupied = Vec::new();
            for z in 0..5u16 {
                for y in 0..4u16 {
                    for x in 0..3u16 {
                        state ^= state << 13;
                        state ^= state >> 7;
                        state ^= state << 17;
                        if state % 3 != 0 {
                            occupied.push(cell(x, y, z));
                        }
                    }
                }
            }
            assert_occupancy_equivalent(&occupied);
        }
    }

    #[test]
    fn apply_waiting_derived_stops_before_emptying_over_cap_queue() {
        let budget = DerivedApplyBudget::desktop_reference_v1();
        let mut waiting: VecDeque<u64> = (0..20).map(|_| 1).collect();
        let mut slice = DerivedApplySlice::new();
        let stop = apply_waiting_derived(
            &mut waiting,
            |bytes| *bytes,
            |_, _| Ok(()),
            &mut slice,
            budget,
            || WallClockNanos::new(0),
        )
        .expect("stub apply cannot fail");
        assert_eq!(stop, ApplyAdmission::StopJobs);
        assert_eq!(slice.applied_jobs(), MAIN_WORLD_APPLY_JOB_CAP);
        assert_eq!(
            slice.applied_bytes(),
            u64::try_from(MAIN_WORLD_APPLY_JOB_CAP).expect("job cap fits u64")
        );
        assert_eq!(waiting.len(), 4, "jobs over the cap must remain queued");

        let half = RuntimeLimits::MAIN_WORLD_APPLY_BYTE_CAP / 2 + 1;
        let mut waiting = VecDeque::from([half, half, 1]);
        let mut slice = DerivedApplySlice::new();
        let stop = apply_waiting_derived(
            &mut waiting,
            |bytes| *bytes,
            |_, _| Ok(()),
            &mut slice,
            budget,
            || WallClockNanos::new(0),
        )
        .expect("stub apply cannot fail");
        assert_eq!(stop, ApplyAdmission::StopBytes);
        assert_eq!(slice.applied_jobs(), 1);
        assert_eq!(slice.applied_bytes(), half);
        assert_eq!(
            waiting.len(),
            2,
            "jobs that would exceed the byte cap must remain queued"
        );

        let oversized = RuntimeLimits::MAIN_WORLD_APPLY_BYTE_CAP.saturating_add(1);
        let mut waiting = VecDeque::from([oversized, 1]);
        let mut slice = DerivedApplySlice::new();
        let stop = apply_waiting_derived(
            &mut waiting,
            |bytes| *bytes,
            |_, _| Ok(()),
            &mut slice,
            budget,
            || WallClockNanos::new(0),
        )
        .expect("stub apply cannot fail");
        assert_eq!(stop, ApplyAdmission::StopBytes);
        assert_eq!(slice.applied_jobs(), 1);
        assert_eq!(slice.applied_bytes(), oversized);
        assert_eq!(
            waiting.len(),
            1,
            "an oversized first job must still apply so the queue cannot stall"
        );
    }

    #[test]
    fn apply_waiting_derived_stops_on_host_wall_clock() {
        let budget = DerivedApplyBudget::desktop_reference_v1();
        let mut waiting = VecDeque::from([1_u64, 1, 1]);
        let mut slice = DerivedApplySlice::new();
        let mut samples = [
            WallClockNanos::new(0),
            WallClockNanos::main_world_apply_cap(),
        ]
        .into_iter();
        let stop = apply_waiting_derived(
            &mut waiting,
            |bytes| *bytes,
            |_, _| Ok(()),
            &mut slice,
            budget,
            || {
                samples
                    .next()
                    .unwrap_or(WallClockNanos::main_world_apply_cap())
            },
        )
        .expect("stub apply cannot fail");
        assert_eq!(stop, ApplyAdmission::StopWallClock);
        assert_eq!(slice.applied_jobs(), 1);
        assert_eq!(waiting.len(), 2);
    }
}
