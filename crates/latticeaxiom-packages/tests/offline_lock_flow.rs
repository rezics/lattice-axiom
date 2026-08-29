//! Offline bootstrap-first lock transaction over shipped packages.

use std::fmt::Debug;
use std::fs;
use std::path::{Path, PathBuf};

use latticeaxiom_compose::{
    ArtifactIntent, COMPOSITION_BOOTSTRAP_FILE_NAME, LockActionMode, PRODUCT_LOCK_FILE_NAME,
    ProductLockError, ProductLockReceiptKind, RealizationKind, RealizedDataRootError,
    RealizedDataRootV1, SourceSnapshot,
};
use latticeaxiom_core::{CanonicalHash, CanonicalLogicalPath, PackageName};
use latticeaxiom_packages::{
    CasObjectId, CasObjectKind, CasObjectStore, FilesystemCas, LOCAL_CATALOG_CAS_DIRECTORY,
    OfflineLockRequestV1, TransactionError, persist_offline_lock, reopen_offline_lock_frozen,
    verify_product_lock_from_cas,
};

#[derive(Debug)]
struct TestDirectory(tempfile::TempDir);

impl TestDirectory {
    fn create() -> Self {
        Self(succeeded(
            tempfile::Builder::new()
                .prefix("latticeaxiom-packages-offline-lock-")
                .tempdir(),
        ))
    }

    fn path(&self) -> &Path {
        self.0.path()
    }
}

fn succeeded<T, E>(result: Result<T, E>) -> T
where
    E: Debug,
{
    result.unwrap_or_else(|error| panic!("operation unexpectedly failed: {error:?}"))
}

fn shipped_workspace() -> PathBuf {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    succeeded(std::path::absolute(manifest.join("../..")))
}

fn package_name(value: &str) -> PackageName {
    succeeded(value.parse())
}

fn copy_tree(source: &Path, dest: &Path) {
    succeeded(fs::create_dir_all(dest));
    for entry in succeeded(fs::read_dir(source)) {
        let entry = succeeded(entry);
        let name = entry.file_name();
        if name == ".git" || name == "target" {
            continue;
        }
        let from = entry.path();
        let to = dest.join(&name);
        let metadata = succeeded(fs::symlink_metadata(&from));
        if metadata.file_type().is_symlink() {
            continue;
        }
        if metadata.is_dir() {
            copy_tree(&from, &to);
        } else {
            succeeded(fs::copy(&from, &to));
        }
    }
}

fn stage_shipped_workspace(dest: &Path) {
    let source = shipped_workspace();
    succeeded(fs::copy(
        source.join(COMPOSITION_BOOTSTRAP_FILE_NAME),
        dest.join(COMPOSITION_BOOTSTRAP_FILE_NAME),
    ));
    copy_tree(&source.join("packages"), &dest.join("packages"));
}

fn toml_string_array(values: &[&str]) -> String {
    format!(
        "[{}]",
        values
            .iter()
            .map(|value| format!("\"{value}\""))
            .collect::<Vec<_>>()
            .join(", ")
    )
}

#[derive(Clone, Copy)]
struct FixtureSource<'a> {
    included: &'a [&'a str],
    excluded: &'a [&'a str],
    files: &'a [(&'a str, &'a [u8])],
    directories: &'a [&'a str],
}

fn stage_single_package_workspace(
    workspace: &Path,
    realization_policy: &str,
    realization_kind: &str,
    artifact: &str,
    source: FixtureSource<'_>,
) {
    let bootstrap = format!(
        r#"schema_version = 1
projection = "headless-test"
projection_domains = ["authoritative"]
evaluation_policy = "latticeaxiom:nickel-evaluation-policy/r0@1"
realization_policy = ["{realization_policy}"]
nickel_profile_entry = "profiles/test.ncl"

[roots.terrain]
version = "=1.0.0"
realization = {{ mode = "auto" }}

[[sources]]
kind = "path"
package = "terrain"
path = "packages/terrain"
"#
    );
    succeeded(fs::write(
        workspace.join(COMPOSITION_BOOTSTRAP_FILE_NAME),
        bootstrap,
    ));

    let package_root = workspace.join("packages/terrain");
    succeeded(fs::create_dir_all(&package_root));
    for directory in source.directories {
        succeeded(fs::create_dir_all(package_root.join(directory)));
    }
    for (logical_path, bytes) in source.files {
        let path = package_root.join(logical_path);
        if let Some(parent) = path.parent() {
            succeeded(fs::create_dir_all(parent));
        }
        succeeded(fs::write(path, bytes));
    }
    let targets = if realization_kind == "data" {
        String::new()
    } else {
        "targets = [\"x86_64-pc-windows-msvc\"]\n".to_owned()
    };
    let manifest = format!(
        r#"schema_version = 1
name = "terrain"
version = "1.0.0"
domains = ["authoritative"]
trust = "data-only"

[realizations.selected]
id = "selected"
kind = "{realization_kind}"
domains = ["authoritative"]
{targets}artifact = {artifact}
trust = "data-only"

[nickel_public_entrypoints]
default = "package.ncl"

[source_inclusion]
include = {included}
exclude = {excluded}
"#,
        included = toml_string_array(source.included),
        excluded = toml_string_array(source.excluded),
    );
    succeeded(fs::write(
        package_root.join("latticeaxiom-package.toml"),
        manifest,
    ));
}

fn single_package_request(workspace: &Path) -> OfflineLockRequestV1 {
    OfflineLockRequestV1 {
        workspace_root: workspace.to_path_buf(),
        bootstrap_path: PathBuf::from(COMPOSITION_BOOTSTRAP_FILE_NAME),
        catalog_root: workspace.join("catalog"),
        lock_path: PathBuf::from(PRODUCT_LOCK_FILE_NAME),
        target: succeeded("x86_64-pc-windows-msvc".parse()),
        toolchain: CanonicalHash::digest(b"single-package-offline-lock"),
    }
}

#[test]
fn offline_bootstrap_lock_transaction_reopens_frozen_and_rejects_tampered_source() {
    let workspace = TestDirectory::create();
    stage_shipped_workspace(workspace.path());

    let catalog_root = workspace.path().join("catalog");
    let request = OfflineLockRequestV1 {
        workspace_root: workspace.path().to_path_buf(),
        bootstrap_path: PathBuf::from(COMPOSITION_BOOTSTRAP_FILE_NAME),
        catalog_root: catalog_root.clone(),
        lock_path: PathBuf::from(PRODUCT_LOCK_FILE_NAME),
        target: succeeded("x86_64-pc-windows-msvc".parse()),
        toolchain: CanonicalHash::digest(b"latticeaxiom-packages-offline-lock"),
    };

    let outcome = succeeded(persist_offline_lock(&request));
    let lock_path = workspace.path().join(PRODUCT_LOCK_FILE_NAME);
    assert!(lock_path.is_file(), "product lock must be persisted");
    assert_eq!(
        outcome.lock.portable_resolution.resolution_receipt_hash,
        outcome.resolution.receipt_hash
    );
    assert!(outcome.resolution.roots.contains(&package_name("terrenia")));
    assert!(
        outcome
            .resolution
            .roots
            .contains(&package_name("@latticeaxiom/settings"))
    );
    assert!(
        outcome
            .resolution
            .packages
            .contains_key(&package_name("@terrenia/blocks"))
    );
    assert!(
        !outcome
            .resolution
            .packages
            .contains_key(&package_name("@terrenia/presentation")),
        "client-only presentation must stay outside the dedicated-server projection"
    );

    let terrenia_ncl = workspace
        .path()
        .join("packages")
        .join("terrenia")
        .join("main")
        .join("package.ncl");
    succeeded(fs::write(
        &terrenia_ncl,
        b"// mutated after catalog acquire\n",
    ));
    succeeded(reopen_offline_lock_frozen(
        &lock_path,
        &catalog_root,
        &outcome.host,
    ));

    let source_digest = outcome
        .lock
        .portable_resolution
        .packages
        .values()
        .next()
        .map_or_else(
            || panic!("portable resolution must contain at least one package"),
            |package| package.source_object_digest,
        );
    let source_object = catalog_root
        .join(LOCAL_CATALOG_CAS_DIRECTORY)
        .join(CasObjectKind::SourceTree.as_str())
        .join(source_digest.to_string());
    succeeded(fs::write(&source_object, b"tampered-source-digest\n"));

    match reopen_offline_lock_frozen(&lock_path, &catalog_root, &outcome.host) {
        Err(TransactionError::ProductLock(ProductLockError::ReceiptMismatch {
            receipt: ProductLockReceiptKind::Source,
            expected,
            ..
        })) => {
            assert_eq!(expected, source_digest);
        }
        other => panic!("tampered source digest must fail frozen reopen, got {other:?}"),
    }

    let store = succeeded(latticeaxiom_packages::FilesystemCas::open(
        catalog_root.join(LOCAL_CATALOG_CAS_DIRECTORY),
    ));
    match verify_product_lock_from_cas(&outcome.lock, &store, &outcome.host, LockActionMode::Frozen)
    {
        Err(ProductLockError::ReceiptMismatch {
            receipt: ProductLockReceiptKind::Source,
            ..
        }) => {}
        other => panic!("independent frozen verification must observe the tamper, got {other:?}"),
    }
}

#[test]
fn offline_data_root_artifact_round_trips_with_exact_slice_and_package_binding() {
    let workspace = TestDirectory::create();
    stage_single_package_workspace(
        workspace.path(),
        "data",
        "data",
        r#"{ kind = "data-root", path = "data" }"#,
        FixtureSource {
            included: &[
                "package.ncl",
                "latticeaxiom-package.toml",
                "data",
                "database",
            ],
            excluded: &["data/private"],
            files: &[
                ("package.ncl", b"{}\n"),
                ("data/public.bin", b"public-data"),
                ("data/private/secret.bin", b"excluded-data"),
                ("database/leak.bin", b"wrong-prefix"),
            ],
            directories: &[],
        },
    );
    let request = single_package_request(workspace.path());
    let outcome = succeeded(persist_offline_lock(&request));
    let terrain = package_name("terrain");
    let target = outcome
        .lock
        .realizations
        .get(&request.target)
        .unwrap_or_else(|| panic!("target realization is missing"));
    let locked_realization = target
        .packages
        .get(&terrain)
        .unwrap_or_else(|| panic!("terrain realization is missing"));
    let store = succeeded(FilesystemCas::open(
        request.catalog_root.join(LOCAL_CATALOG_CAS_DIRECTORY),
    ));
    let artifact_bytes = succeeded(store.get(&CasObjectId::new(
        CasObjectKind::RealizedArtifact,
        locked_realization.artifact_digest,
    )));
    let artifact = succeeded(RealizedDataRootV1::from_canonical_bytes_for_package(
        &artifact_bytes,
        &terrain,
    ));
    assert_eq!(artifact.package(), &terrain);
    assert_eq!(artifact.root().as_str(), "data");
    assert_eq!(
        artifact
            .files()
            .map(|(path, _)| path.as_str())
            .collect::<Vec<_>>(),
        vec!["data/public.bin"]
    );
    assert_eq!(
        artifact.file(&succeeded(CanonicalLogicalPath::new("data/public.bin"))),
        Some(b"public-data".as_slice())
    );

    let wrong_package = package_name("other");
    match RealizedDataRootV1::from_canonical_bytes_for_package(&artifact_bytes, &wrong_package) {
        Err(RealizedDataRootError::PackageMismatch { expected, actual }) => {
            assert_eq!(expected, wrong_package);
            assert_eq!(actual, terrain);
        }
        other => panic!("wrong package binding must fail, got {other:?}"),
    }

    let portable = outcome
        .lock
        .portable_resolution
        .packages
        .get(&terrain)
        .unwrap_or_else(|| panic!("portable terrain package is missing"));
    let source_bytes = succeeded(store.get(&CasObjectId::new(
        CasObjectKind::SourceTree,
        portable.source_object_digest,
    )));
    assert_ne!(source_bytes, artifact_bytes);
    let source: SourceSnapshot = succeeded(serde_json::from_slice(&source_bytes));
    assert!(source.files().contains_key("package.ncl"));
    assert!(source.files().contains_key("latticeaxiom-package.toml"));
    assert!(source.files().contains_key("data/public.bin"));
    assert!(source.files().contains_key("database/leak.bin"));
    assert!(!source.files().contains_key("data/private/secret.bin"));
}

fn assert_empty_data_root_case(
    root: &str,
    included: &[&str],
    excluded: &[&str],
    files: &[(&str, &[u8])],
    directories: &[&str],
) {
    let workspace = TestDirectory::create();
    let artifact = format!(r#"{{ kind = "data-root", path = "{root}" }}"#);
    stage_single_package_workspace(
        workspace.path(),
        "data",
        "data",
        &artifact,
        FixtureSource {
            included,
            excluded,
            files,
            directories,
        },
    );
    let request = single_package_request(workspace.path());
    match persist_offline_lock(&request) {
        Err(TransactionError::RealizedDataRoot {
            package,
            source: RealizedDataRootError::Empty { root: actual },
        }) => {
            assert_eq!(package, package_name("terrain"));
            assert_eq!(actual.as_str(), root);
        }
        other => panic!("empty data root `{root}` must fail closed, got {other:?}"),
    }
    assert!(!workspace.path().join(PRODUCT_LOCK_FILE_NAME).exists());
}

#[test]
fn offline_data_root_rejects_missing_empty_and_fully_excluded_roots() {
    assert_empty_data_root_case(
        "data/missing",
        &["package.ncl", "latticeaxiom-package.toml"],
        &[],
        &[("package.ncl", b"{}\n")],
        &[],
    );
    assert_empty_data_root_case(
        "data/empty",
        &["package.ncl", "latticeaxiom-package.toml", "data/empty"],
        &[],
        &[("package.ncl", b"{}\n")],
        &["data/empty"],
    );
    assert_empty_data_root_case(
        "data",
        &["package.ncl", "latticeaxiom-package.toml", "data"],
        &["data"],
        &[("package.ncl", b"{}\n"), ("data/private.bin", b"excluded")],
        &[],
    );
}

fn assert_unsupported_artifact_case(
    policy: &str,
    kind: &str,
    artifact_toml: &str,
    expected_kind: RealizationKind,
    expected_artifact: &ArtifactIntent,
) {
    let workspace = TestDirectory::create();
    stage_single_package_workspace(
        workspace.path(),
        policy,
        kind,
        artifact_toml,
        FixtureSource {
            included: &["package.ncl", "latticeaxiom-package.toml", "data"],
            excluded: &[],
            files: &[("package.ncl", b"{}\n"), ("data/value.bin", b"fixture")],
            directories: &[],
        },
    );
    let request = single_package_request(workspace.path());
    match persist_offline_lock(&request) {
        Err(TransactionError::UnsupportedArtifactMaterialization {
            package,
            realization,
            artifact,
        }) => {
            assert_eq!(package, package_name("terrain"));
            assert_eq!(realization, expected_kind);
            assert_eq!(&artifact, expected_artifact);
        }
        other => panic!("unsupported {kind} artifact must fail closed, got {other:?}"),
    }
    assert!(!workspace.path().join(PRODUCT_LOCK_FILE_NAME).exists());
}

#[test]
fn offline_lock_rejects_unsupported_artifact_intents_and_realization_kinds() {
    assert_unsupported_artifact_case(
        "data",
        "data",
        r#"{ kind = "source-build" }"#,
        RealizationKind::Data,
        &ArtifactIntent::SourceBuild,
    );
    assert_unsupported_artifact_case(
        "data",
        "data",
        r#"{ kind = "local-prebuilt", path = "bin/terrain.dll" }"#,
        RealizationKind::Data,
        &ArtifactIntent::LocalPrebuilt {
            path: succeeded(CanonicalLogicalPath::new("bin/terrain.dll")),
        },
    );
    assert_unsupported_artifact_case(
        "native-static",
        "native-static",
        r#"{ kind = "data-root", path = "data" }"#,
        RealizationKind::NativeStatic,
        &ArtifactIntent::DataRoot {
            path: succeeded(CanonicalLogicalPath::new("data")),
        },
    );
}
