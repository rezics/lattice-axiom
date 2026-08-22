//! Production HUD: crosshair, inspect, status, hotbar, and inventory.

use bevy::{
    ecs::query::QueryFilter,
    input::{ButtonInput, keyboard::KeyCode},
    prelude::{
        AlignItems, BackgroundColor, Button, Changed, Children, Color, Commands, Component,
        Display, FlexDirection, FlexWrap, GlobalZIndex, Interaction, JustifyContent, Name, Node,
        Overflow, Pickable, PositionType, Query, Res, ResMut, Resource, Text, TextColor, TextFont,
        UiRect, Val, With, Without,
    },
    ui::FocusPolicy,
};
use latticeaxiom_gameplay::{ContainerId, RecipeId, SlotIndex, WorkstationId};
use latticeaxiom_player::{
    ActionState, BlockEditRejectV1, HeadlessTargetInspectV1, LeafwingPlayerAction, LocalPlayerInput,
};

use super::{
    HOTBAR_SLOTS, INVENTORY_SLOTS, ProductionSessionPause, ProductionSpine,
    WorkingSetDiagnosticsV1, gameplay::ProductionInventoryView,
};

const RECIPE_LIST_CAPACITY: usize = 24;
const HOST_WORKBENCH_CONTAINER: ContainerId = ContainerId::new(1);

/// Latch for the in-session inventory and workbench overlays.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Resource)]
pub(super) struct ProductionHudSurfaces {
    inventory_open: bool,
    workbench_open: bool,
    cursor_slot: Option<u16>,
}

impl ProductionHudSurfaces {
    /// Returns whether a blocking inventory or workbench overlay is open.
    #[must_use]
    pub const fn inventory_open(self) -> bool {
        self.inventory_open || self.workbench_open
    }

    /// Returns whether the inventory panel itself is open.
    #[must_use]
    pub const fn inventory_panel_open(self) -> bool {
        self.inventory_open
    }

    /// Returns whether the workbench overlay is open.
    #[must_use]
    pub const fn workbench_open(self) -> bool {
        self.workbench_open
    }

    /// Returns the latched click-to-swap source slot.
    #[must_use]
    pub const fn cursor_slot(self) -> Option<u16> {
        self.cursor_slot
    }

    /// Opens or closes the inventory panel. Closing also dismisses the workbench.
    pub const fn set_inventory_open(&mut self, open: bool) {
        self.inventory_open = open;
        if open {
            self.workbench_open = false;
        } else {
            self.workbench_open = false;
            self.cursor_slot = None;
        }
    }

    /// Opens or closes the workbench overlay. Opening dismisses the inventory panel.
    pub const fn set_workbench_open(&mut self, open: bool) {
        self.workbench_open = open;
        if open {
            self.inventory_open = false;
            self.cursor_slot = None;
        }
    }

    /// Latches a click-to-swap slot. Returns a completed `(from, to)` move.
    ///
    /// The first click stores `slot`. A second click on a different slot
    /// submits that pair. Clicking the same slot clears the latch.
    pub const fn click_slot(&mut self, slot: u16) -> Option<(u16, u16)> {
        match self.cursor_slot {
            None => {
                self.cursor_slot = Some(slot);
                None
            }
            Some(from) if from == slot => {
                self.cursor_slot = None;
                None
            }
            Some(from) => {
                self.cursor_slot = None;
                Some((from, slot))
            }
        }
    }
}

/// Marker on the inspect overlay text.
#[derive(Clone, Copy, Component, Debug, Default, Eq, PartialEq)]
pub(super) struct ProductionInspectReadout;

/// Marker on the inspect overlay's missing-presentation icon swatch.
#[derive(Clone, Copy, Component, Debug, Default, Eq, PartialEq)]
pub(super) struct ProductionInspectIcon;

/// Marker on the working-set occupancy overlay.
#[derive(Clone, Copy, Component, Debug, Default, Eq, PartialEq)]
pub(super) struct ProductionWorkingSetReadout;

/// Marker on the status strip (vitality, mining, durability, view distance).
#[derive(Clone, Copy, Component, Debug, Default, Eq, PartialEq)]
pub(super) struct ProductionStatusReadout;

/// Marker on a hotbar slot node. `0..HOTBAR_SLOTS`.
#[derive(Clone, Copy, Component, Debug, Eq, PartialEq)]
pub(super) struct ProductionHotbarSlot(u16);

/// Marker on the inventory overlay root.
#[derive(Clone, Copy, Component, Debug, Default, Eq, PartialEq)]
pub(super) struct ProductionInventoryOverlay;

/// Marker on an inventory slot node. `0..INVENTORY_SLOTS`.
#[derive(Clone, Copy, Component, Debug, Eq, PartialEq)]
pub(super) struct ProductionInventorySlot(u16);

/// Marker on the workbench overlay root.
#[derive(Clone, Copy, Component, Debug, Default, Eq, PartialEq)]
pub(super) struct ProductionWorkbenchOverlay;

/// Marker on the inventory-panel hand-recipe list.
#[derive(Clone, Copy, Component, Debug, Default, Eq, PartialEq)]
pub(super) struct ProductionHandRecipeList;

/// Marker on the workbench recipe list.
#[derive(Clone, Copy, Component, Debug, Default, Eq, PartialEq)]
pub(super) struct ProductionWorkbenchRecipeList;

/// Clickable catalog recipe row.
#[derive(Clone, Component, Debug, Eq, PartialEq)]
pub(super) struct ProductionRecipeButton {
    recipe: String,
    workbench: bool,
}

/// Spawns a non-interactive crosshair, inspect, status, hotbar, and inventory.
///
/// Overlay nodes ignore picking except the inventory grid, which is display-only
/// in v1 and still passes focus through to close-on-key handling.
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
            spawn_status_readout(hud);
            spawn_working_set_readout(hud);
            spawn_hotbar(hud);
            spawn_inventory_overlay(hud);
            spawn_workbench_overlay(hud);
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
    parent
        .spawn((
            Name::new("Inspect overlay"),
            Node {
                position_type: PositionType::Absolute,
                top: Val::Px(18.0),
                left: Val::Px(18.0),
                padding: UiRect::all(Val::Px(8.0)),
                flex_direction: FlexDirection::Row,
                column_gap: Val::Px(8.0),
                align_items: AlignItems::FlexStart,
                ..Node::default()
            },
            BackgroundColor(Color::srgba(0.03, 0.04, 0.035, 0.64)),
            FocusPolicy::Pass,
            Pickable::IGNORE,
        ))
        .with_children(|overlay| {
            overlay.spawn((
                ProductionInspectIcon,
                Name::new("Inspect icon"),
                Node {
                    width: Val::Px(18.0),
                    height: Val::Px(18.0),
                    ..Node::default()
                },
                BackgroundColor(icon_swatch_color("")),
                FocusPolicy::Pass,
                Pickable::IGNORE,
            ));
            overlay.spawn((
                ProductionInspectReadout,
                Name::new("Inspect text"),
                Text::new(inspect_overlay_label(None)),
                TextFont::from_font_size(16.0),
                TextColor(Color::srgb(0.92, 0.93, 0.88)),
            ));
        });
}

fn spawn_status_readout(parent: &mut bevy::ecs::hierarchy::ChildSpawnerCommands<'_>) {
    parent.spawn((
        ProductionStatusReadout,
        Name::new("Status strip"),
        Text::new(status_line(None, None, 20, 1, 1)),
        TextFont::from_font_size(16.0),
        TextColor(Color::srgb(0.94, 0.86, 0.72)),
        Node {
            position_type: PositionType::Absolute,
            top: Val::Px(18.0),
            right: Val::Px(18.0),
            padding: UiRect::all(Val::Px(8.0)),
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
        Name::new("Working set overlay"),
        Text::new(WorkingSetDiagnosticsV1::default().overlay_line()),
        TextFont::from_font_size(14.0),
        TextColor(Color::srgb(0.78, 0.80, 0.74)),
        Node {
            position_type: PositionType::Absolute,
            left: Val::Px(18.0),
            bottom: Val::Px(78.0),
            padding: UiRect::all(Val::Px(6.0)),
            ..Node::default()
        },
        BackgroundColor(Color::srgba(0.03, 0.04, 0.035, 0.64)),
        FocusPolicy::Pass,
        Pickable::IGNORE,
    ));
}

fn spawn_hotbar(parent: &mut bevy::ecs::hierarchy::ChildSpawnerCommands<'_>) {
    parent
        .spawn((
            Name::new("Hotbar"),
            Node {
                position_type: PositionType::Absolute,
                bottom: Val::Px(18.0),
                height: Val::Px(52.0),
                flex_direction: FlexDirection::Row,
                column_gap: Val::Px(4.0),
                padding: UiRect::all(Val::Px(4.0)),
                align_items: AlignItems::Center,
                ..Node::default()
            },
            BackgroundColor(Color::srgba(0.03, 0.04, 0.035, 0.72)),
            FocusPolicy::Pass,
            Pickable::IGNORE,
        ))
        .with_children(|bar| {
            for slot in 0..HOTBAR_SLOTS {
                spawn_item_slot(
                    bar,
                    ProductionHotbarSlot(slot),
                    format!("Hotbar {slot}"),
                    44.0,
                );
            }
        });
}

fn spawn_inventory_overlay(parent: &mut bevy::ecs::hierarchy::ChildSpawnerCommands<'_>) {
    parent
        .spawn((
            ProductionInventoryOverlay,
            Name::new("Inventory overlay"),
            Node {
                position_type: PositionType::Absolute,
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                display: Display::None,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                ..Node::default()
            },
            BackgroundColor(Color::srgba(0.02, 0.03, 0.03, 0.55)),
            GlobalZIndex(80),
            FocusPolicy::Pass,
            Pickable::IGNORE,
        ))
        .with_children(|overlay| {
            overlay
                .spawn((
                    Name::new("Inventory panel"),
                    Node {
                        width: Val::Px(420.0),
                        padding: UiRect::all(Val::Px(12.0)),
                        flex_direction: FlexDirection::Column,
                        row_gap: Val::Px(8.0),
                        ..Node::default()
                    },
                    BackgroundColor(Color::srgba(0.07, 0.09, 0.08, 0.94)),
                ))
                .with_children(|panel| {
                    panel.spawn((
                        Text::new("Inventory — E closes · click slots to swap · 1-9 select hotbar"),
                        TextFont::from_font_size(16.0),
                        TextColor(Color::srgb(0.92, 0.93, 0.88)),
                    ));
                    panel
                        .spawn((
                            Name::new("Inventory grid"),
                            Node {
                                flex_direction: FlexDirection::Row,
                                flex_wrap: FlexWrap::Wrap,
                                column_gap: Val::Px(4.0),
                                row_gap: Val::Px(4.0),
                                width: Val::Px(396.0),
                                ..Node::default()
                            },
                        ))
                        .with_children(|grid| {
                            for slot in 0..u16::try_from(INVENTORY_SLOTS).unwrap_or(36) {
                                spawn_item_slot(
                                    grid,
                                    ProductionInventorySlot(slot),
                                    format!("Inventory {slot}"),
                                    40.0,
                                );
                            }
                        });
                    panel.spawn((
                        Text::new("Hand recipes"),
                        TextFont::from_font_size(14.0),
                        TextColor(Color::srgb(0.82, 0.84, 0.78)),
                    ));
                    spawn_recipe_list(panel, ProductionHandRecipeList, "Hand recipe list", false);
                });
        });
}

fn spawn_workbench_overlay(parent: &mut bevy::ecs::hierarchy::ChildSpawnerCommands<'_>) {
    parent
        .spawn((
            ProductionWorkbenchOverlay,
            Name::new("Workbench overlay"),
            Node {
                position_type: PositionType::Absolute,
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                display: Display::None,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                ..Node::default()
            },
            BackgroundColor(Color::srgba(0.02, 0.03, 0.03, 0.55)),
            GlobalZIndex(81),
            FocusPolicy::Pass,
            Pickable::IGNORE,
        ))
        .with_children(|overlay| {
            overlay
                .spawn((
                    Name::new("Workbench panel"),
                    Node {
                        width: Val::Px(420.0),
                        max_height: Val::Px(520.0),
                        padding: UiRect::all(Val::Px(12.0)),
                        flex_direction: FlexDirection::Column,
                        row_gap: Val::Px(8.0),
                        overflow: Overflow::scroll(),
                        ..Node::default()
                    },
                    BackgroundColor(Color::srgba(0.07, 0.09, 0.08, 0.94)),
                ))
                .with_children(|panel| {
                    panel.spawn((
                        Text::new("Workbench — C closes · click a recipe to craft"),
                        TextFont::from_font_size(16.0),
                        TextColor(Color::srgb(0.92, 0.93, 0.88)),
                    ));
                    spawn_recipe_list(
                        panel,
                        ProductionWorkbenchRecipeList,
                        "Workbench recipe list",
                        true,
                    );
                });
        });
}

fn spawn_recipe_list<M: Component>(
    parent: &mut bevy::ecs::hierarchy::ChildSpawnerCommands<'_>,
    marker: M,
    name: &'static str,
    workbench: bool,
) {
    parent
        .spawn((
            marker,
            Name::new(name),
            Node {
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(4.0),
                width: Val::Px(396.0),
                ..Node::default()
            },
        ))
        .with_children(|list| {
            for index in 0..RECIPE_LIST_CAPACITY {
                list.spawn((
                    Button,
                    ProductionRecipeButton {
                        recipe: String::new(),
                        workbench,
                    },
                    Name::new(format!("{name} {index}")),
                    Node {
                        width: Val::Percent(100.0),
                        height: Val::Px(28.0),
                        display: Display::None,
                        padding: UiRect::axes(Val::Px(8.0), Val::Px(4.0)),
                        align_items: AlignItems::Center,
                        ..Node::default()
                    },
                    BackgroundColor(Color::srgb(0.12, 0.16, 0.14)),
                    Pickable::IGNORE,
                ))
                .with_children(|row| {
                    row.spawn((
                        Text::new(""),
                        TextFont::from_font_size(14.0),
                        TextColor(Color::srgb(0.92, 0.93, 0.88)),
                    ));
                });
            }
        });
}

fn spawn_item_slot<M: Component>(
    parent: &mut bevy::ecs::hierarchy::ChildSpawnerCommands<'_>,
    marker: M,
    name: String,
    size: f32,
) {
    parent
        .spawn((
            marker,
            Button,
            Name::new(name),
            Node {
                width: Val::Px(size),
                height: Val::Px(size),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                ..Node::default()
            },
            BackgroundColor(empty_slot_color(false)),
            FocusPolicy::Pass,
            Pickable::IGNORE,
        ))
        .with_children(|slot| {
            slot.spawn((
                Text::new(""),
                TextFont::from_font_size(12.0),
                TextColor(Color::srgb(0.94, 0.95, 0.90)),
            ));
        });
}

/// Toggles the inventory overlay on E while the pause menu is closed.
#[allow(clippy::needless_pass_by_value)] // Bevy systems receive SystemParams by value.
pub(super) fn toggle_inventory(
    keyboard: Res<'_, ButtonInput<KeyCode>>,
    pause: Res<'_, ProductionSessionPause>,
    mut surfaces: ResMut<'_, ProductionHudSurfaces>,
) {
    if pause.is_paused() {
        return;
    }
    if keyboard.just_pressed(KeyCode::KeyE) {
        let open = !surfaces.inventory_panel_open();
        surfaces.set_inventory_open(open);
    }
}

/// Toggles the workbench overlay on C while the pause menu is closed.
#[allow(clippy::needless_pass_by_value)] // Bevy systems receive SystemParams by value.
pub(super) fn toggle_workbench(
    keyboard: Res<'_, ButtonInput<KeyCode>>,
    pause: Res<'_, ProductionSessionPause>,
    spine: Res<'_, ProductionSpine>,
    mut surfaces: ResMut<'_, ProductionHudSurfaces>,
) {
    if pause.is_paused() {
        return;
    }
    if !keyboard.just_pressed(KeyCode::KeyC) {
        return;
    }
    let open = !surfaces.workbench_open();
    if open {
        bind_host_workbench(&spine);
    }
    surfaces.set_workbench_open(open);
}

/// Opens the workbench when `SurfaceActivate` aims at a crafting workstation block.
///
/// Does not consume [`PlayerActionV1::PlaceBlock`].
#[allow(clippy::needless_pass_by_value)] // Bevy systems receive SystemParams by value.
pub(super) fn activate_workbench_from_target(
    pause: Res<'_, ProductionSessionPause>,
    action_states: Query<'_, '_, &ActionState<LeafwingPlayerAction>, With<LocalPlayerInput>>,
    spine: Res<'_, ProductionSpine>,
    mut surfaces: ResMut<'_, ProductionHudSurfaces>,
) {
    if pause.is_paused() {
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
    surfaces.set_workbench_open(true);
}

fn bind_host_workbench(spine: &ProductionSpine) {
    let _ = spine.bind_workstation(crafting_workstation(), HOST_WORKBENCH_CONTAINER);
}

fn crafting_workstation() -> WorkstationId {
    match WorkstationId::parse("latticeaxiom:workstation/crafting@1") {
        Ok(workstation) => workstation,
        Err(error) => {
            panic!("crafting workstation is a platform contract: {error}")
        }
    }
}

/// Selects hotbar slots 0..8 from Digit1..Digit9 / Numpad1..Numpad9.
#[allow(clippy::needless_pass_by_value)] // Bevy systems receive SystemParams by value.
pub(super) fn select_hotbar_from_keys(
    keyboard: Res<'_, ButtonInput<KeyCode>>,
    pause: Res<'_, ProductionSessionPause>,
    spine: Res<'_, ProductionSpine>,
) {
    if pause.is_paused() {
        return;
    }
    for (code, slot) in hotbar_digit_bindings() {
        if keyboard.just_pressed(code) {
            let _ = spine.select_hotbar_slot(slot);
            return;
        }
    }
}

const fn hotbar_digit_bindings() -> [(KeyCode, u16); 18] {
    [
        (KeyCode::Digit1, 0),
        (KeyCode::Digit2, 1),
        (KeyCode::Digit3, 2),
        (KeyCode::Digit4, 3),
        (KeyCode::Digit5, 4),
        (KeyCode::Digit6, 5),
        (KeyCode::Digit7, 6),
        (KeyCode::Digit8, 7),
        (KeyCode::Digit9, 8),
        (KeyCode::Numpad1, 0),
        (KeyCode::Numpad2, 1),
        (KeyCode::Numpad3, 2),
        (KeyCode::Numpad4, 3),
        (KeyCode::Numpad5, 4),
        (KeyCode::Numpad6, 5),
        (KeyCode::Numpad7, 6),
        (KeyCode::Numpad8, 7),
        (KeyCode::Numpad9, 8),
    ]
}

#[allow(clippy::needless_pass_by_value)] // Bevy systems receive SystemParams by value.
pub(super) fn sync_inventory_overlay(
    surfaces: Res<'_, ProductionHudSurfaces>,
    mut overlay: Query<'_, '_, &mut Node, With<ProductionInventoryOverlay>>,
) {
    let Ok(mut node) = overlay.single_mut() else {
        return;
    };
    node.display = if surfaces.inventory_panel_open() {
        Display::Flex
    } else {
        Display::None
    };
}

#[allow(clippy::needless_pass_by_value)] // Bevy systems receive SystemParams by value.
pub(super) fn sync_workbench_overlay(
    surfaces: Res<'_, ProductionHudSurfaces>,
    mut overlay: Query<'_, '_, &mut Node, With<ProductionWorkbenchOverlay>>,
) {
    let Ok(mut node) = overlay.single_mut() else {
        return;
    };
    node.display = if surfaces.workbench_open() {
        Display::Flex
    } else {
        Display::None
    };
}

#[allow(clippy::needless_pass_by_value)] // Bevy systems receive SystemParams by value.
#[allow(clippy::type_complexity)] // Slot pickable queries are one overlay mapping.
pub(super) fn sync_slot_pickable(
    pause: Res<'_, ProductionSessionPause>,
    surfaces: Res<'_, ProductionHudSurfaces>,
    mut inventory_slots: Query<
        '_,
        '_,
        &mut Pickable,
        (
            With<ProductionInventorySlot>,
            Without<ProductionHotbarSlot>,
            Without<ProductionRecipeButton>,
        ),
    >,
    mut hotbar_slots: Query<
        '_,
        '_,
        &mut Pickable,
        (
            With<ProductionHotbarSlot>,
            Without<ProductionInventorySlot>,
            Without<ProductionRecipeButton>,
        ),
    >,
    mut recipes: Query<
        '_,
        '_,
        &mut Pickable,
        (
            With<ProductionRecipeButton>,
            Without<ProductionInventorySlot>,
            Without<ProductionHotbarSlot>,
        ),
    >,
) {
    let inventory_interactive = !pause.is_paused() && surfaces.inventory_panel_open();
    let workbench_interactive = !pause.is_paused() && surfaces.workbench_open();
    let slot_pickable = if inventory_interactive {
        Pickable::default()
    } else {
        Pickable::IGNORE
    };
    for mut pickable in &mut inventory_slots {
        *pickable = slot_pickable;
    }
    for mut pickable in &mut hotbar_slots {
        *pickable = slot_pickable;
    }
    for mut pickable in &mut recipes {
        *pickable = if inventory_interactive || workbench_interactive {
            Pickable::default()
        } else {
            Pickable::IGNORE
        };
    }
}

#[allow(clippy::needless_pass_by_value)] // Bevy systems receive SystemParams by value.
#[allow(clippy::type_complexity)] // Slot button queries are one click-to-swap mapping.
pub(super) fn inventory_slot_buttons(
    pause: Res<'_, ProductionSessionPause>,
    mut surfaces: ResMut<'_, ProductionHudSurfaces>,
    spine: Res<'_, ProductionSpine>,
    inventory: Query<
        '_,
        '_,
        (&Interaction, &ProductionInventorySlot),
        (
            Changed<Interaction>,
            With<Button>,
            Without<ProductionHotbarSlot>,
        ),
    >,
    hotbar: Query<
        '_,
        '_,
        (&Interaction, &ProductionHotbarSlot),
        (
            Changed<Interaction>,
            With<Button>,
            Without<ProductionInventorySlot>,
        ),
    >,
) {
    if pause.is_paused() || !surfaces.inventory_panel_open() {
        return;
    }
    for (interaction, slot) in &inventory {
        if *interaction == Interaction::Pressed
            && let Some((from, to)) = surfaces.click_slot(slot.0)
        {
            let _ = spine.move_stack(SlotIndex::new(from), SlotIndex::new(to));
        }
    }
    for (interaction, slot) in &hotbar {
        if *interaction == Interaction::Pressed
            && let Some((from, to)) = surfaces.click_slot(slot.0)
        {
            let _ = spine.move_stack(SlotIndex::new(from), SlotIndex::new(to));
        }
    }
}

#[allow(clippy::needless_pass_by_value)] // Bevy systems receive SystemParams by value.
pub(super) fn recipe_buttons(
    pause: Res<'_, ProductionSessionPause>,
    surfaces: Res<'_, ProductionHudSurfaces>,
    spine: Res<'_, ProductionSpine>,
    buttons: Query<'_, '_, (&Interaction, &ProductionRecipeButton), Changed<Interaction>>,
) {
    if pause.is_paused() || !surfaces.inventory_open() {
        return;
    }
    for (interaction, button) in &buttons {
        if *interaction != Interaction::Pressed || button.recipe.is_empty() {
            continue;
        }
        let Ok(recipe) = RecipeId::parse(&button.recipe) else {
            continue;
        };
        let workstation = button.workbench.then_some(HOST_WORKBENCH_CONTAINER);
        let _ = spine.craft_recipe(&recipe, workstation);
    }
}

#[allow(clippy::needless_pass_by_value)] // Bevy systems receive SystemParams by value.
pub(super) fn sync_hand_recipe_list(
    spine: Res<'_, ProductionSpine>,
    surfaces: Res<'_, ProductionHudSurfaces>,
    lists: Query<
        '_,
        '_,
        &Children,
        (
            With<ProductionHandRecipeList>,
            Without<ProductionRecipeButton>,
        ),
    >,
    mut buttons: Query<
        '_,
        '_,
        (&mut ProductionRecipeButton, &mut Node, &Children),
        Without<ProductionHandRecipeList>,
    >,
    mut labels: Query<'_, '_, &mut Text>,
) {
    if !surfaces.inventory_panel_open() {
        return;
    }
    let recipes = spine.craftable_recipe_ids(None);
    let Some(children) = lists.iter().next() else {
        return;
    };
    sync_recipe_buttons(children, &mut buttons, &mut labels, &recipes, false);
}

#[allow(clippy::needless_pass_by_value)] // Bevy systems receive SystemParams by value.
pub(super) fn sync_workbench_recipe_list(
    spine: Res<'_, ProductionSpine>,
    surfaces: Res<'_, ProductionHudSurfaces>,
    lists: Query<
        '_,
        '_,
        &Children,
        (
            With<ProductionWorkbenchRecipeList>,
            Without<ProductionRecipeButton>,
        ),
    >,
    mut buttons: Query<
        '_,
        '_,
        (&mut ProductionRecipeButton, &mut Node, &Children),
        Without<ProductionWorkbenchRecipeList>,
    >,
    mut labels: Query<'_, '_, &mut Text>,
) {
    if !surfaces.workbench_open() {
        return;
    }
    let recipes = spine.craftable_recipe_ids(Some(&crafting_workstation()));
    let Some(children) = lists.iter().next() else {
        return;
    };
    sync_recipe_buttons(children, &mut buttons, &mut labels, &recipes, true);
}

fn sync_recipe_buttons<F: QueryFilter>(
    children: &Children,
    buttons: &mut Query<'_, '_, (&mut ProductionRecipeButton, &mut Node, &Children), F>,
    labels: &mut Query<'_, '_, &mut Text>,
    recipes: &[RecipeId],
    workbench: bool,
) {
    for (index, child) in children.iter().enumerate() {
        let Ok((mut button, mut node, row_children)) = buttons.get_mut(*child) else {
            continue;
        };
        if let Some(recipe) = recipes.get(index) {
            let id = recipe.as_str().to_owned();
            let label = recipe_row_label(recipe);
            button.recipe = id;
            button.workbench = workbench;
            node.display = Display::Flex;
            if let Some(label_entity) = row_children.first()
                && let Ok(mut text) = labels.get_mut(*label_entity)
                && text.0 != label
            {
                *text = Text::new(label);
            }
        } else {
            button.recipe.clear();
            node.display = Display::None;
        }
    }
}

fn recipe_row_label(recipe: &RecipeId) -> String {
    let path = recipe
        .as_str()
        .rsplit_once('/')
        .map_or(recipe.as_str(), |(_, path)| path);
    let path = path.rsplit_once('@').map_or(path, |(path, _)| path);
    let mut display = String::new();
    for segment in path.split('-').filter(|part| !part.is_empty()) {
        if !display.is_empty() {
            display.push(' ');
        }
        let mut chars = segment.chars();
        if let Some(first) = chars.next() {
            display.extend(first.to_uppercase());
            display.push_str(chars.as_str());
        }
    }
    if display.is_empty() {
        recipe.as_str().to_owned()
    } else {
        display
    }
}

#[allow(clippy::needless_pass_by_value)] // Bevy systems receive SystemParams by value.
pub(super) fn sync_production_inspect_hud(
    spine: Res<'_, ProductionSpine>,
    mut readout: Query<'_, '_, &mut Text, With<ProductionInspectReadout>>,
    mut icon: Query<'_, '_, &mut BackgroundColor, With<ProductionInspectIcon>>,
) {
    let Ok(mut text) = readout.single_mut() else {
        return;
    };
    let target = spine.current_target();
    let label = inspect_overlay_label(target.as_ref());
    if text.0 != label {
        *text = Text::new(label);
    }
    let swatch = target.as_ref().map_or_else(
        || icon_swatch_color(""),
        |hit| icon_swatch_color(&hit.block_display_icon),
    );
    if let Ok(mut color) = icon.single_mut()
        && color.0 != swatch
    {
        *color = BackgroundColor(swatch);
    }
}

fn inspect_overlay_label(target: Option<&HeadlessTargetInspectV1>) -> String {
    match target {
        Some(hit) => hit.overlay_lines(),
        None => "Inspect — no target".to_owned(),
    }
}

#[allow(clippy::needless_pass_by_value)] // Bevy systems receive SystemParams by value.
pub(super) fn sync_production_working_set_hud(
    snapshot: Res<'_, WorkingSetDiagnosticsV1>,
    mut readout: Query<'_, '_, &mut Text, With<ProductionWorkingSetReadout>>,
) {
    let Ok(mut text) = readout.single_mut() else {
        return;
    };
    let label = snapshot.overlay_line();
    if text.0 != label {
        *text = Text::new(label);
    }
}

#[allow(clippy::needless_pass_by_value)] // Bevy systems receive SystemParams by value.
pub(super) fn sync_production_status_hud(
    spine: Res<'_, ProductionSpine>,
    mut readout: Query<'_, '_, &mut Text, With<ProductionStatusReadout>>,
) {
    let Ok(mut text) = readout.single_mut() else {
        return;
    };
    let mining = match spine.last_reject() {
        Some(BlockEditRejectV1::RequiresProgress { remaining_work }) => Some(remaining_work),
        _ => None,
    };
    let durability = spine.inventory_view().and_then(|view| {
        let stack = view.selected()?;
        match stack.state() {
            latticeaxiom_gameplay::ItemStateV1::ToolDurability { remaining } => {
                Some(remaining.get())
            }
            latticeaxiom_gameplay::ItemStateV1::Plain => None,
        }
    });
    let label = status_line(
        mining,
        durability,
        20,
        spine.requested_view_distance(),
        spine.effective_view_distance(),
    );
    if text.0 != label {
        *text = Text::new(label);
    }
}

fn status_line(
    mining: Option<u32>,
    durability: Option<u32>,
    vitality: u32,
    requested_view: u32,
    effective_view: u32,
) -> String {
    let mine = mining.map_or_else(|| "Mine —".to_owned(), |left| format!("Mine {left} left"));
    let tool = durability.map_or_else(|| "Tool —".to_owned(), |left| format!("Tool {left}"));
    format!("Vitality {vitality}/20  {mine}  {tool}  View {effective_view}/{requested_view}")
}

#[allow(clippy::needless_pass_by_value)] // Bevy systems receive SystemParams by value.
pub(super) fn sync_production_hotbar_hud(
    spine: Res<'_, ProductionSpine>,
    surfaces: Res<'_, ProductionHudSurfaces>,
    mut slots: Query<'_, '_, (&ProductionHotbarSlot, &mut BackgroundColor, &mut Children)>,
    mut labels: Query<'_, '_, &mut Text>,
) {
    let view = spine.inventory_view();
    let selected = view
        .as_ref()
        .map_or(0, ProductionInventoryView::hotbar_slot);
    for (slot, mut background, children) in &mut slots {
        let selected_slot = slot.0 == selected;
        let latched = surfaces.cursor_slot() == Some(slot.0);
        let stack = view
            .as_ref()
            .and_then(|view| view.slots().get(usize::from(slot.0))?.as_ref());
        let (label, swatch) = slot_visual(spine.as_ref(), stack, slot.0);
        background.0 = if latched {
            latched_slot_color()
        } else if selected_slot {
            selected_slot_color(swatch)
        } else {
            swatch
        };
        if let Some(child) = children.first()
            && let Ok(mut text) = labels.get_mut(*child)
            && text.0 != label
        {
            *text = Text::new(label);
        }
    }
}

#[allow(clippy::needless_pass_by_value)] // Bevy systems receive SystemParams by value.
pub(super) fn sync_production_inventory_hud(
    spine: Res<'_, ProductionSpine>,
    surfaces: Res<'_, ProductionHudSurfaces>,
    mut slots: Query<
        '_,
        '_,
        (
            &ProductionInventorySlot,
            &mut BackgroundColor,
            &mut Children,
        ),
    >,
    mut labels: Query<'_, '_, &mut Text>,
) {
    let view = spine.inventory_view();
    let selected = view
        .as_ref()
        .map_or(0, ProductionInventoryView::hotbar_slot);
    for (slot, mut background, children) in &mut slots {
        let stack = view
            .as_ref()
            .and_then(|view| view.slots().get(usize::from(slot.0))?.as_ref());
        let (label, swatch) = slot_visual(spine.as_ref(), stack, slot.0);
        let selected_hotbar = slot.0 < HOTBAR_SLOTS && slot.0 == selected;
        let latched = surfaces.cursor_slot() == Some(slot.0);
        background.0 = if latched {
            latched_slot_color()
        } else if selected_hotbar {
            selected_slot_color(swatch)
        } else {
            swatch
        };
        if let Some(child) = children.first()
            && let Ok(mut text) = labels.get_mut(*child)
            && text.0 != label
        {
            *text = Text::new(label);
        }
    }
}

fn slot_visual(
    spine: &ProductionSpine,
    stack: Option<&latticeaxiom_gameplay::ItemStackV1>,
    slot: u16,
) -> (String, Color) {
    match stack {
        Some(stack) => {
            let display = spine.content_display(stack.item().as_str());
            (
                format!("{}×{}", slot + 1, stack.quantity()),
                icon_swatch_color(&display.icon),
            )
        }
        None => (String::new(), empty_slot_color(false)),
    }
}

fn empty_slot_color(selected: bool) -> Color {
    if selected {
        Color::srgb(0.28, 0.42, 0.30)
    } else {
        Color::srgb(0.12, 0.14, 0.13)
    }
}

fn selected_slot_color(fill: Color) -> Color {
    let _ = fill;
    Color::srgb(0.42, 0.62, 0.38)
}

fn latched_slot_color() -> Color {
    Color::srgb(0.62, 0.52, 0.28)
}

fn icon_swatch_color(icon: &str) -> Color {
    if icon.is_empty() {
        return Color::srgba(0.18, 0.20, 0.18, 0.85);
    }
    let mut hash = 2_166_136_261_u32;
    for byte in icon.as_bytes() {
        hash ^= u32::from(*byte);
        hash = hash.wrapping_mul(16_777_619);
    }
    let red = 0.22 + f32::from(channel(hash >> 16)) / 255.0 * 0.55;
    let green = 0.22 + f32::from(channel(hash >> 8)) / 255.0 * 0.55;
    let blue = 0.22 + f32::from(channel(hash)) / 255.0 * 0.55;
    Color::srgb(red, green, blue)
}

fn channel(value: u32) -> u8 {
    u8::try_from(value & 0xff).unwrap_or(0)
}

/// Digit keys that select hotbar slots; used by tests of the shipped binding table.
#[cfg(test)]
#[must_use]
fn hotbar_key_slot(code: KeyCode) -> Option<u16> {
    hotbar_digit_bindings()
        .into_iter()
        .find_map(|(bound, slot)| (bound == code).then_some(slot))
}

#[cfg(test)]
mod tests {
    use super::{HOTBAR_SLOTS, ProductionHudSurfaces, hotbar_key_slot, status_line};
    use bevy::input::keyboard::KeyCode;

    #[test]
    fn inventory_click_latches_then_submits_move_and_same_slot_clears() {
        let mut surfaces = ProductionHudSurfaces::default();
        assert_eq!(surfaces.cursor_slot(), None);
        assert_eq!(surfaces.click_slot(3), None);
        assert_eq!(surfaces.cursor_slot(), Some(3));
        assert_eq!(surfaces.click_slot(3), None);
        assert_eq!(surfaces.cursor_slot(), None);
        assert_eq!(surfaces.click_slot(1), None);
        assert_eq!(surfaces.click_slot(4), Some((1, 4)));
        assert_eq!(surfaces.cursor_slot(), None);
    }

    #[test]
    fn digit_and_numpad_keys_select_hotbar_slots() {
        assert_eq!(hotbar_key_slot(KeyCode::Digit1), Some(0));
        assert_eq!(hotbar_key_slot(KeyCode::Digit9), Some(HOTBAR_SLOTS - 1));
        assert_eq!(hotbar_key_slot(KeyCode::Numpad5), Some(4));
        assert_eq!(hotbar_key_slot(KeyCode::KeyE), None);
    }

    #[test]
    fn status_line_includes_vitality_mining_and_view_distance() {
        let idle = status_line(None, None, 20, 4, 2);
        assert!(idle.contains("Vitality 20/20"), "{idle}");
        assert!(idle.contains("View 2/4"), "{idle}");
        let mining = status_line(Some(7), Some(12), 20, 2, 2);
        assert!(mining.contains("Mine 7 left"), "{mining}");
        assert!(mining.contains("Tool 12"), "{mining}");
    }
}
