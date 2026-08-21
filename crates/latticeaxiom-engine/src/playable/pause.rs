//! Pause overlay and cursor capture for the local playable fixture.

use avian3d::prelude::LinearVelocity;
use bevy::{
    app::AppExit,
    input::{ButtonInput, keyboard::KeyCode},
    prelude::{
        AlignItems, BackgroundColor, Button, Changed, Color, Commands, Component, Display,
        FlexDirection, GlobalZIndex, Interaction, JustifyContent, MessageWriter, Name, Node,
        Pickable, PositionType, Query, Res, ResMut, Resource, Text, TextColor, TextFont, UiRect,
        Val, With,
    },
    ui::FocusPolicy,
    window::{CursorGrabMode, CursorOptions, PrimaryWindow, Window},
};
use latticeaxiom_player::{ActionFrameInbox, D2Player};

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
                TextFont::from_font_size(36.0),
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
    parent
        .spawn((
            Button,
            action,
            Name::new(label),
            Node {
                width: Val::Px(240.0),
                height: Val::Px(44.0),
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

/// Toggles pause from Escape. This is not a process exit.
#[allow(clippy::needless_pass_by_value)] // Bevy systems receive SystemParams by value.
pub(super) fn toggle_pause(
    keyboard: Res<'_, ButtonInput<KeyCode>>,
    mut pause: ResMut<'_, PlayablePause>,
) {
    if keyboard.just_pressed(KeyCode::Escape) {
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

/// Grabs the cursor after the window is focused; releases it on pause or focus loss.
#[allow(clippy::needless_pass_by_value)] // Bevy systems receive SystemParams by value.
pub(super) fn sync_cursor_capture(
    pause: Res<'_, PlayablePause>,
    windows: Query<'_, '_, &Window, With<PrimaryWindow>>,
    mut cursors: Query<'_, '_, &mut CursorOptions, With<PrimaryWindow>>,
) {
    let Ok(window) = windows.single() else {
        return;
    };
    let Ok(mut cursor) = cursors.single_mut() else {
        return;
    };
    let capture = window.focused && !pause.is_paused();
    cursor.grab_mode = if capture {
        CursorGrabMode::Locked
    } else {
        CursorGrabMode::None
    };
    cursor.visible = !capture;
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

#[allow(clippy::needless_pass_by_value)] // Bevy systems receive SystemParams by value.
#[allow(clippy::type_complexity)] // Button interaction query is one pause-menu mapping.
pub(super) fn pause_menu_buttons(
    mut interactions: Query<
        '_,
        '_,
        (&Interaction, &PauseMenuAction, &mut BackgroundColor),
        (Changed<Interaction>, With<Button>),
    >,
    mut pause: ResMut<'_, PlayablePause>,
    mut exits: MessageWriter<'_, AppExit>,
) {
    for (interaction, action, mut background) in &mut interactions {
        background.0 = button_color(*interaction);
        if *interaction != Interaction::Pressed {
            continue;
        }
        match action {
            PauseMenuAction::Resume => pause.set(false),
            PauseMenuAction::Quit => {
                exits.write(AppExit::Success);
            }
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
