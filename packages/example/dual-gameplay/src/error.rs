//! Fixture-domain failures.

use latticeaxiom_abi::AbiContractError;
use latticeaxiom_core::{
    CanonicalHash, CanonicalJsonError, IdentifierError, SourceProvenanceError,
};
use latticeaxiom_sdk::RegistrationIrError;
use thiserror::Error;

/// A deterministic D1 fixture or safe reference-host failure.
#[derive(Debug, Error)]
pub enum FixtureError {
    /// A fixture-owned stable identifier was malformed.
    #[error(transparent)]
    Identifier(#[from] IdentifierError),
    /// Fixture source provenance was malformed.
    #[error(transparent)]
    Provenance(#[from] SourceProvenanceError),
    /// SDK registration generation failed.
    #[error(transparent)]
    Registration(#[from] RegistrationIrError),
    /// A canonical receipt or snapshot could not be encoded.
    #[error(transparent)]
    Canonical(#[from] CanonicalJsonError),
    /// ABI metadata, batch, or command validation failed closed.
    #[error(transparent)]
    Abi(#[from] AbiContractError),
    /// A platform-sized value could not fit the portable wire width.
    #[error("{field} does not fit the portable u64 wire width")]
    WireSizeOverflow {
        /// Field whose conversion failed.
        field: &'static str,
    },
    /// Entry negotiation found a different generated manifest.
    #[error(
        "LAX-D1-ENTRY-001: registration manifest mismatch: expected {expected}, found {actual}"
    )]
    ManifestMismatch {
        /// Manifest requested by the host.
        expected: CanonicalHash,
        /// Manifest embedded by the module.
        actual: CanonicalHash,
    },
    /// Entry negotiation found a different callback-map artifact.
    #[error("LAX-D1-ENTRY-002: callback map mismatch: expected {expected}, found {actual}")]
    CallbackMapMismatch {
        /// Callback map requested by the host.
        expected: CanonicalHash,
        /// Callback map embedded by the module.
        actual: CanonicalHash,
    },
    /// An operation violated the fixed instance lifecycle.
    #[error("LAX-D1-LIFECYCLE-001: operation `{operation}` is invalid in phase {phase}")]
    InvalidLifecycle {
        /// Rejected lifecycle operation.
        operation: &'static str,
        /// Stable phase display name.
        phase: &'static str,
    },
    /// The SDK boundary caught the injected callback panic.
    #[error("LAX-D1-CALLBACK-001: gameplay callback panicked; instance failed")]
    CallbackPanicked,
    /// The host attempted a callback after quiesce or stop.
    #[error("LAX-D1-CALLBACK-002: callback rejected after instance stopped accepting work")]
    CallbackAfterStop,
    /// Static and portable evidence differed in a normative field.
    #[error("LAX-D1-EQUIVALENCE-001: static and dynamic receipts differ")]
    ReceiptMismatch,
    /// A generated batch did not contain the same row count in every column.
    #[error("LAX-D1-BATCH-001: component columns have different row counts")]
    ColumnCountMismatch,
}
