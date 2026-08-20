//! Package-driven playable spine: storage, projection, meshing, and edits.

use std::{
    collections::{BTreeMap, BTreeSet},
    mem,
    sync::{Arc, Mutex},
};

use avian3d::prelude::Collider;
use bevy::prelude::{Quat, Resource, Vec3};
use latticeaxiom_compose::PlayableWorldHardLimitsV1;
use latticeaxiom_core::{SchemaId, WorldId};
use latticeaxiom_gameplay::{BlockId, BlockPosition, PlayerId};
use latticeaxiom_player::{
    AuthoritativeBlockEditRequestV1, AuthoritativeTargetInspectRequestV1, BlockEditActionV1,
    BlockEditAuthority, BlockEditRejectV1, BlockEditSuccessV1, BlockFaceV1,
    ClientTargetObservationV1, HeadlessTargetInspectV1, MAX_BLOCK_EDIT_REACH_M, TargetEyePoseV1,
    TargetInspectRejectV1,
};
use latticeaxiom_storage::{
    AuthoritativeTransactionKernel, ChangedDomains, ChunkCoordinate, ChunkData, ChunkKey,
    ChunkMutation, ChunkRevision, ChunkRevisionExpectation, DimensionId, MemoryTransactionKernel,
    PayloadSchemaVersion, StoredChunk, TransactionId, VersionedPayload, WorldTransaction,
};
use latticeaxiom_voxel_mesh::{
    Face, FaceDescriptor, FaceOcclusion, MeshGroup, MeshReceipt, MeshSource, PaddedChunk, Voxel,
    visible_faces,
};
use latticeaxiom_voxel_runtime::{
    ApplyByteDeclaration, CellSelection, ColliderSemanticFingerprint, CollisionSemantics,
    CommittedChunkProjection, CompletionOutcome, DdaOrigin, DdaOutcome, DdaQuery, DerivedInput,
    DerivedKind, DerivedMemoryBudget, DerivedOwner, DerivedPriority, DerivedQueueLimits,
    DerivedRequest, DerivedRequestSet, DispatchOutcome, EvictionLeaseGeneration, FixedTick,
    MeshSemanticFingerprint, RetainedBytes, RuntimeDiagnostics, RuntimeGeneration, RuntimeLimits,
    VoxelCoordinate, VoxelRuntime, WorkingSetScope, WorldEpoch,
};
use latticeaxiom_worldgen::{
    BoundedGeneratedRegionV1, GenerationPlanV1, MAX_BOUNDED_REGION_CHUNKS,
};

use super::{
    ChunkLifecycle, ChunkMeshCursor, ProductionHostError, ProductionPlayerPose,
    stream::{StreamClamps, desired_chunks, look_ahead_axis, prioritize_chunks},
    worldgen::{compile_plan, host_hard_limits, spine_config},
};
use crate::LockVerifiedComposeImages;

const MESH_SEMANTICS: MeshSemanticFingerprint = MeshSemanticFingerprint::new([0x51; 32]);
const COLLIDER_SEMANTICS: ColliderSemanticFingerprint =
    ColliderSemanticFingerprint::new([0xC1; 32]);
const VOXEL_SCHEMA: &str = "latticeaxiom:schema/chunk-voxels@1";
const WORLD_ID: &str = "00000000-0000-4000-8000-0000000000b1";
const REACH_MM: u16 = 5_000;

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
}

fn count_u32(value: usize) -> u32 {
    u32::try_from(value).unwrap_or(u32::MAX)
}

pub(super) struct ProductionSpineInner {
    runtime: VoxelRuntime<HostVoxel>,
    plan: GenerationPlanV1,
    clamps: StreamClamps,
    palette: Vec<BlockId>,
    empty: HostVoxel,
    chunk_edge: u16,
    world: WorldId,
    dimension: DimensionId,
    next_transaction: u128,
    voxel_schema: SchemaId,
    voxel_schema_version: PayloadSchemaVersion,
    derived: BTreeMap<ChunkCoordinate, ChunkDerived>,
    dirty: BTreeSet<ChunkCoordinate>,
    removed: BTreeSet<ChunkCoordinate>,
    edited: BTreeSet<ChunkCoordinate>,
    lifecycle: BTreeMap<ChunkCoordinate, ChunkLifecycle>,
    eviction_lease: u64,
    stream_anchor_xz: [f32; 2],
    spawn_center: Vec3,
    placement_content: BlockId,
    last_success: Option<BlockEditSuccessV1>,
    last_reject: Option<BlockEditRejectV1>,
    current_target: Option<HeadlessTargetInspectV1>,
    last_inspect: Option<Result<HeadlessTargetInspectV1, TargetInspectRejectV1>>,
    player_pose: ProductionPlayerPose,
    last_stream_error: Option<String>,
}

/// Collider upserts and evictions consumed by the production presentation system.
#[derive(Debug)]
pub(super) struct PresentationDelta {
    pub(super) upserts: Vec<(ChunkCoordinate, Collider, Vec3)>,
    pub(super) removals: Vec<ChunkCoordinate>,
}

#[derive(Clone, Debug)]
struct ChunkDerived {
    mesh_receipt: Option<MeshReceipt>,
    mesh_source: Option<MeshSource>,
    faces: BTreeSet<Face>,
    collider: Option<Collider>,
    revision: ChunkRevision,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct HostVoxel {
    palette_index: u16,
}

#[derive(Clone, Debug)]
struct HostDerivedMesh {
    mesh: Option<(MeshReceipt, MeshSource, BTreeSet<Face>)>,
    occupied: Vec<[u16; 3]>,
}

impl ProductionSpine {
    /// Materializes spawn-neighborhood interest into memory storage and projection.
    ///
    /// The host does not pre-generate a large finite map. Further chunks stream
    /// from player interest. This path does not open a durable writer.
    ///
    /// # Errors
    ///
    /// Returns [`ProductionHostError`] when lock-bound generation, storage
    /// publication, voxel projection, or derived meshing fails.
    #[allow(clippy::too_many_lines)]
    pub fn materialize(images: &LockVerifiedComposeImages) -> Result<Self, ProductionHostError> {
        let config = spine_config();
        let chunk_edge = config.chunk_edge_voxels;
        let plan = compile_plan(
            images.product_lock_hash(),
            images.images().registration().image.image_hash,
        )?;
        let world: WorldId = WORLD_ID.parse()?;
        let dimension = super::worldgen::dimension_id()?;
        let kernel = Arc::new(MemoryTransactionKernel::new());
        let palette = catalog_palette()?;
        let empty = HostVoxel { palette_index: 0 };
        let placement_content = BlockId::parse("terrenia:block/dirt")?;
        let voxel_schema: SchemaId = VOXEL_SCHEMA.parse()?;
        let voxel_schema_version =
            PayloadSchemaVersion::new(1).map_err(ProductionHostError::from)?;
        let clamps = StreamClamps::new(host_hard_limits()?, &config)?;
        let scope = WorkingSetScope::new(world, dimension.clone(), WorldEpoch::new(1));
        let limits = runtime_limits(clamps.hard_limits)?;
        let runtime =
            VoxelRuntime::new(scope, RuntimeGeneration::new(1), chunk_edge, empty, limits)?;

        let mut inner = ProductionSpineInner {
            runtime,
            plan,
            clamps,
            palette,
            empty,
            chunk_edge,
            world,
            dimension,
            next_transaction: 1,
            voxel_schema,
            voxel_schema_version,
            derived: BTreeMap::new(),
            dirty: BTreeSet::new(),
            removed: BTreeSet::new(),
            edited: BTreeSet::new(),
            lifecycle: BTreeMap::new(),
            eviction_lease: 0,
            stream_anchor_xz: [0.0, 0.0],
            spawn_center: Vec3::ZERO,
            placement_content,
            last_success: None,
            last_reject: None,
            current_target: None,
            last_inspect: None,
            player_pose: ProductionPlayerPose::default(),
            last_stream_error: None,
        };
        fill_working_set(
            &mut inner,
            &kernel,
            ChunkCoordinate::new(0, 0, 0),
            [0, 0],
            FixedTick::new(0),
        )?;
        place_exposed_probe(&mut inner, &kernel)?;
        inner.last_success = None;
        inner.spawn_center = find_spawn(&inner)?;
        inner.player_pose.translation = inner.spawn_center;
        inner.stream_anchor_xz = [inner.spawn_center.x, inner.spawn_center.z];
        let spawn_chunk = translation_chunk(inner.spawn_center, inner.chunk_edge)
            .ok_or(ProductionHostError::InvalidPlayerPose)?;
        fill_working_set(&mut inner, &kernel, spawn_chunk, [0, 0], FixedTick::new(0))?;

        Ok(Self {
            inner: Arc::new(Mutex::new(inner)),
            storage: ProductionWorldStorage { kernel },
        })
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

    /// Returns the union of derived mesh faces currently retained.
    #[must_use]
    pub fn visible_faces(&self) -> BTreeSet<Face> {
        match self.lock_inner() {
            Ok(inner) => {
                let mut faces = BTreeSet::new();
                for derived in inner.derived.values() {
                    faces.extend(derived.faces.iter().copied());
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

    /// Returns the structural host clamps for this session.
    #[must_use]
    pub fn hard_limits(&self) -> Option<PlayableWorldHardLimitsV1> {
        self.lock_inner().ok().map(|inner| inner.clamps.hard_limits)
    }

    /// Returns occupancy copied from [`VoxelRuntime`] diagnostics.
    #[must_use]
    pub fn working_set_diagnostics(&self) -> WorkingSetDiagnosticsV1 {
        self.lock_inner().map_or_else(
            |_| WorkingSetDiagnosticsV1::default(),
            |inner| WorkingSetDiagnosticsV1::from_runtime(inner.runtime.diagnostics()),
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

    /// Returns the last chunk-stream failure, if any.
    #[must_use]
    pub fn last_stream_error(&self) -> Option<String> {
        self.lock_inner()
            .ok()
            .and_then(|inner| inner.last_stream_error.clone())
    }

    fn sync_interest_inner(&self, fixed_tick: u64) -> Result<(), ProductionHostError> {
        let mut inner = self.lock_inner()?;
        let translation = inner.player_pose.translation;
        let chunk = translation_chunk(translation, inner.chunk_edge)
            .ok_or(ProductionHostError::InvalidPlayerPose)?;
        let look_ahead = look_ahead_axis([
            translation.x - inner.stream_anchor_xz[0],
            translation.z - inner.stream_anchor_xz[1],
        ]);
        inner.stream_anchor_xz = [translation.x, translation.z];
        let admit_limit = inner.clamps.max_in_flight();
        sync_working_set(
            &mut inner,
            self.storage.kernel(),
            chunk,
            look_ahead,
            FixedTick::new(fixed_tick),
            admit_limit,
        )
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
        let dirty = mem::take(&mut inner.dirty);
        let removals = mem::take(&mut inner.removed).into_iter().collect();
        let edge = f32::from(inner.chunk_edge);
        let mut upserts = Vec::new();
        for coordinate in dirty {
            let Some(derived) = inner.derived.get(&coordinate) else {
                continue;
            };
            let Some(collider) = derived.collider.clone() else {
                continue;
            };
            upserts.push((coordinate, collider, chunk_origin(coordinate, edge)));
        }
        Ok(PresentationDelta { upserts, removals })
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

        let (target, old_voxel, new_voxel) = match request.intent.action {
            BlockEditActionV1::Break => {
                if hit.voxel == self.empty {
                    return Err(BlockEditRejectV1::NotBreakable);
                }
                (hit.position, hit.voxel, self.empty)
            }
            BlockEditActionV1::Place => {
                let placement = request
                    .intent
                    .placement_content
                    .clone()
                    .ok_or(BlockEditRejectV1::NoPlacementContent)?;
                let palette_index = palette_index(&self.palette, &placement)
                    .ok_or(BlockEditRejectV1::ContentUnavailable)?;
                let adjacent = hit
                    .face
                    .adjacent(hit.position)
                    .ok_or(BlockEditRejectV1::PermissionDenied)?;
                if cell_intersects_player(adjacent, request.eye_pose.origin_m) {
                    return Err(BlockEditRejectV1::WouldIntersectActor);
                }
                let adjacent_voxel = VoxelCoordinate::new(
                    i64::from(adjacent.x),
                    i64::from(adjacent.y),
                    i64::from(adjacent.z),
                );
                let old = *self
                    .runtime
                    .cell(adjacent_voxel)
                    .map_err(|_| BlockEditRejectV1::PermissionDenied)?;
                if old != self.empty {
                    return Err(BlockEditRejectV1::NotReplaceable);
                }
                (adjacent, old, HostVoxel { palette_index })
            }
        };

        self.commit_cell(kernel, request.fixed_tick, target, old_voxel, new_voxel)
    }

    fn commit_cell(
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
        let mut cells = decode_cells(stored.data().voxels().bytes(), self.chunk_edge)
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
        )
        .map_err(|_| BlockEditRejectV1::StorageUnavailable)?;
        self.lifecycle
            .entry(coordinate)
            .and_modify(|state| {
                if *state == ChunkLifecycle::Active {
                    *state = ChunkLifecycle::MeshCollider;
                }
            })
            .or_insert(ChunkLifecycle::Resident);
        drain_derived(self, FixedTick::new(fixed_tick))
            .map_err(|_| BlockEditRejectV1::StorageUnavailable)?;
        refresh_lifecycle(self);
        Ok(BlockEditSuccessV1 {
            position,
            old_content: self.block_id(old_voxel),
            new_content: self.block_id(new_voxel),
            committed_chunk_revision: stored.revision(),
        })
    }

    fn block_id(&self, voxel: HostVoxel) -> Option<BlockId> {
        if voxel == self.empty {
            None
        } else {
            self.palette.get(usize::from(voxel.palette_index)).cloned()
        }
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
        Ok(HeadlessTargetInspectV1 {
            observation: ClientTargetObservationV1 {
                position: hit.position,
                face: hit.face,
                chunk_revision: hit.revision,
                distance_mm: quantized_distance_mm(hit.distance),
            },
            block_id,
        })
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
        self.palette_index != 0
    }
}

impl Voxel for HostVoxel {
    type MergeKey = u16;

    fn face(&self, _face: Face) -> Option<FaceDescriptor<u16>> {
        self.collision_occupied().then(|| {
            FaceDescriptor::new(MeshGroup::Opaque, self.palette_index, FaceOcclusion::Full)
        })
    }
}

impl RetainedBytes for HostDerivedMesh {
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
            sync_working_set(inner, kernel, origin, look_ahead, tick, limit)?;
            return Ok(());
        }
        let before = inner.runtime.diagnostics().resident_chunks();
        sync_working_set(inner, kernel, origin, look_ahead, tick, limit)?;
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
    admit_limit: usize,
) -> Result<(), ProductionHostError> {
    let desired = desired_chunks(origin, inner.clamps, look_ahead, &inner.edited);
    evict_unwanted(inner, &desired, tick)?;
    let ordered = prioritize_chunks(&desired, origin, look_ahead);
    admit_desired(inner, kernel, &ordered, tick, admit_limit)?;
    drain_derived(inner, tick)?;
    refresh_lifecycle(inner);
    Ok(())
}

fn evict_unwanted(
    inner: &mut ProductionSpineInner,
    desired: &BTreeSet<ChunkCoordinate>,
    tick: FixedTick,
) -> Result<(), ProductionHostError> {
    let victims = inner
        .lifecycle
        .keys()
        .copied()
        .filter(|coordinate| !desired.contains(coordinate) && !inner.edited.contains(coordinate))
        .collect::<Vec<_>>();
    for coordinate in victims {
        if !inner.runtime.is_resident(coordinate) {
            forget_chunk(inner, coordinate);
            continue;
        }
        inner.eviction_lease = inner.eviction_lease.saturating_add(1);
        let permit = inner.runtime.prepare_eviction(
            coordinate,
            EvictionLeaseGeneration::new(inner.eviction_lease),
        )?;
        inner
            .runtime
            .evict_committed(permit, tick, derived_requests())?;
        forget_chunk(inner, coordinate);
    }
    Ok(())
}

fn admit_desired(
    inner: &mut ProductionSpineInner,
    kernel: &MemoryTransactionKernel,
    ordered: &[ChunkCoordinate],
    tick: FixedTick,
    admit_limit: usize,
) -> Result<(), ProductionHostError> {
    let mut generate = Vec::new();
    let mut admitted = 0_usize;
    let snapshot = kernel.reference_snapshot(inner.world)?;
    let admit_limit = admit_limit.max(1);
    for coordinate in ordered {
        if inner.runtime.is_resident(*coordinate) {
            continue;
        }
        let upcoming = inner
            .runtime
            .diagnostics()
            .resident_chunks()
            .saturating_add(generate.len());
        if upcoming >= inner.clamps.max_resident() || admitted >= admit_limit {
            break;
        }
        let key = ChunkKey::new(inner.world, inner.dimension.clone(), *coordinate);
        if let Some(stored) = snapshot.chunk(&key) {
            inner.lifecycle.insert(*coordinate, ChunkLifecycle::Load);
            project_stored(&mut inner.runtime, stored, inner.chunk_edge, tick)?;
            inner
                .lifecycle
                .insert(*coordinate, ChunkLifecycle::Resident);
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
        admitted = admitted.saturating_add(1);
    }
    if generate.is_empty() {
        return Ok(());
    }
    publish_generated(inner, kernel, &generate, tick)
}

fn publish_generated(
    inner: &mut ProductionSpineInner,
    kernel: &MemoryTransactionKernel,
    coordinates: &[ChunkCoordinate],
    tick: FixedTick,
) -> Result<(), ProductionHostError> {
    let region = BoundedGeneratedRegionV1::materialize_coordinates(
        &inner.plan,
        coordinates.iter().copied(),
    )?;
    let mut mutations = Vec::with_capacity(region.len());
    for (coordinate, candidate) in region.candidates() {
        if !inner
            .lifecycle
            .get(&coordinate)
            .is_some_and(|state| *state == ChunkLifecycle::Generate)
        {
            continue;
        }
        let cells = draft_cells(candidate.draft(), &inner.palette)?;
        mutations.push(ChunkMutation::new(
            ChunkKey::new(inner.world, inner.dimension.clone(), coordinate),
            ChunkRevisionExpectation::Absent,
            ChangedDomains::ALL,
            chunk_data(&inner.voxel_schema, inner.voxel_schema_version, &cells),
        ));
    }
    if mutations.is_empty() {
        for coordinate in coordinates {
            if inner.lifecycle.get(coordinate) == Some(&ChunkLifecycle::Generate) {
                inner.lifecycle.remove(coordinate);
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
        project_stored(&mut inner.runtime, stored, inner.chunk_edge, tick)?;
        inner
            .lifecycle
            .insert(*coordinate, ChunkLifecycle::Resident);
    }
    Ok(())
}

fn forget_chunk(inner: &mut ProductionSpineInner, coordinate: ChunkCoordinate) {
    inner.lifecycle.remove(&coordinate);
    inner.derived.remove(&coordinate);
    inner.dirty.remove(&coordinate);
    inner.removed.insert(coordinate);
}

fn refresh_lifecycle(inner: &mut ProductionSpineInner) {
    let coordinates = inner.lifecycle.keys().copied().collect::<Vec<_>>();
    for coordinate in coordinates {
        let Some(state) = inner.lifecycle.get_mut(&coordinate) else {
            continue;
        };
        if !matches!(
            state,
            ChunkLifecycle::Resident | ChunkLifecycle::MeshCollider | ChunkLifecycle::Active
        ) {
            continue;
        }
        let Some(derived) = inner.derived.get(&coordinate) else {
            *state = ChunkLifecycle::Resident;
            continue;
        };
        if derived.mesh_receipt.is_some() {
            *state = ChunkLifecycle::Active;
        } else {
            *state = ChunkLifecycle::MeshCollider;
        }
    }
}

fn project_stored(
    runtime: &mut VoxelRuntime<HostVoxel>,
    stored: &StoredChunk,
    edge: u16,
    tick: FixedTick,
) -> Result<(), ProductionHostError> {
    let cells = decode_cells(stored.data().voxels().bytes(), edge)?;
    let projection = CommittedChunkProjection::from_stored_chunk(
        stored,
        edge,
        cells,
        MESH_SEMANTICS,
        COLLIDER_SEMANTICS,
    )?;
    runtime.project_committed(projection, tick, derived_requests())?;
    Ok(())
}

fn drain_derived(
    inner: &mut ProductionSpineInner,
    tick: FixedTick,
) -> Result<(), ProductionHostError> {
    for kind in DerivedKind::ALL {
        loop {
            match inner.runtime.dispatch_next(kind)? {
                DispatchOutcome::Empty | DispatchOutcome::Backpressured { .. } => break,
                DispatchOutcome::Started(input) => complete_derived(inner, kind, input, tick)?,
                DispatchOutcome::MemoryContractViolation { .. } => {
                    return Err(ProductionHostError::DerivedMemory);
                }
            }
        }
    }
    Ok(())
}

fn complete_derived(
    inner: &mut ProductionSpineInner,
    kind: DerivedKind,
    input: DerivedInput<HostVoxel>,
    tick: FixedTick,
) -> Result<(), ProductionHostError> {
    let coordinate = input.ticket().key().coordinate();
    let revision = input.ticket().key().revision();
    let derived = match kind {
        DerivedKind::Mesh => {
            let source = input
                .ticket()
                .key()
                .mesh_source()
                .ok_or(ProductionHostError::MissingMeshSource { coordinate })?;
            let mesh = visible_faces(input.samples(), input.dimensions(), source)?;
            let faces = mesh
                .geometry()
                .iter()
                .map(|(_, face, _)| face)
                .collect::<BTreeSet<_>>();
            HostDerivedMesh {
                mesh: Some((mesh.receipt(), source, faces)),
                occupied: occupied_voxels(input.samples(), input.dimensions())?,
            }
        }
        DerivedKind::Collider => HostDerivedMesh {
            mesh: None,
            occupied: occupied_voxels(input.samples(), input.dimensions())?,
        },
    };
    let apply_bytes = ApplyByteDeclaration::new(derived.retained_bytes());
    match inner
        .runtime
        .complete_derived(input, derived, apply_bytes, tick, |value| {
            Ok::<HostDerivedMesh, ProductionHostError>(value)
        }) {
        CompletionOutcome::Applied { value, .. } => {
            apply_derived(inner, coordinate, revision, kind, value);
            Ok(())
        }
        CompletionOutcome::StaleRejected { .. } | CompletionOutcome::Cancelled { .. } => Ok(()),
        CompletionOutcome::ApplyFailed { error, .. } => Err(error),
        CompletionOutcome::ApplyPanicked { .. } => Err(ProductionHostError::DerivedApplyPanicked),
        CompletionOutcome::MemoryContractViolation { .. }
        | CompletionOutcome::UnknownJob { .. } => Err(ProductionHostError::DerivedRejected),
    }
}

fn apply_derived(
    inner: &mut ProductionSpineInner,
    coordinate: ChunkCoordinate,
    revision: ChunkRevision,
    kind: DerivedKind,
    value: HostDerivedMesh,
) {
    let entry = inner
        .derived
        .entry(coordinate)
        .or_insert_with(|| ChunkDerived {
            mesh_receipt: None,
            mesh_source: None,
            faces: BTreeSet::new(),
            collider: None,
            revision,
        });
    entry.revision = revision;
    if let Some((receipt, source, faces)) = value.mesh {
        entry.mesh_receipt = Some(receipt);
        entry.mesh_source = Some(source);
        entry.faces = faces;
    }
    if kind == DerivedKind::Collider || entry.collider.is_none() {
        entry.collider = compound_collider(&value.occupied);
    }
    inner.dirty.insert(coordinate);
}

fn occupied_voxels(
    samples: &[HostVoxel],
    dimensions: PaddedChunk,
) -> Result<Vec<[u16; 3]>, ProductionHostError> {
    let interior = dimensions.interior_size();
    let mut occupied = Vec::new();
    for z in 0..interior[2] {
        for y in 0..interior[1] {
            for x in 0..interior[0] {
                let padded = [x + 1, y + 1, z + 1];
                let index = dimensions
                    .linearize(padded)
                    .ok_or(ProductionHostError::PaddedIndex)?;
                if samples
                    .get(index)
                    .is_some_and(CollisionSemantics::collision_occupied)
                {
                    occupied.push([
                        u16::try_from(x).map_err(|_| ProductionHostError::PaddedIndex)?,
                        u16::try_from(y).map_err(|_| ProductionHostError::PaddedIndex)?,
                        u16::try_from(z).map_err(|_| ProductionHostError::PaddedIndex)?,
                    ]);
                }
            }
        }
    }
    Ok(occupied)
}

fn compound_collider(occupied: &[[u16; 3]]) -> Option<Collider> {
    if occupied.is_empty() {
        return None;
    }
    let shapes = occupied
        .iter()
        .map(|[x, y, z]| {
            (
                Vec3::new(
                    f32::from(*x) + 0.5,
                    f32::from(*y) + 0.5,
                    f32::from(*z) + 0.5,
                ),
                Quat::IDENTITY,
                Collider::cuboid(1.0, 1.0, 1.0),
            )
        })
        .collect();
    Some(Collider::compound(shapes))
}

fn draft_cells(
    draft: &latticeaxiom_worldgen::ChunkDraftV1,
    palette: &[BlockId],
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
                cells.push(HostVoxel { palette_index });
            }
        }
    }
    Ok(cells)
}

fn chunk_data(
    schema: &SchemaId,
    schema_version: PayloadSchemaVersion,
    cells: &[HostVoxel],
) -> ChunkData {
    let mut bytes = Vec::with_capacity(cells.len().saturating_mul(2));
    for cell in cells {
        bytes.extend_from_slice(&cell.palette_index.to_le_bytes());
    }
    ChunkData::new(
        VersionedPayload::new(schema.clone(), schema_version, bytes),
        BTreeMap::new(),
        BTreeMap::new(),
        BTreeMap::new(),
    )
}

fn decode_cells(bytes: &[u8], edge: u16) -> Result<Vec<HostVoxel>, ProductionHostError> {
    let expected = usize::from(edge)
        .checked_pow(3)
        .and_then(|count| count.checked_mul(2))
        .ok_or(ProductionHostError::PayloadLength)?;
    if bytes.len() != expected {
        return Err(ProductionHostError::PayloadLength);
    }
    Ok(bytes
        .chunks_exact(2)
        .map(|pair| HostVoxel {
            palette_index: u16::from_le_bytes([pair[0], pair[1]]),
        })
        .collect())
}

fn catalog_palette() -> Result<Vec<BlockId>, ProductionHostError> {
    [
        "air",
        "grass",
        "dirt",
        "stone",
        "obsidian",
        "copper-block",
        "clay",
        "sand",
        "red-sand",
        "gravel",
        "limestone",
        "basalt",
        "sandstone",
        "red-sandstone",
        "copper-ore",
        "oak-log",
        "oak-leaves",
        "tall-grass",
    ]
    .into_iter()
    .map(|path| BlockId::parse(&format!("terrenia:block/{path}")))
    .collect::<Result<Vec<_>, _>>()
    .map_err(ProductionHostError::from)
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
    let max_in_flight = usize::try_from(hard_limits.max_in_flight_chunks).unwrap_or(4);
    let queue =
        DerivedQueueLimits::new(max_resident.max(1), max_in_flight.max(1), 4 * 1024 * 1024)?;
    Ok(RuntimeLimits::new(
        max_resident.max(1),
        32 * 1024 * 1024,
        queue,
        queue,
    )?)
}

fn derived_requests() -> DerivedRequestSet {
    let request = DerivedRequest::new(
        DerivedPriority::new(0),
        DerivedOwner::new(1),
        DerivedMemoryBudget::new(64 * 1024, 64 * 1024),
    );
    DerivedRequestSet::new(request, request)
}

#[allow(clippy::cast_precision_loss)] // Spawn stays inside the finite near-origin V2 region.
fn place_exposed_probe(
    inner: &mut ProductionSpineInner,
    kernel: &MemoryTransactionKernel,
) -> Result<(), ProductionHostError> {
    let stone = palette_index(&inner.palette, &BlockId::parse("terrenia:block/stone")?)
        .ok_or(ProductionHostError::UnknownDraftBlock)?;
    let probe = BlockPosition {
        x: -1,
        y: 30,
        z: -1,
    };
    inner
        .commit_cell(
            kernel,
            0,
            probe,
            inner.empty,
            HostVoxel {
                palette_index: stone,
            },
        )
        .map_err(|_| ProductionHostError::NoSafeSpawn)?;
    Ok(())
}

#[allow(clippy::cast_precision_loss)] // Spawn stays inside the finite near-origin V2 region.
fn find_spawn(inner: &ProductionSpineInner) -> Result<Vec3, ProductionHostError> {
    let profile = latticeaxiom_player::PlayerMovementProfileV1::default();
    for z in -5..-1 {
        for x in -5..-1 {
            if let Some(surface_y) = surface_y(inner, x, z) {
                let feet_y = (surface_y + 1) as f32;
                return Ok(Vec3::new(
                    x as f32 + 0.5,
                    feet_y + profile.capsule_total_height_m() * 0.5,
                    z as f32 + 0.5,
                ));
            }
        }
    }
    Err(ProductionHostError::NoSafeSpawn)
}

fn surface_y(inner: &ProductionSpineInner, x: i32, z: i32) -> Option<i32> {
    let ceiling = i32::from(inner.chunk_edge) * 4 - 1;
    for y in (0..=ceiling).rev() {
        let coordinate = VoxelCoordinate::new(i64::from(x), i64::from(y), i64::from(z));
        match inner.runtime.cell(coordinate) {
            Ok(voxel) if voxel.collision_occupied() => {
                let above = VoxelCoordinate::new(i64::from(x), i64::from(y) + 1, i64::from(z));
                let head = VoxelCoordinate::new(i64::from(x), i64::from(y) + 2, i64::from(z));
                let clear = inner
                    .runtime
                    .cell(above)
                    .ok()
                    .is_some_and(|cell| !cell.collision_occupied())
                    && inner
                        .runtime
                        .cell(head)
                        .ok()
                        .is_some_and(|cell| !cell.collision_occupied());
                if clear {
                    return Some(y);
                }
            }
            Ok(_) => {}
            Err(_) => return None,
        }
    }
    None
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

const fn canonical_index(edge: usize, x: usize, y: usize, z: usize) -> usize {
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
