//! Opt-in domain lifecycle exercise in a marked QA workspace.
//!
//! Evidence covers domain commands and persistence, not DOM or WebView rendering.
use bevy::prelude::*;
use serde_json::{Value, json};
use std::{
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Phase {
    Shell,
    AwaitGame,
    Playing,
    AwaitShell,
    Finished,
}
#[derive(Resource, Debug)]
struct Driver {
    phase: Phase,
    started: Instant,
    entered_world: Option<Instant>,
    next_action: Instant,
    minimum_world_duration: Duration,
    evidence_path: PathBuf,
    events: Vec<Value>,
}

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
pub(crate) fn install(app: &mut App, workspace: &Path) {
    if std::env::var_os("LATTICEAXIOM_LIFECYCLE_QA").is_none()
        || !workspace.join(".latticeaxiom-qa").is_file()
    {
        return;
    }
    let hold_ms = std::env::var("LATTICEAXIOM_LIFECYCLE_HOLD_MS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(0)
        .clamp(1000, 60_000);
    let now = Instant::now();
    app.insert_resource(Driver {
        phase: Phase::Shell,
        started: now,
        entered_world: None,
        next_action: now + Duration::from_millis(500),
        minimum_world_duration: Duration::from_millis(hold_ms),
        evidence_path: workspace.join("lifecycle-domain-evidence.json"),
        events: Vec::new(),
    })
    .add_systems(Update, drive);
}
fn drive(world: &mut World) {
    let Some(mut driver) = world.remove_resource::<Driver>() else {
        return;
    };
    if driver.phase == Phase::Finished {
        world.insert_resource(driver);
        return;
    }
    let snapshot = crate::host::web::qa_snapshot(world);
    if driver.started.elapsed() > Duration::from_secs(120) {
        fail(
            world,
            &mut driver,
            &snapshot,
            "Lifecycle exercise timed out".to_owned(),
        );
        world.insert_resource(driver);
        return;
    }
    let playing = snapshot["mode"].as_str() == Some("game");
    if playing && matches!(driver.phase, Phase::Shell | Phase::AwaitGame) {
        driver.phase = Phase::Playing;
        driver.entered_world = Some(Instant::now());
        driver.record("world-entered", evidence(world, &snapshot));
        let state = legacy_world_state(world);
        let path = driver
            .evidence_path
            .with_file_name("lifecycle-world-state.json");
        if let Err(error) = serde_json::to_vec_pretty(&state)
            .map_err(|error| error.to_string())
            .and_then(|bytes| std::fs::write(path, bytes).map_err(|error| error.to_string()))
        {
            fail(
                world,
                &mut driver,
                &snapshot,
                format!("World-state evidence could not be written: {error}"),
            );
            world.insert_resource(driver);
            return;
        }
    }
    if driver.phase == Phase::AwaitShell && !playing && snapshot["mode"].as_str() == Some("shell") {
        driver.phase = Phase::Finished;
        driver.record("returned-to-shell", evidence(world, &snapshot));
        match crate::host::web::qa_command(world, "app.quit", &json!({})) {
            Ok(()) => driver.record("completed", json!({"command":"app.quit"})),
            Err(error) => fail(world, &mut driver, &snapshot, error),
        }
        world.insert_resource(driver);
        return;
    }
    if Instant::now() < driver.next_action {
        world.insert_resource(driver);
        return;
    }
    let command = match driver.phase {
        Phase::Shell if !playing => shell_command(&snapshot),
        Phase::Playing
            if driver
                .entered_world
                .is_some_and(|entered| entered.elapsed() >= driver.minimum_world_duration) =>
        {
            Some(("game.exit", json!({})))
        }
        _ => None,
    };
    if let Some((method, params)) = command {
        match crate::host::web::qa_command(world, method, &params) {
            Ok(()) => {
                driver.record(
                    "command-accepted",
                    json!({"method":method,"params":params,"state":evidence(world,&snapshot)}),
                );
                if method == "game.exit" {
                    driver.phase = Phase::AwaitShell;
                } else if params["action"].as_str() == Some("continue-world") {
                    driver.phase = Phase::AwaitGame;
                }
            }
            Err(error) => fail(world, &mut driver, &snapshot, error),
        }
    }
    driver.next_action = Instant::now() + Duration::from_millis(250);
    world.insert_resource(driver);
}
fn shell_command(snapshot: &Value) -> Option<(&'static str, Value)> {
    ["continue-world", "quick-create"]
        .into_iter()
        .find_map(|action| {
            semantic_target(&snapshot["shell"]["tree"], action)
                .map(|target| ("shell.action", json!({"target":target,"action":action})))
        })
}
fn semantic_target<'a>(node: &'a Value, action: &str) -> Option<&'a str> {
    if node["state"]["disabled"].as_bool() != Some(true)
        && node["actions"]
            .as_array()
            .is_some_and(|actions| actions.iter().any(|value| value.as_str() == Some(action)))
        && let Some(id) = node["id"].as_str()
    {
        return Some(id);
    }
    node["children"]
        .as_array()?
        .iter()
        .find_map(|child| semantic_target(child, action))
}
fn evidence(world: &World, snapshot: &Value) -> Value {
    let spine = world.get_resource::<crate::ProductionSpine>();
    json!({"mode":snapshot["mode"],"shell_screen":snapshot["shell"]["screen"],
        "world":spine.and_then(crate::ProductionSpine::world_id),
        "lock":world.get_resource::<crate::VerifiedProductLockHash>().map(|lock|lock.get()),
        "inventory_items":spine.and_then(crate::ProductionSpine::inventory_view).map(|view|view.slots().iter().flatten().map(|stack|u64::from(stack.quantity())).sum::<u64>()),
        "saving":snapshot["game"]["saving"]})
}

fn legacy_world_state(world: &World) -> Value {
    let spine = world.get_resource::<crate::ProductionSpine>();
    let mode = spine
        .and_then(crate::ProductionSpine::gameplay_mode)
        .map(|mode| match mode {
            latticeaxiom_gameplay::GameplayModeV1::Survival => "survival",
            latticeaxiom_gameplay::GameplayModeV1::Creative => "creative",
        });
    json!({"world":spine.and_then(crate::ProductionSpine::world_id),"mode":mode,
        "lock":world.get_resource::<crate::VerifiedProductLockHash>().map(|lock|lock.get()),
        "inventory_items":spine.and_then(crate::ProductionSpine::inventory_view).map(|view|view.slots().iter().flatten().map(|stack|u64::from(stack.quantity())).sum::<u64>())})
}
fn fail(world: &mut World, driver: &mut Driver, snapshot: &Value, error: String) {
    driver.phase = Phase::Finished;
    driver.record(
        "failed",
        json!({"error":error,"state":evidence(world,snapshot)}),
    );
    bevy::log::error!(%error,"Domain lifecycle exercise failed");
    world.write_message(AppExit::Error(std::num::NonZeroU8::MIN));
}
impl Driver {
    fn record(&mut self, event: &str, data: Value) {
        if self.events.len() < 32 {
            self.events.push(
                json!({"event":event,"elapsed_ms":self.started.elapsed().as_millis(),"data":data}),
            );
        }
        let evidence = json!({"schema":"latticeaxiom.lifecycle-domain-evidence.v1","scope":"domain-commands-and-state","dom_verified":false,"webview_rendering_verified":false,"events":self.events});
        if let Err(error) = serde_json::to_vec_pretty(&evidence)
            .map_err(|error| error.to_string())
            .and_then(|bytes| {
                std::fs::write(&self.evidence_path, bytes).map_err(|error| error.to_string())
            })
        {
            bevy::log::error!(%error,"Lifecycle evidence could not be written");
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ready_continue_wins_and_disabled_continue_falls_back_to_creation() {
        let mut snapshot = json!({"shell":{"tree":{"children":[
            {"id":"home/new","actions":["quick-create"],"state":{"disabled":false}},
            {"id":"home/continue","actions":["continue-world"],"state":{"disabled":false}}
        ]}}});
        assert_eq!(
            shell_command(&snapshot).expect("continue").1["target"],
            "home/continue"
        );
        snapshot["shell"]["tree"]["children"][1]["state"]["disabled"] = json!(true);
        assert_eq!(
            shell_command(&snapshot).expect("create").1["target"],
            "home/new"
        );
    }
    #[test]
    fn nested_creation_action_is_discovered_without_native_widget_names() {
        let snapshot = json!({"shell":{"tree":{"children":[{"children":[
            {"id":"new-world/quick-create","actions":["activate","quick-create"],"state":{"disabled":false}}
        ]}]}}});
        assert_eq!(
            shell_command(&snapshot),
            Some((
                "shell.action",
                json!({"target":"new-world/quick-create","action":"quick-create"})
            ))
        );
        assert!(shell_command(&json!({"mode":"game"})).is_none());
    }
}
