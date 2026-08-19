//! Backend-agnostic rendering facade for Lattice Axiom (ADR 0006).
//!
//! Core and content crates describe *what* to present by building a
//! [`RenderWorld`]; renderer implementations decide *how* to execute it. The
//! facade owns no GPU types: the wgpu backend lives in
//! `latticeaxiom-render-wgpu` and a GPU-free implementation lives in
//! `latticeaxiom-render-headless`. Both must pass the shared
//! [`conformance`] suite.
//!
//! The milestone 1 surface is deliberately small: a camera, mesh and material
//! uploads, mesh instances, and [`Renderer::submit`]. Chunk-mesh instances and
//! further primitives join in later milestones.
//!
//! World space follows ADR 0011: right-handed, `+Z` up, meters and radians.
//! Any clip-space or depth-range conversion is a backend concern.

pub mod camera;
#[allow(
    clippy::expect_used,
    reason = "the conformance suite is test support: failing fast with a message is the point"
)]
pub mod conformance;
mod renderer;
mod world;

pub use camera::Camera;
pub use renderer::{RenderError, Renderer};
pub use world::{
    FrameReport, MaterialData, MaterialId, MeshData, MeshId, MeshInstance, RenderWorld,
};

// Re-exported so facade consumers use the exact math types the facade was
// built against instead of aligning a `glam` version themselves.
pub use glam;
