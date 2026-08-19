//! Fly camera controller in canonical Z-up world space (ADR 0011).

use glam::Vec3;
use latticeaxiom_core::space;
use latticeaxiom_render::Camera;
use winit::keyboard::KeyCode;

const MOVE_SPEED: f32 = 6.0; // meters per second
const LOOK_SENSITIVITY: f32 = 0.0025; // radians per mouse count
const MAX_PITCH: f32 = 1.54; // just under 90 degrees

#[derive(Debug, Default)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "raw digital input state: each key really is an independent pressed/released bit"
)]
struct Movement {
    forward: bool,
    backward: bool,
    left: bool,
    right: bool,
    up: bool,
    down: bool,
}

/// WASD + mouse-look fly camera.
///
/// Yaw is measured around world `+Z` starting at `+X` (east) and increasing
/// toward `+Y` (north), matching the positive-rotation convention of ADR
/// 0011; pitch raises the view toward `+Z`.
#[derive(Debug)]
pub struct CameraController {
    position: Vec3,
    yaw: f32,
    pitch: f32,
    movement: Movement,
}

impl CameraController {
    /// Creates a controller at `position` looking toward `target`.
    #[must_use]
    pub fn looking_at(position: Vec3, target: Vec3) -> Self {
        let direction = (target - position).normalize_or(Vec3::X);
        Self {
            position,
            yaw: direction.y.atan2(direction.x),
            pitch: direction.z.clamp(-1.0, 1.0).asin(),
            movement: Movement::default(),
        }
    }

    /// Records a key press or release.
    pub fn set_key(&mut self, key: KeyCode, pressed: bool) {
        match key {
            KeyCode::KeyW => self.movement.forward = pressed,
            KeyCode::KeyS => self.movement.backward = pressed,
            KeyCode::KeyA => self.movement.left = pressed,
            KeyCode::KeyD => self.movement.right = pressed,
            KeyCode::Space => self.movement.up = pressed,
            KeyCode::ControlLeft | KeyCode::ControlRight => self.movement.down = pressed,
            _ => {}
        }
    }

    /// Applies a mouse-look delta in raw mouse counts.
    ///
    /// Moving the mouse right turns the view clockwise seen from above
    /// (decreasing yaw); moving it up raises the pitch.
    #[expect(
        clippy::cast_possible_truncation,
        reason = "mouse deltas are tiny; f64 -> f32 loses nothing relevant"
    )]
    pub fn apply_look(&mut self, delta_x: f64, delta_y: f64) {
        self.yaw -= delta_x as f32 * LOOK_SENSITIVITY;
        self.pitch = (self.pitch - delta_y as f32 * LOOK_SENSITIVITY).clamp(-MAX_PITCH, MAX_PITCH);
    }

    /// Advances the position by the active movement for `dt` seconds.
    pub fn advance(&mut self, dt: f32) {
        let forward = self.direction();
        let right = forward.cross(space::UP).normalize_or_zero();

        let mut wish = Vec3::ZERO;
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
        if self.movement.up {
            wish += space::UP;
        }
        if self.movement.down {
            wish -= space::UP;
        }

        self.position += wish.normalize_or_zero() * MOVE_SPEED * dt;
    }

    /// Current view direction.
    #[must_use]
    pub fn direction(&self) -> Vec3 {
        Vec3::new(
            self.pitch.cos() * self.yaw.cos(),
            self.pitch.cos() * self.yaw.sin(),
            self.pitch.sin(),
        )
    }

    /// Extracts the current camera.
    #[must_use]
    pub fn camera(&self) -> Camera {
        let mut camera = Camera::look_at(self.position, self.position + self.direction());
        camera.z_far = 300.0;
        camera
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn looking_at_recovers_direction() {
        let controller = CameraController::looking_at(Vec3::new(5.0, -5.0, 3.0), Vec3::ZERO);
        let expected = (Vec3::ZERO - Vec3::new(5.0, -5.0, 3.0)).normalize();
        assert!((controller.direction() - expected).length() < 1e-5);
    }

    #[test]
    fn forward_movement_follows_view_direction() {
        let mut controller = CameraController::looking_at(Vec3::ZERO, Vec3::new(1.0, 0.0, 0.0));
        controller.set_key(KeyCode::KeyW, true);
        controller.advance(1.0);
        let camera = controller.camera();
        assert!(camera.position.x > 0.9 * MOVE_SPEED);
        assert!(camera.position.y.abs() < 1e-4);
    }
}
