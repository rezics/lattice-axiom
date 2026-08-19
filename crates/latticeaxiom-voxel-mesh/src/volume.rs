//! Input contracts: voxel traits and padded volume dimensions.

/// A voxel sample that meshing can classify.
///
/// Milestone scope: a voxel either fully occludes its neighbors' faces
/// (opaque) or not at all (empty). Translucency is deliberately out of scope
/// until the demo has content that needs it.
pub trait Voxel {
    /// Whether this voxel fully occludes the faces of adjacent voxels.
    ///
    /// Opaque voxels emit faces wherever their neighbor is not opaque; empty
    /// voxels emit nothing.
    fn is_opaque(&self) -> bool;
}

/// A voxel that can merge with equal neighbors into larger quads.
pub trait MergeVoxel: Voxel {
    /// Identity used to decide whether two adjacent faces may merge.
    ///
    /// Typically a palette or material id. Only faces with equal merge
    /// values are combined by [`greedy_quads`](crate::greedy_quads).
    type MergeValue: Copy + Eq;

    /// Returns the merge identity of this voxel.
    ///
    /// Only called for voxels that are [`Voxel::is_opaque`].
    fn merge_value(&self) -> Self::MergeValue;
}

/// Maximum allowed padded edge length.
///
/// Bounding the edge keeps every coordinate exactly representable in the
/// `u32` and `f32` outputs; chunk-scale inputs are far below this.
pub const MAX_PADDED_EDGE: usize = 4096;

/// Dimensions of a padded voxel sample volume.
///
/// The volume is the meshable *interior* plus a one-voxel apron on every
/// side. The caller fills the apron with neighboring samples (or empty
/// voxels at world borders) so that face visibility at the interior boundary
/// is decided without branching or seam special-cases.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PaddedDims {
    size: [usize; 3],
}

impl PaddedDims {
    /// Creates padded dimensions from the full (apron-inclusive) size.
    ///
    /// # Panics
    ///
    /// Panics when an edge is shorter than 3 (no interior left) or longer
    /// than [`MAX_PADDED_EDGE`]; both are programmer errors in the caller's
    /// chunking configuration.
    #[must_use]
    pub fn new(size: [usize; 3]) -> Self {
        for edge in size {
            assert!(
                (3..=MAX_PADDED_EDGE).contains(&edge),
                "padded edge must be within 3..={MAX_PADDED_EDGE}, got {edge}"
            );
        }
        Self { size }
    }

    /// Full padded size, including the apron.
    #[must_use]
    pub fn padded_size(&self) -> [usize; 3] {
        self.size
    }

    /// Size of the meshable interior (padded size minus the apron).
    #[must_use]
    pub fn interior_size(&self) -> [usize; 3] {
        [self.size[0] - 2, self.size[1] - 2, self.size[2] - 2]
    }

    /// Number of samples the input slice must contain.
    #[must_use]
    pub fn volume_len(&self) -> usize {
        self.size[0] * self.size[1] * self.size[2]
    }

    /// Linear index of a padded-space position, x-fastest:
    /// `x + sx * (y + sy * z)`.
    #[must_use]
    pub fn linearize(&self, [x, y, z]: [usize; 3]) -> usize {
        debug_assert!(x < self.size[0] && y < self.size[1] && z < self.size[2]);
        x + self.size[0] * (y + self.size[1] * z)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn linearize_is_x_fastest() {
        let dims = PaddedDims::new([3, 4, 5]);
        assert_eq!(dims.linearize([0, 0, 0]), 0);
        assert_eq!(dims.linearize([1, 0, 0]), 1);
        assert_eq!(dims.linearize([0, 1, 0]), 3);
        assert_eq!(dims.linearize([0, 0, 1]), 12);
        assert_eq!(dims.linearize([2, 3, 4]), 2 + 3 * (3 + 4 * 4));
        assert_eq!(dims.volume_len(), 60);
        assert_eq!(dims.interior_size(), [1, 2, 3]);
    }

    #[test]
    #[should_panic(expected = "padded edge must be within")]
    fn rejects_volumes_without_interior() {
        let _ = PaddedDims::new([2, 3, 3]);
    }
}
