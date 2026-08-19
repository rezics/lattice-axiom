//! The renderer contract implemented by every backend.

use thiserror::Error;

use crate::world::{FrameReport, MaterialData, MaterialId, MeshData, MeshId, RenderWorld};

/// A renderer that can receive resources and draw extracted frames.
///
/// Implementations must be interchangeable: the wgpu backend and the headless
/// backend consume the same [`RenderWorld`] and reject the same invalid input.
/// The shared expectations are encoded in [`crate::conformance`], which every
/// implementation runs in its test suite.
pub trait Renderer {
    /// Uploads a mesh and returns a handle valid for this renderer.
    ///
    /// # Errors
    ///
    /// Returns [`RenderError::InvalidMesh`] when `mesh` fails
    /// [`MeshData::validate`], or [`RenderError::Backend`] when the backend
    /// cannot allocate the resource.
    fn upload_mesh(&mut self, mesh: &MeshData) -> Result<MeshId, RenderError>;

    /// Uploads a material and returns a handle valid for this renderer.
    ///
    /// # Errors
    ///
    /// Returns [`RenderError::InvalidMaterial`] when the material contains
    /// non-finite components, or [`RenderError::Backend`] when the backend
    /// cannot allocate the resource.
    fn upload_material(&mut self, material: &MaterialData) -> Result<MaterialId, RenderError>;

    /// Notifies the renderer that the output size changed (in physical
    /// pixels). Implementations without a swapchain may ignore this.
    fn resize(&mut self, width: u32, height: u32);

    /// Draws one extracted frame.
    ///
    /// # Errors
    ///
    /// Returns a validation error from [`RenderWorld::validate`] when the
    /// world references unknown handles or contains non-finite state, or
    /// [`RenderError::Backend`] when presentation fails.
    fn submit(&mut self, world: &RenderWorld) -> Result<FrameReport, RenderError>;
}

/// Errors produced by renderer implementations.
///
/// Backend-specific failures are carried as strings so that no GPU types leak
/// through the facade (ADR 0006).
#[derive(Debug, Error)]
pub enum RenderError {
    /// The mesh data failed structural validation.
    #[error("invalid mesh data: {reason}")]
    InvalidMesh {
        /// Human-readable explanation of the failed invariant.
        reason: String,
    },
    /// The material data failed validation.
    #[error("invalid material data: {reason}")]
    InvalidMaterial {
        /// Human-readable explanation of the failed invariant.
        reason: String,
    },
    /// An instance referenced a mesh handle this renderer never issued.
    #[error("unknown mesh handle {0:?}")]
    UnknownMesh(MeshId),
    /// An instance referenced a material handle this renderer never issued.
    #[error("unknown material handle {0:?}")]
    UnknownMaterial(MaterialId),
    /// An instance transform contained non-finite components.
    #[error("instance {index} has a non-finite transform")]
    NonFiniteTransform {
        /// Index of the offending instance in [`RenderWorld::instances`].
        index: usize,
    },
    /// The camera state was not finite or not usable.
    #[error("camera state is not finite")]
    NonFiniteCamera,
    /// A backend-specific failure, reported without leaking backend types.
    #[error("render backend error: {message}")]
    Backend {
        /// Human-readable backend diagnostic.
        message: String,
    },
}
