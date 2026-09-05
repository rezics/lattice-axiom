//! Non-Bevy product supervisor adapter for `task play`.

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use latticeaxiom_core::{CanonicalHash, WorldId};
use latticeaxiom_launcher::{
    AtomicChildExitStore, AtomicLaunchIntentStore, BOOTSTRAP_ACK_SCHEMA_VERSION, BootObservationV1,
    BootstrapAckV1, BootstrapSafeStateV1, ChildObservationV1, FileChildExitStore,
    FileLaunchIntentStore, LaunchIntentV1, LaunchTargetV1, PriorChildStatusV1, ProcessControl,
    ProcessEpoch, ProcessLaunchRequestV1, ProcessSupervisorIdentityV1, RecoveryBootstrapAckV1,
    RecoveryChildStatusV1, RecoveryLaunchRequestV1, SettingTransactionRevision, SpawnFailureV1,
    SpawnedProcess, SupervisorConfigV1, SupervisorMachine, SupervisorReportV1,
    TerminationFailureV1,
};
use thiserror::Error;

use crate::client::{ProductionClientError, load_lock_verified_images};

#[cfg(test)]
mod publication_tests;

/// Environment variable selecting the supervised child role.
pub const ENV_CHILD_ROLE: &str = "LATTICEAXIOM_CHILD_ROLE";
/// Environment variable naming the lock the child reopens.
pub const ENV_LOCK: &str = "LATTICEAXIOM_LOCK";
/// Environment variable naming the confined launcher store root.
pub const ENV_LAUNCH_ROOT: &str = "LATTICEAXIOM_LAUNCH_ROOT";
/// Environment variable carrying the child's process epoch.
pub const ENV_PROCESS_EPOCH: &str = "LATTICEAXIOM_PROCESS_EPOCH";
/// Environment variable carrying the launch generation.
pub const ENV_GENERATION: &str = "LATTICEAXIOM_GENERATION";
/// Environment variable carrying the world identity for a world child.
pub const ENV_WORLD_ID: &str = "LATTICEAXIOM_WORLD_ID";

/// Failure to boot the product supervisor.
#[derive(Debug, Error)]
pub enum ProductSupervisorError {
    /// The workspace could not be resolved.
    #[error("workspace directory is unavailable: {source}")]
    Workspace {
        /// Underlying error.
        #[source]
        source: io::Error,
    },
    /// A required lock or catalog could not be reopened.
    #[error(transparent)]
    Client(#[from] ProductionClientError),
    /// The launcher store could not be opened.
    #[error(transparent)]
    Store(#[from] latticeaxiom_launcher::IntentStoreError),
    /// The durable user-settings journal could not be reopened.
    #[error(transparent)]
    Settings(#[from] crate::settings::HostSettingsError),
    /// The engine child executable is missing.
    #[error("supervised engine executable is missing at {path}")]
    MissingEngine {
        /// Expected executable.
        path: PathBuf,
    },
}

/// Runs the non-Bevy product supervisor until it exits or halts.
///
/// The supervisor never constructs a Bevy `App`. Children reopen the selected
/// shell or game lock and publish one-shot intents plus child-exit reports.
///
/// # Errors
///
/// Returns [`ProductSupervisorError`] when locks, CAS, the durable settings
/// journal, or the launcher store cannot be opened.
pub fn run_product_supervisor_from_workspace(
    workspace: &Path,
) -> Result<SupervisorReportV1, ProductSupervisorError> {
    let shell_lock = workspace
        .join("run")
        .join("shell")
        .join("latticeaxiom.lock");
    let game_lock = workspace.join("latticeaxiom.lock");
    let shell_images = load_lock_verified_images_at(workspace, &shell_lock)?;
    let _game_images = load_lock_verified_images_at(workspace, &game_lock)?;
    let launch_root = ensure_launch_root(workspace)?;
    let mut intent_store = FileLaunchIntentStore::open(&launch_root)?;
    let mut exit_store = FileChildExitStore::open(&launch_root)?;
    let now_ms = unix_now_ms();
    let settings_revision = load_confirmed_settings_revision(workspace)?;
    let mut supervisor = SupervisorMachine::new(SupervisorConfigV1::new(
        shell_images.product_lock_hash(),
        now_ms,
        settings_revision,
    ));
    let mut process = OsProcessControl::new(workspace, &launch_root, &shell_lock, &game_lock)?;
    let report = supervisor.run(&mut intent_store, &mut exit_store, &mut process);
    drop((intent_store, exit_store, process));
    retire_completed_run(workspace, &launch_root, &report)?;
    Ok(report)
}

fn retire_completed_run(
    workspace: &Path,
    root: &Path,
    report: &SupervisorReportV1,
) -> Result<(), ProductSupervisorError> {
    if !matches!(
        report.outcome(),
        latticeaxiom_launcher::SupervisorOutcomeV1::ProductExited { .. }
    ) {
        return Ok(());
    }
    let history = workspace.join("run/launcher-history");
    fs::create_dir_all(&history).map_err(|source| ProductSupervisorError::Workspace { source })?;
    let bytes = serde_json::to_vec_pretty(report).map_err(|error| {
        ProductSupervisorError::Client(ProductionClientError::ChildHandoff {
            reason: error.to_string(),
        })
    })?;
    fs::write(root.join("supervisor-result.json"), bytes)
        .map_err(|source| ProductSupervisorError::Workspace { source })?;
    fs::rename(root, history.join(WorldId::new_v4().to_string()))
        .map_err(|source| ProductSupervisorError::Workspace { source })
}

fn load_confirmed_settings_revision(
    workspace: &Path,
) -> Result<SettingTransactionRevision, crate::settings::HostSettingsError> {
    let settings = crate::settings::HostUserSettings::load(workspace.join("run").join("user"))?;
    Ok(SettingTransactionRevision::new(
        settings.transaction_revision(),
    ))
}

fn load_lock_verified_images_at(
    workspace: &Path,
    lock_path: &Path,
) -> Result<crate::LockVerifiedComposeImages, ProductSupervisorError> {
    if lock_path == workspace.join("latticeaxiom.lock") {
        return Ok(load_lock_verified_images(workspace)?);
    }
    load_named_lock(workspace, lock_path).map_err(ProductSupervisorError::from)
}

fn load_named_lock(
    workspace: &Path,
    lock_path: &Path,
) -> Result<crate::LockVerifiedComposeImages, ProductionClientError> {
    crate::client::load_lock_verified_images_from(workspace, lock_path)
}

fn ensure_launch_root(workspace: &Path) -> Result<PathBuf, ProductSupervisorError> {
    let root = workspace.join("run").join("launcher");
    fs::create_dir_all(&root).map_err(|source| ProductSupervisorError::Workspace { source })?;
    fs::canonicalize(&root).map_err(|source| ProductSupervisorError::Workspace { source })
}

fn unix_now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|duration| u64::try_from(duration.as_millis()).ok())
        .unwrap_or(0)
}

#[derive(Debug)]
struct OsProcessControl {
    identity: ProcessSupervisorIdentityV1,
    engine: PathBuf,
    workspace: PathBuf,
    launch_root: PathBuf,
    shell_lock: PathBuf,
    game_lock: PathBuf,
    children: BTreeMap<u64, Child>,
    next_handle: u64,
}

impl OsProcessControl {
    fn new(
        workspace: &Path,
        launch_root: &Path,
        shell_lock: &Path,
        game_lock: &Path,
    ) -> Result<Self, ProductSupervisorError> {
        let engine = sibling_engine_executable()?;
        if !engine.is_file() {
            return Err(ProductSupervisorError::MissingEngine { path: engine });
        }
        Ok(Self {
            identity: ProcessSupervisorIdentityV1::new(CanonicalHash::digest(
                b"latticeaxiom-play-supervisor",
            )),
            engine,
            workspace: workspace.to_path_buf(),
            launch_root: launch_root.to_path_buf(),
            shell_lock: shell_lock.to_path_buf(),
            game_lock: game_lock.to_path_buf(),
            children: BTreeMap::new(),
            next_handle: 1,
        })
    }

    fn spawn_role(
        &mut self,
        role: &str,
        lock: &Path,
        epoch: ProcessEpoch,
        generation: u64,
        world_id: Option<WorldId>,
    ) -> Result<SpawnedProcess, SpawnFailureV1> {
        let handle = self.next_handle;
        self.next_handle = self.next_handle.saturating_add(1);
        let process =
            SpawnedProcess::new(handle).map_err(|_| SpawnFailureV1::ResourceUnavailable)?;
        let mut command = Command::new(&self.engine);
        command
            .current_dir(&self.workspace)
            .env(ENV_CHILD_ROLE, role)
            .env(ENV_LOCK, lock)
            .env(ENV_LAUNCH_ROOT, &self.launch_root)
            .env(ENV_PROCESS_EPOCH, epoch.get().to_string())
            .env(ENV_GENERATION, generation.to_string())
            .stdin(Stdio::null())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit());
        if let Some(world_id) = world_id {
            command.env(ENV_WORLD_ID, world_id.to_string());
        }
        let _ = fs::remove_file(self.launch_root.join("bootstrap.ack"));
        let child = command
            .spawn()
            .map_err(|_| SpawnFailureV1::ResourceUnavailable)?;
        self.children.insert(handle, child);
        Ok(process)
    }
}

impl ProcessControl for OsProcessControl {
    fn current_time_ms(&self) -> Option<u64> {
        Some(unix_now_ms())
    }

    fn supervisor_identity(&self) -> ProcessSupervisorIdentityV1 {
        self.identity
    }

    fn reconcile_prior_child(&mut self, _intent: &LaunchIntentV1) -> PriorChildStatusV1 {
        PriorChildStatusV1::Unknown
    }

    fn spawn(
        &mut self,
        request: &ProcessLaunchRequestV1,
    ) -> Result<SpawnedProcess, SpawnFailureV1> {
        match request {
            ProcessLaunchRequestV1::InitialShell { generation, .. } => {
                let lock = self.shell_lock.clone();
                self.spawn_role("shell", &lock, ProcessEpoch::FIRST, generation.get(), None)
            }
            ProcessLaunchRequestV1::Intent(intent) => match intent.target() {
                LaunchTargetV1::Shell => {
                    let epoch = ProcessEpoch::new(intent.generation().get())
                        .map_err(|_| SpawnFailureV1::PolicyDenied)?;
                    let lock = self.shell_lock.clone();
                    self.spawn_role("shell", &lock, epoch, intent.generation().get(), None)
                }
                LaunchTargetV1::World { world_id } => {
                    let epoch = ProcessEpoch::new(intent.generation().get())
                        .map_err(|_| SpawnFailureV1::PolicyDenied)?;
                    let lock = self.game_lock.clone();
                    self.spawn_role(
                        "world",
                        &lock,
                        epoch,
                        intent.generation().get(),
                        Some(world_id),
                    )
                }
            },
            ProcessLaunchRequestV1::RecoveryShell(request) => {
                let lock = self.shell_lock.clone();
                self.spawn_role(
                    "recovery",
                    &lock,
                    ProcessEpoch::FIRST,
                    request.recovery_generation().get(),
                    None,
                )
            }
        }
    }

    fn acquire_recovery(
        &mut self,
        request: &RecoveryLaunchRequestV1,
    ) -> Result<RecoveryChildStatusV1, SpawnFailureV1> {
        let spawned = self.spawn(&ProcessLaunchRequestV1::RecoveryShell(request.clone()))?;
        Ok(RecoveryChildStatusV1::Acquired(spawned))
    }

    fn await_bootstrap(
        &mut self,
        process: SpawnedProcess,
        request: &ProcessLaunchRequestV1,
        deadline_ms: u64,
    ) -> BootObservationV1 {
        let deadline = Instant::now() + Duration::from_millis(deadline_ms);
        let ack = self.launch_root.join("bootstrap.ack");
        while Instant::now() < deadline {
            if ack.is_file() {
                let epoch = fs::read_to_string(&ack)
                    .ok()
                    .and_then(|text| text.trim().parse().ok())
                    .and_then(|value| ProcessEpoch::new(value).ok())
                    .unwrap_or(ProcessEpoch::FIRST);
                return match request {
                    ProcessLaunchRequestV1::InitialShell { .. } => {
                        BootObservationV1::InitialShellAcknowledged {
                            process_epoch: epoch,
                        }
                    }
                    ProcessLaunchRequestV1::RecoveryShell(recovery) => {
                        match recovery_ack(recovery, epoch) {
                            Some(ack) => BootObservationV1::RecoveryAcknowledged(ack),
                            None => BootObservationV1::BootFailed,
                        }
                    }
                    ProcessLaunchRequestV1::Intent(intent) => match intent_ack(intent, epoch) {
                        Some(ack) => BootObservationV1::Acknowledged(ack),
                        None => BootObservationV1::BootFailed,
                    },
                };
            }
            if let Some(child) = self.children.get_mut(&process.get())
                && child.try_wait().ok().flatten().is_some()
            {
                return BootObservationV1::Crashed { exit_code: None };
            }
            thread::sleep(Duration::from_millis(20));
        }
        BootObservationV1::TimedOut
    }

    fn terminate(&mut self, process: SpawnedProcess) -> Result<(), TerminationFailureV1> {
        let Some(mut child) = self.children.remove(&process.get()) else {
            return Err(TerminationFailureV1::UnknownProcess);
        };
        child
            .kill()
            .map_err(|_| TerminationFailureV1::PlatformRejected)?;
        let _ = child.wait();
        Ok(())
    }

    fn await_exit(&mut self, process: SpawnedProcess, deadline_ms: u64) -> ChildObservationV1 {
        let deadline = Instant::now() + Duration::from_millis(deadline_ms);
        loop {
            match self.children.get_mut(&process.get()) {
                Some(child) => match child.try_wait() {
                    Ok(Some(status)) => {
                        self.children.remove(&process.get());
                        return ChildObservationV1::Exited {
                            exit_code: status.code(),
                        };
                    }
                    Ok(None) if Instant::now() >= deadline => return ChildObservationV1::Running,
                    Ok(None) => thread::sleep(Duration::from_millis(50)),
                    Err(_) => return ChildObservationV1::Unknown,
                },
                None => return ChildObservationV1::Unknown,
            }
        }
    }
}

fn sibling_engine_executable() -> Result<PathBuf, ProductSupervisorError> {
    let current =
        std::env::current_exe().map_err(|source| ProductSupervisorError::Workspace { source })?;
    let mut engine = current.clone();
    engine.set_file_name("latticeaxiom-engine");
    #[cfg(windows)]
    engine.set_extension("exe");
    Ok(engine)
}

fn intent_ack(intent: &LaunchIntentV1, process_epoch: ProcessEpoch) -> Option<BootstrapAckV1> {
    let safe_state = match intent.target() {
        LaunchTargetV1::Shell => BootstrapSafeStateV1::ShellReady,
        LaunchTargetV1::World { .. } => BootstrapSafeStateV1::WorldReady,
    };
    let wire = serde_json::json!({
        "schema_version": BOOTSTRAP_ACK_SCHEMA_VERSION,
        "generation": intent.generation(),
        "attempt": intent.attempt(),
        "target": intent.target(),
        "intent_checksum": intent.checksum(),
        "process_epoch": process_epoch,
        "safe_state": safe_state,
    });
    serde_json::from_value(wire).ok()
}

/// Publishes a one-shot child-exit report, and optional intent, into the launch root.
///
/// # Errors
///
/// Returns [`ProductSupervisorError`] when the confined store rejects the write.
pub fn publish_child_exit(
    launch_root: &Path,
    report: &latticeaxiom_launcher::ChildExitReportV1,
    intent: Option<&LaunchIntentV1>,
) -> Result<(), ProductSupervisorError> {
    if let Some(intent) = intent {
        let mut intent_store = FileLaunchIntentStore::open(launch_root)?;
        let bytes = intent.canonical_bytes().map_err(|error| {
            ProductSupervisorError::Client(ProductionClientError::ChildHandoff {
                reason: error.to_string(),
            })
        })?;
        let slot = intent_store.read()?;
        match slot {
            latticeaxiom_launcher::IntentSlot::Occupied {
                disposition,
                bytes: previous_bytes,
                blob_hash,
                ..
            } if disposition.is_terminal() && previous_bytes != bytes => {
                let previous =
                    LaunchIntentV1::authenticate_at_rest(&previous_bytes).map_err(|error| {
                        ProductSupervisorError::Client(ProductionClientError::ChildHandoff {
                            reason: error.to_string(),
                        })
                    })?;
                if previous.generation() != report.child_generation()
                    || previous.shell_lock_hash() != report.shell_lock_hash()
                {
                    return Err(ProductSupervisorError::Client(
                        ProductionClientError::ChildHandoff {
                            reason: "terminal intent does not belong to the exiting child"
                                .to_owned(),
                        },
                    ));
                }
                intent_store.publish_replacing_terminal(blob_hash, &bytes)?;
            }
            _ => {
                intent_store.publish(&bytes)?;
            }
        }
    }
    let mut exit_store = FileChildExitStore::open(launch_root)?;
    let bytes = report.canonical_bytes().map_err(|error| {
        ProductSupervisorError::Client(ProductionClientError::ChildHandoff {
            reason: error.to_string(),
        })
    })?;
    exit_store
        .publish(&bytes)
        .map_err(ProductSupervisorError::Store)?;
    Ok(())
}

fn recovery_ack(
    request: &RecoveryLaunchRequestV1,
    process_epoch: ProcessEpoch,
) -> Option<RecoveryBootstrapAckV1> {
    let wire = serde_json::json!({
        "schema_version": latticeaxiom_launcher::RECOVERY_ACK_SCHEMA_VERSION,
        "request_checksum": request.checksum(),
        "process_epoch": process_epoch,
        "safe_state": BootstrapSafeStateV1::RecoveryReady,
    });
    serde_json::from_value(wire).ok()
}

#[cfg(test)]
mod tests {
    use latticeaxiom_core::StableId;
    use latticeaxiom_input::BindingProfileV1;

    use super::load_confirmed_settings_revision;
    use crate::settings::HostUserSettings;

    #[test]
    fn supervisor_reopens_the_persisted_settings_revision() {
        let workspace = TestDirectory::create();
        let settings_root = workspace.path().join("run").join("user");
        let mut settings = HostUserSettings::load(&settings_root)
            .unwrap_or_else(|error| panic!("empty settings load: {error}"));
        let mut profile = BindingProfileV1::empty();
        let action = "example:action/supervisor-revision"
            .parse::<StableId>()
            .unwrap_or_else(|error| panic!("fixture action is canonical: {error}"));
        profile.set_override(action, Vec::new());
        settings
            .persist_binding_profile(&settings_root, profile)
            .unwrap_or_else(|error| panic!("settings fixture persists: {error}"));

        let loaded = load_confirmed_settings_revision(workspace.path())
            .unwrap_or_else(|error| panic!("supervisor settings reopen: {error}"));
        assert_eq!(loaded.get(), settings.transaction_revision());
        assert_eq!(loaded.get(), 1);
    }

    struct TestDirectory(tempfile::TempDir);

    impl TestDirectory {
        fn create() -> Self {
            Self(
                tempfile::Builder::new()
                    .prefix("latticeaxiom-engine-supervisor-settings-")
                    .tempdir()
                    .unwrap_or_else(|error| panic!("supervisor test directory: {error}")),
            )
        }

        fn path(&self) -> &std::path::Path {
            self.0.path()
        }
    }
}
