//! Public immutable DTOs for committed voxel projections and derived work.

use std::mem;

use latticeaxiom_core::WorldId;
use latticeaxiom_storage::{
    ChunkCoordinate, ChunkKey, ChunkRevision, CommitReceipt, DimensionId, ReferenceDurability,
    StoredChunk, TransactionId, VoxelRevision, WorldRevision,
};
use latticeaxiom_voxel_mesh::{
    Face, MeshSource, PaddedChunk, SourceEpoch, SourceFingerprint, SourceRevision,
};

use crate::{RuntimeError, RuntimeResult};

/// Accepted committed-projection break/place reach from ADR 0025.
pub const MAX_AUTHORITATIVE_REACH_METERS: f64 = 5.0;

/// Conservative retained-memory contract for values cloned into derived inputs.
///
/// The value includes inline and heap storage and must remain a deterministic
/// upper bound after [`Clone`]. Under-reporting is a provider contract fault.
pub trait RetainedBytes {
    /// Returns a conservative total retained-byte upper bound.
    fn retained_bytes(&self) -> u64;
}

macro_rules! fixed_retained_bytes {
    ($($value:ty),+ $(,)?) => {$(
        impl RetainedBytes for $value {
            fn retained_bytes(&self) -> u64 {
                u64::try_from(mem::size_of::<Self>()).unwrap_or(u64::MAX)
            }
        }
    )+};
}

fixed_retained_bytes!(
    bool, u8, u16, u32, u64, u128, usize, i8, i16, i32, i64, i128, isize, f32, f64
);

impl RetainedBytes for String {
    fn retained_bytes(&self) -> u64 {
        u64::try_from(mem::size_of::<Self>())
            .ok()
            .and_then(|inline| {
                u64::try_from(self.capacity())
                    .ok()
                    .and_then(|heap| inline.checked_add(heap))
            })
            .unwrap_or(u64::MAX)
    }
}

/// Collision occupancy used by the conservative committed-projection fallback.
pub trait CollisionSemantics {
    /// Returns whether the committed voxel must block collision queries.
    fn collision_occupied(&self) -> bool;
}

impl CollisionSemantics for bool {
    fn collision_occupied(&self) -> bool {
        *self
    }
}

macro_rules! nonzero_collision_semantics {
    ($($value:ty),+ $(,)?) => {$(
        impl CollisionSemantics for $value {
            fn collision_occupied(&self) -> bool { *self != 0 }
        }
    )+};
}

nonzero_collision_semantics!(
    u8, u16, u32, u64, u128, usize, i8, i16, i32, i64, i128, isize
);

/// Runtime epoch invalidating process-local derived jobs in one world session.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct WorldEpoch(u64);
impl WorldEpoch {
    /// Creates a caller-owned epoch.
    #[must_use]
    pub const fn new(value: u64) -> Self {
        Self(value)
    }
    /// Numeric epoch value.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

/// Caller-supplied fixed-tick sequence for latency diagnostics.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct FixedTick(u64);
impl FixedTick {
    /// Creates a fixed-tick value.
    #[must_use]
    pub const fn new(value: u64) -> Self {
        Self(value)
    }
    /// Numeric tick value.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

/// Stable process-local identity of one dispatched derived job.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct DerivedJobId(u64);
impl DerivedJobId {
    pub(crate) const fn new(value: u64) -> Self {
        Self(value)
    }
    /// Numeric process-local job value.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

/// Stable owner identity for cancellation routing and diagnostics.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct DerivedOwner(u64);
impl DerivedOwner {
    /// Creates a host-assigned owner identity.
    #[must_use]
    pub const fn new(value: u64) -> Self {
        Self(value)
    }
    /// Numeric owner identity.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

/// Host-assigned identity distinguishing simultaneous runtime instances.
///
/// The host must not reuse a generation while tickets from the old instance
/// may still be returned by an executor.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct RuntimeGeneration(u64);
impl RuntimeGeneration {
    /// Creates a process-local runtime generation.
    #[must_use]
    pub const fn new(value: u64) -> Self {
        Self(value)
    }
    /// Numeric generation value.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

/// Process-local cancellation identity bound to runtime, epoch, and job.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct CancellationToken {
    epoch: WorldEpoch,
    runtime_generation: RuntimeGeneration,
    job: DerivedJobId,
}
impl CancellationToken {
    pub(crate) const fn new(
        epoch: WorldEpoch,
        runtime_generation: RuntimeGeneration,
        job: DerivedJobId,
    ) -> Self {
        Self {
            epoch,
            runtime_generation,
            job,
        }
    }
    /// Runtime epoch that issued the token.
    #[must_use]
    pub const fn epoch(self) -> WorldEpoch {
        self.epoch
    }
    /// Runtime instance that issued the token.
    #[must_use]
    pub const fn runtime_generation(self) -> RuntimeGeneration {
        self.runtime_generation
    }
    /// Job bound to the token.
    #[must_use]
    pub const fn job(self) -> DerivedJobId {
        self.job
    }
}

/// Stable scope shared by every committed projection in one working set.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorkingSetScope {
    world: WorldId,
    dimension: DimensionId,
    epoch: WorldEpoch,
}
impl WorkingSetScope {
    /// Creates a world/dimension/session scope.
    #[must_use]
    pub const fn new(world: WorldId, dimension: DimensionId, epoch: WorldEpoch) -> Self {
        Self {
            world,
            dimension,
            epoch,
        }
    }
    /// Persistent world identity.
    #[must_use]
    pub const fn world(&self) -> WorldId {
        self.world
    }
    /// Stable dimension identity.
    #[must_use]
    pub const fn dimension(&self) -> &DimensionId {
        &self.dimension
    }
    /// Process-local world epoch.
    #[must_use]
    pub const fn epoch(&self) -> WorldEpoch {
        self.epoch
    }
}

/// Signed world-space voxel coordinate in native `(x, y, z)` order.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct VoxelCoordinate {
    /// +X right.
    pub x: i64,
    /// +Y up.
    pub y: i64,
    /// Depth axis; conventional forward is negative.
    pub z: i64,
}
impl VoxelCoordinate {
    /// Creates a world voxel coordinate.
    #[must_use]
    pub const fn new(x: i64, y: i64, z: i64) -> Self {
        Self { x, y, z }
    }
    pub(crate) const fn as_array(self) -> [i64; 3] {
        [self.x, self.y, self.z]
    }
}

/// Local voxel coordinate inside one cubic chunk.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct LocalVoxelCoordinate {
    /// Local X.
    pub x: u16,
    /// Local Y.
    pub y: u16,
    /// Local Z.
    pub z: u16,
}
impl LocalVoxelCoordinate {
    /// Creates a local coordinate.
    #[must_use]
    pub const fn new(x: u16, y: u16, z: u16) -> Self {
        Self { x, y, z }
    }
}

macro_rules! semantic_fingerprint {
    ($name:ident, $documentation:literal) => {
        #[doc = $documentation]
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub struct $name([u8; 32]);
        impl $name {
            /// Creates canonical fingerprint bytes.
            #[must_use]
            pub const fn new(bytes: [u8; 32]) -> Self {
                Self(bytes)
            }
            /// Returns canonical bytes.
            #[must_use]
            pub const fn into_bytes(self) -> [u8; 32] {
                self.0
            }
        }
    };
}
semantic_fingerprint!(
    MeshSemanticFingerprint,
    "Presentation and voxel-face semantics affecting mesh derivation."
);
semantic_fingerprint!(
    ColliderSemanticFingerprint,
    "Occupancy and collision semantics affecting collider derivation."
);

/// Storage evidence used to build a decoded committed projection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProjectionEvidence {
    /// Complete atomic publication acknowledged by storage.
    CommitReceipt {
        /// Transaction identity.
        transaction: TransactionId,
        /// Current reference acknowledgement level.
        durability: ReferenceDurability,
    },
    /// Reconstructed directly from a storage-owned snapshot.
    StoredSnapshot,
}

/// Immutable decoded chunk carrying storage-issued publication evidence.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommittedChunkProjection<V> {
    key: ChunkKey,
    world_revision: WorldRevision,
    revision: ChunkRevision,
    voxel_revision: VoxelRevision,
    edge: u16,
    cells: Vec<V>,
    mesh_semantics: MeshSemanticFingerprint,
    collider_semantics: ColliderSemanticFingerprint,
    evidence: ProjectionEvidence,
}
impl<V> CommittedChunkProjection<V> {
    /// Decodes one chunk published by a storage commit receipt.
    ///
    /// # Errors
    ///
    /// Returns an error when the key is absent or the cubic shape is invalid.
    #[allow(
        clippy::too_many_arguments,
        reason = "both semantic domains and storage evidence are explicit"
    )]
    pub fn from_commit_receipt(
        receipt: &CommitReceipt,
        key: ChunkKey,
        edge: u16,
        cells: Vec<V>,
        mesh_semantics: MeshSemanticFingerprint,
        collider_semantics: ColliderSemanticFingerprint,
    ) -> RuntimeResult<Self> {
        let coordinate = key.coordinate;
        let committed = receipt
            .chunks()
            .iter()
            .find(|chunk| chunk.key() == &key)
            .ok_or_else(|| RuntimeError::ChunkMissingFromCommit {
                coordinate,
                world_revision: receipt.world_revision(),
            })?;
        validate_projection_shape(coordinate, edge, cells.len())?;
        Ok(Self {
            key,
            world_revision: receipt.world_revision(),
            revision: committed.chunk_revision(),
            voxel_revision: committed.domain_revisions().voxels(),
            edge,
            cells,
            mesh_semantics,
            collider_semantics,
            evidence: ProjectionEvidence::CommitReceipt {
                transaction: receipt.transaction_id(),
                durability: receipt.durability(),
            },
        })
    }
    /// Decodes one storage-owned reconstructed chunk.
    ///
    /// # Errors
    ///
    /// Returns an error for an unsupported edge or cell count.
    pub fn from_stored_chunk(
        stored: &StoredChunk,
        edge: u16,
        cells: Vec<V>,
        mesh_semantics: MeshSemanticFingerprint,
        collider_semantics: ColliderSemanticFingerprint,
    ) -> RuntimeResult<Self> {
        validate_projection_shape(stored.key().coordinate, edge, cells.len())?;
        Ok(Self {
            key: stored.key().clone(),
            world_revision: stored.captured_world_revision(),
            revision: stored.revision(),
            voxel_revision: stored.domain_revisions().voxels(),
            edge,
            cells,
            mesh_semantics,
            collider_semantics,
            evidence: ProjectionEvidence::StoredSnapshot,
        })
    }
    /// Complete committed chunk key.
    #[must_use]
    pub const fn key(&self) -> &ChunkKey {
        &self.key
    }
    /// Storage publication revision.
    #[must_use]
    pub const fn world_revision(&self) -> WorldRevision {
        self.world_revision
    }
    /// Total storage-issued chunk revision.
    #[must_use]
    pub const fn revision(&self) -> ChunkRevision {
        self.revision
    }
    /// Storage-issued voxel revision.
    #[must_use]
    pub const fn voxel_revision(&self) -> VoxelRevision {
        self.voxel_revision
    }
    /// Cubic edge.
    #[must_use]
    pub const fn edge(&self) -> u16 {
        self.edge
    }
    /// Decoded committed cells.
    #[must_use]
    pub fn cells(&self) -> &[V] {
        &self.cells
    }
    /// Mesh-only semantics.
    #[must_use]
    pub const fn mesh_semantics(&self) -> MeshSemanticFingerprint {
        self.mesh_semantics
    }
    /// Collider-only semantics.
    #[must_use]
    pub const fn collider_semantics(&self) -> ColliderSemanticFingerprint {
        self.collider_semantics
    }
    /// Storage construction evidence.
    #[must_use]
    pub const fn evidence(&self) -> ProjectionEvidence {
        self.evidence
    }
    pub(crate) fn into_parts(self) -> ProjectionParts<V> {
        ProjectionParts {
            world_revision: self.world_revision,
            revision: self.revision,
            voxel_revision: self.voxel_revision,
            cells: self.cells,
            mesh_semantics: self.mesh_semantics,
            collider_semantics: self.collider_semantics,
        }
    }
}

#[derive(Debug)]
pub(super) struct ProjectionParts<V> {
    pub(super) world_revision: WorldRevision,
    pub(super) revision: ChunkRevision,
    pub(super) voxel_revision: VoxelRevision,
    pub(super) cells: Vec<V>,
    pub(super) mesh_semantics: MeshSemanticFingerprint,
    pub(super) collider_semantics: ColliderSemanticFingerprint,
}
/// Kind of discardable derived work.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum DerivedKind {
    /// CPU terrain mesh.
    Mesh,
    /// Backend-neutral collider.
    Collider,
}
impl DerivedKind {
    /// Both kinds in stable order.
    pub const ALL: [Self; 2] = [Self::Mesh, Self::Collider];
    pub(crate) const fn index(self) -> usize {
        match self {
            Self::Mesh => 0,
            Self::Collider => 1,
        }
    }
}

/// Caller-owned deterministic priority; lower values dispatch first.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct DerivedPriority(u16);
impl DerivedPriority {
    /// Creates a priority bucket.
    #[must_use]
    pub const fn new(value: u16) -> Self {
        Self(value)
    }
    /// Numeric priority.
    #[must_use]
    pub const fn get(self) -> u16 {
        self.0
    }
}

/// Upper bounds for retained result and apply-staging memory.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DerivedMemoryBudget {
    result_bytes: u64,
    apply_bytes: u64,
}
impl DerivedMemoryBudget {
    /// Creates retained-memory limits.
    #[must_use]
    pub const fn new(result_bytes: u64, apply_bytes: u64) -> Self {
        Self {
            result_bytes,
            apply_bytes,
        }
    }
    /// Maximum result bytes.
    #[must_use]
    pub const fn result_bytes(self) -> u64 {
        self.result_bytes
    }
    /// Maximum apply-staging bytes.
    #[must_use]
    pub const fn apply_bytes(self) -> u64 {
        self.apply_bytes
    }
}

/// Scheduling, ownership, and retained-memory metadata for one kind.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DerivedRequest {
    priority: DerivedPriority,
    owner: DerivedOwner,
    memory: DerivedMemoryBudget,
}
impl DerivedRequest {
    /// Creates a derived-work request.
    #[must_use]
    pub const fn new(
        priority: DerivedPriority,
        owner: DerivedOwner,
        memory: DerivedMemoryBudget,
    ) -> Self {
        Self {
            priority,
            owner,
            memory,
        }
    }
    /// Dispatch priority.
    #[must_use]
    pub const fn priority(self) -> DerivedPriority {
        self.priority
    }
    /// Cancellation owner.
    #[must_use]
    pub const fn owner(self) -> DerivedOwner {
        self.owner
    }
    /// Result/apply retained-memory limits.
    #[must_use]
    pub const fn memory(self) -> DerivedMemoryBudget {
        self.memory
    }
}

/// Mesh and collider requests supplied together for projection invalidation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DerivedRequestSet {
    mesh: DerivedRequest,
    collider: DerivedRequest,
}
impl DerivedRequestSet {
    /// Creates independent request policies.
    #[must_use]
    pub const fn new(mesh: DerivedRequest, collider: DerivedRequest) -> Self {
        Self { mesh, collider }
    }
    /// Request for one kind.
    #[must_use]
    pub const fn get(self, kind: DerivedKind) -> DerivedRequest {
        match kind {
            DerivedKind::Mesh => self.mesh,
            DerivedKind::Collider => self.collider,
        }
    }
}

/// Revision state contributed by one face-adjacent committed projection.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum NeighborRevision {
    /// No committed neighbor is resident.
    Missing,
    /// Resident committed publication and domain revisions.
    Resident {
        /// Storage publication revision.
        world_revision: WorldRevision,
        /// Total chunk revision.
        revision: ChunkRevision,
        /// Voxel revision.
        voxel_revision: VoxelRevision,
    },
}

/// Six neighbor revisions in [`Face::ALL`] order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct NeighborRevisions([NeighborRevision; 6]);
impl NeighborRevisions {
    pub(crate) const fn new(values: [NeighborRevision; 6]) -> Self {
        Self(values)
    }
    /// State for one face.
    #[must_use]
    pub const fn get(self, face: Face) -> NeighborRevision {
        self.0[face.index()]
    }
    /// Stable face-ordered values.
    #[must_use]
    pub const fn as_array(self) -> [NeighborRevision; 6] {
        self.0
    }
}

/// Kind-specific semantic fingerprint in a derived key.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum DerivedSemanticFingerprint {
    /// Mesh semantics.
    Mesh(MeshSemanticFingerprint),
    /// Collider semantics.
    Collider(ColliderSemanticFingerprint),
}

/// Canonical complete source fingerprint.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct DerivedSourceFingerprint([u8; 32]);
impl DerivedSourceFingerprint {
    pub(crate) const fn new(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }
    /// Returns canonical bytes.
    #[must_use]
    pub const fn into_bytes(self) -> [u8; 32] {
        self.0
    }
}

/// Complete immutable identity of mesh or collider work.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct DerivedJobKey {
    kind: DerivedKind,
    coordinate: ChunkCoordinate,
    epoch: WorldEpoch,
    world_revision: WorldRevision,
    revision: ChunkRevision,
    voxel_revision: VoxelRevision,
    neighbors: NeighborRevisions,
    semantics: DerivedSemanticFingerprint,
    source_fingerprint: DerivedSourceFingerprint,
}
impl DerivedJobKey {
    #[allow(
        clippy::too_many_arguments,
        reason = "source invalidation inputs are deliberately explicit"
    )]
    pub(crate) const fn new(
        kind: DerivedKind,
        coordinate: ChunkCoordinate,
        epoch: WorldEpoch,
        world_revision: WorldRevision,
        revision: ChunkRevision,
        voxel_revision: VoxelRevision,
        neighbors: NeighborRevisions,
        semantics: DerivedSemanticFingerprint,
        source_fingerprint: DerivedSourceFingerprint,
    ) -> Self {
        Self {
            kind,
            coordinate,
            epoch,
            world_revision,
            revision,
            voxel_revision,
            neighbors,
            semantics,
            source_fingerprint,
        }
    }
    /// Work kind.
    #[must_use]
    pub const fn kind(&self) -> DerivedKind {
        self.kind
    }
    /// Target coordinate.
    #[must_use]
    pub const fn coordinate(&self) -> ChunkCoordinate {
        self.coordinate
    }
    /// Runtime epoch.
    #[must_use]
    pub const fn epoch(&self) -> WorldEpoch {
        self.epoch
    }
    /// Storage world revision.
    #[must_use]
    pub const fn world_revision(&self) -> WorldRevision {
        self.world_revision
    }
    /// Total chunk revision.
    #[must_use]
    pub const fn revision(&self) -> ChunkRevision {
        self.revision
    }
    /// Voxel revision.
    #[must_use]
    pub const fn voxel_revision(&self) -> VoxelRevision {
        self.voxel_revision
    }
    /// Neighbor revisions.
    #[must_use]
    pub const fn neighbors(&self) -> NeighborRevisions {
        self.neighbors
    }
    /// Kind-specific semantics.
    #[must_use]
    pub const fn semantics(&self) -> DerivedSemanticFingerprint {
        self.semantics
    }
    /// Complete source fingerprint.
    #[must_use]
    pub const fn source_fingerprint(&self) -> DerivedSourceFingerprint {
        self.source_fingerprint
    }
    /// Adapts a mesh key to the CPU mesher receipt.
    #[must_use]
    pub fn mesh_source(&self) -> Option<MeshSource> {
        if self.kind != DerivedKind::Mesh {
            return None;
        }
        Some(MeshSource::new(
            latticeaxiom_voxel_mesh::ChunkCoordinate::new(
                i64::from(self.coordinate.x),
                i64::from(self.coordinate.y),
                i64::from(self.coordinate.z),
            ),
            SourceEpoch::new(self.epoch.get()),
            SourceRevision::new(self.revision.get()),
            SourceFingerprint::new(self.source_fingerprint.into_bytes()),
        ))
    }
}

/// Hard limits for one deterministic derived queue.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DerivedQueueLimits {
    pending_slots: usize,
    concurrency: usize,
    reserved_byte_budget: u64,
}
impl DerivedQueueLimits {
    /// Validates queue limits.
    ///
    /// # Errors
    ///
    /// Returns an error when any limit is zero.
    pub const fn new(
        max_pending: usize,
        max_in_flight: usize,
        max_reserved_bytes: u64,
    ) -> RuntimeResult<Self> {
        if max_pending == 0 {
            return Err(RuntimeError::InvalidLimit {
                name: "max_pending",
            });
        }
        if max_in_flight == 0 {
            return Err(RuntimeError::InvalidLimit {
                name: "max_in_flight",
            });
        }
        if max_reserved_bytes == 0 {
            return Err(RuntimeError::InvalidLimit {
                name: "max_reserved_bytes",
            });
        }
        Ok(Self {
            pending_slots: max_pending,
            concurrency: max_in_flight,
            reserved_byte_budget: max_reserved_bytes,
        })
    }
    /// Pending hard limit.
    #[must_use]
    pub const fn max_pending(self) -> usize {
        self.pending_slots
    }
    /// Concurrent job hard limit.
    #[must_use]
    pub const fn max_in_flight(self) -> usize {
        self.concurrency
    }
    /// Per-kind combined reservation limit.
    #[must_use]
    pub const fn max_reserved_bytes(self) -> u64 {
        self.reserved_byte_budget
    }
}

/// Hard limits for a committed projection working set.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RuntimeLimits {
    resident_chunks: usize,
    combined_reserved_bytes: u64,
    mesh: DerivedQueueLimits,
    collider: DerivedQueueLimits,
}
impl RuntimeLimits {
    /// Validates global limits.
    ///
    /// # Errors
    ///
    /// Returns an error when either global limit is zero.
    pub const fn new(
        max_resident_chunks: usize,
        max_combined_reserved_bytes: u64,
        mesh: DerivedQueueLimits,
        collider: DerivedQueueLimits,
    ) -> RuntimeResult<Self> {
        if max_resident_chunks == 0 {
            return Err(RuntimeError::InvalidLimit {
                name: "max_resident_chunks",
            });
        }
        if max_combined_reserved_bytes == 0 {
            return Err(RuntimeError::InvalidLimit {
                name: "max_combined_reserved_bytes",
            });
        }
        Ok(Self {
            resident_chunks: max_resident_chunks,
            combined_reserved_bytes: max_combined_reserved_bytes,
            mesh,
            collider,
        })
    }
    /// Resident projection limit.
    #[must_use]
    pub const fn max_resident_chunks(self) -> usize {
        self.resident_chunks
    }
    /// Cross-kind combined reservation limit.
    #[must_use]
    pub const fn max_combined_reserved_bytes(self) -> u64 {
        self.combined_reserved_bytes
    }
    /// Mesh limits.
    #[must_use]
    pub const fn mesh(self) -> DerivedQueueLimits {
        self.mesh
    }
    /// Collider limits.
    #[must_use]
    pub const fn collider(self) -> DerivedQueueLimits {
        self.collider
    }
}

/// Inclusive Chebyshev cube of chunks retained around a streaming interest center.
///
/// Radius is measured in chunk units with Bevy-native `(x, y, z)` order. A
/// radius of zero keeps only the center chunk in interest.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InterestWindow {
    center: ChunkCoordinate,
    radius_chunks: u32,
}

impl InterestWindow {
    /// Creates an inclusive Chebyshev interest cube.
    #[must_use]
    pub const fn new(center: ChunkCoordinate, radius_chunks: u32) -> Self {
        Self {
            center,
            radius_chunks,
        }
    }

    /// Interest center in canonical chunk coordinates.
    #[must_use]
    pub const fn center(self) -> ChunkCoordinate {
        self.center
    }

    /// Inclusive Chebyshev radius in chunk units.
    #[must_use]
    pub const fn radius_chunks(self) -> u32 {
        self.radius_chunks
    }

    /// Returns whether `coordinate` is inside this inclusive cube.
    #[must_use]
    pub const fn contains(self, coordinate: ChunkCoordinate) -> bool {
        self.chebyshev_distance(coordinate) <= self.radius_chunks
    }

    /// Chebyshev distance from the interest center to `coordinate`.
    #[must_use]
    pub const fn chebyshev_distance(self, coordinate: ChunkCoordinate) -> u32 {
        let dx = self.center.x.abs_diff(coordinate.x);
        let dy = self.center.y.abs_diff(coordinate.y);
        let dz = self.center.z.abs_diff(coordinate.z);
        let mut distance = dx;
        if dy > distance {
            distance = dy;
        }
        if dz > distance {
            distance = dz;
        }
        distance
    }
}

/// Result of requesting one derived job.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EnqueueDecision {
    /// Inserted.
    Enqueued,
    /// Replaced older pending source or priority.
    Replaced,
    /// Equal key already pending or active.
    AlreadyQueued,
    /// Pending capacity rejected it.
    RejectedCapacity,
    /// Combined reservation exceeds a hard limit.
    RejectedMemoryBudget,
}

/// Evidence for one derived queue request.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DerivedEnqueueReceipt {
    key: DerivedJobKey,
    decision: EnqueueDecision,
    reserved_bytes: u64,
}
impl DerivedEnqueueReceipt {
    pub(crate) const fn new(
        key: DerivedJobKey,
        decision: EnqueueDecision,
        reserved_bytes: u64,
    ) -> Self {
        Self {
            key,
            decision,
            reserved_bytes,
        }
    }
    /// Requested key.
    #[must_use]
    pub const fn key(&self) -> &DerivedJobKey {
        &self.key
    }
    /// Queue decision.
    #[must_use]
    pub const fn decision(&self) -> EnqueueDecision {
        self.decision
    }
    /// Combined reservation.
    #[must_use]
    pub const fn reserved_bytes(&self) -> u64 {
        self.reserved_bytes
    }
}

/// How a storage-issued projection affected residency.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProjectionDecision {
    /// Newly resident.
    Admitted,
    /// Newer projection replaced it.
    Updated,
    /// Identical idempotent replay.
    Replay,
}

/// Receipt for projecting storage-committed state into the cache.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProjectionReceipt {
    pub(crate) coordinate: ChunkCoordinate,
    pub(crate) decision: ProjectionDecision,
    pub(crate) previous_world_revision: Option<WorldRevision>,
    pub(crate) world_revision: WorldRevision,
    pub(crate) revision: ChunkRevision,
    pub(crate) voxel_revision: VoxelRevision,
    pub(crate) changed_cells: usize,
    pub(crate) derived: Vec<DerivedEnqueueReceipt>,
}
impl ProjectionReceipt {
    /// Projected coordinate.
    #[must_use]
    pub const fn coordinate(&self) -> ChunkCoordinate {
        self.coordinate
    }
    /// Admission/update/replay decision.
    #[must_use]
    pub const fn decision(&self) -> ProjectionDecision {
        self.decision
    }
    /// Previously resident world revision.
    #[must_use]
    pub const fn previous_world_revision(&self) -> Option<WorldRevision> {
        self.previous_world_revision
    }
    /// Projected world revision.
    #[must_use]
    pub const fn world_revision(&self) -> WorldRevision {
        self.world_revision
    }
    /// Projected total revision.
    #[must_use]
    pub const fn revision(&self) -> ChunkRevision {
        self.revision
    }
    /// Projected voxel revision.
    #[must_use]
    pub const fn voxel_revision(&self) -> VoxelRevision {
        self.voxel_revision
    }
    /// Changed decoded cell count.
    #[must_use]
    pub const fn changed_cells(&self) -> usize {
        self.changed_cells
    }
    /// Deterministic queue decisions.
    #[must_use]
    pub fn derived(&self) -> &[DerivedEnqueueReceipt] {
        &self.derived
    }
}

/// Caller lifecycle generation bound into an eviction permit.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct EvictionLeaseGeneration(u64);
impl EvictionLeaseGeneration {
    /// Creates a lease generation.
    #[must_use]
    pub const fn new(value: u64) -> Self {
        Self(value)
    }
    /// Numeric generation.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

/// Unforgeable proof that an exact clean committed projection may be evicted.
#[derive(Debug)]
pub struct EvictionPermit {
    pub(crate) key: ChunkKey,
    pub(crate) world_revision: WorldRevision,
    pub(crate) revision: ChunkRevision,
    pub(crate) voxel_revision: VoxelRevision,
    pub(crate) lease: EvictionLeaseGeneration,
}
impl EvictionPermit {
    /// Permitted key.
    #[must_use]
    pub const fn key(&self) -> &ChunkKey {
        &self.key
    }
    /// Exact committed world revision.
    #[must_use]
    pub const fn world_revision(&self) -> WorldRevision {
        self.world_revision
    }
    /// Lifecycle lease generation.
    #[must_use]
    pub const fn lease(&self) -> EvictionLeaseGeneration {
        self.lease
    }
}

/// Immutable dispatched job ticket.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DerivedTicket {
    pub(crate) id: DerivedJobId,
    pub(crate) owner: DerivedOwner,
    pub(crate) cancel_token: CancellationToken,
    pub(crate) key: DerivedJobKey,
    pub(crate) input_bytes: u64,
    pub(crate) result_byte_budget: u64,
    pub(crate) apply_byte_budget: u64,
    pub(crate) reserved_bytes: u64,
    pub(crate) projected_tick: FixedTick,
}
impl DerivedTicket {
    /// Job ID.
    #[must_use]
    pub const fn id(&self) -> DerivedJobId {
        self.id
    }
    /// Executor/provider owner.
    #[must_use]
    pub const fn owner(&self) -> DerivedOwner {
        self.owner
    }
    /// Cancellation token.
    #[must_use]
    pub const fn cancellation_token(&self) -> CancellationToken {
        self.cancel_token
    }
    /// Source key.
    #[must_use]
    pub const fn key(&self) -> &DerivedJobKey {
        &self.key
    }
    /// Input reservation.
    #[must_use]
    pub const fn input_bytes(&self) -> u64 {
        self.input_bytes
    }
    /// Result limit.
    #[must_use]
    pub const fn result_byte_budget(&self) -> u64 {
        self.result_byte_budget
    }
    /// Apply-staging limit.
    #[must_use]
    pub const fn apply_byte_budget(&self) -> u64 {
        self.apply_byte_budget
    }
    /// Combined reservation retained through apply/cancel acknowledgement.
    #[must_use]
    pub const fn reserved_bytes(&self) -> u64 {
        self.reserved_bytes
    }
    /// Projection tick.
    #[must_use]
    pub const fn projected_tick(&self) -> FixedTick {
        self.projected_tick
    }
}

/// Cancellation request that does not release its job reservation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CancellationRequest {
    job: DerivedJobId,
    owner: DerivedOwner,
    token: CancellationToken,
    key: DerivedJobKey,
    reserved_bytes: u64,
}
impl CancellationRequest {
    pub(crate) fn new(ticket: &DerivedTicket) -> Self {
        Self {
            job: ticket.id,
            owner: ticket.owner,
            token: ticket.cancel_token,
            key: ticket.key.clone(),
            reserved_bytes: ticket.reserved_bytes,
        }
    }
    /// Job to cancel.
    #[must_use]
    pub const fn job(&self) -> DerivedJobId {
        self.job
    }
    /// Owner that must receive cancellation.
    #[must_use]
    pub const fn owner(&self) -> DerivedOwner {
        self.owner
    }
    /// Bound token.
    #[must_use]
    pub const fn token(&self) -> CancellationToken {
        self.token
    }
    /// Cancelled source key.
    #[must_use]
    pub const fn key(&self) -> &DerivedJobKey {
        &self.key
    }
    /// Reservation retained until acknowledgement.
    #[must_use]
    pub const fn reserved_bytes(&self) -> u64 {
        self.reserved_bytes
    }
}

/// Successful clean projection eviction receipt.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EvictionReceipt {
    pub(crate) coordinate: ChunkCoordinate,
    pub(crate) lease: EvictionLeaseGeneration,
    pub(crate) cancellations: Vec<CancellationRequest>,
    pub(crate) derived: Vec<DerivedEnqueueReceipt>,
}
impl EvictionReceipt {
    /// Evicted coordinate.
    #[must_use]
    pub const fn coordinate(&self) -> ChunkCoordinate {
        self.coordinate
    }
    /// Consumed lease.
    #[must_use]
    pub const fn lease(&self) -> EvictionLeaseGeneration {
        self.lease
    }
    /// Jobs now in `CancelRequested`; bytes remain reserved.
    #[must_use]
    pub fn cancellations(&self) -> &[CancellationRequest] {
        &self.cancellations
    }
    /// Neighbor rebuild requests.
    #[must_use]
    pub fn derived(&self) -> &[DerivedEnqueueReceipt] {
        &self.derived
    }
}

/// Immutable chunk plus one-voxel halo owned by an executor.
///
/// This value is deliberately not [`Clone`]. Returning it to completion or
/// cancellation acknowledgement proves release of the owned halo buffer.
#[derive(Debug, PartialEq, Eq)]
pub struct DerivedInput<V> {
    pub(crate) ticket: DerivedTicket,
    dimensions: PaddedChunk,
    samples: Vec<V>,
    actual_retained_bytes: u64,
}
impl<V> DerivedInput<V> {
    pub(crate) const fn new(
        ticket: DerivedTicket,
        dimensions: PaddedChunk,
        samples: Vec<V>,
        actual_retained_bytes: u64,
    ) -> Self {
        Self {
            ticket,
            dimensions,
            samples,
            actual_retained_bytes,
        }
    }
    /// Source ticket.
    #[must_use]
    pub const fn ticket(&self) -> &DerivedTicket {
        &self.ticket
    }
    /// Padded dimensions.
    #[must_use]
    pub const fn dimensions(&self) -> PaddedChunk {
        self.dimensions
    }
    /// X-fastest samples including halo.
    #[must_use]
    pub fn samples(&self) -> &[V] {
        &self.samples
    }
    /// Measured retained bytes after cloning.
    #[must_use]
    pub const fn actual_retained_bytes(&self) -> u64 {
        self.actual_retained_bytes
    }
}
/// Why a known completion no longer matches committed state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StaleReason {
    /// Target projection was evicted.
    TargetEvicted,
    /// Storage publication changed.
    WorldRevision,
    /// Total chunk revision changed.
    ChunkRevision,
    /// Voxel revision changed.
    VoxelRevision,
    /// Neighbor state changed.
    NeighborRevision {
        /// First changed stable face.
        face: Face,
    },
    /// Mesh-only semantics changed.
    MeshSemantics,
    /// Collider-only semantics changed.
    ColliderSemantics,
    /// Runtime epoch changed.
    WorldEpoch,
}

/// Retained-memory stage that violated its declaration.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MemoryStage {
    /// Halo input.
    Input,
    /// Derived result.
    Result,
    /// Apply staging.
    Apply,
}

/// Explicit apply-staging retained-byte declaration.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ApplyByteDeclaration(u64);
impl ApplyByteDeclaration {
    /// Creates an exact declaration.
    #[must_use]
    pub const fn new(bytes: u64) -> Self {
        Self(bytes)
    }
    /// Declared bytes.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

/// Successful apply and reservation-release evidence.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DerivedApplyReceipt {
    job: DerivedJobId,
    key: DerivedJobKey,
    completed_tick: FixedTick,
    commit_to_apply_ticks: u64,
    result_bytes: u64,
    apply_bytes: u64,
    released_reserved_bytes: u64,
}
impl DerivedApplyReceipt {
    #[allow(
        clippy::too_many_arguments,
        reason = "resource release evidence is explicit"
    )]
    pub(crate) const fn new(
        job: DerivedJobId,
        key: DerivedJobKey,
        completed_tick: FixedTick,
        commit_to_apply_ticks: u64,
        result_bytes: u64,
        apply_bytes: u64,
        released_reserved_bytes: u64,
    ) -> Self {
        Self {
            job,
            key,
            completed_tick,
            commit_to_apply_ticks,
            result_bytes,
            apply_bytes,
            released_reserved_bytes,
        }
    }
    /// Applied job.
    #[must_use]
    pub const fn job(&self) -> DerivedJobId {
        self.job
    }
    /// Accepted key.
    #[must_use]
    pub const fn key(&self) -> &DerivedJobKey {
        &self.key
    }
    /// Completion tick.
    #[must_use]
    pub const fn completed_tick(&self) -> FixedTick {
        self.completed_tick
    }
    /// Projection-to-apply latency.
    #[must_use]
    pub const fn commit_to_apply_ticks(&self) -> u64 {
        self.commit_to_apply_ticks
    }
    /// Measured result bytes.
    #[must_use]
    pub const fn result_bytes(&self) -> u64 {
        self.result_bytes
    }
    /// Declared apply bytes.
    #[must_use]
    pub const fn apply_bytes(&self) -> u64 {
        self.apply_bytes
    }
    /// Released combined reservation.
    #[must_use]
    pub const fn released_reserved_bytes(&self) -> u64 {
        self.released_reserved_bytes
    }
}

/// Resource release evidence for cancellation acknowledgement.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CancellationReleaseReceipt {
    job: DerivedJobId,
    input_bytes: u64,
    result_bytes: u64,
    released_reserved_bytes: u64,
}
impl CancellationReleaseReceipt {
    pub(crate) const fn new(
        job: DerivedJobId,
        input_bytes: u64,
        result_bytes: u64,
        released_reserved_bytes: u64,
    ) -> Self {
        Self {
            job,
            input_bytes,
            result_bytes,
            released_reserved_bytes,
        }
    }
    /// Released job.
    #[must_use]
    pub const fn job(self) -> DerivedJobId {
        self.job
    }
    /// Returned input bytes.
    #[must_use]
    pub const fn input_bytes(self) -> u64 {
        self.input_bytes
    }
    /// Returned result bytes.
    #[must_use]
    pub const fn result_bytes(self) -> u64 {
        self.result_bytes
    }
    /// Released combined reservation.
    #[must_use]
    pub const fn released_reserved_bytes(self) -> u64 {
        self.released_reserved_bytes
    }
}

/// Deterministic dispatch outcome.
#[derive(Debug, PartialEq, Eq)]
#[allow(
    clippy::large_enum_variant,
    reason = "boxing input adds a hot-path allocation"
)]
pub enum DispatchOutcome<V> {
    /// Captured and fully reserved.
    Started(DerivedInput<V>),
    /// Nothing pending.
    Empty,
    /// Temporarily resource-bound.
    Backpressured {
        /// Limiting resource.
        reason: BackpressureReason,
    },
    /// [`RetainedBytes`] under-reported cloned input.
    MemoryContractViolation {
        /// Source key.
        key: DerivedJobKey,
        /// Reserved input bytes.
        reserved_input_bytes: u64,
        /// Measured bytes.
        actual_input_bytes: u64,
    },
}

/// Hard resource preventing dispatch.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BackpressureReason {
    /// Concurrent jobs.
    InFlightJobs,
    /// Per-kind bytes.
    KindReservedBytes,
    /// Cross-kind bytes.
    CombinedReservedBytes,
    /// Target already has an old job.
    TargetInFlight,
}

/// Collider safety for a resident committed projection.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ColliderSafetyState {
    /// Host must query projected occupancy synchronously.
    PendingConservative {
        /// Current storage revision.
        world_revision: WorldRevision,
        /// Current voxel revision.
        voxel_revision: VoxelRevision,
    },
    /// Exact matching collider applied successfully.
    Ready {
        /// Matching collider source identity.
        source_fingerprint: DerivedSourceFingerprint,
        /// Matching storage revision.
        world_revision: WorldRevision,
        /// Matching voxel revision.
        voxel_revision: VoxelRevision,
    },
    /// Apply failed; projected occupancy remains mandatory.
    FailedConservative {
        /// Current storage revision.
        world_revision: WorldRevision,
        /// Current voxel revision.
        voxel_revision: VoxelRevision,
        /// Failure class.
        failure: ColliderFailure,
    },
}

/// Collider apply failure class.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ColliderFailure {
    /// Typed apply rejection.
    ApplyRejected,
    /// Contained panic.
    ApplyPanicked,
    /// Retained-memory violation.
    MemoryContractViolation,
}

/// Stale-gated completion result.
#[derive(Debug)]
#[allow(
    clippy::large_enum_variant,
    reason = "unknown resources must be returned without allocation"
)]
pub enum CompletionOutcome<A, E, V, R> {
    /// Apply succeeded; state advanced and reservation released.
    Applied {
        /// Receipt.
        receipt: DerivedApplyReceipt,
        /// Caller value.
        value: A,
    },
    /// Typed apply failed; old state remains.
    ApplyFailed {
        /// Job.
        job: DerivedJobId,
        /// Caller error.
        error: E,
    },
    /// Apply panicked but was contained.
    ApplyPanicked {
        /// Job.
        job: DerivedJobId,
    },
    /// Source was stale and never applied.
    StaleRejected {
        /// Job.
        job: DerivedJobId,
        /// First mismatch.
        reason: StaleReason,
    },
    /// Cancel-requested work returned and released resources.
    Cancelled {
        /// Release evidence.
        receipt: CancellationReleaseReceipt,
    },
    /// Result/apply memory exceeded its declaration.
    MemoryContractViolation {
        /// Job.
        job: DerivedJobId,
        /// Stage.
        stage: MemoryStage,
        /// Maximum.
        budget: u64,
        /// Actual.
        actual: u64,
    },
    /// Input belongs elsewhere; resources are returned.
    UnknownJob {
        /// Job.
        job: DerivedJobId,
        /// Returned input.
        input: DerivedInput<V>,
        /// Returned result.
        result: R,
    },
}

/// Cancellation acknowledgement result.
#[derive(Debug)]
#[allow(
    clippy::large_enum_variant,
    reason = "rejected acknowledgement returns owned resources"
)]
pub enum CancellationAckOutcome<V, R> {
    /// Cancel-requested job released.
    Released(CancellationReleaseReceipt),
    /// Result exceeded budget; resources still released fail-closed.
    MemoryContractViolation {
        /// Job.
        job: DerivedJobId,
        /// Maximum.
        budget: u64,
        /// Actual.
        actual: u64,
    },
    /// Job was active; resources returned.
    NotRequested {
        /// Input.
        input: DerivedInput<V>,
        /// Optional result.
        result: Option<R>,
    },
    /// Job was unknown; resources returned.
    UnknownJob {
        /// Input.
        input: DerivedInput<V>,
        /// Optional result.
        result: Option<R>,
    },
}

fn validate_projection_shape(
    coordinate: ChunkCoordinate,
    edge: u16,
    actual: usize,
) -> RuntimeResult<()> {
    PaddedChunk::new([usize::from(edge); 3])
        .map_err(|_| RuntimeError::InvalidChunkEdge { edge })?;
    let edge_usize = usize::from(edge);
    let expected = edge_usize
        .checked_mul(edge_usize)
        .and_then(|square| square.checked_mul(edge_usize))
        .ok_or(RuntimeError::InvalidChunkEdge { edge })?;
    if actual != expected {
        return Err(RuntimeError::InvalidCellCount {
            coordinate,
            expected,
            actual,
        });
    }
    Ok(())
}
