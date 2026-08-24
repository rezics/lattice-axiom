//! Ordinary launch reopens and verifies `latticeaxiom.lock` before host boot.
//!
//! [`ReopenedFinalLockV1`] is the only launch-time product-lock token. It is
//! produced by reading the persisted lock and verifying every receipt in frozen
//! mode. Client and headless hosts must share one value and must not re-resolve
//! packages. This module does not construct [`latticeaxiom_compose::RuntimeImage`],
//! load native modules, create a Bevy `App`, or open a world writer.

use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
    sync::Arc,
};

use latticeaxiom_compose::{
    LockActionMode, LockV1, ProductLockError, ProductLockHostReceipts, ProductLockObjects,
    reopen_product_lock, verify_product_lock,
};
use latticeaxiom_core::CanonicalHash;
use latticeaxiom_packages::{CasObjectStore, product_lock_objects_from_cas};
use thiserror::Error;

use crate::LaunchIntentV1;

/// Host toolchain and engine-build receipts compared during frozen reopen.
///
/// Registration-image and runtime-image fingerprints are taken from the
/// reopened lock and independently rebound by the engine after this token is
/// produced. This DTO is not a second lock schema.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HostBuildReceipts {
    /// Content identity of the current host toolchain.
    pub toolchain: CanonicalHash,
    /// Exact engine build present on the host, when required.
    pub engine_build_id: Option<CanonicalHash>,
}

impl HostBuildReceipts {
    /// Returns the toolchain and engine-build identity sealed by a product lock.
    ///
    /// Frozen reopen uses this so the client does not reconstruct host identity
    /// from ambient compiler state. Registration and runtime fingerprints are
    /// still rebound independently after this token is produced.
    ///
    /// # Errors
    ///
    /// Returns [`ProductLockBootError::ProductLock`] when the lock has no
    /// target realization.
    pub fn sealed_by_lock(lock: &LockV1) -> Result<Self, ProductLockBootError> {
        let Some(realization) = lock.realizations.values().next() else {
            return Err(ProductLockError::InvalidStructure {
                reason: "reopened product lock has no target realization".to_owned(),
            }
            .into());
        };
        Ok(Self {
            toolchain: lock.producer.toolchain,
            engine_build_id: realization.engine_build_id,
        })
    }
}

/// Failure to reopen or freeze-verify a final product lock for launch.
#[derive(Debug, Error)]
pub enum ProductLockBootError {
    /// The product lock could not be reopened or independently verified.
    #[error(transparent)]
    ProductLock(#[from] ProductLockError),
    /// A launch intent named a different product lock than the reopened file.
    #[error("launch intent shell lock does not match the reopened product lock")]
    LaunchIntentShellLockMismatch,
}

/// Immutable artifact objects proven by one frozen product-lock reopen.
///
/// Cloning this handle does not copy artifact bytes. Digests still need to be
/// selected from the corresponding final lock before lookup.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerifiedArtifactObjects {
    objects: Arc<BTreeMap<CanonicalHash, Vec<u8>>>,
}

impl VerifiedArtifactObjects {
    fn new(objects: BTreeMap<CanonicalHash, Vec<u8>>) -> Self {
        Self {
            objects: Arc::new(objects),
        }
    }

    /// Returns exact verified bytes for `digest`, if the final lock required it.
    #[must_use]
    pub fn get(&self, digest: CanonicalHash) -> Option<&[u8]> {
        self.objects.get(&digest).map(Vec::as_slice)
    }

    /// Returns the number of distinct lock-required artifacts retained.
    #[must_use]
    pub fn len(&self) -> usize {
        self.objects.len()
    }

    /// Returns whether no artifact objects were required.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.objects.is_empty()
    }
}

/// Final `latticeaxiom.lock` that has been reopened and fully verified.
///
/// Ordinary launch must obtain this token before constructing a runtime image,
/// loading native modules, or creating a Bevy application. The value is `Clone`
/// so client and headless hosts can share one reopened lock without resolving
/// again. [`latticeaxiom_packages::ResolutionReceiptV1`] is not this type.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReopenedFinalLockV1 {
    lock: LockV1,
    artifacts: VerifiedArtifactObjects,
}

impl ReopenedFinalLockV1 {
    /// Reopens `latticeaxiom.lock` and verifies every receipt in frozen mode.
    ///
    /// Missing or mismatched source, manifest, toolchain, engine-build,
    /// artifact, alias, or registration evidence fails closed. Acquisition
    /// paths are never consulted. This does not re-resolve packages, construct
    /// a runtime image, load native modules, or open a world writer.
    ///
    /// # Errors
    ///
    /// Returns [`ProductLockBootError::ProductLock`] when the file is missing,
    /// truncated, corrupted, or fails frozen receipt verification.
    pub fn reopen_frozen(
        path: impl AsRef<Path>,
        objects: &ProductLockObjects,
        host: &HostBuildReceipts,
    ) -> Result<Self, ProductLockBootError> {
        let lock = reopen_product_lock(path)?;
        verify_reopened(&lock, objects, host)?;
        Ok(Self {
            artifacts: VerifiedArtifactObjects::new(retain_borrowed_artifacts(
                &lock,
                &objects.artifacts,
            )),
            lock,
        })
    }

    /// Reopens `latticeaxiom.lock` and verifies frozen receipts against CAS.
    ///
    /// Absent CAS objects fail closed and are never rebuilt from a path.
    ///
    /// # Errors
    ///
    /// Returns [`ProductLockBootError::ProductLock`] when reopen or frozen
    /// verification fails, including a digest mismatch in the store.
    pub fn reopen_frozen_from_cas<S: CasObjectStore>(
        path: impl AsRef<Path>,
        store: &S,
        host: &HostBuildReceipts,
    ) -> Result<Self, ProductLockBootError> {
        let lock = reopen_product_lock(path)?;
        let mut objects = product_lock_objects_from_cas(&lock, store)?;
        verify_reopened(&lock, &objects, host)?;
        let required = required_artifact_digests(&lock);
        objects
            .artifacts
            .retain(|digest, _| required.contains(digest));
        Ok(Self {
            artifacts: VerifiedArtifactObjects::new(objects.artifacts),
            lock,
        })
    }

    /// Returns the verified product lock. This is not a resolution receipt.
    #[must_use]
    pub const fn product_lock(&self) -> &LockV1 {
        &self.lock
    }

    /// Returns the exact product-lock hash frozen by the reopened file.
    #[must_use]
    pub const fn product_lock_hash(&self) -> CanonicalHash {
        self.lock.product_lock_hash
    }

    /// Returns exact bytes of one artifact verified during frozen reopen.
    ///
    /// Callers must obtain `digest` from this value's target realization. No
    /// CAS access or acquisition fallback occurs here.
    #[must_use]
    pub fn verified_artifact(&self, digest: CanonicalHash) -> Option<&[u8]> {
        self.artifacts.get(digest)
    }

    /// Returns the number of distinct lock-required artifacts retained.
    #[must_use]
    pub fn verified_artifact_count(&self) -> usize {
        self.artifacts.len()
    }

    /// Returns a cheap immutable handle to all verified artifact objects.
    ///
    /// Package-to-digest selection remains sealed in [`Self::product_lock`].
    #[must_use]
    pub fn verified_artifacts(&self) -> VerifiedArtifactObjects {
        self.artifacts.clone()
    }

    /// Rejects a launch intent that does not name this reopened product lock.
    ///
    /// # Errors
    ///
    /// Returns [`ProductLockBootError::LaunchIntentShellLockMismatch`] when
    /// [`LaunchIntentV1::shell_lock_hash`] differs from this lock.
    pub fn bind_launch_intent(&self, intent: &LaunchIntentV1) -> Result<(), ProductLockBootError> {
        if intent.shell_lock_hash() != self.lock.product_lock_hash {
            return Err(ProductLockBootError::LaunchIntentShellLockMismatch);
        }
        Ok(())
    }
}

fn verify_reopened(
    lock: &LockV1,
    objects: &ProductLockObjects,
    host: &HostBuildReceipts,
) -> Result<(), ProductLockBootError> {
    let receipts = host_receipts(lock, host)?;
    verify_product_lock(lock, objects, &receipts, LockActionMode::Frozen)?;
    Ok(())
}

fn required_artifact_digests(lock: &LockV1) -> BTreeSet<CanonicalHash> {
    lock.realizations
        .values()
        .flat_map(|realization| {
            realization
                .packages
                .values()
                .map(|package| package.artifact_digest)
        })
        .collect()
}

fn retain_borrowed_artifacts(
    lock: &LockV1,
    artifacts: &BTreeMap<CanonicalHash, Vec<u8>>,
) -> BTreeMap<CanonicalHash, Vec<u8>> {
    required_artifact_digests(lock)
        .into_iter()
        .filter_map(|digest| artifacts.get(&digest).cloned().map(|bytes| (digest, bytes)))
        .collect()
}

fn host_receipts(
    lock: &LockV1,
    host: &HostBuildReceipts,
) -> Result<ProductLockHostReceipts, ProductLockError> {
    let Some(realization) = lock.realizations.values().next() else {
        return Err(ProductLockError::InvalidStructure {
            reason: "reopened product lock has no target realization".to_owned(),
        });
    };
    Ok(ProductLockHostReceipts {
        toolchain: host.toolchain,
        engine_build_id: host.engine_build_id,
        registration_image_hash: lock.registration.image_hash,
        runtime_image_fingerprint: realization.runtime_image_fingerprint,
    })
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};
    use std::fmt::Debug;
    use std::fs;
    use std::io;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    use latticeaxiom_compose::{
        CompositionBootstrapV1, LOCK_SCHEMA_VERSION, LockActionMode, LockedGameGraph,
        LockedPackage, NickelEvaluationLimits, ObservabilityCatalog, PRODUCT_LOCK_FILE_NAME,
        PRODUCT_LOCK_PRODUCER_MACHINE, PackageDomain, ProductLockDraftV1, ProductLockError,
        ProductLockProducerV1, ProductLockReceiptKind, ProfileKind, RealizationId, RealizationKind,
        RegistrationImage, RuntimeBinding, RuntimeImage, SemanticCatalog, SettingsCatalog,
        TargetPackageRealizationV1, TargetRealizationLockV1, persist_product_lock,
    };
    use latticeaxiom_core::{
        CanonicalHash, PackageName, PackageVersion, SchemaId, SourceId, StableId, TargetTriple,
    };
    use latticeaxiom_packages::{CasObjectKind, MemoryCas};

    use super::*;
    use crate::{
        LaunchAttempt, LaunchGeneration, LaunchIntentDraftV1, LaunchTargetV1,
        SettingTransactionRevision,
    };

    static NEXT_TEST_DIRECTORY: AtomicU64 = AtomicU64::new(0);

    struct TestDirectory(PathBuf);

    impl TestDirectory {
        fn create() -> Self {
            let serial = NEXT_TEST_DIRECTORY.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "latticeaxiom-launcher-boot-{}-{serial}",
                std::process::id()
            ));
            fs::create_dir_all(&path)
                .unwrap_or_else(|error| panic!("test directory was not created: {error}"));
            Self(
                fs::canonicalize(&path)
                    .unwrap_or_else(|error| panic!("test directory did not canonicalize: {error}")),
            )
        }

        fn lock_path(&self) -> PathBuf {
            self.0.join(PRODUCT_LOCK_FILE_NAME)
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let temporary_root = fs::canonicalize(std::env::temp_dir())
                .unwrap_or_else(|error| panic!("temporary root did not canonicalize: {error}"));
            assert!(
                self.0.starts_with(&temporary_root),
                "refusing to delete a test directory outside the process temporary root"
            );
            if let Err(error) = fs::remove_dir_all(&self.0)
                && error.kind() != io::ErrorKind::NotFound
            {
                panic!("test directory cleanup failed: {error}");
            }
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
kind = "fixture"
package = "terrain"
version = "1.0.0"
source_id = "latticeaxiom:source/terrain"
path = "packages/terrain"
"#,
        ))
    }

    fn fixture_lock() -> (LockV1, Vec<u8>, Vec<u8>, Vec<u8>) {
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
        (lock, source, manifest, artifact)
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

    fn host_from(lock: &LockV1) -> HostBuildReceipts {
        let realization = lock
            .realizations
            .values()
            .next()
            .unwrap_or_else(|| panic!("fixture lock must seal a realization"));
        HostBuildReceipts {
            toolchain: lock.producer.toolchain,
            engine_build_id: realization.engine_build_id,
        }
    }

    fn objects_from(source: &[u8], manifest: &[u8], artifact: &[u8]) -> ProductLockObjects {
        let mut objects = ProductLockObjects::default();
        objects
            .sources
            .insert(CanonicalHash::digest(source), source.to_vec());
        objects
            .manifests
            .insert(CanonicalHash::digest(manifest), manifest.to_vec());
        objects
            .artifacts
            .insert(CanonicalHash::digest(artifact), artifact.to_vec());
        objects
    }

    fn persist_fixture(
        directory: &TestDirectory,
    ) -> (LockV1, ProductLockObjects, HostBuildReceipts) {
        let (lock, source, manifest, artifact) = fixture_lock();
        let objects = objects_from(&source, &manifest, &artifact);
        let persisted = succeeded(persist_product_lock(
            directory.lock_path(),
            &lock,
            LockActionMode::Persist,
        ));
        let host = host_from(&persisted);
        (persisted, objects, host)
    }

    fn shell_intent(shell_lock_hash: CanonicalHash) -> LaunchIntentV1 {
        succeeded(LaunchIntentV1::seal(LaunchIntentDraftV1 {
            generation: LaunchGeneration::FIRST,
            attempt: LaunchAttempt::FIRST,
            issued_at_ms: 1_000,
            expires_at_ms: 2_000,
            target: LaunchTargetV1::Shell,
            shell_lock_hash,
            world_lock_hash: None,
            world_open_plan_hash: None,
            confirmed_setting_transaction_revision: SettingTransactionRevision::new(0),
        }))
    }

    #[test]
    fn frozen_reopen_is_shared_and_does_not_re_resolve() {
        let directory = TestDirectory::create();
        let (lock, objects, host) = persist_fixture(&directory);
        let reopened = succeeded(ReopenedFinalLockV1::reopen_frozen(
            directory.lock_path(),
            &objects,
            &host,
        ));
        let shared = reopened.clone();
        assert_eq!(reopened, shared);
        assert_eq!(reopened.product_lock_hash(), lock.product_lock_hash);
        assert_eq!(
            reopened
                .product_lock()
                .portable_resolution
                .resolution_receipt_hash,
            lock.portable_resolution.resolution_receipt_hash
        );
        succeeded(reopened.bind_launch_intent(&shell_intent(lock.product_lock_hash)));
    }

    #[test]
    fn frozen_reopen_retains_only_exact_required_artifact_bytes() {
        let directory = TestDirectory::create();
        let (_, mut objects, host) = persist_fixture(&directory);
        let required_bytes = b"terrain-artifact";
        let required_digest = CanonicalHash::digest(required_bytes);
        let extra_bytes = b"unlocked-artifact";
        let extra_digest = CanonicalHash::digest(extra_bytes);
        objects.artifacts.insert(extra_digest, extra_bytes.to_vec());

        let reopened = succeeded(ReopenedFinalLockV1::reopen_frozen(
            directory.lock_path(),
            &objects,
            &host,
        ));

        assert_eq!(reopened.verified_artifact_count(), 1);
        assert_eq!(
            reopened.verified_artifact(required_digest),
            Some(required_bytes.as_slice())
        );
        assert_eq!(reopened.verified_artifact(extra_digest), None);
    }

    #[test]
    fn missing_lock_refuses_before_runtime_image() {
        let directory = TestDirectory::create();
        let (_, objects, host) = persist_fixture(&directory);
        let missing = directory.0.join("absent.lock");
        match ReopenedFinalLockV1::reopen_frozen(&missing, &objects, &host) {
            Err(ProductLockBootError::ProductLock(ProductLockError::MissingLock { path })) => {
                assert_eq!(path, missing);
            }
            other => panic!("missing lock must fail closed, got {other:?}"),
        }
    }

    #[test]
    fn tampered_lock_bytes_refuse_before_runtime_image() {
        let directory = TestDirectory::create();
        let (_, objects, host) = persist_fixture(&directory);
        let path = directory.lock_path();
        let mut bytes = succeeded(fs::read(&path));
        let last = bytes
            .last_mut()
            .unwrap_or_else(|| panic!("persisted lock must contain bytes"));
        *last ^= 0xFF;
        succeeded(fs::write(&path, bytes));
        assert!(
            ReopenedFinalLockV1::reopen_frozen(&path, &objects, &host).is_err(),
            "tampered product lock must fail closed before runtime-image construction"
        );
    }

    #[test]
    fn missing_source_receipt_refuses_frozen_reopen() {
        let directory = TestDirectory::create();
        let (_, mut objects, host) = persist_fixture(&directory);
        objects.sources.clear();
        match ReopenedFinalLockV1::reopen_frozen(directory.lock_path(), &objects, &host) {
            Err(ProductLockBootError::ProductLock(ProductLockError::MissingReceipt {
                receipt: ProductLockReceiptKind::Source,
                ..
            })) => {}
            other => panic!("missing source must fail frozen reopen, got {other:?}"),
        }
    }

    #[test]
    fn missing_artifact_receipt_refuses_frozen_reopen() {
        let directory = TestDirectory::create();
        let (_, mut objects, host) = persist_fixture(&directory);
        objects.artifacts.clear();

        match ReopenedFinalLockV1::reopen_frozen(directory.lock_path(), &objects, &host) {
            Err(ProductLockBootError::ProductLock(ProductLockError::MissingReceipt {
                receipt: ProductLockReceiptKind::Artifact,
                package,
                expected,
            })) => {
                assert_eq!(package, Some(package_name("terrain")));
                assert_eq!(expected, CanonicalHash::digest(b"terrain-artifact"));
            }
            other => panic!("missing artifact must fail frozen reopen, got {other:?}"),
        }
    }

    #[test]
    fn tampered_artifact_bytes_refuse_frozen_reopen() {
        let directory = TestDirectory::create();
        let (_, mut objects, host) = persist_fixture(&directory);
        objects.artifacts.insert(
            CanonicalHash::digest(b"terrain-artifact"),
            b"tampered-artifact".to_vec(),
        );

        match ReopenedFinalLockV1::reopen_frozen(directory.lock_path(), &objects, &host) {
            Err(ProductLockBootError::ProductLock(ProductLockError::ReceiptMismatch {
                receipt: ProductLockReceiptKind::Artifact,
                package,
                expected,
                actual,
            })) => {
                assert_eq!(package, Some(package_name("terrain")));
                assert_eq!(expected, CanonicalHash::digest(b"terrain-artifact"));
                assert_eq!(actual, CanonicalHash::digest(b"tampered-artifact"));
            }
            other => panic!("tampered artifact must fail frozen reopen, got {other:?}"),
        }
    }

    #[test]
    fn cas_mismatch_refuses_frozen_reopen() {
        let directory = TestDirectory::create();
        let (lock, source, manifest, artifact) = fixture_lock();
        succeeded(persist_product_lock(
            directory.lock_path(),
            &lock,
            LockActionMode::Persist,
        ));
        let mut store = MemoryCas::new();
        succeeded(store.put(CasObjectKind::SourceTree, &source));
        succeeded(store.put(CasObjectKind::PackageManifest, &manifest));
        succeeded(store.put(CasObjectKind::RealizedArtifact, &artifact));
        let mut tampered = MemoryCas::new();
        succeeded(tampered.put(CasObjectKind::SourceTree, b"tampered-source"));
        succeeded(tampered.put(CasObjectKind::PackageManifest, &manifest));
        succeeded(tampered.put(CasObjectKind::RealizedArtifact, &artifact));
        match ReopenedFinalLockV1::reopen_frozen_from_cas(
            directory.lock_path(),
            &tampered,
            &host_from(&lock),
        ) {
            Err(ProductLockBootError::ProductLock(ProductLockError::MissingReceipt {
                receipt: ProductLockReceiptKind::Source,
                ..
            })) => {}
            other => panic!("tampered CAS source must fail frozen reopen, got {other:?}"),
        }
        succeeded(ReopenedFinalLockV1::reopen_frozen_from_cas(
            directory.lock_path(),
            &store,
            &host_from(&lock),
        ));
    }

    #[test]
    fn launch_intent_for_another_lock_is_rejected() {
        let directory = TestDirectory::create();
        let (_, objects, host) = persist_fixture(&directory);
        let reopened = succeeded(ReopenedFinalLockV1::reopen_frozen(
            directory.lock_path(),
            &objects,
            &host,
        ));
        let mismatched = shell_intent(CanonicalHash::digest(b"other-product-lock"));
        assert!(matches!(
            reopened.bind_launch_intent(&mismatched),
            Err(ProductLockBootError::LaunchIntentShellLockMismatch)
        ));
    }
}
