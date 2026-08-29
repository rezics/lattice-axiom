//! Pause overlay and cursor capture for the local playable fixture.

use avian3d::prelude::LinearVelocity;
use bevy::{
    app::AppExit,
    ecs::observer::On,
    ecs::query::Has,
    ecs::system::NonSendMarker,
    input::{ButtonInput, keyboard::KeyCode, mouse::MouseButton},
    picking::hover::Hovered,
    prelude::{
        AlignItems, BackgroundColor, Color, Commands, Component, Display, Entity, FlexDirection,
        GlobalZIndex, JustifyContent, MessageWriter, Name, Node, Pickable, PositionType, Query,
        Res, ResMut, Resource, Text, TextColor, UiRect, Val, With,
    },
    ui::{FocusPolicy, Pressed},
    ui_widgets::{Activate, Button},
    window::{CursorOptions, PrimaryWindow, Window},
};
use latticeaxiom_player::{ActionFrameInbox, ClientInputOwnership, D2Player};

use crate::{
    cursor_capture::{
        ConfirmedPrimaryWindowFocus, CursorCaptureGesture, CursorCaptureState,
        apply_cursor_capture, screenshot_release_requested,
    },
    ui_font::ui_text_font,
};

/// In-session pause latch for the development playable fixture.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Resource)]
pub(super) struct PlayablePause {
    paused: bool,
}

impl PlayablePause {
    pub(super) const fn is_paused(self) -> bool {
        self.paused
    }

    const fn set(&mut self, paused: bool) {
        self.paused = paused;
    }
}

#[derive(Clone, Copy, Component, Debug, Default, Eq, PartialEq)]
pub(super) struct PauseOverlay;

#[derive(Clone, Copy, Component, Debug, Eq, PartialEq)]
pub(super) enum PauseMenuAction {
    Resume,
    Quit,
}

/// Spawns the pause overlay hidden until Escape is pressed.
pub(super) fn setup_pause_overlay(mut commands: Commands<'_, '_>) {
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
                ui_text_font(36.0),
                TextColor(Color::srgb(0.94, 0.95, 0.90)),
                Node {
                    margin: UiRect::bottom(Val::Px(8.0)),
                    ..Node::default()
                },
            ));
            spawn_pause_button(overlay, PauseMenuAction::Resume, "Resume");
            spawn_pause_button(overlay, PauseMenuAction::Quit, "Quit Game");
            overlay.spawn((
                Name::new("Pause hint"),
                Text::new("Esc resumes"),
                ui_text_font(16.0),
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
    parent
        .spawn((
            Button,
            action,
            Name::new(label),
            Hovered::default(),
            Node {
                width: Val::Px(240.0),
                height: Val::Px(44.0),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                ..Node::default()
            },
            BackgroundColor(button_color(false, false)),
        ))
        .with_children(|button| {
            button.spawn((
                Text::new(label),
                ui_text_font(20.0),
                TextColor(Color::srgb(0.94, 0.95, 0.90)),
            ));
        });
}

/// Toggles pause from Escape. This is not a process exit.
#[allow(clippy::needless_pass_by_value)] // Bevy systems receive SystemParams by value.
pub(super) fn toggle_pause(
    keyboard: Res<'_, ButtonInput<KeyCode>>,
    ownership: Res<'_, ClientInputOwnership>,
    mut pause: ResMut<'_, PlayablePause>,
) {
    if *ownership != ClientInputOwnership::Released && keyboard.just_pressed(KeyCode::Escape) {
        let paused = !pause.is_paused();
        pause.set(paused);
    }
}

#[allow(clippy::needless_pass_by_value)] // Bevy systems receive SystemParams by value.
pub(super) fn sync_pause_overlay(
    pause: Res<'_, PlayablePause>,
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

/// Advances gesture-owned cursor capture before direct input sampling.
#[allow(clippy::needless_pass_by_value)] // Bevy systems receive SystemParams by value.
pub(super) fn update_cursor_capture(
    pause: Res<'_, PlayablePause>,
    mut capture: ResMut<'_, CursorCaptureState>,
    confirmed_focus: Res<'_, ConfirmedPrimaryWindowFocus>,
    windows: Query<'_, '_, Entity, With<PrimaryWindow>>,
    mut mouse: ResMut<'_, ButtonInput<MouseButton>>,
    keyboard: Res<'_, ButtonInput<KeyCode>>,
    mut ownership: ResMut<'_, ClientInputOwnership>,
) {
    let Ok(window) = windows.single() else {
        *ownership = ClientInputOwnership::Released;
        return;
    };
    let focused = confirmed_focus.is_focused(window);
    let route_allows_capture = !pause.is_paused();
    let release_requested =
        keyboard.just_pressed(KeyCode::Escape) || screenshot_release_requested(&keyboard);
    capture.reconcile(
        focused && route_allows_capture,
        CursorCaptureGesture::from_primary_button(&mouse),
        release_requested,
    );
    if capture.release_pending() {
        mouse.reset_all();
    }
    *ownership = if !focused {
        ClientInputOwnership::Released
    } else if route_allows_capture && capture.owns_gameplay_input() {
        ClientInputOwnership::Gameplay
    } else {
        ClientInputOwnership::Surface
    };
}

/// Centers and locks the cursor while the gesture-owned state permits it.
#[allow(clippy::needless_pass_by_value)] // Bevy systems receive SystemParams by value.
pub(super) fn sync_cursor_capture(
    pause: Res<'_, PlayablePause>,
    mut capture: ResMut<'_, CursorCaptureState>,
    confirmed_focus: Res<'_, ConfirmedPrimaryWindowFocus>,
    mut windows: Query<'_, '_, (Entity, &mut Window, &mut CursorOptions), With<PrimaryWindow>>,
    main_thread: NonSendMarker,
) {
    let Ok((window_entity, mut window, mut cursor)) = windows.single_mut() else {
        return;
    };
    let capture_allowed = confirmed_focus.is_focused(window_entity) && !pause.is_paused();
    if let Err(error) = apply_cursor_capture(
        &mut capture,
        capture_allowed,
        window_entity,
        &mut window,
        &mut cursor,
        &main_thread,
    ) {
        bevy::log::warn!(error = %error, "cursor capture transition failed");
    }
}

#[allow(clippy::needless_pass_by_value)] // Bevy systems receive SystemParams by value.
pub(super) fn suppress_gameplay_while_paused(
    pause: Res<'_, PlayablePause>,
    mut inbox: ResMut<'_, ActionFrameInbox>,
) {
    if pause.is_paused() {
        inbox.suppress_live_gameplay();
    }
}

#[allow(clippy::needless_pass_by_value)] // Bevy systems receive SystemParams by value.
pub(super) fn freeze_player_while_paused(
    pause: Res<'_, PlayablePause>,
    mut players: Query<'_, '_, &mut LinearVelocity, With<D2Player>>,
) {
    if !pause.is_paused() {
        return;
    }
    for mut velocity in &mut players {
        *velocity = LinearVelocity::ZERO;
    }
}

#[allow(clippy::needless_pass_by_value)] // Bevy observers receive SystemParams by value.
pub(super) fn pause_menu_activated(
    activate: On<'_, '_, Activate>,
    actions: Query<'_, '_, &PauseMenuAction, With<Button>>,
    mut pause: ResMut<'_, PlayablePause>,
    mut exits: MessageWriter<'_, AppExit>,
) {
    let Ok(action) = actions.get(activate.entity) else {
        return;
    };
    match action {
        PauseMenuAction::Resume => pause.set(false),
        PauseMenuAction::Quit => {
            exits.write(AppExit::Success);
        }
    }
}

#[allow(clippy::needless_pass_by_value)] // Bevy systems receive SystemParams by value.
pub(super) fn sync_pause_button_visuals(
    mut buttons: Query<
        '_,
        '_,
        (&Hovered, Has<Pressed>, &mut BackgroundColor),
        With<PauseMenuAction>,
    >,
) {
    for (hovered, pressed, mut background) in &mut buttons {
        let desired = button_color(pressed, hovered.get());
        if background.0 != desired {
            background.0 = desired;
        }
    }
}

const fn button_color(pressed: bool, hovered: bool) -> Color {
    if pressed {
        Color::srgb(0.18, 0.42, 0.36)
    } else if hovered {
        Color::srgb(0.16, 0.22, 0.20)
    } else {
        Color::srgb(0.08, 0.11, 0.10)
    }
}
