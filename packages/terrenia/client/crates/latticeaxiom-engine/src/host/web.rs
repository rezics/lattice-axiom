//! Terrenia's package-owned adapters to the embedded browser presentation.
//!
//! Browser callbacks only enqueue bytes. This main-thread Bevy boundary validates
//! session identity and dispatches typed domain operations before projecting state.

use std::{
    collections::VecDeque,
    time::{Duration, Instant},
};

use bevy::prelude::*;
use latticeaxiom_client_ui::{GameModalV1, GameOverlayV1, SurfaceCommandV1};
use latticeaxiom_gameplay::{GameplayModeV1, ItemId, RecipeId, SlotIndex};
use latticeaxiom_webview::{
    AssetBundle, EndpointHandler, EndpointMetadata, EndpointRegistrationError, EndpointRegistry,
    WebViewHost,
};
use serde::Deserialize;
use serde_json::{Value, json};

use super::{ProductionSessionPause, ProductionSpine, ProductionSurfaceRouter};

const HISTORY_LIMIT: usize = 128;
const REQUESTS_PER_FRAME: usize = 32;
const SNAPSHOT_INTERVAL: Duration = Duration::from_millis(100);

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    id: String,
    session: String,
    revision: u64,
    method: String,
    #[serde(default)]
    params: Value,
}

#[derive(Debug)]
struct BrowserHost {
    view: Option<WebViewHost>,
    ready: bool,
    last_sent: Instant,
    size: (u32, u32),
    interactive: Option<bool>,
    closing: bool,
}

#[derive(Resource, Debug)]
struct BridgeState {
    boot: String,
    generation: u64,
    session: String,
    world: Option<String>,
    revision: u64,
    last: Value,
    receipts: VecDeque<(String, Value)>,
    inventories: VecDeque<(u64, Value)>,
    debug: bool,
    text_focus: bool,
    error: Option<String>,
}

#[derive(Resource, Debug, Default)]
struct PackageEndpoints(EndpointRegistry<World>);

#[derive(Component)]
struct ShellInput;

impl crate::EngineInstance {
    /// Installs an explicitly selected package's Web API before running the client.
    ///
    /// Handlers execute on the Bevy main thread and enforce their domain rules.
    /// Registering a method never invokes it or activates world systems.
    ///
    /// # Errors
    /// Rejects invalid package namespaces or duplicate methods.
    pub fn register_web_endpoint(
        &mut self,
        metadata: EndpointMetadata,
        handler: EndpointHandler<World>,
    ) -> Result<(), EndpointRegistrationError> {
        self.app.init_resource::<PackageEndpoints>();
        self.app
            .world_mut()
            .resource_mut::<PackageEndpoints>()
            .0
            .register(metadata, handler)
    }
}

/// Installs one browser host without taking over Bevy's event loop.
pub(crate) fn install(app: &mut App) {
    app.init_resource::<super::item_models::PreviewImages>();
    app.init_resource::<PackageEndpoints>();
    let boot = super::start::unix_now_ms().to_string();
    app.insert_resource(BridgeState {
        session: format!("{boot}:0"),
        boot,
        generation: 0,
        world: None,
        revision: 0,
        last: Value::Null,
        receipts: VecDeque::new(),
        inventories: VecDeque::new(),
        debug: false,
        text_focus: false,
        error: None,
    });
    app.insert_non_send(BrowserHost {
        view: None,
        ready: false,
        last_sent: Instant::now() - SNAPSHOT_INTERVAL,
        size: (0, 0),
        interactive: None,
        closing: false,
    });
    app.add_systems(
        Update,
        pump.after(super::surface::apply_surface_actions)
            .before(super::pause::sync_cursor_capture),
    );
    app.add_systems(
        PreUpdate,
        reconcile_native_focus
            .after(crate::cursor_capture::observe_primary_window_focus)
            .before(super::pause::update_cursor_capture),
    );
    app.add_systems(
        PreUpdate,
        refresh_web_input
            .after(super::pause::update_cursor_capture)
            .before(latticeaxiom_player::ClientInputSystemSet::Sample),
    );
    app.add_systems(PreUpdate, release_browser_before_close);
    app.add_systems(Last, release_browser_before_close);
}

// Child WebView focus changes do not always produce a fresh Bevy focus message.
// Reconcile from the OS on the same UI thread, never from widget intent.
fn reconcile_native_focus(world: &mut World) {
    let Some(primary) = world
        .query_filtered::<Entity, With<bevy::window::PrimaryWindow>>()
        .iter(world)
        .next()
    else {
        return;
    };
    let focused = bevy_winit::WINIT_WINDOWS.with_borrow(|windows| {
        windows
            .get_window(primary)
            .is_some_and(|window| latticeaxiom_webview::window_has_focus(&**window))
    });
    if let Some(mut confirmed) =
        world.get_resource_mut::<crate::cursor_capture::ConfirmedPrimaryWindowFocus>()
    {
        *confirmed = if focused {
            crate::cursor_capture::ConfirmedPrimaryWindowFocus::Focused(primary)
        } else {
            crate::cursor_capture::ConfirmedPrimaryWindowFocus::Unfocused(primary)
        };
    }
}

fn release_browser_before_close(world: &mut World) {
    let closing = world
        .get_resource::<Messages<bevy::window::WindowCloseRequested>>()
        .is_some_and(|messages| !messages.is_empty())
        || world
            .get_resource::<Messages<AppExit>>()
            .is_some_and(|messages| !messages.is_empty());
    if closing && let Some(mut host) = world.get_non_send_mut::<BrowserHost>() {
        host.closing = true;
        host.view = None;
    }
}

pub(crate) fn install_settings(
    world: &mut World,
    workspace: &std::path::Path,
    images: &crate::LockVerifiedComposeImages,
) -> Result<(), String> {
    super::web_settings::install(world, workspace, images)
}

fn refresh_web_input(world: &mut World) {
    let inputs = world
        .query_filtered::<Entity, With<ShellInput>>()
        .iter(world)
        .collect::<Vec<_>>();
    if world.contains_resource::<ProductionSpine>() {
        for input in inputs {
            world.despawn(input);
        }
    } else if inputs.is_empty() {
        if let Some(maps) = world.get_resource::<latticeaxiom_player::CompiledClientInputMaps>() {
            let bundle = latticeaxiom_player::LocalPlayerClientInputBundle::from_compiled(maps);
            world.spawn((ShellInput, latticeaxiom_player::LocalPlayerInput, bundle));
        }
    }
    let Some(host) = world.get_non_send::<BrowserHost>() else {
        return;
    };
    let Some(view) = &host.view else {
        return;
    };
    let focused = view.is_application_focused();
    let interactive = host.interactive.unwrap_or(false);
    if let Some(mut ownership) =
        world.get_resource_mut::<latticeaxiom_player::ClientInputOwnership>()
    {
        if !focused {
            *ownership = latticeaxiom_player::ClientInputOwnership::Released;
        } else if interactive {
            *ownership = latticeaxiom_player::ClientInputOwnership::Surface;
        }
    }
}

// Exclusive systems run on Bevy's main thread; WebView and WINIT_WINDOWS never
// cross a task-pool boundary. No callback borrows World or waits for a response.
fn pump(world: &mut World) {
    let Some(mut host) = world.remove_non_send::<BrowserHost>() else {
        return;
    };
    if host.closing {
        world.insert_non_send(host);
        return;
    }
    let Some((entity, width, height)) = world
        .query_filtered::<(Entity, &Window), With<bevy::window::PrimaryWindow>>()
        .iter(world)
        .next()
        .map(|(e, w)| (e, w.physical_width(), w.physical_height()))
    else {
        world.insert_non_send(host);
        return;
    };
    if host.view.is_none() {
        let generated = world
            .resource::<super::item_models::PreviewImages>()
            .0
            .clone();
        let attached = bevy_winit::WINIT_WINDOWS.with_borrow(|windows| {
            let Some(native) = windows.get_window(entity) else {
                return None;
            };
            Some(AssetBundle::discover().and_then(|assets| {
                WebViewHost::attach(
                    &**native,
                    assets.with_generated(generated.clone()),
                    width,
                    height,
                )
            }))
        });
        match attached {
            Some(Ok(view)) => {
                host.view = Some(view);
                host.size = (width, height);
            }
            Some(Err(error)) => {
                bevy::log::error!(%error, "Embedded UI could not start");
                world.write_message(AppExit::Error(std::num::NonZeroU8::MIN));
                world.insert_non_send(host);
                return;
            }
            None => {
                world.insert_non_send(host);
                return;
            }
        }
    }
    let current_world = world
        .get_resource::<ProductionSpine>()
        .and_then(ProductionSpine::world_id)
        .map(|id| id.to_string());
    if world.resource::<BridgeState>().world != current_world {
        let mut state = world.resource_mut::<BridgeState>();
        state.world = current_world;
        renew_session(&mut state);
        drop(state);
        super::web_projection::refresh_catalog(world);
        host.last_sent = Instant::now() - SNAPSHOT_INTERVAL;
    }
    if world
        .get_resource::<ButtonInput<KeyCode>>()
        .is_some_and(|keys| keys.just_pressed(KeyCode::F3))
    {
        let mut state = world.resource_mut::<BridgeState>();
        state.debug = !state.debug;
    }
    let requests = host
        .view
        .as_mut()
        .map(|view| view.drain_commands::<Request>(REQUESTS_PER_FRAME))
        .unwrap_or_default();
    for request in requests {
        match request {
            Ok(request) if request.method == "ready" => {
                renew_session(&mut world.resource_mut::<BridgeState>());
                host.ready = true;
                host.last_sent = Instant::now() - SNAPSHOT_INTERVAL;
            }
            Ok(request) => {
                let response = execute(world, request);
                if let Some(view) = &host.view {
                    let _ = view.send_state(&response);
                }
                host.last_sent = Instant::now() - SNAPSHOT_INTERVAL;
            }
            Err(error) => {
                world.resource_mut::<BridgeState>().error = Some(error.to_string());
            }
        }
    }
    let interactive = world.get_resource::<ProductionSpine>().is_none()
        || world
            .get_resource::<ProductionSurfaceRouter>()
            .is_none_or(|router| !super::surface::cursor_locked(router))
        || world
            .get_resource::<super::persistent::PersistentGameSession>()
            .is_some_and(|session| session.shutdown_requested());
    if let Some(view) = &host.view {
        if interactive && view.is_application_focused() {
            if let Some(frame) = world.get_resource::<latticeaxiom_player::SurfaceActionFrame>() {
                use latticeaxiom_input::ClientSurfaceActionV1 as Action;
                for (action, name) in [
                    (Action::NavUp, "nav-up"),
                    (Action::NavDown, "nav-down"),
                    (Action::NavLeft, "nav-left"),
                    (Action::NavRight, "nav-right"),
                    (Action::NavNext, "nav-next"),
                    (Action::NavPrevious, "nav-previous"),
                    (Action::Activate, "activate"),
                ] {
                    if frame.just_started(action) {
                        let _ = view.send_state(&json!({"type":"navigation","action":name}));
                    }
                }
                if !world.contains_resource::<ProductionSpine>() && frame.just_started(Action::Back)
                {
                    let _ = view.send_state(&json!({"type":"navigation","action":"back"}));
                }
            }
        }
        if host.size != (width, height) {
            if let Err(error) = view.resize(width, height) {
                world.resource_mut::<BridgeState>().error = Some(error.to_string());
            }
            host.size = (width, height);
        }
        if host.interactive != Some(interactive) {
            if let Err(error) = view.set_interactive(interactive) {
                world.resource_mut::<BridgeState>().error = Some(error.to_string());
            } else {
                host.interactive = Some(interactive);
            }
            world.resource_mut::<BridgeState>().text_focus = false;
            if let Some(mut keys) = world.get_resource_mut::<ButtonInput<KeyCode>>() {
                keys.reset_all();
            }
            if let Some(mut mouse) = world.get_resource_mut::<ButtonInput<MouseButton>>() {
                mouse.reset_all();
            }
        }
    }
    if host.ready && host.last_sent.elapsed() >= SNAPSHOT_INTERVAL {
        let mut snapshot = snapshot(world);
        let changed = world.resource::<BridgeState>().last != snapshot;
        if changed {
            let mut state = world.resource_mut::<BridgeState>();
            state.last = snapshot.clone();
            state.revision += 1;
            snapshot["revision"] = json!(state.revision);
            let revision = state.revision;
            let slots = snapshot["game"]["slots"].clone();
            state.inventories.push_back((revision, slots));
            while state.inventories.len() > HISTORY_LIMIT {
                state.inventories.pop_front();
            }
            if let Some(view) = &host.view {
                let _ = view.send_state(&json!({"type":"snapshot","state":snapshot}));
            }
        }
        host.last_sent = Instant::now();
    }
    world.insert_non_send(host);
}

fn renew_session(state: &mut BridgeState) {
    state.generation += 1;
    state.session = format!("{}:{}", state.boot, state.generation);
    state.last = Value::Null;
    state.receipts.clear();
    state.inventories.clear();
    state.text_focus = false;
}

fn execute(world: &mut World, request: Request) -> Value {
    let state = world.resource::<BridgeState>();
    if request.id.is_empty() || request.id.len() > 128 || request.method.len() > 128 {
        return json!({"type":"result","id":request.id,"ok":false,"error":"Invalid request identity"});
    }
    if request.session != state.session || request.revision > state.revision {
        return json!({"type":"result","id":request.id,"ok":false,"error":"The UI session changed. Refresh and try again."});
    }
    if let Some((_, response)) = state.receipts.iter().find(|(id, _)| *id == request.id) {
        return response.clone();
    }
    let handler = world
        .get_resource::<PackageEndpoints>()
        .and_then(|registry| registry.0.lookup(&request.method));
    let result = if let Some(handler) = handler {
        if world
            .get_resource::<super::persistent::PersistentGameSession>()
            .is_some_and(|s| s.shutdown_requested())
        {
            Err("Wait for the save to finish".into())
        } else {
            handler(world, &request.params)
        }
    } else {
        dispatch(world, &request).map(|()| Value::Null)
    };
    let response = match result {
        Ok(value) => json!({"type":"result","id":request.id,"ok":true,"value":value}),
        Err(error) => json!({"type":"result","id":request.id,"ok":false,"error":error}),
    };
    let mut state = world.resource_mut::<BridgeState>();
    state.receipts.push_back((request.id, response.clone()));
    while state.receipts.len() > HISTORY_LIMIT {
        state.receipts.pop_front();
    }
    response
}

fn dispatch(world: &mut World, request: &Request) -> Result<(), String> {
    let params = &request.params;
    if (request.method.starts_with("game.")
        || request.method.starts_with("inventory.")
        || request.method.starts_with("recipe."))
        && !world.contains_resource::<ProductionSpine>()
    {
        return Err("There is no active world for this operation".into());
    }
    if world
        .get_resource::<super::persistent::PersistentGameSession>()
        .is_some_and(|s| s.shutdown_requested())
        && !matches!(
            request.method.as_str(),
            "game.save" | "game.exit" | "ui.focus" | "debug.toggle"
        )
    {
        return Err("Wait for the current save to finish".into());
    }
    match request.method.as_str() {
        "shell.action" => super::shell_view::web_action(
            world,
            string(params, "target")?,
            string(params, "action")?,
        ),
        "shell.name" => super::shell_view::web_name(world, string(params, "value")?),
        "settings.open" => {
            if super::shell_view::loading(world) {
                return Err("Wait for the world to finish loading".into());
            }
            if world.contains_resource::<ProductionSpine>() {
                apply_surface(world, SurfaceCommandV1::OpenSettings)?;
            }
            super::web_settings::handle(world, "settings.open", params)
        }
        "settings.cancel" => {
            super::web_settings::handle(world, "settings.cancel", params)?;
            if world.contains_resource::<ProductionSpine>() {
                if world
                    .resource::<ProductionSurfaceRouter>()
                    .inner()
                    .route()
                    .modal()
                    == GameModalV1::Settings
                {
                    apply_surface(world, SurfaceCommandV1::Back)?;
                }
            } else {
                super::shell_view::web_close_settings(world)?;
            }
            Ok(())
        }
        method if method.starts_with("settings.") => {
            super::web_settings::handle(world, method, params)
        }
        "ui.focus" => {
            let focused = params["text"].as_bool().unwrap_or(false);
            world.resource_mut::<BridgeState>().text_focus = focused;
            if let Some(mut surfaces) =
                world.get_resource_mut::<super::hud::ProductionHudSurfaces>()
            {
                surfaces.set_text_focus(focused);
            }
            Ok(())
        }
        "debug.toggle" => {
            let mut state = world.resource_mut::<BridgeState>();
            state.debug = !state.debug;
            Ok(())
        }
        "game.surface" => {
            let action = string(params, "action")?;
            let command = match action {
                "inventory" => SurfaceCommandV1::ToggleInventory,
                "workbench" => SurfaceCommandV1::OpenWorkbench,
                "pause" => SurfaceCommandV1::Pause,
                "back" => SurfaceCommandV1::Back,
                "settings" => SurfaceCommandV1::OpenSettings,
                "resume" => SurfaceCommandV1::Resume,
                _ => return Err("Unknown surface action".into()),
            };
            apply_surface(world, command)?;
            if action == "settings" {
                super::web_settings::handle(world, "settings.open", &Value::Null)?;
            }
            Ok(())
        }
        "catalog.query" => super::web_projection::handle_catalog_query(world, params),
        "inventory.move" => {
            require_inventory(world)?;
            let spine = world.resource::<ProductionSpine>();
            let current = super::web_projection::inventory_slots(world);
            if !world
                .resource::<BridgeState>()
                .inventories
                .iter()
                .any(|(revision, slots)| *revision == request.revision && *slots == current)
            {
                return Err("Inventory changed. Review the refreshed slots and try again.".into());
            }
            spine
                .move_stack(
                    SlotIndex::new(slot(params, "from")?),
                    SlotIndex::new(slot(params, "to")?),
                )
                .map(|_| ())
                .map_err(|e| e.to_string())
        }
        "inventory.select" => world
            .get_resource::<ProductionSpine>()
            .ok_or("No active world")?
            .select_hotbar_slot(slot(params, "slot")?)
            .map_err(|e| e.to_string()),
        "inventory.pick" => {
            require_inventory(world)?;
            let item = ItemId::parse(string(params, "item")?).map_err(|e| e.to_string())?;
            world
                .resource::<ProductionSpine>()
                .creative_pick_item(item)
                .map(|_| ())
                .map_err(|e| e.to_string())
        }
        "recipe.craft" => {
            let router = world
                .get_resource::<ProductionSurfaceRouter>()
                .ok_or("No active world")?;
            if router.inner().route().modal() != GameModalV1::None
                || router.inner().route().overlay() == GameOverlayV1::None
            {
                return Err("Open an inventory or workbench first".into());
            }
            let workstation = (router.inner().route().overlay() == GameOverlayV1::Workbench)
                .then_some(super::hud::HOST_WORKBENCH_CONTAINER);
            let recipe = RecipeId::parse(string(params, "recipe")?).map_err(|e| e.to_string())?;
            world
                .resource::<ProductionSpine>()
                .craft_recipe(&recipe, workstation)
                .map(|_| ())
                .map_err(|e| e.to_string())
        }
        "game.save" | "game.exit" => {
            let session = world
                .get_resource::<super::persistent::PersistentGameSession>()
                .ok_or("No persistent world session")?
                .clone();
            if request.method == "game.exit" {
                session.request_save();
            } else {
                session.request_checkpoint();
            }
            Ok(())
        }
        "app.quit" => {
            if world.contains_resource::<ProductionSpine>() {
                return Err("Save and leave the world before quitting".into());
            }
            world.write_message(AppExit::Success);
            Ok(())
        }
        _ => Err("No selected UI package provides this method".into()),
    }
}

fn string<'a>(params: &'a Value, key: &str) -> Result<&'a str, String> {
    params[key]
        .as_str()
        .ok_or_else(|| format!("Missing string field: {key}"))
}
fn slot(params: &Value, key: &str) -> Result<u16, String> {
    params[key]
        .as_u64()
        .and_then(|value| u16::try_from(value).ok())
        .ok_or_else(|| format!("Invalid slot: {key}"))
}
fn require_inventory(world: &World) -> Result<(), String> {
    let route = world
        .get_resource::<ProductionSurfaceRouter>()
        .ok_or("No active world")?
        .inner()
        .route();
    if route.overlay() != GameOverlayV1::Inventory || route.modal() != GameModalV1::None {
        return Err("Open the inventory first".into());
    }
    if world
        .get_resource::<super::persistent::PersistentGameSession>()
        .is_some_and(|s| s.shutdown_requested())
    {
        return Err("World is saving".into());
    }
    Ok(())
}

fn apply_surface(world: &mut World, command: SurfaceCommandV1) -> Result<(), String> {
    if command == SurfaceCommandV1::OpenWorkbench {
        let spine = world
            .get_resource::<ProductionSpine>()
            .ok_or("No active world")?;
        if spine.gameplay_mode() != Some(GameplayModeV1::Creative)
            && !spine
                .current_target()
                .is_some_and(|hit| spine.block_workstation(&hit.block_id).is_some())
        {
            return Err("Aim at a workbench first".into());
        }
        super::hud::bind_host_workbench(spine);
    }
    let receipt = world
        .get_resource_mut::<ProductionSurfaceRouter>()
        .ok_or("No active world")?
        .apply(&command)
        .map_err(|e| e.to_string())?;
    world.resource_scope(|world, mut pause: Mut<'_, ProductionSessionPause>| {
        world.resource_scope(
            |world, mut surfaces: Mut<'_, super::hud::ProductionHudSurfaces>| {
                if let Some(mut suppressed) =
                    world.get_resource_mut::<latticeaxiom_player::GameplaySuppressed>()
                {
                    super::surface::sync_derived_state(
                        &receipt,
                        &mut pause,
                        &mut surfaces,
                        &mut suppressed,
                    );
                }
            },
        );
    });
    Ok(())
}

fn snapshot(world: &World) -> Value {
    let state = world.resource::<BridgeState>();
    let game = super::web_projection::game(world, state.debug);
    let mut settings = super::web_settings::snapshot(world);
    let shell = super::shell_view::web_snapshot(world);
    settings["open"] = json!(if game.is_some() {
        world
            .get_resource::<ProductionSurfaceRouter>()
            .is_some_and(|r| r.inner().route().modal() == GameModalV1::Settings)
    } else {
        settings["open"].as_bool().unwrap_or(false)
            || shell
                .as_ref()
                .is_some_and(|s| s["screen"].as_str() == Some("settings"))
    });
    json!({"protocol":1,"session":state.session,"mode":if game.is_some(){"game"}else{"shell"},"shell":shell,"game":game,"settings":settings,"error":state.error,"uiScale":super::web_settings::effective(world,"latticeaxiom:setting/ui-scale").unwrap_or(json!(1.0)),"theme":super::web_projection::theme(world),"shortcuts":super::web_settings::surface_shortcuts(world),"endpoints":world.get_resource::<PackageEndpoints>().map(|e|e.0.metadata().collect::<Vec<_>>()).unwrap_or_default()})
}

/// Domain evidence for an explicitly marked development QA runtime.
#[cfg(feature = "development")]
pub(crate) fn qa_snapshot(world: &World) -> Value {
    snapshot(world)
}

/// Exercises the same validated operations as Web UI without claiming DOM input.
#[cfg(feature = "development")]
pub(crate) fn qa_command(world: &mut World, method: &str, params: &Value) -> Result<(), String> {
    if std::env::var_os("LATTICEAXIOM_LIFECYCLE_QA").is_none()
        || !std::env::current_dir().is_ok_and(|root| root.join(".latticeaxiom-qa").is_file())
    {
        return Err("Lifecycle automation requires a marked development QA runtime".into());
    }
    let state = world.resource::<BridgeState>();
    let request = Request {
        id: "lifecycle-qa".into(),
        session: state.session.clone(),
        revision: state.revision,
        method: method.into(),
        params: params.clone(),
    };
    dispatch(world, &request)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn test_world() -> World {
        let mut world = World::new();
        world.insert_resource(BridgeState {
            boot: "test".into(),
            generation: 0,
            session: "test:0".into(),
            world: None,
            revision: 1,
            last: Value::Null,
            receipts: VecDeque::new(),
            inventories: VecDeque::new(),
            debug: false,
            text_focus: false,
            error: None,
        });
        world
    }
    fn request(method: &str) -> Request {
        Request {
            id: "one".into(),
            session: "test:0".into(),
            revision: 1,
            method: method.into(),
            params: json!({}),
        }
    }
    #[test]
    fn duplicate_request_does_not_repeat_a_mutation() {
        let mut world = test_world();
        assert_eq!(execute(&mut world, request("debug.toggle"))["ok"], true);
        assert!(world.resource::<BridgeState>().debug);
        assert_eq!(execute(&mut world, request("debug.toggle"))["ok"], true);
        assert!(world.resource::<BridgeState>().debug);
    }
    #[test]
    fn old_page_cannot_submit_into_a_reopened_session() {
        let mut world = test_world();
        renew_session(&mut world.resource_mut::<BridgeState>());
        assert_eq!(execute(&mut world, request("debug.toggle"))["ok"], false);
        assert!(!world.resource::<BridgeState>().debug);
    }
    #[test]
    fn gameplay_requests_in_shell_fail_without_accessing_world_resources() {
        let mut world = test_world();
        for method in [
            "game.surface",
            "inventory.move",
            "recipe.craft",
            "game.exit",
        ] {
            assert_eq!(execute(&mut world, request(method))["ok"], false);
        }
    }
}
