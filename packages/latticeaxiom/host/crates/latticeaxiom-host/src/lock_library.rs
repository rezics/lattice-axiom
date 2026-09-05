//! Lazy archived-lock loading without opening a world or rebuilding sources.

use std::path::Path;

use latticeaxiom_core::{CanonicalHash, TargetTriple};
use latticeaxiom_launcher::{HostBuildReceipts, ReopenedFinalLockV1};
use latticeaxiom_packages::FilesystemCas;
use thiserror::Error;

use crate::{LockVerifiedComposeImages, PreparationError};

/// Failure to reopen an archived product's exact source/artifact closure.
#[derive(Debug, Error)]
pub enum FrozenImageLoadError {
    /// The archive's canonical identity does not match its requested key.
    #[error("archived product lock does not match its requested identity")]
    Identity,
    /// The lock is absent or invalid.
    #[error(transparent)]
    Lock(#[from] latticeaxiom_compose::ProductLockError),
    /// CAS or host receipt verification failed.
    #[error(transparent)]
    Boot(#[from] latticeaxiom_launcher::ProductLockBootError),
    /// The CAS store cannot be opened.
    #[error(transparent)]
    Cas(#[from] latticeaxiom_packages::CasError),
    /// Verified artifacts do not satisfy the runtime image contract.
    #[error(transparent)]
    Preparation(Box<PreparationError>),
}

/// Loads an exact archived product from catalog metadata and CAS, never live source paths.
///
/// # Errors
/// Returns [`FrozenImageLoadError`] when identity, lock, CAS, target or receipts fail.
pub fn load_archived_product_images(
    catalog: &Path,
    hash: CanonicalHash,
    target: &TargetTriple,
) -> Result<LockVerifiedComposeImages, FrozenImageLoadError> {
    let path = latticeaxiom_compose::archived_product_lock_path(catalog, hash);
    let lock = latticeaxiom_compose::reopen_product_lock(&path)?;
    if lock.product_lock_hash != hash {
        return Err(FrozenImageLoadError::Identity);
    }
    let host = HostBuildReceipts::sealed_by_lock(&lock)?;
    let cas = FilesystemCas::open(catalog.join("cas"))?;
    let reopened = ReopenedFinalLockV1::reopen_frozen_from_cas(path, &cas, &host)?;
    LockVerifiedComposeImages::from_reopened_product_lock(&reopened, target)
        .map_err(|error| FrozenImageLoadError::Preparation(Box::new(error)))
}
