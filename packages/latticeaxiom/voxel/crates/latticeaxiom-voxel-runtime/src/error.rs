//! Typed committed-projection adapter failures.

use latticeaxiom_storage::{ChunkCoordinate, ChunkRevision, VoxelRevision, WorldRevision};
use thiserror::Error;

/// Result returned by voxel projection operations.
pub type RuntimeResult<T> = Result<T, RuntimeError>;

/// Failures detected before committed projection or derived state is changed.
#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum RuntimeError {
    /// A configured hard limit was zero.
    #[error("runtime limit {name} must be greater than zero")]
    InvalidLimit {
        /// Stable limit name.
        name: &'static str,
    },
    /// The cubic chunk edge is unsupported by the mesh halo contract.
    #[error("invalid cubic chunk edge {edge}")]
    InvalidChunkEdge {
        /// Rejected edge length.
        edge: u16,
    },
    /// A decoded projection did not contain exactly one cubic chunk.
    #[error("chunk {coordinate:?} has {actual} cells; expected {expected}")]
    InvalidCellCount {
        /// Affected chunk coordinate.
        coordinate: ChunkCoordinate,
        /// Expected cubic cell count.
        expected: usize,
        /// Supplied decoded cell count.
        actual: usize,
    },
    /// A commit receipt did not publish the requested chunk.
    #[error("commit revision {world_revision:?} did not publish chunk {coordinate:?}")]
    ChunkMissingFromCommit {
        /// Requested chunk.
        coordinate: ChunkCoordinate,
        /// Commit world revision.
        world_revision: WorldRevision,
    },
    /// A projection belongs to another world or dimension.
    #[error("chunk {coordinate:?} is outside this working-set scope")]
    ScopeMismatch {
        /// Rejected coordinate.
        coordinate: ChunkCoordinate,
    },
    /// A projection uses another chunk edge.
    #[error("chunk {coordinate:?} edge {actual} does not match runtime edge {expected}")]
    ChunkEdgeMismatch {
        /// Rejected coordinate.
        coordinate: ChunkCoordinate,
        /// Runtime edge.
        expected: u16,
        /// Projection edge.
        actual: u16,
    },
    /// The resident chunk hard limit was reached.
    #[error("resident chunk limit {limit} reached")]
    ResidentLimitExceeded {
        /// Configured hard limit.
        limit: usize,
    },
    /// An operation requires a resident committed projection.
    #[error("chunk {coordinate:?} is not resident")]
    ChunkNotResident {
        /// Missing coordinate.
        coordinate: ChunkCoordinate,
    },
    /// A projection predates the resident committed state.
    #[error(
        "stale projection for {coordinate:?}: incoming world revision {incoming:?}, current {current:?}"
    )]
    StaleProjection {
        /// Affected coordinate.
        coordinate: ChunkCoordinate,
        /// Resident committed world revision.
        current: WorldRevision,
        /// Rejected committed world revision.
        incoming: WorldRevision,
    },
    /// Equal world revisions carried conflicting decoded state.
    #[error("conflicting projection at world revision {world_revision:?} for {coordinate:?}")]
    ConflictingProjection {
        /// Affected coordinate.
        coordinate: ChunkCoordinate,
        /// Shared but conflicting world revision.
        world_revision: WorldRevision,
    },
    /// Total or voxel revisions regressed despite a newer world revision.
    #[error(
        "projection revision regression for {coordinate:?}: chunk {chunk_revision:?}, voxel {voxel_revision:?}"
    )]
    ProjectionRevisionRegression {
        /// Affected coordinate.
        coordinate: ChunkCoordinate,
        /// Rejected total chunk revision.
        chunk_revision: ChunkRevision,
        /// Rejected voxel-domain revision.
        voxel_revision: VoxelRevision,
    },
    /// An eviction permit no longer matches the exact resident projection.
    #[error("stale eviction permit for chunk {coordinate:?}")]
    StaleEvictionPermit {
        /// Affected coordinate.
        coordinate: ChunkCoordinate,
    },
    /// A dirty edited chunk is pinned in the bounded resident set.
    #[error("dirty edited chunk {coordinate:?} cannot be evicted")]
    DirtyChunkPinned {
        /// Pinned coordinate.
        coordinate: ChunkCoordinate,
    },
    /// Clean-outside-interest eviction requires a streaming interest window.
    #[error("interest window is required to evict clean generated chunks")]
    InterestWindowRequired,

    /// A world-space voxel coordinate cannot map into the canonical i32 chunk range.
    #[error("world voxel coordinate {coordinate:?} exceeds the canonical chunk range")]
    CoordinateOutOfRange {
        /// Rejected voxel coordinate.
        coordinate: crate::VoxelCoordinate,
    },
    /// Retained-byte accounting overflowed `u64`.
    #[error("retained-byte accounting overflow")]
    RetainedByteOverflow,
    /// A checked process-local counter was exhausted.
    #[error("{counter} overflow at chunk {coordinate:?}")]
    CounterOverflow {
        /// Stable counter name.
        counter: &'static str,
        /// Affected chunk.
        coordinate: ChunkCoordinate,
    },
    /// A DDA query contained a non-finite or otherwise invalid value.
    #[error("invalid committed-projection DDA query: {reason}")]
    InvalidDdaQuery {
        /// Stable validation explanation.
        reason: &'static str,
    },
    /// A committed-projection DDA query exceeded accepted gameplay reach.
    #[error("DDA reach {requested_millimeters} mm exceeds {maximum_millimeters} mm")]
    ReachExceeded {
        /// Requested reach in millimeters.
        requested_millimeters: u32,
        /// Hard maximum reach in millimeters.
        maximum_millimeters: u32,
    },
}
