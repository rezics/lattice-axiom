//! Pure-data world catalog and preflight safety contracts.
//!
//! This crate implements the metadata-only side of ADR 0027 `WORLD-10`
//! through `WORLD-18`. Its source boundary exposes reads only: it cannot open
//! a world writer, load package code, create directories, construct a Bevy
//! world, or execute a migration. Filesystem and storage implementations live
//! outside this crate; [`MemoryWorldSource`] is a deterministic test adapter.

mod catalog;
mod disk;
mod header;
mod identity;
mod lifecycle;
mod migration;
mod preflight;
mod source;
mod trash;

pub use catalog::*;
pub use disk::*;
pub use header::*;
pub use identity::*;
pub use latticeaxiom_core::{CanonicalHash, WorldId};
pub use lifecycle::*;
pub use migration::*;
pub use preflight::*;
pub use source::*;
pub use trash::*;
