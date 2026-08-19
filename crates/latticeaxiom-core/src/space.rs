//! Canonical spatial conventions (ADR 0011).
//!
//! World space is a right-handed Cartesian system with `+Z` up: `+X` is east,
//! `+Y` is north, and the basis satisfies `+X x +Y = +Z`. Coordinates and
//! indices are always ordered `(x, y, z)`; height is `z`. Lengths are meters,
//! angles are radians. Positive rotation follows the right-hand rule.
//!
//! The integer voxel cell `(i, j, k)` covers the half-open box
//! `[i, i+1) x [j, j+1) x [k, k+1)`; conversion from continuous positions
//! floors toward negative infinity, and chunk/local splits use Euclidean
//! division so negative blocks land in the trailing local cell of the
//! previous chunk.

use glam::Vec3;

/// World up direction (`+Z`).
pub const UP: Vec3 = Vec3::Z;
/// World down direction (`-Z`).
pub const DOWN: Vec3 = Vec3::NEG_Z;
/// World east direction (`+X`).
pub const EAST: Vec3 = Vec3::X;
/// World west direction (`-X`).
pub const WEST: Vec3 = Vec3::NEG_X;
/// World north direction (`+Y`).
pub const NORTH: Vec3 = Vec3::Y;
/// World south direction (`-Y`).
pub const SOUTH: Vec3 = Vec3::NEG_Y;

/// Integer position of a voxel cell.
///
/// The cell covers the half-open box `[x, x+1) x [y, y+1) x [z, z+1)` in
/// world space, so its center sits at `(x + 0.5, y + 0.5, z + 0.5)`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct BlockPos {
    /// East-west coordinate (east is positive).
    pub x: i32,
    /// North-south coordinate (north is positive).
    pub y: i32,
    /// Vertical coordinate (up is positive); height is always `z`.
    pub z: i32,
}

impl BlockPos {
    /// Creates a block position from its components.
    #[must_use]
    pub const fn new(x: i32, y: i32, z: i32) -> Self {
        Self { x, y, z }
    }

    /// Returns the block containing a continuous world position.
    ///
    /// Each component floors toward negative infinity (never truncation
    /// toward zero), so `-0.25` lands in block `-1`, and integer boundaries
    /// belong to the cell they start (`1.0` lands in block `1`).
    #[must_use]
    #[expect(
        clippy::cast_possible_truncation,
        reason = "floor() yields a mathematical integer; world positions stay far below i32 range"
    )]
    pub fn containing(position: Vec3) -> Self {
        Self::new(
            position.x.floor() as i32,
            position.y.floor() as i32,
            position.z.floor() as i32,
        )
    }

    /// Returns the world-space center of this cell.
    #[must_use]
    #[expect(
        clippy::cast_precision_loss,
        reason = "block coordinates in play are far below f32 integer precision limits"
    )]
    pub fn center(self) -> Vec3 {
        Vec3::new(
            self.x as f32 + 0.5,
            self.y as f32 + 0.5,
            self.z as f32 + 0.5,
        )
    }

    /// Adds an integer offset, returning `None` on coordinate overflow.
    #[must_use]
    pub const fn checked_offset(self, x: i32, y: i32, z: i32) -> Option<Self> {
        let Some(x) = self.x.checked_add(x) else {
            return None;
        };
        let Some(y) = self.y.checked_add(y) else {
            return None;
        };
        let Some(z) = self.z.checked_add(z) else {
            return None;
        };
        Some(Self::new(x, y, z))
    }
}

/// Splits one block-axis coordinate into `(chunk, local)` for a chunk edge
/// length, using Euclidean division as required by ADR 0011.
///
/// The result satisfies `block == chunk * edge + local` and
/// `0 <= local < edge` on every axis, so `block = -1` maps to the last local
/// cell of chunk `-1` rather than incorrectly landing in chunk `0`.
///
/// # Panics
///
/// Panics if `edge` is not strictly positive; chunk edge lengths are static
/// configuration, so a non-positive value is a programmer error.
#[must_use]
pub fn split_axis(block: i32, edge: i32) -> (i32, i32) {
    assert!(edge > 0, "chunk edge length must be positive, got {edge}");
    (block.div_euclid(edge), block.rem_euclid(edge))
}

#[cfg(test)]
mod tests {
    use std::f32::consts::FRAC_PI_2;

    use glam::Quat;
    use proptest::prelude::*;

    use super::*;

    #[test]
    fn basis_is_right_handed_z_up() {
        assert_eq!(EAST.cross(NORTH), UP);
    }

    #[test]
    fn positive_rotation_about_up_turns_east_to_north() {
        let rotated = Quat::from_rotation_z(FRAC_PI_2) * EAST;
        assert!((rotated - NORTH).length() < 1e-6);
    }

    #[test]
    fn containing_floors_toward_negative_infinity() {
        assert_eq!(
            BlockPos::containing(Vec3::new(0.5, 0.5, 0.5)),
            BlockPos::new(0, 0, 0)
        );
        assert_eq!(
            BlockPos::containing(Vec3::new(-0.25, -1.0, -1.75)),
            BlockPos::new(-1, -1, -2)
        );
        // Integer boundaries belong to the cell they start.
        assert_eq!(
            BlockPos::containing(Vec3::new(1.0, -2.0, 0.0)),
            BlockPos::new(1, -2, 0)
        );
    }

    #[test]
    fn split_axis_boundary_values() {
        // The boundary cases required by ADR 0011 for an edge length `s`.
        let s = 32;
        assert_eq!(split_axis(-s - 1, s), (-2, s - 1));
        assert_eq!(split_axis(-s, s), (-1, 0));
        assert_eq!(split_axis(-1, s), (-1, s - 1));
        assert_eq!(split_axis(0, s), (0, 0));
        assert_eq!(split_axis(s - 1, s), (0, s - 1));
        assert_eq!(split_axis(s, s), (1, 0));
        assert_eq!(split_axis(s + 1, s), (1, 1));
    }

    proptest! {
        #[test]
        fn split_axis_reconstructs_block(block in any::<i32>(), edge in 1i32..=64) {
            let (chunk, local) = split_axis(block, edge);
            prop_assert!((0..edge).contains(&local));
            // Reconstruct in i64: chunk * edge can exceed i32 range transiently.
            prop_assert_eq!(
                i64::from(chunk) * i64::from(edge) + i64::from(local),
                i64::from(block)
            );
        }

        #[test]
        #[allow(
            clippy::float_cmp,
            reason = "floor() results are mathematically exact; the property is exact equality"
        )]
        fn containing_matches_component_floor(
            x in -1.0e6f32..1.0e6,
            y in -1.0e6f32..1.0e6,
            z in -1.0e6f32..1.0e6,
        ) {
            let block = BlockPos::containing(Vec3::new(x, y, z));
            prop_assert_eq!(f64::from(block.x), f64::from(x.floor()));
            prop_assert_eq!(f64::from(block.y), f64::from(y.floor()));
            prop_assert_eq!(f64::from(block.z), f64::from(z.floor()));
        }
    }
}
