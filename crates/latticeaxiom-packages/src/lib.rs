//! Deterministic, engine-independent package resolution.
//!
//! The resolver consumes fully evaluated composition and package DTOs. It has
//! no Bevy, network, compiler, or process-global dependencies, so the same
//! implementation can be used by local-directory acquisition and in-memory
//! conformance fixtures.

mod error;
mod model;
mod resolver;

pub use error::*;
pub use model::*;
pub use resolver::*;
