//! One offline bootstrap-first lock transaction.
//!
//! The sequence is [`CompositionBootstrapV1`] → local catalog
//! check/pack/publish/acquire → [`PackageResolver`] → persist [`LockV1`] →
//! reopen `--frozen`. Path values are acquisition locators only. Runnable
//! identity is catalogued CAS object locators. Nickel composition is not
//! evaluated from the lock; [`LockV1`] still requires composition and
//! registration receipts, which are filled from the same in-memory empty
//! image helper used by product-lock tests rather than invented hashes.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use latticeaxiom_compose::{
    ArtifactIntent, BootstrapSourceProviderV1, COMPOSITION_SCHEMA_VERSION, CompositionBootstrapV1,
    CompositionPolicy, CompositionSpec, LOCK_SCHEMA_VERSION, LockActionMode, LockV1,
    LockedAliasEdgeV1, LockedDependency, LockedGameGraph, LockedPackage, ObservabilityCatalog,
    PACKAGE_MODEL_VERSION, PRODUCT_LOCK_PRODUCER_MACHINE, PackageAlias, PackageDependency,
    PackageMetadata, PackageSourceManifestV1, PackageSpec, ProductLockDraftV1, ProductLockError,
    ProductLockHostReceipts, ProductLockProducerV1, RealizationKind, RealizationSpec,
    RealizedDataRootV1, RegistrationFragment, RegistrationImage, ResolutionStep, RuntimeBinding,
    RuntimeImage, SemanticCatalog, SettingsCatalog, SourceCandidate, SourceSnapshot,
    TargetPackageRealizationV1, TargetRealizationLockV1, TrustClass, persist_product_lock,
    reopen_product_lock,
};
use latticeaxiom_core::{
    CanonicalHash, CanonicalLogicalPath, PackageName, SourceId, SourceProvenance, StableId,
    TargetTriple, canonical_json_hash,
};

use crate::cas::{CasObjectKind, CasObjectStore, FilesystemCas, MemoryCas};
use crate::catalog::{
    AcquiredPackageV1, CheckedPackageV1, LOCAL_CATALOG_CAS_DIRECTORY, LocalCatalogIndexV1,
    acquire_from_directory, acquire_package, check_path_package, pack_package,
    publish_to_directory,
};
use crate::error::TransactionError;
use crate::lock_verify::verify_product_lock_from_cas;
use crate::model::{
    PackageCandidate, PackageSourceKind, ResolutionReceiptV1, ResolvedRealizationV1,
};
use crate::resolver::PackageResolver;

const BOOTSTRAP_SOURCE_ID: &str = "latticeaxiom:source/bootstrap";
const DEFAULT_NICKEL_ENTRY: &str = "default";
const METADATA_LICENSE: &str = "MIT OR Apache-2.0";

/// Inputs for one offline bootstrap-first lock transaction.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OfflineLockRequestV1 {
    /// Directory against which bootstrap Path locators are resolved.
    pub workspace_root: PathBuf,
    /// Bootstrap file. Relative paths are resolved against `workspace_root`.
    pub bootstrap_path: PathBuf,
    /// Directory that receives the local catalog index and CAS objects.
    pub catalog_root: PathBuf,
    /// Destination of the atomically written `latticeaxiom.lock`.
    pub lock_path: PathBuf,
    /// Realization target recorded on the resolution receipt.
    pub target: TargetTriple,
    /// Host toolchain identity sealed into the product lock.
    pub toolchain: CanonicalHash,
}

/// Receipts produced by one offline bootstrap-first lock transaction.
///
/// [`ResolutionReceiptV1`] remains the resolver-stage receipt. [`LockV1`] is
/// the product lock. Composition and registration lock receipts are hashed
/// placeholders required by [`LockV1`]; they are not a Nickel evaluation.
#[derive(Clone, Debug, PartialEq)]
pub struct OfflineLockOutcomeV1 {
    /// Validated bootstrap that authorized acquisition.
    pub bootstrap: CompositionBootstrapV1,
    /// Resolver-stage selection receipt. This is not the product lock.
    pub resolution: ResolutionReceiptV1,
    /// Matching pre-build intent hashed into the product lock.
    pub build_intent_hash: CanonicalHash,
    /// Sealed, persisted, and frozen-verified product lock.
    pub lock: LockV1,
    /// Host receipts required to reopen the lock frozen.
    pub host: ProductLockHostReceipts,
}

/// Runs check/pack/publish/acquire, resolves, persists `latticeaxiom.lock`,
/// and reopens the lock frozen against catalog CAS.
///
/// Nickel composition is not executed. Composition and registration receipts
/// sealed into [`LockV1`] are produced by the in-memory empty registration
/// and runtime-image helpers so the lock schema can close without inventing
/// digest bytes.
///
/// # Errors
///
/// Returns [`TransactionError`] when the bootstrap is invalid, a catalog
/// step fails closed, resolution is unsatisfiable, lock persistence fails,
/// or frozen reopen observes a missing or mismatched CAS object.
pub fn persist_offline_lock(
    request: &OfflineLockRequestV1,
) -> Result<OfflineLockOutcomeV1, TransactionError> {
    let workspace_root = absolute_dir(&request.workspace_root)?;
    let bootstrap_path = resolve_against(&workspace_root, &request.bootstrap_path);
    let catalog_root = absolute_dir(&request.catalog_root)?;
    let lock_path = resolve_against(&workspace_root, &request.lock_path);

    let bootstrap = load_bootstrap(&bootstrap_path)?;
    let packed = check_pack_declared_paths(&workspace_root, &bootstrap)?;
    let mut memory = MemoryCas::new();
    let mut packed_rows = Vec::new();
    for checked in packed {
        packed_rows.push(pack_package(&checked, &mut memory)?);
    }
    let index = publish_to_directory(&catalog_root, &memory, packed_rows)?;
    let acquired = acquire_bootstrap_sources(&workspace_root, &catalog_root, &bootstrap, &index)?;

    let composition = composition_from_bootstrap(&bootstrap, &acquired, &request.target)?;
    let candidates = candidates_from_acquired(&bootstrap, &acquired)?;
    let resolver = PackageResolver::new(candidates)?;
    let resolution = resolver.resolve(&composition)?;

    let mut catalog_store = FilesystemCas::open(catalog_root.join(LOCAL_CATALOG_CAS_DIRECTORY))?;
    let artifacts = publish_data_artifacts(&mut catalog_store, &resolution.receipt, &acquired)?;
    let graph = locked_graph_from_resolution(&resolution.receipt, &acquired, &artifacts)?;
    let alias_edges = alias_edges_from_acquired(&resolution.receipt, &acquired);
    let registration_image = placeholder_registration_image(&graph)?;
    let runtime_image = runtime_image_from_graph(&graph, registration_image.image_hash);
    let evaluation_policy_receipt_hash = canonical_json_hash(&EvaluationPolicyReceipt {
        evaluation_policy: &bootstrap.evaluation_policy,
        evaluation_limits: bootstrap.evaluation_limits,
    })?;
    let registration_semantic_hash = canonical_json_hash(&registration_image.semantics)?;
    let runtime_image_fingerprint = canonical_json_hash(&runtime_image)?;
    let source_objects = source_object_digests(&resolution.receipt, &acquired)?;
    let package_features = resolution
        .receipt
        .packages
        .iter()
        .map(|(name, package)| (name.clone(), package.features.clone()))
        .collect();
    let realizations = BTreeMap::from([(
        request.target.clone(),
        TargetRealizationLockV1 {
            projection: bootstrap.projection,
            target: request.target.clone(),
            toolchain: request.toolchain,
            build_intent_hash: resolution.build_intent.build_intent_hash,
            packages: target_packages(&graph),
            engine_build_id: None,
            registration_image_hash: registration_image.image_hash,
            runtime_image_fingerprint,
        },
    )]);

    let lock = LockV1::seal(ProductLockDraftV1 {
        producer: ProductLockProducerV1 {
            machine: PRODUCT_LOCK_PRODUCER_MACHINE.to_owned(),
            toolchain: request.toolchain,
        },
        bootstrap: bootstrap.clone(),
        graph,
        alias_edges,
        resolution_receipt_hash: resolution.receipt.receipt_hash,
        evaluation_policy_receipt_hash,
        package_features,
        source_objects,
        realizations,
        registration_image,
        registration_semantic_hash,
        runtime_image,
    })?;
    persist_product_lock(&lock_path, &lock, LockActionMode::Offline)?;

    let host = ProductLockHostReceipts {
        toolchain: request.toolchain,
        engine_build_id: None,
        registration_image_hash: lock.registration.image_hash,
        runtime_image_fingerprint,
    };
    let reopened = reopen_offline_lock_frozen(&lock_path, &catalog_root, &host)?;

    Ok(OfflineLockOutcomeV1 {
        bootstrap,
        resolution: resolution.receipt,
        build_intent_hash: resolution.build_intent.build_intent_hash,
        lock: reopened,
        host,
    })
}

/// Reopens `latticeaxiom.lock` and verifies every CAS receipt in frozen mode.
///
/// Acquisition paths are never consulted. Missing or mismatched source,
/// manifest, or artifact objects fail closed.
///
/// # Errors
///
/// Returns [`TransactionError`] when the lock is absent, truncated, or fails
/// frozen verification against the catalog object store.
pub fn reopen_offline_lock_frozen(
    lock_path: impl AsRef<Path>,
    catalog_root: impl AsRef<Path>,
    host: &ProductLockHostReceipts,
) -> Result<LockV1, TransactionError> {
    let lock = reopen_product_lock(lock_path)?;
    let store = FilesystemCas::open(catalog_root.as_ref().join(LOCAL_CATALOG_CAS_DIRECTORY))?;
    verify_product_lock_from_cas(&lock, &store, host, LockActionMode::Frozen)?;
    Ok(lock)
}

#[derive(serde::Serialize)]
struct EvaluationPolicyReceipt<'a> {
    evaluation_policy: &'a StableId,
    evaluation_limits: latticeaxiom_compose::NickelEvaluationLimits,
}

struct AcquiredSourceV1 {
    locator: CanonicalLogicalPath,
    source_kind: PackageSourceKind,
    acquired: AcquiredPackageV1,
}

fn load_bootstrap(path: &Path) -> Result<CompositionBootstrapV1, TransactionError> {
    let text = fs::read_to_string(path).map_err(|source| TransactionError::io(path, source))?;
    Ok(CompositionBootstrapV1::from_toml_str(&text)?)
}

fn check_pack_declared_paths(
    workspace_root: &Path,
    bootstrap: &CompositionBootstrapV1,
) -> Result<Vec<CheckedPackageV1>, TransactionError> {
    let mut checked = Vec::new();
    for source in &bootstrap.sources {
        let Some(path) = source_acquisition_path(source)? else {
            continue;
        };
        let root = join_logical(workspace_root, path.as_str());
        let package = check_path_package(&root)?;
        if package.manifest.name != *source.package() {
            return Err(TransactionError::BootstrapPackageMismatch {
                bootstrap: source.package().clone(),
                packed: package.manifest.name.clone(),
                version: package.manifest.version.clone(),
            });
        }
        checked.push(package);
    }
    Ok(checked)
}

fn acquire_bootstrap_sources(
    workspace_root: &Path,
    catalog_root: &Path,
    bootstrap: &CompositionBootstrapV1,
    index: &LocalCatalogIndexV1,
) -> Result<BTreeMap<PackageName, AcquiredSourceV1>, TransactionError> {
    let catalog_store = FilesystemCas::open(catalog_root.join(LOCAL_CATALOG_CAS_DIRECTORY))?;
    let mut acquired = BTreeMap::new();
    for source in &bootstrap.sources {
        let (package, version, locator, source_kind, from_this_catalog) = match source {
            BootstrapSourceProviderV1::Path { package, path } => (
                package.clone(),
                packed_identity(index, package)?,
                path.clone(),
                PackageSourceKind::LocalDirectory,
                true,
            ),
            BootstrapSourceProviderV1::LocalCatalog {
                package,
                version,
                catalog,
            } => (
                package.clone(),
                version.clone(),
                match catalog {
                    Some(path) => path.clone(),
                    None => catalog_locator()?,
                },
                PackageSourceKind::LocalDirectory,
                catalog.is_none(),
            ),
            BootstrapSourceProviderV1::Fixture {
                package,
                version,
                path: Some(path),
                ..
            } => (
                package.clone(),
                version.clone(),
                path.clone(),
                PackageSourceKind::LocalDirectory,
                true,
            ),
            BootstrapSourceProviderV1::Workspace { package } => {
                return Err(TransactionError::UnsupportedSourceLocator {
                    package: package.clone(),
                    kind: "workspace",
                });
            }
            BootstrapSourceProviderV1::Fixture { package, .. } => {
                return Err(TransactionError::UnsupportedSourceLocator {
                    package: package.clone(),
                    kind: "fixture",
                });
            }
        };

        let acquired_package = if from_this_catalog {
            acquire_package(index, &catalog_store, &package, &version)?
        } else {
            let foreign = join_logical(workspace_root, locator.as_str());
            acquire_from_directory(foreign, &package, &version)?
        };
        acquired.insert(
            package,
            AcquiredSourceV1 {
                locator,
                source_kind,
                acquired: acquired_package,
            },
        );
    }
    Ok(acquired)
}

fn packed_identity(
    index: &LocalCatalogIndexV1,
    package: &PackageName,
) -> Result<latticeaxiom_core::PackageVersion, TransactionError> {
    let mut found = None;
    for entry in index.entries() {
        if &entry.package == package {
            if found.is_some() {
                return Err(TransactionError::Catalog(
                    crate::error::CatalogError::InvalidCatalogIndex {
                        reason: format!("package {package} has multiple published versions"),
                    },
                ));
            }
            found = Some(entry.version.clone());
        }
    }
    found.ok_or_else(|| TransactionError::MissingPublishedPackage {
        package: package.clone(),
    })
}

fn catalog_locator() -> Result<CanonicalLogicalPath, TransactionError> {
    let value = "latticeaxiom-catalog.json";
    CanonicalLogicalPath::new(value).map_err(|source| TransactionError::InvalidPath {
        value: value.to_owned(),
        source,
    })
}

fn source_acquisition_path(
    source: &BootstrapSourceProviderV1,
) -> Result<Option<&CanonicalLogicalPath>, TransactionError> {
    match source {
        BootstrapSourceProviderV1::Path { path, .. }
        | BootstrapSourceProviderV1::Fixture {
            path: Some(path), ..
        } => Ok(Some(path)),
        BootstrapSourceProviderV1::LocalCatalog { .. } => Ok(None),
        BootstrapSourceProviderV1::Workspace { package } => {
            Err(TransactionError::UnsupportedSourceLocator {
                package: package.clone(),
                kind: "workspace",
            })
        }
        BootstrapSourceProviderV1::Fixture { package, .. } => {
            Err(TransactionError::UnsupportedSourceLocator {
                package: package.clone(),
                kind: "fixture",
            })
        }
    }
}

fn composition_from_bootstrap(
    bootstrap: &CompositionBootstrapV1,
    acquired: &BTreeMap<PackageName, AcquiredSourceV1>,
    target: &TargetTriple,
) -> Result<CompositionSpec, TransactionError> {
    let mut sources = Vec::new();
    for source in &bootstrap.sources {
        let acquired = require_acquired(acquired, source.package())?;
        sources.push(source_candidate(source.package(), acquired)?);
    }
    let profile = profile_id(bootstrap)?;
    let provenance = SourceProvenance::new(
        parse_source_id(BOOTSTRAP_SOURCE_ID)?,
        bootstrap.nickel_profile_entry.as_str(),
        bootstrap.canonical_hash()?,
        None,
        Vec::new(),
    )?;
    Ok(CompositionSpec {
        schema_version: COMPOSITION_SCHEMA_VERSION,
        profile,
        profile_kind: bootstrap.projection,
        projection_domains: bootstrap.projection_domains.clone(),
        roots: bootstrap.roots.clone(),
        capabilities: BTreeMap::new(),
        features: bootstrap.features.clone(),
        parameters: bootstrap.parameters.clone(),
        semantic_bindings: BTreeMap::new(),
        overlays: Vec::new(),
        sources,
        policy: CompositionPolicy {
            target: target.clone(),
            realization_order: bootstrap.realization_policy.clone(),
            namespace_grants: BTreeMap::new(),
            maximum_trust: TrustClass::DataOnly,
            allow_force_override: false,
            allow_recovery: false,
            evaluation_policy: bootstrap.evaluation_policy.clone(),
            evaluation_limits: bootstrap.evaluation_limits,
        },
        provenance,
    })
}

fn candidates_from_acquired(
    bootstrap: &CompositionBootstrapV1,
    acquired: &BTreeMap<PackageName, AcquiredSourceV1>,
) -> Result<Vec<PackageCandidate>, TransactionError> {
    let mut candidates = Vec::new();
    for source in &bootstrap.sources {
        let acquired = require_acquired(acquired, source.package())?;
        let source_row = source_candidate(source.package(), acquired)?;
        let package = package_spec_from_manifest(&acquired.acquired.manifest, &source_row)?;
        candidates.push(PackageCandidate {
            source_kind: acquired.source_kind,
            source: source_row,
            package,
        });
    }
    Ok(candidates)
}

fn source_candidate(
    package: &PackageName,
    acquired: &AcquiredSourceV1,
) -> Result<SourceCandidate, TransactionError> {
    let snapshot = &acquired.acquired.snapshot;
    let provenance = package_entry_provenance(&acquired.acquired.manifest, snapshot)?;
    Ok(SourceCandidate {
        source_id: snapshot.source_id().clone(),
        package: package.clone(),
        version: acquired.acquired.manifest.version.clone(),
        path: acquired.locator.as_str().to_owned(),
        content_hash: snapshot.source_hash(),
        priority: 0,
        provenance,
    })
}

fn package_spec_from_manifest(
    manifest: &PackageSourceManifestV1,
    source: &SourceCandidate,
) -> Result<PackageSpec, TransactionError> {
    let fragment_hash = canonical_json_hash(&RegistrationFragment::default())?;
    let mut realizations = BTreeMap::new();
    for (id, realization) in &manifest.realizations {
        realizations.insert(
            id.clone(),
            RealizationSpec {
                id: realization.id.clone(),
                kind: realization.kind,
                domains: realization.domains.clone(),
                targets: realization.targets.clone(),
                interfaces: realization.interfaces.clone(),
                required_features: realization.required_features.clone(),
                artifact: realization.artifact.clone(),
                trust: realization.trust,
                engine_build: realization.engine_build,
                registration_fragment: fragment_hash,
            },
        );
    }
    let mut dependencies = BTreeMap::new();
    for dependency in manifest.dependencies.values() {
        dependencies.insert(
            dependency.package.clone(),
            PackageDependency {
                version: dependency.version.clone(),
                optional: dependency.optional,
                when_features: dependency.when_features.clone(),
                features: dependency.features.clone(),
                domains: dependency.domains.clone(),
            },
        );
    }
    let spec = PackageSpec {
        model_version: PACKAGE_MODEL_VERSION,
        name: manifest.name.clone(),
        version: manifest.version.clone(),
        metadata: PackageMetadata {
            display_name: manifest.name.as_str().to_owned(),
            documentation: None,
            license: METADATA_LICENSE.to_owned(),
        },
        features: manifest.features.clone(),
        dependencies,
        requires: manifest.requires.clone(),
        provides: manifest.provides.clone(),
        realizations,
        domains: manifest.domains.clone(),
        parameters: manifest.parameters.clone(),
        namespace_requests: BTreeSet::new(),
        namespace_delegations: BTreeMap::new(),
        trust: manifest.trust,
        registration: RegistrationFragment::default(),
        provenance: source.provenance.clone(),
    };
    spec.validate()
        .map_err(|source| TransactionError::InvalidPackage {
            package: manifest.name.clone(),
            source: Box::new(source),
        })?;
    Ok(spec)
}

fn package_entry_provenance(
    manifest: &PackageSourceManifestV1,
    snapshot: &latticeaxiom_compose::SourceSnapshot,
) -> Result<SourceProvenance, TransactionError> {
    let entry = manifest
        .nickel_public_entrypoints
        .get(DEFAULT_NICKEL_ENTRY)
        .map_or("package.ncl", CanonicalLogicalPath::as_str);
    let content_hash = snapshot.resolve_path(entry).map_or_else(
        |_| snapshot.source_hash(),
        |file| file.receipt().content_hash(),
    );
    Ok(SourceProvenance::new(
        snapshot.source_id().clone(),
        entry,
        content_hash,
        None,
        Vec::new(),
    )?)
}

fn publish_data_artifacts<S: CasObjectStore>(
    store: &mut S,
    receipt: &ResolutionReceiptV1,
    acquired: &BTreeMap<PackageName, AcquiredSourceV1>,
) -> Result<BTreeMap<PackageName, CanonicalHash>, TransactionError> {
    let mut artifacts = BTreeMap::new();
    for (name, package) in &receipt.packages {
        let acquired = require_acquired(acquired, name)?;
        let bytes = materialize_artifact(name, &package.realization, &acquired.acquired.snapshot)?;
        let id = store.put(CasObjectKind::RealizedArtifact, &bytes)?;
        artifacts.insert(name.clone(), id.digest());
    }
    Ok(artifacts)
}

fn materialize_artifact(
    package: &PackageName,
    realization: &ResolvedRealizationV1,
    snapshot: &SourceSnapshot,
) -> Result<Vec<u8>, TransactionError> {
    let ArtifactIntent::DataRoot { path: root } = &realization.artifact else {
        return Err(TransactionError::UnsupportedArtifactMaterialization {
            package: package.clone(),
            realization: realization.kind,
            artifact: realization.artifact.clone(),
        });
    };
    if realization.kind != RealizationKind::Data {
        return Err(TransactionError::UnsupportedArtifactMaterialization {
            package: package.clone(),
            realization: realization.kind,
            artifact: realization.artifact.clone(),
        });
    }

    let descendant_prefix = format!("{root}/");
    let files = snapshot
        .files()
        .iter()
        .filter(|(logical_path, _)| {
            logical_path.as_str() == root.as_str()
                || logical_path.starts_with(descendant_prefix.as_str())
        })
        .map(|(logical_path, file)| (logical_path.clone(), file.bytes().to_vec()))
        .collect();
    let artifact = RealizedDataRootV1::from_file_bytes(package.clone(), root.clone(), files)
        .map_err(|source| TransactionError::RealizedDataRoot {
            package: package.clone(),
            source,
        })?;
    artifact
        .canonical_bytes()
        .map_err(|source| TransactionError::RealizedDataRoot {
            package: package.clone(),
            source,
        })
}

fn locked_graph_from_resolution(
    receipt: &ResolutionReceiptV1,
    acquired: &BTreeMap<PackageName, AcquiredSourceV1>,
    artifacts: &BTreeMap<PackageName, CanonicalHash>,
) -> Result<LockedGameGraph, TransactionError> {
    let mut packages = BTreeMap::new();
    for (name, package) in &receipt.packages {
        let acquired = require_acquired(acquired, name)?;
        let artifact_hash = artifacts.get(name).copied().ok_or_else(|| {
            TransactionError::MissingPublishedPackage {
                package: name.clone(),
            }
        })?;
        let dependencies = package
            .dependencies
            .iter()
            .map(|(dependency, edge)| {
                (
                    dependency.clone(),
                    LockedDependency {
                        version: edge.version.clone(),
                        features: edge.features.clone(),
                    },
                )
            })
            .collect();
        let interfaces = package
            .realization
            .interfaces
            .iter()
            .map(|(interface, requirement)| (interface.clone(), requirement.version.clone()))
            .collect();
        packages.insert(
            name.clone(),
            LockedPackage {
                name: name.clone(),
                version: package.version.clone(),
                source_id: package.source_id.clone(),
                source_hash: package.source_hash,
                provenance_hash: package.package_provenance_hash,
                realization: package.realization.kind,
                realization_id: package.realization.id.clone(),
                manifest_hash: acquired.acquired.entry.manifest_digest,
                artifact_hash,
                interfaces,
                engine_build_id: package.realization.engine_build_id,
                domains: package.domains.clone(),
                dependencies,
                schemas: package.schemas.clone(),
                source_path: package.source_path.to_string(),
            },
        );
    }

    let capability_providers = receipt
        .capabilities
        .iter()
        .map(|(capability, resolution)| {
            (
                capability.clone(),
                resolution
                    .providers
                    .iter()
                    .map(|provider| provider.package.clone())
                    .collect(),
            )
        })
        .collect();

    let mut graph = LockedGameGraph {
        schema_version: LOCK_SCHEMA_VERSION,
        composition_hash: receipt.composition_hash,
        composition_provenance_hash: receipt.composition_provenance_hash,
        evaluation_policy: receipt.evaluation_policy.clone(),
        evaluation_limits: receipt.evaluation_limits,
        roots: receipt.roots.clone(),
        packages,
        capability_providers,
        namespace_grants: receipt.namespace_grants.clone(),
        explanation: locked_explanation(receipt),
        graph_hash: CanonicalHash::digest(b"unverified-graph"),
        lock_hash: CanonicalHash::digest(b"unverified-lock"),
    };
    graph.graph_hash = graph.recompute_graph_hash()?;
    graph.lock_hash = graph.recompute_lock_hash()?;
    graph
        .verify_hashes()
        .map_err(|source| TransactionError::ProductLock(ProductLockError::Graph { source }))?;
    Ok(graph)
}

fn locked_explanation(receipt: &ResolutionReceiptV1) -> Vec<ResolutionStep> {
    let mut steps = Vec::new();
    for name in &receipt.roots {
        steps.push(ResolutionStep::Root {
            package: name.clone(),
        });
    }
    for (name, package) in &receipt.packages {
        for dependency in package.dependencies.keys() {
            steps.push(ResolutionStep::Dependency {
                required_by: name.clone(),
                package: dependency.clone(),
            });
        }
    }
    for (capability, resolution) in &receipt.capabilities {
        for provider in &resolution.providers {
            steps.push(ResolutionStep::Capability {
                capability: capability.clone(),
                provider: provider.package.clone(),
            });
        }
    }
    steps
}

fn alias_edges_from_acquired(
    receipt: &ResolutionReceiptV1,
    acquired: &BTreeMap<PackageName, AcquiredSourceV1>,
) -> BTreeMap<PackageName, BTreeMap<PackageAlias, LockedAliasEdgeV1>> {
    let mut edges = BTreeMap::new();
    for (importer, package) in &receipt.packages {
        let Some(acquired) = acquired.get(importer) else {
            continue;
        };
        let mut aliases = BTreeMap::new();
        for (alias, dependency) in &acquired.acquired.manifest.dependencies {
            let Some(selected) = receipt.packages.get(&dependency.package) else {
                continue;
            };
            let Some(edge) = package.dependencies.get(&dependency.package) else {
                continue;
            };
            aliases.insert(
                alias.clone(),
                LockedAliasEdgeV1 {
                    package: dependency.package.clone(),
                    version: selected.version.clone(),
                    features: edge.features.clone(),
                },
            );
        }
        if !aliases.is_empty() {
            edges.insert(importer.clone(), aliases);
        }
    }
    edges
}

fn placeholder_registration_image(
    graph: &LockedGameGraph,
) -> Result<RegistrationImage, TransactionError> {
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
    image.verify_image_hash().map_err(|error| {
        TransactionError::ProductLock(ProductLockError::InvalidStructure {
            reason: error.to_string(),
        })
    })?;
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

fn source_object_digests(
    receipt: &ResolutionReceiptV1,
    acquired: &BTreeMap<PackageName, AcquiredSourceV1>,
) -> Result<BTreeMap<PackageName, CanonicalHash>, TransactionError> {
    let mut objects = BTreeMap::new();
    for name in receipt.packages.keys() {
        let acquired = require_acquired(acquired, name)?;
        objects.insert(name.clone(), acquired.acquired.entry.source_object.digest());
    }
    Ok(objects)
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

fn require_acquired<'a>(
    acquired: &'a BTreeMap<PackageName, AcquiredSourceV1>,
    package: &PackageName,
) -> Result<&'a AcquiredSourceV1, TransactionError> {
    acquired
        .get(package)
        .ok_or_else(|| TransactionError::MissingPublishedPackage {
            package: package.clone(),
        })
}

fn profile_id(bootstrap: &CompositionBootstrapV1) -> Result<StableId, TransactionError> {
    let entry = bootstrap.nickel_profile_entry.as_str();
    let file = entry.rsplit('/').next().unwrap_or(entry);
    let stem = file.strip_suffix(".ncl").unwrap_or(file);
    let value = format!("latticeaxiom:profile/{stem}@1");
    parse_stable_id(&value)
}

fn parse_source_id(value: &str) -> Result<SourceId, TransactionError> {
    value
        .parse()
        .map_err(|source| TransactionError::InvalidIdentifier {
            value: value.to_owned(),
            source,
        })
}

fn parse_stable_id(value: &str) -> Result<StableId, TransactionError> {
    value
        .parse()
        .map_err(|source| TransactionError::InvalidIdentifier {
            value: value.to_owned(),
            source,
        })
}

fn absolute_dir(path: &Path) -> Result<PathBuf, TransactionError> {
    fs::create_dir_all(path).map_err(|source| TransactionError::io(path, source))?;
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::path::absolute(path).map_err(|source| TransactionError::io(path, source))?
    };
    Ok(absolute)
}

fn resolve_against(root: &Path, path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        join_host(root, path)
    }
}

fn join_host(root: &Path, path: &Path) -> PathBuf {
    let mut dest = root.to_path_buf();
    for component in path.components() {
        dest.push(component);
    }
    dest
}

fn join_logical(root: &Path, logical_path: &str) -> PathBuf {
    let mut dest = root.to_path_buf();
    for segment in logical_path.split('/') {
        dest.push(segment);
    }
    dest
}
