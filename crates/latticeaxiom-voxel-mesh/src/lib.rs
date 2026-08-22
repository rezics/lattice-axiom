//! Deterministic, CPU-only voxel mesh derivation.
//!
//! This crate turns an immutable chunk sample plus a one-voxel halo into
//! renderer-independent quad data. It does not own authoritative chunks,
//! Bevy entities, renderer resources, tasks, or process handles. The caller
//! owns all of those concerns and must compare the returned [`MeshReceipt`]
//! with current authoritative state before applying a result.
//!
//! # Coordinates
//!
//! Coordinates follow the project's native right-handed Y-up convention:
//! `+X` is right, `+Y` is up, and conventional forward is `-Z`. Voxel and
//! chunk coordinates are always ordered `(x, y, z)`. Mesh positions are
//! chunk-interior-local meters; negative world chunk coordinates remain in
//! the source receipt and are never converted through floating point.
//!
//! # Input and output
//!
//! [`PaddedChunk`] describes the meshable interior plus exactly one sample on
//! every side. A source face is emitted unless the adjacent halo/interior
//! face occludes it. Output is grouped first by the stable [`MeshGroup`]
//! order and then by [`Face`] order. No hash-table iteration participates in
//! output ordering.

mod geometry;
mod mesher;
mod source;
mod volume;

pub use geometry::{Aabb, Face, LayerMergeKey, MeshAlphaMode, MeshBuffer, MeshGroup, Quad};
pub use mesher::{
    ChunkMesh, GreedyMesher, MeshError, greedy_quads, visible_faces, visible_faces_into,
};
pub use source::{
    ChunkCoordinate, MeshReceipt, MeshSource, SourceEpoch, SourceFingerprint, SourceRevision,
};
pub use volume::{
    FaceDescriptor, FaceOcclusion, MAX_INTERIOR_EDGE, ONE_VOXEL_HALO, PaddedChunk,
    PaddedChunkError, Voxel,
};
