//! GPU-free implementation of the Lattice Axiom rendering facade.
//!
//! [`HeadlessRenderer`] performs every semantic step of a real renderer —
//! resource bookkeeping, validation, frame accounting — without creating a
//! window or a GPU device. It backs unit tests, the shared conformance suite,
//! and the `--headless` mode of the demo host, which is what keeps CI able to
//! verify the milestone 1 exit criteria on machines without a GPU.

use latticeaxiom_render::{
    FrameReport, MaterialData, MaterialId, MeshData, MeshId, RenderError, RenderWorld, Renderer,
};

/// Bookkeeping for one uploaded mesh; the headless path stores metadata only.
#[derive(Clone, Copy, Debug)]
struct StoredMesh {
    #[allow(
        dead_code,
        reason = "kept for parity with GPU-side bookkeeping and debugging"
    )]
    vertex_count: usize,
    #[allow(
        dead_code,
        reason = "kept for parity with GPU-side bookkeeping and debugging"
    )]
    index_count: usize,
}

/// A renderer that validates and accounts for frames without any GPU work.
#[derive(Debug)]
pub struct HeadlessRenderer {
    meshes: Vec<StoredMesh>,
    materials: Vec<MaterialData>,
    width: u32,
    height: u32,
    frames_submitted: u64,
}

impl HeadlessRenderer {
    /// Creates a headless renderer with a virtual output size.
    #[must_use]
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            meshes: Vec::new(),
            materials: Vec::new(),
            width: width.max(1),
            height: height.max(1),
            frames_submitted: 0,
        }
    }

    /// Current virtual output size in physical pixels.
    #[must_use]
    pub fn size(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    /// Number of frames successfully submitted so far.
    #[must_use]
    pub fn frames_submitted(&self) -> u64 {
        self.frames_submitted
    }

    fn mesh_count(&self) -> u32 {
        u32::try_from(self.meshes.len()).unwrap_or(u32::MAX)
    }

    fn material_count(&self) -> u32 {
        u32::try_from(self.materials.len()).unwrap_or(u32::MAX)
    }
}

impl Renderer for HeadlessRenderer {
    fn upload_mesh(&mut self, mesh: &MeshData) -> Result<MeshId, RenderError> {
        mesh.validate()?;
        let id = MeshId::from_raw(u32::try_from(self.meshes.len()).map_err(|_| {
            RenderError::Backend {
                message: "mesh table exceeded u32 range".to_owned(),
            }
        })?);
        self.meshes.push(StoredMesh {
            vertex_count: mesh.positions.len(),
            index_count: mesh.indices.len(),
        });
        Ok(id)
    }

    fn replace_mesh(&mut self, id: MeshId, mesh: &MeshData) -> Result<(), RenderError> {
        let slot = self
            .meshes
            .get_mut(id.to_raw() as usize)
            .ok_or(RenderError::UnknownMesh(id))?;
        mesh.validate()?;
        *slot = StoredMesh {
            vertex_count: mesh.positions.len(),
            index_count: mesh.indices.len(),
        };
        Ok(())
    }

    fn upload_material(&mut self, material: &MaterialData) -> Result<MaterialId, RenderError> {
        if !material.base_color.iter().all(|c| c.is_finite()) {
            return Err(RenderError::InvalidMaterial {
                reason: "base color contains non-finite components".to_owned(),
            });
        }
        let id = MaterialId::from_raw(u32::try_from(self.materials.len()).map_err(|_| {
            RenderError::Backend {
                message: "material table exceeded u32 range".to_owned(),
            }
        })?);
        self.materials.push(*material);
        Ok(id)
    }

    fn resize(&mut self, width: u32, height: u32) {
        self.width = width.max(1);
        self.height = height.max(1);
    }

    fn submit(&mut self, world: &RenderWorld) -> Result<FrameReport, RenderError> {
        world.validate(self.mesh_count(), self.material_count())?;
        self.frames_submitted += 1;
        Ok(FrameReport {
            instances_drawn: world.instances.len(),
        })
    }
}

#[cfg(test)]
mod tests {
    use latticeaxiom_render::conformance;

    use super::*;

    #[test]
    fn conforms_to_render_contract() {
        conformance::run_all(|| HeadlessRenderer::new(64, 64));
    }

    #[test]
    fn counts_submitted_frames() {
        use latticeaxiom_render::glam::Vec3;

        let mut renderer = HeadlessRenderer::new(16, 16);
        let world = RenderWorld {
            camera: latticeaxiom_render::Camera::look_at(Vec3::new(3.0, -4.0, 2.0), Vec3::ZERO),
            instances: Vec::new(),
        };
        assert_eq!(renderer.frames_submitted(), 0);
        renderer.submit(&world).unwrap();
        renderer.submit(&world).unwrap();
        assert_eq!(renderer.frames_submitted(), 2);
    }
}
