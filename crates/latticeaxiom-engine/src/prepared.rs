//! Receipt verification and runtime binding before Bevy host construction.

use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    fmt,
};

use bevy::prelude::Resource;
use latticeaxiom_compose::{
    GraphHashError, LOCK_SCHEMA_VERSION, LockedGameGraph, RealizationKind, RuntimeBinding,
    RuntimeImage, TargetRealizationLockV1,
};
use latticeaxiom_core::{
    CanonicalHash, CanonicalJsonError, CapabilityId, PackageName, PackageVersion, SchemaId,
    StableId, TargetTriple, canonical_json_hash,
};
use latticeaxiom_launcher::ReopenedFinalLockV1;
use latticeaxiom_registration::{CallbackMapReceipt, CompiledRegistration, ReceiptValidationError};
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

/// Exact lock, compiled registration, and runtime image bound to a reopened
/// final `latticeaxiom.lock`.
///
/// Construction reopens no files and does not call the package resolver. It
/// constructs [`RuntimeImage`] from the verified target realization and
/// callback receipt, then applies the same structural gate as
/// [`StructurallyValidatedComposeImages`]. Native modules are never mapped.
/// Client and headless hosts must share one value.
#[derive(Clone, Debug, Resource)]
pub struct LockVerifiedComposeImages {
    images: StructurallyValidatedComposeImages,
    product_lock_hash: CanonicalHash,
    target: TargetTriple,
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
        Ok(Self {
            images,
            product_lock_hash: lock.product_lock_hash(),
            target: target.clone(),
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

    /// Consumes the lock-verified images.
    #[must_use]
    pub fn into_images(self) -> StructurallyValidatedComposeImages {
        self.images
    }
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
