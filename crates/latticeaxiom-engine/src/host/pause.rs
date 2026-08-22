//! In-session pause overlay and cursor capture for the interactive client.

use avian3d::prelude::LinearVelocity;
use bevy::{
    app::AppExit,
    prelude::{
        AlignItems, BackgroundColor, Button, Changed, Color, Commands, Component, Display,
        FlexDirection, GlobalZIndex, Interaction, JustifyContent, MessageWriter, Name, Node,
        Pickable, PositionType, Query, Res, ResMut, Resource, Text, TextColor, TextFont, UiRect,
        Val, With,
    },
    ui::FocusPolicy,
    window::{CursorGrabMode, CursorOptions, PrimaryWindow, Window},
};
use latticeaxiom_input::ClientSurfaceActionV1;
use latticeaxiom_player::{
    ActionFrameInbox, ActionState, D2Player, LeafwingPlayerAction, LocalPlayerInput,
    SurfaceActionFrame,
};

use super::{ProductionSessionPause, ProductionSpine, hud::ProductionHudSurfaces};
use crate::EngineProfile;

/// Marker on the pause hint / settings readout.
#[derive(Clone, Copy, Component, Debug, Default, Eq, PartialEq)]
pub(super) struct PauseSettingsHint;

/// Which pause-menu page is visible.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Resource)]
pub(super) struct PauseMenuPage {
    settings: bool,
}

impl PauseMenuPage {
    const fn showing_settings(self) -> bool {
        self.settings
    }

    const fn set_settings(&mut self, settings: bool) {
        self.settings = settings;
    }
}

/// Marker on the full-screen pause overlay.
#[derive(Clone, Copy, Component, Debug, Default, Eq, PartialEq)]
pub(super) struct PauseOverlay;

/// Pause-menu button action.
#[derive(Clone, Copy, Component, Debug, Eq, PartialEq)]
pub(super) enum PauseMenuAction {
    Resume,
    Settings,
    ViewMinus,
    ViewPlus,
    Back,
    Quit,
}

/// Spawns the pause overlay when this Bevy app is the interactive client.
#[allow(clippy::needless_pass_by_value)] // Bevy systems receive SystemParams by value.
pub(super) fn spawn_pause_overlay_if_client(
    mut commands: Commands<'_, '_>,
    profile: Res<'_, EngineProfile>,
) {
    if *profile != EngineProfile::Client {
        return;
    }
    spawn_pause_overlay(&mut commands);
}

fn spawn_pause_overlay(commands: &mut Commands<'_, '_>) {
    commands
        .spawn((
            PauseOverlay,
            Name::new("Pause overlay"),
            Node {
                position_type: PositionType::Absolute,
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                display: Display::None,
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                row_gap: Val::Px(12.0),
                ..Node::default()
            },
            BackgroundColor(Color::srgba(0.02, 0.03, 0.03, 0.62)),
            GlobalZIndex(100),
            FocusPolicy::Block,
            Pickable::IGNORE,
        ))
        .with_children(|overlay| {
            overlay.spawn((
                Name::new("Paused title"),
                Text::new("Paused"),
                TextFont::from_font_size(36.0),
                TextColor(Color::srgb(0.94, 0.95, 0.90)),
                Node {
                    margin: UiRect::bottom(Val::Px(8.0)),
                    ..Node::default()
                },
            ));
            spawn_pause_button(overlay, PauseMenuAction::Resume, "Resume");
            spawn_pause_button(overlay, PauseMenuAction::Settings, "Settings");
            spawn_pause_button(overlay, PauseMenuAction::ViewMinus, "View −");
            spawn_pause_button(overlay, PauseMenuAction::ViewPlus, "View +");
            spawn_pause_button(overlay, PauseMenuAction::Back, "Back");
            spawn_pause_button(overlay, PauseMenuAction::Quit, "Quit Game");
            overlay.spawn((
                PauseSettingsHint,
                Name::new("Pause hint"),
                Text::new("Esc resumes · Settings: render distance"),
                TextFont::from_font_size(16.0),
                TextColor(Color::srgb(0.72, 0.74, 0.68)),
                Node {
                    margin: UiRect::top(Val::Px(8.0)),
                    ..Node::default()
                },
            ));
        });
}

fn spawn_pause_button(
    parent: &mut bevy::ecs::hierarchy::ChildSpawnerCommands<'_>,
    action: PauseMenuAction,
    label: &'static str,
) {
    let hidden = matches!(
        action,
        PauseMenuAction::ViewMinus | PauseMenuAction::ViewPlus | PauseMenuAction::Back
    );
    parent
        .spawn((
            Button,
            action,
            Name::new(label),
            Node {
                width: Val::Px(240.0),
                height: Val::Px(44.0),
                display: if hidden { Display::None } else { Display::Flex },
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                ..Node::default()
            },
            BackgroundColor(button_color(Interaction::None)),
        ))
        .with_children(|button| {
            button.spawn((
                Text::new(label),
                TextFont::from_font_size(20.0),
                TextColor(Color::srgb(0.94, 0.95, 0.90)),
            ));
        });
}

/// Toggles the in-session pause overlay from the Pause action or Escape.
#[allow(clippy::needless_pass_by_value)] // Bevy systems receive SystemParams by value.
pub(super) fn toggle_pause(
    action_states: Query<'_, '_, &ActionState<LeafwingPlayerAction>, With<LocalPlayerInput>>,
    surface: Option<Res<'_, SurfaceActionFrame>>,
    mut pause: ResMut<'_, ProductionSessionPause>,
    mut surfaces: ResMut<'_, ProductionHudSurfaces>,
    mut page: ResMut<'_, PauseMenuPage>,
    mut router: Option<ResMut<'_, super::ProductionSurfaceRouter>>,
) {
    if surface.is_some_and(|frame| frame.just_started(ClientSurfaceActionV1::Pause)) {
        return;
    }
    let from_action = action_states
        .iter()
        .any(|state| state.just_pressed(&LeafwingPlayerAction::Pause));
    if !from_action {
        return;
    }
    if let Some(router) = router.as_mut() {
        let command = if pause.is_paused() {
            latticeaxiom_client_ui::SurfaceCommandV1::Back
        } else {
            latticeaxiom_client_ui::SurfaceCommandV1::Pause
        };
        if router.apply(&command).is_ok() {
            pause.set(router.inner().route().modal() == latticeaxiom_client_ui::GameModalV1::Pause);
            if !pause.is_paused() {
                page.set_settings(false);
            }
            return;
        }
    }
    if surfaces.inventory_open() {
        surfaces.set_inventory_open(false);
        return;
    }
    if pause.is_paused() && page.showing_settings() {
        page.set_settings(false);
        return;
    }
    let paused = !pause.is_paused();
    pause.set(paused);
    if !paused {
        page.set_settings(false);
    }
}

/// Shows or hides the overlay to match the pause latch.
#[allow(clippy::needless_pass_by_value)] // Bevy systems receive SystemParams by value.
pub(super) fn sync_pause_overlay(
    pause: Res<'_, ProductionSessionPause>,
    mut overlay: Query<'_, '_, (&mut Node, &mut Pickable), With<PauseOverlay>>,
) {
    let Ok((mut node, mut pickable)) = overlay.single_mut() else {
        return;
    };
    if pause.is_paused() {
        node.display = Display::Flex;
        *pickable = Pickable::default();
    } else {
        node.display = Display::None;
        *pickable = Pickable::IGNORE;
    }
}

/// Locks the cursor only while the window is focused and the session is live.
#[allow(clippy::needless_pass_by_value)] // Bevy systems receive SystemParams by value.
pub(super) fn sync_cursor_capture(
    pause: Res<'_, ProductionSessionPause>,
    surfaces: Option<Res<'_, ProductionHudSurfaces>>,
    router: Option<Res<'_, super::ProductionSurfaceRouter>>,
    windows: Query<'_, '_, &Window, With<PrimaryWindow>>,
    mut cursors: Query<'_, '_, &mut CursorOptions, With<PrimaryWindow>>,
) {
    let Ok(window) = windows.single() else {
        return;
    };
    let Ok(mut cursor) = cursors.single_mut() else {
        return;
    };
    let blocked = if let Some(router) = router.as_ref() {
        !super::surface::cursor_locked(router)
    } else {
        pause.is_paused() || surfaces.is_some_and(|surfaces| surfaces.inventory_open())
    };
    let capture = window.focused && !blocked;
    cursor.grab_mode = if capture {
        CursorGrabMode::Locked
    } else {
        CursorGrabMode::None
    };
    cursor.visible = !capture;
}

/// Drops live walk, look, and edit input while the overlay is open.
#[allow(clippy::needless_pass_by_value)] // Bevy systems receive SystemParams by value.
pub(super) fn suppress_gameplay_while_paused(
    pause: Res<'_, ProductionSessionPause>,
    surfaces: Option<Res<'_, ProductionHudSurfaces>>,
    mut inbox: ResMut<'_, ActionFrameInbox>,
) {
    if pause.is_paused() || surfaces.is_some_and(|surfaces| surfaces.inventory_open()) {
        inbox.suppress_live_gameplay();
    }
}

/// Holds the kinematic capsule still while paused so gravity does not continue.
#[allow(clippy::needless_pass_by_value)] // Bevy systems receive SystemParams by value.
pub(super) fn freeze_player_while_paused(
    pause: Res<'_, ProductionSessionPause>,
    surfaces: Option<Res<'_, ProductionHudSurfaces>>,
    mut players: Query<'_, '_, &mut LinearVelocity, With<D2Player>>,
) {
    if !pause.is_paused() && !surfaces.is_some_and(|surfaces| surfaces.inventory_open()) {
        return;
    }
    for mut velocity in &mut players {
        *velocity = LinearVelocity::ZERO;
    }
}

/// Applies Resume and Quit from the pause overlay buttons.
#[allow(clippy::needless_pass_by_value)] // Bevy systems receive SystemParams by value.
#[allow(clippy::type_complexity)] // Button interaction query is one pause-menu mapping.
pub(super) fn pause_menu_buttons(
    mut interactions: Query<
        '_,
        '_,
        (&Interaction, &PauseMenuAction, &mut BackgroundColor),
        (Changed<Interaction>, With<Button>),
    >,
    mut pause: ResMut<'_, ProductionSessionPause>,
    mut page: ResMut<'_, PauseMenuPage>,
    spine: Option<Res<'_, ProductionSpine>>,
    mut router: Option<ResMut<'_, super::ProductionSurfaceRouter>>,
    mut exits: MessageWriter<'_, AppExit>,
) {
    for (interaction, action, mut background) in &mut interactions {
        background.0 = button_color(*interaction);
        if *interaction != Interaction::Pressed {
            continue;
        }
        match action {
            PauseMenuAction::Resume => {
                pause.set(false);
                page.set_settings(false);
            }
            PauseMenuAction::Settings => page.set_settings(true),
            PauseMenuAction::Back => page.set_settings(false),
            PauseMenuAction::ViewMinus => {
                if let Some(spine) = spine.as_ref() {
                    let next = spine.requested_view_distance().saturating_sub(1).max(1);
                    let _ = spine.set_requested_view_distance(next);
                }
            }
            PauseMenuAction::ViewPlus => {
                if let Some(spine) = spine.as_ref() {
                    let next = spine.requested_view_distance().saturating_add(1);
                    let _ = spine.set_requested_view_distance(next);
                }
            }
            PauseMenuAction::Quit => {
                if let Some(router) = router.as_mut() {
                    let _ =
                        router.apply(&latticeaxiom_client_ui::SurfaceCommandV1::RequestSaveQuit);
                    let _ = router.apply(&latticeaxiom_client_ui::SurfaceCommandV1::Confirm);
                }
                exits.write(AppExit::Success);
            }
        }
    }
}

/// Shows home or settings controls on the pause overlay.
#[allow(clippy::needless_pass_by_value)] // Bevy systems receive SystemParams by value.
pub(super) fn sync_pause_menu_page(
    page: Res<'_, PauseMenuPage>,
    pause: Res<'_, ProductionSessionPause>,
    spine: Option<Res<'_, ProductionSpine>>,
    mut buttons: Query<'_, '_, (&PauseMenuAction, &mut Node)>,
    mut hint: Query<'_, '_, &mut Text, With<PauseSettingsHint>>,
) {
    if !pause.is_paused() {
        return;
    }
    let settings = page.showing_settings();
    for (action, mut node) in &mut buttons {
        let visible = match action {
            PauseMenuAction::Resume | PauseMenuAction::Settings | PauseMenuAction::Quit => {
                !settings
            }
            PauseMenuAction::ViewMinus | PauseMenuAction::ViewPlus | PauseMenuAction::Back => {
                settings
            }
        };
        node.display = if visible {
            Display::Flex
        } else {
            Display::None
        };
    }
    if let Ok(mut text) = hint.single_mut() {
        let label = if settings {
            spine.map_or_else(
                || "Render distance unavailable".to_owned(),
                |spine| {
                    format!(
                        "Render distance {} chunks (effective {})",
                        spine.requested_view_distance(),
                        spine.effective_view_distance()
                    )
                },
            )
        } else {
            "Esc resumes · Settings: render distance".to_owned()
        };
        if text.0 != label {
            *text = Text::new(label);
        }
    }
}

const fn button_color(interaction: Interaction) -> Color {
    match interaction {
        Interaction::Pressed => Color::srgb(0.18, 0.42, 0.36),
        Interaction::Hovered => Color::srgb(0.16, 0.22, 0.20),
        Interaction::None => Color::srgb(0.08, 0.11, 0.10),
    }
}
