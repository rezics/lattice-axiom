//! Perspective camera in canonical world space.

use glam::{Mat4, Vec3};

/// Perspective camera in right-handed, Z-up world space (ADR 0011).
///
/// The facade expresses only world-space semantics; conversion to a backend's
/// clip space and depth range happens inside renderer implementations. The
/// matrices produced here target a `[0, 1]` depth range.
#[derive(Clone, Copy, Debug)]
pub struct Camera {
    /// Eye position in world space (meters).
    pub position: Vec3,
    /// Unit view direction in world space.
    pub direction: Vec3,
    /// Vertical field of view in radians.
    pub fov_y: f32,
    /// Near clip plane distance in meters.
    pub z_near: f32,
    /// Far clip plane distance in meters.
    pub z_far: f32,
}

impl Camera {
    /// Default vertical field of view (60 degrees).
    pub const DEFAULT_FOV_Y: f32 = std::f32::consts::FRAC_PI_3;

    /// Creates a camera at `position` looking toward `target`, with default
    /// field of view and clip planes.
    #[must_use]
    pub fn look_at(position: Vec3, target: Vec3) -> Self {
        Self {
            position,
            direction: (target - position).normalize_or(Vec3::X),
            fov_y: Self::DEFAULT_FOV_Y,
            z_near: 0.05,
            z_far: 500.0,
        }
    }

    /// World-to-view matrix (right-handed, up is world `+Z`).
    #[must_use]
    pub fn view(&self) -> Mat4 {
        glam::camera::rh::view::look_to_mat4(
            self.position,
            self.direction.normalize_or(Vec3::X),
            Vec3::Z,
        )
    }

    /// View-to-clip matrix for the given aspect ratio.
    ///
    /// Uses the DirectX/WebGPU convention (`[0, 1]` depth, Y-up NDC), which
    /// is what the wgpu backend consumes directly.
    #[must_use]
    pub fn projection(&self, aspect: f32) -> Mat4 {
        glam::camera::rh::proj::directx::perspective(self.fov_y, aspect, self.z_near, self.z_far)
    }

    /// Combined world-to-clip matrix for the given aspect ratio.
    #[must_use]
    pub fn view_projection(&self, aspect: f32) -> Mat4 {
        self.projection(aspect) * self.view()
    }

    /// Returns whether every camera parameter is finite and usable.
    #[must_use]
    pub fn is_finite(&self) -> bool {
        self.position.is_finite()
            && self.direction.is_finite()
            && self.direction.length_squared() > 0.0
            && self.fov_y.is_finite()
            && self.fov_y > 0.0
            && self.z_near.is_finite()
            && self.z_near > 0.0
            && self.z_far.is_finite()
            && self.z_far > self.z_near
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn look_at_produces_finite_camera() {
        let camera = Camera::look_at(Vec3::new(4.0, -6.0, 3.0), Vec3::ZERO);
        assert!(camera.is_finite());
        assert!(camera.view_projection(16.0 / 9.0).is_finite());
    }

    #[test]
    fn world_up_maps_to_view_up() {
        // Looking east from the origin, world +Z must be "up" in view space
        // (negative-ish Y is *not* acceptable: view space is right-handed
        // with +Y up and -Z forward).
        let camera = Camera::look_at(Vec3::ZERO, Vec3::X);
        let up_in_view = camera.view().transform_vector3(Vec3::Z);
        assert!((up_in_view - Vec3::Y).length() < 1e-6);
    }
}
