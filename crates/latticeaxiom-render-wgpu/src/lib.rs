//! wgpu implementation of the Lattice Axiom rendering facade (ADR 0006).
//!
//! This is the only crate in the workspace allowed to depend on `wgpu`
//! (enforced by `deny.toml`). It consumes the same [`RenderWorld`] as the
//! headless implementation and shares its validation logic through the
//! facade, so semantic behavior stays identical across backends.
//!
//! The world is right-handed Z-up (ADR 0011); the conversion to wgpu's
//! clip space and `[0, 1]` depth range happens entirely inside this crate.
//!
//! GPU-dependent correctness (pipelines, presentation) cannot run in headless
//! CI; it is exercised by running the windowed demo locally.
//!
//! [`RenderWorld`]: latticeaxiom_render::RenderWorld

mod renderer;

pub use renderer::WgpuRenderer;
