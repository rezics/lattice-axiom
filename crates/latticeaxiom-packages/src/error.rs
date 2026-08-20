//! Package-resolution and content-addressed store failures.

use std::collections::BTreeSet;
use std::fmt;
use std::io;
use std::path::PathBuf;

use latticeaxiom_compose::{
    BootstrapManifestError, CapabilityCardinality, CompositionError, ProductLockError,
    SourceScanError,
};
use latticeaxiom_core::{
    CanonicalHash, CanonicalJsonError, CanonicalLogicalPath, CanonicalLogicalPathError,
    CapabilityId, IdentifierError, PackageName, PackageVersion, SourceId, SourceProvenanceError,
    StableId,
};
use thiserror::Error;

use crate::cas::CasObjectId;
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

/// A failure from the immutable content-addressed object store.
///
/// Missing objects fail closed. The store never reconstructs bytes from an
/// acquisition path.
#[derive(Debug, Error)]
pub enum CasError {
    /// No object exists for the requested kind-qualified digest.
    #[error("missing CAS object {id}")]
    MissingObject {
        /// Requested object identity.
        id: CasObjectId,
    },

    /// Stored bytes do not match the digest in the requested identity.
    #[error("CAS object {id} digest mismatch: stored digest {actual}")]
    DigestMismatch {
        /// Requested object identity.
        id: CasObjectId,
        /// Digest recomputed from the stored payload.
        actual: CanonicalHash,
    },

    /// An existing object at this identity has different bytes.
    ///
    /// The store is append-only: a digest may not be overwritten.
    #[error("CAS object {id} already exists with different bytes")]
    DigestCollision {
        /// Object identity that would have been overwritten.
        id: CasObjectId,
    },

    /// Filesystem I/O failed while reading or publishing an object.
    #[error("CAS store I/O failed at {path}: {source}")]
    Io {
        /// Store path that could not be read or written.
        path: PathBuf,
        /// Underlying I/O failure.
        #[source]
        source: io::Error,
    },
}

impl CasError {
    pub(crate) fn io(path: impl Into<PathBuf>, source: io::Error) -> Self {
        Self::Io {
            path: path.into(),
            source,
        }
    }
}

/// A local catalog check, pack, publish, or acquire failure.
///
/// Catalog operations never execute install or build scripts and never
/// consult the network. Missing CAS objects fail closed and are not rebuilt
/// from an acquisition path.
#[derive(Debug, Error)]
pub enum CatalogError {
    /// The package root or catalog directory could not be read or written.
    #[error("local catalog I/O failed at {path}: {source}")]
    Io {
        /// Path that could not be read or written.
        path: PathBuf,
        /// Underlying I/O failure.
        #[source]
        source: io::Error,
    },

    /// The package source manifest is missing, malformed, or forbidden.
    #[error("invalid package source manifest: {source}")]
    InvalidManifest {
        /// Restricted-TOML parse or validation failure.
        #[from]
        source: BootstrapManifestError,
    },

    /// Authorized-root scanning rejected the package tree.
    #[error("package source scan failed: {source}")]
    SourceScan {
        /// Source-table scan failure.
        #[from]
        source: SourceScanError,
    },

    /// A content-addressed store operation failed.
    #[error(transparent)]
    Cas(#[from] CasError),

    /// Canonical JSON encoding of catalog metadata or a packed object failed.
    #[error("canonical catalog encoding failed: {0}")]
    Canonical(#[from] CanonicalJsonError),

    /// A local-catalog source identity could not be constructed.
    #[error("invalid local-catalog source ID `{value}`: {source}")]
    InvalidSourceId {
        /// Rejected source identity text.
        value: String,
        /// Stable-identifier grammar failure.
        #[source]
        source: IdentifierError,
    },

    /// The package root has no `latticeaxiom-package.toml`.
    #[error("package source manifest is missing at {path}")]
    MissingManifest {
        /// Expected manifest path.
        path: PathBuf,
    },

    /// The acquisition locator is not a directory.
    #[error("package root is not a directory: {path}")]
    RootNotDirectory {
        /// Rejected acquisition path.
        path: PathBuf,
    },

    /// An explicit source-inclusion path is absent from the package root.
    #[error("package {package} is missing included source path `{path}`")]
    MissingIncludedPath {
        /// Package that declared the inclusion.
        package: PackageName,
        /// Missing root-relative inclusion path.
        path: CanonicalLogicalPath,
    },

    /// A public Nickel entrypoint is absent after inclusion filtering.
    #[error("package {package} is missing Nickel entrypoint `{path}`")]
    MissingNickelEntrypoint {
        /// Package that declared the entrypoint.
        package: PackageName,
        /// Missing entrypoint path.
        path: CanonicalLogicalPath,
    },

    /// Inclusion filtering removed every source file.
    #[error("package {package} has no source files after inclusion filtering")]
    EmptyIncludedSnapshot {
        /// Package whose filtered snapshot was empty.
        package: PackageName,
    },

    /// The same package name and version was published with a different source digest.
    #[error(
        "package {package} version {version} is already published with source digest {existing}; cannot publish {published}"
    )]
    DuplicatePublishedSource {
        /// Logical package identity.
        package: PackageName,
        /// Exact published version.
        version: Box<PackageVersion>,
        /// Source digest already recorded in the catalog.
        existing: CanonicalHash,
        /// Conflicting source digest that was not published.
        published: CanonicalHash,
    },

    /// An exact catalog identity is absent.
    #[error("catalog has no package {package} version {version}")]
    MissingCatalogEntry {
        /// Requested package.
        package: PackageName,
        /// Requested exact version.
        version: PackageVersion,
    },

    /// The catalog directory has no index.
    #[error("local catalog index is missing at {path}")]
    MissingCatalogIndex {
        /// Expected index path.
        path: PathBuf,
    },

    /// The catalog directory has no object store.
    #[error("local catalog object store is missing at {path}")]
    MissingObjectStore {
        /// Expected CAS directory.
        path: PathBuf,
    },

    /// The catalog index schema is unsupported.
    #[error("unsupported local catalog schema {found}; supported schema is {supported}")]
    UnsupportedCatalogSchema {
        /// Version found in the index.
        found: u32,
        /// Version implemented by this crate.
        supported: u32,
    },

    /// Catalog index bytes are not the canonical encoding of the validated DTO.
    #[error("local catalog index is not canonical")]
    NonCanonicalIndex,

    /// Catalog index structure is incomplete or internally inconsistent.
    #[error("invalid local catalog index: {reason}")]
    InvalidCatalogIndex {
        /// Presentation-neutral structural diagnostic.
        reason: String,
    },

    /// Stored manifest bytes do not match the catalogued digest.
    #[error(
        "catalog manifest digest mismatch for {package} {version}: expected {expected}, actual {actual}"
    )]
    ManifestDigestMismatch {
        /// Catalogued package.
        package: PackageName,
        /// Catalogued version.
        version: Box<PackageVersion>,
        /// Digest recorded by the index.
        expected: CanonicalHash,
        /// Digest recomputed from CAS bytes.
        actual: CanonicalHash,
    },

    /// Stored source-tree bytes do not match the catalogued source digest.
    #[error(
        "catalog source digest mismatch for {package} {version}: expected {expected}, actual {actual}"
    )]
    SourceDigestMismatch {
        /// Catalogued package.
        package: PackageName,
        /// Catalogued version.
        version: Box<PackageVersion>,
        /// Digest recorded by the index.
        expected: CanonicalHash,
        /// Digest recomputed from the source snapshot.
        actual: CanonicalHash,
    },

    /// Packed or acquired package identity does not match the catalog row.
    #[error(
        "catalog package identity mismatch: catalog {catalog_package} {catalog_version}, object {object_package} {object_version}"
    )]
    PackageIdentityMismatch {
        /// Package recorded by the catalog row.
        catalog_package: PackageName,
        /// Version recorded by the catalog row.
        catalog_version: Box<PackageVersion>,
        /// Package parsed from the stored object.
        object_package: PackageName,
        /// Version parsed from the stored object.
        object_version: Box<PackageVersion>,
    },

    /// A catalog object locator used an unknown CAS kind token.
    #[error("unknown catalog object kind `{kind}`")]
    UnknownObjectKind {
        /// Rejected kind token.
        kind: String,
    },

    /// A catalog object locator pointed at the wrong CAS kind.
    #[error("catalog object {locator} has kind {actual}, expected {expected}")]
    UnexpectedObjectKind {
        /// Object locator recorded by the catalog.
        locator: String,
        /// Kind required by this catalog field.
        expected: String,
        /// Kind present on the locator.
        actual: String,
    },

    /// CAS payload bytes could not be decoded as the expected packed object.
    #[error("catalog object {locator} is not a valid packed payload: {reason}")]
    InvalidObjectPayload {
        /// Object locator that failed to decode.
        locator: String,
        /// Decoder diagnostic.
        reason: String,
    },
}

impl CatalogError {
    pub(crate) fn io(path: impl Into<PathBuf>, source: io::Error) -> Self {
        Self::Io {
            path: path.into(),
            source,
        }
    }
}

/// A bootstrap-first check/pack/publish/acquire/resolve/lock transaction failure.
///
/// This error does not replace [`ResolutionError`], [`CatalogError`], or
/// [`ProductLockError`]. It names the composed offline transaction that
/// invoked those existing APIs.
#[derive(Debug, Error)]
pub enum TransactionError {
    /// The bootstrap file could not be read.
    #[error("bootstrap I/O failed at {path}: {source}")]
    Io {
        /// Path that could not be read or written.
        path: PathBuf,
        /// Underlying I/O failure.
        #[source]
        source: io::Error,
    },
    /// The composition bootstrap is missing, malformed, or forbidden.
    #[error("invalid composition bootstrap: {source}")]
    Bootstrap {
        /// Restricted-TOML parse or validation failure.
        #[from]
        source: BootstrapManifestError,
    },
    /// A local catalog check, pack, publish, or acquire step failed.
    #[error(transparent)]
    Catalog(#[from] CatalogError),
    /// A content-addressed store operation failed.
    #[error(transparent)]
    Cas(#[from] CasError),
    /// Deterministic package resolution failed.
    #[error(transparent)]
    Resolution(#[from] ResolutionError),
    /// Product-lock seal, persist, reopen, or frozen verification failed.
    #[error(transparent)]
    ProductLock(#[from] ProductLockError),
    /// Canonical JSON encoding of a transaction DTO failed.
    #[error(transparent)]
    Canonical(#[from] CanonicalJsonError),
    /// An evaluated package built from a source manifest is invalid.
    #[error("invalid package {package}: {source}")]
    InvalidPackage {
        /// Package whose static projection could not become a `PackageSpec`.
        package: PackageName,
        /// Package-model validation failure.
        #[source]
        source: Box<CompositionError>,
    },
    /// A bootstrap source locator cannot be acquired by this offline transaction.
    #[error(
        "bootstrap source for {package} is {kind}; offline lock requires path, local-catalog, or fixture path locators"
    )]
    UnsupportedSourceLocator {
        /// Package declared by the bootstrap.
        package: PackageName,
        /// Bootstrap source-provider kind.
        kind: &'static str,
    },
    /// A bootstrap Path or Fixture locator does not match the packed package identity.
    #[error(
        "bootstrap package {bootstrap} does not match packed package {packed} version {version}"
    )]
    BootstrapPackageMismatch {
        /// Logical package named by the bootstrap.
        bootstrap: PackageName,
        /// Package identity packed from the locator.
        packed: PackageName,
        /// Packed exact version.
        version: PackageVersion,
    },
    /// A bootstrap Path package was not present in the published catalog.
    #[error("catalog has no published version of package {package}")]
    MissingPublishedPackage {
        /// Logical package missing from the catalog.
        package: PackageName,
    },
    /// A stable identifier required by the transaction is malformed.
    #[error("invalid transaction identifier `{value}`: {source}")]
    InvalidIdentifier {
        /// Rejected identifier text.
        value: String,
        /// Stable-identifier grammar failure.
        #[source]
        source: IdentifierError,
    },
    /// A diagnostic or provenance path is not a canonical logical path.
    #[error("invalid transaction path `{value}`: {source}")]
    InvalidPath {
        /// Rejected path text.
        value: String,
        /// Logical-path grammar failure.
        #[source]
        source: CanonicalLogicalPathError,
    },
    /// Package or composition provenance could not be constructed.
    #[error("invalid transaction provenance: {source}")]
    InvalidProvenance {
        /// Provenance construction failure.
        #[from]
        source: SourceProvenanceError,
    },
}

impl TransactionError {
    pub(crate) fn io(path: impl Into<PathBuf>, source: io::Error) -> Self {
        Self::Io {
            path: path.into(),
            source,
        }
    }
}
