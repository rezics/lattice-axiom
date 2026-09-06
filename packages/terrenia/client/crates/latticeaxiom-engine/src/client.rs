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

use crate::{
    EngineInstance, LockVerifiedComposeImages, PreparationError, ProductionHostError,
    ProductionMemoryStart, ProductionMemoryStartError,
};

/// Directory beside `latticeaxiom.lock` that holds the local catalog and CAS.
const CLIENT_CATALOG_DIRECTORY: &str = "catalog";

#[derive(Clone, Debug, bevy::prelude::Resource)]
pub(crate) struct ShellExitContext {
    workspace: PathBuf,
    shell_lock: latticeaxiom_core::CanonicalHash,
    handoff: crate::host::ShellHandoffState,
}

impl ShellExitContext {
    pub(crate) fn finish(&self, clean: bool) -> Result<(), String> {
        if std::env::var_os("LATTICEAXIOM_CAPTURE_PATH").is_some()
            || self.handoff.0.load(std::sync::atomic::Ordering::Acquire)
        {
            return Ok(());
        }
        let Some(root) = std::env::var_os(crate::supervisor::ENV_LAUNCH_ROOT).map(PathBuf::from)
        else {
            return Ok(());
        };
        let generation = std::env::var(crate::supervisor::ENV_GENERATION)
            .ok()
            .and_then(|value| value.parse().ok())
            .and_then(|value| latticeaxiom_launcher::LaunchGeneration::new(value).ok())
            .unwrap_or(latticeaxiom_launcher::LaunchGeneration::FIRST);
        let epoch = std::env::var(crate::supervisor::ENV_PROCESS_EPOCH)
            .ok()
            .and_then(|value| value.parse().ok())
            .and_then(|value| ProcessEpoch::new(value).ok())
            .unwrap_or(ProcessEpoch::FIRST);
        let settings = crate::settings::HostUserSettings::load(self.workspace.join("run/user"))
            .map_err(|error| error.to_string())?;
        let report = latticeaxiom_launcher::ChildExitReportV1::seal(
            latticeaxiom_launcher::ChildExitReportDraftV1 {
                child_generation: generation,
                process_epoch: epoch,
                role: crate::supervisor::shell_like_child_role(),
                exit_kind: if clean {
                    latticeaxiom_launcher::ChildExitKindV1::ShellQuit
                } else {
                    latticeaxiom_launcher::ChildExitKindV1::Crash
                },
                intent_generation: None,
                intent_checksum: None,
                confirmed_setting_transaction_revision:
                    latticeaxiom_launcher::SettingTransactionRevision::new(
                        settings.transaction_revision(),
                    ),
                last_written_world: None,
                last_durable_world: None,
                shell_lock_hash: self.shell_lock,
                world_lock_hash: None,
                world_open_plan_hash: None,
                diagnostic_ref: None,
            },
        )
        .map_err(|error| error.to_string())?;
        crate::supervisor::publish_child_exit(&root, &report, None)
            .map_err(|error| error.to_string())
    }
}

/// Failure to boot the production `DefaultPlugins` client from a reopened lock.
#[derive(Debug, Error)]
pub enum ProductionClientError {
    /// Physical world selection, loading or shutdown failed.
    #[error("world persistence failed: {reason}")]
    Persistence {
        /// Actionable persistence diagnostic.
        reason: String,
    },
    /// A selected client resource pack failed validation.
    #[error("client resource pack failed: {reason}")]
    ResourcePack {
        /// Validation diagnostic.
        reason: String,
    },
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
    /// Client-shell graph resolution or start-surface construction failed.
    #[error(transparent)]
    Shell(#[from] ProductionMemoryStartError),
    /// This process already claimed its client App lease.
    #[error(transparent)]
    Lease(#[from] ClientAppLeaseError),
    /// The lock-selected input catalog could not be compiled.
    #[error(transparent)]
    Input(#[from] crate::input::HostInputError),
    /// Local user settings could not be loaded.
    #[error(transparent)]
    Settings(#[from] crate::settings::HostSettingsError),
    /// A supervised child could not persist its one-shot handoff.
    #[error("supervised child handoff failed: {reason}")]
    ChildHandoff {
        /// Diagnostic.
        reason: String,
    },
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

/// Boots the package-driven client from a reopened `latticeaxiom.lock`.
///
/// Ordinary launch reads `latticeaxiom.lock` and `catalog/cas` from the
/// current workspace and freeze-verifies every CAS receipt. Locked capability
/// evidence selects one process role:
///
/// - Exactly one `latticeaxiom:capability/client-shell@1` provider starts one
///   [`bevy::prelude::DefaultPlugins`] shell App from the start-ui semantic
///   tree. Continue/Play of a `ReadyExact` world seals
///   [`latticeaxiom_start_ui::LaunchHandoff::for_ready_exact`] and exits. An
///   external supervisor must spawn the replacement game process; this process
///   does not.
/// - Absence of that capability starts one production game App through
///   [`EngineInstance::new_client_host_from_lock`].
/// - Present evidence with zero or multiple providers fails closed.
///
/// Start-shell and Playing never share one `DefaultPlugins` App. Missing lock
/// or CAS fails closed. This path does not load native modules or open a
/// world writer.
///
/// # Errors
///
/// Returns [`ProductionClientError`] when the lock or CAS is missing, frozen
/// verification fails, reconstructed images do not rebind to the lock, the
/// shell graph is invalid, or the process event loop is already reserved.
pub fn run_client_host_from_lock() -> Result<(), ProductionClientError> {
    let workspace =
        std::env::current_dir().map_err(|source| ProductionClientError::Workspace { source })?;
    run_client_host_from_workspace(&workspace)
}

/// Boots the interactive client from lock and CAS paths under `workspace`.
///
/// Lock-selected `client-shell@1` provider evidence selects the start-shell
/// process; its absence selects the production game process.
///
/// # Errors
///
/// Returns [`ProductionClientError`] when lock or CAS evidence is missing or
/// host construction fails.
pub fn run_client_host_from_workspace(workspace: &Path) -> Result<(), ProductionClientError> {
    let lock_path = std::env::var_os(crate::supervisor::ENV_LOCK)
        .map_or_else(|| workspace.join(PRODUCT_LOCK_FILE_NAME), PathBuf::from);
    let images = load_lock_verified_images_from(workspace, &lock_path)?;
    let epoch = std::env::var(crate::supervisor::ENV_PROCESS_EPOCH)
        .ok()
        .and_then(|value| value.parse().ok())
        .and_then(|value| ProcessEpoch::new(value).ok())
        .unwrap_or(ProcessEpoch::FIRST);
    let lease = claim_fresh_client_app_lease(epoch)?;
    let settings_root = workspace.join("run").join("user");
    let settings = crate::settings::HostUserSettings::load(&settings_root)?;
    let profile = settings.binding_profile().clone();
    let settings_catalog = crate::settings::compile_lock_selected_settings(&images)?;
    let active_lock = images.product_lock_hash();
    let compiled = crate::input::compile_lock_selected_input(&images, &profile)?;
    let selects_shell = ProductionMemoryStart::lock_graph_selects_shell(images.images().graph())?;
    let disk =
        latticeaxiom_world_db::DiskWorldStore::open(&workspace.join("run/worlds/worlds.redb"))
            .map_err(|error| ProductionClientError::Persistence {
                reason: error.to_string(),
            })?;
    let (mut instance, _proof) = if selects_shell {
        let game = load_lock_verified_images(workspace)?;
        let start = ProductionMemoryStart::from_disk_images(&images, game, disk)?
            .with_frozen_lock_catalog(workspace.join("catalog"))?;
        EngineInstance::new_client_shell_with_start(
            images,
            lease,
            latticeaxiom_launcher::SettingTransactionRevision::new(settings.transaction_revision()),
            start,
        )?
    } else {
        let catalog = settings_catalog.ok_or_else(|| {
            crate::settings::HostSettingsError::CatalogUnavailable {
                reason: "the game lock does not select a settings registry".to_owned(),
            }
        })?;
        let maps = compiled
            .as_ref()
            .map(latticeaxiom_player::leafwing_maps_from_catalog);
        let (world, entry, storage, shell_lock) =
            selected_disk_world(workspace, &disk, active_lock)?;
        let (mut instance, proof) = EngineInstance::new_client_host_from_world(
            images,
            lease,
            maps,
            world,
            storage.clone(),
        )?;
        crate::host::persistent::install_disk_session(
            &mut instance,
            workspace,
            disk,
            storage,
            entry,
            shell_lock,
        )?;
        instance.install_user_settings(settings_root, settings, catalog, active_lock)?;
        (instance, proof)
    };
    crate::resource_packs::install_client_resource_packs(&mut instance, workspace)?;
    if selects_shell {
        install_shell_exit_context(&mut instance, workspace, active_lock)?;
    }
    crate::visual_capture::install(&mut instance.app);
    #[cfg(feature = "development")]
    crate::lifecycle_qa::install(&mut instance.app, workspace);
    write_bootstrap_ack(workspace, epoch);
    let role = if selects_shell { "shell" } else { "world" };
    bevy::log::info!(
        target: "latticeaxiom::lifecycle",
        event = "client_host_ready",
        component = "engine",
        process_epoch = epoch.get(),
        role,
        product_lock = %active_lock,
        "lock-verified client host is ready"
    );
    let exit = instance.run();
    bevy::log::info!(
        target: "latticeaxiom::lifecycle",
        event = "client_host_stopped",
        component = "engine",
        process_epoch = epoch.get(),
        role,
        ?exit,
        "client event loop stopped"
    );
    if exit != bevy::app::AppExit::Success {
        return Err(ProductionClientError::Persistence {
            reason:
                "the client did not finish a clean shutdown; inspect the saved-world recovery state"
                    .to_owned(),
        });
    }
    Ok(())
}

fn install_shell_exit_context(
    instance: &mut EngineInstance,
    workspace: &Path,
    active_lock: latticeaxiom_core::CanonicalHash,
) -> Result<(), ProductionClientError> {
    let handoff = instance
        .app
        .world()
        .get_resource::<crate::host::ShellHandoffState>()
        .cloned()
        .ok_or_else(|| ProductionClientError::Persistence {
            reason: "shell shutdown state is missing".to_owned(),
        })?;
    instance.app.insert_resource(ShellExitContext {
        workspace: workspace.to_owned(),
        shell_lock: active_lock,
        handoff,
    });
    Ok(())
}

fn selected_disk_world(
    workspace: &Path,
    disk: &latticeaxiom_world_db::DiskWorldStore,
    active_lock: latticeaxiom_core::CanonicalHash,
) -> Result<
    (
        latticeaxiom_core::WorldId,
        latticeaxiom_world_db::DiskWorldEntryV1,
        latticeaxiom_world_db::DeterministicWorldStorage,
        latticeaxiom_core::CanonicalHash,
    ),
    ProductionClientError,
> {
    let world = std::env::var(crate::supervisor::ENV_WORLD_ID)
        .map_err(|_| ProductionClientError::Persistence {
            reason: "select a saved world through task play before starting a world process"
                .to_owned(),
        })?
        .parse::<latticeaxiom_core::WorldId>()
        .map_err(|error| ProductionClientError::Persistence {
            reason: error.to_string(),
        })?;
    let entry = disk
        .entries()
        .map_err(|error| ProductionClientError::Persistence {
            reason: error.to_string(),
        })?
        .into_iter()
        .find(|entry| entry.world == world)
        .ok_or_else(|| ProductionClientError::Persistence {
            reason: format!("saved world {world} was not found"),
        })?;
    if entry.game_lock != active_lock {
        return Err(ProductionClientError::Persistence {
            reason: "the saved world requires its original gameplay lock".to_owned(),
        });
    }
    let storage = crate::host::persistent::load_disk_world(disk, world)?;
    let shell =
        load_lock_verified_images_from(workspace, &workspace.join("run/shell/latticeaxiom.lock"))?;
    Ok((world, entry, storage, shell.product_lock_hash()))
}

fn write_bootstrap_ack(workspace: &Path, epoch: ProcessEpoch) {
    let Some(root) = std::env::var_os(crate::supervisor::ENV_LAUNCH_ROOT).map(PathBuf::from) else {
        return;
    };
    let path = if root.as_os_str().is_empty() {
        workspace.join("run").join("launcher").join("bootstrap.ack")
    } else {
        root.join("bootstrap.ack")
    };
    let _ = std::fs::write(path, epoch.get().to_string());
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
    load_lock_verified_images_from(workspace, &workspace.join(PRODUCT_LOCK_FILE_NAME))
}

/// Reopens and freeze-verifies `lock_path` against `workspace/catalog/cas`.
///
/// # Errors
///
/// Returns [`ProductionClientError`] when lock or CAS evidence is missing.
pub fn load_lock_verified_images_from(
    workspace: &Path,
    lock_path: &Path,
) -> Result<LockVerifiedComposeImages, ProductionClientError> {
    if !lock_path.is_file() {
        return Err(ProductionClientError::MissingLock {
            path: lock_path.to_path_buf(),
        });
    }
    let cas_root = workspace
        .join(CLIENT_CATALOG_DIRECTORY)
        .join(LOCAL_CATALOG_CAS_DIRECTORY);
    if !cas_root.is_dir() {
        return Err(ProductionClientError::MissingCas { path: cas_root });
    }

    let lock = reopen_product_lock(lock_path)?;
    let host = HostBuildReceipts::sealed_by_lock(&lock)?;
    let store = FilesystemCas::open(&cas_root)?;
    let reopened = ReopenedFinalLockV1::reopen_frozen_from_cas(lock_path, &store, &host)?;
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
    use std::fs;

    struct TestDirectory(tempfile::TempDir);

    impl TestDirectory {
        fn create() -> Self {
            Self(
                tempfile::Builder::new()
                    .prefix("latticeaxiom-engine-client-boot-")
                    .tempdir()
                    .expect("test directory was created"),
            )
        }
    }

    #[test]
    fn missing_lock_fails_closed() {
        let directory = TestDirectory::create();
        match load_lock_verified_images(directory.0.path()) {
            Err(ref error @ ProductionClientError::MissingLock { ref path }) => {
                assert_eq!(path, &directory.0.path().join("latticeaxiom.lock"));
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
        fs::write(directory.0.path().join("latticeaxiom.lock"), b"{}")
            .expect("placeholder lock bytes were written");
        match load_lock_verified_images(directory.0.path()) {
            Err(ref error @ ProductionClientError::MissingCas { ref path }) => {
                assert_eq!(path, &directory.0.path().join("catalog").join("cas"));
                assert_eq!(
                    error.recovery_hint(),
                    Some(ProductionClientError::LOCK_COMMAND)
                );
            }
            other => panic!("expected missing CAS, got {other:?}"),
        }
    }
}
