//! Stable composition-plane data models.
//!
//! These DTOs are evaluated from Nickel and validated before any package code
//! is loaded. They intentionally contain no Bevy or process-local types.

mod composition;
mod graph;
mod observability;
mod registration;
mod semantic;
mod settings;
mod world;

pub use composition::*;
pub use graph::*;
pub use observability::*;
pub use registration::*;
pub use semantic::*;
pub use settings::*;
pub use world::*;
