//! Deterministic voxel-grid ray traversal for block selection.

use glam::Vec3;

use crate::BlockPos;

/// Axis-aligned outward normal of a voxel face.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FaceNormal {
    /// Face looking west (`-X`).
    NegX,
    /// Face looking east (`+X`).
    PosX,
    /// Face looking south (`-Y`).
    NegY,
    /// Face looking north (`+Y`).
    PosY,
    /// Face looking down (`-Z`).
    NegZ,
    /// Face looking up (`+Z`).
    PosZ,
}

impl FaceNormal {
    /// Integer `(x, y, z)` offset in the normal direction.
    #[must_use]
    pub const fn offset(self) -> [i32; 3] {
        match self {
            Self::NegX => [-1, 0, 0],
            Self::PosX => [1, 0, 0],
            Self::NegY => [0, -1, 0],
            Self::PosY => [0, 1, 0],
            Self::NegZ => [0, 0, -1],
            Self::PosZ => [0, 0, 1],
        }
    }

    /// Normal pointing in the opposite direction.
    #[must_use]
    pub const fn opposite(self) -> Self {
        match self {
            Self::NegX => Self::PosX,
            Self::PosX => Self::NegX,
            Self::NegY => Self::PosY,
            Self::PosY => Self::NegY,
            Self::NegZ => Self::PosZ,
            Self::PosZ => Self::NegZ,
        }
    }
}

/// First solid voxel reached by a grid ray.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RaycastHit {
    /// Solid world cell that the ray selected.
    pub block: BlockPos,
    /// Empty-side adjacent cell suitable for a placement attempt.
    pub placement: BlockPos,
    /// Outward face normal from `block` toward `placement`.
    pub normal: FaceNormal,
    /// Distance from the ray origin in world meters.
    pub distance: f32,
}

/// Traverses world voxels with the Amanatides-Woo DDA algorithm.
///
/// `direction` need not be normalized. Invalid/non-finite input, a negative or
/// non-finite distance, coordinate overflow, and a miss all return `None`.
/// Exact edge/corner ties use stable `X`, then `Y`, then `Z` priority. If the
/// origin is already solid, it is returned at distance zero with the face
/// opposite the dominant ray component.
#[must_use]
pub fn raycast_voxels(
    origin: Vec3,
    direction: Vec3,
    max_distance: f32,
    mut is_solid: impl FnMut(BlockPos) -> bool,
) -> Option<RaycastHit> {
    if !origin.is_finite()
        || !direction.is_finite()
        || !max_distance.is_finite()
        || max_distance < 0.0
        || direction.length_squared() <= f32::EPSILON
    {
        return None;
    }

    let direction = direction.normalize();
    let mut block = BlockPos::containing(origin);
    if is_solid(block) {
        let normal = entry_normal(direction);
        let [x, y, z] = normal.offset();
        let placement = block.checked_offset(x, y, z)?;
        return Some(RaycastHit {
            block,
            placement,
            normal,
            distance: 0.0,
        });
    }

    let (step_x, mut next_x, delta_x) = axis_traversal(origin.x, block.x, direction.x);
    let (step_y, mut next_y, delta_y) = axis_traversal(origin.y, block.y, direction.y);
    let (step_z, mut next_z, delta_z) = axis_traversal(origin.z, block.z, direction.z);

    loop {
        let (distance, normal) = if next_x <= next_y && next_x <= next_z {
            block.x = block.x.checked_add(step_x)?;
            let distance = next_x;
            next_x += delta_x;
            let normal = if step_x > 0 {
                FaceNormal::NegX
            } else {
                FaceNormal::PosX
            };
            (distance, normal)
        } else if next_y <= next_z {
            block.y = block.y.checked_add(step_y)?;
            let distance = next_y;
            next_y += delta_y;
            let normal = if step_y > 0 {
                FaceNormal::NegY
            } else {
                FaceNormal::PosY
            };
            (distance, normal)
        } else {
            block.z = block.z.checked_add(step_z)?;
            let distance = next_z;
            next_z += delta_z;
            let normal = if step_z > 0 {
                FaceNormal::NegZ
            } else {
                FaceNormal::PosZ
            };
            (distance, normal)
        };

        if distance > max_distance {
            return None;
        }
        if is_solid(block) {
            let [x, y, z] = normal.offset();
            let placement = block.checked_offset(x, y, z)?;
            return Some(RaycastHit {
                block,
                placement,
                normal,
                distance,
            });
        }
    }
}

#[allow(
    clippy::cast_precision_loss,
    reason = "ray origins use f32 and cannot distinguish larger integer voxel boundaries"
)]
fn axis_traversal(origin: f32, block: i32, direction: f32) -> (i32, f32, f32) {
    if direction > 0.0 {
        (
            1,
            (block as f32 + 1.0 - origin) / direction,
            direction.recip(),
        )
    } else if direction < 0.0 {
        (-1, (block as f32 - origin) / direction, -direction.recip())
    } else {
        (0, f32::INFINITY, f32::INFINITY)
    }
}

fn entry_normal(direction: Vec3) -> FaceNormal {
    let absolute = direction.abs();
    if absolute.x >= absolute.y && absolute.x >= absolute.z {
        if direction.x >= 0.0 {
            FaceNormal::NegX
        } else {
            FaceNormal::PosX
        }
    } else if absolute.y >= absolute.z {
        if direction.y >= 0.0 {
            FaceNormal::NegY
        } else {
            FaceNormal::PosY
        }
    } else if direction.z >= 0.0 {
        FaceNormal::NegZ
    } else {
        FaceNormal::PosZ
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn positive_axis_hit_includes_placement_cell_and_normal() {
        let hit = raycast_voxels(Vec3::new(0.5, 0.5, 0.5), Vec3::X, 10.0, |block| {
            block == BlockPos::new(3, 0, 0)
        })
        .expect("ray reaches the configured solid cell");
        assert_eq!(hit.block, BlockPos::new(3, 0, 0));
        assert_eq!(hit.placement, BlockPos::new(2, 0, 0));
        assert_eq!(hit.normal, FaceNormal::NegX);
        assert!((hit.distance - 2.5).abs() < 1.0e-6);
    }

    #[test]
    fn negative_coordinates_use_flooring_and_positive_entry_normal() {
        let hit = raycast_voxels(Vec3::new(0.25, -0.5, 1.5), Vec3::NEG_X, 5.0, |block| {
            block == BlockPos::new(-2, -1, 1)
        })
        .expect("negative-axis ray reaches the configured solid cell");
        assert_eq!(hit.block, BlockPos::new(-2, -1, 1));
        assert_eq!(hit.placement, BlockPos::new(-1, -1, 1));
        assert_eq!(hit.normal, FaceNormal::PosX);
        assert!((hit.distance - 1.25).abs() < 1.0e-6);
    }

    #[test]
    fn miss_stops_at_maximum_distance() {
        let hit = raycast_voxels(Vec3::splat(0.5), Vec3::Z, 2.4, |block| {
            block == BlockPos::new(0, 0, 3)
        });
        assert_eq!(hit, None);
    }

    #[test]
    fn origin_inside_solid_hits_at_zero() {
        let hit = raycast_voxels(Vec3::splat(0.5), Vec3::NEG_Z, 0.0, |_| true)
            .expect("the origin cell is solid");
        assert_eq!(hit.block, BlockPos::new(0, 0, 0));
        assert_eq!(hit.placement, BlockPos::new(0, 0, 1));
        assert_eq!(hit.normal, FaceNormal::PosZ);
        assert!(hit.distance.abs() < f32::EPSILON);
    }

    #[test]
    fn invalid_inputs_do_not_invoke_world_query() {
        let mut calls = 0;
        let hit = raycast_voxels(Vec3::ZERO, Vec3::ZERO, 5.0, |_| {
            calls += 1;
            true
        });
        assert_eq!(hit, None);
        assert_eq!(calls, 0);
    }
}
