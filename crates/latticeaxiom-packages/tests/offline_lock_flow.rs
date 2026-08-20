//! Offline bootstrap-first lock transaction over shipped packages.

use std::fmt::Debug;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use latticeaxiom_compose::{
    COMPOSITION_BOOTSTRAP_FILE_NAME, LockActionMode, PRODUCT_LOCK_FILE_NAME, ProductLockError,
    ProductLockReceiptKind,
};
use latticeaxiom_core::{CanonicalHash, PackageName};
use latticeaxiom_packages::{
    CasObjectKind, LOCAL_CATALOG_CAS_DIRECTORY, OfflineLockRequestV1, TransactionError,
    persist_offline_lock, reopen_offline_lock_frozen, verify_product_lock_from_cas,
};

static NEXT_TEST_DIRECTORY: AtomicU64 = AtomicU64::new(0);

#[derive(Debug)]
struct TestDirectory(PathBuf);

impl TestDirectory {
    fn create() -> Self {
        let serial = NEXT_TEST_DIRECTORY.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "latticeaxiom-packages-offline-lock-{}-{serial}",
            std::process::id()
        ));
        succeeded(fs::create_dir_all(&path));
        Self(if path.is_absolute() {
            path
        } else {
            succeeded(std::path::absolute(&path))
        })
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        let temporary_root = std::env::temp_dir();
        let temporary_root = if temporary_root.is_absolute() {
            temporary_root
        } else {
            succeeded(std::path::absolute(&temporary_root))
        };
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
