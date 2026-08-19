//! The milestone 1 scene: one spinning cube on a ground slab.
//!
//! The scene is renderer-agnostic on purpose: the same code drives the wgpu
//! window and the headless CI run, which is exactly the facade boundary the
//! milestone must prove.

use glam::{Mat4, Quat, Vec3};
use latticeaxiom_render::{
    Camera, MaterialData, MaterialId, MeshData, MeshId, MeshInstance, RenderError, RenderWorld,
    Renderer,
};

/// Handles and parameters of the demo scene.
#[derive(Debug)]
pub struct Scene {
    cube_mesh: MeshId,
    cube_material: MaterialId,
    ground_material: MaterialId,
}

impl Scene {
    /// Uploads the scene's resources to a renderer.
    ///
    /// # Errors
    ///
    /// Propagates upload failures from the renderer.
    pub fn upload<R: Renderer>(renderer: &mut R) -> Result<Self, RenderError> {
        let cube_mesh = renderer.upload_mesh(&unit_cube())?;
        let cube_material = renderer.upload_material(&MaterialData {
            base_color: [0.86, 0.42, 0.19, 1.0],
        })?;
        let ground_material = renderer.upload_material(&MaterialData {
            base_color: [0.30, 0.42, 0.28, 1.0],
        })?;
        Ok(Self {
            cube_mesh,
            cube_material,
            ground_material,
        })
    }

    /// Extracts the frame at simulation time `seconds`.
    ///
    /// The cube slowly rotates around world `+Z` (right-hand rule: `+X`
    /// toward `+Y` when seen from above, ADR 0011).
    #[must_use]
    pub fn render_world(&self, camera: Camera, seconds: f32) -> RenderWorld {
        let cube = MeshInstance {
            mesh: self.cube_mesh,
            material: self.cube_material,
            transform: Mat4::from_rotation_translation(
                Quat::from_rotation_z(seconds * 0.6),
                Vec3::new(0.0, 0.0, 0.5),
            ),
        };
        let ground = MeshInstance {
            mesh: self.cube_mesh,
            material: self.ground_material,
            transform: Mat4::from_scale_rotation_translation(
                Vec3::new(24.0, 24.0, 0.1),
                Quat::IDENTITY,
                Vec3::new(0.0, 0.0, -0.05),
            ),
        };
        RenderWorld {
            camera,
            instances: vec![cube, ground],
        }
    }
}

/// A unit cube centered at the origin (edge length 1), with per-face normals.
///
/// Triangles are counter-clockwise seen from outside, matching the facade's
/// front-face convention.
#[must_use]
pub fn unit_cube() -> MeshData {
    // One quad per face; +X, -X, +Y, -Y, +Z, -Z.
    let face_data: [(Vec3, Vec3, Vec3); 6] = [
        (Vec3::X, Vec3::Y, Vec3::Z),
        (Vec3::NEG_X, Vec3::NEG_Y, Vec3::Z),
        (Vec3::Y, Vec3::NEG_X, Vec3::Z),
        (Vec3::NEG_Y, Vec3::X, Vec3::Z),
        (Vec3::Z, Vec3::X, Vec3::Y),
        (Vec3::NEG_Z, Vec3::X, Vec3::NEG_Y),
    ];

    let mut positions = Vec::with_capacity(24);
    let mut normals = Vec::with_capacity(24);
    let mut indices = Vec::with_capacity(36);

    let mut base: u32 = 0;
    for (normal, tangent, bitangent) in face_data {
        let origin = normal * 0.5;
        // Counter-clockwise when viewed from along the outward normal.
        let corners = [
            origin - tangent * 0.5 - bitangent * 0.5,
            origin + tangent * 0.5 - bitangent * 0.5,
            origin + tangent * 0.5 + bitangent * 0.5,
            origin - tangent * 0.5 + bitangent * 0.5,
        ];
        for corner in corners {
            positions.push(corner.to_array());
            normals.push(normal.to_array());
        }
        indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
        base += 4;
    }

    MeshData {
        positions,
        normals,
        indices,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unit_cube_is_valid() {
        unit_cube().validate().unwrap();
        assert_eq!(unit_cube().positions.len(), 24);
        assert_eq!(unit_cube().indices.len(), 36);
    }

    #[test]
    fn cube_faces_point_outward() {
        let cube = unit_cube();
        for (position, normal) in cube.positions.iter().zip(&cube.normals) {
            let p = Vec3::from_array(*position);
            let n = Vec3::from_array(*normal);
            // Every vertex of a face lies on the half of the cube its normal
            // points toward.
            assert!(p.dot(n) > 0.0, "vertex {p:?} behind its face normal {n:?}");
        }
    }
}
