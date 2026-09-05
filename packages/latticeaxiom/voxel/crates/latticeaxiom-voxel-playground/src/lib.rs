//! Revision-gated D2 adapter from authoritative voxels to `bevy_voxel_world`.
//!
//! The default build contains only the headless-testable projection seam. The
//! optional `presentation-adapter` feature adds the concrete upstream sink and
//! therefore Bevy's rendering dependency closure. Authoritative cells,
//! revisions, DDA, command validation, and persistence stay in the existing
//! Lattice runtime, gameplay, and storage crates.

mod adapter;
mod model;
#[cfg(feature = "presentation-adapter")]
mod upstream;

pub use adapter::{ApplyOutcome, PresentationSink, RevisionGate};
pub use model::{
    PresentationCoordinate, PresentationVoxel, ProjectionBatch, ProjectionError, ProjectionResult,
    ProjectionWrite, VoxelChunkEdge,
};
#[cfg(feature = "presentation-adapter")]
pub use upstream::to_upstream_voxel;
