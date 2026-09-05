//! Stable composition-plane data models.
//!
//! These DTOs are evaluated from Nickel and validated before any package code
//! is loaded. They intentionally contain no Bevy or process-local types.

mod bootstrap;
#[cfg(feature = "nickel-evaluator")]
mod cli;
mod composition;
#[cfg(feature = "nickel-evaluator")]
mod controller;
mod diagnostics;
#[cfg(feature = "nickel-evaluator")]
mod evaluation;
mod evaluator_protocol;
mod graph;
mod immutable_file;
mod imports;
#[cfg(feature = "nickel-evaluator")]
mod native_static_product;
mod observability;
mod product_lock;
mod realized_data;
mod registration;
mod semantic;
mod settings;
mod source_closure;
mod supervisor;
#[cfg(feature = "nickel-evaluator")]
mod trusted_staging;
mod versioning;
#[cfg(feature = "nickel-evaluator")]
mod worker;
mod worker_policy;
mod world;

pub use bootstrap::*;
#[cfg(feature = "nickel-evaluator")]
pub use cli::*;
pub use composition::*;
#[cfg(feature = "nickel-evaluator")]
pub use controller::*;
pub use diagnostics::*;
#[cfg(feature = "nickel-evaluator")]
pub use evaluation::*;
pub use evaluator_protocol::*;
pub use graph::*;
pub use immutable_file::{ImmutableFileError, publish_immutable_file};
pub use imports::*;
#[cfg(feature = "nickel-evaluator")]
pub use native_static_product::*;
pub use observability::*;
pub use product_lock::*;
pub use realized_data::*;
pub use registration::*;
pub use semantic::*;
pub use settings::*;
pub use source_closure::*;
pub use supervisor::*;
#[cfg(feature = "nickel-evaluator")]
pub use trusted_staging::*;
pub use versioning::*;
#[cfg(feature = "nickel-evaluator")]
pub use worker::*;
pub use worker_policy::*;
pub use world::*;
