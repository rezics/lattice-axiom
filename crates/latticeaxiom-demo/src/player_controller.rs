//! First-person walking controller in canonical Z-up world space.

use glam::{Vec2, Vec3};
use latticeaxiom_core::{BlockPos, PhysicsConfig, PhysicsError, PlayerBody, step_player};
use latticeaxiom_render::Camera;
use winit::keyboard::KeyCode;

const WALK_SPEED: f32 = 6.0;
const JUMP_SPEED: f32 = 8.0;
const EYE_OFFSET_FROM_CENTER: f32 = 0.65;
const LOOK_SENSITIVITY: f32 = 0.0025;
const MAX_PITCH: f32 = 1.54;
const SIMULATION_STEP: f32 = 1.0 / 60.0;
const MAX_FRAME_DELTA: f32 = 0.25;

#[derive(Debug, Default)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "digital movement keys are independent pressed/released inputs"
)]
struct Movement {
    forward: bool,
    backward: bool,
    left: bool,
    right: bool,
    jump_held: bool,
    jump_requested: bool,
}

/// Walking player body, view angles, and raw digital input state.
///
/// Yaw starts at world `+X` and increases toward `+Y`; pitch raises the view
/// toward `+Z`. Physics remains owned by `latticeaxiom-core` while this host
/// layer translates winit input into the domain step.
#[derive(Debug)]
pub struct PlayerController {
    body: PlayerBody,
    yaw: f32,
    pitch: f32,
    movement: Movement,
    physics: PhysicsConfig,
}

impl PlayerController {
    /// Creates a standing player body at the supplied AABB center.
    ///
    /// # Errors
    ///
    /// Returns [`PhysicsError`] when `position` is not a valid player center.
    pub fn spawn(position: Vec3, yaw: f32, pitch: f32) -> Result<Self, PhysicsError> {
        Ok(Self {
            body: PlayerBody::standard(position)?,
            yaw,
            pitch: pitch.clamp(-MAX_PITCH, MAX_PITCH),
            movement: Movement::default(),
            physics: PhysicsConfig::default(),
        })
    }

    /// Records a movement key transition and buffers a jump on the rising edge.
    pub fn set_key(&mut self, key: KeyCode, pressed: bool) {
        match key {
            KeyCode::KeyW => self.movement.forward = pressed,
            KeyCode::KeyS => self.movement.backward = pressed,
            KeyCode::KeyA => self.movement.left = pressed,
            KeyCode::KeyD => self.movement.right = pressed,
            KeyCode::Space => {
                if pressed && !self.movement.jump_held {
                    self.movement.jump_requested = true;
                }
                self.movement.jump_held = pressed;
            }
            _ => {}
        }
    }

    /// Releases all digital inputs, preventing focus changes from sticking keys.
    pub fn clear_inputs(&mut self) {
        self.movement = Movement::default();
    }

    /// Applies relative raw mouse motion to yaw and pitch.
    #[expect(
        clippy::cast_possible_truncation,
        reason = "converted mouse deltas are rejected unless they remain finite"
    )]
    pub fn apply_look(&mut self, delta_x: f64, delta_y: f64) {
        let yaw_delta = delta_x as f32 * LOOK_SENSITIVITY;
        let pitch_delta = delta_y as f32 * LOOK_SENSITIVITY;
        if !yaw_delta.is_finite() || !pitch_delta.is_finite() {
            return;
        }

        self.yaw = (self.yaw - yaw_delta).rem_euclid(std::f32::consts::TAU);
        self.pitch = (self.pitch - pitch_delta).clamp(-MAX_PITCH, MAX_PITCH);
    }

    /// Advances walking, gravity, collision, and a buffered jump.
    ///
    /// Long renderer stalls are capped, then subdivided so the core collision
    /// step stays within its latency and swept-volume budget.
    ///
    /// # Errors
    ///
    /// Returns [`PhysicsError`] if the body or queried simulation step becomes
    /// invalid.
    pub fn advance(
        &mut self,
        delta_seconds: f32,
        mut is_solid: impl FnMut(BlockPos) -> bool,
    ) -> Result<(), PhysicsError> {
        if !delta_seconds.is_finite() || delta_seconds <= 0.0 {
            return Ok(());
        }

        let forward = Vec2::new(self.yaw.cos(), self.yaw.sin());
        let right = Vec2::new(forward.y, -forward.x);
        let mut wish = Vec2::ZERO;
        if self.movement.forward {
            wish += forward;
        }
        if self.movement.backward {
            wish -= forward;
        }
        if self.movement.right {
            wish += right;
        }
        if self.movement.left {
            wish -= right;
        }
        let desired_velocity = wish.normalize_or_zero() * WALK_SPEED;

        let jump_requested = std::mem::take(&mut self.movement.jump_requested);
        if jump_requested && self.body.is_grounded() {
            let mut velocity = self.body.velocity();
            velocity.z = JUMP_SPEED;
            self.body = self.body.with_velocity(velocity)?;
        }

        let mut remaining = delta_seconds.min(MAX_FRAME_DELTA);
        while remaining > f32::EPSILON {
            let step = remaining.min(SIMULATION_STEP);
            self.body = step_player(
                self.body,
                desired_velocity,
                step,
                self.physics,
                &mut is_solid,
            )?;
            remaining -= step;
        }
        Ok(())
    }

    /// Unit view direction in canonical world space.
    #[must_use]
    pub fn direction(&self) -> Vec3 {
        Vec3::new(
            self.pitch.cos() * self.yaw.cos(),
            self.pitch.cos() * self.yaw.sin(),
            self.pitch.sin(),
        )
    }

    /// Eye position used by camera and block targeting.
    #[must_use]
    pub fn eye_position(&self) -> Vec3 {
        self.body.position() + Vec3::Z * EYE_OFFSET_FROM_CENTER
    }

    /// Current player collision body.
    #[must_use]
    pub const fn body(&self) -> PlayerBody {
        self.body
    }

    /// Voxel containing the player's AABB center, used to recenter streaming.
    #[must_use]
    pub fn center_block(&self) -> BlockPos {
        BlockPos::containing(self.body.position())
    }

    /// Extracts the first-person camera.
    #[must_use]
    pub fn camera(&self) -> Camera {
        Camera::look_at(self.eye_position(), self.eye_position() + self.direction())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn floor(block: BlockPos) -> bool {
        block.z < 0
    }

    #[test]
    fn gravity_settles_the_controller_on_a_floor() {
        let mut player = PlayerController::spawn(Vec3::new(0.5, 0.5, 3.0), 0.0, 0.0)
            .expect("test spawn must be valid");
        for _ in 0..60 {
            player
                .advance(1.0 / 60.0, floor)
                .expect("bounded fall must remain valid");
        }
        assert!((player.body().position().z - 0.9).abs() < 1.0e-4);
        assert!(player.body().is_grounded());
    }

    #[test]
    fn forward_input_follows_yaw_on_the_horizontal_plane() {
        let mut player = PlayerController::spawn(Vec3::new(0.5, 0.5, 0.9), 0.0, 0.7)
            .expect("test spawn must be valid");
        player
            .advance(1.0 / 60.0, floor)
            .expect("settling step must remain valid");
        player.set_key(KeyCode::KeyW, true);
        player
            .advance(0.1, floor)
            .expect("walking step must remain valid");
        assert!(player.body().position().x > 1.0);
        assert!((player.body().position().y - 0.5).abs() < 1.0e-4);
        assert!((player.body().position().z - 0.9).abs() < 1.0e-4);
    }

    #[test]
    fn space_applies_one_grounded_jump_impulse() {
        let mut player = PlayerController::spawn(Vec3::new(0.5, 0.5, 0.9), 0.0, 0.0)
            .expect("test spawn must be valid");
        player
            .advance(1.0 / 60.0, floor)
            .expect("settling step must remain valid");
        player.set_key(KeyCode::Space, true);
        player
            .advance(1.0 / 60.0, floor)
            .expect("jump step must remain valid");
        assert!(player.body().velocity().z > 0.0);
        assert!(!player.body().is_grounded());
    }

    #[test]
    fn invalid_frame_deltas_are_noops_and_preserve_buffered_input() {
        let mut player = PlayerController::spawn(Vec3::new(0.5, 0.5, 0.9), 0.0, 0.0)
            .expect("test spawn must be valid");
        player
            .advance(1.0 / 60.0, floor)
            .expect("settling step must remain valid");
        player.set_key(KeyCode::Space, true);
        let before = player.body();
        let mut queries = 0;

        for delta in [0.0, -1.0, f32::NAN, f32::INFINITY] {
            player
                .advance(delta, |_| {
                    queries += 1;
                    false
                })
                .expect("invalid host-frame deltas are intentionally ignored");
            assert_eq!(player.body(), before);
        }
        assert_eq!(queries, 0);

        player
            .advance(1.0 / 60.0, floor)
            .expect("the buffered jump must survive an ignored frame");
        assert!(player.body().velocity().z > 0.0);
    }

    #[test]
    fn a_huge_finite_frame_delta_is_capped_and_subdivided() {
        let mut player = PlayerController::spawn(Vec3::new(0.5, 0.5, 0.9), 0.0, 0.0)
            .expect("test spawn must be valid");
        player
            .advance(1.0 / 60.0, floor)
            .expect("settling step must remain valid");
        player.set_key(KeyCode::KeyW, true);

        player
            .advance(f32::MAX, floor)
            .expect("a renderer stall must stay inside the core step budget");

        assert!((player.body().position().x - 2.0).abs() < 1.0e-4);
        assert!((player.body().position().z - 0.9).abs() < 1.0e-4);
    }

    #[test]
    fn invalid_mouse_motion_cannot_poison_movement() {
        let mut player = PlayerController::spawn(Vec3::new(0.5, 0.5, 0.9), 0.0, 0.0)
            .expect("test spawn must be valid");
        player
            .advance(1.0 / 60.0, floor)
            .expect("settling step must remain valid");
        let direction = player.direction();

        for delta in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, f64::MAX] {
            player.apply_look(delta, 0.0);
            player.apply_look(0.0, delta);
        }

        assert_eq!(player.direction(), direction);
        player.set_key(KeyCode::KeyW, true);
        player
            .advance(1.0 / 60.0, floor)
            .expect("rejected mouse input must not cause non-finite physics input");
        assert!(player.body().position().is_finite());
    }

    #[test]
    fn jumping_onto_an_elevated_voxel_floor_remains_stable() {
        let terrain = |block: BlockPos| {
            let surface = if block.y >= 2 { 7 } else { 6 };
            block.z <= surface
        };
        let mut player =
            PlayerController::spawn(Vec3::new(0.5, 0.5, 7.9), std::f32::consts::FRAC_PI_2, 0.0)
                .expect("test spawn must be valid");
        player
            .advance(1.0 / 60.0, terrain)
            .expect("settling step must remain valid");
        player.set_key(KeyCode::KeyW, true);
        player.set_key(KeyCode::Space, true);

        for _ in 0..180 {
            player
                .advance(1.0 / 60.0, terrain)
                .expect("elevated landing must not become a persistent overlap");
        }

        assert!(player.body().position().y > 2.0);
        assert!((player.body().position().z - 8.9).abs() < 1.0e-4);
        assert!(player.body().is_grounded());
    }
}
