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

use latticeaxiom_core::{
    CanonicalHash, CanonicalJsonError, CapabilityId, PackageName, SchemaId, SourceId, StableId,
    TargetTriple, canonical_json_bytes, canonical_json_hash,
};
use serde::Serialize;
use thiserror::Error;

use crate::{
    AuthorizedRoot, AuthorizedRootKind, BootstrapManifestError, BootstrapSourceProviderV1,
    COMPOSITION_BOOTSTRAP_FILE_NAME, CompositionBootstrapV1, LOCK_SCHEMA_VERSION, LockActionMode,
    LockV1, LockedAliasEdgeV1, LockedDependency, LockedGameGraph, LockedPackage,
    ManifestRealizationV1, NickelEvaluationLimits, ObservabilityCatalog,
    PACKAGE_SOURCE_MANIFEST_FILE_NAME, PRODUCT_LOCK_FILE_NAME, PRODUCT_LOCK_PRODUCER_MACHINE,
    PackageAlias, PackageDomain, PackageSourceManifestV1, ProductLockDraftV1, ProductLockError,
    ProductLockHostReceipts, ProductLockObjects, ProductLockProducerV1, ProductLockReceiptKind,
    RealizationKind, RegistrationImage, ResolutionStep, RuntimeBinding, RuntimeImage,
    SemanticCatalog, SettingsCatalog, SourceScanError, SourceScanLimits, SourceSnapshot,
    TargetPackageRealizationV1, TargetRealizationLockV1, controller_host_target,
    persist_product_lock, reopen_product_lock, scan_source_snapshot, verify_product_lock,
};

/// Directory that receives the local catalog index and CAS objects.
pub const CLI_CATALOG_DIRECTORY: &str = "catalog";

/// Object-store directory beside the catalog root.
pub const CLI_CAS_DIRECTORY: &str = "cas";

const CAS_SOURCE_TREE: &str = "source-tree";
const CAS_PACKAGE_MANIFEST: &str = "package-manifest";
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
    let mut arguments = arguments.into_iter();
    let command = arguments.next().ok_or_else(CliError::usage)?;
    if command == "evaluate" {
        return parse_evaluate(arguments);
    }
    if command == "lock" || command == "resolve" {
        return parse_lock(arguments);
    }
    if command == "verify" {
        return parse_verify(arguments, false);
    }
    if command == "run-frozen" {
        return parse_verify(arguments, true);
    }
    Err(CliError::usage())
}

fn parse_evaluate(mut arguments: impl Iterator<Item = OsString>) -> Result<CliCommand, CliError> {
    let mut worker = None;
    let mut request = None;
    while let Some(flag) = arguments.next() {
        let value = arguments.next().ok_or_else(CliError::evaluate_usage)?;
        if flag == "--worker" && worker.is_none() {
            worker = Some(PathBuf::from(value));
        } else if flag == "--request" && request.is_none() {
            request = Some(PathBuf::from(value));
        } else {
            return Err(CliError::evaluate_usage());
        }
    }
    Ok(CliCommand::Evaluate {
        worker: worker.ok_or_else(CliError::evaluate_usage)?,
        request: request.ok_or_else(CliError::evaluate_usage)?,
    })
}

fn parse_lock(arguments: impl Iterator<Item = OsString>) -> Result<CliCommand, CliError> {
    let mut workspace = None;
    let mut bootstrap = None;
    let mut catalog = None;
    let mut lock = None;
    let mut target = None;
    let mut toolchain = None;
    let mut flags = FlagParser::new(arguments);
    while let Some(flag) = flags.next_flag() {
        if flag == "--workspace" && workspace.is_none() {
            workspace = Some(PathBuf::from(flags.value()?));
        } else if flag == "--bootstrap" && bootstrap.is_none() {
            bootstrap = Some(PathBuf::from(flags.value()?));
        } else if flag == "--catalog" && catalog.is_none() {
            catalog = Some(PathBuf::from(flags.value()?));
        } else if flag == "--lock" && lock.is_none() {
            lock = Some(PathBuf::from(flags.value()?));
        } else if flag == "--target" && target.is_none() {
            let value = flags.value()?;
            let text = value.to_string_lossy();
            target =
                Some(text.parse::<TargetTriple>().map_err(|error| {
                    CliError::lock(format!("invalid target `{text}`: {error}"))
                })?);
        } else if flag == "--toolchain" && toolchain.is_none() {
            let value = flags.value()?;
            let text = value.to_string_lossy();
            toolchain =
                Some(text.parse::<CanonicalHash>().map_err(|error| {
                    CliError::lock(format!("invalid toolchain `{text}`: {error}"))
                })?);
        } else if flag != "--offline" {
            return Err(CliError::usage());
        }
    }
    Ok(CliCommand::Lock(LockRequest {
        workspace_root: workspace.unwrap_or_else(default_workspace),
        bootstrap_path: bootstrap.unwrap_or_else(|| PathBuf::from(COMPOSITION_BOOTSTRAP_FILE_NAME)),
        catalog_root: catalog.unwrap_or_else(|| PathBuf::from(CLI_CATALOG_DIRECTORY)),
        lock_path: lock.unwrap_or_else(|| PathBuf::from(PRODUCT_LOCK_FILE_NAME)),
        target: match target {
            Some(target) => target,
            None => controller_host_target().map_err(|error| CliError::lock(error.to_string()))?,
        },
        toolchain: toolchain.unwrap_or_else(default_toolchain),
    }))
}

fn parse_verify(
    arguments: impl Iterator<Item = OsString>,
    implied_frozen: bool,
) -> Result<CliCommand, CliError> {
    let mut workspace = None;
    let mut catalog = None;
    let mut lock = None;
    let mut offline = implied_frozen;
    let mut frozen = implied_frozen;
    let mut flags = FlagParser::new(arguments);
    while let Some(flag) = flags.next_flag() {
        if flag == "--workspace" && workspace.is_none() {
            workspace = Some(PathBuf::from(flags.value()?));
        } else if flag == "--catalog" && catalog.is_none() {
            catalog = Some(PathBuf::from(flags.value()?));
        } else if flag == "--lock" && lock.is_none() {
            lock = Some(PathBuf::from(flags.value()?));
        } else if flag == "--offline" {
            offline = true;
        } else if flag == "--frozen" {
            frozen = true;
        } else {
            return Err(CliError::usage());
        }
    }
    if !offline || !frozen {
        return Err(CliError::lock(
            "verify requires --offline --frozen so missing or tampered receipts fail closed",
        ));
    }
    Ok(CliCommand::Verify(VerifyRequest {
        workspace_root: workspace.unwrap_or_else(default_workspace),
        lock_path: lock.unwrap_or_else(|| PathBuf::from(PRODUCT_LOCK_FILE_NAME)),
        catalog_root: catalog.unwrap_or_else(|| PathBuf::from(CLI_CATALOG_DIRECTORY)),
    }))
}

struct FlagParser<I> {
    arguments: I,
}

impl<I: Iterator<Item = OsString>> FlagParser<I> {
    fn new(arguments: I) -> Self {
        Self { arguments }
    }

    fn next_flag(&mut self) -> Option<OsString> {
        self.arguments.next()
    }

    fn value(&mut self) -> Result<OsString, CliError> {
        self.arguments.next().ok_or_else(CliError::usage)
    }
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
    let lock = seal_path_lock(request, &bootstrap, &selected)?;
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

struct PackedPackage {
    locator: String,
    manifest: PackageSourceManifestV1,
    snapshot: SourceSnapshot,
    manifest_bytes: Vec<u8>,
    source_bytes: Vec<u8>,
    artifact_bytes: Vec<u8>,
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
        put_cas(&cas_root, CAS_REALIZED_ARTIFACT, &source_bytes)?;
        packed.insert(
            source.package().clone(),
            PackedPackage {
                locator: path.as_str().to_owned(),
                manifest: package.manifest,
                snapshot: package.snapshot,
                manifest_bytes,
                source_bytes,
                artifact_bytes: Vec::new(),
            },
        );
    }
    for package in packed.values_mut() {
        package.artifact_bytes.clone_from(&package.source_bytes);
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
    let snapshot = scan_source_snapshot(&authorized, PACKAGE_SCAN_LIMITS)?;
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
) -> Result<LockV1, CliError> {
    let selected_graph = selected_lock_graph(bootstrap, selected)?;
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
    let realizations = BTreeMap::from([(
        request.target.clone(),
        TargetRealizationLockV1 {
            projection: bootstrap.projection,
            target: request.target.clone(),
            toolchain: request.toolchain,
            build_intent_hash: CanonicalHash::digest(b"build-intent"),
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
        let (locked, aliases) =
            locked_package_from_selected(name, packed, bootstrap, selected, &mut explanation)?;
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
    bootstrap: &CompositionBootstrapV1,
    selected: &BTreeMap<PackageName, &PackedPackage>,
    explanation: &mut Vec<ResolutionStep>,
) -> Result<(LockedPackage, BTreeMap<PackageAlias, LockedAliasEdgeV1>), CliError> {
    let realization = select_realization(&packed.manifest, &bootstrap.realization_policy)?;
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
            artifact_hash: CanonicalHash::digest(&packed.artifact_bytes),
            interfaces: BTreeMap::new(),
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
) -> Result<&'a ManifestRealizationV1, CliError> {
    for kind in policy {
        if let Some(found) = manifest
            .realizations
            .values()
            .find(|realization| realization.kind == *kind)
        {
            return Ok(found);
        }
    }
    Err(CliError::lock(format!(
        "package {} has no realization matching the bootstrap policy",
        manifest.name
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
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::*;

    static NEXT_TEST_DIRECTORY: AtomicU64 = AtomicU64::new(0);

    struct TestDirectory(PathBuf);

    impl TestDirectory {
        fn create() -> Self {
            let serial = NEXT_TEST_DIRECTORY.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "latticeaxiom-compose-cli-{}-{serial}",
                std::process::id()
            ));
            fs::create_dir_all(&path)
                .unwrap_or_else(|error| panic!("test directory was not created: {error}"));
            Self(
                std::path::absolute(&path).unwrap_or_else(|error| {
                    panic!("test directory was not made absolute: {error}")
                }),
            )
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
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

    fn write_fixture_workspace(root: &Path) {
        let package_dir = root.join("packages").join("terrain");
        fs::create_dir_all(&package_dir)
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
include = ["package.ncl", "latticeaxiom-package.toml"]
"#,
        )
        .unwrap_or_else(|error| panic!("package manifest was not written: {error}"));
        fs::write(package_dir.join("package.ncl"), "{}\n")
            .unwrap_or_else(|error| panic!("package nickel was not written: {error}"));
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
