//! Receipt verification and runtime binding before Bevy host construction.

use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    fmt,
    sync::{Arc, OnceLock},
};

use bevy::prelude::Resource;
use latticeaxiom_compose::{
    GraphHashError, LOCK_SCHEMA_VERSION, LockV1, LockedDependency, LockedGameGraph, LockedPackage,
    ManifestProducer, ObservabilityCatalog, RealizationKind, RealizedDataRootError,
    RealizedDataRootV1, RegistrationImage, ResolutionStep, RuntimeBinding, RuntimeImage,
    SemanticCatalog, SettingsCatalog, TargetRealizationLockV1,
};
use latticeaxiom_core::{
    CanonicalHash, CanonicalJsonError, CapabilityId, PackageName, PackageVersion, SchemaId,
    StableId, TargetTriple, canonical_json_hash,
};
use latticeaxiom_launcher::{ReopenedFinalLockV1, VerifiedArtifactObjects};
use latticeaxiom_registration::{
    CallbackMapReceipt, CompiledRegistration, CompiledSemanticImage, PackageProvenanceReceipt,
    REGISTRATION_COMPILE_RECEIPT_SCHEMA_VERSION, ReceiptValidationError, RegistrationImageReceipt,
    RegistrationProvenanceReceipt, SemanticResolutionReceipt,
};
use thiserror::Error;

/// Exact lock, compiled registration, and runtime image accepted by the host gate.
///
/// Construction verifies the complete compiler receipts before validating the
/// exact graph and runtime callback bindings. `LockedPackage` and `BuildPlan`
/// do not yet freeze `registration_semantic_hash` or `callback_map_hash`, so
/// this boundary does not claim an independent pre-commitment to those hashes.
/// It instead keeps the verified [`CompiledRegistration`] intact and binds it
/// to the exact lock and runtime image supplied for this activation.
/// `RuntimeImage` exposes callback keys but no signature hashes or loaded code,
/// so this gate cannot attest callback ABI or prove Bevy system installation.
#[derive(Clone, Debug, Resource)]
pub struct StructurallyValidatedComposeImages {
    graph: LockedGameGraph,
    registration: CompiledRegistration,
    runtime: RuntimeImage,
}

impl StructurallyValidatedComposeImages {
    /// Verifies and binds the exact lock, compiled registration, and runtime image.
    ///
    /// No package discovery, parsing, loading, or code activation occurs.
    ///
    /// # Errors
    ///
    /// Returns [`PreparationError`] when a receipt, hash, or representable
    /// cross-image invariant is invalid.
    pub fn new(
        graph: LockedGameGraph,
        registration: CompiledRegistration,
        runtime: RuntimeImage,
    ) -> Result<Self, PreparationError> {
        validate_images(&graph, &registration, &runtime)?;
        Ok(Self {
            graph,
            registration,
            runtime,
        })
    }

    /// Returns the exact frozen package graph.
    #[must_use]
    pub const fn graph(&self) -> &LockedGameGraph {
        &self.graph
    }

    /// Returns the verified complete compiled registration.
    #[must_use]
    pub const fn registration(&self) -> &CompiledRegistration {
        &self.registration
    }

    /// Returns the closure-wide registration semantic hash carried by every receipt.
    ///
    /// The current lock and build-plan DTOs do not independently freeze this hash.
    #[must_use]
    pub const fn registration_semantic_hash(&self) -> CanonicalHash {
        self.registration.image_receipt.registration_semantic_hash
    }

    /// Returns the verified callback binding receipt used for runtime validation.
    #[must_use]
    pub const fn callback_receipt(&self) -> &CallbackMapReceipt {
        &self.registration.callback_receipt
    }

    /// Returns the structurally validated runtime binding description.
    ///
    /// This does not prove that callbacks were installed in a Bevy
    /// [`bevy::app::App`].
    #[must_use]
    pub const fn runtime(&self) -> &RuntimeImage {
        &self.runtime
    }

    /// Consumes the scaffold and returns its compose-plane products.
    #[must_use]
    pub fn into_parts(self) -> (LockedGameGraph, CompiledRegistration, RuntimeImage) {
        (self.graph, self.registration, self.runtime)
    }
}

#[derive(Clone, Copy, Debug)]
struct LockedArtifactBinding {
    realization: RealizationKind,
    digest: CanonicalHash,
}

/// Target-selected package artifacts retained from a frozen verified reopen.
///
/// Package bindings come only from one target realization in the final lock.
/// The immutable object handle contains only artifacts whose digests were
/// independently verified during frozen reopen.
#[derive(Clone, Debug)]
pub struct LockedPackageArtifactStore {
    inner: Arc<LockedPackageArtifactStoreInner>,
}

#[derive(Debug)]
struct LockedPackageArtifactStoreInner {
    objects: VerifiedArtifactObjects,
    packages: BTreeMap<PackageName, LockedArtifactBinding>,
    data_roots: BTreeMap<PackageName, OnceLock<Arc<RealizedDataRootV1>>>,
}

impl LockedPackageArtifactStore {
    fn from_reopened(
        lock: &ReopenedFinalLockV1,
        target: &TargetTriple,
    ) -> Result<Self, PreparationError> {
        let Some(realization) = lock.product_lock().realizations.get(target) else {
            return Err(PreparationError::MissingTargetRealization {
                target: target.clone(),
            });
        };
        let objects = lock.verified_artifacts();
        let mut packages = BTreeMap::new();
        for (package, realized) in &realization.packages {
            let Some(bytes) = objects.get(realized.artifact_digest) else {
                return Err(PreparationError::MissingVerifiedArtifact {
                    package: package.clone(),
                    digest: realized.artifact_digest,
                });
            };
            let actual = CanonicalHash::digest(bytes);
            if actual != realized.artifact_digest {
                return Err(PreparationError::VerifiedArtifactDigestMismatch {
                    package: package.clone(),
                    locked: realized.artifact_digest,
                    actual,
                });
            }
            packages.insert(
                package.clone(),
                LockedArtifactBinding {
                    realization: realized.kind,
                    digest: realized.artifact_digest,
                },
            );
        }
        let data_roots = packages
            .iter()
            .filter(|(_, binding)| binding.realization == RealizationKind::Data)
            .map(|(package, _)| (package.clone(), OnceLock::new()))
            .collect();
        Ok(Self {
            inner: Arc::new(LockedPackageArtifactStoreInner {
                objects,
                packages,
                data_roots,
            }),
        })
    }

    /// Decodes one lock-selected data realization using the stable artifact schema.
    ///
    /// # Errors
    ///
    /// Returns [`LockedPackageArtifactError`] when the package is absent, is
    /// not a data realization, or its verified bytes fail the data-root schema.
    pub fn data_root(
        &self,
        package: &PackageName,
    ) -> Result<Arc<RealizedDataRootV1>, LockedPackageArtifactError> {
        let binding = self.inner.packages.get(package).ok_or_else(|| {
            LockedPackageArtifactError::MissingPackage {
                package: package.clone(),
            }
        })?;
        if binding.realization != RealizationKind::Data {
            return Err(LockedPackageArtifactError::NotData {
                package: package.clone(),
                realization: binding.realization,
            });
        }
        let cache = self.inner.data_roots.get(package).ok_or_else(|| {
            LockedPackageArtifactError::NotData {
                package: package.clone(),
                realization: binding.realization,
            }
        })?;
        if let Some(root) = cache.get() {
            return Ok(Arc::clone(root));
        }
        let bytes = self.inner.objects.get(binding.digest).ok_or_else(|| {
            LockedPackageArtifactError::MissingArtifact {
                package: package.clone(),
                digest: binding.digest,
            }
        })?;
        let root = RealizedDataRootV1::from_canonical_bytes_for_package(bytes, package).map_err(
            |source| LockedPackageArtifactError::InvalidDataRoot {
                package: package.clone(),
                source,
            },
        )?;
        let root = Arc::new(root);
        let _ = cache.set(Arc::clone(&root));
        Ok(cache.get().cloned().unwrap_or(root))
    }
}

/// Exact lock, compiled registration, and runtime image bound to a reopened
/// final `latticeaxiom.lock`.
///
/// Construction reopens no files and does not call the package resolver. It
/// constructs [`RuntimeImage`] from the verified target realization and
/// callback receipt, then applies the same structural gate as
/// [`StructurallyValidatedComposeImages`]. Native modules are never mapped.
/// Client and headless hosts must share one value. The production playable
/// spine consumes this type without re-resolving or opening a world writer.
#[derive(Clone, Debug, Resource)]
pub struct LockVerifiedComposeImages {
    images: StructurallyValidatedComposeImages,
    product_lock_hash: CanonicalHash,
    target: TargetTriple,
    artifacts: LockedPackageArtifactStore,
}

impl LockVerifiedComposeImages {
    /// Binds compiled evidence to a reopened final product lock.
    ///
    /// [`RuntimeImage`] is constructed after lock verification from the sealed
    /// realization and callback receipt. This path does not re-resolve
    /// packages, load native modules, or open a world writer.
    ///
    /// # Errors
    ///
    /// Returns [`PreparationError`] when the graph, registration image,
    /// realization target, runtime fingerprint, or native-mapping policy does
    /// not match the reopened lock, or when structural image validation fails.
    pub fn from_reopened_lock(
        lock: &ReopenedFinalLockV1,
        target: &TargetTriple,
        graph: LockedGameGraph,
        registration: CompiledRegistration,
    ) -> Result<Self, PreparationError> {
        let product = lock.product_lock();
        let Some(realization) = product.realizations.get(target) else {
            return Err(PreparationError::MissingTargetRealization {
                target: target.clone(),
            });
        };
        bind_graph_to_lock(
            &graph,
            product.registration.graph_hash,
            product.registration.graph_lock_hash,
        )?;
        if product.portable_resolution.graph_hash != graph.graph_hash {
            return Err(PreparationError::RegistrationGraphMismatch {
                locked: product.portable_resolution.graph_hash,
                registration: graph.graph_hash,
            });
        }
        if registration.image.image_hash != product.registration.image_hash {
            return Err(PreparationError::RegistrationImageDoesNotMatchProductLock {
                locked: product.registration.image_hash,
                registration: registration.image.image_hash,
            });
        }
        refuse_native_module_mapping(realization)?;
        let runtime = runtime_image_from_realization(realization, &registration);
        let actual = canonical_json_hash(&runtime)?;
        if actual != realization.runtime_image_fingerprint {
            return Err(PreparationError::RuntimeImageFingerprintMismatch {
                locked: realization.runtime_image_fingerprint,
                actual,
            });
        }
        let images = StructurallyValidatedComposeImages::new(graph, registration, runtime)?;
        let artifacts = LockedPackageArtifactStore::from_reopened(lock, target)?;
        Ok(Self {
            images,
            product_lock_hash: lock.product_lock_hash(),
            target: target.clone(),
            artifacts,
        })
    }

    /// Returns the structurally validated compose images.
    #[must_use]
    pub const fn images(&self) -> &StructurallyValidatedComposeImages {
        &self.images
    }

    /// Returns the exact product-lock hash of the shared reopened lock.
    #[must_use]
    pub const fn product_lock_hash(&self) -> CanonicalHash {
        self.product_lock_hash
    }

    /// Returns the target realization selected from the reopened lock.
    #[must_use]
    pub const fn target(&self) -> &TargetTriple {
        &self.target
    }

    /// Returns target-selected artifacts retained from the frozen reopen.
    #[must_use]
    pub const fn locked_artifacts(&self) -> &LockedPackageArtifactStore {
        &self.artifacts
    }

    /// Consumes the lock-verified images.
    #[must_use]
    pub fn into_images(self) -> StructurallyValidatedComposeImages {
        self.images
    }

    /// Reconstructs compose images from a reopened final product lock.
    ///
    /// The locked graph is rebuilt from portable resolution and the selected
    /// target realization. Placeholder compiler receipts are bound when the
    /// lock sealed an empty registration image. Native modules are not mapped
    /// and no world writer is opened.
    ///
    /// # Errors
    ///
    /// Returns [`PreparationError`] when the selected target is absent, a
    /// realization package is missing, reconstructed hashes do not match the
    /// lock, or structural image validation fails.
    pub fn from_reopened_product_lock(
        lock: &ReopenedFinalLockV1,
        target: &TargetTriple,
    ) -> Result<Self, PreparationError> {
        let graph = reconstruct_locked_graph(lock.product_lock(), target)?;
        let registration = placeholder_compiled_registration(&graph)?;
        Self::from_reopened_lock(lock, target, graph, registration)
    }
}

fn reconstruct_locked_graph(
    lock: &LockV1,
    target: &TargetTriple,
) -> Result<LockedGameGraph, PreparationError> {
    let Some(realization) = lock.realizations.get(target) else {
        return Err(PreparationError::MissingTargetRealization {
            target: target.clone(),
        });
    };
    let mut packages = BTreeMap::new();
    for (name, portable) in &lock.portable_resolution.packages {
        let Some(realized) = realization.packages.get(name) else {
            return Err(PreparationError::MissingRealizationPackage {
                package: name.clone(),
            });
        };
        let mut dependencies = BTreeMap::new();
        if let Some(aliases) = lock.portable_resolution.alias_edges.get(name) {
            for edge in aliases.values() {
                dependencies.insert(
                    edge.package.clone(),
                    LockedDependency {
                        version: edge.version.clone(),
                        features: edge.features.clone(),
                    },
                );
            }
        }
        packages.insert(
            name.clone(),
            LockedPackage {
                name: name.clone(),
                version: portable.version.clone(),
                source_id: portable.source_id.clone(),
                source_hash: portable.source_digest,
                provenance_hash: portable.provenance_hash,
                realization: realized.kind,
                realization_id: realized.realization_id.clone(),
                manifest_hash: portable.manifest_digest,
                artifact_hash: realized.artifact_digest,
                interfaces: BTreeMap::new(),
                engine_build_id: realized.engine_build_id,
                domains: portable.domains.clone(),
                dependencies,
                schemas: BTreeSet::new(),
                source_path: String::new(),
            },
        );
    }

    let mut explanation = Vec::new();
    for package in &lock.portable_resolution.roots {
        explanation.push(ResolutionStep::Root {
            package: package.clone(),
        });
    }
    for name in lock.portable_resolution.packages.keys() {
        if let Some(aliases) = lock.portable_resolution.alias_edges.get(name) {
            for edge in aliases.values() {
                explanation.push(ResolutionStep::Dependency {
                    required_by: name.clone(),
                    package: edge.package.clone(),
                });
            }
        }
    }
    // Capability explanation is package-major, capability-minor so lock_hash
    // matches the sealing path in `latticeaxiom-compose` `selected_lock_graph`.
    for name in lock.portable_resolution.packages.keys() {
        for (capability, providers) in &lock.portable_resolution.capabilities {
            if providers.iter().any(|provider| provider == name) {
                explanation.push(ResolutionStep::Capability {
                    capability: capability.clone(),
                    provider: name.clone(),
                });
            }
        }
    }

    let mut graph = LockedGameGraph {
        schema_version: LOCK_SCHEMA_VERSION,
        composition_hash: lock.composition.composition_hash,
        composition_provenance_hash: lock.composition.composition_provenance_hash,
        evaluation_policy: lock.portable_resolution.evaluation_policy.clone(),
        evaluation_limits: lock.portable_resolution.evaluation_limits,
        roots: lock.portable_resolution.roots.clone(),
        packages,
        capability_providers: lock.portable_resolution.capabilities.clone(),
        namespace_grants: BTreeSet::new(),
        explanation,
        graph_hash: CanonicalHash::digest(b"unverified-graph"),
        lock_hash: CanonicalHash::digest(b"unverified-lock"),
    };
    graph.graph_hash = graph.recompute_graph_hash()?;
    graph.lock_hash = graph.recompute_lock_hash()?;
    graph.verify_hashes().map_err(PreparationError::GraphHash)?;
    Ok(graph)
}

#[allow(clippy::too_many_lines)] // Receipt construction fills every compiler-owned table.
fn placeholder_compiled_registration(
    graph: &LockedGameGraph,
) -> Result<CompiledRegistration, PreparationError> {
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
    let registration_semantic_hash = canonical_json_hash(&image.semantics)?;

    let mut semantic_image = CompiledSemanticImage {
        tags: BTreeMap::new(),
        maps: BTreeMap::new(),
        state_properties: BTreeMap::new(),
        affordances: BTreeMap::new(),
        roles: BTreeMap::new(),
        role_bindings: BTreeMap::new(),
        semantic_hash: CanonicalHash::digest(b"unsealed-semantic-image"),
    };
    semantic_image.semantic_hash = semantic_image.recompute_hash()?;

    let mut semantic_receipt = SemanticResolutionReceipt {
        schema_version: REGISTRATION_COMPILE_RECEIPT_SCHEMA_VERSION,
        graph_hash: graph.graph_hash,
        registration_semantic_hash,
        semantic_image_hash: semantic_image.semantic_hash,
        active_bundles: BTreeSet::new(),
        role_bindings: BTreeMap::new(),
        explanation: Vec::new(),
        receipt_hash: CanonicalHash::digest(b"unsealed-semantic-receipt"),
    };
    semantic_receipt.receipt_hash = semantic_receipt.recompute_hash()?;

    let mut image_receipt = RegistrationImageReceipt {
        schema_version: REGISTRATION_COMPILE_RECEIPT_SCHEMA_VERSION,
        graph_hash: graph.graph_hash,
        registration_semantic_hash,
        image_hash: image.image_hash,
        numeric_ids: BTreeMap::new(),
        schedule: Vec::new(),
        receipt_hash: CanonicalHash::digest(b"unsealed-image-receipt"),
    };
    image_receipt.receipt_hash = image_receipt.recompute_hash()?;

    let mut callback_receipt = CallbackMapReceipt {
        schema_version: REGISTRATION_COMPILE_RECEIPT_SCHEMA_VERSION,
        graph_hash: graph.graph_hash,
        registration_semantic_hash,
        callbacks: BTreeMap::new(),
        systems: BTreeMap::new(),
        callback_map_hash: CanonicalHash::digest(b"unsealed-callback-receipt"),
    };
    callback_receipt.callback_map_hash = callback_receipt.recompute_hash()?;

    let producer_version = "0.0.0"
        .parse()
        .unwrap_or_else(|error| panic!("0.0.0 is a valid package version: {error}"));
    let producer = ManifestProducer {
        tool: "latticeaxiom".to_owned(),
        version: producer_version,
        input_hash: CanonicalHash::digest(b"placeholder-registration"),
    };
    let mut packages = BTreeMap::new();
    for (name, locked) in &graph.packages {
        packages.insert(
            name.clone(),
            PackageProvenanceReceipt {
                manifest_semantic_hash: locked.manifest_hash,
                manifest_provenance_hash: locked.provenance_hash,
                producer: producer.clone(),
            },
        );
    }
    let mut provenance_receipt = RegistrationProvenanceReceipt {
        schema_version: REGISTRATION_COMPILE_RECEIPT_SCHEMA_VERSION,
        graph_lock_hash: graph.lock_hash,
        composition_provenance_hash: graph.composition_provenance_hash,
        registration_semantic_hash,
        packages,
        provenance_hash: CanonicalHash::digest(b"unsealed-provenance-receipt"),
    };
    provenance_receipt.provenance_hash = provenance_receipt.recompute_hash()?;

    let compiled = CompiledRegistration {
        image,
        semantic_image,
        semantic_receipt,
        image_receipt,
        callback_receipt,
        provenance_receipt,
    };
    compiled
        .verify()
        .map_err(PreparationError::CompiledRegistration)?;
    Ok(compiled)
}

fn bind_graph_to_lock(
    graph: &LockedGameGraph,
    locked_graph_hash: CanonicalHash,
    locked_graph_lock_hash: CanonicalHash,
) -> Result<(), PreparationError> {
    if graph.graph_hash != locked_graph_hash {
        return Err(PreparationError::RegistrationGraphMismatch {
            locked: locked_graph_hash,
            registration: graph.graph_hash,
        });
    }
    if graph.lock_hash != locked_graph_lock_hash {
        return Err(PreparationError::RegistrationLockMismatch {
            locked: locked_graph_lock_hash,
            registration: graph.lock_hash,
        });
    }
    Ok(())
}

fn refuse_native_module_mapping(
    realization: &TargetRealizationLockV1,
) -> Result<(), PreparationError> {
    for (package, realized) in &realization.packages {
        if matches!(
            realized.kind,
            RealizationKind::PortableNative | RealizationKind::EngineCoupledNative
        ) {
            return Err(PreparationError::NativeModuleMappingForbidden {
                package: package.clone(),
                kind: realized.kind,
            });
        }
    }
    Ok(())
}

fn runtime_image_from_realization(
    realization: &TargetRealizationLockV1,
    registration: &CompiledRegistration,
) -> RuntimeImage {
    let mut packages = BTreeMap::new();
    for (name, package) in &realization.packages {
        let mut callbacks = BTreeSet::new();
        for (callback, binding) in &registration.callback_receipt.callbacks {
            if binding.owner == *name {
                callbacks.insert(callback.clone());
            }
        }
        packages.insert(
            name.clone(),
            RuntimeBinding {
                realization: package.kind,
                artifact_hash: package.artifact_digest,
                callbacks,
            },
        );
    }
    RuntimeImage {
        registration_hash: realization.registration_image_hash,
        packages,
    }
}

fn validate_images(
    graph: &LockedGameGraph,
    registration: &CompiledRegistration,
    runtime: &RuntimeImage,
) -> Result<(), PreparationError> {
    if graph.schema_version != LOCK_SCHEMA_VERSION {
        return Err(PreparationError::UnsupportedLockSchema {
            expected: LOCK_SCHEMA_VERSION,
            actual: graph.schema_version,
        });
    }
    graph.verify_hashes().map_err(PreparationError::GraphHash)?;
    registration
        .verify()
        .map_err(PreparationError::CompiledRegistration)?;

    if registration.image.graph_hash != graph.graph_hash {
        return Err(PreparationError::RegistrationGraphMismatch {
            locked: graph.graph_hash,
            registration: registration.image.graph_hash,
        });
    }
    registration
        .verify_locked_manifests(graph)
        .map_err(PreparationError::CompiledRegistration)?;
    if registration.provenance_receipt.graph_lock_hash != graph.lock_hash {
        return Err(PreparationError::RegistrationLockMismatch {
            locked: graph.lock_hash,
            registration: registration.provenance_receipt.graph_lock_hash,
        });
    }
    if registration.provenance_receipt.composition_provenance_hash
        != graph.composition_provenance_hash
    {
        return Err(
            PreparationError::RegistrationCompositionProvenanceMismatch {
                locked: graph.composition_provenance_hash,
                registration: registration.provenance_receipt.composition_provenance_hash,
            },
        );
    }
    if registration
        .provenance_receipt
        .packages
        .keys()
        .ne(graph.packages.keys())
    {
        return Err(PreparationError::RegistrationPackageSetMismatch);
    }
    if runtime.registration_hash != registration.image.image_hash {
        return Err(PreparationError::RuntimeRegistrationMismatch {
            registration: registration.image.image_hash,
            runtime: runtime.registration_hash,
        });
    }

    validate_graph_packages(graph)?;
    validate_registration_tables(graph, registration)?;
    validate_catalogs(graph, registration)?;
    validate_runtime_packages(graph, runtime)?;
    validate_callbacks(registration, runtime)
}

fn validate_graph_packages(graph: &LockedGameGraph) -> Result<(), PreparationError> {
    let mut closure_seeds = graph.roots.clone();

    for root in &graph.roots {
        if !graph.packages.contains_key(root) {
            return Err(PreparationError::UnknownRoot {
                package: root.clone(),
            });
        }
    }

    for (key, package) in &graph.packages {
        if key != &package.name {
            return Err(PreparationError::GraphPackageKeyMismatch {
                key: key.clone(),
                package: package.name.clone(),
            });
        }
        for (dependency, edge) in &package.dependencies {
            let Some(selected) = graph.packages.get(dependency) else {
                return Err(PreparationError::UnknownDependency {
                    package: package.name.clone(),
                    dependency: dependency.clone(),
                });
            };
            if edge.version != selected.version {
                return Err(PreparationError::DependencyVersionMismatch {
                    package: package.name.clone(),
                    dependency: dependency.clone(),
                    edge: Box::new(edge.version.clone()),
                    selected: Box::new(selected.version.clone()),
                });
            }
        }
    }

    for (capability, providers) in &graph.capability_providers {
        let mut unique = BTreeSet::new();
        for provider in providers {
            if !graph.packages.contains_key(provider) {
                return Err(PreparationError::UnknownCapabilityProvider {
                    capability: capability.clone(),
                    package: provider.clone(),
                });
            }
            if !unique.insert(provider) {
                return Err(PreparationError::DuplicateCapabilityProvider {
                    capability: capability.clone(),
                    package: provider.clone(),
                });
            }
            closure_seeds.insert(provider.clone());
        }
    }

    let mut reachable = BTreeSet::new();
    let mut pending = closure_seeds.into_iter().collect::<VecDeque<_>>();
    while let Some(package_name) = pending.pop_front() {
        if !reachable.insert(package_name.clone()) {
            continue;
        }
        let Some(package) = graph.packages.get(&package_name) else {
            // Seed and dependency existence was validated before traversal.
            continue;
        };
        pending.extend(package.dependencies.keys().cloned());
    }

    if let Some(package) = graph
        .packages
        .keys()
        .find(|package| !reachable.contains(*package))
    {
        return Err(PreparationError::UnreferencedGraphPackage {
            package: package.clone(),
        });
    }
    Ok(())
}

fn validate_registration_tables(
    graph: &LockedGameGraph,
    registration: &CompiledRegistration,
) -> Result<(), PreparationError> {
    let image = &registration.image;
    for (registration_id, owner) in &image.owners {
        if !graph.packages.contains_key(owner) {
            return Err(PreparationError::UnknownRegistrationOwner {
                registration: registration_id.clone(),
                package: owner.clone(),
            });
        }
    }

    validate_schema_owners(graph, registration)?;

    for registration_id in &image.authoritative {
        if !image.numeric_ids.contains_key(registration_id) {
            return Err(PreparationError::UnknownAuthoritativeRegistration {
                registration: registration_id.clone(),
            });
        }
    }

    let mut scheduled = BTreeSet::new();
    for system in &image.schedule {
        if !image.numeric_ids.contains_key(system) || !image.owners.contains_key(system) {
            return Err(PreparationError::UnknownScheduledSystem {
                system: system.clone(),
            });
        }
        if system.kind() != "system" {
            return Err(PreparationError::InvalidScheduledSystemKind {
                system: system.clone(),
            });
        }
        if !scheduled.insert(system) {
            return Err(PreparationError::DuplicateScheduledSystem {
                system: system.clone(),
            });
        }
    }
    Ok(())
}

fn validate_schema_owners(
    graph: &LockedGameGraph,
    registration: &CompiledRegistration,
) -> Result<(), PreparationError> {
    let image = &registration.image;
    let mut locked = BTreeMap::new();
    for package in graph.packages.values() {
        for schema in &package.schemas {
            if let Some(first) = locked.insert(schema.clone(), package.name.clone()) {
                return Err(PreparationError::DuplicateLockedSchemaOwner {
                    schema: schema.clone(),
                    first,
                    second: package.name.clone(),
                });
            }
        }
    }

    for schema in locked.keys().chain(image.schema_owners.keys()) {
        let locked_owner = locked.get(schema);
        let registered_owner = image.schema_owners.get(schema);
        if locked_owner != registered_owner {
            return Err(PreparationError::SchemaOwnershipMismatch {
                schema: schema.clone(),
                locked: locked_owner.cloned(),
                registration: registered_owner.cloned(),
            });
        }
    }
    Ok(())
}

fn validate_catalogs(
    graph: &LockedGameGraph,
    registration: &CompiledRegistration,
) -> Result<(), PreparationError> {
    let image = &registration.image;
    for (key, item) in &image.settings.runtime {
        validate_catalog_entry(
            graph,
            registration,
            CatalogKind::RuntimeSetting,
            key,
            &item.id,
            &item.declared_by,
        )?;
    }
    for (key, item) in &image.settings.composition {
        validate_catalog_entry(
            graph,
            registration,
            CatalogKind::CompositionParameter,
            key,
            &item.id,
            &item.declared_by,
        )?;
    }
    for (key, item) in &image.observability.info_items {
        validate_catalog_entry(
            graph,
            registration,
            CatalogKind::InformationItem,
            key,
            &item.id,
            &item.declared_by,
        )?;
        if !image.schema_owners.contains_key(&item.value_schema) {
            return Err(PreparationError::UnknownSchemaReference {
                registration: item.id.clone(),
                schema: Box::new(item.value_schema.clone()),
                catalog: CatalogKind::InformationItem,
            });
        }
    }
    for (key, item) in &image.observability.metrics {
        validate_catalog_entry(
            graph,
            registration,
            CatalogKind::DiagnosticMetric,
            key,
            &item.id,
            &item.declared_by,
        )?;
    }
    for (key, item) in &image.observability.inspect {
        validate_catalog_entry(
            graph,
            registration,
            CatalogKind::InspectProvider,
            key,
            &item.id,
            &item.declared_by,
        )?;
    }
    for (key, item) in &image.observability.visualizers {
        validate_catalog_entry(
            graph,
            registration,
            CatalogKind::DebugVisualizer,
            key,
            &item.id,
            &item.declared_by,
        )?;
    }
    Ok(())
}

fn validate_catalog_entry(
    graph: &LockedGameGraph,
    registration: &CompiledRegistration,
    catalog: CatalogKind,
    key: &StableId,
    declared_id: &StableId,
    declared_by: &PackageName,
) -> Result<(), PreparationError> {
    let image = &registration.image;
    if key != declared_id {
        return Err(PreparationError::CatalogKeyMismatch {
            catalog,
            key: key.clone(),
            declared_id: Box::new(declared_id.clone()),
        });
    }
    if !graph.packages.contains_key(declared_by) {
        return Err(PreparationError::UnknownCatalogOwner {
            catalog,
            registration: declared_id.clone(),
            package: declared_by.clone(),
        });
    }
    if !image.numeric_ids.contains_key(declared_id) {
        return Err(PreparationError::UnknownCatalogRegistration {
            catalog,
            registration: declared_id.clone(),
        });
    }
    let Some(registered_owner) = image.owners.get(declared_id) else {
        return Err(PreparationError::UnknownCatalogRegistration {
            catalog,
            registration: declared_id.clone(),
        });
    };
    if registered_owner != declared_by {
        return Err(PreparationError::CatalogOwnerMismatch {
            catalog,
            registration: declared_id.clone(),
            declared_by: declared_by.clone(),
            registered_owner: registered_owner.clone(),
        });
    }
    Ok(())
}

fn validate_runtime_packages(
    graph: &LockedGameGraph,
    runtime: &RuntimeImage,
) -> Result<(), PreparationError> {
    for (package_name, package) in &graph.packages {
        let Some(binding) = runtime.packages.get(package_name) else {
            return Err(PreparationError::MissingRuntimePackage {
                package: package_name.clone(),
            });
        };
        if binding.realization != package.realization {
            return Err(PreparationError::RuntimeRealizationMismatch {
                package: package_name.clone(),
                locked: package.realization,
                runtime: binding.realization,
            });
        }
        if binding.artifact_hash != package.artifact_hash {
            return Err(PreparationError::RuntimeArtifactMismatch {
                package: package_name.clone(),
                locked: package.artifact_hash,
                runtime: binding.artifact_hash,
            });
        }
    }
    for package in runtime.packages.keys() {
        if !graph.packages.contains_key(package) {
            return Err(PreparationError::UnexpectedRuntimePackage {
                package: package.clone(),
            });
        }
    }
    Ok(())
}

fn validate_callbacks(
    registration: &CompiledRegistration,
    runtime: &RuntimeImage,
) -> Result<(), PreparationError> {
    // `CompiledRegistration::verify` has already proven that every binding has
    // nonempty, unique consumers and that scheduled system consumers map to
    // their owner. Runtime matching is therefore keyed by callback ID, never by
    // the consumer's system ID.
    for (callback, binding) in &registration.callback_receipt.callbacks {
        let expected_runtime = runtime.packages.get(&binding.owner).ok_or_else(|| {
            PreparationError::MissingRuntimePackage {
                package: binding.owner.clone(),
            }
        })?;
        if !expected_runtime.callbacks.contains(callback) {
            if let Some(actual_owner) = runtime.packages.iter().find_map(|(package, runtime)| {
                runtime.callbacks.contains(callback).then_some(package)
            }) {
                return Err(PreparationError::RuntimeCallbackOwnerMismatch {
                    callback: callback.clone(),
                    expected: binding.owner.clone(),
                    actual: actual_owner.clone(),
                });
            }
            return Err(PreparationError::MissingRuntimeCallback {
                package: binding.owner.clone(),
                callback: callback.clone(),
            });
        }
    }

    for (package, runtime_binding) in &runtime.packages {
        for callback in &runtime_binding.callbacks {
            let Some(compiled_binding) = registration.callback_receipt.callbacks.get(callback)
            else {
                return Err(PreparationError::UnexpectedRuntimeCallback {
                    package: package.clone(),
                    callback: callback.clone(),
                });
            };
            if compiled_binding.owner != *package {
                return Err(PreparationError::RuntimeCallbackOwnerMismatch {
                    callback: callback.clone(),
                    expected: compiled_binding.owner.clone(),
                    actual: package.clone(),
                });
            }
        }
    }
    Ok(())
}

/// Registration surface that requires an executable callback.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CallbackContext {
    /// A fixed or variable schedule entry.
    ScheduledSystem,
    /// An information item producer.
    InformationItem,
    /// A target-inspection fragment provider.
    InspectProvider,
}

impl fmt::Display for CallbackContext {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::ScheduledSystem => "scheduled system",
            Self::InformationItem => "information item",
            Self::InspectProvider => "inspect provider",
        })
    }
}

/// Catalog surface carried by a registration image.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CatalogKind {
    /// Compiled schedule.
    Schedule,
    /// Runtime setting catalog.
    RuntimeSetting,
    /// Composition parameter catalog.
    CompositionParameter,
    /// Information item catalog.
    InformationItem,
    /// Diagnostic metric catalog.
    DiagnosticMetric,
    /// Inspect provider catalog.
    InspectProvider,
    /// Debug visualizer catalog.
    DebugVisualizer,
}

impl fmt::Display for CatalogKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Schedule => "schedule",
            Self::RuntimeSetting => "runtime setting",
            Self::CompositionParameter => "composition parameter",
            Self::InformationItem => "information item",
            Self::DiagnosticMetric => "diagnostic metric",
            Self::InspectProvider => "inspect provider",
            Self::DebugVisualizer => "debug visualizer",
        })
    }
}

/// Failure to select or decode a package artifact retained from frozen reopen.
#[derive(Debug, Error)]
pub enum LockedPackageArtifactError {
    /// The selected target realization does not contain the requested package.
    #[error("selected target realization does not contain package `{package}`")]
    MissingPackage {
        /// Requested package.
        package: PackageName,
    },
    /// The package realization is not a data artifact.
    #[error("package `{package}` uses {realization:?}, not a data realization")]
    NotData {
        /// Requested package.
        package: PackageName,
        /// Selected realization family.
        realization: RealizationKind,
    },
    /// The retained verified object set does not contain the selected digest.
    #[error("verified artifact {digest} for package `{package}` is unavailable")]
    MissingArtifact {
        /// Requested package.
        package: PackageName,
        /// Lock-selected digest.
        digest: CanonicalHash,
    },
    /// Artifact bytes fail the stable realized data-root contract.
    #[error("invalid realized data root for package `{package}`: {source}")]
    InvalidDataRoot {
        /// Requested package.
        package: PackageName,
        /// Closed wire-contract failure.
        #[source]
        source: RealizedDataRootError,
    },
}

/// Failure to verify and bind host preparation inputs.
#[derive(Debug, Error)]
pub enum PreparationError {
    /// Canonical encoding of a runtime image failed.
    #[error(transparent)]
    Canonical(#[from] CanonicalJsonError),
    /// The lock schema is not understood by this host.
    #[error("unsupported lock schema {actual}; this host accepts schema {expected}")]
    UnsupportedLockSchema {
        /// Schema supported by the host.
        expected: u32,
        /// Schema carried by the lock.
        actual: u32,
    },
    /// The reopened product lock has no realization for the requested target.
    #[error("reopened product lock has no realization for target {target}")]
    MissingTargetRealization {
        /// Requested build target.
        target: TargetTriple,
    },
    /// The selected target realization is missing a portable-resolution package.
    #[error("reopened product lock realization is missing package {package}")]
    MissingRealizationPackage {
        /// Package present in portable resolution but absent from the target.
        package: PackageName,
    },
    /// Frozen reopen did not retain the artifact selected by the target.
    #[error("verified artifact {digest} for package `{package}` is unavailable")]
    MissingVerifiedArtifact {
        /// Selected package.
        package: PackageName,
        /// Digest selected by the target realization.
        digest: CanonicalHash,
    },
    /// Retained bytes do not match the selected artifact digest.
    #[error("artifact for package `{package}` hashes to {actual}, lock requires {locked}")]
    VerifiedArtifactDigestMismatch {
        /// Selected package.
        package: PackageName,
        /// Digest selected by the target realization.
        locked: CanonicalHash,
        /// Digest recomputed by engine preparation.
        actual: CanonicalHash,
    },
    /// The compiled registration image hash does not match the reopened lock.
    #[error("registration image hash {registration} does not match reopened product lock {locked}")]
    RegistrationImageDoesNotMatchProductLock {
        /// Registration image hash sealed by the product lock.
        locked: CanonicalHash,
        /// Registration image hash supplied with compiled evidence.
        registration: CanonicalHash,
    },
    /// Constructed runtime-image fingerprint does not match the reopened lock.
    #[error("runtime image fingerprint {actual} does not match reopened product lock {locked}")]
    RuntimeImageFingerprintMismatch {
        /// Runtime-image fingerprint sealed by the product lock.
        locked: CanonicalHash,
        /// Fingerprint of the runtime image constructed after reopen.
        actual: CanonicalHash,
    },
    /// A native module would have to be mapped into the process.
    #[error(
        "native module for '{package}' ({kind:?}) is verified by digest only and is not loaded"
    )]
    NativeModuleMappingForbidden {
        /// Package whose native artifact must not be mapped.
        package: PackageName,
        /// Native realization kind recorded by the lock.
        kind: RealizationKind,
    },
    /// A claimed graph or lock hash is invalid.
    #[error("locked game graph hash validation failed: {0}")]
    GraphHash(#[source] GraphHashError),
    /// The compiled registration or one of its receipts is invalid.
    #[error("compiled registration validation failed: {0}")]
    CompiledRegistration(#[source] ReceiptValidationError),
    /// The registration image was built from a different graph hash.
    #[error("registration graph hash {registration} does not match locked graph hash {locked}")]
    RegistrationGraphMismatch {
        /// Graph hash frozen by the lock.
        locked: CanonicalHash,
        /// Graph hash claimed by registration.
        registration: CanonicalHash,
    },
    /// The registration provenance receipt was produced from another exact lock.
    #[error("registration lock hash {registration} does not match locked hash {locked}")]
    RegistrationLockMismatch {
        /// Exact lock hash supplied to the host.
        locked: CanonicalHash,
        /// Lock hash carried by the registration provenance receipt.
        registration: CanonicalHash,
    },
    /// Registration and lock carry different composition provenance.
    #[error(
        "registration composition provenance {registration} does not match locked provenance {locked}"
    )]
    RegistrationCompositionProvenanceMismatch {
        /// Composition provenance frozen by the lock.
        locked: CanonicalHash,
        /// Composition provenance carried by the registration receipt.
        registration: CanonicalHash,
    },
    /// Registration provenance does not cover exactly the locked package set.
    #[error("registration provenance package set does not match the locked package set")]
    RegistrationPackageSetMismatch,
    /// The runtime image was built from a different registration hash.
    #[error("runtime registration hash {runtime} does not match image hash {registration}")]
    RuntimeRegistrationMismatch {
        /// Canonical registration image hash.
        registration: CanonicalHash,
        /// Registration hash claimed by runtime.
        runtime: CanonicalHash,
    },
    /// A graph map key and enclosed package identity disagree.
    #[error("locked graph package key '{key}' contains package '{package}'")]
    GraphPackageKeyMismatch {
        /// Package map key.
        key: PackageName,
        /// Enclosed identity.
        package: PackageName,
    },
    /// A root is absent from the locked package table.
    #[error("root package '{package}' is absent from the locked package table")]
    UnknownRoot {
        /// Missing root.
        package: PackageName,
    },
    /// A dependency target is absent from the package table.
    #[error("package '{package}' depends on missing locked package '{dependency}'")]
    UnknownDependency {
        /// Depending package.
        package: PackageName,
        /// Missing dependency.
        dependency: PackageName,
    },
    /// A dependency edge version differs from its selected node.
    #[error(
        "package '{package}' locks dependency '{dependency}' at {edge}, but selected node is {selected}"
    )]
    DependencyVersionMismatch {
        /// Depending package.
        package: PackageName,
        /// Dependency package.
        dependency: PackageName,
        /// Edge version.
        edge: Box<PackageVersion>,
        /// Selected node version.
        selected: Box<PackageVersion>,
    },
    /// A capability provider is absent from the package table.
    #[error("capability '{capability}' lists missing provider '{package}'")]
    UnknownCapabilityProvider {
        /// Capability identifier.
        capability: CapabilityId,
        /// Missing provider.
        package: PackageName,
    },
    /// A capability provider appears twice.
    #[error("capability '{capability}' lists provider '{package}' more than once")]
    DuplicateCapabilityProvider {
        /// Capability identifier.
        capability: CapabilityId,
        /// Duplicate provider.
        package: PackageName,
    },
    /// A package is outside the dependency closure of roots and active providers.
    #[error("locked package '{package}' is outside the root/provider dependency closure")]
    UnreferencedGraphPackage {
        /// Unreferenced package.
        package: PackageName,
    },
    /// A registration owner is absent from the graph.
    #[error("registration '{registration}' is owned by unlocked package '{package}'")]
    UnknownRegistrationOwner {
        /// Stable registration.
        registration: StableId,
        /// Missing owner.
        package: PackageName,
    },
    /// Two locked packages claim one persistent schema.
    #[error("schema '{schema}' is claimed by both '{first}' and '{second}'")]
    DuplicateLockedSchemaOwner {
        /// Duplicate schema.
        schema: SchemaId,
        /// First owner.
        first: PackageName,
        /// Second owner.
        second: PackageName,
    },
    /// Locked and registration schema owner tables disagree.
    #[error("schema '{schema}' owner mismatch: locked={locked:?}, registration={registration:?}")]
    SchemaOwnershipMismatch {
        /// Persistent schema.
        schema: SchemaId,
        /// Optional locked owner.
        locked: Option<PackageName>,
        /// Optional registration owner.
        registration: Option<PackageName>,
    },
    /// An authoritative ID has no numeric registration.
    #[error("authoritative ID '{registration}' has no numeric registration")]
    UnknownAuthoritativeRegistration {
        /// Missing registration.
        registration: StableId,
    },
    /// A scheduled system has no exact numeric registration and owner.
    #[error("scheduled system '{system}' has no exact registration and owner")]
    UnknownScheduledSystem {
        /// Missing system.
        system: StableId,
    },
    /// A schedule entry does not use the stable `system` registration kind.
    #[error("schedule entry '{system}' is not a system registration")]
    InvalidScheduledSystemKind {
        /// Invalid schedule entry.
        system: StableId,
    },
    /// A scheduled system appears more than once.
    #[error("scheduled system '{system}' appears more than once")]
    DuplicateScheduledSystem {
        /// Duplicate system.
        system: StableId,
    },
    /// A catalog key and enclosed registration ID disagree.
    #[error("{catalog} catalog key '{key}' contains registration '{declared_id}'")]
    CatalogKeyMismatch {
        /// Catalog surface.
        catalog: CatalogKind,
        /// Map key.
        key: StableId,
        /// Enclosed ID.
        declared_id: Box<StableId>,
    },
    /// A catalog ID has no exact numeric registration and owner row.
    #[error("{catalog} '{registration}' has no exact registration")]
    UnknownCatalogRegistration {
        /// Catalog surface.
        catalog: CatalogKind,
        /// Missing exact registration.
        registration: StableId,
    },
    /// A catalog row is owned by a package outside the lock.
    #[error("{catalog} '{registration}' is owned by unlocked package '{package}'")]
    UnknownCatalogOwner {
        /// Catalog surface.
        catalog: CatalogKind,
        /// Registration or callback.
        registration: StableId,
        /// Missing package.
        package: PackageName,
    },
    /// Catalog provenance disagrees with an exact owner row.
    #[error(
        "{catalog} '{registration}' declares '{declared_by}', but exact owner is '{registered_owner}'"
    )]
    CatalogOwnerMismatch {
        /// Catalog surface.
        catalog: CatalogKind,
        /// Catalog registration.
        registration: StableId,
        /// Catalog-declared owner.
        declared_by: PackageName,
        /// Exact registered owner.
        registered_owner: PackageName,
    },
    /// A catalog row references an unknown schema.
    #[error("{catalog} '{registration}' references unknown schema '{schema}'")]
    UnknownSchemaReference {
        /// Referring registration.
        registration: StableId,
        /// Missing schema.
        schema: Box<SchemaId>,
        /// Catalog surface.
        catalog: CatalogKind,
    },
    /// A locked package has no runtime binding.
    #[error("locked package '{package}' has no runtime binding")]
    MissingRuntimePackage {
        /// Missing package.
        package: PackageName,
    },
    /// Runtime contains a package outside the graph.
    #[error("runtime image contains unlocked package '{package}'")]
    UnexpectedRuntimePackage {
        /// Unexpected package.
        package: PackageName,
    },
    /// Runtime and lock realization kinds differ.
    #[error("runtime realization for '{package}' is {runtime:?}, lock requires {locked:?}")]
    RuntimeRealizationMismatch {
        /// Package.
        package: PackageName,
        /// Locked realization.
        locked: RealizationKind,
        /// Runtime realization.
        runtime: RealizationKind,
    },
    /// Runtime and lock artifact hashes differ.
    #[error("runtime artifact for '{package}' is {runtime}, lock requires {locked}")]
    RuntimeArtifactMismatch {
        /// Package.
        package: PackageName,
        /// Locked artifact hash.
        locked: CanonicalHash,
        /// Runtime artifact hash.
        runtime: CanonicalHash,
    },
    /// A callback has no exact registration.
    #[error("{context} callback '{callback}' has no exact registration")]
    UnknownCallbackRegistration {
        /// Callback ID.
        callback: StableId,
        /// Requiring surface.
        context: CallbackContext,
    },
    /// Callback owner and declaring catalog row differ.
    #[error(
        "{context} callback '{callback}' declares '{declared_by}', but exact owner is '{registered_owner}'"
    )]
    CallbackOwnerMismatch {
        /// Callback ID.
        callback: StableId,
        /// Requiring surface.
        context: CallbackContext,
        /// Catalog-declared owner.
        declared_by: PackageName,
        /// Exact registered owner.
        registered_owner: PackageName,
    },
    /// A runtime callback key is installed under a package other than its receipt owner.
    #[error(
        "runtime callback '{callback}' is installed under '{actual}', but receipt owner is '{expected}'"
    )]
    RuntimeCallbackOwnerMismatch {
        /// Stable callback key.
        callback: StableId,
        /// Package owner frozen by the callback receipt.
        expected: PackageName,
        /// Runtime package exposing the callback.
        actual: PackageName,
    },
    /// A required callback is absent from runtime.
    #[error("required callback '{callback}' is missing from package '{package}'")]
    MissingRuntimeCallback {
        /// Expected package.
        package: PackageName,
        /// Missing callback.
        callback: StableId,
    },
    /// Runtime exposes a callback absent from declared surfaces.
    #[error("runtime package '{package}' exposes undeclared callback '{callback}'")]
    UnexpectedRuntimeCallback {
        /// Runtime package.
        package: PackageName,
        /// Extra callback.
        callback: StableId,
    },
}
