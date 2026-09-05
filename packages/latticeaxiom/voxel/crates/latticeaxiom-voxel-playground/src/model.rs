//! Presentation-only voxel commands built from authoritative receipts.

use latticeaxiom_gameplay::{BlockId, BlockPosition};
use latticeaxiom_player::{BlockEditActionV1, BlockEditSuccessV1};
use latticeaxiom_storage::{ChunkCoordinate, ChunkRevision};
use latticeaxiom_voxel_runtime::{CommittedChunkProjection, VoxelCoordinate};
use thiserror::Error;

/// Maximum writes admitted by one projection batch.
///
/// This is one complete 32-cubed D2 chunk. Larger working sets are split by
/// authoritative chunk and revision before they reach the presentation sink.
pub const MAX_PROJECTION_BATCH_WRITES: usize = 32 * 32 * 32;

/// Error produced while translating authoritative data into presentation work.
#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum ProjectionError {
    /// A runtime coordinate cannot be represented by upstream's `IVec3` key.
    #[error("voxel coordinate {coordinate:?} is outside the upstream i32 coordinate range")]
    CoordinateOutOfRange {
        /// Rejected canonical `(x, y, z)` coordinate.
        coordinate: [i64; 3],
    },
    /// A zero chunk edge cannot define Euclidean chunk ownership.
    #[error("voxel chunk edge must be nonzero")]
    ZeroChunkEdge,
    /// The authoritative action and success payload disagree.
    #[error("{action:?} success has incompatible old/new block content")]
    ActionResultMismatch {
        /// Action whose success payload was rejected.
        action: BlockEditActionV1,
    },
    /// Stable block content has no presentation material mapping.
    #[error("stable block `{block}` has no upstream presentation material")]
    MaterialUnavailable {
        /// Stable authoritative block identity.
        block: String,
    },
    /// A projection batch was empty or exceeded its hard bound.
    #[error("projection batch contains {actual} writes; expected 1..={maximum}")]
    InvalidBatchSize {
        /// Maximum accepted writes.
        maximum: usize,
        /// Rejected write count.
        actual: usize,
    },
    /// A write belongs to a different chunk from its batch receipt.
    #[error(
        "projection write at {coordinate:?} belongs to {actual:?}, not declared chunk {expected:?}"
    )]
    ChunkMismatch {
        /// Batch source chunk.
        expected: ChunkCoordinate,
        /// Chunk calculated for the write.
        actual: ChunkCoordinate,
        /// Rejected write coordinate.
        coordinate: PresentationCoordinate,
    },
    /// A batch contains multiple writes for the same authoritative cell.
    #[error("projection batch contains duplicate write at {coordinate:?}")]
    DuplicateCoordinate {
        /// Repeated write coordinate.
        coordinate: PresentationCoordinate,
    },
}

/// Result returned while constructing presentation-only work.
pub type ProjectionResult<T> = Result<T, ProjectionError>;

/// Validated nonzero cubic chunk edge.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct VoxelChunkEdge(u16);

impl VoxelChunkEdge {
    /// D2's provisional 32-voxel chunk edge.
    pub const D2: Self = Self(32);

    /// Validates one cubic chunk edge.
    ///
    /// # Errors
    ///
    /// Returns [`ProjectionError::ZeroChunkEdge`] for zero.
    pub const fn new(edge: u16) -> ProjectionResult<Self> {
        if edge == 0 {
            return Err(ProjectionError::ZeroChunkEdge);
        }
        Ok(Self(edge))
    }

    /// Numeric edge in voxels.
    #[must_use]
    pub const fn get(self) -> u16 {
        self.0
    }
}

/// Signed upstream-addressable voxel position in native right-handed Y-up order.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct PresentationCoordinate {
    /// Horizontal coordinate increasing right.
    pub x: i32,
    /// Vertical coordinate increasing up.
    pub y: i32,
    /// Depth coordinate; conventional forward is negative Z.
    pub z: i32,
}

impl PresentationCoordinate {
    /// Creates a coordinate in canonical `(x, y, z)` order.
    #[must_use]
    pub const fn new(x: i32, y: i32, z: i32) -> Self {
        Self { x, y, z }
    }

    /// Returns the Euclidean chunk coordinate, including for negative axes.
    #[must_use]
    pub fn chunk(self, edge: VoxelChunkEdge) -> ChunkCoordinate {
        let divisor = i32::from(edge.get());
        ChunkCoordinate::new(
            self.x.div_euclid(divisor),
            self.y.div_euclid(divisor),
            self.z.div_euclid(divisor),
        )
    }
}

impl From<BlockPosition> for PresentationCoordinate {
    fn from(position: BlockPosition) -> Self {
        Self::new(position.x, position.y, position.z)
    }
}

impl TryFrom<VoxelCoordinate> for PresentationCoordinate {
    type Error = ProjectionError;

    fn try_from(coordinate: VoxelCoordinate) -> Result<Self, Self::Error> {
        let x = i32::try_from(coordinate.x).map_err(|_| ProjectionError::CoordinateOutOfRange {
            coordinate: [coordinate.x, coordinate.y, coordinate.z],
        })?;
        let y = i32::try_from(coordinate.y).map_err(|_| ProjectionError::CoordinateOutOfRange {
            coordinate: [coordinate.x, coordinate.y, coordinate.z],
        })?;
        let z = i32::try_from(coordinate.z).map_err(|_| ProjectionError::CoordinateOutOfRange {
            coordinate: [coordinate.x, coordinate.y, coordinate.z],
        })?;
        Ok(Self::new(x, y, z))
    }
}

/// Voxel value understood by the presentation seam.
///
/// `M` is deliberately a disposable renderer mapping. It is never a stable
/// content identity and must not be written to world storage.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum PresentationVoxel<M> {
    /// Known empty authoritative cell.
    Air,
    /// Solid cell with a presentation-only material index.
    Solid(M),
}

/// One presentation-only voxel write.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProjectionWrite<M> {
    coordinate: PresentationCoordinate,
    voxel: PresentationVoxel<M>,
}

impl<M> ProjectionWrite<M> {
    /// Creates one write without applying it to an upstream world.
    #[must_use]
    pub const fn new(coordinate: PresentationCoordinate, voxel: PresentationVoxel<M>) -> Self {
        Self { coordinate, voxel }
    }

    /// World-space coordinate of the write.
    #[must_use]
    pub const fn coordinate(&self) -> PresentationCoordinate {
        self.coordinate
    }

    /// Presentation-only voxel value.
    #[must_use]
    pub const fn voxel(&self) -> &PresentationVoxel<M> {
        &self.voxel
    }
}

/// Bounded writes for exactly one authoritative chunk revision.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProjectionBatch<M> {
    chunk: ChunkCoordinate,
    revision: ChunkRevision,
    writes: Vec<ProjectionWrite<M>>,
}

impl<M> ProjectionBatch<M> {
    /// Validates a bounded, single-chunk projection batch.
    ///
    /// # Errors
    ///
    /// Returns [`ProjectionError::InvalidBatchSize`] for an empty or oversized
    /// batch and [`ProjectionError::ChunkMismatch`] when a write crosses the
    /// declared authoritative chunk.
    pub fn new(
        chunk: ChunkCoordinate,
        revision: ChunkRevision,
        edge: VoxelChunkEdge,
        mut writes: Vec<ProjectionWrite<M>>,
    ) -> ProjectionResult<Self> {
        if writes.is_empty() || writes.len() > MAX_PROJECTION_BATCH_WRITES {
            return Err(ProjectionError::InvalidBatchSize {
                maximum: MAX_PROJECTION_BATCH_WRITES,
                actual: writes.len(),
            });
        }
        for write in &writes {
            let actual = write.coordinate.chunk(edge);
            if actual != chunk {
                return Err(ProjectionError::ChunkMismatch {
                    expected: chunk,
                    actual,
                    coordinate: write.coordinate,
                });
            }
        }
        writes.sort_unstable_by_key(|write| write.coordinate);
        if let Some(pair) = writes
            .windows(2)
            .find(|pair| pair[0].coordinate == pair[1].coordinate)
        {
            return Err(ProjectionError::DuplicateCoordinate {
                coordinate: pair[0].coordinate,
            });
        }
        Ok(Self {
            chunk,
            revision,
            writes,
        })
    }

    /// Translates one storage-evidenced committed chunk into presentation work.
    ///
    /// Runtime projections use canonical x-fastest, then Z, then Y cell order.
    /// The mapper receives each decoded authoritative cell and returns a
    /// disposable renderer value. Neither that value nor the resulting batch
    /// may be used as persistence state.
    ///
    /// # Errors
    ///
    /// Returns [`ProjectionError::CoordinateOutOfRange`] when a committed
    /// coordinate cannot be represented by upstream, or
    /// [`ProjectionError::InvalidBatchSize`] when the authoritative chunk is
    /// larger than this bounded D2 presentation seam.
    pub fn from_committed_projection<V>(
        projection: &CommittedChunkProjection<V>,
        mut map: impl FnMut(&V) -> PresentationVoxel<M>,
    ) -> ProjectionResult<Self> {
        let write_count = projection.cells().len();
        if write_count == 0 || write_count > MAX_PROJECTION_BATCH_WRITES {
            return Err(ProjectionError::InvalidBatchSize {
                maximum: MAX_PROJECTION_BATCH_WRITES,
                actual: write_count,
            });
        }
        let edge = VoxelChunkEdge::new(projection.edge())?;
        let chunk = projection.key().coordinate;
        let edge_i64 = i64::from(edge.get());
        let anchor = [
            i64::from(chunk.x) * edge_i64,
            i64::from(chunk.y) * edge_i64,
            i64::from(chunk.z) * edge_i64,
        ];
        let mut writes = Vec::with_capacity(write_count);
        let mut cells = projection.cells().iter();

        for y in 0..edge.get() {
            for z in 0..edge.get() {
                for x in 0..edge.get() {
                    let value = cells.next().ok_or(ProjectionError::InvalidBatchSize {
                        maximum: MAX_PROJECTION_BATCH_WRITES,
                        actual: projection.cells().len(),
                    })?;
                    let coordinate = PresentationCoordinate::try_from(VoxelCoordinate::new(
                        anchor[0] + i64::from(x),
                        anchor[1] + i64::from(y),
                        anchor[2] + i64::from(z),
                    ))?;
                    writes.push(ProjectionWrite::new(coordinate, map(value)));
                }
            }
        }

        Self::new(chunk, projection.revision(), edge, writes)
    }
    /// Translates a successful authoritative player break/place result.
    ///
    /// The material mapper is consulted only for stable block identity in a
    /// successful place result. A break becomes [`PresentationVoxel::Air`].
    ///
    /// # Errors
    ///
    /// Returns [`ProjectionError::ActionResultMismatch`] when the success DTO
    /// contradicts the action, or [`ProjectionError::MaterialUnavailable`]
    /// when stable placed content has no presentation mapping.
    pub fn from_block_edit(
        action: BlockEditActionV1,
        success: &BlockEditSuccessV1,
        material_for: impl FnOnce(&BlockId) -> Option<M>,
    ) -> ProjectionResult<Self> {
        let voxel = match (action, &success.old_content, &success.new_content) {
            (BlockEditActionV1::Break, Some(_), None) => PresentationVoxel::Air,
            (BlockEditActionV1::Place, _, Some(block)) => {
                let material =
                    material_for(block).ok_or_else(|| ProjectionError::MaterialUnavailable {
                        block: block.as_str().to_owned(),
                    })?;
                PresentationVoxel::Solid(material)
            }
            _ => return Err(ProjectionError::ActionResultMismatch { action }),
        };
        let coordinate = PresentationCoordinate::from(success.position);
        let edge = VoxelChunkEdge::D2;
        Self::new(
            coordinate.chunk(edge),
            success.committed_chunk_revision,
            edge,
            vec![ProjectionWrite::new(coordinate, voxel)],
        )
    }

    /// Authoritative source chunk.
    #[must_use]
    pub const fn chunk(&self) -> ChunkCoordinate {
        self.chunk
    }

    /// Authoritative source revision.
    #[must_use]
    pub const fn revision(&self) -> ChunkRevision {
        self.revision
    }

    /// Stable-order writes in this batch.
    #[must_use]
    pub fn writes(&self) -> &[ProjectionWrite<M>] {
        &self.writes
    }
}
