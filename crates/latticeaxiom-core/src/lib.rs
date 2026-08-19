//! Core world model and simulation contracts for Lattice Axiom.
//!
//! This crate owns semantics that every other crate may rely on without
//! pulling in GPU, windowing, or storage backends: at milestone 1 that is the
//! canonical spatial conventions of ADR 0011 (right-handed, Z-up world space).
//! Chunks, palettes, registries, and the simulation loop join in later
//! milestones.
//!
//! This crate must never depend on `wgpu`, `winit`, or `rocksdb`; that
//! boundary is enforced by `deny.toml` at the workspace root.

pub mod space;
