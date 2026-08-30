//! Immutable one-voxel-halo input contract.

use thiserror::Error;

use crate::{Face, MeshGroup};

/// Halo thickness required on each of the six sides of a chunk.
pub const ONE_VOXEL_HALO: usize = 1;

/// Maximum meshable interior edge length.
///
/// Including the halo, coordinates remain at most 4095 and therefore exactly
/// representable in both the `u32` quad coordinates and `f32` positions.
pub const MAX_INTERIOR_EDGE: usize = 4094;

/// Invalid [`PaddedChunk`] dimensions.
#[derive(Clone, Copy, Debug, Error, PartialEq, Eq)]
pub enum PaddedChunkError {
    /// One interior edge was zero.
    #[error("interior edge {axis} must be nonzero")]
    EmptyEdge {
        /// Axis index in `(x, y, z)` order.
        axis: usize,
    },
    /// One interior edge exceeded [`MAX_INTERIOR_EDGE`].
    #[error("interior edge {axis} is {value}, exceeding maximum {maximum}")]
    EdgeTooLarge {
        /// Axis index in `(x, y, z)` order.
        axis: usize,
        /// Rejected edge length.
        value: usize,
        /// Maximum accepted edge length.
        maximum: usize,
    },
    /// The padded sample count overflowed `usize` on this target.
    #[error("padded chunk sample count overflows usize")]
    VolumeOverflow,
}

/// Dimensions and x-fastest indexing for a chunk plus one sample on every
/// side.
///
/// Given interior size `[sx, sy, sz]`, padded size is
/// `[sx + 2, sy + 2, sz + 2]`. Padded sample coordinates are linearized as
/// `x + padded_x * (y + padded_y * z)`. Interior coordinate `[0, 0, 0]`
/// therefore maps to padded coordinate `[1, 1, 1]`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct PaddedChunk {
    interior: [usize; 3],
    padded: [usize; 3],
    volume_len: usize,
}

impl PaddedChunk {
    /// Creates dimensions for a nonempty chunk and its one-voxel halo.
    ///
    /// # Errors
    ///
    /// Returns [`PaddedChunkError`] when an interior edge is zero, exceeds
    /// [`MAX_INTERIOR_EDGE`], or the padded sample count cannot be represented
    /// by `usize`.
    pub fn new(interior: [usize; 3]) -> Result<Self, PaddedChunkError> {
        for (axis, edge) in interior.into_iter().enumerate() {
            if edge == 0 {
                return Err(PaddedChunkError::EmptyEdge { axis });
            }
            if edge > MAX_INTERIOR_EDGE {
                return Err(PaddedChunkError::EdgeTooLarge {
                    axis,
                    value: edge,
                    maximum: MAX_INTERIOR_EDGE,
                });
            }
        }

        let padded = [interior[0] + 2, interior[1] + 2, interior[2] + 2];
        let volume_len = padded[0]
            .checked_mul(padded[1])
            .and_then(|plane| plane.checked_mul(padded[2]))
            .ok_or(PaddedChunkError::VolumeOverflow)?;
        Ok(Self {
            interior,
            padded,
            volume_len,
        })
    }

    /// Meshable interior size in `(x, y, z)` order.
    #[must_use]
    pub const fn interior_size(self) -> [usize; 3] {
        self.interior
    }

    /// Full sample size including one voxel on every side.
    #[must_use]
    pub const fn padded_size(self) -> [usize; 3] {
        self.padded
    }

    /// Exact number of samples required by a meshing call.
    #[must_use]
    pub const fn volume_len(self) -> usize {
        self.volume_len
    }

    /// Converts an interior coordinate to its padded coordinate.
    ///
    /// Returns `None` when the coordinate is outside the interior.
    #[must_use]
    pub fn pad_interior(self, position: [usize; 3]) -> Option<[usize; 3]> {
        (position[0] < self.interior[0]
            && position[1] < self.interior[1]
            && position[2] < self.interior[2])
            .then_some([position[0] + 1, position[1] + 1, position[2] + 1])
    }

    /// Linearizes a padded-space coordinate in x-fastest order.
    ///
    /// Returns `None` when the coordinate is outside the padded volume.
    #[must_use]
    pub fn linearize(self, [x, y, z]: [usize; 3]) -> Option<usize> {
        (x < self.padded[0] && y < self.padded[1] && z < self.padded[2])
            .then(|| self.linearize_unchecked([x, y, z]))
    }

    pub(crate) fn linearize_unchecked(self, [x, y, z]: [usize; 3]) -> usize {
        debug_assert!(x < self.padded[0] && y < self.padded[1] && z < self.padded[2]);
        x + self.padded[0] * (y + self.padded[1] * z)
    }
}

/// How a face hides an adjacent face during face culling.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FaceOcclusion {
    /// Does not hide an adjacent face.
    None,
    /// Hides only a face with the same mesh group and merge key.
    ///
    /// This is useful for cutout and translucent blocks: internal faces of
    /// one material disappear, while interfaces with another material remain.
    Matching,
    /// Hides every adjacent face.
    Full,
}

/// Stable caller-owned identity for one fluid kind in a mesh source.
///
/// The value commonly identifies a row in a locked fluid or presentation
/// table. It is deliberately not a string and is interpreted only by the
/// caller that owns the matching [`crate::MeshSource`] fingerprint.
#[repr(transparent)]
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct FluidMeshIdentity(u16);

impl FluidMeshIdentity {
    /// Creates a fluid identity from a caller-owned stable table index.
    #[must_use]
    pub const fn new(value: u16) -> Self {
        Self(value)
    }

    /// Returns the caller-owned stable table index.
    #[must_use]
    pub const fn get(self) -> u16 {
        self.0
    }
}

/// Invalid v1 fluid level supplied to meshing.
#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
#[error("fluid mesh level {actual} is outside the supported range 0..=7")]
pub struct FluidMeshLevelError {
    actual: u8,
}

/// Validated v1 fluid level preserved by the mesh source.
///
/// Level zero is full and level seven is one eighth of a voxel high. The
/// mesher never reinterprets this value as fluid volume.
#[repr(transparent)]
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct FluidMeshLevel(u8);

impl FluidMeshLevel {
    /// Full/source fluid level.
    pub const SOURCE: Self = Self(0);
    /// Greatest accepted v1 level.
    pub const MAX: u8 = 7;

    /// Creates a validated fluid level.
    ///
    /// # Errors
    ///
    /// Returns [`FluidMeshLevelError`] when `value` exceeds seven.
    pub const fn new(value: u8) -> Result<Self, FluidMeshLevelError> {
        if value <= Self::MAX {
            Ok(Self(value))
        } else {
            Err(FluidMeshLevelError { actual: value })
        }
    }

    /// Returns the preserved v1 level.
    #[must_use]
    pub const fn get(self) -> u8 {
        self.0
    }

    /// Returns the presentation height in exact eighths of one voxel.
    #[must_use]
    pub const fn height_eighths(self) -> u8 {
        8 - self.0
    }
}

/// Explicit flow direction preserved on fluid geometry.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum FluidMeshFlow {
    /// No directional flow.
    #[default]
    Still,
    /// Negative Y waterfall flow.
    Down,
    /// Positive X flow.
    East,
    /// Negative X flow.
    West,
    /// Positive Z flow.
    South,
    /// Negative Z flow.
    North,
}

/// Fluid identity and state exposed by one voxel face.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FluidSurfaceDescriptor<K> {
    identity: FluidMeshIdentity,
    level: FluidMeshLevel,
    flow: FluidMeshFlow,
    merge_key: K,
}

impl<K> FluidSurfaceDescriptor<K> {
    /// Creates a fluid surface descriptor.
    #[must_use]
    pub const fn new(
        identity: FluidMeshIdentity,
        level: FluidMeshLevel,
        flow: FluidMeshFlow,
        merge_key: K,
    ) -> Self {
        Self {
            identity,
            level,
            flow,
            merge_key,
        }
    }

    /// Stable identity of the fluid kind.
    #[must_use]
    pub const fn identity(&self) -> FluidMeshIdentity {
        self.identity
    }

    /// Preserved v1 fluid level.
    #[must_use]
    pub const fn level(&self) -> FluidMeshLevel {
        self.level
    }

    /// Explicit flow direction.
    #[must_use]
    pub const fn flow(&self) -> FluidMeshFlow {
        self.flow
    }

    /// Caller-defined face presentation identity.
    #[must_use]
    pub const fn merge_key(&self) -> &K {
        &self.merge_key
    }
}

/// Caller-defined presentation description of one voxel face.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FaceDescriptor<K> {
    group: MeshGroup,
    merge_key: K,
    occlusion: FaceOcclusion,
}

impl<K> FaceDescriptor<K> {
    /// Creates a face descriptor.
    #[must_use]
    pub const fn new(group: MeshGroup, merge_key: K, occlusion: FaceOcclusion) -> Self {
        Self {
            group,
            merge_key,
            occlusion,
        }
    }

    /// Stable output group for the face.
    #[must_use]
    pub const fn group(&self) -> MeshGroup {
        self.group
    }

    /// Caller-defined identity used for greedy merging.
    #[must_use]
    pub const fn merge_key(&self) -> &K {
        &self.merge_key
    }

    /// Occlusion behavior presented to the neighboring face.
    #[must_use]
    pub const fn occlusion(&self) -> FaceOcclusion {
        self.occlusion
    }
}

impl<K: Eq> FaceDescriptor<K> {
    pub(crate) fn occludes(&self, adjacent: &Self) -> bool {
        match self.occlusion {
            FaceOcclusion::None => false,
            FaceOcclusion::Matching => {
                self.group == adjacent.group && self.merge_key == adjacent.merge_key
            }
            FaceOcclusion::Full => true,
        }
    }
}

/// A voxel sample that exposes renderer-independent face semantics.
///
/// Returning `None` means this voxel emits no face in that direction. The
/// opposite face returned by a neighbor determines whether a candidate face
/// is culled. Implementations must be deterministic and should use a compact,
/// copyable merge key containing every value that changes emitted vertices.
pub trait Voxel {
    /// Identity attached to output quads and used for greedy merging.
    type MergeKey: Copy + Eq;

    /// Presentation and occlusion semantics for one face direction.
    fn face(&self, face: Face) -> Option<FaceDescriptor<Self::MergeKey>>;

    /// Fluid identity, state, and presentation for one face direction.
    ///
    /// Returning a descriptor routes that voxel through the dedicated fluid
    /// geometry path and suppresses its ordinary cube face for `face`.
    fn fluid_surface(&self, _face: Face) -> Option<FluidSurfaceDescriptor<Self::MergeKey>> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dimensions_add_exactly_one_halo_on_every_side() {
        let dimensions = PaddedChunk::new([3, 4, 5]).expect("valid test dimensions");
        assert_eq!(dimensions.interior_size(), [3, 4, 5]);
        assert_eq!(dimensions.padded_size(), [5, 6, 7]);
        assert_eq!(dimensions.volume_len(), 5 * 6 * 7);
        assert_eq!(dimensions.pad_interior([0, 0, 0]), Some([1, 1, 1]));
        assert_eq!(dimensions.pad_interior([2, 3, 4]), Some([3, 4, 5]));
    }

    #[test]
    fn indexing_is_x_fastest() {
        let dimensions = PaddedChunk::new([1, 2, 3]).expect("valid test dimensions");
        assert_eq!(dimensions.linearize([0, 0, 0]), Some(0));
        assert_eq!(dimensions.linearize([1, 0, 0]), Some(1));
        assert_eq!(dimensions.linearize([0, 1, 0]), Some(3));
        assert_eq!(dimensions.linearize([0, 0, 1]), Some(3 * 4));
        assert_eq!(dimensions.linearize([3, 0, 0]), None);
    }

    #[test]
    fn invalid_edges_are_rejected_without_panicking() {
        assert_eq!(
            PaddedChunk::new([0, 1, 1]),
            Err(PaddedChunkError::EmptyEdge { axis: 0 })
        );
        assert_eq!(
            PaddedChunk::new([1, MAX_INTERIOR_EDGE + 1, 1]),
            Err(PaddedChunkError::EdgeTooLarge {
                axis: 1,
                value: MAX_INTERIOR_EDGE + 1,
                maximum: MAX_INTERIOR_EDGE,
            })
        );
    }

    #[test]
    fn fluid_levels_validate_and_map_to_exact_eighths() {
        assert_eq!(FluidMeshLevel::SOURCE.height_eighths(), 8);
        assert_eq!(
            FluidMeshLevel::new(7)
                .expect("seven is the accepted maximum")
                .height_eighths(),
            1
        );
        assert_eq!(
            FluidMeshLevel::new(8),
            Err(FluidMeshLevelError { actual: 8 })
        );
    }
}
