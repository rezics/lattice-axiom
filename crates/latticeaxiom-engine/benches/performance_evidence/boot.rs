//! Reopen the frozen product lock and bind production headless images.

use std::{fs, path::Path};

use latticeaxiom_compose::{LockV1, PRODUCT_LOCK_FILE_NAME, reopen_product_lock};
use latticeaxiom_core::{CanonicalHash, TargetTriple};
use latticeaxiom_engine::LockVerifiedComposeImages;
use latticeaxiom_launcher::{HostBuildReceipts, ReopenedFinalLockV1};
use latticeaxiom_packages::{FilesystemCas, LOCAL_CATALOG_CAS_DIRECTORY};

use crate::HarnessError;

/// Reopened lock, bound images, and file-byte fingerprints.
pub struct BootedLock {
    /// Lock-verified images shared with the production headless host.
    pub images: LockVerifiedComposeImages,
    /// SHA-256 of `latticeaxiom.lock` bytes.
    pub lock_file_sha256: CanonicalHash,
    /// SHA-256 of the shell lock when present.
    pub shell_lock_file_sha256: Option<CanonicalHash>,
    /// Selected realization target.
    pub target: TargetTriple,
    /// Engine build recorded on the selected realization.
    pub engine_build_id: Option<CanonicalHash>,
    /// Registration image hash sealed in the lock.
    pub registration_image_hash: CanonicalHash,
}

/// Reopens `workspace/latticeaxiom.lock` against `workspace/catalog/cas`.
///
/// # Errors
///
/// Returns [`HarnessError`] when the lock or CAS is missing or frozen reopen
/// fails. Missing evidence fails closed; this path never creates CAS objects.
pub fn boot_from_workspace(
    workspace: &Path,
    lock_path: Option<&Path>,
) -> Result<BootedLock, HarnessError> {
    let lock_path =
        lock_path.map_or_else(|| workspace.join(PRODUCT_LOCK_FILE_NAME), Path::to_path_buf);
    if !lock_path.is_file() {
        return Err(HarnessError::MissingLock { path: lock_path });
    }
    let cas_root = workspace.join("catalog").join(LOCAL_CATALOG_CAS_DIRECTORY);
    if !cas_root.is_dir() {
        return Err(HarnessError::MissingCas { path: cas_root });
    }

    let lock_bytes = fs::read(&lock_path).map_err(|source| HarnessError::Io {
        path: lock_path.clone(),
        source,
    })?;
    let lock_file_sha256 = CanonicalHash::digest(&lock_bytes);
    let lock = reopen_product_lock(&lock_path)?;
    let host = HostBuildReceipts::sealed_by_lock(&lock)?;
    let store = FilesystemCas::open(&cas_root)?;
    let reopened = ReopenedFinalLockV1::reopen_frozen_from_cas(&lock_path, &store, &host)?;
    let target = select_target(reopened.product_lock())?;
    let engine_build_id = reopened
        .product_lock()
        .realizations
        .get(&target)
        .and_then(|realization| realization.engine_build_id);
    let registration_image_hash = reopened.product_lock().registration.image_hash;
    let images = LockVerifiedComposeImages::from_reopened_product_lock(&reopened, &target)?;
    let shell_lock_file_sha256 =
        hash_if_present(&workspace.join("run/shell").join(PRODUCT_LOCK_FILE_NAME))?;
    Ok(BootedLock {
        images,
        lock_file_sha256,
        shell_lock_file_sha256,
        target,
        engine_build_id,
        registration_image_hash,
    })
}

fn hash_if_present(path: &Path) -> Result<Option<CanonicalHash>, HarnessError> {
    if !path.is_file() {
        return Ok(None);
    }
    let bytes = fs::read(path).map_err(|source| HarnessError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    Ok(Some(CanonicalHash::digest(&bytes)))
}

fn select_target(lock: &LockV1) -> Result<TargetTriple, HarnessError> {
    if let Some(host) = preferred_host_target()
        && lock.realizations.contains_key(&host)
    {
        return Ok(host);
    }
    let mut targets = lock.realizations.keys();
    match (targets.next(), targets.next()) {
        (Some(target), None) => Ok(target.clone()),
        _ => Err(HarnessError::MissingHostRealization),
    }
}

fn preferred_host_target() -> Option<TargetTriple> {
    let architecture = std::env::consts::ARCH;
    let value = if cfg!(all(target_os = "windows", target_env = "msvc")) {
        format!("{architecture}-pc-windows-msvc")
    } else if cfg!(all(target_os = "windows", target_env = "gnu")) {
        format!("{architecture}-pc-windows-gnu")
    } else if cfg!(all(target_os = "linux", target_env = "musl")) {
        format!("{architecture}-unknown-linux-musl")
    } else if cfg!(target_os = "linux") {
        format!("{architecture}-unknown-linux-gnu")
    } else if cfg!(target_os = "macos") {
        format!("{architecture}-apple-darwin")
    } else {
        return None;
    };
    value.parse().ok()
}
