//! Hash-addressed Cargo product workspaces for NativeStatic link closure.
//!
//! This module closes the build half of ADR 0034 without pretending to close
//! runtime activation. It consumes digest-verified serialized SourceSnapshot objects,
//! materializes them under a fresh generated workspace, emits direct
//! registration glue, generates an offline Cargo.lock, and builds a final
//! executable with Cargo --frozen. The receipt explicitly names the remaining
//! declared platform-input boundary.
//!
//! The generated executable is a link-and-registration proof. Until the engine
//! accepts a typed static callback registry and the product-lock schema seals
//! this receipt, callers must not treat it as an activation-ready game client.
//!
//! Cargo and package build scripts run with the producer process's authority.
//! This builder detects persistent mutation of declared workspace inputs and
//! rejects local Cargo paths outside its generated root, but it is not an OS
//! sandbox and cannot prove the absence of external I/O or mutate-then-restore
//! behavior. Callers must admit every selected package at the build trust
//! boundary before invoking it.
#![allow(
    clippy::doc_markdown,
    reason = "NativeStatic and receipt field names are exact protocol vocabulary"
)]

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::{OsStr, OsString};
use std::fmt::Write as _;
use std::fs;
use std::io::{self, Read as _, Write as _};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

use latticeaxiom_core::{
    CanonicalHash, CanonicalJsonError, CanonicalLogicalPath, PackageName, TargetTriple,
    canonical_json_bytes, canonical_json_hash,
};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use unicode_casefold::UnicodeCaseFold as _;
use unicode_normalization::UnicodeNormalization as _;

use crate::SourceSnapshot;

/// Schema version for NativeStatic product build plans and receipts.
pub const NATIVE_STATIC_PRODUCT_BUILD_SCHEMA_VERSION: u32 = 2;

/// Stable generated Cargo package and binary name.
pub const NATIVE_STATIC_PRODUCT_BINARY_NAME: &str = "latticeaxiom-locked-product";

/// Standard package entry called by generated direct-link registration glue.
///
/// A selected package must export a function with this name. Its success value
/// must implement Display and equal the lowercase hexadecimal registration
/// hash sealed in the plan. Its error must also implement Display.
pub const NATIVE_STATIC_REGISTRATION_ENTRY: &str = "native_static_registration_hash";

const PRODUCT_MEMBER: &str = "product";
const SELECTED_PACKAGES_MEMBER: &str = "packages";
const SELECTED_PACKAGE_MEMBER_PREFIX: &str = "selected-";
const BUILD_DIRECTORY: &str = "native-static-product";
const MAXIMUM_DIAGNOSTIC_BYTES: usize = 8 * 1_024;

static NEXT_STAGING_DIRECTORY: AtomicU64 = AtomicU64::new(0);

/// One lock-selected package source object used by the NativeStatic builder.
#[derive(Clone, Debug)]
pub struct NativeStaticPackageSourceInputV1 {
    /// Logical package selected by the product graph.
    pub package: PackageName,
    /// Exact Cargo package name declared by the materialized Cargo.toml.
    pub cargo_package: String,
    /// Runnable source-table identity sealed by portable resolution.
    pub expected_source_digest: CanonicalHash,
    /// CAS payload digest of source_object_bytes.
    pub expected_source_object_digest: CanonicalHash,
    /// Exact serialized SourceSnapshot bytes loaded from CAS.
    pub source_object_bytes: Vec<u8>,
    /// Package registration hash selected for this target.
    pub registration_hash: CanonicalHash,
}

/// One explicitly declared platform source input copied into the build root.
///
/// Platform inputs are hash-receipted but are not claimed to be package CAS
/// objects. A future EngineBuildId transaction must prove that this list is
/// the complete platform compiler closure before attaching the receipt to a
/// final target realization.
#[derive(Clone, Debug)]
pub struct NativeStaticPlatformSourceInputV1 {
    /// Cargo package name declared by this source root.
    pub cargo_package: String,
    /// Canonical generated-workspace path below `crates/` or `packages/`.
    pub workspace_path: CanonicalLogicalPath,
    /// Exact serialized SourceSnapshot bytes captured for the build.
    pub source_object_bytes: Vec<u8>,
}

/// Complete request for one generated NativeStatic link product.
#[derive(Clone, Debug)]
pub struct NativeStaticProductBuildRequestV1 {
    /// Semantic selected graph identity.
    pub graph_hash: CanonicalHash,
    /// Exact compilation target.
    pub target: TargetTriple,
    /// Requested toolchain identity supplied by the transaction.
    pub requested_toolchain: CanonicalHash,
    /// Registration image consumed by generated package glue.
    pub registration_image_hash: CanonicalHash,
    /// Exact selected NativeStatic package inputs.
    pub selected_packages: Vec<NativeStaticPackageSourceInputV1>,
    /// Explicit platform Cargo source closure.
    pub platform_sources: Vec<NativeStaticPlatformSourceInputV1>,
    /// Root Cargo.toml scaffold carrying workspace package/dependency/lint pins.
    pub workspace_scaffold_bytes: Vec<u8>,
    /// Existing Cargo.lock seed. It may be empty for a dependency-free proof.
    pub cargo_lock_seed_bytes: Vec<u8>,
    /// Cargo executable used by the producer.
    pub cargo_program: OsString,
    /// Rust compiler selected for Cargo through RUSTC.
    pub rustc_program: OsString,
}

/// Canonical selected package row in a generated build plan.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct NativeStaticPackageBuildPlanV1 {
    /// Logical selected package.
    pub package: PackageName,
    /// Exact Cargo package name.
    pub cargo_package: String,
    /// Generated-workspace package root.
    pub workspace_path: CanonicalLogicalPath,
    /// Primary crate manifest relative to the package source root.
    pub cargo_manifest: CanonicalLogicalPath,
    /// All declared internal Cargo manifests relative to the package source root.
    pub cargo_members: BTreeSet<CanonicalLogicalPath>,
    /// Verified source-table digest.
    pub source_digest: CanonicalHash,
    /// Verified serialized source-object digest.
    pub source_object_digest: CanonicalHash,
    /// Expected package registration hash.
    pub registration_hash: CanonicalHash,
}

/// Canonical declared platform input row.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct NativeStaticPlatformBuildPlanV1 {
    /// Exact Cargo package name.
    pub cargo_package: String,
    /// Generated-workspace source root.
    pub workspace_path: CanonicalLogicalPath,
    /// Verified source-table digest.
    pub source_digest: CanonicalHash,
    /// Serialized source-object digest.
    pub source_object_digest: CanonicalHash,
}

/// Build producer evidence measured before Cargo runs.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct NativeStaticProducerReceiptV1 {
    /// Receipt schema version.
    pub schema_version: u32,
    /// Stable producer machine.
    pub machine: String,
    /// Exact target requested from Cargo.
    pub target: TargetTriple,
    /// Transaction-selected toolchain identity.
    pub requested_toolchain: CanonicalHash,
    /// Hash of cargo -Vv output.
    pub cargo_version_hash: CanonicalHash,
    /// Hash of rustc -vV output.
    pub rustc_version_hash: CanonicalHash,
    /// Hash-only fingerprint of the inherited producer environment.
    pub environment_fingerprint: CanonicalHash,
    /// Honest environment policy label.
    pub environment_policy: String,
}

/// Canonical generated workspace/build plan.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct NativeStaticProductBuildPlanV1 {
    /// Plan schema version.
    pub schema_version: u32,
    /// Semantic selected graph identity.
    pub graph_hash: CanonicalHash,
    /// Exact compilation target.
    pub target: TargetTriple,
    /// Transaction-selected toolchain identity.
    pub requested_toolchain: CanonicalHash,
    /// Registration image expected by the product.
    pub registration_image_hash: CanonicalHash,
    /// Root workspace scaffold digest.
    pub workspace_scaffold_digest: CanonicalHash,
    /// Seed Cargo.lock digest, including an explicitly empty seed.
    pub cargo_lock_seed_digest: CanonicalHash,
    /// Exact producer receipt identity.
    pub producer_receipt_digest: CanonicalHash,
    /// NativeStatic packages in stable logical-package order.
    pub packages: Vec<NativeStaticPackageBuildPlanV1>,
    /// Explicit platform source inputs in stable workspace-path order.
    pub platform_sources: Vec<NativeStaticPlatformBuildPlanV1>,
    /// Hash of the declared platform source list and workspace/compiler inputs.
    pub declared_platform_build_id: CanonicalHash,
}

/// Receipt for the actual final executable emitted by Cargo.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct NativeStaticProductBuildReceiptV1 {
    /// Receipt schema version.
    pub schema_version: u32,
    /// Canonical build-plan digest.
    pub build_plan_digest: CanonicalHash,
    /// Hash-addressed generated workspace identity.
    pub product_workspace_digest: CanonicalHash,
    /// Generated Cargo.lock payload digest.
    pub cargo_lock_digest: CanonicalHash,
    /// Producer receipt payload digest.
    pub producer_receipt_digest: CanonicalHash,
    /// Final executable payload digest.
    pub executable_digest: CanonicalHash,
    /// Exact compilation target.
    pub target: TargetTriple,
    /// Requested toolchain identity.
    pub requested_toolchain: CanonicalHash,
    /// Declared platform input identity.
    pub declared_platform_build_id: CanonicalHash,
    /// Registration image identity compiled into the plan.
    pub registration_image_hash: CanonicalHash,
}

/// Build output ready for publication into typed CAS object kinds.
#[derive(Clone, Debug)]
pub struct NativeStaticProductBuildOutputV1 {
    /// Canonical plan.
    pub plan: NativeStaticProductBuildPlanV1,
    /// Exact canonical plan bytes.
    pub plan_bytes: Vec<u8>,
    /// Canonical producer receipt.
    pub producer_receipt: NativeStaticProducerReceiptV1,
    /// Exact canonical producer receipt bytes.
    pub producer_receipt_bytes: Vec<u8>,
    /// Generated exact Cargo.lock bytes.
    pub cargo_lock_bytes: Vec<u8>,
    /// Actual final executable bytes.
    pub executable_bytes: Vec<u8>,
    /// Canonical final build receipt.
    pub receipt: NativeStaticProductBuildReceiptV1,
    /// Hash-addressed generated workspace retained for diagnostics.
    pub workspace_root: PathBuf,
    /// Actual executable path inside workspace_root.
    pub executable_path: PathBuf,
}

/// NativeStatic workspace generation or build failure.
#[derive(Debug, Error)]
pub enum NativeStaticProductBuildError {
    /// Request or generated closure is structurally invalid.
    #[error("invalid NativeStatic product build request: {reason}")]
    InvalidRequest {
        /// Stable structural reason.
        reason: String,
    },
    /// A source object failed its locked digest or snapshot proof.
    #[error("NativeStatic source proof failed for {owner}: {reason}")]
    SourceProof {
        /// Package or platform owner.
        owner: String,
        /// Stable failure detail.
        reason: String,
    },
    /// Canonical JSON encoding failed.
    #[error(transparent)]
    Canonical(#[from] CanonicalJsonError),
    /// A source object was not valid verified JSON.
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    /// Cargo TOML input was invalid.
    #[error(transparent)]
    TomlDecode(#[from] toml::de::Error),
    /// Filesystem materialization failed.
    #[error("NativeStatic product I/O failed at {path}: {source}")]
    Io {
        /// Failed path.
        path: PathBuf,
        /// Underlying I/O failure.
        #[source]
        source: io::Error,
    },
    /// Cargo or rustc could not be started.
    #[error("NativeStatic producer {program} could not start: {source}")]
    ProducerSpawn {
        /// Stable program label.
        program: String,
        /// Underlying process failure.
        #[source]
        source: io::Error,
    },
    /// Cargo or rustc returned failure.
    #[error("NativeStatic producer {program} failed: {diagnostic}")]
    ProducerFailed {
        /// Stable program label.
        program: String,
        /// Bounded diagnostic.
        diagnostic: String,
    },
    /// Cargo emitted no final executable for the generated product package.
    #[error("Cargo emitted no {NATIVE_STATIC_PRODUCT_BINARY_NAME} executable")]
    MissingExecutable,
    /// A previously published cache entry did not match a fresh build of the
    /// same hash-addressed workspace.
    #[error("NativeStatic product cache conflict at {path}: {reason}")]
    CacheConflict {
        /// Existing cache entry that was preserved for diagnosis.
        path: PathBuf,
        /// Stable validation failure.
        reason: String,
    },
}

impl NativeStaticProductBuildError {
    fn invalid(reason: impl Into<String>) -> Self {
        Self::InvalidRequest {
            reason: reason.into(),
        }
    }

    fn source(owner: impl Into<String>, reason: impl Into<String>) -> Self {
        Self::SourceProof {
            owner: owner.into(),
            reason: reason.into(),
        }
    }

    fn io(path: impl Into<PathBuf>, source: io::Error) -> Self {
        Self::Io {
            path: path.into(),
            source,
        }
    }

    fn cache_conflict(path: impl Into<PathBuf>, reason: impl Into<String>) -> Self {
        Self::CacheConflict {
            path: path.into(),
            reason: reason.into(),
        }
    }
}

/// A direct child created by this invocation and removed on every unpublished
/// exit. Cleanup deliberately refuses links, reparse points, special files, or
/// a path whose resolved parent no longer equals the cache root.
#[derive(Debug)]
struct OwnedStagingDirectory {
    cache_root: PathBuf,
    path: PathBuf,
    published: bool,
}

impl OwnedStagingDirectory {
    fn create(cache_root: &Path) -> Result<Self, NativeStaticProductBuildError> {
        const MAXIMUM_CREATE_ATTEMPTS: usize = 1_024;

        let metadata = fs::symlink_metadata(cache_root)
            .map_err(|source| NativeStaticProductBuildError::io(cache_root, source))?;
        if !metadata.is_dir() || is_link_or_reparse(&metadata) {
            return Err(NativeStaticProductBuildError::invalid(format!(
                "NativeStatic staging parent is not a regular unlinked directory: {}",
                cache_root.display()
            )));
        }

        for _ in 0..MAXIMUM_CREATE_ATTEMPTS {
            let sequence = NEXT_STAGING_DIRECTORY.fetch_add(1, Ordering::Relaxed);
            let name = format!(".staging-{}-{sequence}", std::process::id());
            if Path::new(&name).components().count() != 1 {
                return Err(NativeStaticProductBuildError::invalid(
                    "generated staging directory name is unsafe",
                ));
            }
            let path = cache_root.join(name);
            match fs::create_dir(&path) {
                Ok(()) => {
                    return Ok(Self {
                        cache_root: cache_root.to_path_buf(),
                        path,
                        published: false,
                    });
                }
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                Err(error) => {
                    return Err(NativeStaticProductBuildError::io(&path, error));
                }
            }
        }
        Err(NativeStaticProductBuildError::invalid(format!(
            "NativeStatic could not allocate a unique staging directory below {}",
            cache_root.display()
        )))
    }

    fn path(&self) -> &Path {
        &self.path
    }

    fn mark_published(&mut self) {
        self.published = true;
    }
}

impl Drop for OwnedStagingDirectory {
    fn drop(&mut self) {
        if self.published || !safe_owned_staging_tree(&self.cache_root, &self.path) {
            return;
        }
        let _ignored = fs::remove_dir_all(&self.path);
    }
}

/// Generates, locks, and builds one actual NativeStatic product executable.
///
/// Selected package bytes are decoded only from source_object_bytes, whose
/// payload and source-table digests are independently checked. The original
/// acquisition path is neither accepted by this API nor consulted.
///
/// # Errors
///
/// Returns NativeStaticProductBuildError for invalid selections, source proof
/// failures, unsafe materialization paths, Cargo producer failure, or a missing
/// final executable.
#[allow(
    clippy::too_many_lines,
    reason = "the fail-closed build, verify, and publish transaction is kept in one visible order"
)]
pub fn build_native_static_product(
    request: &NativeStaticProductBuildRequestV1,
    build_cache_root: impl AsRef<Path>,
) -> Result<NativeStaticProductBuildOutputV1, NativeStaticProductBuildError> {
    if request.selected_packages.is_empty() {
        return Err(NativeStaticProductBuildError::invalid(
            "at least one NativeStatic package is required",
        ));
    }

    let selected = validate_selected_packages(&request.selected_packages)?;
    let platforms = validate_platform_sources(&request.platform_sources)?;
    validate_workspace_paths(&selected, &platforms)?;

    let scaffold_digest = CanonicalHash::digest(&request.workspace_scaffold_bytes);
    let cargo_lock_seed_digest = CanonicalHash::digest(&request.cargo_lock_seed_bytes);
    let producer_receipt = producer_receipt(request)?;
    let producer_receipt_bytes = canonical_json_bytes(&producer_receipt)?;
    let producer_receipt_digest = CanonicalHash::digest(&producer_receipt_bytes);
    let declared_platform_build_id = canonical_json_hash(&DeclaredPlatformBuildIdentity {
        target: &request.target,
        requested_toolchain: request.requested_toolchain,
        workspace_scaffold_digest: scaffold_digest,
        cargo_lock_seed_digest,
        platform_sources: &platforms.plans,
    })?;

    let plan = NativeStaticProductBuildPlanV1 {
        schema_version: NATIVE_STATIC_PRODUCT_BUILD_SCHEMA_VERSION,
        graph_hash: request.graph_hash,
        target: request.target.clone(),
        requested_toolchain: request.requested_toolchain,
        registration_image_hash: request.registration_image_hash,
        workspace_scaffold_digest: scaffold_digest,
        cargo_lock_seed_digest,
        producer_receipt_digest,
        packages: selected.plans.clone(),
        platform_sources: platforms.plans.clone(),
        declared_platform_build_id,
    };
    let plan_bytes = canonical_json_bytes(&plan)?;
    let build_plan_digest = CanonicalHash::digest(&plan_bytes);
    let cache_root = build_cache_root.as_ref().join(BUILD_DIRECTORY);
    fs::create_dir_all(&cache_root)
        .map_err(|source| NativeStaticProductBuildError::io(&cache_root, source))?;
    let cache_root = fs::canonicalize(&cache_root)
        .map_err(|source| NativeStaticProductBuildError::io(&cache_root, source))?;
    let mut staging = OwnedStagingDirectory::create(&cache_root)?;

    materialize_workspace(
        staging.path(),
        &request.workspace_scaffold_bytes,
        &request.cargo_lock_seed_bytes,
        &selected,
        &platforms,
    )?;
    generate_cargo_lock(request, staging.path())?;
    let product_package_id = verify_cargo_metadata_containment(request, staging.path())?;
    let generated_lock_path = staging.path().join("Cargo.lock");
    let cargo_lock_bytes = fs::read(&generated_lock_path)
        .map_err(|source| NativeStaticProductBuildError::io(&generated_lock_path, source))?;
    let cargo_lock_digest = CanonicalHash::digest(&cargo_lock_bytes);
    let expected_workspace_inputs = collect_declared_workspace_files(staging.path())?;
    let staged_executable_path =
        build_product_executable(request, staging.path(), &product_package_id)?;
    let (executable_relative_path, staged_executable_bytes) =
        read_validated_executable(staging.path(), &staged_executable_path)?;
    verify_materialized_inputs(staging.path(), &selected, &platforms)?;
    verify_declared_workspace_inputs(staging.path(), &expected_workspace_inputs)?;
    let staged_executable_digest = CanonicalHash::digest(&staged_executable_bytes);
    let product_workspace_digest = canonical_json_hash(&ProductWorkspaceIdentity {
        build_plan: build_plan_digest,
        cargo_lock: cargo_lock_digest,
        executable: staged_executable_digest,
    })?;

    let workspace_root = cache_root.join(product_workspace_digest.to_string());
    let (executable_path, executable_bytes) = publish_or_reuse_workspace(
        &mut staging,
        &workspace_root,
        &executable_relative_path,
        staged_executable_bytes,
        &expected_workspace_inputs,
        &selected,
        &platforms,
    )?;
    let executable_digest = CanonicalHash::digest(&executable_bytes);
    let receipt = NativeStaticProductBuildReceiptV1 {
        schema_version: NATIVE_STATIC_PRODUCT_BUILD_SCHEMA_VERSION,
        build_plan_digest,
        product_workspace_digest,
        cargo_lock_digest,
        producer_receipt_digest,
        executable_digest,
        target: request.target.clone(),
        requested_toolchain: request.requested_toolchain,
        declared_platform_build_id,
        registration_image_hash: request.registration_image_hash,
    };

    Ok(NativeStaticProductBuildOutputV1 {
        plan,
        plan_bytes,
        producer_receipt,
        producer_receipt_bytes,
        cargo_lock_bytes,
        executable_bytes,
        receipt,
        workspace_root,
        executable_path,
    })
}

#[derive(Serialize)]
struct DeclaredPlatformBuildIdentity<'a> {
    target: &'a TargetTriple,
    requested_toolchain: CanonicalHash,
    workspace_scaffold_digest: CanonicalHash,
    cargo_lock_seed_digest: CanonicalHash,
    platform_sources: &'a [NativeStaticPlatformBuildPlanV1],
}

#[derive(Serialize)]
struct ProductWorkspaceIdentity {
    build_plan: CanonicalHash,
    cargo_lock: CanonicalHash,
    executable: CanonicalHash,
}

struct VerifiedSelectedPackages {
    plans: Vec<NativeStaticPackageBuildPlanV1>,
    snapshots: BTreeMap<CanonicalLogicalPath, SourceSnapshot>,
}

struct VerifiedPlatformSources {
    plans: Vec<NativeStaticPlatformBuildPlanV1>,
    snapshots: BTreeMap<CanonicalLogicalPath, SourceSnapshot>,
}

fn validate_selected_packages(
    inputs: &[NativeStaticPackageSourceInputV1],
) -> Result<VerifiedSelectedPackages, NativeStaticProductBuildError> {
    let mut ordered = BTreeMap::new();
    for input in inputs {
        if ordered.insert(input.package.clone(), input).is_some() {
            return Err(NativeStaticProductBuildError::invalid(format!(
                "selected package {} is duplicated",
                input.package
            )));
        }
    }
    let mut plans = Vec::with_capacity(ordered.len());
    let mut snapshots = BTreeMap::new();
    let mut cargo_packages = BTreeSet::new();
    for (index, (package, input)) in ordered.into_iter().enumerate() {
        validate_cargo_package_name(&input.cargo_package)?;
        if !cargo_packages.insert(input.cargo_package.clone()) {
            return Err(NativeStaticProductBuildError::invalid(format!(
                "Cargo package {} is selected more than once",
                input.cargo_package
            )));
        }
        let snapshot = decode_source_object(
            package.to_string(),
            &input.source_object_bytes,
            input.expected_source_object_digest,
            Some(input.expected_source_digest),
        )?;
        let (cargo_manifest, cargo_members) = selected_cargo_sources(&snapshot)?;
        verify_cargo_manifest(
            package.as_str(),
            &input.cargo_package,
            &snapshot,
            cargo_manifest.as_str(),
        )?;
        let workspace_path: CanonicalLogicalPath =
            format!("{SELECTED_PACKAGES_MEMBER}/{SELECTED_PACKAGE_MEMBER_PREFIX}{index:04}")
                .parse()
                .map_err(|error| {
                    NativeStaticProductBuildError::invalid(format!(
                        "generated package path is invalid: {error}"
                    ))
                })?;
        plans.push(NativeStaticPackageBuildPlanV1 {
            package: package.clone(),
            cargo_package: input.cargo_package.clone(),
            workspace_path: workspace_path.clone(),
            cargo_manifest,
            cargo_members,
            source_digest: snapshot.source_hash(),
            source_object_digest: input.expected_source_object_digest,
            registration_hash: input.registration_hash,
        });
        snapshots.insert(workspace_path, snapshot);
    }
    Ok(VerifiedSelectedPackages { plans, snapshots })
}

fn validate_platform_sources(
    inputs: &[NativeStaticPlatformSourceInputV1],
) -> Result<VerifiedPlatformSources, NativeStaticProductBuildError> {
    let mut ordered = BTreeMap::new();
    let mut cargo_packages = BTreeSet::new();
    for input in inputs {
        validate_cargo_package_name(&input.cargo_package)?;
        validate_platform_workspace_path(&input.workspace_path)?;
        if ordered
            .insert(input.workspace_path.clone(), input)
            .is_some()
        {
            return Err(NativeStaticProductBuildError::invalid(format!(
                "platform workspace path {} is duplicated",
                input.workspace_path
            )));
        }
        if !cargo_packages.insert(input.cargo_package.clone()) {
            return Err(NativeStaticProductBuildError::invalid(format!(
                "platform Cargo package {} is duplicated",
                input.cargo_package
            )));
        }
    }
    let mut plans = Vec::with_capacity(ordered.len());
    let mut snapshots = BTreeMap::new();
    for (workspace_path, input) in ordered {
        let object_digest = CanonicalHash::digest(&input.source_object_bytes);
        let snapshot = decode_source_object(
            input.cargo_package.clone(),
            &input.source_object_bytes,
            object_digest,
            None,
        )?;
        verify_cargo_root(&input.cargo_package, &input.cargo_package, &snapshot)?;
        plans.push(NativeStaticPlatformBuildPlanV1 {
            cargo_package: input.cargo_package.clone(),
            workspace_path: workspace_path.clone(),
            source_digest: snapshot.source_hash(),
            source_object_digest: object_digest,
        });
        snapshots.insert(workspace_path, snapshot);
    }
    Ok(VerifiedPlatformSources { plans, snapshots })
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PlatformWorkspaceMemberRoot {
    Crates,
    Packages,
}

impl PlatformWorkspaceMemberRoot {
    const ALL: [Self; 2] = [Self::Crates, Self::Packages];

    const fn as_str(self) -> &'static str {
        match self {
            Self::Crates => "crates",
            Self::Packages => SELECTED_PACKAGES_MEMBER,
        }
    }

    fn containing(path: &CanonicalLogicalPath) -> Option<Self> {
        let (root, _relative) = path.as_str().split_once('/')?;
        Self::ALL
            .into_iter()
            .find(|candidate| candidate.as_str() == root)
    }
}

fn validate_platform_workspace_path(
    path: &CanonicalLogicalPath,
) -> Result<(), NativeStaticProductBuildError> {
    let Some(root) = PlatformWorkspaceMemberRoot::containing(path) else {
        return Err(NativeStaticProductBuildError::invalid(format!(
            "platform source {path} must materialize below crates/ or packages/"
        )));
    };
    if root == PlatformWorkspaceMemberRoot::Packages
        && path.as_str().split('/').nth(1).is_some_and(|member| {
            member
                .get(..SELECTED_PACKAGE_MEMBER_PREFIX.len())
                .is_some_and(|prefix| prefix.eq_ignore_ascii_case(SELECTED_PACKAGE_MEMBER_PREFIX))
        })
    {
        return Err(NativeStaticProductBuildError::invalid(format!(
            "platform source {path} overlaps the reserved selected-package namespace"
        )));
    }
    Ok(())
}

fn validate_workspace_paths(
    selected: &VerifiedSelectedPackages,
    platforms: &VerifiedPlatformSources,
) -> Result<(), NativeStaticProductBuildError> {
    let mut paths = vec![PRODUCT_MEMBER.to_owned()];
    paths.extend(
        selected
            .snapshots
            .keys()
            .chain(platforms.snapshots.keys())
            .map(ToString::to_string),
    );
    let folded = paths
        .iter()
        .map(|path| path.case_fold().nfc().collect::<String>())
        .collect::<Vec<_>>();
    for first in 0..paths.len() {
        for second in (first + 1)..paths.len() {
            if folded[first] == folded[second]
                || folded[first].starts_with(&format!("{}/", folded[second]))
                || folded[second].starts_with(&format!("{}/", folded[first]))
            {
                return Err(NativeStaticProductBuildError::invalid(format!(
                    "generated workspace paths {} and {} overlap or collide after case folding",
                    paths[first], paths[second]
                )));
            }
        }
    }
    Ok(())
}

fn decode_source_object(
    owner: String,
    bytes: &[u8],
    expected_object_digest: CanonicalHash,
    expected_source_digest: Option<CanonicalHash>,
) -> Result<SourceSnapshot, NativeStaticProductBuildError> {
    let actual_object_digest = CanonicalHash::digest(bytes);
    if actual_object_digest != expected_object_digest {
        return Err(NativeStaticProductBuildError::source(
            owner,
            format!(
                "source object digest expected {expected_object_digest}, found {actual_object_digest}"
            ),
        ));
    }
    let snapshot: SourceSnapshot = serde_json::from_slice(bytes)?;
    let canonical = canonical_json_bytes(&snapshot)?;
    if canonical.as_slice() != bytes {
        return Err(NativeStaticProductBuildError::source(
            owner,
            "source object is not canonical JSON",
        ));
    }
    if let Some(expected) = expected_source_digest
        && snapshot.source_hash() != expected
    {
        return Err(NativeStaticProductBuildError::source(
            owner,
            format!(
                "source-table digest expected {expected}, found {}",
                snapshot.source_hash()
            ),
        ));
    }
    Ok(snapshot)
}

fn verify_cargo_root(
    owner: &str,
    expected_cargo_package: &str,
    snapshot: &SourceSnapshot,
) -> Result<(), NativeStaticProductBuildError> {
    verify_cargo_manifest(owner, expected_cargo_package, snapshot, "Cargo.toml")
}

fn selected_cargo_sources(
    snapshot: &SourceSnapshot,
) -> Result<(CanonicalLogicalPath, BTreeSet<CanonicalLogicalPath>), NativeStaticProductBuildError> {
    if let Ok(file) = snapshot.resolve_path("latticeaxiom-package.toml") {
        let text = std::str::from_utf8(file.bytes())
            .map_err(|error| NativeStaticProductBuildError::invalid(error.to_string()))?;
        let manifest = crate::PackageSourceManifestV1::from_toml_str(text)
            .map_err(|error| NativeStaticProductBuildError::invalid(error.to_string()))?;
        if let Some(rust) = manifest.rust {
            for member in &rust.members {
                snapshot.resolve_path(member.as_str()).map_err(|error| {
                    NativeStaticProductBuildError::invalid(format!(
                        "declared Rust member {member} is missing: {error}"
                    ))
                })?;
            }
            return Ok((rust.entry, rust.members));
        }
    }
    let entry = "Cargo.toml"
        .parse::<CanonicalLogicalPath>()
        .map_err(|error| NativeStaticProductBuildError::invalid(error.to_string()))?;
    Ok((entry.clone(), BTreeSet::from([entry])))
}

fn verify_cargo_manifest(
    owner: &str,
    expected_cargo_package: &str,
    snapshot: &SourceSnapshot,
    entry: &str,
) -> Result<(), NativeStaticProductBuildError> {
    let prefix = entry.strip_suffix("Cargo.toml").unwrap_or_default();
    let manifest = snapshot.resolve_path(entry).map_err(|error| {
        NativeStaticProductBuildError::source(owner, format!("Cargo.toml is missing: {error}"))
    })?;
    if !snapshot
        .files()
        .keys()
        .any(|logical_path| logical_path.starts_with(&format!("{prefix}src/")))
    {
        return Err(NativeStaticProductBuildError::source(
            owner,
            "package contains no src/ files",
        ));
    }
    let text = std::str::from_utf8(manifest.bytes()).map_err(|error| {
        NativeStaticProductBuildError::source(owner, format!("Cargo.toml is not UTF-8: {error}"))
    })?;
    let value: toml::Value = toml::from_str(text)?;
    let actual = value
        .get("package")
        .and_then(|package| package.get("name"))
        .and_then(toml::Value::as_str)
        .ok_or_else(|| {
            NativeStaticProductBuildError::source(owner, "Cargo.toml has no package.name")
        })?;
    if actual != expected_cargo_package {
        return Err(NativeStaticProductBuildError::source(
            owner,
            format!("Cargo.toml package.name is {actual}, expected {expected_cargo_package}"),
        ));
    }
    Ok(())
}

fn validate_cargo_package_name(value: &str) -> Result<(), NativeStaticProductBuildError> {
    let mut bytes = value.bytes();
    let Some(first) = bytes.next() else {
        return Err(NativeStaticProductBuildError::invalid(
            "Cargo package name is empty",
        ));
    };
    if !first.is_ascii_alphanumeric()
        || !bytes.all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return Err(NativeStaticProductBuildError::invalid(format!(
            "Cargo package name {value} is outside the generated-root vocabulary"
        )));
    }
    Ok(())
}

fn producer_receipt(
    request: &NativeStaticProductBuildRequestV1,
) -> Result<NativeStaticProducerReceiptV1, NativeStaticProductBuildError> {
    Ok(NativeStaticProducerReceiptV1 {
        schema_version: NATIVE_STATIC_PRODUCT_BUILD_SCHEMA_VERSION,
        machine: "latticeaxiom".to_owned(),
        target: request.target.clone(),
        requested_toolchain: request.requested_toolchain,
        cargo_version_hash: command_version_hash(&request.cargo_program, "-Vv")?,
        rustc_version_hash: command_version_hash(&request.rustc_program, "-vV")?,
        environment_fingerprint: environment_fingerprint()?,
        environment_policy: "unsandboxed-build-trusted-ambient-hash-receipted-v1".to_owned(),
    })
}

fn environment_fingerprint() -> Result<CanonicalHash, NativeStaticProductBuildError> {
    let environment = std::env::vars_os()
        .map(|(name, value)| {
            (
                name.to_string_lossy().into_owned(),
                CanonicalHash::digest(value.to_string_lossy().as_bytes()),
            )
        })
        .collect::<BTreeMap<_, _>>();
    Ok(canonical_json_hash(&environment)?)
}

fn command_version_hash(
    program: &OsStr,
    flag: &str,
) -> Result<CanonicalHash, NativeStaticProductBuildError> {
    let output = Command::new(program).arg(flag).output().map_err(|source| {
        NativeStaticProductBuildError::ProducerSpawn {
            program: program.to_string_lossy().into_owned(),
            source,
        }
    })?;
    require_success(program, output).map(|output| CanonicalHash::digest(output.stdout))
}

fn materialize_workspace(
    root: &Path,
    scaffold_bytes: &[u8],
    lock_seed: &[u8],
    selected: &VerifiedSelectedPackages,
    platforms: &VerifiedPlatformSources,
) -> Result<(), NativeStaticProductBuildError> {
    let manifest = generated_workspace_manifest(scaffold_bytes, selected, platforms)?;
    write_file(&root.join("Cargo.toml"), manifest.as_bytes())?;
    if !lock_seed.is_empty() {
        write_file(&root.join("Cargo.lock"), lock_seed)?;
    }
    for (path, snapshot) in selected.snapshots.iter().chain(platforms.snapshots.iter()) {
        materialize_snapshot(&root.join(logical_path(path)), snapshot)?;
    }
    let product_root = root.join(PRODUCT_MEMBER);
    write_file(
        &product_root.join("Cargo.toml"),
        generated_product_manifest(&selected.plans).as_bytes(),
    )?;
    write_file(
        &product_root.join("src").join("main.rs"),
        generated_product_main(&selected.plans).as_bytes(),
    )?;
    Ok(())
}

fn generated_workspace_manifest(
    scaffold_bytes: &[u8],
    selected: &VerifiedSelectedPackages,
    platforms: &VerifiedPlatformSources,
) -> Result<String, NativeStaticProductBuildError> {
    let text = std::str::from_utf8(scaffold_bytes).map_err(|error| {
        NativeStaticProductBuildError::invalid(format!("workspace scaffold is not UTF-8: {error}"))
    })?;
    let document: toml::Value = toml::from_str(text)?;
    let workspace = document
        .get("workspace")
        .and_then(toml::Value::as_table)
        .ok_or_else(|| {
            NativeStaticProductBuildError::invalid("workspace scaffold has no [workspace] table")
        })?;
    for key in workspace.keys() {
        if !matches!(
            key.as_str(),
            "resolver"
                | "members"
                | "default-members"
                | "exclude"
                | "package"
                | "dependencies"
                | "lints"
                | "metadata"
        ) {
            return Err(NativeStaticProductBuildError::invalid(format!(
                "workspace scaffold key workspace.{key} cannot be preserved by schema v1"
            )));
        }
    }
    let resolver = workspace
        .get("resolver")
        .and_then(toml::Value::as_str)
        .unwrap_or("3");
    if resolver != "3" {
        return Err(NativeStaticProductBuildError::invalid(format!(
            "workspace resolver {resolver} is unsupported; resolver 3 is required"
        )));
    }
    let lines = text.lines().collect::<Vec<_>>();
    let workspace_line = lines
        .iter()
        .position(|line| line.trim() == "[workspace]")
        .ok_or_else(|| {
            NativeStaticProductBuildError::invalid("workspace table header is missing")
        })?;
    let suffix_line = lines
        .iter()
        .enumerate()
        .skip(workspace_line + 1)
        .find_map(|(index, line)| line.trim_start().starts_with('[').then_some(index));
    let suffix = suffix_line.map_or(String::new(), |index| lines[index..].join("\n"));

    let mut members = vec![PRODUCT_MEMBER.to_owned()];
    members.extend(selected.plans.iter().flat_map(|row| {
        row.cargo_members.iter().map(|manifest| {
            format!(
                "{}/{}",
                row.workspace_path,
                manifest.as_str().trim_end_matches("/Cargo.toml")
            )
            .trim_end_matches("/Cargo.toml")
            .to_owned()
        })
    }));
    members.extend(
        platforms
            .plans
            .iter()
            .map(|row| row.workspace_path.to_string()),
    );
    let mut generated = String::from("[workspace]\nresolver = \"3\"\nmembers = [\n");
    for member in members {
        let _ = writeln!(generated, "  \"{member}\",");
    }
    generated.push_str("]\n");
    if !suffix.is_empty() {
        generated.push('\n');
        generated.push_str(&suffix);
        generated.push('\n');
    }
    Ok(generated)
}

fn generated_product_manifest(packages: &[NativeStaticPackageBuildPlanV1]) -> String {
    let mut manifest = String::from(
        "[package]\nname = \"latticeaxiom-locked-product\"\nversion.workspace = true\nedition.workspace = true\nrust-version.workspace = true\npublish.workspace = true\nlicense.workspace = true\nrepository.workspace = true\n\n[[bin]]\nname = \"latticeaxiom-locked-product\"\npath = \"src/main.rs\"\n\n[dependencies]\n",
    );
    for (index, package) in packages.iter().enumerate() {
        let _ = writeln!(
            manifest,
            "latticeaxiom_product_package_{index:04} = {{ package = \"{}\", path = \"../{}\" }}",
            package.cargo_package,
            format!(
                "{}/{}",
                package.workspace_path,
                package.cargo_manifest.as_str()
            )
            .trim_end_matches("/Cargo.toml")
        );
    }
    manifest.push_str("\n[lints]\nworkspace = true\n");
    manifest
}

fn generated_product_main(packages: &[NativeStaticPackageBuildPlanV1]) -> String {
    let mut source = String::from("use std::process::ExitCode;\n\nfn main() -> ExitCode {\n");
    for (index, package) in packages.iter().enumerate() {
        let _ = writeln!(
            source,
            "    let expected_{index:04} = \"{}\";",
            package.registration_hash
        );
        let _ = writeln!(
            source,
            "    let actual_{index:04} = match latticeaxiom_product_package_{index:04}::{NATIVE_STATIC_REGISTRATION_ENTRY}() {{"
        );
        let _ = writeln!(
            source,
            "        Ok(value) => value.to_string(),\n        Err(error) => {{ eprintln!(\"static registration failed for {}: {{error}}\"); return ExitCode::FAILURE; }}\n    }};",
            package.package
        );
        let _ = writeln!(
            source,
            "    if actual_{index:04} != expected_{index:04} {{ eprintln!(\"static registration hash mismatch for {}\"); return ExitCode::FAILURE; }}",
            package.package
        );
    }
    source.push_str("    ExitCode::SUCCESS\n}\n");
    source
}

fn generate_cargo_lock(
    request: &NativeStaticProductBuildRequestV1,
    root: &Path,
) -> Result<(), NativeStaticProductBuildError> {
    let output = cargo_command(request, root)
        .arg("generate-lockfile")
        .arg("--offline")
        .arg("--manifest-path")
        .arg(root.join("Cargo.toml"))
        .output()
        .map_err(|source| NativeStaticProductBuildError::ProducerSpawn {
            program: request.cargo_program.to_string_lossy().into_owned(),
            source,
        })?;
    require_success(&request.cargo_program, output)?;
    Ok(())
}

fn verify_cargo_metadata_containment(
    request: &NativeStaticProductBuildRequestV1,
    root: &Path,
) -> Result<String, NativeStaticProductBuildError> {
    let output = cargo_command(request, root)
        .arg("metadata")
        .arg("--format-version=1")
        .arg("--locked")
        .arg("--offline")
        .arg("--manifest-path")
        .arg(root.join("Cargo.toml"))
        .output()
        .map_err(|source| NativeStaticProductBuildError::ProducerSpawn {
            program: request.cargo_program.to_string_lossy().into_owned(),
            source,
        })?;
    let output = require_success(&request.cargo_program, output)?;
    let metadata: cargo_metadata::Metadata = serde_json::from_slice(&output.stdout)?;
    let canonical_root =
        fs::canonicalize(root).map_err(|source| NativeStaticProductBuildError::io(root, source))?;
    let workspace_root = metadata.workspace_root.as_std_path();
    let canonical_workspace_root = fs::canonicalize(workspace_root)
        .map_err(|source| NativeStaticProductBuildError::io(workspace_root, source))?;
    if canonical_workspace_root != canonical_root {
        return Err(NativeStaticProductBuildError::invalid(format!(
            "Cargo metadata workspace root {} escaped generated root {}",
            canonical_workspace_root.display(),
            canonical_root.display()
        )));
    }

    let product_manifest =
        fs::canonicalize(root.join(PRODUCT_MEMBER).join("Cargo.toml")).map_err(|source| {
            NativeStaticProductBuildError::io(root.join(PRODUCT_MEMBER).join("Cargo.toml"), source)
        })?;
    let mut product_package_id = None;
    for package in metadata.packages {
        let manifest_path = package.manifest_path.as_std_path();
        let manifest = fs::canonicalize(manifest_path)
            .map_err(|source| NativeStaticProductBuildError::io(manifest_path, source))?;
        if package.source.is_none() {
            require_path_below_generated_root(&canonical_root, &manifest, "Cargo manifest")?;
            for target in &package.targets {
                let target_source = target.src_path.as_std_path();
                let source = fs::canonicalize(target_source)
                    .map_err(|error| NativeStaticProductBuildError::io(target_source, error))?;
                require_path_below_generated_root(&canonical_root, &source, "Cargo target source")?;
            }
        }
        if package.name == NATIVE_STATIC_PRODUCT_BINARY_NAME
            && manifest == product_manifest
            && product_package_id.replace(package.id.repr).is_some()
        {
            return Err(NativeStaticProductBuildError::invalid(
                "Cargo metadata emitted the generated product package more than once",
            ));
        }
    }
    product_package_id.ok_or_else(|| {
        NativeStaticProductBuildError::invalid(
            "Cargo metadata omitted the generated product package",
        )
    })
}

fn require_path_below_generated_root(
    root: &Path,
    path: &Path,
    label: &str,
) -> Result<(), NativeStaticProductBuildError> {
    if path == root || path.starts_with(root) {
        Ok(())
    } else {
        Err(NativeStaticProductBuildError::invalid(format!(
            "{label} {} escaped generated root {}",
            path.display(),
            root.display()
        )))
    }
}

fn build_product_executable(
    request: &NativeStaticProductBuildRequestV1,
    root: &Path,
    product_package_id: &str,
) -> Result<PathBuf, NativeStaticProductBuildError> {
    let target_dir = root.join(".target");
    let output = cargo_command(request, root)
        .arg("build")
        .arg("--manifest-path")
        .arg(root.join("Cargo.toml"))
        .arg("--package")
        .arg(NATIVE_STATIC_PRODUCT_BINARY_NAME)
        .arg("--release")
        .arg("--frozen")
        .arg("--target")
        .arg(request.target.as_str())
        .arg("--target-dir")
        .arg(&target_dir)
        .arg("--message-format=json-render-diagnostics")
        .output()
        .map_err(|source| NativeStaticProductBuildError::ProducerSpawn {
            program: request.cargo_program.to_string_lossy().into_owned(),
            source,
        })?;
    let output = require_success(&request.cargo_program, output)?;
    let mut executables = cargo_metadata::Message::parse_stream(io::Cursor::new(&output.stdout))
        .filter_map(Result::ok)
        .filter_map(|message| {
            let cargo_metadata::Message::CompilerArtifact(artifact) = message else {
                return None;
            };
            (artifact.package_id.repr == product_package_id
                && artifact.target.name == NATIVE_STATIC_PRODUCT_BINARY_NAME)
                .then_some(artifact)
        })
        .filter_map(|artifact| {
            (artifact
                .target
                .kind
                .contains(&cargo_metadata::TargetKind::Bin)
                && artifact
                    .target
                    .crate_types
                    .contains(&cargo_metadata::CrateType::Bin))
            .then_some(artifact.executable)
            .flatten()
        });
    let executable = executables
        .next()
        .ok_or(NativeStaticProductBuildError::MissingExecutable)?;
    if executables.next().is_some() {
        return Err(NativeStaticProductBuildError::invalid(
            "Cargo emitted multiple final product executables",
        ));
    }
    Ok(executable.into_std_path_buf())
}

fn cargo_command(request: &NativeStaticProductBuildRequestV1, root: &Path) -> Command {
    let mut command = Command::new(&request.cargo_program);
    command
        .current_dir(root)
        .env("RUSTC", &request.rustc_program)
        .env("CARGO_TARGET_DIR", root.join(".target"))
        .env("CARGO_NET_OFFLINE", "true");
    command
}

fn require_success(
    program: &OsStr,
    output: Output,
) -> Result<Output, NativeStaticProductBuildError> {
    if output.status.success() {
        Ok(output)
    } else {
        let mut combined = output.stderr;
        combined.extend_from_slice(&output.stdout);
        Err(NativeStaticProductBuildError::ProducerFailed {
            program: program.to_string_lossy().into_owned(),
            diagnostic: String::from_utf8_lossy(
                &combined[..combined.len().min(MAXIMUM_DIAGNOSTIC_BYTES)],
            )
            .into_owned(),
        })
    }
}

fn validated_executable_relative_path(
    root: &Path,
    candidate: &Path,
) -> Result<PathBuf, NativeStaticProductBuildError> {
    let candidate = if candidate.is_absolute() {
        candidate.to_path_buf()
    } else {
        root.join(candidate)
    };
    let metadata = fs::symlink_metadata(&candidate)
        .map_err(|source| NativeStaticProductBuildError::io(&candidate, source))?;
    if !metadata.is_file() || is_link_or_reparse(&metadata) {
        return Err(NativeStaticProductBuildError::invalid(format!(
            "Cargo executable {} is not a regular unlinked file",
            candidate.display()
        )));
    }
    let canonical_root =
        fs::canonicalize(root).map_err(|source| NativeStaticProductBuildError::io(root, source))?;
    let target_dir = root.join(".target");
    let target_metadata = fs::symlink_metadata(&target_dir)
        .map_err(|source| NativeStaticProductBuildError::io(&target_dir, source))?;
    if !target_metadata.is_dir() || is_link_or_reparse(&target_metadata) {
        return Err(NativeStaticProductBuildError::invalid(format!(
            "Cargo target directory {} is not a regular unlinked directory",
            target_dir.display()
        )));
    }
    let canonical_target = fs::canonicalize(&target_dir)
        .map_err(|source| NativeStaticProductBuildError::io(&target_dir, source))?;
    require_path_below_generated_root(
        &canonical_root,
        &canonical_target,
        "Cargo target directory",
    )?;
    let canonical_executable = fs::canonicalize(&candidate)
        .map_err(|source| NativeStaticProductBuildError::io(&candidate, source))?;
    require_path_below_generated_root(
        &canonical_target,
        &canonical_executable,
        "Cargo executable",
    )?;
    canonical_executable
        .strip_prefix(&canonical_root)
        .map(Path::to_path_buf)
        .map_err(|error| {
            NativeStaticProductBuildError::invalid(format!(
                "Cargo executable escaped generated root: {error}"
            ))
        })
}

fn read_validated_executable(
    root: &Path,
    candidate: &Path,
) -> Result<(PathBuf, Vec<u8>), NativeStaticProductBuildError> {
    let relative_path = validated_executable_relative_path(root, candidate)?;
    let candidate = if candidate.is_absolute() {
        candidate.to_path_buf()
    } else {
        root.join(candidate)
    };
    let canonical_path = fs::canonicalize(&candidate)
        .map_err(|source| NativeStaticProductBuildError::io(&candidate, source))?;
    let mut file = fs::File::open(&canonical_path)
        .map_err(|source| NativeStaticProductBuildError::io(&canonical_path, source))?;
    let metadata = file
        .metadata()
        .map_err(|source| NativeStaticProductBuildError::io(&canonical_path, source))?;
    if !metadata.is_file() {
        return Err(NativeStaticProductBuildError::invalid(format!(
            "opened Cargo executable {} is not a regular file",
            canonical_path.display()
        )));
    }
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)
        .map_err(|source| NativeStaticProductBuildError::io(&canonical_path, source))?;
    Ok((relative_path, bytes))
}

fn publish_or_reuse_workspace(
    staging: &mut OwnedStagingDirectory,
    workspace_root: &Path,
    executable_relative_path: &Path,
    fresh_executable_bytes: Vec<u8>,
    expected_workspace_inputs: &BTreeMap<String, Vec<u8>>,
    selected: &VerifiedSelectedPackages,
    platforms: &VerifiedPlatformSources,
) -> Result<(PathBuf, Vec<u8>), NativeStaticProductBuildError> {
    match fs::symlink_metadata(workspace_root) {
        Ok(_) => {
            let bytes = verify_cached_workspace(
                workspace_root,
                executable_relative_path,
                &fresh_executable_bytes,
                expected_workspace_inputs,
                selected,
                platforms,
            )?;
            Ok((workspace_root.join(executable_relative_path), bytes))
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            match fs::rename(staging.path(), workspace_root) {
                Ok(()) => {
                    staging.mark_published();
                    Ok((
                        workspace_root.join(executable_relative_path),
                        fresh_executable_bytes,
                    ))
                }
                Err(rename_error) => {
                    if fs::symlink_metadata(workspace_root).is_ok() {
                        let bytes = verify_cached_workspace(
                            workspace_root,
                            executable_relative_path,
                            &fresh_executable_bytes,
                            expected_workspace_inputs,
                            selected,
                            platforms,
                        )?;
                        Ok((workspace_root.join(executable_relative_path), bytes))
                    } else {
                        Err(NativeStaticProductBuildError::io(
                            workspace_root,
                            rename_error,
                        ))
                    }
                }
            }
        }
        Err(error) => Err(NativeStaticProductBuildError::io(workspace_root, error)),
    }
}

fn verify_cached_workspace(
    workspace_root: &Path,
    executable_relative_path: &Path,
    fresh_executable_bytes: &[u8],
    expected_workspace_inputs: &BTreeMap<String, Vec<u8>>,
    selected: &VerifiedSelectedPackages,
    platforms: &VerifiedPlatformSources,
) -> Result<Vec<u8>, NativeStaticProductBuildError> {
    let validate = || -> Result<Vec<u8>, NativeStaticProductBuildError> {
        let metadata = fs::symlink_metadata(workspace_root)
            .map_err(|source| NativeStaticProductBuildError::io(workspace_root, source))?;
        if !metadata.is_dir() || is_link_or_reparse(&metadata) {
            return Err(NativeStaticProductBuildError::invalid(
                "published workspace is not a regular unlinked directory",
            ));
        }
        verify_materialized_inputs(workspace_root, selected, platforms)?;
        verify_declared_workspace_inputs(workspace_root, expected_workspace_inputs)?;
        let candidate = workspace_root.join(executable_relative_path);
        let (actual_relative, bytes) = read_validated_executable(workspace_root, &candidate)?;
        if actual_relative != executable_relative_path {
            return Err(NativeStaticProductBuildError::invalid(
                "cached executable resolved to a different workspace path",
            ));
        }
        if bytes != fresh_executable_bytes {
            return Err(NativeStaticProductBuildError::invalid(
                "cached executable differs from a fresh build of the same plan",
            ));
        }
        Ok(bytes)
    };
    validate().map_err(|error| {
        NativeStaticProductBuildError::cache_conflict(workspace_root, error.to_string())
    })
}

fn collect_declared_workspace_files(
    root: &Path,
) -> Result<BTreeMap<String, Vec<u8>>, NativeStaticProductBuildError> {
    let mut files = BTreeMap::new();
    collect_declared_workspace_files_into(root, root, &mut files)?;
    Ok(files)
}

fn collect_declared_workspace_files_into(
    root: &Path,
    directory: &Path,
    files: &mut BTreeMap<String, Vec<u8>>,
) -> Result<(), NativeStaticProductBuildError> {
    let entries = fs::read_dir(directory)
        .map_err(|source| NativeStaticProductBuildError::io(directory, source))?;
    for entry in entries {
        let entry = entry.map_err(|source| NativeStaticProductBuildError::io(directory, source))?;
        let path = entry.path();
        let kind = entry
            .file_type()
            .map_err(|source| NativeStaticProductBuildError::io(&path, source))?;
        if directory == root && entry.file_name() == OsStr::new(".target") {
            if !kind.is_dir() || kind.is_symlink() {
                return Err(NativeStaticProductBuildError::source(
                    root.display().to_string(),
                    "Cargo target directory is not a regular directory",
                ));
            }
            let metadata = fs::symlink_metadata(&path)
                .map_err(|source| NativeStaticProductBuildError::io(&path, source))?;
            if is_link_or_reparse(&metadata) {
                return Err(NativeStaticProductBuildError::source(
                    root.display().to_string(),
                    "Cargo target directory is a link or reparse point",
                ));
            }
            continue;
        }
        if kind.is_symlink() {
            return Err(NativeStaticProductBuildError::source(
                root.display().to_string(),
                format!("generated workspace gained a link at {}", path.display()),
            ));
        }
        if kind.is_dir() {
            collect_declared_workspace_files_into(root, &path, files)?;
        } else if kind.is_file() {
            let relative = path.strip_prefix(root).map_err(|error| {
                NativeStaticProductBuildError::invalid(format!(
                    "generated workspace path escaped its root: {error}"
                ))
            })?;
            let logical = relative
                .to_str()
                .ok_or_else(|| {
                    NativeStaticProductBuildError::invalid(format!(
                        "generated workspace path is not Unicode: {}",
                        relative.display()
                    ))
                })?
                .replace(std::path::MAIN_SEPARATOR, "/");
            let bytes = fs::read(&path)
                .map_err(|source| NativeStaticProductBuildError::io(&path, source))?;
            if files.insert(logical.clone(), bytes).is_some() {
                return Err(NativeStaticProductBuildError::invalid(format!(
                    "generated workspace path {logical} is duplicated"
                )));
            }
        } else {
            return Err(NativeStaticProductBuildError::source(
                root.display().to_string(),
                format!(
                    "generated workspace gained a special file at {}",
                    path.display()
                ),
            ));
        }
    }
    Ok(())
}

fn verify_declared_workspace_inputs(
    root: &Path,
    expected: &BTreeMap<String, Vec<u8>>,
) -> Result<(), NativeStaticProductBuildError> {
    let actual = collect_declared_workspace_files(root)?;
    if actual == *expected {
        Ok(())
    } else {
        Err(NativeStaticProductBuildError::source(
            root.display().to_string(),
            "declared generated-workspace inputs changed during Cargo build",
        ))
    }
}

fn safe_owned_staging_tree(cache_root: &Path, path: &Path) -> bool {
    if path.parent() != Some(cache_root)
        || !path
            .file_name()
            .and_then(OsStr::to_str)
            .is_some_and(|name| name.starts_with(".staging-"))
    {
        return false;
    }
    let Ok(resolved_parent) = fs::canonicalize(cache_root) else {
        return false;
    };
    if resolved_parent != cache_root {
        return false;
    }
    let Ok(metadata) = fs::symlink_metadata(path) else {
        return false;
    };
    metadata.is_dir()
        && !is_link_or_reparse(&metadata)
        && tree_contains_only_unlinked_files_and_directories(path)
}

fn tree_contains_only_unlinked_files_and_directories(directory: &Path) -> bool {
    let Ok(entries) = fs::read_dir(directory) else {
        return false;
    };
    for entry in entries {
        let Ok(entry) = entry else {
            return false;
        };
        let path = entry.path();
        let Ok(metadata) = fs::symlink_metadata(&path) else {
            return false;
        };
        if is_link_or_reparse(&metadata) {
            return false;
        }
        if metadata.is_dir() {
            if !tree_contains_only_unlinked_files_and_directories(&path) {
                return false;
            }
        } else if !metadata.is_file() {
            return false;
        }
    }
    true
}

fn is_link_or_reparse(metadata: &fs::Metadata) -> bool {
    if metadata.file_type().is_symlink() {
        return true;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt as _;

        const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
        metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
    }
    #[cfg(not(windows))]
    {
        false
    }
}

fn materialize_snapshot(
    root: &Path,
    snapshot: &SourceSnapshot,
) -> Result<(), NativeStaticProductBuildError> {
    for (logical, file) in snapshot.files() {
        reject_reserved_source_path(logical)?;
        write_file(
            &root.join(logical.replace('/', std::path::MAIN_SEPARATOR_STR)),
            file.bytes(),
        )?;
    }
    Ok(())
}

fn reject_reserved_source_path(logical: &str) -> Result<(), NativeStaticProductBuildError> {
    let first = logical.split('/').next().unwrap_or(logical);
    if matches!(first, "target" | ".git" | ".latticeaxiom") {
        Err(NativeStaticProductBuildError::invalid(format!(
            "source object contains reserved build path {logical}"
        )))
    } else {
        Ok(())
    }
}

fn verify_materialized_inputs(
    root: &Path,
    selected: &VerifiedSelectedPackages,
    platforms: &VerifiedPlatformSources,
) -> Result<(), NativeStaticProductBuildError> {
    for (path, snapshot) in selected.snapshots.iter().chain(platforms.snapshots.iter()) {
        let materialized = root.join(logical_path(path));
        let actual = collect_files(&materialized, &materialized)?;
        if actual.len() != snapshot.files().len() {
            return Err(NativeStaticProductBuildError::source(
                path.to_string(),
                "materialized source file set changed during Cargo build",
            ));
        }
        for (logical, expected) in snapshot.files() {
            let Some(bytes) = actual.get(logical) else {
                return Err(NativeStaticProductBuildError::source(
                    path.to_string(),
                    format!("materialized source file {logical} disappeared"),
                ));
            };
            if bytes.as_slice() != expected.bytes() {
                return Err(NativeStaticProductBuildError::source(
                    path.to_string(),
                    format!("materialized source file {logical} changed during Cargo build"),
                ));
            }
        }
    }
    Ok(())
}

fn collect_files(
    root: &Path,
    directory: &Path,
) -> Result<BTreeMap<String, Vec<u8>>, NativeStaticProductBuildError> {
    let mut files = BTreeMap::new();
    collect_files_into(root, directory, &mut files)?;
    Ok(files)
}

fn collect_files_into(
    root: &Path,
    directory: &Path,
    files: &mut BTreeMap<String, Vec<u8>>,
) -> Result<(), NativeStaticProductBuildError> {
    let entries = fs::read_dir(directory)
        .map_err(|source| NativeStaticProductBuildError::io(directory, source))?;
    for entry in entries {
        let entry = entry.map_err(|source| NativeStaticProductBuildError::io(directory, source))?;
        let path = entry.path();
        let kind = entry
            .file_type()
            .map_err(|source| NativeStaticProductBuildError::io(&path, source))?;
        if kind.is_symlink() {
            return Err(NativeStaticProductBuildError::source(
                root.display().to_string(),
                format!("materialized source gained a link at {}", path.display()),
            ));
        }
        if kind.is_dir() {
            collect_files_into(root, &path, files)?;
        } else if kind.is_file() {
            let relative = path.strip_prefix(root).map_err(|error| {
                NativeStaticProductBuildError::invalid(format!(
                    "materialized path escaped its root: {error}"
                ))
            })?;
            let logical = relative
                .components()
                .map(|component| component.as_os_str().to_string_lossy())
                .collect::<Vec<_>>()
                .join("/");
            let bytes = fs::read(&path)
                .map_err(|source| NativeStaticProductBuildError::io(&path, source))?;
            files.insert(logical, bytes);
        } else {
            return Err(NativeStaticProductBuildError::source(
                root.display().to_string(),
                format!(
                    "materialized source gained a special file at {}",
                    path.display()
                ),
            ));
        }
    }
    Ok(())
}

fn logical_path(path: &CanonicalLogicalPath) -> PathBuf {
    PathBuf::from(path.as_str().replace('/', std::path::MAIN_SEPARATOR_STR))
}

fn write_file(path: &Path, bytes: &[u8]) -> Result<(), NativeStaticProductBuildError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|source| NativeStaticProductBuildError::io(parent, source))?;
    }
    let mut file = fs::File::options()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|source| NativeStaticProductBuildError::io(path, source))?;
    file.write_all(bytes)
        .map_err(|source| NativeStaticProductBuildError::io(path, source))?;
    file.sync_all()
        .map_err(|source| NativeStaticProductBuildError::io(path, source))
}

#[cfg(test)]
mod tests {
    use std::process::Command;

    use latticeaxiom_core::{SourceId, canonical_json_bytes};

    use super::*;
    use crate::{AuthorizedRoot, AuthorizedRootKind, SourceScanLimits, scan_source_snapshot};

    const TEST_SCAN_LIMITS: SourceScanLimits = SourceScanLimits {
        maximum_files: 32,
        maximum_bytes: 64 * 1_024,
    };

    #[test]
    fn nested_package_links_two_crates_and_frozen_text_resource() {
        let input = TestDirectory::new("nested-input");
        let cache = TestDirectory::new("nested-cache");
        let registration_hash = CanonicalHash::digest(b"nested-registration");
        input.write(
            "latticeaxiom-package.toml",
            br#"
schema_version = 1
name = "@example/nested"
version = "0.1.0"
domains = ["authoritative"]
trust = "trusted-native"
[realizations.native-static]
id = "native-static"
kind = "native-static"
domains = ["authoritative"]
artifact = { kind = "source-build" }
trust = "trusted-native"
[nickel_public_entrypoints]
default = "package.ncl"
[source_inclusion]
include = ["latticeaxiom-package.toml", "package.ncl", "crates", "data"]
[rust]
entry = "crates/runtime/Cargo.toml"
members = ["crates/runtime/Cargo.toml", "crates/model/Cargo.toml"]
"#,
        );
        input.write("package.ncl", b"{}\n");
        input.write(
            "crates/runtime/Cargo.toml",
            br#"
[package]
name = "nested-runtime"
version.workspace = true
edition.workspace = true
[dependencies]
nested-model = { path = "../model" }
"#,
        );
        input.write(
            "crates/model/Cargo.toml",
            br#"
[package]
name = "nested-model"
version.workspace = true
edition.workspace = true
"#,
        );
        input.write("crates/runtime/src/lib.rs", b"pub fn native_static_registration_hash() -> Result<&'static str, &'static str> { Ok(nested_model::hash()) }\n");
        input.write("crates/model/src/lib.rs", b"pub fn hash() -> &'static str { include_str!(\"../../../data/registration.txt\").trim() }\n");
        input.write(
            "data/registration.txt",
            registration_hash.to_string().as_bytes(),
        );
        let snapshot = snapshot(&input);
        let bytes = canonical_json_bytes(&snapshot).expect("source bytes");
        input.write(
            "data/registration.txt",
            b"mutable source must not be consumed",
        );
        let request = NativeStaticProductBuildRequestV1 {
            graph_hash: CanonicalHash::digest(b"nested-graph"),
            target: host_target(),
            requested_toolchain: CanonicalHash::digest(b"nested-toolchain"),
            registration_image_hash: registration_hash,
            selected_packages: vec![NativeStaticPackageSourceInputV1 {
                package: "@example/nested".parse().expect("package"),
                cargo_package: "nested-runtime".to_owned(),
                expected_source_digest: snapshot.source_hash(),
                expected_source_object_digest: CanonicalHash::digest(&bytes),
                source_object_bytes: bytes,
                registration_hash,
            }],
            platform_sources: Vec::new(),
            workspace_scaffold_bytes: workspace_scaffold().into_bytes(),
            cargo_lock_seed_bytes: Vec::new(),
            cargo_program: std::env::var_os("CARGO").unwrap_or_else(|| OsString::from("cargo")),
            rustc_program: std::env::var_os("RUSTC").unwrap_or_else(|| OsString::from("rustc")),
        };
        let output =
            build_native_static_product(&request, cache.path()).expect("nested product builds");
        assert_eq!(output.plan.packages[0].cargo_members.len(), 2);
        assert!(
            Command::new(&output.executable_path)
                .status()
                .expect("product starts")
                .success()
        );
    }

    #[allow(
        clippy::too_many_lines,
        reason = "the end-to-end build, replay, execution, and cleanup proof is intentionally cohesive"
    )]
    #[test]
    fn generated_product_builds_from_cas_bytes_after_mutable_source_changes() {
        let input = TestDirectory::new("input");
        let cache = TestDirectory::new("cache");
        let registration_hash = CanonicalHash::digest(b"proof-registration");
        input.write(
            "Cargo.toml",
            br#"[package]
name = "proof-native"
version.workspace = true
edition.workspace = true
rust-version.workspace = true
publish.workspace = true
license.workspace = true
repository.workspace = true

[lints]
workspace = true
"#,
        );
        input.write(
            "src/lib.rs",
            format!(
                "pub fn native_static_registration_hash() -> Result<&'static str, &'static str> {{ Ok(\"{registration_hash}\") }}\n"
            )
            .as_bytes(),
        );
        let snapshot = snapshot(&input);
        let source_object_bytes = canonical_json_bytes(&snapshot)
            .unwrap_or_else(|error| panic!("snapshot must encode: {error}"));
        input.write(
            "src/lib.rs",
            b"pub fn native_static_registration_hash() -> Result<&'static str, &'static str> { Ok(\"mutable-path-was-read\") }\n",
        );

        let request = NativeStaticProductBuildRequestV1 {
            graph_hash: CanonicalHash::digest(b"proof-graph"),
            target: host_target(),
            requested_toolchain: CanonicalHash::digest(b"proof-toolchain"),
            registration_image_hash: CanonicalHash::digest(b"proof-registration-image"),
            selected_packages: vec![NativeStaticPackageSourceInputV1 {
                package: "@example/proof-native"
                    .parse()
                    .unwrap_or_else(|error| panic!("package name must parse: {error}")),
                cargo_package: "proof-native".to_owned(),
                expected_source_digest: snapshot.source_hash(),
                expected_source_object_digest: CanonicalHash::digest(&source_object_bytes),
                source_object_bytes,
                registration_hash,
            }],
            platform_sources: Vec::new(),
            workspace_scaffold_bytes: workspace_scaffold().into_bytes(),
            cargo_lock_seed_bytes: Vec::new(),
            cargo_program: std::env::var_os("CARGO").unwrap_or_else(|| OsString::from("cargo")),
            rustc_program: std::env::var_os("RUSTC").unwrap_or_else(|| OsString::from("rustc")),
        };

        let output = build_native_static_product(&request, cache.path())
            .unwrap_or_else(|error| panic!("product executable must build: {error}"));
        assert_eq!(
            CanonicalHash::digest(&output.executable_bytes),
            output.receipt.executable_digest
        );
        assert_eq!(
            CanonicalHash::digest(&output.cargo_lock_bytes),
            output.receipt.cargo_lock_digest
        );
        assert_eq!(
            output.workspace_root.file_name(),
            Some(OsStr::new(
                &output.receipt.product_workspace_digest.to_string()
            ))
        );
        let run = Command::new(&output.executable_path)
            .output()
            .unwrap_or_else(|error| panic!("generated product must start: {error}"));
        assert!(
            run.status.success(),
            "generated product rejected its registration: {}",
            String::from_utf8_lossy(&run.stderr)
        );
        let replay = build_native_static_product(&request, cache.path())
            .unwrap_or_else(|error| panic!("identical product build must be idempotent: {error}"));
        assert_eq!(
            replay.receipt.build_plan_digest,
            output.receipt.build_plan_digest
        );
        assert_eq!(
            CanonicalHash::digest(&replay.executable_bytes),
            replay.receipt.executable_digest
        );
        assert_eq!(
            replay.workspace_root.file_name(),
            Some(OsStr::new(
                &replay.receipt.product_workspace_digest.to_string()
            ))
        );
        let staging_count = fs::read_dir(cache.path().join(BUILD_DIRECTORY))
            .unwrap_or_else(|error| panic!("build cache must remain readable: {error}"))
            .filter_map(Result::ok)
            .filter(|entry| entry.file_name().to_string_lossy().starts_with(".staging-"))
            .count();
        assert_eq!(
            staging_count, 0,
            "successful replay must clean its staging tree"
        );
    }

    #[test]
    fn source_object_digest_mismatch_fails_before_workspace_creation() {
        let input = TestDirectory::new("digest-input");
        let cache = TestDirectory::new("digest-cache");
        input.write(
            "Cargo.toml",
            b"[package]\nname=\"proof-native\"\nversion=\"0.1.0\"\nedition=\"2024\"\n",
        );
        input.write(
            "src/lib.rs",
            b"pub fn native_static_registration_hash() -> Result<&'static str, &'static str> { Ok(\"\") }\n",
        );
        let snapshot = snapshot(&input);
        let bytes = canonical_json_bytes(&snapshot)
            .unwrap_or_else(|error| panic!("snapshot must encode: {error}"));
        let request = NativeStaticProductBuildRequestV1 {
            graph_hash: CanonicalHash::digest(b"proof-graph"),
            target: host_target(),
            requested_toolchain: CanonicalHash::digest(b"proof-toolchain"),
            registration_image_hash: CanonicalHash::digest(b"proof-registration-image"),
            selected_packages: vec![NativeStaticPackageSourceInputV1 {
                package: "@example/proof-native"
                    .parse()
                    .unwrap_or_else(|error| panic!("package name must parse: {error}")),
                cargo_package: "proof-native".to_owned(),
                expected_source_digest: snapshot.source_hash(),
                expected_source_object_digest: CanonicalHash::digest(b"not-the-object"),
                source_object_bytes: bytes,
                registration_hash: CanonicalHash::digest(b"proof-registration"),
            }],
            platform_sources: Vec::new(),
            workspace_scaffold_bytes: workspace_scaffold().into_bytes(),
            cargo_lock_seed_bytes: Vec::new(),
            cargo_program: OsString::from("cargo"),
            rustc_program: OsString::from("rustc"),
        };

        assert!(matches!(
            build_native_static_product(&request, cache.path()),
            Err(NativeStaticProductBuildError::SourceProof { .. })
        ));
        assert!(!cache.path().join(BUILD_DIRECTORY).exists());
    }

    #[test]
    fn platform_workspace_paths_accept_crate_and_package_members() {
        for path in [
            "packages/latticeaxiom/host/crates/latticeaxiom-host",
            "packages/latticeaxiom/input",
        ] {
            let path: CanonicalLogicalPath = path
                .parse()
                .unwrap_or_else(|error| panic!("test path must parse: {error}"));
            assert!(validate_platform_workspace_path(&path).is_ok());
        }
    }

    #[test]
    fn platform_workspace_paths_reject_outside_and_reserved_members() {
        for path in [
            "vendor/latticeaxiom-engine",
            "product/generated-platform",
            "packages/selected-0000",
            "packages/selected-0000/nested",
        ] {
            let path: CanonicalLogicalPath = path
                .parse()
                .unwrap_or_else(|error| panic!("test path must parse: {error}"));
            assert!(validate_platform_workspace_path(&path).is_err());
        }
    }

    #[test]
    fn platform_workspace_paths_reject_traversal_at_the_typed_boundary() {
        assert!(
            "packages/../crates/latticeaxiom-engine"
                .parse::<CanonicalLogicalPath>()
                .is_err()
        );
    }

    fn workspace_scaffold() -> String {
        r#"[workspace]
resolver = "3"
members = []

[workspace.package]
version = "0.1.0"
edition = "2024"
rust-version = "1.85"
publish = false
license = "AGPL-3.0-only"
repository = "https://example.invalid/proof"

[workspace.lints.rust]
unsafe_code = "deny"
"#
        .to_owned()
    }

    fn snapshot(directory: &TestDirectory) -> SourceSnapshot {
        let source_id: SourceId = "latticeaxiom:source/native-static-proof"
            .parse()
            .unwrap_or_else(|error| panic!("source ID must parse: {error}"));
        let root = AuthorizedRoot::new(source_id, AuthorizedRootKind::Test, directory.path())
            .unwrap_or_else(|error| panic!("source root must authorize: {error}"));
        scan_source_snapshot(&root, TEST_SCAN_LIMITS)
            .unwrap_or_else(|error| panic!("source snapshot must scan: {error}"))
    }

    fn host_target() -> TargetTriple {
        let rustc = std::env::var_os("RUSTC").unwrap_or_else(|| OsString::from("rustc"));
        let output = Command::new(rustc)
            .arg("-vV")
            .output()
            .unwrap_or_else(|error| panic!("rustc host query must start: {error}"));
        let text = String::from_utf8(output.stdout)
            .unwrap_or_else(|error| panic!("rustc host output must be UTF-8: {error}"));
        let host = text
            .lines()
            .find_map(|line| line.strip_prefix("host: "))
            .unwrap_or_else(|| panic!("rustc -vV must report host"));
        host.parse()
            .unwrap_or_else(|error| panic!("rustc host triple must parse: {error}"))
    }

    #[derive(Debug)]
    struct TestDirectory(tempfile::TempDir);

    impl TestDirectory {
        fn new(label: &str) -> Self {
            let prefix = format!("latticeaxiom-native-static-{label}-");
            Self(
                tempfile::Builder::new()
                    .prefix(&prefix)
                    .tempdir()
                    .unwrap_or_else(|error| panic!("test directory must create: {error}")),
            )
        }

        fn path(&self) -> &Path {
            self.0.path()
        }

        fn write(&self, logical: &str, bytes: &[u8]) {
            let path = self
                .0
                .path()
                .join(logical.replace('/', std::path::MAIN_SEPARATOR_STR));
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)
                    .unwrap_or_else(|error| panic!("test parent must create: {error}"));
            }
            fs::write(&path, bytes).unwrap_or_else(|error| panic!("test file must write: {error}"));
        }
    }
}
