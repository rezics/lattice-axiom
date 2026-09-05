//! Bootstrap for the deliberately non-durable local playable fixture.

use avian3d::PhysicsPlugins;
use bevy::{
    ecs::schedule::IntoScheduleConfigs,
    input::InputSystems,
    prelude::{
        App, ClearColor, Color, DefaultPlugins, FixedFirst, FixedUpdate, PluginGroup, PreUpdate,
        Startup, Update, Window, WindowPlugin,
    },
};
use latticeaxiom_gameplay::GameplayIdError;
use latticeaxiom_player::{
    BlockEditAuthorityResource, ClientInputOwnership, PlayerPlugin, PlayerSystemSet,
};
use thiserror::Error;

use super::{authority, hud, input, pause, scene};
use crate::video::VideoRuntimePlugin;
use crate::{EngineInstanceError, cursor_capture, instance::reserve_client_event_loop};

/// Failure to construct the single-session playable client fixture.
#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum PlayableClientError {
    /// The process-global Bevy client event loop is unavailable.
    #[error(transparent)]
    Engine(#[from] EngineInstanceError),
    /// A trusted built-in gameplay identifier is invalid.
    #[error(transparent)]
    GameplayId(#[from] GameplayIdError),
}

/// Runs the single-session, non-durable local playable fixture.
///
/// This intentionally bypasses the production package lock and durable world
/// writer. It exists only to exercise the built-in Terrenia seed, player,
/// physics, block-edit, and presentation seams together in one Bevy client.
///
/// # Errors
///
/// Returns [`PlayableClientError`] when the process has already reserved a
/// client event loop or a trusted built-in gameplay identifier is invalid.
pub fn run_playable_client() -> Result<(), PlayableClientError> {
    reserve_client_event_loop()?;
    let (authority, seed_cells, placement_content) = authority::seeded_playable_world()?;

    let mut app = App::new();
    app.insert_resource(ClearColor(Color::srgb(0.48, 0.70, 0.91)))
        .insert_resource(BlockEditAuthorityResource::new(authority))
        .insert_resource(seed_cells)
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "Lattice Axiom — WASD move · mouse look · Ctrl sprint · Space jump · Space×2 fly · Shift descend · left break · right place · Esc pause".into(),
                ..Window::default()
            }),
            ..WindowPlugin::default()
        }));
    crate::observability::install_client_observability(&mut app);
    app.add_plugins(VideoRuntimePlugin)
        .insert_resource(pause::PlayablePause::default())
        .init_resource::<cursor_capture::ConfirmedPrimaryWindowFocus>()
        .init_resource::<cursor_capture::CursorCaptureState>()
        .init_resource::<ClientInputOwnership>()
        .add_plugins(PhysicsPlugins::default())
        .add_plugins(PlayerPlugin)
        .add_plugins(input::PlayableInputPlugin::new(placement_content))
        .add_observer(pause::pause_menu_activated)
        .add_systems(
            Startup,
            (
                scene::setup_playable_scene,
                hud::setup_playable_hud,
                pause::setup_pause_overlay,
            ),
        )
        .add_systems(
            PreUpdate,
            (
                cursor_capture::observe_primary_window_focus,
                pause::update_cursor_capture,
            )
                .chain()
                .after(InputSystems)
                .before(input::PlayableInputSystemSet::Sample),
        )
        .add_systems(
            Update,
            (
                scene::apply_playable_block_receipts,
                scene::sync_playable_camera,
                pause::toggle_pause,
                pause::sync_pause_overlay,
                pause::sync_cursor_capture,
                pause::sync_pause_button_visuals,
            ),
        )
        .add_systems(
            FixedFirst,
            pause::suppress_gameplay_while_paused.before(PlayerSystemSet::SampleInput),
        )
        .add_systems(
            FixedUpdate,
            pause::freeze_player_while_paused
                .after(PlayerSystemSet::PrepareMovement)
                .before(PlayerSystemSet::MoveCapsule),
        );

    app.run();
    Ok(())
}
