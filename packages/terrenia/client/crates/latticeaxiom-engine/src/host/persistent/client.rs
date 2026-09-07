//! Save completion precedes process exit and supervisor acknowledgement.

use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use bevy::{
    ecs::schedule::{IntoScheduleConfigs, common_conditions::resource_exists},
    prelude::*,
    tasks::{AsyncComputeTaskPool, Task, block_on},
};
use latticeaxiom_core::{CanonicalHash, canonical_json_hash};
use latticeaxiom_launcher::{
    ChildExitKindV1, ChildExitReportDraftV1, ChildExitReportV1, ChildRoleV1,
    DurableWorldRevisionV1, LaunchAttempt, LaunchGeneration, LaunchIntentDraftV1, LaunchIntentV1,
    LaunchTargetV1, ProcessEpoch, SettingTransactionRevision, WorldRevision as LauncherRevision,
};
use latticeaxiom_world_db::{
    DeterministicWorldStorage, DiskWorldEntryV1, DiskWorldStore, WorldStorage,
};

use crate::host::start::{unix_now_ms, writable_open_plan};
use crate::{EngineInstance, ProductionClientError, ProductionSpine, SealedWorldWriterHost};

#[derive(Clone, Debug)]
struct SaveContext {
    disk: DiskWorldStore,
    storage: DeterministicWorldStorage,
    entry: DiskWorldEntryV1,
    workspace: PathBuf,
    shell_lock: CanonicalHash,
    open_plan: CanonicalHash,
    spine: ProductionSpine,
    in_process_shell: bool,
}

#[derive(Debug, Default)]
struct SaveState {
    requested: bool,
    saved: bool,
    error: Option<String>,
    task: Option<Task<Result<(), String>>>,
    reopen: bool,
    return_to_shell: bool,
    paused_before_save: bool,
    checkpoints: u64,
}

/// Physical world session retained through window-runner teardown.
#[derive(Clone, Debug, Resource)]
pub(crate) struct PersistentGameSession {
    context: SaveContext,
    state: Arc<Mutex<SaveState>>,
}

/// Requests returning to the in-process start shell after Save & Quit.
#[derive(Clone, Copy, Debug, Default, Resource)]
pub(crate) struct ReturnToShell;

/// Marks that this App owns the start shell and must not spawn a replacement window.
#[derive(Clone, Copy, Debug, Default, Resource)]
pub(crate) struct InProcessShellPlay;

impl PersistentGameSession {
    pub(crate) fn web_status(&self) -> serde_json::Value {
        match self.state.lock() {
            Ok(state) => {
                serde_json::json!({"requested":state.requested,"saved":state.saved,"error":state.error,"returnToShell":state.return_to_shell,"checkpoints":state.checkpoints})
            }
            Err(_) => {
                serde_json::json!({"requested":true,"saved":false,"error":"Save state unavailable"})
            }
        }
    }

    pub(crate) fn shutdown_requested(&self) -> bool {
        self.state.lock().map_or(true, |state| state.requested)
    }

    pub(crate) fn request_save(&self) {
        if let Ok(mut state) = self.state.lock()
            && state.task.is_none()
            && !state.saved
        {
            state.requested = true;
            state.return_to_shell = true;
            state.reopen = state.error.is_some();
            state.error = None;
        }
    }

    /// Save without leaving the active world. A retry retains its original goal.
    pub(crate) fn request_checkpoint(&self) {
        if let Ok(mut state) = self.state.lock()
            && state.task.is_none()
            && !state.saved
        {
            if !state.requested {
                state.return_to_shell = false;
            }
            state.requested = true;
            state.reopen = state.error.is_some();
            state.error = None;
        }
    }

    pub(crate) fn finish(&self) -> Result<(), String> {
        let (task, requested) = {
            let mut state = self.state.lock().map_err(|error| error.to_string())?;
            if state.saved {
                return Ok(());
            }
            if std::env::var_os("LATTICEAXIOM_CAPTURE_PATH").is_some()
                && !state.requested
                && state.task.is_none()
            {
                return Ok(());
            }
            (state.task.take(), state.requested)
        };
        match task {
            Some(task) => block_on(task),
            None => save(&self.context, requested),
        }?;
        self.state.lock().map_err(|error| error.to_string())?.saved = true;
        Ok(())
    }
}

pub(in crate::host::persistent) fn add_save_systems(app: &mut bevy::prelude::App) {
    app.add_systems(
        bevy::prelude::Update,
        poll_save.run_if(resource_exists::<PersistentGameSession>),
    );
}

pub(crate) fn install_disk_session(
    instance: &mut EngineInstance,
    workspace: &Path,
    disk: DiskWorldStore,
    storage: DeterministicWorldStorage,
    entry: DiskWorldEntryV1,
    shell_lock: CanonicalHash,
) -> Result<(), ProductionClientError> {
    let spine = instance
        .app
        .world()
        .get_resource::<ProductionSpine>()
        .cloned()
        .ok_or_else(|| persistence_error("world spine was not installed"))?;
    let in_process_shell = instance
        .app
        .world()
        .get_resource::<InProcessShellPlay>()
        .is_some();
    insert_disk_session_on_world(
        instance.app.world_mut(),
        workspace,
        disk,
        storage,
        entry,
        shell_lock,
        spine,
        in_process_shell,
    )
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn insert_disk_session_on_world(
    world: &mut World,
    workspace: &Path,
    disk: DiskWorldStore,
    storage: DeterministicWorldStorage,
    entry: DiskWorldEntryV1,
    shell_lock: CanonicalHash,
    spine: crate::ProductionSpine,
    in_process_shell: bool,
) -> Result<(), ProductionClientError> {
    let preflight = storage.preflight(entry.world).map_err(persistence_error)?;
    let permit = preflight
        .activation_permit()
        .ok_or_else(|| persistence_error("saved world is not ready for writer activation"))?;
    let plan = writable_open_plan(entry.world, permit);
    let open_plan = canonical_json_hash(&plan).map_err(persistence_error)?;
    let session = PersistentGameSession {
        context: SaveContext {
            disk,
            storage,
            entry,
            workspace: workspace.to_owned(),
            shell_lock,
            open_plan,
            spine,
            in_process_shell,
        },
        state: Arc::new(Mutex::new(SaveState::default())),
    };
    if let Some(shell) = world.get_resource::<crate::client::ShellExitContext>() {
        shell
            .set_active_session(Some(session.clone()))
            .map_err(persistence_error)?;
    }
    world.insert_resource(session);
    Ok(())
}

fn persistence_error(error: impl std::fmt::Display) -> ProductionClientError {
    ProductionClientError::Persistence {
        reason: error.to_string(),
    }
}

fn save(context: &SaveContext, return_to_shell: bool) -> Result<(), String> {
    save_inner(context, return_to_shell, true).map_err(|error| error.to_string())
}

fn save_inner(
    context: &SaveContext,
    return_to_shell: bool,
    publish_report: bool,
) -> Result<(), ProductionClientError> {
    let mut writer = SealedWorldWriterHost::new(context.storage.clone());
    let preflight = writer
        .preflight(context.entry.world)
        .map_err(persistence_error)?;
    let permit = preflight
        .activation_permit()
        .cloned()
        .ok_or_else(|| persistence_error("writer activation is unavailable"))?;
    writer
        .reactivate(&writable_open_plan(context.entry.world, &permit), permit)
        .map_err(persistence_error)?;
    context
        .spine
        .flush_dirty_chunks(&mut writer, preflight.metadata())
        .map_err(persistence_error)?;
    writer.flush_durable().map_err(persistence_error)?;
    let frontier = writer
        .begin_read(context.entry.world)
        .map_err(persistence_error)?
        .frontier();
    writer.close().map_err(persistence_error)?;
    let mut entry = context.entry.clone();
    entry.last_played_at_ms = unix_now_ms();
    context
        .disk
        .publish(&context.storage, &entry)
        .map_err(persistence_error)?;
    #[cfg(feature = "development")]
    if std::env::var_os("LATTICEAXIOM_LIFECYCLE_QA").is_some()
        && context.workspace.join(".latticeaxiom-qa").is_file()
    {
        // This probe is emitted only after the actual disk publication succeeds.
        // It does not change the launcher's shell-role or world-role contracts.
        let revision = frontier.durable().get();
        let mut proof = serde_json::json!({
            "schema":"latticeaxiom.in-process-durable-world.v1",
            "world_id":context.entry.world,"revision":revision,"written_revision":frontier.written().get(),
            "world_lock_hash":context.entry.game_lock,"return_to_shell":return_to_shell,
            "nonce":std::env::var("LATTICEAXIOM_LIFECYCLE_NONCE").unwrap_or_default()
        });
        proof["checksum"] =
            serde_json::json!(canonical_json_hash(&proof).map_err(persistence_error)?);
        let bytes = serde_json::to_vec(&proof).map_err(persistence_error)?;
        std::fs::write(
            context.workspace.join("lifecycle-durable-world.json"),
            bytes,
        )
        .map_err(persistence_error)?;
    }
    if publish_report && !context.in_process_shell {
        publish_exit(context, frontier.durable().get(), return_to_shell)?;
    }
    Ok(())
}

fn publish_exit(
    context: &SaveContext,
    revision: u64,
    return_to_shell: bool,
) -> Result<(), ProductionClientError> {
    let Some(root) = std::env::var_os(crate::supervisor::ENV_LAUNCH_ROOT).map(PathBuf::from) else {
        return Ok(());
    };
    let generation = std::env::var(crate::supervisor::ENV_GENERATION)
        .ok()
        .and_then(|value| value.parse().ok())
        .and_then(|value| LaunchGeneration::new(value).ok())
        .unwrap_or(LaunchGeneration::FIRST);
    let epoch = std::env::var(crate::supervisor::ENV_PROCESS_EPOCH)
        .ok()
        .and_then(|value| value.parse().ok())
        .and_then(|value| ProcessEpoch::new(value).ok())
        .unwrap_or(ProcessEpoch::FIRST);
    let settings = crate::settings::HostUserSettings::load(context.workspace.join("run/user"))?;
    let settings_revision = SettingTransactionRevision::new(settings.transaction_revision());
    let now = unix_now_ms();
    let intent = if return_to_shell {
        let candidate = LaunchIntentV1::seal(LaunchIntentDraftV1 {
            generation: generation.next().map_err(persistence_error)?,
            attempt: LaunchAttempt::FIRST,
            issued_at_ms: now,
            expires_at_ms: now.saturating_add(60_000),
            target: LaunchTargetV1::Shell,
            shell_lock_hash: context.shell_lock,
            world_lock_hash: None,
            world_open_plan_hash: None,
            confirmed_setting_transaction_revision: settings_revision,
        })
        .map_err(persistence_error)?;
        Some(
            crate::supervisor::persist_supervised_intent(&root, &candidate, generation)
                .map_err(persistence_error)?,
        )
    } else {
        None
    };
    let durable = DurableWorldRevisionV1::new(context.entry.world, LauncherRevision::new(revision));
    let report = ChildExitReportV1::seal(ChildExitReportDraftV1 {
        child_generation: generation,
        process_epoch: epoch,
        role: ChildRoleV1::World {
            world_id: context.entry.world,
        },
        exit_kind: if return_to_shell {
            ChildExitKindV1::SaveAndQuit
        } else {
            ChildExitKindV1::OsClose
        },
        intent_generation: intent.as_ref().map(LaunchIntentV1::generation),
        intent_checksum: intent.as_ref().map(LaunchIntentV1::checksum),
        confirmed_setting_transaction_revision: settings_revision,
        last_written_world: Some(durable),
        last_durable_world: Some(durable),
        shell_lock_hash: context.shell_lock,
        world_lock_hash: Some(context.entry.game_lock),
        world_open_plan_hash: Some(context.open_plan),
        diagnostic_ref: None,
    })
    .map_err(persistence_error)?;
    crate::supervisor::publish_child_exit(&root, &report, intent.as_ref())
        .map_err(persistence_error)
}

#[allow(clippy::needless_pass_by_value)]
fn poll_save(
    session: Res<'_, PersistentGameSession>,
    mut pause: ResMut<'_, crate::ProductionSessionPause>,
    mut exits: MessageWriter<'_, AppExit>,
    mut commands: Commands<'_, '_>,
) {
    let Ok(mut state) = session.state.lock() else {
        return;
    };
    if state.requested && state.task.is_none() && !state.saved && state.error.is_none() {
        state.paused_before_save = pause.is_paused();
        pause.set(true);
        let context = session.context.clone();
        let reopen = state.reopen;
        let return_to_shell = state.return_to_shell;
        state.reopen = false;
        state.task = Some(AsyncComputeTaskPool::get().spawn(async move {
            if reopen {
                context.disk.reopen().map_err(|error| error.to_string())?;
            }
            save_inner(&context, return_to_shell, return_to_shell)
                .map_err(|error| error.to_string())
        }));
    }
    if state.task.as_ref().is_some_and(Task::is_finished) {
        let Some(task) = state.task.take() else {
            return;
        };
        match block_on(task) {
            Ok(()) => {
                if state.return_to_shell {
                    state.saved = true;
                    if session.context.in_process_shell {
                        commands.insert_resource(ReturnToShell);
                    } else {
                        exits.write(AppExit::Success);
                    }
                } else {
                    state.requested = false;
                    state.checkpoints = state.checkpoints.saturating_add(1);
                    pause.set(state.paused_before_save);
                }
            }
            Err(error) => {
                bevy::log::error!(%error, "world save failed");
                state.error = Some(error);
            }
        }
    }
}
