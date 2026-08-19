//! In-house voxel meshing for Lattice Axiom.
//!
//! Turns a padded voxel sample volume into per-face quad lists, either as
//! culled unit quads ([`visible_faces`]) or merged rectangles
//! ([`greedy_quads`]). The crate is pure data-to-data: it defines no world
//! data model and depends on no renderer, so it can run in tests, headless
//! CI, and worker threads alike.
//!
//! # Conventions (ADR 0011)
//!
//! - World space is right-handed, Z-up; face directions are named after the
//!   axes (`+X` east, `-X` west, `+Y` north, `-Y` south, `+Z` up, `-Z` down).
//! - Voxel cell `(x, y, z)` spans the half-open unit box starting at that
//!   coordinate; one voxel edge is one meter.
//! - Emitted quad corners wind counter-clockwise seen from outside the
//!   volume, matching the front-face convention of the rendering facade.
//!
//! # Input contract
//!
//! Meshing consumes a *padded* volume: the interior you want meshed plus a
//! one-voxel apron on every side, filled by the caller with the neighboring
//! chunk's samples (or empty voxels at world borders). This keeps neighbor
//! lookups branch-free and makes chunk seams consistent by construction.
//! Output coordinates are interior-local: the apron is subtracted away.
//!
//! Samples are linearized x-fastest: `index = x + sx * (y + sy * z)`; see
//! [`PaddedDims::linearize`].
//!
//! # Example
//!
//! ```
//! use latticeaxiom_voxel_mesh::{greedy_quads, Face, MergeVoxel, PaddedDims, Voxel};
//!
//! #[derive(Clone, Copy)]
//! struct Block(bool);
//! impl Voxel for Block {
//!     fn is_opaque(&self) -> bool {
//!         self.0
//!     }
//! }
//! impl MergeVoxel for Block {
//!     type MergeValue = bool;
//!     fn merge_value(&self) -> bool {
//!         self.0
//!     }
//! }
//!
//! // One solid voxel in a 3x3x3 padded volume (1-voxel apron all around).
//! let dims = PaddedDims::new([3, 3, 3]);
//! let mut voxels = vec![Block(false); dims.volume_len()];
//! voxels[dims.linearize([1, 1, 1])] = Block(true);
//!
//! let quads = greedy_quads(&voxels, dims);
//! assert_eq!(quads.num_quads(), 6);
//!
//! // The up-facing quad lies on the z = 1 plane of interior cell (0, 0, 0).
//! let up = quads.group(Face::PosZ)[0];
//! assert_eq!(Face::PosZ.quad_positions(&up)[0], [0.0, 0.0, 1.0]);
//! ```
//!
//! # Attribution
//!
//! The algorithm structure — padded sample volumes, per-layer face masks,
//! and greedy rectangle growth — references
//! [block-mesh-rs](https://github.com/bonsairobo/block-mesh-rs)
//! (MIT OR Apache-2.0, unmaintained since 2022), reimplemented from scratch
//! natively for this project's Z-up conventions. No code was copied
//! verbatim.

mod geometry;
mod mesher;
mod volume;

pub use geometry::{Face, Quad, QuadBuffer};
pub use mesher::{greedy_quads, visible_faces};
pub use volume::{MAX_PADDED_EDGE, MergeVoxel, PaddedDims, Voxel};
