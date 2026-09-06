//! Opt-in real-process lifecycle exercise, restricted to a marked QA workspace.

use bevy::{
    diagnostic::FrameCount,
    input::{
        ButtonState,
        keyboard::{Key, KeyCode, KeyboardInput},
    },
    prelude::*,
    ui_widgets::{Activate, Button},
};

#[derive(Resource, Debug)]
struct Driver {
    step: u8,
    started: std::time::Instant,
    minimum_world_duration: std::time::Duration,
}

type UiQueries<'w, 's> = (
    Query<'w, 's, (Entity, &'static Name), With<Button>>,
    Query<'w, 's, &'static Text>,
);

/// A fixed identity gives comparison worlds the same UUID-derived seed.
/// This hook only operates in explicitly marked development QA directories.
pub(crate) fn requested_world_id() -> Option<latticeaxiom_core::WorldId> {
    if std::env::var_os("LATTICEAXIOM_LIFECYCLE_QA").is_none()
        || !std::env::current_dir()
            .ok()?
            .join(".latticeaxiom-qa")
            .is_file()
    {
        return None;
    }
    std::env::var("LATTICEAXIOM_QA_WORLD_ID").ok()?.parse().ok()
}

pub(crate) fn install(app: &mut App, workspace: &std::path::Path) {
    if std::env::var_os("LATTICEAXIOM_LIFECYCLE_QA").is_some()
        && workspace.join(".latticeaxiom-qa").is_file()
    {
        let hold_ms = std::env::var("LATTICEAXIOM_LIFECYCLE_HOLD_MS")
            .ok()
            .and_then(|value| value.parse::<u64>().ok())
            .unwrap_or(0)
            .min(60_000);
        app.insert_resource(Driver {
            step: 0,
            started: std::time::Instant::now(),
            minimum_world_duration: std::time::Duration::from_millis(hold_ms),
        })
        .add_systems(Update, drive);
    }
}

#[allow(clippy::needless_pass_by_value)]
#[allow(clippy::too_many_arguments)]
fn drive(
    mut commands: Commands<'_, '_>,
    frame: Res<'_, FrameCount>,
    mut driver: ResMut<'_, Driver>,
    ui: UiQueries<'_, '_>,
    windows: Query<'_, '_, Entity, With<bevy::window::PrimaryWindow>>,
    mut keyboard: MessageWriter<'_, KeyboardInput>,
    mut exits: MessageWriter<'_, AppExit>,
    world_state: (
        Option<Res<'_, crate::ProductionSpine>>,
        Option<Res<'_, crate::VerifiedProductLockHash>>,
    ),
) {
    let (controls, texts) = ui;
    let role = std::env::var(crate::supervisor::ENV_CHILD_ROLE).unwrap_or_default();
    let generation = std::env::var(crate::supervisor::ENV_GENERATION)
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(1);
    if frame.0 == 100 {
        let lines = texts.iter().map(|text| text.0.as_str()).collect::<Vec<_>>();
        if let Ok(bytes) = serde_json::to_vec_pretty(&lines) {
            let _ = std::fs::write(format!("lifecycle-{role}-{generation}.json"), bytes);
        }
        if let (Some(spine), Some(lock)) = world_state {
            let mode = spine.gameplay_mode().map(|mode| match mode {
                latticeaxiom_gameplay::GameplayModeV1::Survival => "survival",
                latticeaxiom_gameplay::GameplayModeV1::Creative => "creative",
            });
            let state = serde_json::json!({
                "world": spine.world_id(), "mode": mode, "lock": lock.get(),
                "inventory_items": spine.inventory_view().map(|view| view.slots().iter().flatten().map(|stack| u64::from(stack.quantity())).sum::<u64>()),
            });
            if let Ok(bytes) = serde_json::to_vec_pretty(&state) {
                let _ = std::fs::write("lifecycle-world-state.json", bytes);
            }
        }
    }
    if role == "shell" {
        // Start with an empty library on the first run and Continue the same
        // saved world on the second run. These are actual native UI actions.
        let desired = if generation == 1 {
            ["Continue", "Create World", "New World"]
                .into_iter()
                .find(|label| {
                    controls
                        .iter()
                        .any(|(_, name)| name.as_str().starts_with(label))
                })
        } else {
            Some("Quit")
        };
        if frame.0 >= 30 + u32::from(driver.step) * 30
            && driver.step < if generation == 1 { 5 } else { 2 }
            && let Some(desired) = desired
            && let Some((entity, _)) = controls
                .iter()
                .find(|(_, name)| name.as_str().starts_with(desired))
        {
            commands.trigger(Activate { entity });
            driver.step += 1;
        }
    } else if role == "world" && driver.started.elapsed() >= driver.minimum_world_duration {
        if frame.0 >= 180
            && driver.step == 0
            && let Ok(window) = windows.single()
        {
            keyboard.write(KeyboardInput {
                key_code: KeyCode::Escape,
                logical_key: Key::Escape,
                state: ButtonState::Pressed,
                text: None,
                repeat: false,
                window,
            });
            driver.step = 1;
        }
        if frame.0 >= 210
            && driver.step == 1
            && let Some((entity, _)) = controls
                .iter()
                .find(|(_, name)| name.as_str() == "Save & Quit")
        {
            commands.trigger(Activate { entity });
            driver.step = 2;
        }
    }
    if driver.started.elapsed() > std::time::Duration::from_mins(2) {
        exits.write(AppExit::Error(std::num::NonZeroU8::MIN));
    }
}
