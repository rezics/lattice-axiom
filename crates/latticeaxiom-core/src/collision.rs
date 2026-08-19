//! Axis-aligned world bounds shared by player physics and block edits.

use glam::Vec3;
use thiserror::Error;

use crate::BlockPos;

/// Invalid axis-aligned bounding box input.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Error)]
pub enum AabbError {
    /// At least one bound is NaN or infinite.
    #[error("AABB bounds must be finite")]
    NonFinite,
    /// At least one maximum component is not greater than its minimum.
    #[error("AABB must have positive extent on every axis")]
    NonPositiveExtent,
}

/// Half-open axis-aligned bounding box `[min, max)` in world meters.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Aabb {
    min: Vec3,
    max: Vec3,
}

impl Aabb {
    /// Validates and creates an axis-aligned bounding box.
    ///
    /// # Errors
    ///
    /// Returns [`AabbError`] for non-finite bounds or non-positive extent.
    pub fn new(min: Vec3, max: Vec3) -> Result<Self, AabbError> {
        if !min.is_finite() || !max.is_finite() {
            return Err(AabbError::NonFinite);
        }
        if min.x >= max.x || min.y >= max.y || min.z >= max.z {
            return Err(AabbError::NonPositiveExtent);
        }
        Ok(Self { min, max })
    }

    /// Creates bounds centered at `center` with positive `half_extents`.
    ///
    /// # Errors
    ///
    /// Returns [`AabbError`] when the resulting bounds are non-finite or any
    /// half extent is not strictly positive.
    pub fn from_center_half_extents(center: Vec3, half_extents: Vec3) -> Result<Self, AabbError> {
        if !half_extents.is_finite()
            || half_extents.x <= 0.0
            || half_extents.y <= 0.0
            || half_extents.z <= 0.0
        {
            return Err(if half_extents.is_finite() {
                AabbError::NonPositiveExtent
            } else {
                AabbError::NonFinite
            });
        }
        Self::new(center - half_extents, center + half_extents)
    }

    /// Inclusive lower world bound.
    #[must_use]
    pub const fn min(self) -> Vec3 {
        self.min
    }

    /// Exclusive upper world bound.
    #[must_use]
    pub const fn max(self) -> Vec3 {
        self.max
    }

    /// Center of the bounds.
    #[must_use]
    pub fn center(self) -> Vec3 {
        (self.min + self.max) * 0.5
    }

    /// Half the box extent on every axis.
    #[must_use]
    pub fn half_extents(self) -> Vec3 {
        (self.max - self.min) * 0.5
    }

    /// Returns whether this box overlaps `other` with positive volume.
    ///
    /// Merely touching faces or edges is not an overlap, matching half-open
    /// voxel cells and allowing a player to stand exactly on a block.
    #[must_use]
    pub fn intersects(self, other: Self) -> bool {
        self.min.x < other.max.x
            && self.max.x > other.min.x
            && self.min.y < other.max.y
            && self.max.y > other.min.y
            && self.min.z < other.max.z
            && self.max.z > other.min.z
    }

    /// Returns whether this box overlaps a unit voxel cell with positive volume.
    #[must_use]
    pub fn intersects_block(self, block: BlockPos) -> bool {
        let min_x = f64::from(block.x);
        let min_y = f64::from(block.y);
        let min_z = f64::from(block.z);
        f64::from(self.min.x) < min_x + 1.0
            && f64::from(self.max.x) > min_x
            && f64::from(self.min.y) < min_y + 1.0
            && f64::from(self.max.y) > min_y
            && f64::from(self.min.z) < min_z + 1.0
            && f64::from(self.max.z) > min_z
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bounds(min: Vec3, max: Vec3) -> Aabb {
        Aabb::new(min, max).expect("test bounds have finite positive extent")
    }

    #[test]
    fn touching_faces_do_not_intersect() {
        let west = bounds(Vec3::ZERO, Vec3::ONE);
        let east = bounds(Vec3::X, Vec3::new(2.0, 1.0, 1.0));
        assert!(!west.intersects(east));
    }

    #[test]
    fn positive_volume_overlap_intersects() {
        let first = bounds(Vec3::ZERO, Vec3::ONE);
        let second = bounds(Vec3::splat(0.5), Vec3::splat(1.5));
        assert!(first.intersects(second));
    }

    #[test]
    fn block_intersection_handles_negative_coordinates() {
        let player = bounds(Vec3::new(-0.5, -0.5, 1.0), Vec3::new(0.5, 0.5, 2.8));
        assert!(player.intersects_block(BlockPos::new(-1, -1, 1)));
        assert!(!player.intersects_block(BlockPos::new(-1, -1, 0)));
    }
}
