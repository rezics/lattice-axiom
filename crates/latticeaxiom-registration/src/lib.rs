//! Deterministic compilation of frozen package registrations.

// Compile failures retain typed IDs and structured closure evidence. Their size
// is paid only on the fail-fast activation path; boxing every helper result
// would add allocation and conversion noise without shrinking successful work.
#![allow(clippy::result_large_err)]
//!
//! This crate validates composition, lock, manifest, ownership, schema,
//! callback, schedule, and semantic boundaries before package code is loaded.
//! It deliberately contains no Bevy types and does not resolve packages.

mod compiler;
mod error;
mod model;
mod schedule;
mod semantics;

pub use compiler::{RegistrationCompiler, RegistrationCompilerLimits};
pub use error::RegistrationCompileError;
pub use model::*;
