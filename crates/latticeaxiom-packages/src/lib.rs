//! Deterministic, engine-independent package resolution and local catalog
//! acquisition.
//!
//! The resolver consumes fully evaluated composition and package DTOs. Local
//! catalog check, pack, publish-to-directory, and acquire operate on
//! [`PackageSourceManifestV1`](latticeaxiom_compose::PackageSourceManifestV1)
//! and immutable CAS objects. Neither path has Bevy, network, compiler, or
//! process-global dependencies.

mod cas;
mod catalog;
mod error;
mod host;
mod lock_verify;
mod model;
mod resolver;
mod transaction;

pub use cas::*;
pub use catalog::*;
pub use error::*;
pub use host::*;
pub use lock_verify::*;
pub use model::*;
pub use resolver::*;
pub use transaction::*;
