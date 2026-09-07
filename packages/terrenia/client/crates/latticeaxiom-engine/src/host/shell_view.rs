//! Web presentation adapter and same-window shell/world lifecycle.
use super::persistent::{
    InProcessShellPlay, ReturnToShell, insert_disk_session_on_world, load_disk_world,
};
use super::{
    ProductionHostError, ProductionInspectSurface, ProductionMemoryStart, ProductionSpine,
    ProductionWorldList, bind_production_world, start::unix_now_ms,
};
use crate::{EngineInstance, LockVerifiedComposeImages, VerifiedProductLockHash};
use bevy::{
    app::{App, AppExit, Plugin, Startup, Update},
    prelude::*,
};
use latticeaxiom_client_ui::desktop_style as style;
use latticeaxiom_core::WorldId;
use latticeaxiom_launcher::{
    FreshClientAppLeaseProof, FreshClientAppLeaseToken, SettingTransactionRevision,
};
use latticeaxiom_start_ui::{
    InputSource, LoadingProgress, LoadingStage, MemoryStartEffect, SemanticActionId,
    SemanticCommand, SemanticNode, SemanticNodeId, ShellEffect,
};

#[derive(Clone, Debug, Default, Resource)]
pub(crate) struct ShellHandoffState(pub std::sync::Arc<std::sync::atomic::AtomicBool>);

#[derive(Debug, Resource)]
struct ClientShellSession {
    start: ProductionMemoryStart,
    focused: Option<SemanticNodeId>,
    tree_epoch: u64,
    world_name: String,
    message: Option<String>,
    pending_world: Option<WorldId>,
}
#[derive(Component, Debug)]
struct ShellCamera;
#[derive(Clone, Copy, Debug, Default)]
struct ClientShellPlugin;
impl Plugin for ClientShellPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_shell_camera).add_systems(
            Update,
            (poll_in_process_play, apply_return_to_shell).chain(),
        );
    }
}
fn spawn_shell_camera(mut commands: Commands<'_, '_>) {
    commands.spawn((Name::new("Shell Camera"), ShellCamera, Camera2d));
}

pub(super) fn web_snapshot(world: &World) -> Option<serde_json::Value> {
    if world.contains_resource::<ProductionSpine>() {
        return None;
    }
    let session = world.get_resource::<ClientShellSession>()?;
    let mut tree = session.start.flow().shell().semantic_tree();
    project_name_value(&mut tree, &session.world_name);
    Some(
        serde_json::json!({"tree":tree,"worldName":session.world_name,"message":session.message,"screen":screen_name(session.start.flow().shell().screen)}),
    )
}
fn screen_name(screen: latticeaxiom_start_ui::ShellScreen) -> &'static str {
    use latticeaxiom_start_ui::ShellScreen;
    match screen {
        ShellScreen::Home => "home",
        ShellScreen::Worlds => "worlds",
        ShellScreen::NewWorld => "new-world",
        ShellScreen::Trash => "trash",
        ShellScreen::QuitConfirm => "quit-confirm",
        ShellScreen::Loading => "loading",
        ShellScreen::PackagesProfiles => "packages-profiles",
        ShellScreen::DiagnosticsAbout => "diagnostics-about",
        ShellScreen::Settings => "settings",
        ShellScreen::Playing => "playing",
        ShellScreen::Pause => "pause",
    }
}
fn project_name_value(node: &mut SemanticNode, name: &str) {
    if node.id.as_str() == "new-world/name" {
        node.value = Some(name.to_owned());
    }
    for child in &mut node.children {
        project_name_value(child, name);
    }
}
pub(super) fn web_name(world: &mut World, name: &str) -> Result<(), String> {
    let mut session = world
        .get_resource_mut::<ClientShellSession>()
        .ok_or("Shell is not available")?;
    if session.start.flow().shell().screen != latticeaxiom_start_ui::ShellScreen::NewWorld {
        return Err("World creation is not open".into());
    }
    session.world_name.clear();
    append_world_name(&mut session.world_name, name);
    session.tree_epoch += 1;
    Ok(())
}
fn append_world_name(name: &mut String, text: &str) {
    let remaining = 64_usize.saturating_sub(name.chars().count());
    name.extend(text.chars().filter(|ch| !ch.is_control()).take(remaining));
}
pub(super) fn loading(world: &World) -> bool {
    world
        .get_resource::<ClientShellSession>()
        .is_some_and(|session| {
            session.pending_world.is_some()
                || session.start.flow().shell().screen
                    == latticeaxiom_start_ui::ShellScreen::Loading
        })
}
pub(super) fn web_close_settings(world: &mut World) -> Result<(), String> {
    let Some(session) = world.get_resource::<ClientShellSession>() else {
        return Ok(());
    };
    if session.start.flow().shell().screen != latticeaxiom_start_ui::ShellScreen::Settings {
        return Ok(());
    }
    fn find(node: &SemanticNode) -> Option<SemanticNodeId> {
        if node.actions.contains(&SemanticActionId::Back) {
            return Some(node.id.clone());
        }
        node.children.iter().find_map(find)
    }
    let target = find(&session.start.flow().shell().semantic_tree())
        .ok_or("Settings has no return action")?;
    web_action(world, target.as_str(), "back")
}
pub(super) fn web_action(world: &mut World, target: &str, action: &str) -> Result<(), String> {
    if world.contains_resource::<ProductionSpine>() {
        return Err("Shell commands are unavailable in a world".into());
    }
    let command = SemanticCommand {
        target: SemanticNodeId::new(target).map_err(|e| e.to_string())?,
        action: serde_json::from_value(serde_json::json!(action)).map_err(|e| e.to_string())?,
        source: InputSource::Keyboard,
    };
    let mut session = world
        .get_resource_mut::<ClientShellSession>()
        .ok_or("Shell is not available")?;
    session.message = None;
    if target == "new-world/quick-create" {
        let intent = session
            .start
            .quick_create_intent(&session.world_name)
            .map_err(|e| e.to_string())?;
        session.start.set_draft(intent);
    }
    session.start.set_now_ms(unix_now_ms());
    let effect = session.start.inject(&command).map_err(|e| e.to_string())?;
    let quit = matches!(
        effect,
        MemoryStartEffect::Shell(ShellEffect::RequestQuitProduct)
    );
    match effect {
        MemoryStartEffect::Shell(ShellEffect::RequestExactWorldLaunch(id)) => {
            session.pending_world = Some(id)
        }
        MemoryStartEffect::Created(_) => {
            if let Ok(intent) = session.start.quick_create_intent("New World") {
                session.start.set_draft(intent);
            }
        }
        _ => {}
    }
    session.focused = first_focusable(&session.start);
    session.tree_epoch += 1;
    let settings =
        session.start.flow().shell().screen == latticeaxiom_start_ui::ShellScreen::Settings;
    drop(session);
    if quit {
        world.write_message(AppExit::Success);
    }
    if settings {
        super::web_settings::handle(world, "settings.open", &serde_json::Value::Null)?;
    }
    Ok(())
}

impl EngineInstance {
    /// Builds the process's sole interactive client as a package-driven start shell.
    ///
    /// The Web adapter renders the package semantic tree. Continue/Play of a `ReadyExact` world
    /// enters Loading and then binds [`super::ProductionSpine`] in this window.
    ///
    /// # Errors
    ///
    /// Returns [`crate::ProductionMemoryStartError`] when the lock does not
    /// resolve a shell-package closure or the process event loop is already
    /// reserved.
    pub fn new_client_shell_from_lock(
        images: LockVerifiedComposeImages,
        lease: FreshClientAppLeaseToken,
        confirmed_setting_transaction_revision: SettingTransactionRevision,
    ) -> Result<(Self, FreshClientAppLeaseProof), crate::ProductionMemoryStartError> {
        let start = ProductionMemoryStart::from_lock_images(images.clone())?;
        Self::new_client_shell_with_start(
            images,
            lease,
            confirmed_setting_transaction_revision,
            start,
        )
    }

    /// Builds the shell around a host-supplied persistent world library.
    ///
    /// # Errors
    /// Returns a start error when the client App cannot be created.
    pub fn new_client_shell_with_start(
        images: LockVerifiedComposeImages,
        lease: FreshClientAppLeaseToken,
        confirmed_setting_transaction_revision: SettingTransactionRevision,
        mut start: ProductionMemoryStart,
    ) -> Result<(Self, FreshClientAppLeaseProof), crate::ProductionMemoryStartError> {
        let product_lock_hash = VerifiedProductLockHash::new(images.product_lock_hash());
        start.set_now_ms(unix_now_ms());
        if let Ok(intent) = start.quick_create_intent("New World") {
            start.set_draft(intent);
        }
        let instance = Self::new_client_with_setup(images.into_images(), move |app| {
            install_client_shell(
                app,
                product_lock_hash,
                start,
                confirmed_setting_transaction_revision,
            );
        })
        .map_err(ProductionHostError::from)?;
        Ok((instance, lease.into_app_created_proof()))
    }
}

fn install_client_shell(
    app: &mut App,
    product_lock_hash: VerifiedProductLockHash,
    mut start: ProductionMemoryStart,
    _confirmed_setting_transaction_revision: SettingTransactionRevision,
) {
    if std::env::var_os("LATTICEAXIOM_CAPTURE_PATH").is_some() {
        let action = match std::env::var("LATTICEAXIOM_CAPTURE_ROUTE").as_deref() {
            Ok("new-world") => Some(SemanticActionId::QuickCreate),
            Ok("worlds") => Some(SemanticActionId::OpenWorlds),
            Ok("settings") => Some(SemanticActionId::OpenSettings),
            _ => None,
        };
        if let Some(action) = action {
            let tree = start.flow().shell().semantic_tree();
            if let Some(node) = tree
                .children
                .iter()
                .find(|node| node.actions.contains(&action))
            {
                let _ = start.inject(&SemanticCommand {
                    target: node.id.clone(),
                    action,
                    source: InputSource::Headless,
                });
            }
        }
    }
    let focused = first_focusable(&start);
    let handoff = ShellHandoffState::default();
    let input_maps = crate::input::compile_lock_selected_input(
        start.images(),
        &latticeaxiom_input::BindingProfileV1::default(),
    )
    .ok()
    .flatten()
    .map(|catalog| latticeaxiom_player::leafwing_maps_from_catalog(&catalog));
    app.insert_resource(handoff.clone())
        .insert_resource(product_lock_hash)
        .insert_resource(ClearColor(style::CANVAS))
        .insert_resource(ClientShellSession {
            start,
            focused,
            tree_epoch: 0,
            world_name: "New World".to_owned(),
            message: None,
            pending_world: None,
        })
        .add_plugins(ClientShellPlugin)
        .insert_resource(InProcessShellPlay);
    super::install_production_schedule(app, false, input_maps);
}

fn poll_in_process_play(world: &mut World) {
    if world.get_resource::<ProductionSpine>().is_some() {
        return;
    }
    let Some(world_id) = world
        .get_resource::<ClientShellSession>()
        .and_then(|session| session.pending_world)
    else {
        return;
    };
    let stage = world
        .get_resource::<ClientShellSession>()
        .and_then(|session| session.start.flow().shell().loading.as_ref())
        .map(|loading| loading.stage);
    let next = match stage {
        None => {
            let mut session = world.resource_mut::<ClientShellSession>();
            session.start.flow_mut().enter_loading();
            session.tree_epoch = session.tree_epoch.saturating_add(1);
            return;
        }
        Some(LoadingStage::CheckingWorld) => (LoadingStage::ResolvingPackages, "packages"),
        Some(LoadingStage::ResolvingPackages) => {
            (LoadingStage::BuildingOrLoading, "world artifacts")
        }
        Some(LoadingStage::BuildingOrLoading) => (LoadingStage::ValidatingContent, "content"),
        Some(LoadingStage::ValidatingContent) => (LoadingStage::LoadingSpawn, "spawn"),
        Some(LoadingStage::LoadingSpawn) => {
            activate_pending_world(world, world_id);
            return;
        }
        Some(LoadingStage::Playing) => return,
    };
    let mut session = world.resource_mut::<ClientShellSession>();
    if let Err(error) = session.start.flow_mut().advance_loading(
        next.0,
        LoadingProgress::Indeterminate,
        Some(next.1.to_owned()),
    ) {
        session.message = Some(error.to_string());
        session.pending_world = None;
        session.start.flow_mut().enter_home();
    }
    session.tree_epoch = session.tree_epoch.saturating_add(1);
}

fn activate_pending_world(world: &mut World, world_id: latticeaxiom_core::WorldId) {
    let prepared = {
        let mut session = world.resource_mut::<ClientShellSession>();
        session.start.set_now_ms(unix_now_ms());
        let materialized = session.start.images_for_world(world_id).and_then(|images| {
            session
                .start
                .materialize_play_spine(world_id)
                .map(|spine| (spine, images))
        });
        match materialized {
            Ok((spine, images)) => {
                if let Err(error) = session
                    .start
                    .flow_mut()
                    .mark_played(world_id, unix_now_ms())
                {
                    session.message = Some(error.to_string());
                    session.pending_world = None;
                    session.start.flow_mut().enter_home();
                    session.tree_epoch = session.tree_epoch.saturating_add(1);
                    return;
                }
                let entry = session.start.saved_entry(world_id).cloned();
                let disk = session.start.disk_store().cloned();
                let shell_lock = session.start.shell_lock_hash();
                let worlds = session.start.flow().worlds().clone();
                Ok((spine, images, entry, disk, shell_lock, worlds))
            }
            Err(error) => Err(error.to_string()),
        }
    };
    let (spine, images, entry, disk, shell_lock, worlds) = match prepared {
        Ok(parts) => parts,
        Err(error) => {
            let mut session = world.resource_mut::<ClientShellSession>();
            session.message = Some(error);
            session.pending_world = None;
            session.start.flow_mut().enter_home();
            session.tree_epoch = session.tree_epoch.saturating_add(1);
            return;
        }
    };
    // The saved world's exact lock owns its client settings and input catalog,
    // even when the shell offers a newer composition for newly created worlds.
    if let Some(workspace) = world
        .get_resource::<crate::client::ShellExitContext>()
        .map(crate::client::ShellExitContext::workspace_path)
    {
        if let Err(error) = super::web_settings::install(world, &workspace, &images) {
            fail_pending_world(world, error);
            return;
        }
    }
    if let (Some(disk), Some(entry)) = (disk, entry) {
        match load_disk_world(&disk, world_id) {
            Ok(storage) => {
                let workspace = world
                    .get_resource::<crate::client::ShellExitContext>()
                    .map(crate::client::ShellExitContext::workspace_path);
                if let Some(workspace) = workspace
                    && let Err(error) = insert_disk_session_on_world(
                        world,
                        &workspace,
                        disk,
                        storage,
                        entry,
                        shell_lock,
                        spine.clone(),
                        true,
                    )
                {
                    fail_pending_world(world, error.to_string());
                    return;
                }
            }
            Err(error) => {
                fail_pending_world(world, error.to_string());
                return;
            }
        }
    }
    reset_surface_state(world);
    let inspect = ProductionInspectSurface::from_lock_images(&images);
    let game_lock = VerifiedProductLockHash::new(images.product_lock_hash());
    bind_production_world(world, game_lock, spine.clone(), inspect, true);
    world.insert_resource(ProductionWorldList::new(worlds));
    install_play_settings(world, &images);
    despawn_named::<ShellCamera>(world);
    super::spawn_play_presentation(world);
    let mut session = world.resource_mut::<ClientShellSession>();
    session.start.flow_mut().enter_playing();
    session.pending_world = None;
    session.tree_epoch = session.tree_epoch.saturating_add(1);
}

fn install_play_settings(world: &mut World, images: &LockVerifiedComposeImages) {
    let Some(workspace) = world
        .get_resource::<crate::client::ShellExitContext>()
        .map(crate::client::ShellExitContext::workspace_path)
    else {
        return;
    };
    let Ok(user) = crate::settings::HostUserSettings::load(workspace.join("run/user")) else {
        return;
    };
    let Ok(Some(catalog)) = crate::settings::compile_lock_selected_settings(images) else {
        return;
    };
    let Some(spine) = world.get_resource::<ProductionSpine>().cloned() else {
        return;
    };
    let Ok(state) = super::pause::ProductionSettingsState::new(
        workspace.join("run/user"),
        user,
        catalog,
        images.product_lock_hash(),
    ) else {
        return;
    };
    let _ = spine.set_terrain_presentation(
        state.applied_terrain_distances(),
        state.applied_far_terrain_quality(),
    );
    if let Some(mut video) = world.get_resource_mut::<crate::VideoRuntimeSettings>() {
        let requested = state.applied_video();
        video.replace(
            requested.vsync(),
            requested.foreground_limit(),
            requested.background_limit(),
        );
    }
    let tick_rate = state.applied_tick_rate();
    world.insert_resource(latticeaxiom_player::SimulationClock::new(tick_rate));
    if let Some(mut fixed) = world.get_resource_mut::<bevy::time::Time<bevy::time::Fixed>>() {
        fixed.set_timestep(tick_rate.timestep());
    }
    world.insert_resource(state);
}

fn apply_return_to_shell(world: &mut World) {
    if world.get_resource::<ReturnToShell>().is_none() {
        return;
    }
    world.remove_resource::<ReturnToShell>();
    let world_id = world
        .get_resource::<ProductionSpine>()
        .and_then(ProductionSpine::world_id);
    let entities = world
        .iter_entities()
        .filter(|entity| {
            entity.contains::<super::InProcessPlayEntity>()
                || entity.contains::<super::ChunkPresentation>()
        })
        .map(|entity| entity.id())
        .collect::<Vec<_>>();
    for entity in entities {
        world.despawn(entity);
    }
    world.remove_resource::<ProductionSpine>();
    world.remove_resource::<super::persistent::PersistentGameSession>();
    if let Some(shell) = world.get_resource::<crate::client::ShellExitContext>()
        && let Err(error) = shell.set_active_session(None)
    {
        bevy::log::error!(%error,"Completed world could not release its shutdown reference");
    }
    world.remove_resource::<super::pause::ProductionSettingsState>();
    world.remove_resource::<latticeaxiom_player::BlockEditAuthorityResource>();
    world.remove_resource::<super::ProductionWorldStorage>();
    world.remove_resource::<super::WorkingSetDiagnosticsV1>();
    world.remove_resource::<ProductionWorldList>();
    #[cfg(feature = "client")]
    {
        world.remove_resource::<super::chunk_mesh::ProductionTerrainPalette>();
        world.remove_resource::<super::chunk_mesh::ProductionTerrainMaterials>();
        world.remove_resource::<super::far_mesh::FarTerrainRolePalette>();
    }
    reset_surface_state(world);
    world.insert_resource(ClearColor(style::CANVAS));
    world.spawn((Name::new("Shell Camera"), ShellCamera, Camera2d));
    let mut session = world.resource_mut::<ClientShellSession>();
    if let Some(world_id) = world_id {
        session.start.release_play_spine(world_id);
    }
    session.pending_world = None;
    session.start.flow_mut().enter_home();
    session.focused = first_focusable(&session.start);
    session.tree_epoch = session.tree_epoch.saturating_add(1);
    drop(session);
    restore_shell_settings(world);
}

fn fail_pending_world(world: &mut World, error: String) {
    let mut session = world.resource_mut::<ClientShellSession>();
    session.message = Some(error);
    session.pending_world = None;
    session.start.flow_mut().enter_home();
    session.tree_epoch = session.tree_epoch.saturating_add(1);
    drop(session);
    restore_shell_settings(world);
}

fn restore_shell_settings(world: &mut World) {
    let images = world
        .resource::<ClientShellSession>()
        .start
        .images()
        .clone();
    if let Some(workspace) = world
        .get_resource::<crate::client::ShellExitContext>()
        .map(crate::client::ShellExitContext::workspace_path)
        && let Err(error) = super::web_settings::install(world, &workspace, &images)
    {
        world.resource_mut::<ClientShellSession>().message = Some(error);
    }
}

fn reset_surface_state(world: &mut World) {
    if let Ok(router) = super::ProductionSurfaceRouter::playing() {
        world.insert_resource(router);
    }
    world.insert_resource(super::ProductionSessionPause::default());
    world.insert_resource(super::hud::ProductionHudSurfaces::default());
    world.insert_resource(latticeaxiom_player::GameplaySuppressed::default());
    world.insert_resource(latticeaxiom_player::SurfaceActionFrame::default());
    world.insert_resource(crate::cursor_capture::CursorCaptureState::ReleaseRequested);
    world.insert_resource(latticeaxiom_player::ClientInputOwnership::Surface);
    if let Some(mut keys) = world.get_resource_mut::<ButtonInput<KeyCode>>() {
        keys.reset_all();
    }
    if let Some(mut mouse) = world.get_resource_mut::<ButtonInput<MouseButton>>() {
        mouse.reset_all();
    }
}

fn despawn_named<T: Component>(world: &mut World) {
    let entities = world
        .query_filtered::<Entity, With<T>>()
        .iter(world)
        .collect::<Vec<_>>();
    for entity in entities {
        world.despawn(entity);
    }
}

fn first_focusable(start: &ProductionMemoryStart) -> Option<SemanticNodeId> {
    if start.flow().shell().screen == latticeaxiom_start_ui::ShellScreen::NewWorld {
        return SemanticNodeId::new("new-world/name").ok();
    }
    start
        .flow()
        .shell()
        .semantic_tree()
        .focus_order()
        .into_iter()
        .next()
}

#[cfg(test)]
mod name_editor_tests {
    use super::append_world_name;

    #[test]
    fn returning_to_shell_resets_modal_and_held_input_but_requests_native_release() {
        use bevy::prelude::*;
        let mut world = World::new();
        let mut router = crate::host::ProductionSurfaceRouter::playing().expect("router");
        router
            .apply(&latticeaxiom_client_ui::SurfaceCommandV1::Pause)
            .expect("pause");
        router
            .apply(&latticeaxiom_client_ui::SurfaceCommandV1::OpenSettings)
            .expect("settings");
        world.insert_resource(router);
        let mut keys = ButtonInput::<KeyCode>::default();
        keys.press(KeyCode::KeyW);
        world.insert_resource(keys);
        world.insert_resource(crate::cursor_capture::CursorCaptureState::Captured);
        super::reset_surface_state(&mut world);
        assert_eq!(
            world
                .resource::<crate::host::ProductionSurfaceRouter>()
                .inner()
                .route()
                .modal(),
            latticeaxiom_client_ui::GameModalV1::None
        );
        assert!(
            !world
                .resource::<ButtonInput<KeyCode>>()
                .pressed(KeyCode::KeyW)
        );
        assert!(
            world
                .resource::<crate::cursor_capture::CursorCaptureState>()
                .release_pending()
        );
        assert_eq!(
            *world.resource::<latticeaxiom_player::ClientInputOwnership>(),
            latticeaxiom_player::ClientInputOwnership::Surface
        );
    }

    #[test]
    fn name_input_preserves_unicode_and_bounds_committed_text() {
        let mut name = String::new();
        append_world_name(&mut name, "晶格世界\n");
        assert_eq!(name, "晶格世界");
        append_world_name(&mut name, &"界".repeat(100));
        assert_eq!(name.chars().count(), 64);
        name.pop();
        assert_eq!(name.chars().count(), 63);
    }
}
