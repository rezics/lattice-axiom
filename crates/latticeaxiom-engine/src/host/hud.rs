//! Minimal production HUD: crosshair, inspect target, and working-set overlay.

use bevy::{
    prelude::{
        AlignItems, BackgroundColor, Color, Commands, Component, JustifyContent, Name, Node,
        Pickable, PositionType, Query, Res, UiRect, Val, With,
    },
    ui::FocusPolicy,
};

use super::spine::{ProductionSpine, WorkingSetDiagnosticsV1};

/// Marker on the compact inspect readout node.
#[derive(Clone, Copy, Component, Debug, Default, Eq, PartialEq)]
pub(super) struct ProductionInspectReadout;

/// Marker on the one-line working-set occupancy overlay.
#[derive(Clone, Copy, Component, Debug, Default, Eq, PartialEq)]
pub(super) struct ProductionWorkingSetReadout;

/// Spawns a non-interactive crosshair and inspect readout.
///
/// Nodes ignore picking and pass UI focus through to gameplay input. Labels
/// use [`Name`] rather than a font asset so the client bootstrap path stays
/// independent of presentation packages.
pub(super) fn spawn_production_hud(mut commands: Commands<'_, '_>) {
    commands
        .spawn((
            Name::new("Production HUD"),
            Node {
                position_type: PositionType::Absolute,
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                ..Node::default()
            },
            FocusPolicy::Pass,
            Pickable::IGNORE,
        ))
        .with_children(|hud| {
            spawn_crosshair(hud);
            spawn_inspect_readout(hud);
            spawn_working_set_readout(hud);
        });
}

fn spawn_crosshair(parent: &mut bevy::ecs::hierarchy::ChildSpawnerCommands<'_>) {
    parent
        .spawn((
            Name::new("Aim crosshair"),
            Node {
                width: Val::Px(22.0),
                height: Val::Px(22.0),
                ..Node::default()
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
                    ..Node::default()
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
                    ..Node::default()
                },
                BackgroundColor(Color::srgba(0.96, 0.97, 0.92, 0.9)),
                FocusPolicy::Pass,
                Pickable::IGNORE,
            ));
        });
}

fn spawn_inspect_readout(parent: &mut bevy::ecs::hierarchy::ChildSpawnerCommands<'_>) {
    parent.spawn((
        ProductionInspectReadout,
        Name::new(no_target_label()),
        Node {
            position_type: PositionType::Absolute,
            top: Val::Px(18.0),
            height: Val::Px(24.0),
            padding: UiRect::all(Val::Px(5.0)),
            align_items: AlignItems::Center,
            ..Node::default()
        },
        BackgroundColor(Color::srgba(0.03, 0.04, 0.035, 0.64)),
        FocusPolicy::Pass,
        Pickable::IGNORE,
    ));
}

fn spawn_working_set_readout(parent: &mut bevy::ecs::hierarchy::ChildSpawnerCommands<'_>) {
    parent.spawn((
        ProductionWorkingSetReadout,
        Name::new(WorkingSetDiagnosticsV1::default().overlay_line()),
        Node {
            position_type: PositionType::Absolute,
            left: Val::Px(18.0),
            bottom: Val::Px(18.0),
            height: Val::Px(24.0),
            padding: UiRect::all(Val::Px(5.0)),
            align_items: AlignItems::Center,
            ..Node::default()
        },
        BackgroundColor(Color::srgba(0.03, 0.04, 0.035, 0.64)),
        FocusPolicy::Pass,
        Pickable::IGNORE,
    ));
}

#[allow(clippy::needless_pass_by_value)] // Bevy systems receive SystemParams by value.
pub(super) fn sync_production_inspect_hud(
    spine: Res<'_, ProductionSpine>,
    mut readout: Query<'_, '_, &mut Name, With<ProductionInspectReadout>>,
) {
    let Ok(mut name) = readout.single_mut() else {
        return;
    };
    let label = spine.current_target().map_or_else(
        || no_target_label().to_owned(),
        |target| format!("Inspect {}", target.block_id),
    );
    if name.as_str() != label {
        *name = Name::new(label);
    }
}

#[allow(clippy::needless_pass_by_value)] // Bevy systems receive SystemParams by value.
pub(super) fn sync_production_working_set_hud(
    snapshot: Res<'_, WorkingSetDiagnosticsV1>,
    mut readout: Query<'_, '_, &mut Name, With<ProductionWorkingSetReadout>>,
) {
    let Ok(mut name) = readout.single_mut() else {
        return;
    };
    let label = snapshot.overlay_line();
    if name.as_str() != label {
        *name = Name::new(label);
    }
}

const fn no_target_label() -> &'static str {
    "Inspect — no target"
}
