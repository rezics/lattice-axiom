//! Package-resolution failures and their deterministic diagnostic context.

use std::collections::BTreeSet;
use std::fmt;

use latticeaxiom_compose::{CapabilityCardinality, CompositionError};
use latticeaxiom_core::{
    CanonicalHash, CanonicalJsonError, CapabilityId, PackageName, SourceId, StableId,
};
use thiserror::Error;

use crate::model::{
    BacktrackingFailureV1, CandidateIdentityV1, PreBuildSurface, ResolutionBudget,
    ResolutionLimitsError, ResolutionReceiptError,
};

/// Stable context explaining how a resolver failure was reached.
///
/// Callers should populate all three collections in deterministic order. A
/// package chain runs from the root request to the package or capability that
/// failed. Requested and available entries are concise, presentation-neutral
/// summaries suitable for CLI diagnostics and conformance assertions.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ResolutionFailureContext {
    /// Root-to-failure package chain.
    pub package_chain: Vec<PackageName>,
    /// Normalized requirements that had to be satisfied.
    pub requested: Vec<String>,
    /// Deterministically ordered candidates that were considered.
    pub available: Vec<String>,
}

impl fmt::Display for ResolutionFailureContext {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "package chain {:?}; requested {:?}; available {:?}",
            self.package_chain, self.requested, self.available
        )
    }
}

/// A deterministic local package-resolution failure.
///
/// Selection failures retain a `ResolutionFailureContext` so diagnostics can
/// report the complete package chain and both requested and available rows
/// without repeating resolution or depending on source discovery order.
#[derive(Debug, Error)]
pub enum ResolutionError {
    /// The normalized composition violates its typed model invariants.
    #[error("invalid composition: {source}")]
    InvalidComposition {
        /// Normative composition validation failure.
        #[source]
        source: Box<CompositionError>,
    },

    /// Resolver safety limits are malformed or unsupported.
    #[error("invalid resolution limits: {source}")]
    InvalidLimits {
        /// Limits validation failure.
        #[source]
        source: ResolutionLimitsError,
    },

    /// An evaluated package violates its typed model invariants.
    #[error("invalid package {package}: {source}")]
    InvalidPackage {
        /// Package whose evaluated model was rejected.
        package: PackageName,
        /// Normative package validation failure.
        #[source]
        source: Box<CompositionError>,
    },

    /// A source-universe row and its evaluated package do not describe the
    /// same package, version, source, or content identity.
    #[error("invalid candidate binding {candidate:?}: {reason}; {context}")]
    InvalidCandidate {
        /// Stable identity of the rejected candidate.
        candidate: Box<CandidateIdentityV1>,
        /// Specific binding invariant that was violated.
        reason: String,
        /// Package chain and requested/available candidate summaries.
        context: Box<ResolutionFailureContext>,
    },

    /// The complete source universe is ambiguous, incomplete, or internally
    /// inconsistent before graph traversal begins.
    #[error("invalid source universe: {reason}; {context}")]
    InvalidSourceUniverse {
        /// Source-universe invariant that was violated.
        reason: String,
        /// Requested source rows and available evaluated candidates.
        context: Box<ResolutionFailureContext>,
    },

    /// A deterministic logical-work safety budget would be exceeded.
    ///
    /// This is a global failure and must never be swallowed as an ordinary
    /// candidate rejection during backtracking.
    #[error(
        "resolution budget {budget:?} exceeded: limit {limit}, attempted observation {observed}; {context}"
    )]
    BudgetExceeded {
        /// Exhausted logical-work counter.
        budget: ResolutionBudget,
        /// Configured maximum.
        limit: u64,
        /// Observation that would cross the configured maximum.
        observed: u64,
        /// Deterministic state and requirement context.
        context: Box<ResolutionFailureContext>,
    },

    /// An active normalized search state was reached recursively.
    ///
    /// This is a resolver-state cycle rather than an authored dependency-cycle.
    /// It is a typed dead end for the current branch and may be backtracked;
    /// invalid inputs and exhausted safety budgets remain globally fatal.
    #[error("active resolution state cycle at {state_hash}; {context}")]
    StateCycle {
        /// Canonical hash of normalized package and provider selections.
        state_hash: CanonicalHash,
        /// Deterministic state and requirement context.
        context: Box<ResolutionFailureContext>,
    },

    /// A known dead search state was recovered from the failed-state memo.
    #[error("memoized resolution state failure {failure:?}; {context}")]
    MemoizedStateFailure {
        /// Typed terminal failure cached for the normalized state.
        failure: Box<BacktrackingFailureV1>,
        /// Deterministic state and requirement context.
        context: Box<ResolutionFailureContext>,
    },

    /// No candidate exists for a requested logical package.
    #[error("missing package {package}; {context}")]
    MissingPackage {
        /// Logical package that could not be found.
        package: PackageName,
        /// Requiring package chain, version requests, and visible candidates.
        context: Box<ResolutionFailureContext>,
    },

    /// Candidates exist for a package, but no single version satisfies every
    /// active requirement under the one-version-per-closure policy.
    #[error("version conflict for package {package}; {context}")]
    VersionConflict {
        /// Logical package whose version constraints conflict.
        package: PackageName,
        /// Requiring package chain, normalized ranges, and available versions.
        context: Box<ResolutionFailureContext>,
    },

    /// An activated root, profile, or dependency feature is not declared by
    /// its package.
    #[error("unknown feature {feature} for package {package}; {context}")]
    UnknownFeature {
        /// Package on which the feature was requested.
        package: PackageName,
        /// Unknown package-local feature name.
        feature: String,
        /// Requiring package chain and requested/declared feature summaries.
        context: Box<ResolutionFailureContext>,
    },

    /// Active dependency edges form a cycle.
    #[error("dependency cycle; {context}")]
    DependencyCycle {
        /// Cycle chain, active edge summaries, and already selected packages.
        context: Box<ResolutionFailureContext>,
    },

    /// No selected or selectable package provides a required capability and
    /// compatible interface version.
    #[error("missing capability {capability}; {context}")]
    MissingCapability {
        /// Stable capability contract that could not be provided.
        capability: CapabilityId,
        /// Requiring package chain, capability ranges, and provider candidates.
        context: Box<ResolutionFailureContext>,
    },

    /// The number of compatible providers cannot satisfy the required
    /// capability cardinality.
    #[error(
        "capability {capability} requires {required:?} but has {actual} compatible providers; {context}"
    )]
    CapabilityCardinalityConflict {
        /// Stable capability whose provider count is invalid.
        capability: CapabilityId,
        /// Cardinality required by the active consumers.
        required: CapabilityCardinality,
        /// Number of compatible providers after all filters.
        actual: usize,
        /// Requiring package chain and requested/available provider summaries.
        context: Box<ResolutionFailureContext>,
    },

    /// Multiple otherwise eligible providers remain for an exclusive
    /// capability that requires an explicit provider selection.
    #[error("conflicting providers for capability {capability}; {context}")]
    ConflictingCapabilityProviders {
        /// Exclusive capability with multiple remaining providers.
        capability: CapabilityId,
        /// Requiring package chain and deterministically ordered providers.
        context: Box<ResolutionFailureContext>,
    },
    /// Evaluated intent uses a surface that this pre-build resolver cannot
    /// compile without silently dropping semantics.
    #[error("unsupported pre-build surface {surface:?}; {context}")]
    UnsupportedPreBuildSurface {
        /// Unsupported evaluated surface.
        surface: PreBuildSurface,
        /// Deterministic package and intent context.
        context: Box<ResolutionFailureContext>,
    },
    /// The selected owner-qualified namespace grant chain is incomplete or widens authority.
    #[error("invalid namespace authorization chain: {reason}; {context}")]
    NamespaceAuthorization {
        /// Stable, presentation-neutral authorization failure.
        reason: String,
        /// Selected package chain and relevant grant/request rows.
        context: Box<ResolutionFailureContext>,
    },

    /// No realization satisfies the active features, domains, target, trust,
    /// engine-build, and profile preference.
    #[error("no available realization for package {package}; {context}")]
    RealizationUnavailable {
        /// Package for which no realization could be selected.
        package: PackageName,
        /// Requiring chain, realization preference, and rejected candidates.
        context: Box<ResolutionFailureContext>,
    },

    /// An exact source pin is unavailable in the controlled source universe.
    #[error("pinned source unavailable for package {package}: {required:?}; {context}")]
    PinnedSourceUnavailable {
        /// Package whose exact source must be retained.
        package: PackageName,
        /// One or more exact acceptable source identities.
        required: BTreeSet<SourceId>,
        /// Requiring chain and available source candidates.
        context: Box<ResolutionFailureContext>,
    },

    /// A mandatory dynamic interface requirement cannot be satisfied.
    #[error("required interface {interface} unavailable for package {package}; {context}")]
    InterfaceRequirementUnavailable {
        /// Package selecting the realization.
        package: PackageName,
        /// Mandatory stable interface.
        interface: StableId,
        /// Requiring chain and available interface candidates.
        context: Box<ResolutionFailureContext>,
    },

    /// An engine-coupled realization does not match the exact host build.
    #[error(
        "engine build mismatch for package {package}: required {required}, available {available:?}; {context}"
    )]
    EngineBuildMismatch {
        /// Package selecting the engine-coupled realization.
        package: PackageName,
        /// Exact engine build required by the realization.
        required: CanonicalHash,
        /// Available engine build, or none when the host environment is absent.
        available: Option<CanonicalHash>,
        /// Requiring chain and realization candidates.
        context: Box<ResolutionFailureContext>,
    },

    /// Canonical encoding or hashing of resolution input or output failed.
    #[error("canonical resolution encoding failed: {0}")]
    Canonical(#[from] CanonicalJsonError),

    /// A serialized resolver-stage receipt is malformed, non-canonical,
    /// unsupported, structurally inconsistent, or carries an invalid hash.
    #[error("invalid package-resolution receipt: {source}")]
    InvalidResolutionReceipt {
        /// Receipt decoding, schema, structure, canonicalization, or hash failure.
        #[source]
        source: ResolutionReceiptError,
    },

    /// A well-formed resolver-stage receipt does not exactly match the current
    /// resolution intent or one of its selected source receipts.
    ///
    /// This error does not claim that built artifacts were validated.
    #[error("package-resolution receipt mismatch: {reason}; {context}")]
    ResolutionReceiptMismatch {
        /// Exact selection-replay invariant that did not match current inputs.
        reason: String,
        /// Package chain and requested/receipt identity summaries.
        context: Box<ResolutionFailureContext>,
    },
}
