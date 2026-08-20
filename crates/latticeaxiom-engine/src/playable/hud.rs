//! Minimal, non-interactive HUD for the local playable slice.

use bevy::{
    prelude::{
        AlignItems, BackgroundColor, Color, Commands, Name, Node, Pickable, PositionType, UiRect,
        Val,
    },
    ui::FocusPolicy,
};

/// Spawns the crosshair, one-slot block bar, and compact control legend.
///
/// Every node ignores picking and passes UI focus through to the gameplay
/// input systems. The legend uses named shapes instead of depending on a font
/// asset during the development slice's bootstrap path.
pub(crate) fn setup_playable_hud(mut commands: Commands<'_, '_>) {
    commands
        .spawn((
            Name::new("Playable HUD — WASD move; Space jump; LMB mine; RMB place; Esc exit"),
            Node {
                position_type: PositionType::Absolute,
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                align_items: AlignItems::Center,
                justify_content: bevy::prelude::JustifyContent::Center,
                ..Default::default()
            },
            FocusPolicy::Pass,
            Pickable::IGNORE,
        ))
        .with_children(|hud| {
            spawn_crosshair(hud);
            spawn_hotbar(hud);
            spawn_control_legend(hud);
        });
}

fn spawn_crosshair(parent: &mut bevy::ecs::hierarchy::ChildSpawnerCommands<'_>) {
    parent
        .spawn((
            Name::new("Aim crosshair"),
            Node {
                width: Val::Px(22.0),
                height: Val::Px(22.0),
                ..Default::default()
            },
            FocusPolicy::Pass,
            Pickable::IGNORE,
        ))
        .with_children(|crosshair| {
            crosshair.spawn((
                Name::new("Crosshair horizontal stroke"),
                Node {
                    position_type: PositionType::Absolute,
                    left: Val::Px(1.0),
                    top: Val::Px(10.0),
                    width: Val::Px(20.0),
                    height: Val::Px(2.0),
                    ..Default::default()
                },
                BackgroundColor(Color::srgba(0.96, 0.97, 0.92, 0.9)),
                FocusPolicy::Pass,
                Pickable::IGNORE,
            ));
            crosshair.spawn((
                Name::new("Crosshair vertical stroke"),
                Node {
                    position_type: PositionType::Absolute,
                    left: Val::Px(10.0),
                    top: Val::Px(1.0),
                    width: Val::Px(2.0),
                    height: Val::Px(20.0),
                    ..Default::default()
                },
                BackgroundColor(Color::srgba(0.96, 0.97, 0.92, 0.9)),
                FocusPolicy::Pass,
                Pickable::IGNORE,
            ));
        });
}

fn spawn_hotbar(parent: &mut bevy::ecs::hierarchy::ChildSpawnerCommands<'_>) {
    parent
        .spawn((
            Name::new("Selected block hotbar slot — grass over dirt"),
            Node {
                position_type: PositionType::Absolute,
                left: Val::Percent(50.0),
                bottom: Val::Px(26.0),
                width: Val::Px(68.0),
                height: Val::Px(68.0),
                margin: UiRect::left(Val::Px(-34.0)),
                padding: UiRect::all(Val::Px(5.0)),
                ..Default::default()
            },
            BackgroundColor(Color::srgba(0.04, 0.05, 0.04, 0.78)),
            FocusPolicy::Pass,
            Pickable::IGNORE,
        ))
        .with_children(|slot| {
            slot.spawn((
                Name::new("Grass block swatch"),
                Node {
                    width: Val::Percent(100.0),
                    height: Val::Percent(100.0),
                    ..Default::default()
                },
                BackgroundColor(Color::srgb(0.31, 0.52, 0.20)),
                FocusPolicy::Pass,
                Pickable::IGNORE,
            ))
            .with_child((
                Name::new("Dirt underside swatch"),
                Node {
                    position_type: PositionType::Absolute,
                    left: Val::Px(0.0),
                    right: Val::Px(0.0),
                    bottom: Val::Px(0.0),
                    height: Val::Percent(38.0),
                    ..Default::default()
                },
                BackgroundColor(Color::srgb(0.43, 0.27, 0.13)),
                FocusPolicy::Pass,
                Pickable::IGNORE,
            ));
        });
}

fn spawn_control_legend(parent: &mut bevy::ecs::hierarchy::ChildSpawnerCommands<'_>) {
    parent
        .spawn((
            Name::new("Controls — WASD move; Space jump; LMB mine; RMB place; Esc exit"),
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(18.0),
                top: Val::Px(18.0),
                height: Val::Px(24.0),
                padding: UiRect::all(Val::Px(5.0)),
                align_items: AlignItems::Center,
                ..Default::default()
            },
            BackgroundColor(Color::srgba(0.03, 0.04, 0.035, 0.64)),
            FocusPolicy::Pass,
            Pickable::IGNORE,
        ))
        .with_children(|legend| {
            for (name, color, width) in [
                ("WASD movement", Color::srgb(0.25, 0.70, 0.66), 32.0),
                ("Space jump", Color::srgb(0.88, 0.90, 0.84), 22.0),
                ("Left mouse mine", Color::srgb(0.88, 0.59, 0.22), 14.0),
                ("Right mouse place", Color::srgb(0.43, 0.27, 0.13), 14.0),
                ("Escape exit", Color::srgb(0.75, 0.20, 0.17), 14.0),
            ] {
                legend.spawn((
                    Name::new(name),
                    Node {
                        width: Val::Px(width),
                        height: Val::Px(14.0),
                        margin: UiRect::right(Val::Px(4.0)),
                        ..Default::default()
                    },
                    BackgroundColor(color),
                    FocusPolicy::Pass,
                    Pickable::IGNORE,
                ));
            }
        });
}
