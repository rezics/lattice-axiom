//! Deterministic package resolution, lock persistence, and build planning.
//!
//! Milestone 2 deliberately supports only exact local sources. The package
//! kernel evaluates those sources through `latticeaxiom-compose`, closes the
//! dependency and capability graph, and produces canonical lock and build
//! models whose bytes do not depend on discovery order.

mod build;
mod canonical;
mod kernel;
mod model;

pub use build::{build_plan, write_build_artifacts};
pub use canonical::{
    canonical_descriptor_bytes, canonical_lock_bytes, canonical_plan_bytes, hash_contract_library,
    hash_directory, read_lock, write_descriptor, write_lock,
};
pub use kernel::PackageKernel;
pub use model::{
    BuildJob, BuildJobKind, BuildPlan, CapabilityBinding, LockedBlock, LockedGameGraph,
    LockedPackage, LockedSource, PackageError, PublishedPackageDescriptor,
};
