//! Native aim visuals and workbench activation; panels are rendered by Web UI.
use super::{ProductionSessionPause, ProductionSpine};
use bevy::{prelude::*, ui::FocusPolicy};
use latticeaxiom_gameplay::{ContainerId, WorkstationId};
use latticeaxiom_player::{
    ActionState, ClientInputOwnership, LeafwingPlayerAction, LocalPlayerInput,
};

pub(super) const HOST_WORKBENCH_CONTAINER: ContainerId = ContainerId::new(1);

/// Host input ownership for mutually exclusive Web inventory/workbench panels.
#[derive(Clone, Debug, Default, Eq, PartialEq, Resource)]
pub(super) struct ProductionHudSurfaces {
    open: bool,
    text_focus: bool,
}
impl ProductionHudSurfaces {
    pub const fn inventory_open(&self) -> bool {
        self.open
    }
    pub const fn set_inventory_open(&mut self, open: bool) {
        self.open = open;
        if !open {
            self.text_focus = false;
        }
    }
    pub const fn set_workbench_open(&mut self, open: bool) {
        self.set_inventory_open(open);
    }
    pub const fn item_browser_search_focused(&self) -> bool {
        self.text_focus
    }
    pub fn dismiss_item_browser_search(&mut self) {
        self.text_focus = false;
    }
    pub fn set_text_focus(&mut self, focused: bool) {
        self.text_focus = focused;
    }
}

pub(super) fn spawn_production_hud(
    mut commands: Commands<'_, '_>,
    _images: Option<Res<'_, crate::StructurallyValidatedComposeImages>>,
) {
    commands
        .spawn((
            Name::new("Production HUD"),
            super::InProcessPlayEntity,
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
        });
}

fn spawn_crosshair(parent: &mut bevy::ecs::hierarchy::ChildSpawnerCommands<'_>) {
    parent
        .spawn((
            Name::new("Aim crosshair"),
            Node {
                width: Val::Px(48.0),
                height: Val::Px(48.0),
                ..Node::default()
            },
            FocusPolicy::Pass,
            Pickable::IGNORE,
        ))
        .with_children(|crosshair| {
            super::mining_ring::spawn_mining_ring(crosshair);
            crosshair.spawn((
                Name::new("Crosshair horizontal stroke"),
                Node {
                    position_type: PositionType::Absolute,
                    left: Val::Px(17.0),
                    top: Val::Px(23.0),
                    width: Val::Px(14.0),
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
                    left: Val::Px(23.0),
                    top: Val::Px(17.0),
                    width: Val::Px(2.0),
                    height: Val::Px(14.0),
                    ..Node::default()
                },
                BackgroundColor(Color::srgba(0.96, 0.97, 0.92, 0.9)),
                FocusPolicy::Pass,
                Pickable::IGNORE,
            ));
        });
}

pub(super) fn activate_workbench_from_target(
    mut pause: ResMut<'_, ProductionSessionPause>,
    ownership: Res<'_, ClientInputOwnership>,
    action_states: Query<'_, '_, &ActionState<LeafwingPlayerAction>, With<LocalPlayerInput>>,
    spine: Res<'_, ProductionSpine>,
    mut surfaces: ResMut<'_, ProductionHudSurfaces>,
    mut router: ResMut<'_, super::ProductionSurfaceRouter>,
    mut suppressed: ResMut<'_, latticeaxiom_player::GameplaySuppressed>,
) {
    if pause.is_paused() || !ownership.owns_gameplay_input() {
        return;
    }
    if !action_states
        .iter()
        .any(|state| state.just_pressed(&LeafwingPlayerAction::SurfaceActivate))
    {
        return;
    }
    let Some(target) = spine.current_target() else {
        return;
    };
    let Some(workstation) = spine.block_workstation(&target.block_id) else {
        return;
    };
    if workstation != crafting_workstation() {
        return;
    }
    bind_host_workbench(&spine);
    if let Ok(receipt) = router.apply(&latticeaxiom_client_ui::SurfaceCommandV1::OpenWorkbench) {
        super::surface::sync_derived_state(&receipt, &mut pause, &mut surfaces, &mut suppressed);
    }
}

pub(super) fn bind_host_workbench(spine: &ProductionSpine) {
    let _ = spine.bind_workstation(crafting_workstation(), HOST_WORKBENCH_CONTAINER);
}

pub(super) fn crafting_workstation() -> WorkstationId {
    match WorkstationId::parse("latticeaxiom:workstation/crafting@1") {
        Ok(workstation) => workstation,
        Err(error) => {
            panic!("crafting workstation is a platform contract: {error}")
        }
    }
}

#[cfg(test)]
mod tests {
    use bevy::prelude::*;
    #[test]
    fn shell_buttons_do_not_require_a_bound_game_world() {
        let mut app = App::new();
        app.add_plugins(super::super::ProductionHostPlugin);
        let entity = app.world_mut().spawn(bevy::ui_widgets::Button).id();
        app.world_mut()
            .trigger(bevy::ui_widgets::Activate { entity });
        app.world_mut()
            .trigger(bevy::ui_widgets::Activate { entity });
        assert!(!app.world().contains_resource::<super::ProductionSpine>());
    }
}
