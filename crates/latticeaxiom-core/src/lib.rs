//! Core world model and simulation contracts for Lattice Axiom.
//!
//! This crate owns semantics that every other crate may rely on without
//! pulling in GPU, windowing, or storage backends: at milestone 1 that is the
//! canonical spatial conventions of ADR 0011 (right-handed, Z-up world space).
//! Milestone 3 adds fixed chunks, deterministic terrain, voxel selection,
//! edits, and minimal player physics while keeping storage and rendering out
//! of the domain model.
//!
//! This crate must never depend on `wgpu`, `winit`, or `rocksdb`; that
//! boundary is enforced by `deny.toml` at the workspace root.

pub mod block;
pub mod chunk;
pub mod collision;
pub mod interaction;
pub mod physics;
pub mod raycast;
pub mod space;
pub mod terrain;

pub use block::BlockId;
pub use chunk::{
    CHUNK_EDGE, CHUNK_VOLUME, Chunk, ChunkAddress, ChunkDataError, ChunkPos, LocalBlockPos,
    LocalBlockPosError,
};
pub use collision::{Aabb, AabbError};
pub use interaction::{BlockEdit, BlockEditError, break_block, place_block};
pub use physics::{PhysicsConfig, PhysicsError, PlayerBody, step_player};
pub use raycast::{FaceNormal, RaycastHit, raycast_voxels};
pub use space::BlockPos;
pub use terrain::{Heightmap, HeightmapError, TerrainBlocks};
