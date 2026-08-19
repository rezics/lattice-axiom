//! Data exchanged between simulation and renderer implementations.

use glam::Mat4;

use crate::camera::Camera;
use crate::renderer::RenderError;

/// Opaque handle to a mesh uploaded to a renderer.
///
/// Handles are only meaningful for the renderer instance that issued them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct MeshId(u32);

/// Opaque handle to a material uploaded to a renderer.
///
/// Handles are only meaningful for the renderer instance that issued them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct MaterialId(u32);

impl MeshId {
    /// Wraps a raw slot index. Intended for renderer implementations.
    #[must_use]
    pub const fn from_raw(raw: u32) -> Self {
        Self(raw)
    }

    /// Returns the raw slot index. Intended for renderer implementations.
    #[must_use]
    pub const fn to_raw(self) -> u32 {
        self.0
    }
}

impl MaterialId {
    /// Wraps a raw slot index. Intended for renderer implementations.
    #[must_use]
    pub const fn from_raw(raw: u32) -> Self {
        Self(raw)
    }

    /// Returns the raw slot index. Intended for renderer implementations.
    #[must_use]
    pub const fn to_raw(self) -> u32 {
        self.0
    }
}

/// CPU-side triangle mesh data in canonical world conventions (Z-up, meters).
///
/// Indices form counter-clockwise triangles when viewed from outside, which
/// backends treat as front-facing.
#[derive(Clone, Debug, Default)]
pub struct MeshData {
    /// Vertex positions.
    pub positions: Vec<[f32; 3]>,
    /// Per-vertex normals; must have the same length as `positions`.
    pub normals: Vec<[f32; 3]>,
    /// Triangle list indices into `positions`.
    pub indices: Vec<u32>,
}

impl MeshData {
    /// Checks the structural invariants renderer implementations rely on.
    ///
    /// # Errors
    ///
    /// Returns [`RenderError::InvalidMesh`] when the mesh is empty, when
    /// attribute lengths disagree, when the index count is not a multiple of
    /// three, when an index is out of range, or when any attribute is not
    /// finite.
    pub fn validate(&self) -> Result<(), RenderError> {
        let invalid = |reason: String| Err(RenderError::InvalidMesh { reason });

        if self.positions.is_empty() {
            return invalid("mesh has no vertices".to_owned());
        }
        if self.normals.len() != self.positions.len() {
            return invalid(format!(
                "normal count {} does not match vertex count {}",
                self.normals.len(),
                self.positions.len()
            ));
        }
        if self.indices.is_empty() || !self.indices.len().is_multiple_of(3) {
            return invalid(format!(
                "index count {} is not a positive multiple of 3",
                self.indices.len()
            ));
        }
        let vertex_count =
            u32::try_from(self.positions.len()).map_err(|_| RenderError::InvalidMesh {
                reason: "vertex count exceeds u32 range".to_owned(),
            })?;
        if let Some(&index) = self.indices.iter().find(|&&index| index >= vertex_count) {
            return invalid(format!(
                "index {index} out of range for {vertex_count} vertices"
            ));
        }
        let all_finite = self
            .positions
            .iter()
            .chain(self.normals.iter())
            .flatten()
            .all(|component| component.is_finite());
        if !all_finite {
            return invalid("mesh contains non-finite attribute components".to_owned());
        }
        Ok(())
    }
}

/// Uniform material parameters; milestone 1 supports flat colors only.
#[derive(Clone, Copy, Debug)]
pub struct MaterialData {
    /// Linear-space RGBA base color.
    pub base_color: [f32; 4],
}

/// One drawable placement of a mesh with a material.
#[derive(Clone, Copy, Debug)]
pub struct MeshInstance {
    /// Mesh handle previously returned by the target renderer.
    pub mesh: MeshId,
    /// Material handle previously returned by the target renderer.
    pub material: MaterialId,
    /// Model-to-world transform (Z-up world space).
    pub transform: Mat4,
}

/// Everything a renderer needs to draw one frame.
///
/// The simulation extracts a `RenderWorld` each frame; renderers never reach
/// back into gameplay state (ADR 0006).
#[derive(Clone, Debug)]
pub struct RenderWorld {
    /// Active camera.
    pub camera: Camera,
    /// Mesh instances to draw this frame.
    pub instances: Vec<MeshInstance>,
}

impl RenderWorld {
    /// Validates this world against a renderer's resource tables.
    ///
    /// Shared by renderer implementations so that the headless and GPU paths
    /// reject exactly the same invalid input.
    ///
    /// # Errors
    ///
    /// Returns [`RenderError::NonFiniteCamera`] when the camera state is not
    /// finite, [`RenderError::UnknownMesh`] / [`RenderError::UnknownMaterial`]
    /// when an instance references a handle the renderer never issued, and
    /// [`RenderError::NonFiniteTransform`] when an instance transform contains
    /// non-finite components.
    pub fn validate(&self, mesh_count: u32, material_count: u32) -> Result<(), RenderError> {
        if !self.camera.is_finite() {
            return Err(RenderError::NonFiniteCamera);
        }
        for (index, instance) in self.instances.iter().enumerate() {
            if instance.mesh.to_raw() >= mesh_count {
                return Err(RenderError::UnknownMesh(instance.mesh));
            }
            if instance.material.to_raw() >= material_count {
                return Err(RenderError::UnknownMaterial(instance.material));
            }
            if !instance.transform.is_finite() {
                return Err(RenderError::NonFiniteTransform { index });
            }
        }
        Ok(())
    }
}

/// Summary of a submitted frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FrameReport {
    /// Number of mesh instances accepted and drawn.
    pub instances_drawn: usize,
}
