//! Bootstrap for the deliberately non-durable local playable fixture.

use avian3d::PhysicsPlugins;
use bevy::{
    ecs::schedule::IntoScheduleConfigs,
    prelude::{
        App, ClearColor, Color, DefaultPlugins, FixedFirst, FixedUpdate, PluginGroup, Startup,
        Update, Window, WindowPlugin,
    },
};
use latticeaxiom_gameplay::GameplayIdError;
use latticeaxiom_player::{BlockEditAuthorityResource, PlayerPlugin, PlayerSystemSet};
use thiserror::Error;

use super::{authority, hud, input, pause, scene};
use crate::{EngineInstanceError, instance::reserve_client_event_loop};

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
                title: "Lattice Axiom — WASD move · mouse look · Space jump · left break · right place · Esc pause".into(),
                ..Window::default()
            }),
            ..WindowPlugin::default()
        }))
        .insert_resource(pause::PlayablePause::default())
        .insert_resource(pause::CursorCaptureState::default())
        .add_plugins(PhysicsPlugins::default())
        .add_plugins(PlayerPlugin)
        .add_plugins(input::PlayableInputPlugin::new(placement_content))
        .add_systems(
            Startup,
            (
                scene::setup_playable_scene,
                hud::setup_playable_hud,
                pause::setup_pause_overlay,
            ),
        )
        .add_systems(
            Update,
            (
                scene::apply_playable_block_receipts,
                scene::sync_playable_camera,
                pause::toggle_pause,
                pause::sync_pause_overlay,
                pause::update_cursor_capture,
                pause::sync_cursor_capture,
                pause::pause_menu_buttons,
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
