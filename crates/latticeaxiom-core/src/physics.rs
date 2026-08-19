//! Minimal deterministic player AABB movement against solid voxel cells.

use glam::{Vec2, Vec3};
use thiserror::Error;

use crate::{Aabb, AabbError, BlockPos};

const STANDARD_HALF_EXTENTS: Vec3 = Vec3::new(0.3, 0.3, 0.9);
const MAX_PLAYER_HALF_EXTENT: f32 = 4.0;
const PRECISE_VOXEL_LIMIT: f32 = 8_000_000.0;
const COLLISION_EPSILON: f32 = 1.0e-5;

/// Tunable constants for the first-person sandbox movement step.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PhysicsConfig {
    /// Downward acceleration in meters per second squared.
    pub gravity: f32,
    /// Maximum downward speed in meters per second.
    pub terminal_velocity: f32,
    /// Maximum requested horizontal speed in meters per second.
    pub max_horizontal_speed: f32,
    /// Largest accepted simulation step, limiting latency and scan work.
    pub max_delta_seconds: f32,
}

impl Default for PhysicsConfig {
    fn default() -> Self {
        Self {
            gravity: 24.0,
            terminal_velocity: 55.0,
            max_horizontal_speed: 7.0,
            max_delta_seconds: 0.1,
        }
    }
}

/// Invalid player state, configuration, or simulation input.
#[derive(Clone, Copy, Debug, PartialEq, Error)]
pub enum PhysicsError {
    /// Player bounds cannot be constructed.
    #[error(transparent)]
    InvalidBounds(#[from] AabbError),
    /// Player half extents exceed the bounded voxel scan budget.
    #[error("player half extents {half_extents:?} exceed {MAX_PLAYER_HALF_EXTENT} meters")]
    BodyTooLarge {
        /// Requested player half extents.
        half_extents: Vec3,
    },
    /// A position is outside the range where `f32` distinguishes unit voxels.
    #[error("player position {position:?} is outside the precise voxel simulation range")]
    PositionOutsidePrecisionRange {
        /// Rejected player center.
        position: Vec3,
    },
    /// Velocity or requested movement contains NaN or infinity.
    #[error("player movement vectors must be finite")]
    NonFiniteMovement,
    /// Delta time is non-positive, non-finite, or above the configured bound.
    #[error("delta time {delta_seconds} must be within (0, {maximum}]")]
    InvalidDelta {
        /// Rejected time step.
        delta_seconds: f32,
        /// Configured maximum time step.
        maximum: f32,
    },
    /// Physics constants are non-finite or not strictly positive.
    #[error("physics configuration values must be finite and strictly positive")]
    InvalidConfig,
    /// The incoming player body already overlaps a solid cell.
    #[error("player body starts inside solid block {block:?}")]
    BodyStartsInSolid {
        /// First overlapping solid cell in stable x/y/z scan order.
        block: BlockPos,
    },
}

/// Immutable player body state consumed and returned by [`step_player`].
///
/// `position` is the AABB center. The conventional standing body has a width
/// of `0.6 m`, depth of `0.6 m`, and height of `1.8 m`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlayerBody {
    position: Vec3,
    velocity: Vec3,
    half_extents: Vec3,
    grounded: bool,
}

impl PlayerBody {
    /// Creates a conventional `0.6 x 0.6 x 1.8 m` standing player body.
    ///
    /// # Errors
    ///
    /// Returns [`PhysicsError`] when `position` is non-finite or outside the
    /// precise voxel simulation range.
    pub fn standard(position: Vec3) -> Result<Self, PhysicsError> {
        Self::new(position, STANDARD_HALF_EXTENTS)
    }

    /// Creates a player body with custom positive half extents.
    ///
    /// # Errors
    ///
    /// Returns [`PhysicsError`] for invalid bounds, an unbounded scan size, or
    /// a position outside the precise voxel simulation range.
    pub fn new(position: Vec3, half_extents: Vec3) -> Result<Self, PhysicsError> {
        Aabb::from_center_half_extents(position, half_extents)?;
        if half_extents.max_element() > MAX_PLAYER_HALF_EXTENT {
            return Err(PhysicsError::BodyTooLarge { half_extents });
        }
        validate_precise_position(position, half_extents)?;
        Ok(Self {
            position,
            velocity: Vec3::ZERO,
            half_extents,
            grounded: false,
        })
    }

    /// Player AABB center in world meters.
    #[must_use]
    pub const fn position(self) -> Vec3 {
        self.position
    }

    /// Current velocity in meters per second.
    #[must_use]
    pub const fn velocity(self) -> Vec3 {
        self.velocity
    }

    /// Half of the collision-box size on each axis.
    #[must_use]
    pub const fn half_extents(self) -> Vec3 {
        self.half_extents
    }

    /// Whether the most recent step collided while moving downward.
    #[must_use]
    pub const fn is_grounded(self) -> bool {
        self.grounded
    }

    /// Current half-open collision bounds.
    #[must_use]
    pub fn bounds(self) -> Aabb {
        match Aabb::from_center_half_extents(self.position, self.half_extents) {
            Ok(bounds) => bounds,
            Err(_) => unreachable!("PlayerBody constructors and steps preserve valid bounds"),
        }
    }

    /// Replaces velocity while preserving position and body dimensions.
    ///
    /// This supports impulses such as jumping without exposing invalid body
    /// construction. Grounded state is cleared for positive vertical motion.
    ///
    /// # Errors
    ///
    /// Returns [`PhysicsError::NonFiniteMovement`] for NaN or infinity.
    pub fn with_velocity(mut self, velocity: Vec3) -> Result<Self, PhysicsError> {
        if !velocity.is_finite() {
            return Err(PhysicsError::NonFiniteMovement);
        }
        self.velocity = velocity;
        if velocity.z > 0.0 {
            self.grounded = false;
        }
        Ok(self)
    }
}

/// Advances player velocity and AABB movement against solid voxel cells.
///
/// Horizontal input is a desired world-space velocity and is clamped to
/// [`PhysicsConfig::max_horizontal_speed`]. Gravity updates vertical velocity.
/// Motion resolves in stable `X`, `Y`, `Z` order against the full swept AABB,
/// so a valid bounded step cannot tunnel through a one-cell wall or floor.
///
/// # Errors
///
/// Returns [`PhysicsError`] for invalid input/configuration, an out-of-range
/// result, or a body that starts inside a solid voxel.
pub fn step_player(
    mut body: PlayerBody,
    desired_horizontal_velocity: Vec2,
    delta_seconds: f32,
    config: PhysicsConfig,
    mut is_solid: impl FnMut(BlockPos) -> bool,
) -> Result<PlayerBody, PhysicsError> {
    validate_config(config)?;
    if !delta_seconds.is_finite()
        || delta_seconds <= 0.0
        || delta_seconds > config.max_delta_seconds
    {
        return Err(PhysicsError::InvalidDelta {
            delta_seconds,
            maximum: config.max_delta_seconds,
        });
    }
    if !desired_horizontal_velocity.is_finite() || !body.velocity.is_finite() {
        return Err(PhysicsError::NonFiniteMovement);
    }
    if let Some(block) = first_solid_overlap(body.bounds(), &mut is_solid)? {
        return Err(PhysicsError::BodyStartsInSolid { block });
    }

    let horizontal = desired_horizontal_velocity.clamp_length_max(config.max_horizontal_speed);
    body.velocity.x = horizontal.x;
    body.velocity.y = horizontal.y;
    body.velocity.z =
        (body.velocity.z - config.gravity * delta_seconds).max(-config.terminal_velocity);
    body.grounded = false;

    for axis in 0..3 {
        let displacement = body.velocity[axis] * delta_seconds;
        let collided = move_axis(&mut body, axis, displacement, &mut is_solid)?;
        if collided {
            body.velocity[axis] = 0.0;
            if axis == 2 && displacement < 0.0 {
                body.grounded = true;
            }
        }
    }
    validate_precise_position(body.position, body.half_extents)?;
    Ok(body)
}

fn validate_config(config: PhysicsConfig) -> Result<(), PhysicsError> {
    let values = [
        config.gravity,
        config.terminal_velocity,
        config.max_horizontal_speed,
        config.max_delta_seconds,
    ];
    if values
        .iter()
        .any(|value| !value.is_finite() || *value <= 0.0)
    {
        Err(PhysicsError::InvalidConfig)
    } else {
        Ok(())
    }
}

fn validate_precise_position(position: Vec3, half_extents: Vec3) -> Result<(), PhysicsError> {
    if !position.is_finite() || (position.abs() + half_extents).max_element() >= PRECISE_VOXEL_LIMIT
    {
        Err(PhysicsError::PositionOutsidePrecisionRange { position })
    } else {
        Ok(())
    }
}

fn first_solid_overlap(
    bounds: Aabb,
    is_solid: &mut impl FnMut(BlockPos) -> bool,
) -> Result<Option<BlockPos>, PhysicsError> {
    let [(min_x, max_x), (min_y, max_y), (min_z, max_z)] = cell_ranges(bounds)?;
    for x in min_x..=max_x {
        for y in min_y..=max_y {
            for z in min_z..=max_z {
                let block = BlockPos::new(x, y, z);
                if is_solid(block) && overlaps_block_beyond_tolerance(bounds, block) {
                    return Ok(Some(block));
                }
            }
        }
    }
    Ok(None)
}

fn overlaps_block_beyond_tolerance(bounds: Aabb, block: BlockPos) -> bool {
    let minimum = bounds.min();
    let maximum = bounds.max();
    axis_overlap_beyond_tolerance(minimum.x, maximum.x, block.x)
        && axis_overlap_beyond_tolerance(minimum.y, maximum.y, block.y)
        && axis_overlap_beyond_tolerance(minimum.z, maximum.z, block.z)
}

fn axis_overlap_beyond_tolerance(minimum: f32, maximum: f32, cell: i32) -> bool {
    let cell_minimum = f64::from(cell);
    let overlap = f64::from(maximum).min(cell_minimum + 1.0) - f64::from(minimum).max(cell_minimum);
    overlap > f64::from(COLLISION_EPSILON)
}

fn move_axis(
    body: &mut PlayerBody,
    axis: usize,
    displacement: f32,
    is_solid: &mut impl FnMut(BlockPos) -> bool,
) -> Result<bool, PhysicsError> {
    if displacement.abs() <= f32::EPSILON {
        return Ok(false);
    }

    let bounds = body.bounds();
    let mut swept_min = bounds.min();
    let mut swept_max = bounds.max();
    if displacement > 0.0 {
        swept_max[axis] += displacement;
    } else {
        swept_min[axis] += displacement;
    }
    let swept = Aabb::new(swept_min, swept_max)?;
    let [(min_x, max_x), (min_y, max_y), (min_z, max_z)] = cell_ranges(swept)?;
    let mut allowed = displacement;
    let mut collided = false;

    for x in min_x..=max_x {
        for y in min_y..=max_y {
            for z in min_z..=max_z {
                let block = BlockPos::new(x, y, z);
                if !is_solid(block) {
                    continue;
                }
                let coordinate = [x, y, z][axis];
                #[allow(
                    clippy::cast_precision_loss,
                    reason = "physics positions are bounded below exact f32 unit-cell precision"
                )]
                let block_min = coordinate as f32;
                if displacement > 0.0 {
                    let separation = block_min - bounds.max()[axis];
                    if separation >= -COLLISION_EPSILON && separation < allowed {
                        allowed = separation;
                        collided = true;
                    }
                } else {
                    let separation = block_min + 1.0 - bounds.min()[axis];
                    if separation <= COLLISION_EPSILON && separation > allowed {
                        allowed = separation;
                        collided = true;
                    }
                }
            }
        }
    }

    body.position[axis] += allowed;
    Ok(collided)
}

fn cell_ranges(bounds: Aabb) -> Result<[(i32, i32); 3], PhysicsError> {
    Ok([
        cell_range(bounds.min().x, bounds.max().x)?,
        cell_range(bounds.min().y, bounds.max().y)?,
        cell_range(bounds.min().z, bounds.max().z)?,
    ])
}

#[allow(
    clippy::cast_possible_truncation,
    reason = "range checks prove the integral f64 values fit in i32"
)]
fn cell_range(minimum: f32, maximum: f32) -> Result<(i32, i32), PhysicsError> {
    let first = f64::from(minimum).floor();
    let last = f64::from(maximum).ceil() - 1.0;
    if first < f64::from(i32::MIN)
        || first > f64::from(i32::MAX)
        || last < f64::from(i32::MIN)
        || last > f64::from(i32::MAX)
    {
        return Err(PhysicsError::PositionOutsidePrecisionRange {
            position: Vec3::splat(minimum),
        });
    }
    Ok((first as i32, last as i32))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gravity_lands_player_on_floor_and_sets_grounded() {
        let mut body =
            PlayerBody::standard(Vec3::new(0.5, 0.5, 3.0)).expect("test spawn has valid bounds");
        for _ in 0..40 {
            body = step_player(body, Vec2::ZERO, 0.05, PhysicsConfig::default(), |block| {
                block.z < 0
            })
            .expect("bounded fall remains valid");
        }
        assert!((body.position().z - 0.9).abs() < 1.0e-5);
        assert!(body.velocity().z.abs() < f32::EPSILON);
        assert!(body.is_grounded());
    }

    #[test]
    fn swept_aabb_cannot_tunnel_through_wall() {
        let mut body =
            PlayerBody::standard(Vec3::new(0.5, 0.5, 1.9)).expect("test spawn has valid bounds");
        for _ in 0..6 {
            body = step_player(
                body,
                Vec2::new(100.0, 0.0),
                0.1,
                PhysicsConfig::default(),
                |block| block.x == 2 || block.z < 0,
            )
            .expect("bounded movement remains valid");
        }
        assert!((body.position().x - 1.7).abs() < 1.0e-5);
        assert!(body.velocity().x.abs() < f32::EPSILON);
    }

    #[test]
    fn downward_terminal_step_cannot_tunnel_through_floor() {
        let body = PlayerBody::standard(Vec3::new(-0.5, -0.5, 4.0))
            .expect("test spawn has valid bounds")
            .with_velocity(Vec3::new(0.0, 0.0, -55.0))
            .expect("test velocity is finite");
        let body = step_player(body, Vec2::ZERO, 0.1, PhysicsConfig::default(), |block| {
            block.z < 0
        })
        .expect("swept collision catches the floor");
        assert!((body.position().z - 0.9).abs() < 1.0e-5);
        assert!(body.is_grounded());
    }

    #[test]
    fn elevated_floor_rounding_does_not_embed_the_player() {
        let mut body =
            PlayerBody::standard(Vec3::new(0.5, 0.5, 12.0)).expect("test spawn has valid bounds");
        for _ in 0..120 {
            body = step_player(
                body,
                Vec2::ZERO,
                1.0 / 60.0,
                PhysicsConfig::default(),
                |block| block.z <= 7,
            )
            .expect("landing tolerance must absorb sub-micrometer rounding drift");
        }
        assert!((body.position().z - 8.9).abs() < COLLISION_EPSILON);
        assert!(body.is_grounded());
    }

    #[test]
    fn material_initial_penetration_is_still_rejected() {
        let body =
            PlayerBody::standard(Vec3::new(0.5, 0.5, 8.89)).expect("test spawn has valid bounds");
        let result = step_player(
            body,
            Vec2::ZERO,
            1.0 / 60.0,
            PhysicsConfig::default(),
            |block| block.z <= 7,
        );
        assert!(matches!(
            result,
            Err(PhysicsError::BodyStartsInSolid { .. })
        ));
    }

    #[test]
    fn jumping_clears_grounded_and_preserves_upward_velocity() {
        let mut body =
            PlayerBody::standard(Vec3::new(0.5, 0.5, 0.9)).expect("test spawn has valid bounds");
        body = step_player(body, Vec2::ZERO, 0.05, PhysicsConfig::default(), |block| {
            block.z < 0
        })
        .expect("resting body detects the floor");
        assert!(body.is_grounded());
        body = body
            .with_velocity(Vec3::new(0.0, 0.0, 8.0))
            .expect("jump impulse is finite");
        body = step_player(body, Vec2::ZERO, 0.05, PhysicsConfig::default(), |block| {
            block.z < 0
        })
        .expect("jump step remains valid");
        assert!(!body.is_grounded());
        assert!(body.velocity().z > 0.0);
    }

    #[test]
    fn invalid_delta_does_not_query_world() {
        let body =
            PlayerBody::standard(Vec3::new(0.5, 0.5, 2.0)).expect("test spawn has valid bounds");
        let mut calls = 0;
        let result = step_player(body, Vec2::ZERO, 1.0, PhysicsConfig::default(), |_| {
            calls += 1;
            false
        });
        assert!(matches!(result, Err(PhysicsError::InvalidDelta { .. })));
        assert_eq!(calls, 0);
    }
}
