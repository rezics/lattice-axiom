//! Reopen verification of a product lock against local CAS objects.
//!
//! This module consumes the existing content-addressed store and local catalog
//! object locators. It does not persist a lock, open a world writer, load
//! native modules, or start a Bevy App. Missing objects fail closed and are
//! never reconstructed from an acquisition path.

use latticeaxiom_compose::{
    LockActionMode, LockV1, ProductLockError, ProductLockHostReceipts, ProductLockObjects,
    ProductLockReceiptKind, verify_product_lock,
};
use latticeaxiom_core::{CanonicalHash, PackageName};

use crate::cas::{CasObjectId, CasObjectKind, CasObjectStore};
use crate::error::CasError;

/// Loads lock-required CAS objects without consulting acquisition paths.
///
/// Absent objects are omitted so [`LockActionMode::Frozen`] can fail closed
/// while `--locked` may still observe a partial store.
///
/// # Errors
///
/// Returns [`ProductLockError`] when stored bytes do not match their digest.
pub fn product_lock_objects_from_cas<S: CasObjectStore>(
    lock: &LockV1,
    store: &S,
) -> Result<ProductLockObjects, ProductLockError> {
    let mut objects = ProductLockObjects::default();
    for (name, package) in &lock.portable_resolution.packages {
        insert_optional(
            &mut objects.manifests,
            store,
            CasObjectKind::PackageManifest,
            package.manifest_digest,
            ProductLockReceiptKind::Manifest,
            Some(name),
        )?;
        insert_optional(
            &mut objects.sources,
            store,
            CasObjectKind::SourceTree,
            package.source_object_digest,
            ProductLockReceiptKind::Source,
            Some(name),
        )?;
    }
    for realization in lock.realizations.values() {
        for (name, package) in &realization.packages {
            insert_optional(
                &mut objects.artifacts,
                store,
                CasObjectKind::RealizedArtifact,
                package.artifact_digest,
                ProductLockReceiptKind::Artifact,
                Some(name),
            )?;
        }
    }
    Ok(objects)
}

/// Verifies a reopened product lock against CAS and host receipts.
///
/// [`LockActionMode::Frozen`] requires exact manifest, source, toolchain,
/// engine-build, artifact, alias, and registration evidence. The original
/// package path is never read.
///
/// # Errors
///
/// Returns [`ProductLockError`] naming the first missing or mismatched
/// receipt.
pub fn verify_product_lock_from_cas<S: CasObjectStore>(
    lock: &LockV1,
    store: &S,
    host: &ProductLockHostReceipts,
    mode: LockActionMode,
) -> Result<(), ProductLockError> {
    let objects = product_lock_objects_from_cas(lock, store)?;
    verify_product_lock(lock, &objects, host, mode)
}

fn insert_optional<S: CasObjectStore>(
    dest: &mut std::collections::BTreeMap<CanonicalHash, Vec<u8>>,
    store: &S,
    kind: CasObjectKind,
    digest: CanonicalHash,
    receipt: ProductLockReceiptKind,
    package: Option<&PackageName>,
) -> Result<(), ProductLockError> {
    let id = CasObjectId::new(kind, digest);
    match store.get(&id) {
        Ok(bytes) => {
            dest.insert(digest, bytes);
            Ok(())
        }
        Err(CasError::MissingObject { .. }) => Ok(()),
        Err(CasError::DigestMismatch { actual, .. }) => Err(ProductLockError::ReceiptMismatch {
            receipt,
            package: package.cloned(),
            expected: digest,
            actual,
        }),
        Err(error) => Err(ProductLockError::InvalidStructure {
            reason: error.to_string(),
        }),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};
    use std::fmt::Debug;
    use std::path::PathBuf;

    use latticeaxiom_compose::{
        CompositionBootstrapV1, LOCK_SCHEMA_VERSION, LockActionMode, LockedGameGraph,
        LockedPackage, NickelEvaluationLimits, ObservabilityCatalog, PRODUCT_LOCK_PRODUCER_MACHINE,
        PackageDomain, ProductLockDraftV1, ProductLockError, ProductLockHostReceipts,
        ProductLockProducerV1, ProfileKind, RealizationId, RealizationKind, RegistrationImage,
        RuntimeBinding, RuntimeImage, SemanticCatalog, SettingsCatalog, TargetPackageRealizationV1,
        TargetRealizationLockV1,
    };
    use latticeaxiom_core::{
        CanonicalHash, PackageName, PackageVersion, SchemaId, SourceId, StableId, TargetTriple,
    };

    use super::*;
    use crate::cas::MemoryCas;

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

    fn bootstrap() -> CompositionBootstrapV1 {
        succeeded(CompositionBootstrapV1::from_toml_str(
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
        ))
    }

    fn fixture_lock() -> (LockV1, Vec<u8>, Vec<u8>, Vec<u8>, PathBuf) {
        let source = b"terrain-source".to_vec();
        let manifest = b"terrain-manifest".to_vec();
        let artifact = b"terrain-artifact".to_vec();
        let package = locked_package(&source, &manifest, &artifact);
        let name = package.name.clone();
        let graph = hashed_graph(LockedGameGraph {
            schema_version: LOCK_SCHEMA_VERSION,
            composition_hash: CanonicalHash::digest(b"composition"),
            composition_provenance_hash: CanonicalHash::digest(b"composition-provenance"),
            evaluation_policy: policy(),
            evaluation_limits: NickelEvaluationLimits::default(),
            roots: BTreeSet::from([name.clone()]),
            packages: BTreeMap::from([(name.clone(), package)]),
            capability_providers: BTreeMap::new(),
            namespace_grants: BTreeSet::new(),
            explanation: Vec::new(),
            graph_hash: CanonicalHash::digest(b"unverified-graph"),
            lock_hash: CanonicalHash::digest(b"unverified-lock"),
        });
        let registration_image = registration_image(&graph);
        let runtime_image = RuntimeImage {
            registration_hash: registration_image.image_hash,
            packages: BTreeMap::from([(
                name.clone(),
                RuntimeBinding {
                    realization: RealizationKind::Data,
                    artifact_hash: CanonicalHash::digest(&artifact),
                    callbacks: BTreeSet::new(),
                },
            )]),
        };
        let toolchain = CanonicalHash::digest(b"toolchain");
        let runtime_image_fingerprint =
            succeeded(latticeaxiom_core::canonical_json_hash(&runtime_image));
        let lock = succeeded(LockV1::seal(ProductLockDraftV1 {
            producer: ProductLockProducerV1 {
                machine: PRODUCT_LOCK_PRODUCER_MACHINE.to_owned(),
                toolchain,
            },
            bootstrap: bootstrap(),
            alias_edges: BTreeMap::new(),
            resolution_receipt_hash: CanonicalHash::digest(b"resolution-receipt"),
            evaluation_policy_receipt_hash: CanonicalHash::digest(b"evaluation-policy"),
            package_features: BTreeMap::new(),
            source_objects: BTreeMap::from([(name.clone(), CanonicalHash::digest(&source))]),
            realizations: BTreeMap::from([(
                target(),
                TargetRealizationLockV1 {
                    projection: ProfileKind::HeadlessTest,
                    target: target(),
                    toolchain,
                    build_intent_hash: CanonicalHash::digest(b"build-intent"),
                    packages: BTreeMap::from([(
                        name,
                        TargetPackageRealizationV1 {
                            realization_id: succeeded(RealizationId::new("data")),
                            kind: RealizationKind::Data,
                            artifact_digest: CanonicalHash::digest(&artifact),
                            engine_build_id: None,
                            registration_hash: CanonicalHash::digest(&manifest),
                        },
                    )]),
                    engine_build_id: None,
                    registration_image_hash: registration_image.image_hash,
                    runtime_image_fingerprint,
                },
            )]),
            registration_image,
            registration_semantic_hash: CanonicalHash::digest(b"registration-semantic"),
            runtime_image,
            graph,
        }));
        (
            lock,
            source,
            manifest,
            artifact,
            PathBuf::from("packages/terrain"),
        )
    }

    fn locked_package(source: &[u8], manifest: &[u8], artifact: &[u8]) -> LockedPackage {
        LockedPackage {
            name: package_name("terrain"),
            version: version("1.0.0"),
            source_id: source_id("terrain"),
            source_hash: CanonicalHash::digest(source),
            provenance_hash: CanonicalHash::digest(b"provenance"),
            realization: RealizationKind::Data,
            realization_id: succeeded(RealizationId::new("data")),
            manifest_hash: CanonicalHash::digest(manifest),
            artifact_hash: CanonicalHash::digest(artifact),
            interfaces: BTreeMap::new(),
            engine_build_id: None,
            domains: BTreeSet::from([PackageDomain::Authoritative]),
            dependencies: BTreeMap::new(),
            schemas: BTreeSet::<SchemaId>::new(),
            source_path: "packages/terrain".to_owned(),
        }
    }

    fn hashed_graph(mut graph: LockedGameGraph) -> LockedGameGraph {
        graph.graph_hash = succeeded(graph.recompute_graph_hash());
        graph.lock_hash = succeeded(graph.recompute_lock_hash());
        succeeded(graph.verify_hashes());
        graph
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

    fn populated_cas(source: &[u8], manifest: &[u8], artifact: &[u8]) -> MemoryCas {
        let mut store = MemoryCas::new();
        succeeded(store.put(CasObjectKind::SourceTree, source));
        succeeded(store.put(CasObjectKind::PackageManifest, manifest));
        succeeded(store.put(CasObjectKind::RealizedArtifact, artifact));
        store
    }

    #[test]
    fn frozen_verification_reads_cas_not_acquisition_path() {
        let (lock, source, manifest, artifact, acquisition) = fixture_lock();
        let store = populated_cas(&source, &manifest, &artifact);
        succeeded(verify_product_lock_from_cas(
            &lock,
            &store,
            &host_from(&lock),
            LockActionMode::Frozen,
        ));

        let mut missing = MemoryCas::new();
        succeeded(missing.put(CasObjectKind::PackageManifest, &manifest));
        succeeded(missing.put(CasObjectKind::RealizedArtifact, &artifact));
        let _ = acquisition;
        match verify_product_lock_from_cas(
            &lock,
            &missing,
            &host_from(&lock),
            LockActionMode::Frozen,
        ) {
            Err(ProductLockError::MissingReceipt {
                receipt: ProductLockReceiptKind::Source,
                ..
            }) => {}
            other => panic!("frozen mode must not rebuild source from a path, got {other:?}"),
        }
    }

    #[test]
    fn frozen_cas_mismatch_names_the_receipt() {
        let (lock, source, manifest, artifact, _) = fixture_lock();
        let inner = populated_cas(&source, &manifest, &artifact);
        let store = TamperedCas {
            inner,
            tampered: (
                CasObjectId::from_payload(CasObjectKind::PackageManifest, &manifest),
                b"tampered-manifest".to_vec(),
            ),
        };
        match verify_product_lock_from_cas(&lock, &store, &host_from(&lock), LockActionMode::Frozen)
        {
            Err(ProductLockError::ReceiptMismatch {
                receipt: ProductLockReceiptKind::Manifest,
                ..
            }) => {}
            other => panic!("expected manifest mismatch from CAS, got {other:?}"),
        }
    }

    struct TamperedCas {
        inner: MemoryCas,
        tampered: (CasObjectId, Vec<u8>),
    }

    impl CasObjectStore for TamperedCas {
        fn put(&mut self, kind: CasObjectKind, bytes: &[u8]) -> Result<CasObjectId, CasError> {
            self.inner.put(kind, bytes)
        }

        fn get(&self, id: &CasObjectId) -> Result<Vec<u8>, CasError> {
            if id == &self.tampered.0 {
                Ok(self.tampered.1.clone())
            } else {
                self.inner.get(id)
            }
        }
    }
}
