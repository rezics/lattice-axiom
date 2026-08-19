//! Stable, engine-independent data types for Lattice Axiom boundaries.
//!
//! This crate intentionally does not depend on Bevy. Its identifiers and
//! canonical encoding APIs are suitable for package manifests, locks,
//! persistence envelopes, generated ABI metadata, and process boundaries.

mod canonical;
mod identifier;
mod provenance;

pub use canonical::{
    CanonicalHash, CanonicalHashParseError, CanonicalJsonError, canonical_json_bytes,
    canonical_json_hash,
};
pub use identifier::{
    CapabilityId, IdentifierError, PackageName, PackageVersion, PackageVersionReq, SchemaId,
    SourceId, StableId, TargetTriple, VersionComparator, VersionComparatorOperator, WorldId,
};
pub use provenance::{
    SourceOrigin, SourceOriginKind, SourceProvenance, SourceProvenanceError, SourceSpan,
    provenance_hash,
};
