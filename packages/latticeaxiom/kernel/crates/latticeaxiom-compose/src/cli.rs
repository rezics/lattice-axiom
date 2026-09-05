//! Public `latticeaxiom-compose` argument parsing, offline lock, and frozen verify.
//!
//! `lock` / `resolve` read [`CompositionBootstrapV1`], run the bootstrap-first
//! packages transaction when path sources are present, and persist
//! [`LockV1`]. `verify` / `run-frozen` reopen that lock with offline frozen
//! semantics and fail closed on missing or tampered CAS receipts. This module
//! does not open a world writer, construct a launch intent, or load native
//! modules.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::ffi::OsString;
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

use clap::Parser as _;
use latticeaxiom_core::{
    CanonicalHash, CanonicalJsonError, CanonicalLogicalPath, CapabilityId, PackageName, SchemaId,
    SourceId, StableId, TargetTriple, canonical_json_bytes, canonical_json_hash,
};
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU64, Ordering};
use thiserror::Error;

use crate::{
    ArtifactIntent, AuthorizedRoot, AuthorizedRootKind, BootstrapManifestError,
    BootstrapSourceProviderV1, COMPOSITION_BOOTSTRAP_FILE_NAME, CompositionBootstrapV1,
    LOCK_SCHEMA_VERSION, LockActionMode, LockV1, LockedAliasEdgeV1, LockedDependency,
    LockedGameGraph, LockedPackage, ManifestRealizationV1, NickelEvaluationLimits,
    ObservabilityCatalog, PACKAGE_SOURCE_MANIFEST_FILE_NAME, PRODUCT_LOCK_FILE_NAME,
    PRODUCT_LOCK_PRODUCER_MACHINE, PackageAlias, PackageDomain, PackageSourceManifestV1,
    ProductLockDraftV1, ProductLockError, ProductLockHostReceipts, ProductLockObjects,
    ProductLockProducerV1, ProductLockReceiptKind, RealizationId, RealizationKind,
    RealizedDataRootV1, RegistrationImage, ResolutionStep, RuntimeBinding, RuntimeImage,
    SemanticCatalog, SettingsCatalog, SourceScanError, SourceScanLimits, SourceSnapshot,
    TargetPackageRealizationV1, TargetRealizationLockV1, controller_host_target,
    persist_product_lock, reopen_product_lock, scan_included_source_snapshot, scan_source_snapshot,
    verify_product_lock,
};

/// Directory that receives the local catalog index and CAS objects.
pub const CLI_CATALOG_DIRECTORY: &str = "catalog";

/// Object-store directory beside the catalog root.
pub const CLI_CAS_DIRECTORY: &str = "cas";

const CAS_SOURCE_TREE: &str = "source-tree";
const CAS_PACKAGE_MANIFEST: &str = "package-manifest";
static NEXT_SOURCE_BUILD_STAGING_DIRECTORY: AtomicU64 = AtomicU64::new(0);

const CAS_REALIZED_ARTIFACT: &str = "realized-artifact";
const CLI_TOOLCHAIN_DOMAIN: &[u8] = b"latticeaxiom:cli-lock-toolchain/r0";
const PACKAGE_SCAN_LIMITS: SourceScanLimits = SourceScanLimits {
    maximum_files: 4_096,
    maximum_bytes: 64 * 1024 * 1024,
};

/// Parsed public controller command.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CliCommand {
    /// Existing complete-request evaluation.
    Evaluate {
        /// Absolute worker executable path.
        worker: PathBuf,
        /// Request JSON path.
        request: PathBuf,
    },
    /// Bootstrap-first offline lock.
    Lock(LockRequest),
    /// Reopen `latticeaxiom.lock` with `--offline --frozen`.
    Verify(VerifyRequest),
}

/// Inputs for one offline lock command.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LockRequest {
    /// Directory against which bootstrap Path locators are resolved.
    pub workspace_root: PathBuf,
    /// Bootstrap file. Relative paths are resolved against `workspace_root`.
    pub bootstrap_path: PathBuf,
    /// Directory that receives catalog CAS objects.
    pub catalog_root: PathBuf,
    /// Destination of the atomically written `latticeaxiom.lock`.
    pub lock_path: PathBuf,
    /// Realization target sealed into the product lock.
    pub target: TargetTriple,
    /// Host toolchain identity sealed into the product lock.
    pub toolchain: CanonicalHash,
}

/// Inputs for one frozen verify command.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerifyRequest {
    /// Workspace used to resolve relative lock and catalog paths.
    pub workspace_root: PathBuf,
    /// Product lock to reopen.
    pub lock_path: PathBuf,
    /// Catalog root that holds the CAS object store.
    pub catalog_root: PathBuf,
}

/// Machine-readable report from lock or frozen verify.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct CliLockReport {
    /// Subcommand that produced this report.
    pub command: String,
    /// Exact product-lock hash of the reopened lock.
    pub product_lock_hash: CanonicalHash,
}

/// A CLI setup, lock, or frozen-verify failure.
#[derive(Debug, Error)]
#[error("[{code}] {details}")]
pub struct CliError {
    /// Stable diagnostic code.
    pub code: &'static str,
    /// Presentation-neutral diagnostic text.
    pub details: String,
}

impl CliError {
    /// Returns the evaluate-only usage diagnostic.
    #[must_use]
    pub fn evaluate_usage() -> Self {
        Self {
            code: "compose.worker_protocol",
            details: "usage: latticeaxiom-compose evaluate --worker <absolute-path> --request <request.json>".to_owned(),
        }
    }

    /// Returns the public command usage diagnostic.
    #[must_use]
    pub fn usage() -> Self {
        Self {
            code: "compose.lock",
            details: "usage: latticeaxiom-compose evaluate --worker <absolute-path> --request <request.json>
       latticeaxiom-compose lock [--workspace <dir>] [--bootstrap <file>] [--catalog <dir>] [--lock <file>] [--target <triple>] [--offline]
       latticeaxiom-compose verify --offline --frozen [--workspace <dir>] [--lock <file>] [--catalog <dir>]".to_owned(),
        }
    }

    fn lock(details: impl Into<String>) -> Self {
        Self {
            code: "compose.lock",
            details: details.into(),
        }
    }

    fn io(path: impl AsRef<Path>, source: &io::Error) -> Self {
        Self::lock(format!(
            "I/O failed at `{}`: {source}",
            path.as_ref().display()
        ))
    }

    fn protocol(error: &crate::WorkerProtocolError) -> Self {
        Self {
            code: error.code(),
            details: error.to_string(),
        }
    }

    fn supervisor(error: &crate::SupervisorError) -> Self {
        Self {
            code: error.code(),
            details: error.to_string(),
        }
    }
}

impl From<ProductLockError> for CliError {
    fn from(error: ProductLockError) -> Self {
        Self::lock(error.to_string())
    }
}

impl From<BootstrapManifestError> for CliError {
    fn from(error: BootstrapManifestError) -> Self {
        Self::lock(error.to_string())
    }
}

impl From<SourceScanError> for CliError {
    fn from(error: SourceScanError) -> Self {
        Self::lock(error.to_string())
    }
}

impl From<CanonicalJsonError> for CliError {
    fn from(error: CanonicalJsonError) -> Self {
        Self::lock(error.to_string())
    }
}

/// Parses public controller arguments without executing them.
///
/// # Errors
///
/// Returns [`CliError`] for a missing command, unknown flag, or incomplete
/// evaluate / lock / verify invocation.
pub fn parse_cli(arguments: impl IntoIterator<Item = OsString>) -> Result<CliCommand, CliError> {
    let arguments = arguments.into_iter().collect::<Vec<_>>();
    let evaluate = arguments
        .first()
        .is_some_and(|command| command == "evaluate");
    let parsed = ParsedCli::try_parse_from(
        std::iter::once(OsString::from("latticeaxiom-compose")).chain(arguments),
    )
    .map_err(|_| {
        if evaluate {
            CliError::evaluate_usage()
        } else {
            CliError::usage()
        }
    })?;

    match parsed.command {
        ParsedCommand::Evaluate(arguments) => Ok(CliCommand::Evaluate {
            worker: arguments.worker,
            request: arguments.request,
        }),
        ParsedCommand::Lock(arguments) => parse_lock(arguments),
        ParsedCommand::Verify(arguments) => parse_verify(arguments, false),
        ParsedCommand::RunFrozen(arguments) => parse_verify(arguments, true),
    }
}

#[derive(Debug, clap::Parser)]
#[command(
    name = "latticeaxiom-compose",
    disable_help_flag = true,
    disable_version_flag = true,
    disable_help_subcommand = true
)]
struct ParsedCli {
    #[command(subcommand)]
    command: ParsedCommand,
}

#[derive(Debug, clap::Subcommand)]
enum ParsedCommand {
    Evaluate(EvaluateArguments),
    #[command(alias = "resolve")]
    Lock(LockArguments),
    Verify(VerifyArguments),
    #[command(name = "run-frozen")]
    RunFrozen(VerifyArguments),
}

#[derive(Debug, clap::Args)]
struct EvaluateArguments {
    #[arg(long, allow_hyphen_values = true)]
    worker: PathBuf,
    #[arg(long, allow_hyphen_values = true)]
    request: PathBuf,
}

#[derive(Debug, clap::Args)]
struct LockArguments {
    #[arg(long, allow_hyphen_values = true)]
    workspace: Option<PathBuf>,
    #[arg(long, allow_hyphen_values = true)]
    bootstrap: Option<PathBuf>,
    #[arg(long, allow_hyphen_values = true)]
    catalog: Option<PathBuf>,
    #[arg(long, allow_hyphen_values = true)]
    lock: Option<PathBuf>,
    #[arg(long, allow_hyphen_values = true)]
    target: Option<OsString>,
    #[arg(long, allow_hyphen_values = true)]
    toolchain: Option<OsString>,
    #[arg(long, action = clap::ArgAction::Count)]
    offline: u8,
}

#[derive(Debug, clap::Args)]
struct VerifyArguments {
    #[arg(long, allow_hyphen_values = true)]
    workspace: Option<PathBuf>,
    #[arg(long, allow_hyphen_values = true)]
    catalog: Option<PathBuf>,
    #[arg(long, allow_hyphen_values = true)]
    lock: Option<PathBuf>,
    #[arg(long, action = clap::ArgAction::Count)]
    offline: u8,
    #[arg(long, action = clap::ArgAction::Count)]
    frozen: u8,
}

fn parse_lock(arguments: LockArguments) -> Result<CliCommand, CliError> {
    let target = match arguments.target {
        Some(value) => {
            let text = value.to_string_lossy();
            text.parse::<TargetTriple>()
                .map_err(|error| CliError::lock(format!("invalid target `{text}`: {error}")))?
        }
        None => controller_host_target().map_err(|error| CliError::lock(error.to_string()))?,
    };
    let toolchain = match arguments.toolchain {
        Some(value) => {
            let text = value.to_string_lossy();
            text.parse::<CanonicalHash>()
                .map_err(|error| CliError::lock(format!("invalid toolchain `{text}`: {error}")))?
        }
        None => default_toolchain(),
    };
    let _ = arguments.offline;
    Ok(CliCommand::Lock(LockRequest {
        workspace_root: arguments.workspace.unwrap_or_else(default_workspace),
        bootstrap_path: arguments
            .bootstrap
            .unwrap_or_else(|| PathBuf::from(COMPOSITION_BOOTSTRAP_FILE_NAME)),
        catalog_root: arguments
            .catalog
            .unwrap_or_else(|| PathBuf::from(CLI_CATALOG_DIRECTORY)),
        lock_path: arguments
            .lock
            .unwrap_or_else(|| PathBuf::from(PRODUCT_LOCK_FILE_NAME)),
        target,
        toolchain,
    }))
}

fn parse_verify(arguments: VerifyArguments, implied_frozen: bool) -> Result<CliCommand, CliError> {
    let offline = implied_frozen || arguments.offline > 0;
    let frozen = implied_frozen || arguments.frozen > 0;
    if !offline || !frozen {
        return Err(CliError::lock(
            "verify requires --offline --frozen so missing or tampered receipts fail closed",
        ));
    }
    Ok(CliCommand::Verify(VerifyRequest {
        workspace_root: arguments.workspace.unwrap_or_else(default_workspace),
        lock_path: arguments
            .lock
            .unwrap_or_else(|| PathBuf::from(PRODUCT_LOCK_FILE_NAME)),
        catalog_root: arguments
            .catalog
            .unwrap_or_else(|| PathBuf::from(CLI_CATALOG_DIRECTORY)),
    }))
}

fn default_workspace() -> PathBuf {
    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}

/// Returns the default CLI lock toolchain identity.
#[must_use]
pub fn default_toolchain() -> CanonicalHash {
    CanonicalHash::digest(CLI_TOOLCHAIN_DOMAIN)
}

/// Host receipts derived from a reopened product lock.
///
/// Frozen CLI verify checks CAS objects against the lock's own sealed host
/// identity. It does not reconstruct a launch-time toolchain from ambient
/// environment state.
///
/// # Errors
///
/// Returns [`ProductLockError::InvalidStructure`] when the lock has no target
/// realization.
pub fn host_receipts_from_lock(lock: &LockV1) -> Result<ProductLockHostReceipts, ProductLockError> {
    let Some(realization) = lock.realizations.values().next() else {
        return Err(ProductLockError::InvalidStructure {
            reason: "reopened product lock has no target realization".to_owned(),
        });
    };
    Ok(ProductLockHostReceipts {
        toolchain: lock.producer.toolchain,
        engine_build_id: realization.engine_build_id,
        registration_image_hash: lock.registration.image_hash,
        runtime_image_fingerprint: realization.runtime_image_fingerprint,
    })
}

/// Reads [`CompositionBootstrapV1`], runs the path-package transaction when
/// sources are present, writes `latticeaxiom.lock`, and reopens it frozen.
///
/// [`crate`] types remain the lock schema. Intermediate resolution and build
/// intent hashes are recorded; they are not a second lock. Native modules are
/// not loaded.
///
/// # Errors
///
/// Returns [`CliError`] when the bootstrap is invalid, a path source cannot be
/// packed, lock persistence fails, or frozen reopen observes a missing or
/// mismatched receipt.
pub fn lock_workspace(request: &LockRequest) -> Result<(LockV1, CliLockReport), CliError> {
    let workspace_root = absolute_dir(&request.workspace_root)?;
    let bootstrap_path = resolve_against(&workspace_root, &request.bootstrap_path);
    let catalog_root = resolve_against(&workspace_root, &request.catalog_root);
    let lock_path = resolve_against(&workspace_root, &request.lock_path);
    fs::create_dir_all(&catalog_root).map_err(|source| CliError::io(&catalog_root, &source))?;

    let bootstrap = load_bootstrap(&bootstrap_path)?;
    if bootstrap.sources.is_empty() {
        return Err(CliError::lock(
            "packages transaction is absent: bootstrap declares no sources",
        ));
    }

    let packed = pack_declared_path_sources(&workspace_root, &bootstrap, &catalog_root)?;
    let selected = select_closure(&bootstrap, &packed)?;
    let realized = realize_selected_packages(request, &bootstrap, &selected, &catalog_root)?;
    let lock = seal_path_lock(request, &bootstrap, &selected, &realized)?;
    persist_product_lock(&lock_path, &lock, LockActionMode::Offline)?;
    let reopened = verify_lock_at(&lock_path, &catalog_root)?;
    Ok((
        reopened.clone(),
        CliLockReport {
            command: "lock".to_owned(),
            product_lock_hash: reopened.product_lock_hash,
        },
    ))
}

/// Reopens `latticeaxiom.lock` with [`LockActionMode::Frozen`] and
/// [`LockActionMode::Offline`] semantics.
///
/// Missing or tampered CAS receipts fail closed. Acquisition paths are never
/// consulted. Native modules are not loaded.
///
/// # Errors
///
/// Returns [`CliError`] when the lock is missing, truncated, or fails frozen
/// verification.
pub fn verify_workspace_frozen(
    request: &VerifyRequest,
) -> Result<(LockV1, CliLockReport), CliError> {
    let workspace_root = absolute_dir(&request.workspace_root)?;
    let lock_path = resolve_against(&workspace_root, &request.lock_path);
    let catalog_root = resolve_against(&workspace_root, &request.catalog_root);
    let lock = verify_lock_at(&lock_path, &catalog_root)?;
    Ok((
        lock.clone(),
        CliLockReport {
            command: "verify".to_owned(),
            product_lock_hash: lock.product_lock_hash,
        },
    ))
}

/// Reads and decodes an evaluate request payload with the worker protocol cap.
///
/// # Errors
///
/// Returns [`CliError`] when the file cannot be read, exceeds the protocol
/// cap, or is not a valid worker request.
pub fn read_evaluate_request(path: &Path) -> Result<crate::WorkerRequest, CliError> {
    let limit = u64::from(crate::WORKER_PROTOCOL_MAX_PAYLOAD_BYTES);
    let mut file = File::open(path).map_err(|error| CliError {
        code: "compose.worker_protocol",
        details: format!("failed to open request `{}`: {error}", path.display()),
    })?;
    let mut bytes = Vec::new();
    Read::by_ref(&mut file)
        .take(limit.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|error| CliError {
            code: "compose.worker_protocol",
            details: format!("failed to read request `{}`: {error}", path.display()),
        })?;
    if u64::try_from(bytes.len()).map_or(true, |length| length > limit) {
        return Err(CliError {
            code: "compose.worker_protocol",
            details: format!(
                "request `{}` exceeds the {} byte protocol cap",
                path.display(),
                crate::WORKER_PROTOCOL_MAX_PAYLOAD_BYTES
            ),
        });
    }
    crate::decode_worker_request_payload(&bytes).map_err(|error| CliError::protocol(&error))
}

/// Builds a worker command from an absolute executable path.
///
/// # Errors
///
/// Returns [`CliError`] when the supervisor rejects the command.
pub fn worker_command(worker: PathBuf) -> Result<crate::WorkerCommand, CliError> {
    crate::WorkerCommand::new(worker).map_err(|error| CliError::supervisor(&error))
}

#[derive(Clone, Debug, Serialize)]
#[serde(deny_unknown_fields)]
struct ArtifactBuildReceiptV1 {
    package: PackageName,
    source_hash: CanonicalHash,
    realization_id: RealizationId,
    realization_kind: RealizationKind,
    target: TargetTriple,
    requested_toolchain: CanonicalHash,
    producer_kind: &'static str,
    cargo_version_hash: Option<CanonicalHash>,
    rustc_version_hash: Option<CanonicalHash>,
    producer_configuration_hash: Option<CanonicalHash>,
    artifact_hash: CanonicalHash,
}

struct RealizedPackage {
    bytes: Vec<u8>,
    receipt: ArtifactBuildReceiptV1,
}

struct PackedPackage {
    locator: String,
    manifest: PackageSourceManifestV1,
    snapshot: SourceSnapshot,
    manifest_bytes: Vec<u8>,
    source_bytes: Vec<u8>,
}

fn load_bootstrap(path: &Path) -> Result<CompositionBootstrapV1, CliError> {
    let text = fs::read_to_string(path).map_err(|source| CliError::io(path, &source))?;
    Ok(CompositionBootstrapV1::from_toml_str(&text)?)
}

fn pack_declared_path_sources(
    workspace_root: &Path,
    bootstrap: &CompositionBootstrapV1,
    catalog_root: &Path,
) -> Result<BTreeMap<PackageName, PackedPackage>, CliError> {
    let cas_root = catalog_root.join(CLI_CAS_DIRECTORY);
    let mut packed = BTreeMap::new();
    for source in &bootstrap.sources {
        let Some(path) = source_acquisition_path(source)? else {
            continue;
        };
        let root = join_logical(workspace_root, path.as_str());
        let package = check_path_package(&root)?;
        if package.manifest.name != *source.package() {
            return Err(CliError::lock(format!(
                "bootstrap package {} does not match packed package {}",
                source.package(),
                package.manifest.name
            )));
        }
        let manifest_bytes = canonical_json_bytes(&package.manifest)?;
        let source_bytes = canonical_json_bytes(&package.snapshot)?;
        put_cas(&cas_root, CAS_PACKAGE_MANIFEST, &manifest_bytes)?;
        put_cas(&cas_root, CAS_SOURCE_TREE, &source_bytes)?;
        packed.insert(
            source.package().clone(),
            PackedPackage {
                locator: path.as_str().to_owned(),
                manifest: package.manifest,
                snapshot: package.snapshot,
                manifest_bytes,
                source_bytes,
            },
        );
    }
    Ok(packed)
}

struct CheckedPackage {
    manifest: PackageSourceManifestV1,
    snapshot: SourceSnapshot,
}

fn check_path_package(root: &Path) -> Result<CheckedPackage, CliError> {
    let absolute = if root.is_absolute() {
        root.to_path_buf()
    } else {
        std::path::absolute(root).map_err(|source| CliError::io(root, &source))?
    };
    let metadata =
        fs::symlink_metadata(&absolute).map_err(|source| CliError::io(&absolute, &source))?;
    if !metadata.is_dir() {
        return Err(CliError::lock(format!(
            "package root is not a directory: {}",
            absolute.display()
        )));
    }
    let manifest_path = absolute.join(PACKAGE_SOURCE_MANIFEST_FILE_NAME);
    let manifest_text = fs::read_to_string(&manifest_path).map_err(|source| {
        if source.kind() == io::ErrorKind::NotFound {
            CliError::lock(format!(
                "package source manifest is missing at {}",
                manifest_path.display()
            ))
        } else {
            CliError::io(&manifest_path, &source)
        }
    })?;
    let manifest = PackageSourceManifestV1::from_toml_str(&manifest_text)?;
    let source_id = catalog_source_id(&manifest.name, &manifest.version.to_string())?;
    let authorized = AuthorizedRoot::new(source_id, AuthorizedRootKind::Package, absolute)?;
    let snapshot = scan_included_source_snapshot(
        &authorized,
        PACKAGE_SCAN_LIMITS,
        &manifest.source_inclusion,
    )?;
    for included in &manifest.source_inclusion.include {
        let prefix = included.as_str();
        let present = snapshot.files().keys().any(|logical_path| {
            logical_path == prefix
                || logical_path.starts_with(prefix)
                    && logical_path
                        .as_bytes()
                        .get(prefix.len())
                        .is_some_and(|byte| *byte == b'/')
        });
        if !present {
            return Err(CliError::lock(format!(
                "package {} source inclusion path `{included}` is missing, empty, or fully excluded",
                manifest.name
            )));
        }
    }
    if snapshot.files().is_empty() {
        return Err(CliError::lock(format!(
            "package {} source inclusion selected no files",
            manifest.name
        )));
    }
    for entrypoint in manifest.nickel_public_entrypoints.values() {
        snapshot.resolve_path(entrypoint.as_str()).map_err(|error| {
            CliError::lock(format!(
                "package {} Nickel entrypoint `{entrypoint}` is outside its source inclusion: {error}",
                manifest.name
            ))
        })?;
    }
    Ok(CheckedPackage { manifest, snapshot })
}

fn source_acquisition_path(
    source: &BootstrapSourceProviderV1,
) -> Result<Option<&latticeaxiom_core::CanonicalLogicalPath>, CliError> {
    match source {
        BootstrapSourceProviderV1::Path { path, .. }
        | BootstrapSourceProviderV1::Fixture {
            path: Some(path), ..
        } => Ok(Some(path)),
        BootstrapSourceProviderV1::LocalCatalog { .. } => Ok(None),
        BootstrapSourceProviderV1::Workspace { package } => Err(CliError::lock(format!(
            "bootstrap source for {package} is workspace; offline lock requires path locators"
        ))),
        BootstrapSourceProviderV1::Fixture { package, .. } => Err(CliError::lock(format!(
            "bootstrap source for {package} is a path-less fixture; offline lock requires path locators"
        ))),
    }
}

fn select_closure<'a>(
    bootstrap: &'a CompositionBootstrapV1,
    packed: &'a BTreeMap<PackageName, PackedPackage>,
) -> Result<BTreeMap<PackageName, &'a PackedPackage>, CliError> {
    let mut selected = BTreeMap::new();
    let mut queue: VecDeque<PackageName> = bootstrap.roots.keys().cloned().collect();
    while let Some(name) = queue.pop_front() {
        if selected.contains_key(&name) {
            continue;
        }
        let package = packed.get(&name).ok_or_else(|| {
            CliError::lock(format!(
                "catalog has no published version of package {name}"
            ))
        })?;
        if let Some(request) = bootstrap.roots.get(&name)
            && !request.version.matches(&package.manifest.version)
        {
            return Err(CliError::lock(format!(
                "root {name} version {} does not match packed {}",
                request.version, package.manifest.version
            )));
        }
        selected.insert(name.clone(), package);
        let mut dependencies: Vec<PackageName> = package
            .manifest
            .dependencies
            .values()
            .filter(|dependency| {
                !dependency.optional
                    && domains_overlap(&dependency.domains, &bootstrap.projection_domains)
            })
            .map(|dependency| dependency.package.clone())
            .collect();
        dependencies.sort();
        for dependency in dependencies {
            if !selected.contains_key(&dependency) {
                queue.push_back(dependency);
            }
        }
    }
    Ok(selected)
}

struct RealizedPayload {
    bytes: Vec<u8>,
    producer_kind: &'static str,
    cargo_version_hash: Option<CanonicalHash>,
    rustc_version_hash: Option<CanonicalHash>,
    producer_configuration_hash: Option<CanonicalHash>,
}

fn realize_selected_packages(
    request: &LockRequest,
    bootstrap: &CompositionBootstrapV1,
    selected: &BTreeMap<PackageName, &PackedPackage>,
    catalog_root: &Path,
) -> Result<BTreeMap<PackageName, RealizedPackage>, CliError> {
    let cas_root = catalog_root.join(CLI_CAS_DIRECTORY);
    let mut realized = BTreeMap::new();
    for (name, package) in selected {
        let realization = select_realization(
            &package.manifest,
            &bootstrap.realization_policy,
            &bootstrap.projection_domains,
            &request.target,
        )?;
        let payload = realize_package_payload(package, realization, &request.target, catalog_root)?;
        let artifact_hash = put_cas(&cas_root, CAS_REALIZED_ARTIFACT, &payload.bytes)?;
        realized.insert(
            name.clone(),
            RealizedPackage {
                bytes: payload.bytes,
                receipt: ArtifactBuildReceiptV1 {
                    package: name.clone(),
                    source_hash: package.snapshot.source_hash(),
                    realization_id: realization.id.clone(),
                    realization_kind: realization.kind,
                    target: request.target.clone(),
                    requested_toolchain: request.toolchain,
                    producer_kind: payload.producer_kind,
                    cargo_version_hash: payload.cargo_version_hash,
                    rustc_version_hash: payload.rustc_version_hash,
                    producer_configuration_hash: payload.producer_configuration_hash,
                    artifact_hash,
                },
            },
        );
    }
    Ok(realized)
}

fn realize_package_payload(
    package: &PackedPackage,
    realization: &ManifestRealizationV1,
    target: &TargetTriple,
    catalog_root: &Path,
) -> Result<RealizedPayload, CliError> {
    match &realization.artifact {
        ArtifactIntent::SourceBuild => {
            source_build_payload(package, realization, target, catalog_root)
        }
        ArtifactIntent::LocalPrebuilt { path } => {
            let file = package.snapshot.resolve_path(path.as_str()).map_err(|error| {
                CliError::lock(format!(
                    "package {} prebuilt artifact `{path}` is outside its frozen source inclusion: {error}",
                    package.manifest.name
                ))
            })?;
            Ok(RealizedPayload {
                bytes: file.bytes().to_vec(),
                producer_kind: "local-prebuilt",
                cargo_version_hash: None,
                rustc_version_hash: None,
                producer_configuration_hash: None,
            })
        }
        ArtifactIntent::DataRoot { path } => data_root_payload(package, path),
    }
}

fn data_root_payload(
    package: &PackedPackage,
    root: &CanonicalLogicalPath,
) -> Result<RealizedPayload, CliError> {
    let mut files = BTreeMap::new();
    let root_path = root.as_str();
    for (logical_path, file) in package.snapshot.files() {
        if logical_path == root_path
            || logical_path.starts_with(root_path)
                && logical_path
                    .as_bytes()
                    .get(root_path.len())
                    .is_some_and(|byte| *byte == b'/')
        {
            files.insert(logical_path.clone(), file.bytes().to_vec());
        }
    }
    if files.is_empty() {
        return Err(CliError::lock(format!(
            "package {} data artifact root `{root}` selected no frozen source files",
            package.manifest.name
        )));
    }
    let artifact =
        RealizedDataRootV1::from_file_bytes(package.manifest.name.clone(), root.clone(), files)
            .map_err(|error| {
                CliError::lock(format!(
                    "package {} data artifact root `{root}` is invalid: {error}",
                    package.manifest.name
                ))
            })?;
    Ok(RealizedPayload {
        bytes: artifact.canonical_bytes().map_err(|error| {
            CliError::lock(format!(
                "package {} data artifact encoding failed: {error}",
                package.manifest.name
            ))
        })?,
        producer_kind: "data-root",
        cargo_version_hash: None,
        rustc_version_hash: None,
        producer_configuration_hash: None,
    })
}

const SOURCE_BUILD_PATH_REMAP_DESTINATION: &str = "/latticeaxiom/source-build";
const SOURCE_BUILD_RUST_ENVIRONMENT_POLICY: &str = "latticeaxiom:source-build-rust-environment/r0";

#[derive(Serialize)]
#[serde(deny_unknown_fields)]
struct SourceBuildProducerConfigurationV1 {
    schema_version: u32,
    path_remap_destination: &'static str,
    rust_environment_policy: &'static str,
}

#[derive(Deserialize)]
struct CargoManifestV1 {
    package: CargoManifestPackageV1,
}

#[derive(Deserialize)]
struct CargoManifestPackageV1 {
    name: String,
}

#[allow(clippy::too_many_lines)]
/// One uniquely-owned `SourceBuild` tree. The tree is always ephemeral: only
/// the verified artifact bytes leave it, through the caller's CAS publish.
struct SourceBuildStagingDirectory {
    parent: PathBuf,
    canonical_parent: PathBuf,
    path: PathBuf,
    cleaned: bool,
}

impl SourceBuildStagingDirectory {
    fn create(parent: &Path) -> Result<Self, CliError> {
        const MAXIMUM_CREATE_ATTEMPTS: usize = 1_024;

        fs::create_dir_all(parent).map_err(|source| CliError::io(parent, &source))?;
        let parent = std::path::absolute(parent).map_err(|source| CliError::io(parent, &source))?;
        let metadata =
            fs::symlink_metadata(&parent).map_err(|source| CliError::io(&parent, &source))?;
        if !metadata.is_dir() || source_build_is_link_or_reparse(&metadata) {
            return Err(CliError::lock(format!(
                "source build staging parent is not a regular directory: {}",
                parent.display()
            )));
        }
        let canonical_parent =
            fs::canonicalize(&parent).map_err(|source| CliError::io(&parent, &source))?;

        for _ in 0..MAXIMUM_CREATE_ATTEMPTS {
            let serial = NEXT_SOURCE_BUILD_STAGING_DIRECTORY.fetch_add(1, Ordering::Relaxed);
            let path = parent.join(format!(".staging-{}-{serial}", std::process::id()));
            match fs::create_dir(&path) {
                Ok(()) => {
                    return Ok(Self {
                        parent,
                        canonical_parent,
                        path,
                        cleaned: false,
                    });
                }
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                Err(error) => return Err(CliError::io(&path, &error)),
            }
        }
        Err(CliError::lock(format!(
            "source build could not allocate a unique staging directory below {}",
            parent.display()
        )))
    }

    fn path(&self) -> &Path {
        &self.path
    }
    fn cleanup(mut self) -> Result<(), CliError> {
        if !safe_source_build_staging_tree(&self.parent, &self.canonical_parent, &self.path) {
            return Err(CliError::lock(format!(
                "source build refused to clean an unsafe staging tree: {}",
                self.path.display()
            )));
        }
        fs::remove_dir_all(&self.path).map_err(|source| CliError::io(&self.path, &source))?;
        self.cleaned = true;
        Ok(())
    }
}

impl Drop for SourceBuildStagingDirectory {
    fn drop(&mut self) {
        if !self.cleaned
            && safe_source_build_staging_tree(&self.parent, &self.canonical_parent, &self.path)
        {
            let _ignored = fs::remove_dir_all(&self.path);
        }
    }
}

#[allow(clippy::too_many_lines)]
fn source_build_payload(
    package: &PackedPackage,
    realization: &ManifestRealizationV1,
    target: &TargetTriple,
    catalog_root: &Path,
) -> Result<RealizedPayload, CliError> {
    let entry = package
        .manifest
        .rust
        .as_ref()
        .map_or("Cargo.toml", |rust| rust.entry.as_str());
    let source_prefix = entry.strip_suffix("Cargo.toml").unwrap_or_default();
    let cargo_manifest_file = package.snapshot.resolve_path(entry).map_err(|error| {
        CliError::lock(format!(
            "package {} SourceBuild must include its declared Cargo entry in the frozen source snapshot: {error}",
            package.manifest.name
        ))
    })?;
    let cargo_manifest: CargoManifestV1 = toml::from_str(
        std::str::from_utf8(cargo_manifest_file.bytes()).map_err(|error| {
            CliError::lock(format!(
                "package {} Cargo.toml is not UTF-8: {error}",
                package.manifest.name
            ))
        })?,
    )
    .map_err(|error| {
        CliError::lock(format!(
            "package {} Cargo.toml identity is invalid: {error}",
            package.manifest.name
        ))
    })?;
    if !package
        .snapshot
        .files()
        .keys()
        .any(|logical_path| logical_path.starts_with(&format!("{source_prefix}src/")))
    {
        return Err(CliError::lock(format!(
            "package {} SourceBuild must include package-local src files",
            package.manifest.name
        )));
    }
    let extension = match realization.kind {
        RealizationKind::NativeStatic => "rlib",
        RealizationKind::PortableNative if target.as_str().contains("windows") => "dll",
        RealizationKind::PortableNative if target.as_str().contains("apple") => "dylib",
        RealizationKind::PortableNative => "so",
        unsupported => {
            return Err(CliError::lock(format!(
                "package {} SourceBuild does not yet support {unsupported:?} artifact production",
                package.manifest.name
            )));
        }
    };
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| OsString::from("cargo"));
    let rustc = std::env::var_os("RUSTC").unwrap_or_else(|| OsString::from("rustc"));
    let cargo_version_hash = command_version_hash(&cargo, "-Vv")?;
    let rustc_version_hash = command_version_hash(&rustc, "-vV")?;
    let build_root = catalog_root.join("build");
    let staging = SourceBuildStagingDirectory::create(&build_root)?;
    let staging_path = staging.path().to_str().ok_or_else(|| {
        CliError::lock(format!(
            "source build staging path is not Unicode: {}",
            staging.path().display()
        ))
    })?;
    let encoded_rustflags =
        format!("--remap-path-prefix={staging_path}={SOURCE_BUILD_PATH_REMAP_DESTINATION}");
    let producer_configuration_hash = canonical_json_hash(&SourceBuildProducerConfigurationV1 {
        schema_version: 1,
        path_remap_destination: SOURCE_BUILD_PATH_REMAP_DESTINATION,
        rust_environment_policy: SOURCE_BUILD_RUST_ENVIRONMENT_POLICY,
    })?;

    let staged_root = staging.path().join("source");
    materialize_source_snapshot(&package.snapshot, &staged_root)?;
    if let Some(rust_sources) = &package.manifest.rust {
        let members = rust_sources
            .members
            .iter()
            .map(|manifest| {
                format!(
                    "source/{}",
                    manifest.as_str().trim_end_matches("/Cargo.toml")
                )
            })
            .collect::<Vec<_>>();
        let workspace = format!(
            "[workspace]\nresolver = \"3\"\nmembers = {}\n",
            serde_json::to_string(&members).map_err(|error| CliError::lock(error.to_string()))?
        );
        fs::write(staging.path().join("Cargo.toml"), workspace)
            .map_err(|source| CliError::io(staging.path(), &source))?;
        let lock = package
            .snapshot
            .resolve_path("Cargo.lock")
            .map_err(|error| {
                CliError::lock(format!(
                    "nested source build requires a frozen Cargo.lock: {error}"
                ))
            })?;
        fs::write(staging.path().join("Cargo.lock"), lock.bytes())
            .map_err(|source| CliError::io(staging.path(), &source))?;
    }
    let target_dir =
        create_source_build_target_directory(staging.path(), target, realization.id.as_str())?;
    let manifest_path = staged_root.join(entry);
    let expected_crate_type = match realization.kind {
        RealizationKind::NativeStatic => "rlib",
        RealizationKind::PortableNative => "cdylib",
        _ => unreachable!("unsupported kinds returned before Cargo execution"),
    };
    let metadata_root = if package.manifest.rust.is_some() {
        staging.path()
    } else {
        &staged_root
    };
    let metadata = load_cargo_metadata(&cargo, &manifest_path, metadata_root)?;
    let canonical_manifest_path =
        canonical_source_build_regular_file(&manifest_path, "primary Cargo manifest")?;
    let mut primary_packages = metadata.packages.iter().filter(|candidate| {
        candidate.name == cargo_manifest.package.name
            && metadata
                .workspace_members
                .iter()
                .any(|member| member == &candidate.id)
            && fs::canonicalize(candidate.manifest_path.as_std_path())
                .is_ok_and(|path| path == canonical_manifest_path)
    });
    let primary_package = primary_packages.next().ok_or_else(|| {
        CliError::lock(format!(
            "package {} Cargo metadata did not identify its primary workspace package",
            package.manifest.name
        ))
    })?;
    if primary_packages.next().is_some() {
        return Err(CliError::lock(format!(
            "package {} Cargo metadata identified multiple primary packages",
            package.manifest.name
        )));
    }
    let expected_target = primary_package
        .targets
        .iter()
        .find(|cargo_target| cargo_target_supports(cargo_target, expected_crate_type))
        .ok_or_else(|| {
            CliError::lock(format!(
                "package {} Cargo metadata declares no {expected_crate_type} library target",
                package.manifest.name
            ))
        })?;
    let output = Command::new(&cargo)
        .arg("build")
        .arg("--manifest-path")
        .arg(&manifest_path)
        .arg("--package")
        .arg(primary_package.name.as_ref())
        .arg("--lib")
        .arg("--release")
        .arg("--frozen")
        .arg("--target")
        .arg(target.as_str())
        .arg("--target-dir")
        .arg(&target_dir)
        .arg("--message-format=json-render-diagnostics")
        .current_dir(&staged_root)
        .env("RUSTC", &rustc)
        .env_remove("RUSTFLAGS")
        .env("CARGO_ENCODED_RUSTFLAGS", &encoded_rustflags)
        .env("CARGO_INCREMENTAL", "0")
        .env_remove("RUSTC_WRAPPER")
        .env_remove("RUSTC_WORKSPACE_WRAPPER")
        .output()
        .map_err(|source| CliError::io(&manifest_path, &source))?;
    if !output.status.success() {
        return Err(CliError::lock(format!(
            "package {} SourceBuild failed: {}",
            package.manifest.name,
            bounded_diagnostic(&output.stderr)
        )));
    }
    let artifact_path = cargo_metadata::Message::parse_stream(io::Cursor::new(&output.stdout))
        .filter_map(Result::ok)
        .filter_map(|message| match message {
            cargo_metadata::Message::CompilerArtifact(artifact) => Some(artifact),
            _ => None,
        })
        .filter(|artifact| {
            artifact.package_id == primary_package.id
                && cargo_targets_match(&artifact.target, expected_target)
        })
        .flat_map(|artifact| artifact.filenames)
        .find(|path| path.extension().is_some_and(|found| found == extension))
        .ok_or_else(|| {
            CliError::lock(format!(
                "package {} SourceBuild emitted no .{extension} library for {target}",
                package.manifest.name
            ))
        })?;
    let bytes = read_source_build_artifact(
        staging.path(),
        &staged_root,
        &target_dir,
        artifact_path.as_std_path(),
    )?;
    verify_materialized_source_snapshot(&package.snapshot, &staged_root)?;
    let payload = RealizedPayload {
        bytes,
        producer_kind: "cargo-library-transitional",
        cargo_version_hash: Some(cargo_version_hash),
        rustc_version_hash: Some(rustc_version_hash),
        producer_configuration_hash: Some(producer_configuration_hash),
    };
    staging.cleanup()?;
    Ok(payload)
}

fn load_cargo_metadata(
    cargo: &OsString,
    manifest_path: &Path,
    staged_root: &Path,
) -> Result<cargo_metadata::Metadata, CliError> {
    let mut metadata_command = cargo_metadata::MetadataCommand::new();
    metadata_command
        .cargo_path(PathBuf::from(cargo))
        .manifest_path(manifest_path)
        .current_dir(staged_root)
        .other_options(vec!["--frozen".to_owned(), "--offline".to_owned()]);
    let output = metadata_command
        .cargo_command()
        .output()
        .map_err(|source| CliError::io(manifest_path, &source))?;
    if !output.status.success() {
        return Err(CliError::lock(format!(
            "Cargo metadata failed for staged package: {}",
            bounded_diagnostic(&output.stderr)
        )));
    }
    let metadata: cargo_metadata::Metadata =
        serde_json::from_slice(&output.stdout).map_err(|error| {
            CliError::lock(format!(
                "Cargo metadata emitted an invalid primary-package receipt: {error}"
            ))
        })?;
    verify_source_build_metadata_containment(&metadata, manifest_path, staged_root)?;
    Ok(metadata)
}

fn cargo_target_supports(target: &cargo_metadata::Target, expected_crate_type: &str) -> bool {
    let expected_target_kind = cargo_metadata::TargetKind::from(expected_crate_type);
    let expected_crate_type = cargo_metadata::CrateType::from(expected_crate_type);
    target.crate_types.contains(&expected_crate_type)
        && (target.kind.contains(&cargo_metadata::TargetKind::Lib)
            || target.kind.contains(&expected_target_kind))
}

fn cargo_targets_match(left: &cargo_metadata::Target, right: &cargo_metadata::Target) -> bool {
    left.name == right.name
        && left.kind == right.kind
        && left.crate_types == right.crate_types
        && left.src_path == right.src_path
}

fn verify_source_build_metadata_containment(
    metadata: &cargo_metadata::Metadata,
    manifest_path: &Path,
    staged_root: &Path,
) -> Result<(), CliError> {
    let root_metadata =
        fs::symlink_metadata(staged_root).map_err(|source| CliError::io(staged_root, &source))?;
    if !root_metadata.is_dir() || source_build_is_link_or_reparse(&root_metadata) {
        return Err(CliError::lock(format!(
            "source build root is not a regular unlinked directory: {}",
            staged_root.display()
        )));
    }
    let canonical_root =
        fs::canonicalize(staged_root).map_err(|source| CliError::io(staged_root, &source))?;
    let workspace_root = metadata.workspace_root.as_std_path();
    let canonical_workspace_root =
        fs::canonicalize(workspace_root).map_err(|source| CliError::io(workspace_root, &source))?;
    if canonical_workspace_root != canonical_root {
        return Err(CliError::lock(format!(
            "Cargo metadata workspace root {} escaped staged source root {}",
            canonical_workspace_root.display(),
            canonical_root.display()
        )));
    }

    let canonical_manifest =
        canonical_source_build_regular_file(manifest_path, "primary Cargo manifest")?;
    require_source_build_path_below(
        &canonical_root,
        &canonical_manifest,
        "primary Cargo manifest",
    )?;

    for package in &metadata.packages {
        if package.source.is_some() {
            continue;
        }
        let manifest = canonical_source_build_regular_file(
            package.manifest_path.as_std_path(),
            "local Cargo manifest",
        )?;
        require_source_build_path_below(&canonical_root, &manifest, "local Cargo manifest")?;
        for target in &package.targets {
            let source = canonical_source_build_regular_file(
                target.src_path.as_std_path(),
                "local Cargo target source",
            )?;
            require_source_build_path_below(&canonical_root, &source, "local Cargo target source")?;
        }
    }
    Ok(())
}

fn create_source_build_target_directory(
    staging_root: &Path,
    target: &TargetTriple,
    realization_id: &str,
) -> Result<PathBuf, CliError> {
    let target_root = staging_root.join("target");
    fs::create_dir(&target_root).map_err(|source| CliError::io(&target_root, &source))?;
    let target_triple_root = target_root.join(target.as_str());
    fs::create_dir(&target_triple_root)
        .map_err(|source| CliError::io(&target_triple_root, &source))?;
    let target_dir = target_triple_root.join(realization_id);
    fs::create_dir(&target_dir).map_err(|source| CliError::io(&target_dir, &source))?;
    Ok(target_dir)
}

fn read_source_build_artifact(
    staging_root: &Path,
    working_directory: &Path,
    target_dir: &Path,
    cargo_path: &Path,
) -> Result<Vec<u8>, CliError> {
    let candidate = if cargo_path.is_absolute() {
        cargo_path.to_path_buf()
    } else {
        working_directory.join(cargo_path)
    };
    let metadata =
        fs::symlink_metadata(&candidate).map_err(|source| CliError::io(&candidate, &source))?;
    if !metadata.is_file() || source_build_is_link_or_reparse(&metadata) {
        return Err(CliError::lock(format!(
            "Cargo artifact is not a regular unlinked file: {}",
            candidate.display()
        )));
    }
    let target_metadata =
        fs::symlink_metadata(target_dir).map_err(|source| CliError::io(target_dir, &source))?;
    if !target_metadata.is_dir() || source_build_is_link_or_reparse(&target_metadata) {
        return Err(CliError::lock(format!(
            "Cargo target root is not a regular unlinked directory: {}",
            target_dir.display()
        )));
    }
    let canonical_staging =
        fs::canonicalize(staging_root).map_err(|source| CliError::io(staging_root, &source))?;
    let canonical_target =
        fs::canonicalize(target_dir).map_err(|source| CliError::io(target_dir, &source))?;
    require_source_build_path_below(&canonical_staging, &canonical_target, "Cargo target root")?;
    let canonical_artifact =
        fs::canonicalize(&candidate).map_err(|source| CliError::io(&candidate, &source))?;
    require_source_build_path_below(&canonical_target, &canonical_artifact, "Cargo artifact")?;

    let mut artifact = File::open(&canonical_artifact)
        .map_err(|source| CliError::io(&canonical_artifact, &source))?;
    let opened_metadata = artifact
        .metadata()
        .map_err(|source| CliError::io(&canonical_artifact, &source))?;
    if !opened_metadata.is_file() {
        return Err(CliError::lock(format!(
            "opened Cargo artifact is not a regular file: {}",
            canonical_artifact.display()
        )));
    }
    let mut bytes = Vec::new();
    artifact
        .read_to_end(&mut bytes)
        .map_err(|source| CliError::io(&canonical_artifact, &source))?;
    Ok(bytes)
}

fn canonical_source_build_regular_file(path: &Path, label: &str) -> Result<PathBuf, CliError> {
    let metadata = fs::symlink_metadata(path).map_err(|source| CliError::io(path, &source))?;
    if !metadata.is_file() || source_build_is_link_or_reparse(&metadata) {
        return Err(CliError::lock(format!(
            "{label} is not a regular unlinked file: {}",
            path.display()
        )));
    }
    fs::canonicalize(path).map_err(|source| CliError::io(path, &source))
}

fn require_source_build_path_below(root: &Path, path: &Path, label: &str) -> Result<(), CliError> {
    if path != root && path.starts_with(root) {
        Ok(())
    } else {
        Err(CliError::lock(format!(
            "{label} {} escaped source build root {}",
            path.display(),
            root.display()
        )))
    }
}

fn materialize_source_snapshot(snapshot: &SourceSnapshot, dest: &Path) -> Result<(), CliError> {
    fs::create_dir(dest).map_err(|error| CliError::io(dest, &error))?;
    for (logical_path, source) in snapshot.files() {
        let path = join_logical(dest, logical_path);
        let Some(parent) = path.parent() else {
            return Err(CliError::lock(format!(
                "frozen source path `{logical_path}` has no parent"
            )));
        };
        fs::create_dir_all(parent).map_err(|error| CliError::io(parent, &error))?;
        let mut file = File::options()
            .write(true)
            .create_new(true)
            .open(&path)
            .map_err(|error| CliError::io(&path, &error))?;
        file.write_all(source.bytes())
            .map_err(|error| CliError::io(&path, &error))?;
        file.sync_all()
            .map_err(|error| CliError::io(&path, &error))?;
    }
    verify_materialized_source_snapshot(snapshot, dest)
}

fn verify_materialized_source_snapshot(
    snapshot: &SourceSnapshot,
    dest: &Path,
) -> Result<(), CliError> {
    let metadata = fs::symlink_metadata(dest).map_err(|error| CliError::io(dest, &error))?;
    if !metadata.is_dir() || source_build_is_link_or_reparse(&metadata) {
        return Err(CliError::lock(format!(
            "source build staging root is not a regular directory: {}",
            dest.display()
        )));
    }
    let authorized = AuthorizedRoot::new(
        snapshot.source_id().clone(),
        AuthorizedRootKind::Package,
        dest.to_path_buf(),
    )?;
    let staged = scan_source_snapshot(&authorized, PACKAGE_SCAN_LIMITS)?;
    if &staged != snapshot {
        return Err(CliError::lock(format!(
            "source build staging root {} does not exactly match frozen source {}",
            dest.display(),
            snapshot.source_hash()
        )));
    }
    Ok(())
}

fn safe_source_build_staging_tree(parent: &Path, canonical_parent: &Path, path: &Path) -> bool {
    if path.parent() != Some(parent)
        || !path
            .file_name()
            .and_then(std::ffi::OsStr::to_str)
            .is_some_and(|name| name.starts_with(".staging-"))
    {
        return false;
    }
    let Ok(parent_metadata) = fs::symlink_metadata(parent) else {
        return false;
    };
    if !parent_metadata.is_dir() || source_build_is_link_or_reparse(&parent_metadata) {
        return false;
    }
    let Ok(resolved_parent) = fs::canonicalize(parent) else {
        return false;
    };
    if resolved_parent != canonical_parent {
        return false;
    }
    let Ok(metadata) = fs::symlink_metadata(path) else {
        return false;
    };
    metadata.is_dir()
        && !source_build_is_link_or_reparse(&metadata)
        && source_build_tree_contains_only_unlinked_entries(path)
}

fn source_build_tree_contains_only_unlinked_entries(directory: &Path) -> bool {
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
        if source_build_is_link_or_reparse(&metadata) {
            return false;
        }
        if metadata.is_dir() {
            if !source_build_tree_contains_only_unlinked_entries(&path) {
                return false;
            }
        } else if !metadata.is_file() {
            return false;
        }
    }
    true
}

fn source_build_is_link_or_reparse(metadata: &fs::Metadata) -> bool {
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

fn command_version_hash(program: &OsString, flag: &str) -> Result<CanonicalHash, CliError> {
    let output = Command::new(program)
        .arg(flag)
        .output()
        .map_err(|source| CliError::io(Path::new(program), &source))?;
    if !output.status.success() {
        return Err(CliError::lock(format!(
            "build producer `{}` rejected {flag}: {}",
            program.to_string_lossy(),
            bounded_diagnostic(&output.stderr)
        )));
    }
    Ok(CanonicalHash::digest(output.stdout))
}

fn bounded_diagnostic(bytes: &[u8]) -> String {
    const MAXIMUM_DIAGNOSTIC_BYTES: usize = 8 * 1_024;
    String::from_utf8_lossy(&bytes[..bytes.len().min(MAXIMUM_DIAGNOSTIC_BYTES)]).into_owned()
}

fn domains_overlap(left: &BTreeSet<PackageDomain>, right: &BTreeSet<PackageDomain>) -> bool {
    left.iter().any(|domain| right.contains(domain))
}

struct SelectedLockGraph {
    packages: BTreeMap<PackageName, LockedPackage>,
    source_objects: BTreeMap<PackageName, CanonicalHash>,
    alias_edges: BTreeMap<PackageName, BTreeMap<PackageAlias, LockedAliasEdgeV1>>,
    capability_providers: BTreeMap<CapabilityId, Vec<PackageName>>,
    explanation: Vec<ResolutionStep>,
}

fn seal_path_lock(
    request: &LockRequest,
    bootstrap: &CompositionBootstrapV1,
    selected: &BTreeMap<PackageName, &PackedPackage>,
    realized: &BTreeMap<PackageName, RealizedPackage>,
) -> Result<LockV1, CliError> {
    let selected_graph = selected_lock_graph(bootstrap, selected, realized, &request.target)?;
    let mut graph = LockedGameGraph {
        schema_version: LOCK_SCHEMA_VERSION,
        composition_hash: bootstrap.canonical_hash()?,
        composition_provenance_hash: canonical_json_hash(&bootstrap.nickel_profile_entry)?,
        evaluation_policy: bootstrap.evaluation_policy.clone(),
        evaluation_limits: bootstrap.evaluation_limits,
        roots: bootstrap.roots.keys().cloned().collect(),
        packages: selected_graph.packages,
        capability_providers: selected_graph.capability_providers,
        namespace_grants: BTreeSet::new(),
        explanation: selected_graph.explanation,
        graph_hash: CanonicalHash::digest(b"unverified-graph"),
        lock_hash: CanonicalHash::digest(b"unverified-lock"),
    };
    graph.graph_hash = graph.recompute_graph_hash()?;
    graph.lock_hash = graph.recompute_lock_hash()?;
    graph
        .verify_hashes()
        .map_err(|source| CliError::from(ProductLockError::Graph { source }))?;

    let registration_image = placeholder_registration_image(&graph)?;
    let runtime_image = runtime_image_from_graph(&graph, registration_image.image_hash);
    let evaluation_policy_receipt_hash = canonical_json_hash(&EvaluationPolicyReceipt {
        evaluation_policy: &bootstrap.evaluation_policy,
        evaluation_limits: bootstrap.evaluation_limits,
    })?;
    let registration_semantic_hash = canonical_json_hash(&registration_image.semantics)?;
    let runtime_image_fingerprint = canonical_json_hash(&runtime_image)?;
    let build_intent_hash = canonical_json_hash(
        &realized
            .values()
            .map(|row| &row.receipt)
            .collect::<Vec<_>>(),
    )?;
    let realizations = BTreeMap::from([(
        request.target.clone(),
        TargetRealizationLockV1 {
            projection: bootstrap.projection,
            target: request.target.clone(),
            toolchain: request.toolchain,
            build_intent_hash,
            packages: target_packages(&graph),
            engine_build_id: None,
            registration_image_hash: registration_image.image_hash,
            runtime_image_fingerprint,
        },
    )]);

    Ok(LockV1::seal(ProductLockDraftV1 {
        producer: ProductLockProducerV1 {
            machine: PRODUCT_LOCK_PRODUCER_MACHINE.to_owned(),
            toolchain: request.toolchain,
        },
        bootstrap: bootstrap.clone(),
        graph,
        alias_edges: selected_graph.alias_edges,
        resolution_receipt_hash: CanonicalHash::digest(b"resolution-receipt"),
        evaluation_policy_receipt_hash,
        package_features: BTreeMap::new(),
        source_objects: selected_graph.source_objects,
        realizations,
        registration_image,
        registration_semantic_hash,
        runtime_image,
    })?)
}

fn selected_lock_graph(
    bootstrap: &CompositionBootstrapV1,
    selected: &BTreeMap<PackageName, &PackedPackage>,
    realized: &BTreeMap<PackageName, RealizedPackage>,
    target: &TargetTriple,
) -> Result<SelectedLockGraph, CliError> {
    let mut packages = BTreeMap::new();
    let mut source_objects = BTreeMap::new();
    let mut alias_edges = BTreeMap::new();
    let mut explanation = Vec::new();
    for name in bootstrap.roots.keys() {
        explanation.push(ResolutionStep::Root {
            package: name.clone(),
        });
    }
    for (name, packed) in selected {
        let artifact = realized.get(name).ok_or_else(|| {
            CliError::lock(format!(
                "selected package {name} has no realization artifact"
            ))
        })?;
        let (locked, aliases) = locked_package_from_selected(
            name,
            packed,
            artifact,
            bootstrap,
            selected,
            target,
            &mut explanation,
        )?;
        if !aliases.is_empty() {
            alias_edges.insert(name.clone(), aliases);
        }
        source_objects.insert(name.clone(), CanonicalHash::digest(&packed.source_bytes));
        packages.insert(name.clone(), locked);
    }
    let capability_providers =
        capability_providers_from_selected(bootstrap, selected, &mut explanation);
    Ok(SelectedLockGraph {
        packages,
        source_objects,
        alias_edges,
        capability_providers,
        explanation,
    })
}

fn capability_providers_from_selected(
    bootstrap: &CompositionBootstrapV1,
    selected: &BTreeMap<PackageName, &PackedPackage>,
    explanation: &mut Vec<ResolutionStep>,
) -> BTreeMap<CapabilityId, Vec<PackageName>> {
    let mut providers = BTreeMap::<CapabilityId, Vec<PackageName>>::new();
    for (name, packed) in selected {
        for provision in packed.manifest.provides.values() {
            if !domains_overlap(&provision.domains, &bootstrap.projection_domains) {
                continue;
            }
            explanation.push(ResolutionStep::Capability {
                capability: provision.capability.clone(),
                provider: name.clone(),
            });
            providers
                .entry(provision.capability.clone())
                .or_default()
                .push(name.clone());
        }
    }
    for list in providers.values_mut() {
        list.sort();
        list.dedup();
    }
    providers
}

fn locked_package_from_selected(
    name: &PackageName,
    packed: &PackedPackage,
    artifact: &RealizedPackage,
    bootstrap: &CompositionBootstrapV1,
    selected: &BTreeMap<PackageName, &PackedPackage>,
    target: &TargetTriple,
    explanation: &mut Vec<ResolutionStep>,
) -> Result<(LockedPackage, BTreeMap<PackageAlias, LockedAliasEdgeV1>), CliError> {
    let realization = select_realization(
        &packed.manifest,
        &bootstrap.realization_policy,
        &bootstrap.projection_domains,
        target,
    )?;
    let mut dependencies = BTreeMap::new();
    let mut aliases = BTreeMap::new();
    for (alias, dependency) in &packed.manifest.dependencies {
        let Some(selected_dep) = selected.get(&dependency.package) else {
            continue;
        };
        if !domains_overlap(&dependency.domains, &bootstrap.projection_domains) {
            continue;
        }
        if !dependency.version.matches(&selected_dep.manifest.version) {
            return Err(CliError::lock(format!(
                "package {name} dependency {} version {} does not match packed {}",
                dependency.package, dependency.version, selected_dep.manifest.version
            )));
        }
        explanation.push(ResolutionStep::Dependency {
            required_by: name.clone(),
            package: dependency.package.clone(),
        });
        dependencies.insert(
            dependency.package.clone(),
            LockedDependency {
                version: selected_dep.manifest.version.clone(),
                features: dependency.features.clone(),
            },
        );
        aliases.insert(
            alias.clone(),
            LockedAliasEdgeV1 {
                package: dependency.package.clone(),
                version: selected_dep.manifest.version.clone(),
                features: dependency.features.clone(),
            },
        );
    }
    Ok((
        LockedPackage {
            name: name.clone(),
            version: packed.manifest.version.clone(),
            source_id: packed.snapshot.source_id().clone(),
            source_hash: packed.snapshot.source_hash(),
            provenance_hash: canonical_json_hash(&packed.snapshot.source_id())?,
            realization: realization.kind,
            realization_id: realization.id.clone(),
            manifest_hash: CanonicalHash::digest(&packed.manifest_bytes),
            artifact_hash: CanonicalHash::digest(&artifact.bytes),
            interfaces: realization
                .interfaces
                .iter()
                .map(|(id, requirement)| (id.clone(), requirement.version.clone()))
                .collect(),
            engine_build_id: realization.engine_build,
            domains: packed.manifest.domains.clone(),
            dependencies,
            schemas: BTreeSet::<SchemaId>::new(),
            source_path: packed.locator.clone(),
        },
        aliases,
    ))
}

fn select_realization<'a>(
    manifest: &'a PackageSourceManifestV1,
    policy: &[RealizationKind],
    projection_domains: &BTreeSet<PackageDomain>,
    target: &TargetTriple,
) -> Result<&'a ManifestRealizationV1, CliError> {
    for kind in policy {
        if let Some(found) = manifest.realizations.values().find(|realization| {
            realization.kind == *kind
                && domains_overlap(&realization.domains, projection_domains)
                && (realization.targets.is_empty() || realization.targets.contains(target))
                && realization.required_features.is_empty()
        }) {
            return Ok(found);
        }
    }
    Err(CliError::lock(format!(
        "package {} has no realization matching the bootstrap policy, projection domains, target {target}, and active feature set",
        manifest.name,
    )))
}

fn placeholder_registration_image(graph: &LockedGameGraph) -> Result<RegistrationImage, CliError> {
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
    image.image_hash = image.recompute_image_hash()?;
    image
        .verify_image_hash()
        .map_err(|error| CliError::lock(error.to_string()))?;
    Ok(image)
}

fn runtime_image_from_graph(
    graph: &LockedGameGraph,
    registration_hash: CanonicalHash,
) -> RuntimeImage {
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

fn target_packages(graph: &LockedGameGraph) -> BTreeMap<PackageName, TargetPackageRealizationV1> {
    graph
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
        .collect()
}

fn verify_lock_at(lock_path: &Path, catalog_root: &Path) -> Result<LockV1, CliError> {
    let lock = reopen_product_lock(lock_path)?;
    let objects = product_lock_objects_from_cas_dir(&lock, catalog_root)?;
    let host = host_receipts_from_lock(&lock)?;
    verify_product_lock(&lock, &objects, &host, LockActionMode::Frozen)?;
    Ok(lock)
}

fn product_lock_objects_from_cas_dir(
    lock: &LockV1,
    catalog_root: &Path,
) -> Result<ProductLockObjects, CliError> {
    let cas_root = catalog_root.join(CLI_CAS_DIRECTORY);
    let mut objects = ProductLockObjects::default();
    for package in lock.portable_resolution.packages.values() {
        insert_optional(
            &mut objects.manifests,
            &cas_root,
            CAS_PACKAGE_MANIFEST,
            package.manifest_digest,
        )?;
        insert_optional(
            &mut objects.sources,
            &cas_root,
            CAS_SOURCE_TREE,
            package.source_object_digest,
        )?;
    }
    for realization in lock.realizations.values() {
        for package in realization.packages.values() {
            insert_optional(
                &mut objects.artifacts,
                &cas_root,
                CAS_REALIZED_ARTIFACT,
                package.artifact_digest,
            )?;
        }
    }
    Ok(objects)
}

fn receipt_kind(kind: &str) -> ProductLockReceiptKind {
    match kind {
        CAS_PACKAGE_MANIFEST => ProductLockReceiptKind::Manifest,
        CAS_REALIZED_ARTIFACT => ProductLockReceiptKind::Artifact,
        _ => ProductLockReceiptKind::Source,
    }
}

fn insert_optional(
    dest: &mut BTreeMap<CanonicalHash, Vec<u8>>,
    cas_root: &Path,
    kind: &str,
    digest: CanonicalHash,
) -> Result<(), CliError> {
    let path = cas_root.join(kind).join(digest.to_string());
    match fs::read(&path) {
        Ok(bytes) => {
            let actual = CanonicalHash::digest(&bytes);
            if actual != digest {
                return Err(ProductLockError::ReceiptMismatch {
                    receipt: receipt_kind(kind),
                    package: None,
                    expected: digest,
                    actual,
                }
                .into());
            }
            dest.insert(digest, bytes);
            Ok(())
        }
        Err(source) if source.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(source) => Err(CliError::io(&path, &source)),
    }
}

fn put_cas(cas_root: &Path, kind: &str, bytes: &[u8]) -> Result<CanonicalHash, CliError> {
    let digest = CanonicalHash::digest(bytes);
    let directory = cas_root.join(kind);
    fs::create_dir_all(&directory).map_err(|source| CliError::io(&directory, &source))?;
    let dest = directory.join(digest.to_string());
    match fs::read(&dest) {
        Ok(existing) if existing == bytes => return Ok(digest),
        Ok(_) => {
            return Err(CliError::lock(format!(
                "CAS object {kind}:{digest} already exists with different bytes"
            )));
        }
        Err(source) if source.kind() == io::ErrorKind::NotFound => {}
        Err(source) => return Err(CliError::io(&dest, &source)),
    }
    let temp = dest.with_extension("tmp");
    if let Err(error) = write_temporary(&temp, bytes) {
        let _ = fs::remove_file(&temp);
        return Err(error);
    }
    if let Err(source) = fs::rename(&temp, &dest) {
        let _ = fs::remove_file(&temp);
        return Err(CliError::io(&dest, &source));
    }
    Ok(digest)
}

fn write_temporary(path: &Path, bytes: &[u8]) -> Result<(), CliError> {
    let mut file = File::create(path).map_err(|source| CliError::io(path, &source))?;
    file.write_all(bytes)
        .map_err(|source| CliError::io(path, &source))?;
    file.sync_all()
        .map_err(|source| CliError::io(path, &source))?;
    Ok(())
}

fn catalog_source_id(package: &PackageName, version: &str) -> Result<SourceId, CliError> {
    let digest = CanonicalHash::digest(format!("{package}\0{version}"));
    let value = format!("latticeaxiom:source/local-catalog/{digest}");
    value
        .parse()
        .map_err(|error| CliError::lock(format!("invalid source ID `{value}`: {error}")))
}

fn absolute_dir(path: &Path) -> Result<PathBuf, CliError> {
    fs::create_dir_all(path).map_err(|source| CliError::io(path, &source))?;
    if path.is_absolute() {
        Ok(path.to_path_buf())
    } else {
        std::path::absolute(path).map_err(|source| CliError::io(path, &source))
    }
}

fn resolve_against(root: &Path, path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        let mut dest = root.to_path_buf();
        for component in path.components() {
            dest.push(component);
        }
        dest
    }
}

fn join_logical(root: &Path, logical_path: &str) -> PathBuf {
    let mut dest = root.to_path_buf();
    for segment in logical_path.split('/') {
        dest.push(segment);
    }
    dest
}

#[derive(Serialize)]
struct EvaluationPolicyReceipt<'a> {
    evaluation_policy: &'a StableId,
    evaluation_limits: NickelEvaluationLimits,
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;

    static SOURCE_BUILD_TEST_LOCK: Mutex<()> = Mutex::new(());

    struct TestDirectory(tempfile::TempDir);

    impl TestDirectory {
        fn create() -> Self {
            Self(
                tempfile::Builder::new()
                    .prefix("latticeaxiom-compose-cli-")
                    .tempdir()
                    .unwrap_or_else(|error| panic!("test directory was not created: {error}")),
            )
        }

        fn path(&self) -> &Path {
            self.0.path()
        }
    }

    fn os(args: &[&str]) -> Vec<OsString> {
        args.iter().map(OsString::from).collect()
    }

    #[test]
    fn parse_evaluate_requires_worker_and_request() {
        match parse_cli(os(&["evaluate", "--worker", "w", "--request", "r.json"])) {
            Ok(CliCommand::Evaluate { worker, request }) => {
                assert_eq!(worker, PathBuf::from("w"));
                assert_eq!(request, PathBuf::from("r.json"));
            }
            other => panic!("expected evaluate command, got {other:?}"),
        }
        assert!(parse_cli(os(&["evaluate", "--worker", "w"])).is_err());
        assert!(parse_cli(os(&["unknown"])).is_err());
    }

    #[test]
    fn parse_lock_and_resolve_accept_offline_and_defaults() {
        match parse_cli(os(&["lock", "--offline", "--workspace", "ws"])) {
            Ok(CliCommand::Lock(request)) => {
                assert_eq!(request.workspace_root, PathBuf::from("ws"));
                assert_eq!(
                    request.bootstrap_path,
                    PathBuf::from(COMPOSITION_BOOTSTRAP_FILE_NAME)
                );
                assert_eq!(request.catalog_root, PathBuf::from(CLI_CATALOG_DIRECTORY));
                assert_eq!(request.lock_path, PathBuf::from(PRODUCT_LOCK_FILE_NAME));
            }
            other => panic!("expected lock command, got {other:?}"),
        }
        assert!(matches!(
            parse_cli(os(&["resolve", "--workspace", "ws"])),
            Ok(CliCommand::Lock(_))
        ));
    }

    #[test]
    fn parse_verify_requires_offline_and_frozen() {
        match parse_cli(os(&["verify"])) {
            Err(error) => assert!(error.details.contains("--offline --frozen")),
            other => panic!("verify without flags must fail, got {other:?}"),
        }
        match parse_cli(os(&["verify", "--offline"])) {
            Err(error) => assert!(error.details.contains("--offline --frozen")),
            other => panic!("verify without --frozen must fail, got {other:?}"),
        }
        match parse_cli(os(&[
            "verify",
            "--offline",
            "--frozen",
            "--lock",
            "custom.lock",
        ])) {
            Ok(CliCommand::Verify(request)) => {
                assert_eq!(request.lock_path, PathBuf::from("custom.lock"));
            }
            other => panic!("expected verify command, got {other:?}"),
        }
        assert!(matches!(
            parse_cli(os(&["run-frozen"])),
            Ok(CliCommand::Verify(_))
        ));
    }

    #[test]
    fn clap_parser_preserves_flag_multiplicity_and_error_domains() {
        assert!(matches!(
            parse_cli(os(&[
                "verify",
                "--offline",
                "--offline",
                "--frozen",
                "--frozen"
            ])),
            Ok(CliCommand::Verify(_))
        ));
        assert!(matches!(
            parse_cli(os(&["lock", "--offline", "--offline"])),
            Ok(CliCommand::Lock(_))
        ));

        let duplicate_evaluate = parse_cli(os(&[
            "evaluate",
            "--worker",
            "first",
            "--worker",
            "second",
            "--request",
            "request.json",
        ]))
        .expect_err("duplicate evaluate value flags must be rejected");
        assert_eq!(duplicate_evaluate.code, "compose.worker_protocol");
        assert_eq!(
            duplicate_evaluate.details,
            CliError::evaluate_usage().details
        );

        let duplicate_lock = parse_cli(os(&[
            "lock",
            "--workspace",
            "first",
            "--workspace",
            "second",
        ]))
        .expect_err("duplicate lock value flags must be rejected");
        assert_eq!(duplicate_lock.code, "compose.lock");
        assert_eq!(duplicate_lock.details, CliError::usage().details);

        let invalid_target = parse_cli(os(&["lock", "--target", "invalid"]))
            .expect_err("invalid target must retain its domain diagnostic");
        assert_eq!(invalid_target.code, "compose.lock");
        assert!(
            invalid_target
                .details
                .starts_with("invalid target `invalid`:")
        );

        let invalid_toolchain = parse_cli(os(&["lock", "--toolchain", "invalid"]))
            .expect_err("invalid toolchain must retain its domain diagnostic");
        assert_eq!(invalid_toolchain.code, "compose.lock");
        assert!(
            invalid_toolchain
                .details
                .starts_with("invalid toolchain `invalid`:")
        );
    }

    fn write_fixture_workspace(root: &Path) {
        let package_dir = root.join("packages").join("terrain");
        fs::create_dir_all(package_dir.join("data"))
            .unwrap_or_else(|error| panic!("fixture package directory was not created: {error}"));
        fs::write(
            root.join(COMPOSITION_BOOTSTRAP_FILE_NAME),
            r#"
schema_version = 1
projection = "headless-test"
projection_domains = ["authoritative"]
evaluation_policy = "latticeaxiom:nickel-evaluation-policy/r0@1"
realization_policy = ["data"]
nickel_profile_entry = "profiles/test.ncl"

[roots.terrain]
version = "=1.0.0"
realization = { mode = "auto" }

[[sources]]
kind = "path"
package = "terrain"
path = "packages/terrain"
"#,
        )
        .unwrap_or_else(|error| panic!("bootstrap was not written: {error}"));
        fs::write(
            package_dir.join(PACKAGE_SOURCE_MANIFEST_FILE_NAME),
            r#"
schema_version = 1
name = "terrain"
version = "1.0.0"
domains = ["authoritative"]
trust = "data-only"

[realizations.data]
id = "data"
kind = "data"
domains = ["authoritative"]
artifact = { kind = "data-root", path = "data" }
trust = "data-only"

[nickel_public_entrypoints]
default = "package.ncl"

[source_inclusion]
include = ["data", "package.ncl", "latticeaxiom-package.toml"]
"#,
        )
        .unwrap_or_else(|error| panic!("package manifest was not written: {error}"));
        fs::write(package_dir.join("package.ncl"), "{}\n")
            .unwrap_or_else(|error| panic!("package nickel was not written: {error}"));
        fs::write(
            package_dir.join("data").join("terrain-catalog-v1.json"),
            b"{\"schema_version\":1,\"terrain\":\"fixture\"}\n",
        )
        .unwrap_or_else(|error| panic!("terrain data descriptor was not written: {error}"));
    }

    fn write_source_build_fixture_workspace(root: &Path) -> PathBuf {
        let package_dir = root.join("packages").join("source-build");
        fs::create_dir_all(package_dir.join("src"))
            .unwrap_or_else(|error| panic!("source-build fixture directory failed: {error}"));
        fs::write(
            root.join(COMPOSITION_BOOTSTRAP_FILE_NAME),
            r#"
schema_version = 1
projection = "headless-test"
projection_domains = ["authoritative"]
evaluation_policy = "latticeaxiom:nickel-evaluation-policy/r0@1"
realization_policy = ["native-static"]
nickel_profile_entry = "profiles/test.ncl"

[roots.source-build]
version = "=1.0.0"
realization = { mode = "auto" }

[[sources]]
kind = "path"
package = "source-build"
path = "packages/source-build"
"#,
        )
        .unwrap_or_else(|error| panic!("source-build bootstrap was not written: {error}"));
        fs::write(
            package_dir.join(PACKAGE_SOURCE_MANIFEST_FILE_NAME),
            r#"
schema_version = 1
name = "source-build"
version = "1.0.0"
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
include = ["Cargo.lock", "Cargo.toml", "package.ncl", "latticeaxiom-package.toml", "src"]
"#,
        )
        .unwrap_or_else(|error| panic!("source-build package manifest failed: {error}"));
        fs::write(package_dir.join("package.ncl"), "{}\n")
            .unwrap_or_else(|error| panic!("source-build Nickel failed: {error}"));
        fs::write(
            package_dir.join("Cargo.toml"),
            r#"[package]
name = "fixture-source-build"
version = "0.1.0"
edition = "2024"
publish = false

[lib]
crate-type = ["rlib"]
"#,
        )
        .unwrap_or_else(|error| panic!("source-build Cargo manifest failed: {error}"));
        fs::write(
            package_dir.join("Cargo.lock"),
            r#"# This file is automatically @generated by Cargo.
# It is not intended for manual editing.
version = 4

[[package]]
name = "fixture-source-build"
version = "0.1.0"
"#,
        )
        .unwrap_or_else(|error| panic!("source-build Cargo lock failed: {error}"));
        fs::write(
            package_dir.join("src").join("lib.rs"),
            "pub fn fixture_state() -> u64 { 41 }\n",
        )
        .unwrap_or_else(|error| panic!("source-build Rust source failed: {error}"));
        package_dir
    }

    #[test]
    fn source_build_hashes_package_code_and_freezes_real_artifact_bytes() {
        let _guard = SOURCE_BUILD_TEST_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let directory = TestDirectory::create();
        let package_dir = write_source_build_fixture_workspace(directory.path());
        let request = LockRequest {
            workspace_root: directory.path().to_path_buf(),
            bootstrap_path: PathBuf::from(COMPOSITION_BOOTSTRAP_FILE_NAME),
            catalog_root: PathBuf::from(CLI_CATALOG_DIRECTORY),
            lock_path: PathBuf::from(PRODUCT_LOCK_FILE_NAME),
            target: controller_host_target()
                .unwrap_or_else(|error| panic!("host target must be supported: {error}")),
            toolchain: default_toolchain(),
        };
        let package_name: PackageName = "source-build"
            .parse()
            .unwrap_or_else(|error| panic!("source-build package name must parse: {error}"));
        let first = check_path_package(&package_dir)
            .unwrap_or_else(|error| panic!("source-build package must check: {error}"));
        let (first_lock, _) = lock_workspace(&request)
            .unwrap_or_else(|error| panic!("initial source-build lock failed: {error}"));
        let first_artifact_digest = first_lock
            .realizations
            .get(&request.target)
            .and_then(|realization| realization.packages.get(&package_name))
            .map_or_else(
                || panic!("source-build target artifact must be locked"),
                |package| package.artifact_digest,
            );

        fs::write(package_dir.join("excluded.txt"), "not an input\n")
            .unwrap_or_else(|error| panic!("excluded source failed: {error}"));
        let excluded = check_path_package(&package_dir)
            .unwrap_or_else(|error| panic!("package with excluded file must check: {error}"));
        assert_eq!(
            first.snapshot.source_hash(),
            excluded.snapshot.source_hash()
        );
        let (lock, _) = lock_workspace(&request)
            .unwrap_or_else(|error| panic!("source-build relock failed: {error}"));
        let portable_package = lock
            .portable_resolution
            .packages
            .get(&package_name)
            .unwrap_or_else(|| panic!("source-build portable package must be locked"));
        let target_package = lock
            .realizations
            .get(&request.target)
            .and_then(|realization| realization.packages.get(&package_name))
            .unwrap_or_else(|| panic!("source-build target artifact must be locked"));
        assert_eq!(first_artifact_digest, target_package.artifact_digest);
        assert_ne!(
            portable_package.source_object_digest,
            target_package.artifact_digest
        );

        fs::write(
            package_dir.join("src").join("lib.rs"),
            "pub fn fixture_state() -> u64 { 42 }\n",
        )
        .unwrap_or_else(|error| panic!("included source mutation failed: {error}"));
        let changed = check_path_package(&package_dir)
            .unwrap_or_else(|error| panic!("mutated package must check: {error}"));
        assert_ne!(first.snapshot.source_hash(), changed.snapshot.source_hash());

        let artifact_path = directory
            .path()
            .join(CLI_CATALOG_DIRECTORY)
            .join(CLI_CAS_DIRECTORY)
            .join(CAS_REALIZED_ARTIFACT)
            .join(target_package.artifact_digest.to_string());
        let artifact = fs::read(&artifact_path)
            .unwrap_or_else(|error| panic!("realized artifact must exist: {error}"));
        assert!(artifact.starts_with(b"!<arch>\n"));

        fs::remove_dir_all(&package_dir)
            .unwrap_or_else(|error| panic!("mutable package removal failed: {error}"));
        let verify = VerifyRequest {
            workspace_root: directory.path().to_path_buf(),
            lock_path: PathBuf::from(PRODUCT_LOCK_FILE_NAME),
            catalog_root: PathBuf::from(CLI_CATALOG_DIRECTORY),
        };
        verify_workspace_frozen(&verify)
            .unwrap_or_else(|error| panic!("frozen verify must not read mutable package: {error}"));
        fs::write(&artifact_path, b"tampered-artifact\n")
            .unwrap_or_else(|error| panic!("artifact tamper failed: {error}"));
        let error = verify_workspace_frozen(&verify)
            .err()
            .unwrap_or_else(|| panic!("tampered artifact must fail frozen verification"));
        assert!(error.details.contains("artifact") || error.details.contains("mismatch"));
    }

    #[test]
    fn source_build_uses_declared_nested_crate_entry() {
        let _guard = SOURCE_BUILD_TEST_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let directory = TestDirectory::create();
        let package = write_source_build_fixture_workspace(directory.path());
        let manifest_path = package.join(PACKAGE_SOURCE_MANIFEST_FILE_NAME);
        let text = fs::read_to_string(&manifest_path)
            .expect("fixture manifest")
            .replace("\"Cargo.toml\",", "")
            .replace("\"src\"]", "\"crates\"]");
        fs::write(&manifest_path, format!("{text}\n[rust]\nentry = 'crates/runtime/Cargo.toml'\nmembers = ['crates/runtime/Cargo.toml']\n")).expect("nested manifest");
        let crate_dir = package.join("crates/runtime");
        fs::create_dir_all(&crate_dir).expect("crate folder");
        fs::rename(package.join("Cargo.toml"), crate_dir.join("Cargo.toml"))
            .expect("move manifest inside package");
        fs::rename(package.join("src"), crate_dir.join("src"))
            .expect("move implementation inside package");
        let request = LockRequest {
            workspace_root: directory.path().to_path_buf(),
            bootstrap_path: PathBuf::from(COMPOSITION_BOOTSTRAP_FILE_NAME),
            catalog_root: PathBuf::from(CLI_CATALOG_DIRECTORY),
            lock_path: PathBuf::from(PRODUCT_LOCK_FILE_NAME),
            target: controller_host_target().expect("host target"),
            toolchain: default_toolchain(),
        };
        let (lock, _) = lock_workspace(&request).expect("nested source realization");
        assert!(lock.realizations.contains_key(&request.target));
        assert!(
            !package.join("Cargo.toml").exists(),
            "builder must not rewrite the source package"
        );
    }

    #[test]
    fn source_build_failure_cleans_owned_staging_and_retry_skips_poison() {
        let _guard = SOURCE_BUILD_TEST_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let directory = TestDirectory::create();
        let package_dir = write_source_build_fixture_workspace(directory.path());
        fs::write(
            package_dir.join("src").join("lib.rs"),
            "compile_error!(\"intentional SourceBuild failure\");\n",
        )
        .unwrap_or_else(|error| panic!("failing source fixture was not written: {error}"));
        let request = LockRequest {
            workspace_root: directory.path().to_path_buf(),
            bootstrap_path: PathBuf::from(COMPOSITION_BOOTSTRAP_FILE_NAME),
            catalog_root: PathBuf::from(CLI_CATALOG_DIRECTORY),
            lock_path: PathBuf::from(PRODUCT_LOCK_FILE_NAME),
            target: controller_host_target()
                .unwrap_or_else(|error| panic!("host target must be supported: {error}")),
            toolchain: default_toolchain(),
        };

        let build_root = directory.path().join(CLI_CATALOG_DIRECTORY).join("build");
        fs::create_dir_all(&build_root)
            .unwrap_or_else(|error| panic!("build root was not created: {error}"));
        let poisoned_serial = NEXT_SOURCE_BUILD_STAGING_DIRECTORY.load(Ordering::Relaxed);
        let poisoned =
            build_root.join(format!(".staging-{}-{poisoned_serial}", std::process::id()));
        fs::create_dir(&poisoned)
            .unwrap_or_else(|error| panic!("poisoned staging tree was not created: {error}"));
        fs::write(
            poisoned.join("foreign-marker"),
            b"must remain owned by the test\n",
        )
        .unwrap_or_else(|error| panic!("poison marker was not written: {error}"));

        let error = lock_workspace(&request)
            .err()
            .unwrap_or_else(|| panic!("intentional compiler failure must reject SourceBuild"));
        assert!(
            error.details.contains("SourceBuild failed"),
            "unexpected SourceBuild diagnostic: {}",
            error.details
        );
        let mut staging_entries = fs::read_dir(&build_root)
            .unwrap_or_else(|error| panic!("build root must remain readable: {error}"))
            .filter_map(Result::ok)
            .filter(|entry| entry.file_name().to_string_lossy().starts_with(".staging-"))
            .map(|entry| entry.file_name())
            .collect::<Vec<_>>();
        staging_entries.sort();
        assert_eq!(
            staging_entries,
            vec![
                poisoned
                    .file_name()
                    .unwrap_or_else(|| panic!("poison path must have a file name"))
                    .to_os_string()
            ],
            "failed build must clean only the staging tree it created"
        );

        fs::write(
            package_dir.join("src").join("lib.rs"),
            "pub fn fixture_state() -> u64 { 42 }\n",
        )
        .unwrap_or_else(|error| panic!("repaired source fixture was not written: {error}"));
        lock_workspace(&request)
            .unwrap_or_else(|error| panic!("SourceBuild retry must succeed: {error}"));
        assert_eq!(
            fs::read(poisoned.join("foreign-marker"))
                .unwrap_or_else(|error| panic!("foreign poison must remain untouched: {error}")),
            b"must remain owned by the test\n"
        );
        fs::remove_dir_all(&poisoned)
            .unwrap_or_else(|error| panic!("test poison cleanup failed: {error}"));
        assert!(
            fs::read_dir(&build_root)
                .unwrap_or_else(|error| panic!("build root must remain readable: {error}"))
                .filter_map(Result::ok)
                .all(|entry| !entry.file_name().to_string_lossy().starts_with(".staging-")),
            "successful retry must leave no owned staging tree"
        );
    }

    #[test]
    fn cargo_metadata_rejects_local_dependency_outside_staged_source() {
        let directory = TestDirectory::create();
        let staged_root = directory.path().join("staged");
        let escaped_root = directory.path().join("escaped");
        fs::create_dir_all(staged_root.join("src"))
            .unwrap_or_else(|error| panic!("staged source directory was not created: {error}"));
        fs::create_dir_all(escaped_root.join("src"))
            .unwrap_or_else(|error| panic!("escaped source directory was not created: {error}"));
        fs::write(
            staged_root.join("Cargo.toml"),
            r#"[package]
name = "staged-primary"
version = "0.1.0"
edition = "2024"
publish = false

[dependencies]
escaped-local = { path = "../escaped" }
"#,
        )
        .unwrap_or_else(|error| panic!("staged Cargo manifest was not written: {error}"));
        fs::write(
            staged_root.join("Cargo.lock"),
            r#"# This file is automatically @generated by Cargo.
# It is not intended for manual editing.
version = 4

[[package]]
name = "escaped-local"
version = "0.1.0"

[[package]]
name = "staged-primary"
version = "0.1.0"
dependencies = [
 "escaped-local",
]
"#,
        )
        .unwrap_or_else(|error| panic!("staged Cargo lock was not written: {error}"));
        fs::write(
            staged_root.join("src").join("lib.rs"),
            "pub fn staged() {}\n",
        )
        .unwrap_or_else(|error| panic!("staged source was not written: {error}"));
        fs::write(
            escaped_root.join("Cargo.toml"),
            r#"[package]
name = "escaped-local"
version = "0.1.0"
edition = "2024"
publish = false
"#,
        )
        .unwrap_or_else(|error| panic!("escaped Cargo manifest was not written: {error}"));
        fs::write(
            escaped_root.join("src").join("lib.rs"),
            "pub fn escaped() {}\n",
        )
        .unwrap_or_else(|error| panic!("escaped source was not written: {error}"));

        let cargo = std::env::var_os("CARGO").unwrap_or_else(|| OsString::from("cargo"));
        let manifest_path = staged_root.join("Cargo.toml");
        let error = load_cargo_metadata(&cargo, &manifest_path, &staged_root)
            .err()
            .unwrap_or_else(|| panic!("external local dependency must fail containment"));
        assert!(
            error.details.contains("local Cargo manifest")
                && error.details.contains("escaped source build root"),
            "unexpected local dependency diagnostic: {}",
            error.details
        );
    }

    #[test]
    fn source_build_artifact_must_be_regular_and_below_owned_target() {
        let directory = TestDirectory::create();
        let staging_root = directory.path().join("staging");
        let working_directory = staging_root.join("source");
        let target_dir = staging_root.join("target");
        fs::create_dir_all(&working_directory)
            .unwrap_or_else(|error| panic!("working directory was not created: {error}"));
        fs::create_dir(&target_dir)
            .unwrap_or_else(|error| panic!("target directory was not created: {error}"));

        let artifact_path = target_dir.join("libfixture.rlib");
        fs::write(&artifact_path, b"verified artifact bytes")
            .unwrap_or_else(|error| panic!("artifact fixture was not written: {error}"));
        assert_eq!(
            read_source_build_artifact(
                &staging_root,
                &working_directory,
                &target_dir,
                &artifact_path,
            )
            .unwrap_or_else(|error| panic!("contained artifact must read: {error}")),
            b"verified artifact bytes"
        );

        let escaped_path = staging_root.join("escaped.rlib");
        fs::write(&escaped_path, b"outside target")
            .unwrap_or_else(|error| panic!("escaped artifact fixture was not written: {error}"));
        let escaped = read_source_build_artifact(
            &staging_root,
            &working_directory,
            &target_dir,
            &escaped_path,
        )
        .err()
        .unwrap_or_else(|| panic!("artifact outside owned target must fail"));
        assert!(
            escaped.details.contains("Cargo artifact")
                && escaped.details.contains("escaped source build root"),
            "unexpected artifact escape diagnostic: {}",
            escaped.details
        );

        let directory_error =
            read_source_build_artifact(&staging_root, &working_directory, &target_dir, &target_dir)
                .err()
                .unwrap_or_else(|| panic!("artifact directory must fail regular-file proof"));
        assert!(
            directory_error
                .details
                .contains("not a regular unlinked file"),
            "unexpected artifact kind diagnostic: {}",
            directory_error.details
        );
    }

    #[test]
    fn package_check_reports_missing_source_inclusion_stably() {
        let directory = TestDirectory::create();
        let package_dir = write_source_build_fixture_workspace(directory.path());
        let manifest_path = package_dir.join(PACKAGE_SOURCE_MANIFEST_FILE_NAME);
        let manifest = fs::read_to_string(&manifest_path)
            .unwrap_or_else(|error| panic!("fixture manifest must be readable: {error}"));
        let missing = manifest.replacen(
            "\"latticeaxiom-package.toml\", \"src\"]",
            "\"latticeaxiom-package.toml\", \"src\", \"missing\"]",
            1,
        );
        assert_ne!(manifest, missing, "fixture inclusion row must be replaced");
        fs::write(&manifest_path, missing)
            .unwrap_or_else(|error| panic!("fixture manifest mutation failed: {error}"));

        let error = check_path_package(&package_dir)
            .err()
            .unwrap_or_else(|| panic!("missing explicit inclusion must fail package check"));
        assert!(
            error
                .details
                .contains("source inclusion path `missing` is missing, empty, or fully excluded"),
            "unexpected missing-inclusion diagnostic: {}",
            error.details
        );
    }

    #[test]
    fn lock_writes_product_lock_and_frozen_verify_rejects_tampered_source() {
        let directory = TestDirectory::create();
        write_fixture_workspace(directory.path());
        let request = LockRequest {
            workspace_root: directory.path().to_path_buf(),
            bootstrap_path: PathBuf::from(COMPOSITION_BOOTSTRAP_FILE_NAME),
            catalog_root: PathBuf::from(CLI_CATALOG_DIRECTORY),
            lock_path: PathBuf::from(PRODUCT_LOCK_FILE_NAME),
            target: "x86_64-pc-windows-msvc"
                .parse()
                .unwrap_or_else(|error| panic!("fixture target is invalid: {error}")),
            toolchain: default_toolchain(),
        };
        let (lock, report) =
            lock_workspace(&request).unwrap_or_else(|error| panic!("lock failed: {error}"));
        assert_eq!(report.command, "lock");
        assert_eq!(report.product_lock_hash, lock.product_lock_hash);
        assert!(directory.path().join(PRODUCT_LOCK_FILE_NAME).is_file());

        let verify = VerifyRequest {
            workspace_root: directory.path().to_path_buf(),
            lock_path: PathBuf::from(PRODUCT_LOCK_FILE_NAME),
            catalog_root: PathBuf::from(CLI_CATALOG_DIRECTORY),
        };
        verify_workspace_frozen(&verify)
            .unwrap_or_else(|error| panic!("frozen verify of a fresh lock failed: {error}"));

        let source_digest = lock
            .portable_resolution
            .packages
            .values()
            .next()
            .map_or_else(
                || panic!("lock must seal a source object"),
                |package| package.source_object_digest,
            );
        let source_path = directory
            .path()
            .join(CLI_CATALOG_DIRECTORY)
            .join(CLI_CAS_DIRECTORY)
            .join(CAS_SOURCE_TREE)
            .join(source_digest.to_string());
        fs::write(&source_path, b"tampered-source-digest\n")
            .unwrap_or_else(|error| panic!("tamper write failed: {error}"));
        match verify_workspace_frozen(&verify) {
            Err(error) => {
                assert!(
                    error.details.contains("source") || error.details.contains("mismatch"),
                    "frozen verify must name the tampered source receipt, got {error}"
                );
            }
            Ok(_) => panic!("tampered source must fail frozen verify"),
        }

        fs::remove_file(&source_path)
            .unwrap_or_else(|error| panic!("missing-receipt setup failed: {error}"));
        match verify_workspace_frozen(&verify) {
            Err(error) => {
                assert!(
                    error.details.contains("missing") || error.details.contains("source"),
                    "frozen verify must fail closed on a missing receipt, got {error}"
                );
            }
            Ok(_) => panic!("missing source must fail frozen verify"),
        }
    }
}
