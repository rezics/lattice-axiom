//! Save completion precedes process exit and supervisor acknowledgement.

use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use bevy::{
    ecs::schedule::{IntoScheduleConfigs, common_conditions::resource_exists},
    prelude::*,
    tasks::{AsyncComputeTaskPool, Task, block_on},
    ui_widgets::{Activate, Button},
};
use latticeaxiom_client_ui::desktop_style as style;
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
    pub(crate) fn shutdown_requested(&self) -> bool {
        self.state.lock().map_or(true, |state| state.requested)
    }

    pub(crate) fn request_save(&self) {
        if let Ok(mut state) = self.state.lock()
            && state.task.is_none()
            && !state.saved
        {
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
        (poll_save, show_save_status).run_if(resource_exists::<PersistentGameSession>),
    )
    .add_observer(retry_save);
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
    world.insert_resource(PersistentGameSession {
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
    });
    Ok(())
}

fn persistence_error(error: impl std::fmt::Display) -> ProductionClientError {
    ProductionClientError::Persistence {
        reason: error.to_string(),
    }
}

fn save(context: &SaveContext, return_to_shell: bool) -> Result<(), String> {
    save_inner(context, return_to_shell).map_err(|error| error.to_string())
}

fn save_inner(context: &SaveContext, return_to_shell: bool) -> Result<(), ProductionClientError> {
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
    if !context.in_process_shell {
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
        pause.set(true);
        let context = session.context.clone();
        let reopen = state.reopen;
        state.reopen = false;
        state.task = Some(AsyncComputeTaskPool::get().spawn(async move {
            if reopen {
                context.disk.reopen().map_err(|error| error.to_string())?;
            }
            save(&context, true)
        }));
    }
    if state.task.as_ref().is_some_and(Task::is_finished) {
        let Some(task) = state.task.take() else {
            return;
        };
        match block_on(task) {
            Ok(()) => {
                state.saved = true;
                if session.context.in_process_shell {
                    commands.insert_resource(ReturnToShell);
                } else {
                    exits.write(AppExit::Success);
                }
            }
            Err(error) => {
                bevy::log::error!(%error, "world save failed");
                state.error = Some(error);
            }
        }
    }
}

#[derive(Component)]
struct SaveOverlay;
#[derive(Component)]
struct SaveMessage;
#[derive(Component)]
struct SaveRetry;

type RetryControls<'w, 's> =
    Query<'w, 's, (Entity, &'static mut Node, &'static mut BorderColor), With<SaveRetry>>;

#[allow(clippy::needless_pass_by_value)]
fn show_save_status(
    mut commands: Commands<'_, '_>,
    session: Res<'_, PersistentGameSession>,
    roots: Query<'_, '_, Entity, With<SaveOverlay>>,
    mut messages: Query<'_, '_, &mut Text, With<SaveMessage>>,
    mut retries: RetryControls<'_, '_>,
    mut focus: ResMut<'_, bevy::input_focus::InputFocus>,
) {
    let Ok(state) = session.state.lock() else {
        return;
    };
    if !state.requested {
        return;
    }
    let message = state.error.as_ref().map_or_else(
        || "Saving your world…".to_owned(),
        |error| format!("Save failed: {error}"),
    );
    if roots.is_empty() {
        commands
            .spawn((
                SaveOverlay,
                bevy::input_focus::tab_navigation::TabGroup::modal(),
                bevy::ui::FocusPolicy::Block,
                GlobalZIndex(10000),
                Node {
                    width: Val::Percent(100.0),
                    height: Val::Percent(100.0),
                    align_items: AlignItems::Center,
                    justify_content: JustifyContent::Center,
                    flex_direction: FlexDirection::Column,
                    row_gap: Val::Px(20.0),
                    ..default()
                },
                BackgroundColor(style::CANVAS),
            ))
            .with_children(|root| {
                root.spawn((
                    SaveMessage,
                    Text::new(message.clone()),
                    crate::ui_font::ui_text_font(24.0),
                    TextColor(style::TEXT),
                    Node {
                        max_width: Val::Percent(80.0),
                        ..default()
                    },
                ));
                root.spawn((
                    SaveRetry,
                    Button,
                    bevy::input_focus::tab_navigation::TabIndex(0),
                    Node {
                        display: Display::None,
                        ..style::button_node()
                    },
                    BackgroundColor(style::RAISED),
                    BorderColor::all(style::BORDER),
                ))
                .with_children(|button| {
                    button.spawn((
                        Text::new("Retry save"),
                        crate::ui_font::ui_text_font(20.0),
                        TextColor(style::TEXT),
                    ));
                });
            });
    }
    for mut text in &mut messages {
        if text.0 != message {
            text.0.clone_from(&message);
        }
    }
    for (entity, mut node, mut border) in &mut retries {
        let display = if state.error.is_some() {
            Display::Flex
        } else {
            Display::None
        };
        if node.display != display {
            node.display = display;
            if display == Display::Flex {
                focus.set(entity, bevy::input_focus::FocusCause::Navigated);
            }
        }
        *border = BorderColor::all(style::focus_border(focus.get() == Some(entity)));
    }
}

#[allow(clippy::needless_pass_by_value)]
fn retry_save(
    event: On<'_, '_, Activate>,
    retries: Query<'_, '_, (), With<SaveRetry>>,
    session: Res<'_, PersistentGameSession>,
) {
    if retries.contains(event.entity) {
        session.request_save();
    }
}
