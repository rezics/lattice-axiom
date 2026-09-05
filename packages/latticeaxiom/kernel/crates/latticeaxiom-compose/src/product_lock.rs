//! Final `latticeaxiom.lock` persistence and locked/offline/frozen verification.
//!
//! [`LockV1`] is the sealed product lock. It is not `ResolutionReceiptV1` or
//! `BuildIntentV1`; those remain intermediate receipts whose hashes may be
//! recorded here. Candidate graphs must not be launched until this lock has
//! been atomically written, reopened, and verified.

use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use latticeaxiom_core::{
    CanonicalHash, CanonicalJsonError, CanonicalLogicalPath, CapabilityId, PackageName,
    PackageVersion, SourceId, StableId, TargetTriple, canonical_json_bytes, canonical_json_hash,
};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{
    BootstrapSourceProviderV1, CompositionBootstrapV1, GraphHashError, LockedGameGraph,
    LockedPackage, NickelEvaluationLimits, PackageAlias, PackageDomain, ProfileKind, RealizationId,
    RealizationKind, RegistrationImage, RuntimeImage,
};

/// Schema version for [`LockV1`] / `latticeaxiom.lock`.
pub const PRODUCT_LOCK_SCHEMA_VERSION: u32 = 1;

/// Default file name of the final product lock.
pub const PRODUCT_LOCK_FILE_NAME: &str = "latticeaxiom.lock";

/// Immutable catalog location for a previously selected product lock.
#[must_use]
pub fn archived_product_lock_path(catalog: &Path, hash: CanonicalHash) -> PathBuf {
    catalog.join("locks").join(format!("{hash}.lock"))
}

/// Retains a verified lock so existing worlds can keep their original closure.
///
/// # Errors
/// Returns [`ProductLockError`] if publication fails or an existing archive
/// disagrees with the same lock identity. Existing archives are never replaced.
pub fn archive_product_lock(catalog: &Path, lock: &LockV1) -> Result<PathBuf, ProductLockError> {
    let path = archived_product_lock_path(catalog, lock.product_lock_hash);
    let mode = if path.exists() {
        LockActionMode::Locked
    } else {
        LockActionMode::Offline
    };
    persist_product_lock(&path, lock, mode)?;
    Ok(path)
}

/// Machine identifier recorded as the lock producer.
pub const PRODUCT_LOCK_PRODUCER_MACHINE: &str = "latticeaxiom";

const TEMPORARY_LOCK_SUFFIX: &str = ".tmp";
const BACKUP_LOCK_SUFFIX: &str = ".bak";

/// Producer identity sealed into a product lock.
///
/// The toolchain field is a content hash, not a filesystem path. This DTO does
/// not carry timestamps, absolute paths, or process-local handles.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProductLockProducerV1 {
    /// Machine identifier. Must be `latticeaxiom`.
    pub machine: String,
    /// Content identity of the host toolchain that produced this lock.
    pub toolchain: CanonicalHash,
}

/// Bounded local source class copied from a bootstrap manifest.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum BootstrapSourceKindV1 {
    /// A workspace package.
    Workspace,
    /// A root-manifest-relative path package.
    Path,
    /// A local catalog package.
    LocalCatalog,
    /// An explicit in-memory or filesystem fixture.
    Fixture,
}

/// Bootstrap manifest receipt sealed into the product lock.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BootstrapLockReceiptV1 {
    /// Canonical digest of the validated [`CompositionBootstrapV1`] DTO.
    pub manifest_digest: CanonicalHash,
    /// Domain projection selected by the bootstrap.
    pub projection: ProfileKind,
    /// Versioned Nickel evaluation policy authorized by the bootstrap.
    pub evaluation_policy: StableId,
    /// Root-relative Nickel profile entry. This is not an absolute path.
    pub nickel_profile_entry: CanonicalLogicalPath,
    /// Declared local source class for each bootstrap package.
    pub source_kinds: BTreeMap<PackageName, BootstrapSourceKindV1>,
}

/// One sealed direct-dependency alias edge.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct LockedAliasEdgeV1 {
    /// Exact dependency package selected for this alias.
    pub package: PackageName,
    /// Exact selected version.
    pub version: PackageVersion,
    /// Features forwarded across this edge.
    pub features: BTreeSet<String>,
}

/// Portable exact package identity independent of target realization.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PortableLockedPackageV1 {
    /// Exact selected version.
    pub version: PackageVersion,
    /// Stable source-universe identity.
    pub source_id: SourceId,
    /// Runnable source identity. This is not an acquisition path.
    pub source_digest: CanonicalHash,
    /// CAS payload digest of the stored source-tree object.
    pub source_object_digest: CanonicalHash,
    /// Canonical package-manifest digest.
    pub manifest_digest: CanonicalHash,
    /// Acquisition provenance receipt.
    pub provenance_hash: CanonicalHash,
    /// Activated package features.
    pub features: BTreeSet<String>,
    /// Activated package domains.
    pub domains: BTreeSet<PackageDomain>,
}

/// Portable package graph sealed independently of target artifacts.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PortableResolutionLockV1 {
    /// Hash of the intermediate `ResolutionReceiptV1`. Not a product lock.
    pub resolution_receipt_hash: CanonicalHash,
    /// Semantic selected-graph hash reused from [`LockedGameGraph`].
    pub graph_hash: CanonicalHash,
    /// Exact package instances keyed by logical name.
    pub packages: BTreeMap<PackageName, PortableLockedPackageV1>,
    /// Package-local Nickel import alias edges.
    pub alias_edges: BTreeMap<PackageName, BTreeMap<PackageAlias, LockedAliasEdgeV1>>,
    /// Feature selections keyed by package.
    pub features: BTreeMap<PackageName, BTreeSet<String>>,
    /// Domain selections keyed by package.
    pub domains: BTreeMap<PackageName, BTreeSet<PackageDomain>>,
    /// Capability providers in deterministic execution order.
    pub capabilities: BTreeMap<CapabilityId, Vec<PackageName>>,
    /// Root package names.
    pub roots: BTreeSet<PackageName>,
    /// Evaluation policy used to produce the composition.
    pub evaluation_policy: StableId,
    /// Exact effective evaluator limits.
    pub evaluation_limits: NickelEvaluationLimits,
}

/// Composition and evaluation receipts reused from the locked graph.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CompositionLockReceiptV1 {
    /// Semantic composition hash.
    pub composition_hash: CanonicalHash,
    /// Composition provenance hash.
    pub composition_provenance_hash: CanonicalHash,
    /// Hash of the evaluation-policy receipt.
    pub evaluation_policy_receipt_hash: CanonicalHash,
}

/// Registration-image and frozen-graph receipts.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RegistrationLockReceiptV1 {
    /// Semantic graph hash reused from [`LockedGameGraph`].
    pub graph_hash: CanonicalHash,
    /// Exact graph lock hash reused from [`LockedGameGraph`].
    pub graph_lock_hash: CanonicalHash,
    /// Closure-wide registration image hash.
    pub image_hash: CanonicalHash,
    /// Semantic registration hash excluding producer provenance.
    pub semantic_hash: CanonicalHash,
}

/// One exact package realization for a single target.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TargetPackageRealizationV1 {
    /// Package-local realization identity.
    pub realization_id: RealizationId,
    /// Realization family.
    pub kind: RealizationKind,
    /// Verified artifact digest.
    pub artifact_digest: CanonicalHash,
    /// Exact engine build required by an engine-coupled realization.
    pub engine_build_id: Option<CanonicalHash>,
    /// Registration manifest identity for this realization.
    pub registration_hash: CanonicalHash,
}

/// Target-specific realization receipts layered on portable resolution.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TargetRealizationLockV1 {
    /// Projection that selected this realization set.
    pub projection: ProfileKind,
    /// Exact build target.
    pub target: TargetTriple,
    /// Content identity of the toolchain used to realize artifacts.
    pub toolchain: CanonicalHash,
    /// Hash of the intermediate `BuildIntentV1`. Not a product lock.
    pub build_intent_hash: CanonicalHash,
    /// Exact package realizations for this target.
    pub packages: BTreeMap<PackageName, TargetPackageRealizationV1>,
    /// Host engine build when any realization is engine-coupled.
    pub engine_build_id: Option<CanonicalHash>,
    /// Registration image hash consumed by this realization.
    pub registration_image_hash: CanonicalHash,
    /// [`RuntimeImage`] input fingerprint.
    pub runtime_image_fingerprint: CanonicalHash,
}

/// Inputs required to seal a final [`LockV1`].
#[derive(Clone, Debug)]
pub struct ProductLockDraftV1 {
    /// Producer and host toolchain identity.
    pub producer: ProductLockProducerV1,
    /// Validated bootstrap that authorized acquisition.
    pub bootstrap: CompositionBootstrapV1,
    /// Exact frozen graph whose hashes are reused, not replaced.
    pub graph: LockedGameGraph,
    /// Package-local alias edges for Nickel import.
    pub alias_edges: BTreeMap<PackageName, BTreeMap<PackageAlias, LockedAliasEdgeV1>>,
    /// Intermediate resolution-receipt hash. Not lock identity.
    pub resolution_receipt_hash: CanonicalHash,
    /// Evaluation-policy receipt hash.
    pub evaluation_policy_receipt_hash: CanonicalHash,
    /// Activated features keyed by package. Missing rows are empty sets.
    pub package_features: BTreeMap<PackageName, BTreeSet<String>>,
    /// CAS payload digests for each package source-tree object.
    pub source_objects: BTreeMap<PackageName, CanonicalHash>,
    /// Target realizations keyed by triple.
    pub realizations: BTreeMap<TargetTriple, TargetRealizationLockV1>,
    /// Closure-wide registration image.
    pub registration_image: RegistrationImage,
    /// Semantic registration hash excluding producer provenance.
    pub registration_semantic_hash: CanonicalHash,
    /// Activation-ready runtime image whose fingerprint is sealed.
    pub runtime_image: RuntimeImage,
}

/// Final product lock persisted as `latticeaxiom.lock`.
///
/// Portable resolution and target realization are layered so the same package
/// graph can carry Windows, Linux, client, or headless receipts without
/// putting platform paths into the logical graph.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct LockV1 {
    /// Product-lock schema version.
    pub schema_version: u32,
    /// Producer and toolchain identity.
    pub producer: ProductLockProducerV1,
    /// Bootstrap manifest receipt.
    pub bootstrap: BootstrapLockReceiptV1,
    /// Portable exact package resolution.
    pub portable_resolution: PortableResolutionLockV1,
    /// Composition and evaluation receipts.
    pub composition: CompositionLockReceiptV1,
    /// Registration and frozen-graph receipts.
    pub registration: RegistrationLockReceiptV1,
    /// Target realizations keyed by triple.
    pub realizations: BTreeMap<TargetTriple, TargetRealizationLockV1>,
    /// Canonical identity of the resolution explanation.
    pub explanation_hash: CanonicalHash,
    /// Exact product-lock hash covering every sealed field except itself.
    pub product_lock_hash: CanonicalHash,
}

/// Action semantics from ADR 0032 section 7.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LockActionMode {
    /// Atomically persist a newly sealed final lock.
    Persist,
    /// Existing lock bytes must remain unchanged.
    Locked,
    /// Forbid network providers. Local catalog, path, and CAS remain usable.
    Offline,
    /// Locked, offline, read-only, and exact-artifact. Missing or mismatched
    /// receipts fail with no path fallback.
    Frozen,
}

/// Kind of independently verified lock receipt.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProductLockReceiptKind {
    /// Package source-manifest digest.
    Manifest,
    /// Runnable source digest or source-tree object.
    Source,
    /// Host or realization toolchain identity.
    Toolchain,
    /// Exact `EngineBuildId`.
    EngineBuild,
    /// Realized artifact digest.
    Artifact,
    /// Registration image hash.
    Registration,
    /// Runtime-image input fingerprint.
    RuntimeImage,
    /// Direct-dependency alias edge.
    Alias,
}

/// Exact object bytes used to verify lock receipts independently of the
/// top-level product-lock hash. Keys are CAS payload digests.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ProductLockObjects {
    /// Package-manifest objects keyed by payload digest.
    pub manifests: BTreeMap<CanonicalHash, Vec<u8>>,
    /// Source-tree objects keyed by payload digest.
    pub sources: BTreeMap<CanonicalHash, Vec<u8>>,
    /// Realized-artifact objects keyed by payload digest.
    pub artifacts: BTreeMap<CanonicalHash, Vec<u8>>,
    /// Registration-image objects keyed by payload digest.
    pub registrations: BTreeMap<CanonicalHash, Vec<u8>>,
}

/// Host receipts compared during locked/offline/frozen verification.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProductLockHostReceipts {
    /// Content identity of the current host toolchain.
    pub toolchain: CanonicalHash,
    /// Exact engine build present on the host, when required.
    pub engine_build_id: Option<CanonicalHash>,
    /// Registration image hash compiled from the reopened lock.
    pub registration_image_hash: CanonicalHash,
    /// Runtime-image fingerprint derived from that registration image.
    pub runtime_image_fingerprint: CanonicalHash,
}

/// A product-lock seal, persist, reopen, or verification failure.
#[derive(Debug, Error)]
pub enum ProductLockError {
    /// Canonical JSON encoding failed.
    #[error(transparent)]
    Canonical(#[from] CanonicalJsonError),
    /// Lock bytes were not valid JSON for [`LockV1`].
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    /// Filesystem I/O failed while persisting or reopening a lock.
    #[error("product lock I/O failed at {path}: {source}")]
    Io {
        /// Path that could not be read or written.
        path: PathBuf,
        /// Underlying I/O failure.
        #[source]
        source: io::Error,
    },
    /// The bootstrap manifest is invalid.
    #[error("invalid bootstrap for product lock: {source}")]
    Bootstrap {
        /// Bootstrap validation failure.
        #[from]
        source: crate::BootstrapManifestError,
    },
    /// The locked graph hashes are invalid.
    #[error("invalid locked graph for product lock: {source}")]
    Graph {
        /// Graph hash verification failure.
        #[source]
        source: GraphHashError,
    },
    /// JSON input was valid but not the unique canonical encoding.
    #[error("product lock JSON is not canonical")]
    NonCanonicalEncoding,
    /// The product-lock schema is unsupported.
    #[error("unsupported product lock schema {found}; supported schema is {supported}")]
    UnsupportedSchema {
        /// Schema version found in the lock.
        found: u32,
        /// Schema version implemented by this crate.
        supported: u32,
    },
    /// Frozen or locked mode forbids writing a new lock.
    #[error("product lock write is forbidden in {mode:?} mode")]
    WriteForbidden {
        /// Mode that rejected the write.
        mode: LockActionMode,
    },
    /// `--locked` found different lock bytes than the sealed input.
    #[error("existing product lock bytes differ from the sealed lock")]
    LockBytesChanged,
    /// A lock file required by locked or frozen mode is absent.
    #[error("product lock is missing at {path}")]
    MissingLock {
        /// Expected lock path.
        path: PathBuf,
    },
    /// Lock bytes are truncated or otherwise not a complete [`LockV1`].
    #[error("product lock is incomplete")]
    IncompleteLock,
    /// `--offline` or `--frozen` observed a network source provider.
    #[error("network source provider is forbidden for package {package}")]
    NetworkProviderForbidden {
        /// Package whose provider is not local.
        package: PackageName,
    },
    /// A required store object is missing. Frozen mode never rebuilds it from a path.
    #[error("missing {receipt} receipt{package} expected {expected}", package = package.as_ref().map(|name| format!(" for {name}")).unwrap_or_default())]
    MissingReceipt {
        /// Receipt that could not be loaded.
        receipt: ProductLockReceiptKind,
        /// Package that owns the receipt, when applicable.
        package: Option<PackageName>,
        /// Digest the lock requires.
        expected: CanonicalHash,
    },
    /// An independent receipt does not match the lock.
    #[error(
        "{receipt} receipt mismatch{package}: expected {expected}, actual {actual}",
        package = package.as_ref().map(|name| format!(" for {name}")).unwrap_or_default()
    )]
    ReceiptMismatch {
        /// Receipt that failed independent verification.
        receipt: ProductLockReceiptKind,
        /// Package that owns the receipt, when applicable.
        package: Option<PackageName>,
        /// Digest recorded by the lock.
        expected: CanonicalHash,
        /// Digest recomputed from store or host evidence.
        actual: CanonicalHash,
    },
    /// An alias edge is missing, duplicated, or does not match the selected graph.
    #[error("alias `{alias}` on package {importer} is invalid: {reason}")]
    AliasMismatch {
        /// Importing package.
        importer: PackageName,
        /// Package-local alias.
        alias: String,
        /// Stable structural diagnostic.
        reason: String,
    },
    /// The producer machine identifier is not `latticeaxiom`.
    #[error("product lock producer machine `{machine}` is not {PRODUCT_LOCK_PRODUCER_MACHINE}")]
    InvalidProducer {
        /// Rejected producer machine.
        machine: String,
    },
    /// Cross-field lock structure is inconsistent.
    #[error("invalid product lock structure: {reason}")]
    InvalidStructure {
        /// Presentation-neutral structural diagnostic.
        reason: String,
    },
    /// The stored product-lock hash differs from its payload.
    #[error("product lock hash mismatch: expected {expected}, recomputed {actual}")]
    ProductLockHashMismatch {
        /// Hash stored in the lock.
        expected: CanonicalHash,
        /// Hash recomputed from the sealed payload.
        actual: CanonicalHash,
    },
}

impl ProductLockError {
    fn io(path: impl Into<PathBuf>, source: io::Error) -> Self {
        Self::Io {
            path: path.into(),
            source,
        }
    }

    fn missing(
        receipt: ProductLockReceiptKind,
        package: Option<&PackageName>,
        expected: CanonicalHash,
    ) -> Self {
        Self::MissingReceipt {
            receipt,
            package: package.cloned(),
            expected,
        }
    }

    fn mismatch(
        receipt: ProductLockReceiptKind,
        package: Option<&PackageName>,
        expected: CanonicalHash,
        actual: CanonicalHash,
    ) -> Self {
        Self::ReceiptMismatch {
            receipt,
            package: package.cloned(),
            expected,
            actual,
        }
    }
}

impl std::fmt::Display for ProductLockReceiptKind {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Manifest => "manifest",
            Self::Source => "source",
            Self::Toolchain => "toolchain",
            Self::EngineBuild => "engine-build",
            Self::Artifact => "artifact",
            Self::Registration => "registration",
            Self::RuntimeImage => "runtime-image",
            Self::Alias => "alias",
        })
    }
}

impl LockV1 {
    /// Seals a final product lock from a verified graph and receipts.
    ///
    /// `ResolutionReceiptV1` and `BuildIntentV1` remain intermediate: only
    /// their hashes are stored. [`LockedGameGraph`] hashes are reused.
    ///
    /// # Errors
    ///
    /// Returns [`ProductLockError`] when the bootstrap, graph, alias edges,
    /// realizations, or registration receipts are inconsistent, or when
    /// canonical encoding fails.
    pub fn seal(draft: ProductLockDraftV1) -> Result<Self, ProductLockError> {
        draft.bootstrap.validate()?;
        draft
            .graph
            .verify_hashes()
            .map_err(|source| ProductLockError::Graph { source })?;
        if draft.producer.machine != PRODUCT_LOCK_PRODUCER_MACHINE {
            return Err(ProductLockError::InvalidProducer {
                machine: draft.producer.machine,
            });
        }

        let bootstrap = bootstrap_receipt(&draft.bootstrap)?;
        let portable_resolution = portable_resolution(&draft)?;
        let explanation_hash = canonical_json_hash(&draft.graph.explanation)?;
        let registration_image_hash = draft.registration_image.image_hash;
        draft
            .registration_image
            .verify_image_hash()
            .map_err(|error| ProductLockError::InvalidStructure {
                reason: error.to_string(),
            })?;
        if draft.registration_image.graph_hash != draft.graph.graph_hash {
            return Err(ProductLockError::InvalidStructure {
                reason: "registration image graph hash differs from the locked graph".to_owned(),
            });
        }
        if draft.runtime_image.registration_hash != registration_image_hash {
            return Err(ProductLockError::InvalidStructure {
                reason: "runtime image registration hash differs from the registration image"
                    .to_owned(),
            });
        }
        let runtime_image_fingerprint = canonical_json_hash(&draft.runtime_image)?;
        verify_realizations(&draft, runtime_image_fingerprint, registration_image_hash)?;

        let mut lock = Self {
            schema_version: PRODUCT_LOCK_SCHEMA_VERSION,
            producer: draft.producer,
            bootstrap,
            portable_resolution,
            composition: CompositionLockReceiptV1 {
                composition_hash: draft.graph.composition_hash,
                composition_provenance_hash: draft.graph.composition_provenance_hash,
                evaluation_policy_receipt_hash: draft.evaluation_policy_receipt_hash,
            },
            registration: RegistrationLockReceiptV1 {
                graph_hash: draft.graph.graph_hash,
                graph_lock_hash: draft.graph.lock_hash,
                image_hash: registration_image_hash,
                semantic_hash: draft.registration_semantic_hash,
            },
            realizations: draft.realizations,
            explanation_hash,
            product_lock_hash: CanonicalHash::digest(b"unsealed-product-lock"),
        };
        lock.product_lock_hash = lock.recompute_product_lock_hash()?;
        lock.verify_hashes()?;
        Ok(lock)
    }

    /// Projects lock-scoped Nickel import tables from a reopened product lock.
    ///
    /// Package-aware bare aliases look up the current package instance in
    /// these tables. This is not a grant-global alias map.
    #[must_use]
    pub fn nickel_import_alias_tables(
        &self,
    ) -> (
        BTreeMap<SourceId, PackageName>,
        BTreeMap<PackageName, BTreeMap<PackageAlias, PackageName>>,
    ) {
        let package_instances = self
            .portable_resolution
            .packages
            .iter()
            .map(|(name, package)| (package.source_id.clone(), name.clone()))
            .collect();
        let alias_edges = self
            .portable_resolution
            .alias_edges
            .iter()
            .map(|(importer, edges)| {
                (
                    importer.clone(),
                    edges
                        .iter()
                        .map(|(alias, edge)| (alias.clone(), edge.package.clone()))
                        .collect(),
                )
            })
            .collect();
        (package_instances, alias_edges)
    }

    /// Recomputes the exact product-lock hash without the stored hash field.
    ///
    /// # Errors
    ///
    /// Returns [`CanonicalJsonError`] when the lock cannot be encoded.
    pub fn recompute_product_lock_hash(&self) -> Result<CanonicalHash, CanonicalJsonError> {
        canonical_json_hash(&ProductLockIdentity::from(self))
    }

    /// Encodes this lock using canonical JSON.
    ///
    /// # Errors
    ///
    /// Returns [`CanonicalJsonError`] when encoding fails.
    pub fn canonical_bytes(&self) -> Result<Vec<u8>, CanonicalJsonError> {
        canonical_json_bytes(self)
    }

    /// Validates schema and the stored product-lock hash.
    ///
    /// # Errors
    ///
    /// Returns [`ProductLockError`] for an unsupported schema or hash mismatch.
    pub fn verify_hashes(&self) -> Result<(), ProductLockError> {
        if self.schema_version != PRODUCT_LOCK_SCHEMA_VERSION {
            return Err(ProductLockError::UnsupportedSchema {
                found: self.schema_version,
                supported: PRODUCT_LOCK_SCHEMA_VERSION,
            });
        }
        if self.producer.machine != PRODUCT_LOCK_PRODUCER_MACHINE {
            return Err(ProductLockError::InvalidProducer {
                machine: self.producer.machine.clone(),
            });
        }
        let actual = self.recompute_product_lock_hash()?;
        if actual != self.product_lock_hash {
            return Err(ProductLockError::ProductLockHashMismatch {
                expected: self.product_lock_hash,
                actual,
            });
        }
        Ok(())
    }

    /// Decodes canonical JSON and verifies the stored product-lock hash.
    ///
    /// Partial or non-canonical bytes are rejected. Crash fixtures may only
    /// observe an old complete lock or a new complete lock.
    ///
    /// # Errors
    ///
    /// Returns [`ProductLockError`] for truncated, non-canonical, or
    /// hash-mismatched bytes.
    pub fn from_canonical_bytes(bytes: &[u8]) -> Result<Self, ProductLockError> {
        if bytes.is_empty() {
            return Err(ProductLockError::IncompleteLock);
        }
        let lock = serde_json::from_slice::<Self>(bytes).map_err(|error| {
            if error.is_eof() {
                ProductLockError::IncompleteLock
            } else {
                ProductLockError::Json(error)
            }
        })?;
        if lock.canonical_bytes()?.as_slice() != bytes {
            return Err(ProductLockError::NonCanonicalEncoding);
        }
        lock.verify_hashes()?;
        Ok(lock)
    }
}

/// Atomically writes `latticeaxiom.lock`, reopens it, and verifies hashes.
///
/// The write uses a temporary file in the same directory, `flush`/`sync`,
/// replace, reopen, and hash verification. [`LockActionMode::Frozen`] and
/// [`LockActionMode::Locked`] never replace lock bytes.
///
/// # Errors
///
/// Returns [`ProductLockError`] when the mode forbids writes, encoding or I/O
/// fails, or the reopened bytes are not the sealed lock.
pub fn persist_product_lock(
    path: impl AsRef<Path>,
    lock: &LockV1,
    mode: LockActionMode,
) -> Result<LockV1, ProductLockError> {
    lock.verify_hashes()?;
    reject_network_providers(lock, mode);
    let dest = path.as_ref();
    match mode {
        LockActionMode::Frozen => {
            return Err(ProductLockError::WriteForbidden { mode });
        }
        LockActionMode::Locked => return reopen_existing_unchanged(dest, lock),
        LockActionMode::Persist | LockActionMode::Offline => {}
    }

    let bytes = lock.canonical_bytes()?;
    if let Some(parent) = dest
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent).map_err(|source| ProductLockError::io(parent, source))?;
    }

    let temp = temporary_lock_path(dest);
    if let Err(error) = write_temporary(&temp, &bytes) {
        let _ = fs::remove_file(&temp);
        return Err(error);
    }
    if let Err(error) = replace_lock_file(&temp, dest) {
        let _ = fs::remove_file(&temp);
        return Err(error);
    }

    let reopened = reopen_product_lock(dest)?;
    if reopened.canonical_bytes()?.as_slice() != bytes.as_slice() {
        return Err(ProductLockError::LockBytesChanged);
    }
    Ok(reopened)
}

/// Reads `latticeaxiom.lock` from `path` and accepts only complete bytes.
///
/// Temporary or backup siblings are never treated as the lock. Acquisition
/// paths recorded on a graph are not consulted.
///
/// # Errors
///
/// Returns [`ProductLockError`] when the file is missing, truncated,
/// corrupted, or fails hash verification.
pub fn reopen_product_lock(path: impl AsRef<Path>) -> Result<LockV1, ProductLockError> {
    let path = path.as_ref();
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(source) if source.kind() == io::ErrorKind::NotFound => {
            return Err(ProductLockError::MissingLock {
                path: path.to_path_buf(),
            });
        }
        Err(source) => return Err(ProductLockError::io(path, source)),
    };
    LockV1::from_canonical_bytes(&bytes)
}

/// Verifies lock hashes and independent receipt evidence.
///
/// Manifest, source, toolchain, `EngineBuildId`, registration, artifact, and
/// alias receipts are checked on their own. Matching only
/// [`LockV1::product_lock_hash`] is not sufficient. [`LockActionMode::Frozen`]
/// requires every object to be present and exact and never falls back to a
/// source path.
///
/// # Errors
///
/// Returns [`ProductLockError`] naming the first mismatched or missing
/// receipt in stable package order.
pub fn verify_product_lock(
    lock: &LockV1,
    objects: &ProductLockObjects,
    host: &ProductLockHostReceipts,
    mode: LockActionMode,
) -> Result<(), ProductLockError> {
    lock.verify_hashes()?;
    reject_network_providers(lock, mode);
    verify_alias_edges(lock)?;
    verify_host_receipts(lock, host)?;

    let require_objects = matches!(mode, LockActionMode::Frozen);
    for (name, package) in &lock.portable_resolution.packages {
        verify_object(
            ProductLockReceiptKind::Manifest,
            Some(name),
            package.manifest_digest,
            objects.manifests.get(&package.manifest_digest),
            require_objects,
        )?;
        let source_bytes = objects.sources.get(&package.source_object_digest);
        verify_object(
            ProductLockReceiptKind::Source,
            Some(name),
            package.source_object_digest,
            source_bytes,
            require_objects,
        )?;
        if let Some(bytes) = source_bytes {
            verify_source_identity(name, package, bytes)?;
        }
    }

    for realization in lock.realizations.values() {
        if realization.toolchain != lock.producer.toolchain {
            return Err(ProductLockError::mismatch(
                ProductLockReceiptKind::Toolchain,
                None,
                lock.producer.toolchain,
                realization.toolchain,
            ));
        }
        for (name, package) in &realization.packages {
            verify_object(
                ProductLockReceiptKind::Artifact,
                Some(name),
                package.artifact_digest,
                objects.artifacts.get(&package.artifact_digest),
                require_objects,
            )?;
        }
        verify_object(
            ProductLockReceiptKind::Registration,
            None,
            realization.registration_image_hash,
            objects
                .registrations
                .get(&realization.registration_image_hash),
            false,
        )?;
    }
    Ok(())
}

#[derive(Serialize)]
struct ProductLockIdentity<'a> {
    schema_version: u32,
    producer: &'a ProductLockProducerV1,
    bootstrap: &'a BootstrapLockReceiptV1,
    portable_resolution: &'a PortableResolutionLockV1,
    composition: &'a CompositionLockReceiptV1,
    registration: &'a RegistrationLockReceiptV1,
    realizations: &'a BTreeMap<TargetTriple, TargetRealizationLockV1>,
    explanation_hash: CanonicalHash,
}

impl<'a> From<&'a LockV1> for ProductLockIdentity<'a> {
    fn from(lock: &'a LockV1) -> Self {
        Self {
            schema_version: lock.schema_version,
            producer: &lock.producer,
            bootstrap: &lock.bootstrap,
            portable_resolution: &lock.portable_resolution,
            composition: &lock.composition,
            registration: &lock.registration,
            realizations: &lock.realizations,
            explanation_hash: lock.explanation_hash,
        }
    }
}

fn bootstrap_receipt(
    bootstrap: &CompositionBootstrapV1,
) -> Result<BootstrapLockReceiptV1, ProductLockError> {
    let mut source_kinds = BTreeMap::new();
    for source in &bootstrap.sources {
        source_kinds.insert(source.package().clone(), source_kind(source));
    }
    Ok(BootstrapLockReceiptV1 {
        manifest_digest: bootstrap.canonical_hash()?,
        projection: bootstrap.projection,
        evaluation_policy: bootstrap.evaluation_policy.clone(),
        nickel_profile_entry: bootstrap.nickel_profile_entry.clone(),
        source_kinds,
    })
}

fn source_kind(source: &BootstrapSourceProviderV1) -> BootstrapSourceKindV1 {
    match source {
        BootstrapSourceProviderV1::Workspace { .. } => BootstrapSourceKindV1::Workspace,
        BootstrapSourceProviderV1::Path { .. } => BootstrapSourceKindV1::Path,
        BootstrapSourceProviderV1::LocalCatalog { .. } => BootstrapSourceKindV1::LocalCatalog,
        BootstrapSourceProviderV1::Fixture { .. } => BootstrapSourceKindV1::Fixture,
    }
}

fn portable_resolution(
    draft: &ProductLockDraftV1,
) -> Result<PortableResolutionLockV1, ProductLockError> {
    if draft.bootstrap.evaluation_policy != draft.graph.evaluation_policy {
        return Err(ProductLockError::InvalidStructure {
            reason: "bootstrap evaluation policy differs from the locked graph".to_owned(),
        });
    }
    if draft
        .bootstrap
        .roots
        .keys()
        .cloned()
        .collect::<BTreeSet<_>>()
        != draft.graph.roots
    {
        return Err(ProductLockError::InvalidStructure {
            reason: "bootstrap roots differ from the locked graph".to_owned(),
        });
    }

    let mut packages = BTreeMap::new();
    let mut features = BTreeMap::new();
    let mut domains = BTreeMap::new();
    for (name, package) in &draft.graph.packages {
        let source_object_digest = draft.source_objects.get(name).copied().ok_or_else(|| {
            ProductLockError::missing(
                ProductLockReceiptKind::Source,
                Some(name),
                package.source_hash,
            )
        })?;
        let selected_features = draft
            .package_features
            .get(name)
            .cloned()
            .unwrap_or_default();
        packages.insert(
            name.clone(),
            portable_package(package, source_object_digest, selected_features.clone()),
        );
        features.insert(name.clone(), selected_features);
        domains.insert(name.clone(), package.domains.clone());
    }
    verify_alias_edges_against_graph(&draft.graph, &draft.alias_edges)?;

    Ok(PortableResolutionLockV1 {
        resolution_receipt_hash: draft.resolution_receipt_hash,
        graph_hash: draft.graph.graph_hash,
        packages,
        alias_edges: draft.alias_edges.clone(),
        features,
        domains,
        capabilities: draft.graph.capability_providers.clone(),
        roots: draft.graph.roots.clone(),
        evaluation_policy: draft.graph.evaluation_policy.clone(),
        evaluation_limits: draft.graph.evaluation_limits,
    })
}

fn portable_package(
    package: &LockedPackage,
    source_object_digest: CanonicalHash,
    features: BTreeSet<String>,
) -> PortableLockedPackageV1 {
    PortableLockedPackageV1 {
        version: package.version.clone(),
        source_id: package.source_id.clone(),
        source_digest: package.source_hash,
        source_object_digest,
        manifest_digest: package.manifest_hash,
        provenance_hash: package.provenance_hash,
        features,
        domains: package.domains.clone(),
    }
}

fn verify_alias_edges_against_graph(
    graph: &LockedGameGraph,
    alias_edges: &BTreeMap<PackageName, BTreeMap<PackageAlias, LockedAliasEdgeV1>>,
) -> Result<(), ProductLockError> {
    for (importer, aliases) in alias_edges {
        let Some(package) = graph.packages.get(importer) else {
            return Err(ProductLockError::AliasMismatch {
                importer: importer.clone(),
                alias: String::new(),
                reason: "importing package is absent from the locked graph".to_owned(),
            });
        };
        for (alias, edge) in aliases {
            let Some(dependency) = package.dependencies.get(&edge.package) else {
                return Err(ProductLockError::AliasMismatch {
                    importer: importer.clone(),
                    alias: alias.to_string(),
                    reason: format!("alias does not name a direct dependency {}", edge.package),
                });
            };
            if dependency.version != edge.version {
                return Err(ProductLockError::AliasMismatch {
                    importer: importer.clone(),
                    alias: alias.to_string(),
                    reason: format!(
                        "alias version {} differs from selected {}",
                        edge.version, dependency.version
                    ),
                });
            }
            if dependency.features != edge.features {
                return Err(ProductLockError::AliasMismatch {
                    importer: importer.clone(),
                    alias: alias.to_string(),
                    reason: "alias feature set differs from the selected dependency".to_owned(),
                });
            }
            let Some(selected) = graph.packages.get(&edge.package) else {
                return Err(ProductLockError::AliasMismatch {
                    importer: importer.clone(),
                    alias: alias.to_string(),
                    reason: format!(
                        "alias target {} is absent from the locked graph",
                        edge.package
                    ),
                });
            };
            if selected.version != edge.version {
                return Err(ProductLockError::AliasMismatch {
                    importer: importer.clone(),
                    alias: alias.to_string(),
                    reason: format!(
                        "alias target version {} differs from selected {}",
                        edge.version, selected.version
                    ),
                });
            }
        }
    }
    Ok(())
}

fn verify_realizations(
    draft: &ProductLockDraftV1,
    runtime_image_fingerprint: CanonicalHash,
    registration_image_hash: CanonicalHash,
) -> Result<(), ProductLockError> {
    if draft.realizations.is_empty() {
        return Err(ProductLockError::InvalidStructure {
            reason: "product lock must seal at least one target realization".to_owned(),
        });
    }
    for (target, realization) in &draft.realizations {
        verify_one_realization(
            draft,
            target,
            realization,
            runtime_image_fingerprint,
            registration_image_hash,
        )?;
    }
    Ok(())
}

fn verify_one_realization(
    draft: &ProductLockDraftV1,
    target: &TargetTriple,
    realization: &TargetRealizationLockV1,
    runtime_image_fingerprint: CanonicalHash,
    registration_image_hash: CanonicalHash,
) -> Result<(), ProductLockError> {
    if &realization.target != target {
        return Err(ProductLockError::InvalidStructure {
            reason: format!(
                "realization target {} is keyed as {target}",
                realization.target
            ),
        });
    }
    if realization.projection != draft.bootstrap.projection {
        return Err(ProductLockError::InvalidStructure {
            reason: "realization projection differs from the bootstrap projection".to_owned(),
        });
    }
    if realization.toolchain != draft.producer.toolchain {
        return Err(ProductLockError::mismatch(
            ProductLockReceiptKind::Toolchain,
            None,
            draft.producer.toolchain,
            realization.toolchain,
        ));
    }
    if realization.registration_image_hash != registration_image_hash {
        return Err(ProductLockError::mismatch(
            ProductLockReceiptKind::Registration,
            None,
            registration_image_hash,
            realization.registration_image_hash,
        ));
    }
    if realization.runtime_image_fingerprint != runtime_image_fingerprint {
        return Err(ProductLockError::mismatch(
            ProductLockReceiptKind::RuntimeImage,
            None,
            runtime_image_fingerprint,
            realization.runtime_image_fingerprint,
        ));
    }
    verify_realization_packages(draft, target, realization)
}

fn verify_realization_packages(
    draft: &ProductLockDraftV1,
    target: &TargetTriple,
    realization: &TargetRealizationLockV1,
) -> Result<(), ProductLockError> {
    if realization.packages.len() != draft.graph.packages.len() {
        return Err(ProductLockError::InvalidStructure {
            reason: format!("target {target} omits or adds packages relative to the graph"),
        });
    }
    let mut required_engine = None;
    for (name, package) in &draft.graph.packages {
        let Some(realized) = realization.packages.get(name) else {
            return Err(ProductLockError::InvalidStructure {
                reason: format!("target {target} omits package {name}"),
            });
        };
        if realized.realization_id != package.realization_id {
            return Err(ProductLockError::InvalidStructure {
                reason: format!("package {name} realization id differs from the locked graph"),
            });
        }
        if realized.kind != package.realization {
            return Err(ProductLockError::InvalidStructure {
                reason: format!("package {name} realization kind differs from the locked graph"),
            });
        }
        if realized.artifact_digest != package.artifact_hash {
            return Err(ProductLockError::mismatch(
                ProductLockReceiptKind::Artifact,
                Some(name),
                package.artifact_hash,
                realized.artifact_digest,
            ));
        }
        if realized.engine_build_id != package.engine_build_id {
            return Err(ProductLockError::mismatch(
                ProductLockReceiptKind::EngineBuild,
                Some(name),
                package.engine_build_id.unwrap_or_else(zero_hash),
                realized.engine_build_id.unwrap_or_else(zero_hash),
            ));
        }
        if let Some(engine_build_id) = realized.engine_build_id {
            match required_engine {
                None => required_engine = Some(engine_build_id),
                Some(existing) if existing == engine_build_id => {}
                Some(existing) => {
                    return Err(ProductLockError::mismatch(
                        ProductLockReceiptKind::EngineBuild,
                        Some(name),
                        existing,
                        engine_build_id,
                    ));
                }
            }
        }
        let expected_binding = draft.runtime_image.packages.get(name).ok_or_else(|| {
            ProductLockError::InvalidStructure {
                reason: format!("runtime image omits package {name}"),
            }
        })?;
        if expected_binding.artifact_hash != realized.artifact_digest
            || expected_binding.realization != realized.kind
        {
            return Err(ProductLockError::InvalidStructure {
                reason: format!("runtime image binding for {name} differs from the realization"),
            });
        }
    }
    if realization.engine_build_id != required_engine {
        return Err(ProductLockError::mismatch(
            ProductLockReceiptKind::EngineBuild,
            None,
            required_engine.unwrap_or_else(zero_hash),
            realization.engine_build_id.unwrap_or_else(zero_hash),
        ));
    }
    Ok(())
}

fn zero_hash() -> CanonicalHash {
    CanonicalHash::digest(b"")
}

fn verify_alias_edges(lock: &LockV1) -> Result<(), ProductLockError> {
    for (importer, aliases) in &lock.portable_resolution.alias_edges {
        let Some(package) = lock.portable_resolution.packages.get(importer) else {
            return Err(ProductLockError::AliasMismatch {
                importer: importer.clone(),
                alias: String::new(),
                reason: "importing package is absent from portable resolution".to_owned(),
            });
        };
        let _ = package;
        for (alias, edge) in aliases {
            let Some(target) = lock.portable_resolution.packages.get(&edge.package) else {
                return Err(ProductLockError::AliasMismatch {
                    importer: importer.clone(),
                    alias: alias.to_string(),
                    reason: format!(
                        "alias target {} is absent from portable resolution",
                        edge.package
                    ),
                });
            };
            if target.version != edge.version {
                return Err(ProductLockError::AliasMismatch {
                    importer: importer.clone(),
                    alias: alias.to_string(),
                    reason: format!(
                        "alias version {} differs from selected {}",
                        edge.version, target.version
                    ),
                });
            }
        }
    }
    Ok(())
}

fn reject_network_providers(lock: &LockV1, mode: LockActionMode) {
    if !matches!(mode, LockActionMode::Offline | LockActionMode::Frozen) {
        return;
    }
    for kind in lock.bootstrap.source_kinds.values() {
        match kind {
            BootstrapSourceKindV1::Workspace
            | BootstrapSourceKindV1::Path
            | BootstrapSourceKindV1::LocalCatalog
            | BootstrapSourceKindV1::Fixture => {}
        }
    }
}

fn verify_host_receipts(
    lock: &LockV1,
    host: &ProductLockHostReceipts,
) -> Result<(), ProductLockError> {
    if host.toolchain != lock.producer.toolchain {
        return Err(ProductLockError::mismatch(
            ProductLockReceiptKind::Toolchain,
            None,
            lock.producer.toolchain,
            host.toolchain,
        ));
    }
    if host.registration_image_hash != lock.registration.image_hash {
        return Err(ProductLockError::mismatch(
            ProductLockReceiptKind::Registration,
            None,
            lock.registration.image_hash,
            host.registration_image_hash,
        ));
    }
    for realization in lock.realizations.values() {
        if host.runtime_image_fingerprint != realization.runtime_image_fingerprint {
            return Err(ProductLockError::mismatch(
                ProductLockReceiptKind::RuntimeImage,
                None,
                realization.runtime_image_fingerprint,
                host.runtime_image_fingerprint,
            ));
        }
        if host.engine_build_id != realization.engine_build_id {
            return Err(ProductLockError::mismatch(
                ProductLockReceiptKind::EngineBuild,
                None,
                realization.engine_build_id.unwrap_or_else(zero_hash),
                host.engine_build_id.unwrap_or_else(zero_hash),
            ));
        }
    }
    Ok(())
}

fn verify_object(
    receipt: ProductLockReceiptKind,
    package: Option<&PackageName>,
    expected: CanonicalHash,
    bytes: Option<&Vec<u8>>,
    required: bool,
) -> Result<(), ProductLockError> {
    match bytes {
        None if required => Err(ProductLockError::missing(receipt, package, expected)),
        None => Ok(()),
        Some(bytes) => {
            let actual = CanonicalHash::digest(bytes);
            if actual == expected {
                Ok(())
            } else {
                Err(ProductLockError::mismatch(
                    receipt, package, expected, actual,
                ))
            }
        }
    }
}

fn verify_source_identity(
    package: &PackageName,
    locked: &PortableLockedPackageV1,
    bytes: &[u8],
) -> Result<(), ProductLockError> {
    if let Ok(snapshot) = serde_json::from_slice::<crate::SourceSnapshot>(bytes) {
        snapshot
            .verify()
            .map_err(|error| ProductLockError::InvalidStructure {
                reason: format!("source snapshot for {package} failed verification: {error}"),
            })?;
        if snapshot.source_hash() != locked.source_digest {
            return Err(ProductLockError::mismatch(
                ProductLockReceiptKind::Source,
                Some(package),
                locked.source_digest,
                snapshot.source_hash(),
            ));
        }
        if snapshot.source_id() != &locked.source_id {
            return Err(ProductLockError::InvalidStructure {
                reason: format!(
                    "source snapshot identity {} differs from lock {}",
                    snapshot.source_id(),
                    locked.source_id
                ),
            });
        }
        return Ok(());
    }
    let actual = CanonicalHash::digest(bytes);
    if actual == locked.source_digest {
        Ok(())
    } else {
        Err(ProductLockError::mismatch(
            ProductLockReceiptKind::Source,
            Some(package),
            locked.source_digest,
            actual,
        ))
    }
}

fn reopen_existing_unchanged(path: &Path, lock: &LockV1) -> Result<LockV1, ProductLockError> {
    let existing = reopen_product_lock(path)?;
    if existing.canonical_bytes()? != lock.canonical_bytes()? {
        return Err(ProductLockError::LockBytesChanged);
    }
    Ok(existing)
}

fn temporary_lock_path(dest: &Path) -> PathBuf {
    let name = dest
        .file_name()
        .map_or(PRODUCT_LOCK_FILE_NAME.as_ref(), |name| name);
    dest.with_file_name(format!("{}{TEMPORARY_LOCK_SUFFIX}", name.to_string_lossy()))
}

fn backup_lock_path(dest: &Path) -> PathBuf {
    let name = dest
        .file_name()
        .map_or(PRODUCT_LOCK_FILE_NAME.as_ref(), |name| name);
    dest.with_file_name(format!("{}{BACKUP_LOCK_SUFFIX}", name.to_string_lossy()))
}

fn write_temporary(path: &Path, bytes: &[u8]) -> Result<(), ProductLockError> {
    let mut file = File::create(path).map_err(|source| ProductLockError::io(path, source))?;
    file.write_all(bytes)
        .map_err(|source| ProductLockError::io(path, source))?;
    file.sync_all()
        .map_err(|source| ProductLockError::io(path, source))?;
    Ok(())
}

fn replace_lock_file(temp: &Path, dest: &Path) -> Result<(), ProductLockError> {
    match fs::rename(temp, dest) {
        Ok(()) => Ok(()),
        Err(_) if dest.exists() => replace_existing_lock(temp, dest),
        Err(source) => Err(ProductLockError::io(dest, source)),
    }
}

fn replace_existing_lock(temp: &Path, dest: &Path) -> Result<(), ProductLockError> {
    let backup = backup_lock_path(dest);
    let _ = fs::remove_file(&backup);
    fs::rename(dest, &backup).map_err(|source| ProductLockError::io(dest, source))?;
    match fs::rename(temp, dest) {
        Ok(()) => {
            let _ = fs::remove_file(&backup);
            Ok(())
        }
        Err(source) => {
            let _ = fs::rename(&backup, dest);
            Err(ProductLockError::io(dest, source))
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fmt::Debug;

    use latticeaxiom_core::SchemaId;

    use super::*;
    use crate::{
        LOCK_SCHEMA_VERSION, LockedDependency, ObservabilityCatalog, RuntimeBinding,
        SemanticCatalog, SettingsCatalog,
    };

    #[derive(Debug)]
    struct TestDirectory(tempfile::TempDir);

    impl TestDirectory {
        fn create() -> Self {
            Self(
                tempfile::Builder::new()
                    .prefix("latticeaxiom-compose-product-lock-")
                    .tempdir()
                    .unwrap_or_else(|error| panic!("test directory was not created: {error}")),
            )
        }

        fn lock_path(&self) -> PathBuf {
            self.0.path().join(PRODUCT_LOCK_FILE_NAME)
        }
    }

    fn succeeded<T, E>(result: Result<T, E>) -> T
    where
        E: Debug,
    {
        result.unwrap_or_else(|error| panic!("operation unexpectedly failed: {error:?}"))
    }

    fn package_name(value: &str) -> PackageName {
        value
            .parse()
            .unwrap_or_else(|error| panic!("fixture package `{value}` is invalid: {error}"))
    }

    fn version(value: &str) -> PackageVersion {
        value
            .parse()
            .unwrap_or_else(|error| panic!("fixture version `{value}` is invalid: {error}"))
    }

    fn source_id(label: &str) -> SourceId {
        format!("latticeaxiom:source/{label}")
            .parse()
            .unwrap_or_else(|error| panic!("fixture source `{label}` is invalid: {error}"))
    }

    fn policy() -> StableId {
        "latticeaxiom:nickel-evaluation-policy/r0@1"
            .parse()
            .unwrap_or_else(|error| panic!("fixture policy ID is invalid: {error}"))
    }

    fn target() -> TargetTriple {
        "x86_64-pc-windows-msvc"
            .parse()
            .unwrap_or_else(|error| panic!("fixture target is invalid: {error}"))
    }

    fn realization_id() -> RealizationId {
        RealizationId::new("data")
            .unwrap_or_else(|error| panic!("fixture realization is invalid: {error}"))
    }

    fn bootstrap() -> CompositionBootstrapV1 {
        succeeded(CompositionBootstrapV1::from_toml_str(
            r#"
schema_version = 1
projection = "headless-test"
projection_domains = ["authoritative"]
evaluation_policy = "latticeaxiom:nickel-evaluation-policy/r0@1"
realization_policy = ["data"]
nickel_profile_entry = "profiles/test.ncl"

[roots.terrenia]
version = "=0.1.0"
realization = { mode = "auto" }

[[sources]]
kind = "workspace"
package = "terrenia"

[[sources]]
kind = "path"
package = "@terrenia/blocks"
path = "packages/terrenia/blocks"
"#,
        ))
    }

    fn locked_package(
        name: &str,
        source: &[u8],
        manifest: &[u8],
        artifact: &[u8],
        dependencies: BTreeMap<PackageName, LockedDependency>,
        engine_build_id: Option<CanonicalHash>,
    ) -> LockedPackage {
        LockedPackage {
            name: package_name(name),
            version: version("0.1.0"),
            source_id: source_id(name.trim_start_matches('@').replace('/', "-").as_str()),
            source_hash: CanonicalHash::digest(source),
            provenance_hash: CanonicalHash::digest(b"provenance"),
            realization: RealizationKind::Data,
            realization_id: realization_id(),
            manifest_hash: CanonicalHash::digest(manifest),
            artifact_hash: CanonicalHash::digest(artifact),
            interfaces: BTreeMap::new(),
            engine_build_id,
            domains: BTreeSet::from([PackageDomain::Authoritative]),
            dependencies,
            schemas: BTreeSet::<SchemaId>::new(),
            source_path: format!("packages/{name}"),
        }
    }

    fn hashed_graph(mut graph: LockedGameGraph) -> LockedGameGraph {
        graph.graph_hash = succeeded(graph.recompute_graph_hash());
        graph.lock_hash = succeeded(graph.recompute_lock_hash());
        succeeded(graph.verify_hashes());
        graph
    }

    fn fixture_graph(engine_build_id: Option<CanonicalHash>) -> LockedGameGraph {
        let root = package_name("terrenia");
        let dependency = package_name("@terrenia/blocks");
        let blocks = locked_package(
            "@terrenia/blocks",
            b"blocks-source",
            b"blocks-manifest",
            b"blocks-artifact",
            BTreeMap::new(),
            engine_build_id,
        );
        let terrenia = locked_package(
            "terrenia",
            b"terrenia-source",
            b"terrenia-manifest",
            b"terrenia-artifact",
            BTreeMap::from([(
                dependency.clone(),
                LockedDependency {
                    version: version("0.1.0"),
                    features: BTreeSet::new(),
                },
            )]),
            engine_build_id,
        );
        hashed_graph(LockedGameGraph {
            schema_version: LOCK_SCHEMA_VERSION,
            composition_hash: CanonicalHash::digest(b"composition"),
            composition_provenance_hash: CanonicalHash::digest(b"composition-provenance"),
            evaluation_policy: policy(),
            evaluation_limits: NickelEvaluationLimits::default(),
            roots: BTreeSet::from([root.clone()]),
            packages: BTreeMap::from([(root, terrenia), (dependency, blocks)]),
            capability_providers: BTreeMap::new(),
            namespace_grants: BTreeSet::new(),
            explanation: Vec::new(),
            graph_hash: CanonicalHash::digest(b"unverified-graph"),
            lock_hash: CanonicalHash::digest(b"unverified-lock"),
        })
    }

    fn registration_image(graph: &LockedGameGraph) -> RegistrationImage {
        let mut image = RegistrationImage {
            graph_hash: graph.graph_hash,
            numeric_ids: BTreeMap::new(),
            owners: BTreeMap::new(),
            schema_owners: BTreeMap::new(),
            semantics: SemanticCatalog {
                tags: BTreeMap::new(),
                maps: BTreeMap::new(),
                role_bindings: BTreeMap::new(),
                active_bundles: BTreeSet::new(),
            },
            settings: SettingsCatalog {
                runtime: BTreeMap::new(),
                composition: BTreeMap::new(),
            },
            observability: ObservabilityCatalog {
                info_items: BTreeMap::new(),
                metrics: BTreeMap::new(),
                inspect: BTreeMap::new(),
                visualizers: BTreeMap::new(),
            },
            schedule: Vec::new(),
            authoritative: BTreeSet::new(),
            image_hash: CanonicalHash::digest(b"unverified-image"),
        };
        image.image_hash = succeeded(image.recompute_image_hash());
        succeeded(image.verify_image_hash());
        image
    }

    fn runtime_image(graph: &LockedGameGraph, registration_hash: CanonicalHash) -> RuntimeImage {
        RuntimeImage {
            registration_hash,
            packages: graph
                .packages
                .iter()
                .map(|(name, package)| {
                    (
                        name.clone(),
                        RuntimeBinding {
                            realization: package.realization,
                            artifact_hash: package.artifact_hash,
                            callbacks: BTreeSet::new(),
                        },
                    )
                })
                .collect(),
        }
    }

    fn alias_edges() -> BTreeMap<PackageName, BTreeMap<PackageAlias, LockedAliasEdgeV1>> {
        let alias = succeeded(PackageAlias::new("blocks"));
        BTreeMap::from([(
            package_name("terrenia"),
            BTreeMap::from([(
                alias,
                LockedAliasEdgeV1 {
                    package: package_name("@terrenia/blocks"),
                    version: version("0.1.0"),
                    features: BTreeSet::new(),
                },
            )]),
        )])
    }

    fn source_objects(graph: &LockedGameGraph) -> BTreeMap<PackageName, CanonicalHash> {
        graph
            .packages
            .iter()
            .map(|(name, package)| (name.clone(), package.source_hash))
            .collect()
    }

    fn realization(
        graph: &LockedGameGraph,
        toolchain: CanonicalHash,
        registration_image_hash: CanonicalHash,
        runtime_image_fingerprint: CanonicalHash,
        engine_build_id: Option<CanonicalHash>,
    ) -> TargetRealizationLockV1 {
        TargetRealizationLockV1 {
            projection: ProfileKind::HeadlessTest,
            target: target(),
            toolchain,
            build_intent_hash: CanonicalHash::digest(b"build-intent"),
            packages: graph
                .packages
                .iter()
                .map(|(name, package)| {
                    (
                        name.clone(),
                        TargetPackageRealizationV1 {
                            realization_id: package.realization_id.clone(),
                            kind: package.realization,
                            artifact_digest: package.artifact_hash,
                            engine_build_id: package.engine_build_id,
                            registration_hash: package.manifest_hash,
                        },
                    )
                })
                .collect(),
            engine_build_id,
            registration_image_hash,
            runtime_image_fingerprint,
        }
    }

    fn sealed_lock(engine_build_id: Option<CanonicalHash>) -> (LockV1, ProductLockObjects) {
        let graph = fixture_graph(engine_build_id);
        let registration_image = registration_image(&graph);
        let runtime_image = runtime_image(&graph, registration_image.image_hash);
        let toolchain = CanonicalHash::digest(b"toolchain");
        let runtime_image_fingerprint = succeeded(canonical_json_hash(&runtime_image));
        let mut objects = ProductLockObjects::default();
        objects.manifests.insert(
            CanonicalHash::digest(b"terrenia-manifest"),
            b"terrenia-manifest".to_vec(),
        );
        objects.manifests.insert(
            CanonicalHash::digest(b"blocks-manifest"),
            b"blocks-manifest".to_vec(),
        );
        objects.sources.insert(
            CanonicalHash::digest(b"terrenia-source"),
            b"terrenia-source".to_vec(),
        );
        objects.sources.insert(
            CanonicalHash::digest(b"blocks-source"),
            b"blocks-source".to_vec(),
        );
        objects.artifacts.insert(
            CanonicalHash::digest(b"terrenia-artifact"),
            b"terrenia-artifact".to_vec(),
        );
        objects.artifacts.insert(
            CanonicalHash::digest(b"blocks-artifact"),
            b"blocks-artifact".to_vec(),
        );
        let lock = succeeded(LockV1::seal(ProductLockDraftV1 {
            producer: ProductLockProducerV1 {
                machine: PRODUCT_LOCK_PRODUCER_MACHINE.to_owned(),
                toolchain,
            },
            bootstrap: bootstrap(),
            package_features: BTreeMap::new(),
            source_objects: source_objects(&graph),
            alias_edges: alias_edges(),
            resolution_receipt_hash: CanonicalHash::digest(b"resolution-receipt"),
            evaluation_policy_receipt_hash: CanonicalHash::digest(b"evaluation-policy"),
            realizations: BTreeMap::from([(
                target(),
                realization(
                    &graph,
                    toolchain,
                    registration_image.image_hash,
                    runtime_image_fingerprint,
                    engine_build_id,
                ),
            )]),
            registration_semantic_hash: CanonicalHash::digest(b"registration-semantic"),
            registration_image,
            runtime_image,
            graph,
        }));
        (lock, objects)
    }

    fn host_from(lock: &LockV1) -> ProductLockHostReceipts {
        let realization = lock
            .realizations
            .values()
            .next()
            .unwrap_or_else(|| panic!("fixture lock must seal a realization"));
        ProductLockHostReceipts {
            toolchain: lock.producer.toolchain,
            engine_build_id: realization.engine_build_id,
            registration_image_hash: lock.registration.image_hash,
            runtime_image_fingerprint: realization.runtime_image_fingerprint,
        }
    }

    fn persist_path(directory: &TestDirectory) -> PathBuf {
        directory.lock_path()
    }

    #[test]
    fn persist_reopens_complete_canonical_bytes() {
        let directory = TestDirectory::create();
        let (lock, _) = sealed_lock(None);
        let reopened = succeeded(persist_product_lock(
            persist_path(&directory),
            &lock,
            LockActionMode::Persist,
        ));
        assert_eq!(reopened, lock);
        assert_eq!(
            succeeded(reopen_product_lock(persist_path(&directory))),
            lock
        );
        assert!(
            !temporary_lock_path(&persist_path(&directory)).exists(),
            "temporary lock file must not remain after persist"
        );
    }

    #[test]
    fn archived_locks_keep_both_closures_and_reject_modified_archive_bytes() {
        let directory = TestDirectory::create();
        let catalog = persist_path(&directory).with_file_name("catalog");
        let (old, _) = sealed_lock(None);
        let old_path = succeeded(archive_product_lock(&catalog, &old));
        let (mut replacement, _) = sealed_lock(None);
        replacement.composition.evaluation_policy_receipt_hash =
            CanonicalHash::digest(b"next-default");
        replacement.product_lock_hash = succeeded(replacement.recompute_product_lock_hash());
        let new_path = succeeded(archive_product_lock(&catalog, &replacement));
        assert_ne!(old_path, new_path);
        assert_eq!(succeeded(reopen_product_lock(&old_path)), old);
        assert_eq!(succeeded(reopen_product_lock(&new_path)), replacement);
        assert_eq!(succeeded(archive_product_lock(&catalog, &old)), old_path);
        succeeded(fs::write(&old_path, b"corrupt archive"));
        assert!(archive_product_lock(&catalog, &old).is_err());
        assert_eq!(succeeded(fs::read(&old_path)), b"corrupt archive");
    }

    #[test]
    fn atomic_replace_leaves_only_the_new_complete_lock() {
        let directory = TestDirectory::create();
        let (old, _) = sealed_lock(None);
        succeeded(persist_product_lock(
            persist_path(&directory),
            &old,
            LockActionMode::Persist,
        ));

        let (mut new_lock, _) = sealed_lock(None);
        new_lock.composition.evaluation_policy_receipt_hash =
            CanonicalHash::digest(b"replaced-evaluation-policy");
        new_lock.product_lock_hash = succeeded(new_lock.recompute_product_lock_hash());
        let replaced = succeeded(persist_product_lock(
            persist_path(&directory),
            &new_lock,
            LockActionMode::Persist,
        ));
        assert_eq!(replaced, new_lock);
        assert_ne!(replaced, old);
        assert_eq!(
            succeeded(fs::read(persist_path(&directory))),
            succeeded(new_lock.canonical_bytes())
        );
    }

    #[test]
    fn crash_corpus_keeps_old_complete_lock_when_temp_is_partial() {
        let directory = TestDirectory::create();
        let (old, _) = sealed_lock(None);
        succeeded(persist_product_lock(
            persist_path(&directory),
            &old,
            LockActionMode::Persist,
        ));
        let dest = persist_path(&directory);
        succeeded(fs::write(
            temporary_lock_path(&dest),
            b"{\"schema_version\":",
        ));
        assert_eq!(succeeded(reopen_product_lock(&dest)), old);
        assert!(LockV1::from_canonical_bytes(b"{\"schema_version\":").is_err());
    }

    #[test]
    fn reopen_rejects_truncated_and_corrupted_lock_bytes() {
        let directory = TestDirectory::create();
        let (lock, _) = sealed_lock(None);
        let dest = persist_path(&directory);
        succeeded(persist_product_lock(&dest, &lock, LockActionMode::Persist));

        let complete = succeeded(fs::read(&dest));
        succeeded(fs::write(&dest, &complete[..complete.len() / 2]));
        assert!(matches!(
            reopen_product_lock(&dest),
            Err(ProductLockError::IncompleteLock)
        ));

        succeeded(fs::write(&dest, b"not-json"));
        assert!(matches!(
            reopen_product_lock(&dest),
            Err(ProductLockError::Json(_) | ProductLockError::IncompleteLock)
        ));

        let mut tampered = complete;
        let last = tampered
            .len()
            .checked_sub(2)
            .unwrap_or_else(|| panic!("canonical lock must not be empty"));
        tampered[last] ^= 0x7f;
        succeeded(fs::write(&dest, tampered));
        assert!(reopen_product_lock(&dest).is_err());
    }

    #[test]
    fn frozen_mode_forbids_writes_and_offline_accepts_local_providers() {
        let directory = TestDirectory::create();
        let (lock, objects) = sealed_lock(None);
        assert!(matches!(
            persist_product_lock(persist_path(&directory), &lock, LockActionMode::Frozen),
            Err(ProductLockError::WriteForbidden {
                mode: LockActionMode::Frozen
            })
        ));
        succeeded(verify_product_lock(
            &lock,
            &objects,
            &host_from(&lock),
            LockActionMode::Offline,
        ));
        succeeded(verify_product_lock(
            &lock,
            &objects,
            &host_from(&lock),
            LockActionMode::Frozen,
        ));
    }

    #[test]
    fn frozen_rejects_missing_source_without_path_fallback() {
        let (lock, mut objects) = sealed_lock(None);
        let source_digest = lock
            .portable_resolution
            .packages
            .get(&package_name("terrenia"))
            .unwrap_or_else(|| panic!("root package must be sealed"))
            .source_object_digest;
        objects.sources.remove(&source_digest);
        let encoded = succeeded(serde_json::to_string(&lock));
        assert!(
            !encoded.contains("packages/terrenia"),
            "graph source paths must not participate in frozen identity"
        );
        match verify_product_lock(&lock, &objects, &host_from(&lock), LockActionMode::Frozen) {
            Err(ProductLockError::MissingReceipt {
                receipt: ProductLockReceiptKind::Source,
                ..
            }) => {}
            other => panic!("expected missing source receipt, got {other:?}"),
        }
    }

    #[test]
    fn frozen_names_the_mismatched_receipt() {
        let (lock, mut objects) = sealed_lock(Some(CanonicalHash::digest(b"engine-build")));
        let host = host_from(&lock);

        let manifest = CanonicalHash::digest(b"terrenia-manifest");
        objects
            .manifests
            .insert(manifest, b"tampered-manifest".to_vec());
        match verify_product_lock(&lock, &objects, &host, LockActionMode::Frozen) {
            Err(ProductLockError::ReceiptMismatch {
                receipt: ProductLockReceiptKind::Manifest,
                ..
            }) => {}
            other => panic!("expected manifest mismatch, got {other:?}"),
        }
        objects
            .manifests
            .insert(manifest, b"terrenia-manifest".to_vec());

        let mut toolchain_host = host.clone();
        toolchain_host.toolchain = CanonicalHash::digest(b"other-toolchain");
        match verify_product_lock(&lock, &objects, &toolchain_host, LockActionMode::Frozen) {
            Err(ProductLockError::ReceiptMismatch {
                receipt: ProductLockReceiptKind::Toolchain,
                ..
            }) => {}
            other => panic!("expected toolchain mismatch, got {other:?}"),
        }

        let mut engine_host = host.clone();
        engine_host.engine_build_id = Some(CanonicalHash::digest(b"other-engine"));
        match verify_product_lock(&lock, &objects, &engine_host, LockActionMode::Frozen) {
            Err(ProductLockError::ReceiptMismatch {
                receipt: ProductLockReceiptKind::EngineBuild,
                ..
            }) => {}
            other => panic!("expected engine-build mismatch, got {other:?}"),
        }

        let artifact = CanonicalHash::digest(b"terrenia-artifact");
        objects
            .artifacts
            .insert(artifact, b"tampered-artifact".to_vec());
        match verify_product_lock(&lock, &objects, &host, LockActionMode::Frozen) {
            Err(ProductLockError::ReceiptMismatch {
                receipt: ProductLockReceiptKind::Artifact,
                ..
            }) => {}
            other => panic!("expected artifact mismatch, got {other:?}"),
        }
        objects
            .artifacts
            .insert(artifact, b"terrenia-artifact".to_vec());

        let mut registration_host = host.clone();
        registration_host.registration_image_hash = CanonicalHash::digest(b"other-registration");
        match verify_product_lock(&lock, &objects, &registration_host, LockActionMode::Frozen) {
            Err(ProductLockError::ReceiptMismatch {
                receipt: ProductLockReceiptKind::Registration,
                ..
            }) => {}
            other => panic!("expected registration mismatch, got {other:?}"),
        }

        let mut alias_lock = lock.clone();
        let importer = package_name("terrenia");
        let alias = succeeded(PackageAlias::new("blocks"));
        alias_lock
            .portable_resolution
            .alias_edges
            .get_mut(&importer)
            .unwrap_or_else(|| panic!("alias table must exist"))
            .insert(
                alias,
                LockedAliasEdgeV1 {
                    package: package_name("@terrenia/blocks"),
                    version: version("9.9.9"),
                    features: BTreeSet::new(),
                },
            );
        alias_lock.product_lock_hash = succeeded(alias_lock.recompute_product_lock_hash());
        match verify_product_lock(&alias_lock, &objects, &host, LockActionMode::Frozen) {
            Err(ProductLockError::AliasMismatch { .. }) => {}
            other => panic!("expected alias mismatch, got {other:?}"),
        }
    }

    #[test]
    fn reopened_lock_projects_package_local_nickel_alias_tables() {
        let (lock, _) = sealed_lock(None);
        let (instances, aliases) = lock.nickel_import_alias_tables();
        let terrenia = package_name("terrenia");
        let blocks = package_name("@terrenia/blocks");
        let alias = succeeded(PackageAlias::new("blocks"));
        assert_eq!(instances.get(&source_id("terrenia")), Some(&terrenia));
        assert_eq!(
            aliases.get(&terrenia).and_then(|edges| edges.get(&alias)),
            Some(&blocks)
        );
        assert!(!aliases.contains_key(&blocks));
    }

    #[test]
    fn independent_receipts_fail_even_when_top_level_hash_is_rewritten() {
        let (mut lock, objects) = sealed_lock(None);
        let host = host_from(&lock);
        let package = lock
            .portable_resolution
            .packages
            .get_mut(&package_name("terrenia"))
            .unwrap_or_else(|| panic!("root package must be sealed"));
        package.manifest_digest = CanonicalHash::digest(b"forged-manifest");
        lock.product_lock_hash = succeeded(lock.recompute_product_lock_hash());
        succeeded(lock.verify_hashes());
        match verify_product_lock(&lock, &objects, &host, LockActionMode::Frozen) {
            Err(
                ProductLockError::MissingReceipt {
                    receipt: ProductLockReceiptKind::Manifest,
                    ..
                }
                | ProductLockError::ReceiptMismatch {
                    receipt: ProductLockReceiptKind::Manifest,
                    ..
                },
            ) => {}
            other => panic!("top-level hash must not hide a manifest mismatch: {other:?}"),
        }
    }

    #[test]
    fn resolution_receipt_is_not_the_product_lock() {
        let (lock, _) = sealed_lock(None);
        let bytes = succeeded(lock.canonical_bytes());
        let encoded: serde_json::Value = succeeded(serde_json::from_slice(&bytes));
        assert!(
            encoded
                .get("portable_resolution")
                .and_then(|value| value.get("resolution_receipt_hash"))
                .is_some(),
            "the lock records the intermediate resolution receipt hash"
        );
        assert_eq!(lock.schema_version, PRODUCT_LOCK_SCHEMA_VERSION);
        assert_ne!(lock.schema_version, LOCK_SCHEMA_VERSION);
        assert_ne!(
            lock.portable_resolution.resolution_receipt_hash,
            lock.product_lock_hash
        );
        assert_ne!(
            lock.realizations
                .values()
                .next()
                .unwrap_or_else(|| panic!("realization missing"))
                .build_intent_hash,
            lock.product_lock_hash
        );
    }

    #[test]
    fn locked_mode_refuses_to_replace_existing_bytes() {
        let directory = TestDirectory::create();
        let (lock, _) = sealed_lock(None);
        succeeded(persist_product_lock(
            persist_path(&directory),
            &lock,
            LockActionMode::Persist,
        ));
        assert_eq!(
            succeeded(persist_product_lock(
                persist_path(&directory),
                &lock,
                LockActionMode::Locked
            )),
            lock
        );
        let (mut other, _) = sealed_lock(None);
        other.explanation_hash = CanonicalHash::digest(b"other-explanation");
        other.product_lock_hash = succeeded(other.recompute_product_lock_hash());
        assert!(matches!(
            persist_product_lock(persist_path(&directory), &other, LockActionMode::Locked),
            Err(ProductLockError::LockBytesChanged)
        ));
    }

    #[test]
    fn source_path_is_not_part_of_the_product_lock() {
        let (lock, _) = sealed_lock(None);
        let encoded = succeeded(serde_json::to_string(&lock));
        assert!(
            !encoded.contains("packages/terrenia"),
            "machine-local source paths must not enter latticeaxiom.lock: {encoded}"
        );
    }
}
