//! Stable SDK IR and artifact diagnostics.

use latticeaxiom_core::{CanonicalHash, CanonicalJsonError, SchemaId, StableId};
use thiserror::Error;

/// A failure while validating generated registration IR or artifacts.
#[derive(Debug, Error)]
pub enum RegistrationIrError {
    /// A macro-emitted identifier failed canonical parsing during defense in depth.
    #[error("invalid {field} `{value}`: {reason}")]
    InvalidIdentifier {
        /// Stable field name.
        field: &'static str,
        /// Rejected source text.
        value: String,
        /// Canonical parser diagnostic.
        reason: String,
    },
    /// A versioned callback or schema-like identifier omitted its contract major.
    #[error("{field} `{id}` must include a positive @major suffix")]
    MissingContractMajor {
        /// Stable field name.
        field: &'static str,
        /// Rejected identifier.
        id: StableId,
    },
    /// A generated or runtime component omitted its public schema.
    #[error("component `{component}` in mode {mode} requires a versioned public schema")]
    MissingPublicSchema {
        /// Component registration ID.
        component: StableId,
        /// Stable component-mode spelling.
        mode: &'static str,
    },
    /// Two generated rows claimed the same stable registration ID.
    #[error("duplicate generated registration ID `{id}`")]
    DuplicateRegistration {
        /// Duplicated ID.
        id: StableId,
    },
    /// Two systems claimed the same callback key.
    #[error("duplicate generated callback key `{callback}`")]
    DuplicateCallback {
        /// Duplicated callback key.
        callback: StableId,
    },
    /// A system referenced a component that was not explicitly listed.
    #[error("system `{system}` references undeclared component `{component}`")]
    UndeclaredComponent {
        /// System registration ID.
        system: StableId,
        /// Missing component registration ID.
        component: Box<StableId>,
    },
    /// A dual system referenced a package-private component without a public ABI schema.
    #[error("dual system `{system}` references component `{component}` without a public schema")]
    DualComponentWithoutSchema {
        /// System registration ID.
        system: StableId,
        /// Component registration ID.
        component: Box<StableId>,
    },
    /// A system declared incompatible access for the same component.
    #[error("system `{system}` has conflicting access for component `{component}`")]
    ConflictingComponentAccess {
        /// System registration ID.
        system: StableId,
        /// Component registration ID.
        component: Box<StableId>,
    },
    /// A query required and excluded the same component.
    #[error("system `{system}` uses both With and Without for component `{component}`")]
    ConflictingQueryFilter {
        /// System registration ID.
        system: StableId,
        /// Component registration ID.
        component: Box<StableId>,
    },
    /// A system ordering edge targeted itself.
    #[error("system `{system}` has a self ordering edge")]
    SelfOrderingEdge {
        /// System registration ID.
        system: StableId,
    },
    /// Two component declarations disagreed on the Rust API for one schema.
    #[error("schema `{schema}` has inconsistent Rust API fingerprints")]
    ConflictingSchemaFingerprint {
        /// Conflicting public schema.
        schema: SchemaId,
    },
    /// Provenance for one generated row is absent.
    #[error("missing SourceProvenance for generated row `{id}`")]
    MissingProvenance {
        /// Row requiring provenance.
        id: StableId,
    },
    /// Provenance was supplied for a row not emitted by this IR.
    #[error("SourceProvenance contains unknown generated row `{id}`")]
    UnknownProvenance {
        /// Unknown row ID.
        id: StableId,
    },
    /// A system source parameter list exceeded the portable ordinal range.
    #[error("system `{system}` has too many parameters for the v1 signature")]
    TooManyParameters {
        /// System registration ID.
        system: StableId,
    },
    /// A producer identity omitted its required contract major.
    #[error("producer field {field} `{id}` must include a positive @major suffix")]
    InvalidProducerContract {
        /// Stable producer field.
        field: &'static str,
        /// Rejected identifier.
        id: StableId,
    },
    /// A generated artifact used an unsupported schema version.
    #[error("unsupported generated artifact schema {found}")]
    UnsupportedArtifactSchema {
        /// Encountered schema version.
        found: u32,
    },
    /// A generated artifact content address did not match its payload.
    #[error("{artifact} hash mismatch: expected {expected}, recomputed {actual}")]
    ArtifactHashMismatch {
        /// Stable artifact name.
        artifact: &'static str,
        /// Claimed content address.
        expected: CanonicalHash,
        /// Recomputed content address.
        actual: CanonicalHash,
    },
    /// The canonical registration manifest failed its own hash verification.
    #[error("generated registration manifest failed verification: {reason}")]
    ManifestHashMismatch {
        /// Underlying manifest diagnostic.
        reason: String,
    },
    /// Callback and system artifacts disagree on one signature.
    #[error("callback `{callback}` does not match system `{system}` signature")]
    CallbackSignatureMismatch {
        /// Callback key.
        callback: StableId,
        /// System registration ID.
        system: Box<StableId>,
    },
    /// Canonical JSON encoding failed.
    #[error(transparent)]
    Canonical(#[from] CanonicalJsonError),
}

impl RegistrationIrError {
    /// Returns the stable diagnostic code used by golden and fault fixtures.
    #[must_use]
    pub const fn code(&self) -> &'static str {
        match self {
            Self::InvalidIdentifier { .. } => "LAX-SDK-001",
            Self::MissingContractMajor { .. } => "LAX-SDK-002",
            Self::MissingPublicSchema { .. } => "LAX-SDK-003",
            Self::DuplicateRegistration { .. } => "LAX-SDK-004",
            Self::DuplicateCallback { .. } => "LAX-SDK-005",
            Self::UndeclaredComponent { .. } => "LAX-SDK-006",
            Self::DualComponentWithoutSchema { .. } => "LAX-SDK-007",
            Self::ConflictingComponentAccess { .. } => "LAX-SDK-008",
            Self::ConflictingQueryFilter { .. } => "LAX-SDK-009",
            Self::SelfOrderingEdge { .. } => "LAX-SDK-010",
            Self::ConflictingSchemaFingerprint { .. } => "LAX-SDK-011",
            Self::MissingProvenance { .. } => "LAX-SDK-012",
            Self::UnknownProvenance { .. } => "LAX-SDK-013",
            Self::TooManyParameters { .. } => "LAX-SDK-014",
            Self::InvalidProducerContract { .. } => "LAX-SDK-015",
            Self::UnsupportedArtifactSchema { .. } => "LAX-SDK-016",
            Self::ArtifactHashMismatch { .. } => "LAX-SDK-017",
            Self::ManifestHashMismatch { .. } => "LAX-SDK-018",
            Self::CallbackSignatureMismatch { .. } => "LAX-SDK-019",
            Self::Canonical(_) => "LAX-SDK-020",
        }
    }
}
