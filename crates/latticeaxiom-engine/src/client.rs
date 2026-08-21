//! Interactive `DefaultPlugins` client boot from a reopened product lock.

use std::{
    io,
    path::{Path, PathBuf},
};

use latticeaxiom_compose::{LockV1, PRODUCT_LOCK_FILE_NAME, ProductLockError, reopen_product_lock};
use latticeaxiom_core::TargetTriple;
use latticeaxiom_launcher::{
    ClientAppLeaseError, HostBuildReceipts, ProcessEpoch, ProductLockBootError,
    ReopenedFinalLockV1, claim_fresh_client_app_lease,
};
use latticeaxiom_packages::{CasError, FilesystemCas, LOCAL_CATALOG_CAS_DIRECTORY};
use thiserror::Error;

use crate::{EngineInstance, LockVerifiedComposeImages, PreparationError, ProductionHostError};

/// Directory beside `latticeaxiom.lock` that holds the local catalog and CAS.
const CLIENT_CATALOG_DIRECTORY: &str = "catalog";

/// Failure to boot the production `DefaultPlugins` client from a reopened lock.
#[derive(Debug, Error)]
pub enum ProductionClientError {
    /// The workspace current directory could not be resolved.
    #[error("workspace directory is unavailable: {source}")]
    Workspace {
        /// Underlying directory lookup failure.
        #[source]
        source: io::Error,
    },
    /// `latticeaxiom.lock` is absent from the launch workspace.
    #[error("product lock is missing at {path}; ordinary launch does not create it")]
    MissingLock {
        /// Expected lock path.
        path: PathBuf,
    },
    /// Catalog CAS is absent; frozen reopen never creates the store.
    #[error("catalog CAS object store is missing at {path}; frozen reopen never creates the store")]
    MissingCas {
        /// Expected CAS directory.
        path: PathBuf,
    },
    /// The product lock could not be read or decoded.
    #[error(transparent)]
    ProductLock(#[from] ProductLockError),
    /// The product lock could not be freeze-verified against CAS.
    #[error(transparent)]
    Boot(#[from] ProductLockBootError),
    /// The lock has no realization this host can boot.
    #[error("product lock has no realization for this host")]
    MissingHostRealization,
    /// The catalog object store could not be opened.
    #[error(transparent)]
    Cas(#[from] CasError),
    /// Lock-verified images could not be reconstructed or rebound.
    #[error(transparent)]
    Preparation(Box<PreparationError>),
    /// Production spine construction failed.
    #[error(transparent)]
    Host(Box<ProductionHostError>),
    /// This process already claimed its client App lease.
    #[error(transparent)]
    Lease(#[from] ClientAppLeaseError),
}

impl ProductionClientError {
    /// Compose command that writes the client-world product lock and catalog CAS.
    pub const LOCK_COMMAND: &'static str = "cargo run -p latticeaxiom-compose --bin latticeaxiom-compose --features nickel-evaluator -- lock --offline --bootstrap profiles/dev.toml";

    /// Returns a recovery line for ordinary-launch absences.
    ///
    /// Frozen reopen never creates the lock or CAS. Missing evidence therefore
    /// points at [`Self::LOCK_COMMAND`] rather than inventing receipts.
    #[must_use]
    pub const fn recovery_hint(&self) -> Option<&'static str> {
        match self {
            Self::MissingLock { .. } | Self::MissingCas { .. } => Some(Self::LOCK_COMMAND),
            Self::MissingHostRealization => Some(
                "relock with --bootstrap profiles/dev.toml so the product lock seals a client-world realization for this host",
            ),
            _ => None,
        }
    }
}

/// Boots the V2/V4 production host from a reopened `latticeaxiom.lock`.
///
/// Ordinary launch reads `latticeaxiom.lock` and `catalog/cas` from the
/// current workspace, freeze-verifies every CAS receipt, and starts one
/// [`bevy::prelude::DefaultPlugins`] client through
/// [`EngineInstance::new_client_host_from_lock`]. Missing lock or CAS fails
/// closed. This path does not load native modules or open a world writer.
///
/// # Errors
///
/// Returns [`ProductionClientError`] when the lock or CAS is missing, frozen
/// verification fails, reconstructed images do not rebind to the lock, or the
/// process event loop is already reserved.
pub fn run_client_host_from_lock() -> Result<(), ProductionClientError> {
    let workspace =
        std::env::current_dir().map_err(|source| ProductionClientError::Workspace { source })?;
    run_client_host_from_workspace(&workspace)
}

/// Boots the production client from lock and CAS paths under `workspace`.
///
/// # Errors
///
/// Returns [`ProductionClientError`] when lock or CAS evidence is missing or
/// host construction fails.
pub fn run_client_host_from_workspace(workspace: &Path) -> Result<(), ProductionClientError> {
    let images = load_lock_verified_images(workspace)?;
    let lease = claim_fresh_client_app_lease(ProcessEpoch::FIRST)?;
    let (instance, _proof) = EngineInstance::new_client_host_from_lock(images, lease)?;
    instance.run();
    Ok(())
}

/// Reopens and freeze-verifies the workspace product lock, then binds images.
///
/// # Errors
///
/// Returns [`ProductionClientError::MissingLock`] or
/// [`ProductionClientError::MissingCas`] when those paths are absent, and
/// otherwise the frozen-reopen or image-binding failure.
pub fn load_lock_verified_images(
    workspace: &Path,
) -> Result<LockVerifiedComposeImages, ProductionClientError> {
    let lock_path = workspace.join(PRODUCT_LOCK_FILE_NAME);
    if !lock_path.is_file() {
        return Err(ProductionClientError::MissingLock { path: lock_path });
    }
    let cas_root = workspace
        .join(CLIENT_CATALOG_DIRECTORY)
        .join(LOCAL_CATALOG_CAS_DIRECTORY);
    if !cas_root.is_dir() {
        return Err(ProductionClientError::MissingCas { path: cas_root });
    }

    let lock = reopen_product_lock(&lock_path)?;
    let host = HostBuildReceipts::sealed_by_lock(&lock)?;
    let store = FilesystemCas::open(&cas_root)?;
    let reopened = ReopenedFinalLockV1::reopen_frozen_from_cas(&lock_path, &store, &host)?;
    let target = select_target(reopened.product_lock())?;
    Ok(LockVerifiedComposeImages::from_reopened_product_lock(
        &reopened, &target,
    )?)
}

fn select_target(lock: &LockV1) -> Result<TargetTriple, ProductionClientError> {
    if let Some(host) = preferred_host_target()
        && lock.realizations.contains_key(&host)
    {
        return Ok(host);
    }
    let mut targets = lock.realizations.keys();
    match (targets.next(), targets.next()) {
        (Some(target), None) => Ok(target.clone()),
        _ => Err(ProductionClientError::MissingHostRealization),
    }
}

impl From<PreparationError> for ProductionClientError {
    fn from(error: PreparationError) -> Self {
        Self::Preparation(Box::new(error))
    }
}

impl From<ProductionHostError> for ProductionClientError {
    fn from(error: ProductionHostError) -> Self {
        Self::Host(Box::new(error))
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

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::{ProductionClientError, load_lock_verified_images};
    use std::{
        fs, io,
        path::PathBuf,
        sync::atomic::{AtomicU64, Ordering},
    };

    struct TestDirectory(PathBuf);

    impl TestDirectory {
        fn create() -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let serial = NEXT.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "latticeaxiom-engine-client-boot-{}-{serial}",
                std::process::id()
            ));
            fs::create_dir_all(&path).expect("test directory was created");
            Self(fs::canonicalize(&path).expect("test directory canonicalized"))
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            if let Err(error) = fs::remove_dir_all(&self.0)
                && error.kind() != io::ErrorKind::NotFound
            {
                panic!("test directory cleanup failed: {error}");
            }
        }
    }

    #[test]
    fn missing_lock_fails_closed() {
        let directory = TestDirectory::create();
        match load_lock_verified_images(&directory.0) {
            Err(ref error @ ProductionClientError::MissingLock { ref path }) => {
                assert_eq!(path, &directory.0.join("latticeaxiom.lock"));
                assert_eq!(
                    error.recovery_hint(),
                    Some(ProductionClientError::LOCK_COMMAND)
                );
                assert!(
                    ProductionClientError::LOCK_COMMAND.contains("--bin latticeaxiom-compose")
                        && ProductionClientError::LOCK_COMMAND.contains("profiles/dev.toml"),
                    "client lock command must name the compose binary and client-world bootstrap"
                );
            }
            other => panic!("expected missing lock, got {other:?}"),
        }
    }

    #[test]
    fn missing_cas_fails_closed() {
        let directory = TestDirectory::create();
        fs::write(directory.0.join("latticeaxiom.lock"), b"{}")
            .expect("placeholder lock bytes were written");
        match load_lock_verified_images(&directory.0) {
            Err(ref error @ ProductionClientError::MissingCas { ref path }) => {
                assert_eq!(path, &directory.0.join("catalog").join("cas"));
                assert_eq!(
                    error.recovery_hint(),
                    Some(ProductionClientError::LOCK_COMMAND)
                );
            }
            other => panic!("expected missing CAS, got {other:?}"),
        }
    }
}
