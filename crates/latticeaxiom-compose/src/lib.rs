//! Stable composition-plane data models.
//!
//! These DTOs are evaluated from Nickel and validated before any package code
//! is loaded. They intentionally contain no Bevy or process-local types.

mod composition;
mod diagnostics;
#[cfg(feature = "nickel-evaluator")]
mod evaluation;
mod graph;
mod imports;
mod observability;
mod registration;
mod semantic;
mod settings;
mod source_closure;
mod versioning;
mod worker_policy;
mod world;

pub use composition::*;
pub use diagnostics::*;
#[cfg(feature = "nickel-evaluator")]
pub use evaluation::*;
pub use graph::*;
pub use imports::*;
pub use observability::*;
pub use registration::*;
pub use semantic::*;
pub use settings::*;
pub use source_closure::*;
pub use versioning::*;
pub use worker_policy::*;
pub use world::*;
