//! Production HUD: crosshair, inspect, status, hotbar, and inventory.

use bevy::{
    asset::Handle,
    ecs::observer::On,
    ecs::query::QueryFilter,
    image::{Image, TRANSPARENT_IMAGE_HANDLE},
    input::{
        ButtonState,
        keyboard::{Key, KeyboardInput},
    },
    prelude::{
        AlignItems, BackgroundColor, BorderColor, Children, Color, Commands, Component, Display,
        Entity, FlexDirection, FlexWrap, GlobalZIndex, JustifyContent, MessageReader, Name, Node,
        Overflow, Pickable, PositionType, Query, Res, ResMut, Resource, Text, TextColor, UiRect,
        Val, With, Without,
    },
    ui::FocusPolicy,
    ui::widget::ImageNode,
    ui_widgets::{Activate, Button, ScrollArea},
};
use latticeaxiom_client_ui::desktop_style as style;
use latticeaxiom_gameplay::{
    ContainerId, GameplayModeV1, ItemCategoryId, ItemId, RecipeId, SlotIndex, WorkstationId,
};
use latticeaxiom_player::{
    ActionState, BlockEditRejectV1, ClientInputOwnership, HeadlessTargetInspectV1,
    LeafwingPlayerAction, LocalPlayerInput,
};

use crate::ui_font::ui_text_font;

use super::{
    HOTBAR_SLOTS, INVENTORY_SLOTS, ProductionSessionPause, ProductionSpine,
    WorkingSetDiagnosticsV1, gameplay::ProductionInventoryView,
    voxel_icon::ProductionVoxelIconCache,
};

const RECIPE_LIST_CAPACITY: usize = 24;
const CATEGORY_TAB_CAPACITY: usize = 16;
const ITEM_BROWSER_COLUMNS: usize = 8;
const ITEM_BROWSER_ROWS: usize = 6;
const ITEM_BROWSER_CAPACITY: usize = ITEM_BROWSER_COLUMNS * ITEM_BROWSER_ROWS;
const ITEM_BROWSER_QUERY_CHARS: usize = 64;
const HOST_WORKBENCH_CONTAINER: ContainerId = ContainerId::new(1);

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum ProductionHudSurfaceStateV1 {
    #[default]
    Closed,
    Inventory,
    Workbench,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct ItemBrowserStateV1 {
    category: Option<ItemCategoryId>,
    query: String,
    search_focused: bool,
    first_item_index: usize,
    projection_dirty: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum ItemBrowserTransitionV1 {
    SelectCategory(Option<ItemCategoryId>),
    FocusSearch(bool),
    AppendSearch(String),
    Backspace,
    ClearSearch,
    NavigatePage { delta: i8, item_count: usize },
}

impl ItemBrowserStateV1 {
    fn apply(&mut self, transition: ItemBrowserTransitionV1) {
        self.projection_dirty = true;
        match transition {
            ItemBrowserTransitionV1::SelectCategory(category) => {
                self.category = category;
                self.first_item_index = 0;
                self.search_focused = false;
            }
            ItemBrowserTransitionV1::FocusSearch(focused) => self.search_focused = focused,
            ItemBrowserTransitionV1::AppendSearch(text) => {
                let remaining = ITEM_BROWSER_QUERY_CHARS.saturating_sub(self.query.chars().count());
                self.query.extend(
                    text.chars()
                        .filter(|value| !value.is_control())
                        .take(remaining),
                );
                self.first_item_index = 0;
            }
            ItemBrowserTransitionV1::Backspace => {
                self.query.pop();
                self.first_item_index = 0;
            }
            ItemBrowserTransitionV1::ClearSearch => {
                self.query.clear();
                self.first_item_index = 0;
            }
            ItemBrowserTransitionV1::NavigatePage { delta, item_count } => {
                let last_page_start = item_count
                    .saturating_sub(1)
                    .checked_div(ITEM_BROWSER_CAPACITY)
                    .unwrap_or(0)
                    .saturating_mul(ITEM_BROWSER_CAPACITY);
                self.first_item_index = match delta {
                    -1 => self.first_item_index.saturating_sub(ITEM_BROWSER_CAPACITY),
                    1 => self
                        .first_item_index
                        .saturating_add(ITEM_BROWSER_CAPACITY)
                        .min(last_page_start),
                    _ => self.first_item_index.min(last_page_start),
                };
            }
        }
    }
}

/// Latch for the in-session inventory and workbench overlays.
#[derive(Clone, Debug, Default, Eq, PartialEq, Resource)]
pub(super) struct ProductionHudSurfaces {
    surface: ProductionHudSurfaceStateV1,
    cursor_slot: Option<u16>,
    browser: ItemBrowserStateV1,
}

impl ProductionHudSurfaces {
    /// Returns whether a blocking inventory or workbench overlay is open.
    #[must_use]
    pub const fn inventory_open(&self) -> bool {
        !matches!(self.surface, ProductionHudSurfaceStateV1::Closed)
    }

    /// Returns whether the inventory panel itself is open.
    #[must_use]
    pub const fn inventory_panel_open(&self) -> bool {
        matches!(self.surface, ProductionHudSurfaceStateV1::Inventory)
    }

    /// Returns whether the workbench overlay is open.
    #[must_use]
    pub const fn workbench_open(&self) -> bool {
        matches!(self.surface, ProductionHudSurfaceStateV1::Workbench)
    }

    /// Returns the latched click-to-swap source slot.
    #[must_use]
    pub const fn cursor_slot(&self) -> Option<u16> {
        self.cursor_slot
    }

    /// Opens or closes the inventory panel. Closing also dismisses the workbench.
    pub const fn set_inventory_open(&mut self, open: bool) {
        if open {
            self.surface = ProductionHudSurfaceStateV1::Inventory;
            self.browser.projection_dirty = true;
        } else {
            self.surface = ProductionHudSurfaceStateV1::Closed;
            self.cursor_slot = None;
            self.browser.search_focused = false;
        }
    }

    /// Opens or closes the workbench overlay. Opening dismisses the inventory panel.
    pub const fn set_workbench_open(&mut self, open: bool) {
        if open {
            self.surface = ProductionHudSurfaceStateV1::Workbench;
            self.cursor_slot = None;
            self.browser.search_focused = false;
        } else {
            self.surface = ProductionHudSurfaceStateV1::Closed;
            self.cursor_slot = None;
            self.browser.search_focused = false;
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

    pub(super) const fn item_browser_search_focused(&self) -> bool {
        self.browser.search_focused
    }

    pub(super) fn dismiss_item_browser_search(&mut self) {
        self.browser
            .apply(ItemBrowserTransitionV1::FocusSearch(false));
    }
}

/// Marker on the inspect overlay text.
#[derive(Clone, Copy, Component, Debug, Default, Eq, PartialEq)]
pub(super) struct ProductionInspectReadout;

/// Marker on the inspect overlay's cached voxel thumbnail.
#[derive(Clone, Copy, Component, Debug, Default, Eq, PartialEq)]
pub(super) struct ProductionInspectIcon;

/// Marker on the working-set occupancy overlay.
#[derive(Clone, Copy, Component, Debug, Default, Eq, PartialEq)]
pub(super) struct ProductionWorkingSetReadout;

/// Marker on the status strip (mining and durability).
#[derive(Clone, Copy, Component, Debug, Default, Eq, PartialEq)]
pub(super) struct ProductionStatusReadout;

/// Marker on a hotbar slot node. `0..HOTBAR_SLOTS`.
#[derive(Clone, Copy, Component, Debug, Eq, PartialEq)]
pub(super) struct ProductionHotbarSlot(u16);

/// Border-only selector drawn above a hotbar/inventory swatch.
#[derive(Clone, Copy, Component, Debug, Default, Eq, PartialEq)]
pub(super) struct ProductionSlotSelector;

/// Cached cube image drawn inside a hotbar or inventory slot.
#[derive(Clone, Copy, Component, Debug, Default, Eq, PartialEq)]
pub(super) struct ProductionSlotVoxelIcon;

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

/// Cached output-item cube drawn on a recipe row.
#[derive(Clone, Copy, Component, Debug, Default, Eq, PartialEq)]
pub(super) struct ProductionRecipeVoxelIcon;

/// Clickable package-authored item-browser category tab. Empty means `All`.
#[derive(Clone, Component, Debug, Default, Eq, PartialEq)]
pub(super) struct ProductionItemCategoryButton {
    index: usize,
    category: String,
}

/// Clickable item-browser entry populated from the compiled gameplay catalog.
#[derive(Clone, Component, Debug, Default, Eq, PartialEq)]
pub(super) struct ProductionItemBrowserButton {
    index: usize,
    item: String,
}

/// Cached item cube drawn in one item-browser entry.
#[derive(Clone, Copy, Component, Debug, Default, Eq, PartialEq)]
pub(super) struct ProductionItemBrowserVoxelIcon;

/// Search field button and label container.
#[derive(Clone, Copy, Component, Debug, Default, Eq, PartialEq)]
pub(super) struct ProductionItemBrowserSearch;

/// Previous or next page control.
#[derive(Clone, Copy, Component, Debug, Eq, PartialEq)]
pub(super) struct ProductionItemBrowserPageButton(i8);

/// Page-number label.
#[derive(Clone, Copy, Component, Debug, Default, Eq, PartialEq)]
pub(super) struct ProductionItemBrowserPageLabel;

/// Creative/survival interaction hint below the item browser.
#[derive(Clone, Copy, Component, Debug, Default, Eq, PartialEq)]
pub(super) struct ProductionItemBrowserModeHint;

/// Item grid root.
#[derive(Clone, Copy, Component, Debug, Default, Eq, PartialEq)]
pub(super) struct ProductionItemBrowserGrid;

/// Spawns a non-interactive crosshair, inspect, status, hotbar, and inventory.
///
/// Overlay nodes ignore picking except the inventory grid, which is display-only
/// in v1 and still passes focus through to close-on-key handling.
#[allow(clippy::needless_pass_by_value)]
pub(super) fn spawn_production_hud(
    mut commands: Commands<'_, '_>,
    images: Option<Res<'_, crate::StructurallyValidatedComposeImages>>,
) {
    let developer_overlay = images.as_ref().is_some_and(|images| {
        super::capability_present(
            &images.graph().capability_providers,
            super::DEBUG_WORKBENCH_CAPABILITY,
        )
    });
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
            if developer_overlay {
                spawn_working_set_readout(hud);
            }
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
                Name::new("Inspect voxel icon"),
                Node {
                    width: Val::Px(38.0),
                    height: Val::Px(38.0),
                    ..Node::default()
                },
                ImageNode::default(),
                FocusPolicy::Pass,
                Pickable::IGNORE,
            ));
            overlay.spawn((
                ProductionInspectReadout,
                Name::new("Inspect text"),
                Text::new(inspect_overlay_label(None)),
                ui_text_font(16.0),
                TextColor(Color::srgb(0.92, 0.93, 0.88)),
            ));
        });
}

fn spawn_status_readout(parent: &mut bevy::ecs::hierarchy::ChildSpawnerCommands<'_>) {
    parent.spawn((
        ProductionStatusReadout,
        Name::new("Status strip"),
        Text::new(status_line("Mine —", None)),
        ui_text_font(16.0),
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
        ui_text_font(14.0),
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

#[allow(clippy::too_many_lines)] // Keep the one inventory hierarchy visible as a layout unit.
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
                        width: Val::Px(860.0),
                        max_width: Val::Percent(94.0),
                        max_height: Val::Percent(94.0),
                        padding: UiRect::all(Val::Px(14.0)),
                        flex_direction: FlexDirection::Column,
                        row_gap: Val::Px(10.0),
                        ..Node::default()
                    },
                    BackgroundColor(Color::srgba(0.07, 0.09, 0.08, 0.94)),
                ))
                .with_children(|panel| {
                    panel.spawn((
                        Text::new(
                            "Inventory · E closes · click two slots to move · 1–9 or wheel selects",
                        ),
                        ui_text_font(17.0),
                        TextColor(Color::srgb(0.92, 0.93, 0.88)),
                    ));
                    panel
                        .spawn((
                            Name::new("Inventory and item browser"),
                            Node {
                                flex_direction: FlexDirection::Row,
                                column_gap: Val::Px(18.0),
                                align_items: AlignItems::FlexStart,
                                ..Node::default()
                            },
                        ))
                        .with_children(|content| {
                            content
                                .spawn((
                                    Name::new("Backpack pane"),
                                    Node {
                                        width: Val::Px(396.0),
                                        flex_direction: FlexDirection::Column,
                                        row_gap: Val::Px(8.0),
                                        ..Node::default()
                                    },
                                ))
                                .with_children(|backpack| {
                                    backpack.spawn((
                                        Text::new("Backpack"),
                                        ui_text_font(14.0),
                                        TextColor(style::MUTED),
                                    ));
                                    backpack
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
                                            for slot in
                                                0..u16::try_from(INVENTORY_SLOTS).unwrap_or(36)
                                            {
                                                spawn_item_slot(
                                                    grid,
                                                    ProductionInventorySlot(slot),
                                                    format!("Inventory {slot}"),
                                                    40.0,
                                                );
                                            }
                                        });
                                    backpack.spawn((
                                        Text::new("Hand recipes"),
                                        ui_text_font(14.0),
                                        TextColor(style::MUTED),
                                    ));
                                    spawn_recipe_list(
                                        backpack,
                                        ProductionHandRecipeList,
                                        "Hand recipe list",
                                        false,
                                    );
                                });
                            spawn_item_browser(content);
                        });
                });
        });
}

#[allow(clippy::too_many_lines)] // Keep the bounded browser hierarchy visible as a layout unit.
fn spawn_item_browser(parent: &mut bevy::ecs::hierarchy::ChildSpawnerCommands<'_>) {
    parent
        .spawn((
            Name::new("Item browser pane"),
            Node {
                width: Val::Px(390.0),
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(7.0),
                ..Node::default()
            },
            BackgroundColor(Color::srgba(0.045, 0.06, 0.052, 0.92)),
        ))
        .with_children(|browser| {
            browser.spawn((
                Text::new("Items"),
                ui_text_font(14.0),
                TextColor(style::MUTED),
                Node {
                    margin: UiRect::axes(Val::Px(8.0), Val::Px(0.0)),
                    ..Node::default()
                },
            ));
            browser
                .spawn((
                    Name::new("Item category tabs"),
                    Node {
                        width: Val::Percent(100.0),
                        flex_direction: FlexDirection::Row,
                        flex_wrap: FlexWrap::Wrap,
                        column_gap: Val::Px(3.0),
                        row_gap: Val::Px(3.0),
                        padding: UiRect::horizontal(Val::Px(6.0)),
                        ..Node::default()
                    },
                ))
                .with_children(|tabs| {
                    for index in 0..CATEGORY_TAB_CAPACITY {
                        tabs.spawn((
                            Button,
                            ProductionItemCategoryButton {
                                index,
                                category: String::new(),
                            },
                            Name::new(format!("Item category tab {index}")),
                            Node {
                                height: Val::Px(28.0),
                                min_width: Val::Px(42.0),
                                display: Display::None,
                                padding: UiRect::axes(Val::Px(7.0), Val::Px(4.0)),
                                align_items: AlignItems::Center,
                                justify_content: JustifyContent::Center,
                                ..Node::default()
                            },
                            BackgroundColor(Color::srgb(0.12, 0.16, 0.14)),
                            Pickable::default(),
                        ))
                        .with_children(|tab| {
                            tab.spawn((
                                Text::new(""),
                                ui_text_font(11.0),
                                TextColor(Color::srgb(0.92, 0.93, 0.88)),
                            ));
                        });
                    }
                });
            browser
                .spawn((
                    ProductionItemBrowserGrid,
                    Name::new("JEI-style item grid"),
                    Node {
                        width: Val::Px(364.0),
                        min_height: Val::Px(272.0),
                        flex_direction: FlexDirection::Row,
                        flex_wrap: FlexWrap::Wrap,
                        column_gap: Val::Px(4.0),
                        row_gap: Val::Px(4.0),
                        margin: UiRect::horizontal(Val::Px(6.0)),
                        ..Node::default()
                    },
                ))
                .with_children(|grid| {
                    for index in 0..ITEM_BROWSER_CAPACITY {
                        grid.spawn((
                            Button,
                            ProductionItemBrowserButton {
                                index,
                                item: String::new(),
                            },
                            Name::new(format!("Item browser entry {index}")),
                            Node {
                                width: Val::Px(42.0),
                                height: Val::Px(42.0),
                                display: Display::None,
                                align_items: AlignItems::Center,
                                justify_content: JustifyContent::Center,
                                overflow: Overflow::clip(),
                                ..Node::default()
                            },
                            BackgroundColor(empty_slot_color(false)),
                            Pickable::default(),
                        ))
                        .with_children(|entry| {
                            entry.spawn((
                                ProductionItemBrowserVoxelIcon,
                                Name::new("Item browser voxel icon"),
                                Node {
                                    position_type: PositionType::Absolute,
                                    left: Val::Px(3.0),
                                    top: Val::Px(3.0),
                                    width: Val::Px(36.0),
                                    height: Val::Px(36.0),
                                    ..Node::default()
                                },
                                ImageNode::default(),
                                Pickable::IGNORE,
                            ));
                            entry.spawn((
                                Text::new(""),
                                ui_text_font(9.0),
                                TextColor(Color::srgb(0.96, 0.97, 0.92)),
                                Node {
                                    position_type: PositionType::Absolute,
                                    right: Val::Px(2.0),
                                    bottom: Val::Px(1.0),
                                    ..Node::default()
                                },
                            ));
                        });
                    }
                });
            browser
                .spawn((
                    Name::new("Item browser page controls"),
                    Node {
                        width: Val::Percent(100.0),
                        flex_direction: FlexDirection::Row,
                        justify_content: JustifyContent::Center,
                        align_items: AlignItems::Center,
                        column_gap: Val::Px(10.0),
                        ..Node::default()
                    },
                ))
                .with_children(|pages| {
                    spawn_page_button(pages, -1, "‹");
                    pages.spawn((
                        ProductionItemBrowserPageLabel,
                        Text::new("1 / 1"),
                        ui_text_font(12.0),
                        TextColor(style::MUTED),
                    ));
                    spawn_page_button(pages, 1, "›");
                });
            browser
                .spawn((
                    Button,
                    ProductionItemBrowserSearch,
                    Name::new("Item browser search"),
                    Node {
                        width: Val::Px(364.0),
                        height: Val::Px(30.0),
                        margin: UiRect::axes(Val::Px(6.0), Val::Px(2.0)),
                        padding: UiRect::axes(Val::Px(8.0), Val::Px(5.0)),
                        align_items: AlignItems::Center,
                        ..Node::default()
                    },
                    BackgroundColor(Color::srgb(0.09, 0.12, 0.105)),
                    Pickable::default(),
                ))
                .with_children(|search| {
                    search.spawn((
                        Text::new("Search items or #tags"),
                        ui_text_font(12.0),
                        TextColor(Color::srgb(0.68, 0.72, 0.66)),
                    ));
                });
            browser.spawn((
                ProductionItemBrowserModeHint,
                Text::new("Items follow this world's gameplay rules"),
                ui_text_font(11.0),
                TextColor(Color::srgb(0.63, 0.68, 0.61)),
                Node {
                    margin: UiRect::horizontal(Val::Px(8.0)),
                    ..Node::default()
                },
            ));
        });
}

fn spawn_page_button(
    parent: &mut bevy::ecs::hierarchy::ChildSpawnerCommands<'_>,
    direction: i8,
    label: &'static str,
) {
    parent
        .spawn((
            Button,
            ProductionItemBrowserPageButton(direction),
            Node {
                width: Val::Px(28.0),
                height: Val::Px(24.0),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                ..Node::default()
            },
            BackgroundColor(Color::srgb(0.12, 0.16, 0.14)),
            Pickable::default(),
        ))
        .with_children(|button| {
            button.spawn((
                Text::new(label),
                ui_text_font(14.0),
                TextColor(Color::srgb(0.92, 0.93, 0.88)),
            ));
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
                    ScrollArea,
                    BackgroundColor(Color::srgba(0.07, 0.09, 0.08, 0.94)),
                ))
                .with_children(|panel| {
                    panel.spawn((
                        Text::new("Workbench — C closes · click a recipe to craft"),
                        ui_text_font(16.0),
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
    let mut list = parent.spawn((
        marker,
        Name::new(name),
        Node {
            flex_direction: FlexDirection::Column,
            row_gap: Val::Px(4.0),
            width: Val::Px(396.0),
            height: if workbench { Val::Auto } else { Val::Px(224.0) },
            overflow: if workbench {
                Overflow::DEFAULT
            } else {
                Overflow::scroll_y()
            },
            ..Node::default()
        },
    ));
    if !workbench {
        list.insert(ScrollArea);
    }
    list.with_children(|list| {
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
                    height: Val::Px(36.0),
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
                    ProductionRecipeVoxelIcon,
                    Name::new("Recipe output voxel icon"),
                    Node {
                        width: Val::Px(24.0),
                        height: Val::Px(24.0),
                        margin: UiRect::right(Val::Px(6.0)),
                        ..Node::default()
                    },
                    ImageNode::default(),
                    Pickable::IGNORE,
                ));
                row.spawn((
                    Text::new(""),
                    ui_text_font(14.0),
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
                ProductionSlotVoxelIcon,
                Name::new("Slot voxel icon"),
                Node {
                    position_type: PositionType::Absolute,
                    left: Val::Px(3.0),
                    top: Val::Px(3.0),
                    width: Val::Px((size - 6.0).max(1.0)),
                    height: Val::Px((size - 6.0).max(1.0)),
                    ..Node::default()
                },
                ImageNode::default(),
                Pickable::IGNORE,
            ));
            slot.spawn((
                Text::new(""),
                ui_text_font(12.0),
                TextColor(style::TEXT),
                Node {
                    position_type: PositionType::Absolute,
                    right: Val::Px(2.0),
                    bottom: Val::Px(1.0),
                    ..Node::default()
                },
            ));
            slot.spawn((
                ProductionSlotSelector,
                Name::new("Slot selector"),
                Node {
                    position_type: PositionType::Absolute,
                    left: Val::Px(0.0),
                    top: Val::Px(0.0),
                    width: Val::Percent(100.0),
                    height: Val::Percent(100.0),
                    border: UiRect::all(Val::Px(2.0)),
                    display: Display::None,
                    ..Node::default()
                },
                BorderColor::all(Color::NONE),
                Pickable::IGNORE,
            ));
        });
}

/// Opens the workbench when `SurfaceActivate` aims at a crafting workstation block.
///
/// Does not consume [`PlayerActionV1::PlaceBlock`].
#[allow(clippy::needless_pass_by_value)] // Bevy systems receive SystemParams by value.
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

#[allow(clippy::needless_pass_by_value)] // Bevy observers receive SystemParams by value.
pub(super) fn inventory_slot_activated(
    activate: On<'_, '_, Activate>,
    pause: Res<'_, ProductionSessionPause>,
    mut surfaces: ResMut<'_, ProductionHudSurfaces>,
    spine: Res<'_, ProductionSpine>,
    inventory: Query<
        '_,
        '_,
        &ProductionInventorySlot,
        (With<Button>, Without<ProductionHotbarSlot>),
    >,
    hotbar: Query<'_, '_, &ProductionHotbarSlot, (With<Button>, Without<ProductionInventorySlot>)>,
) {
    if pause.is_paused() || !surfaces.inventory_panel_open() {
        return;
    }
    let slot = inventory
        .get(activate.entity)
        .map(|slot| slot.0)
        .ok()
        .or_else(|| hotbar.get(activate.entity).ok().map(|slot| slot.0));
    if let Some(slot) = slot
        && let Some((from, to)) = surfaces.click_slot(slot)
    {
        let _ = spine.move_stack(SlotIndex::new(from), SlotIndex::new(to));
    }
}

#[allow(clippy::needless_pass_by_value)] // Bevy observers receive SystemParams by value.
pub(super) fn recipe_activated(
    activate: On<'_, '_, Activate>,
    pause: Res<'_, ProductionSessionPause>,
    surfaces: Res<'_, ProductionHudSurfaces>,
    spine: Res<'_, ProductionSpine>,
    buttons: Query<'_, '_, &ProductionRecipeButton, With<Button>>,
) {
    if pause.is_paused() || !surfaces.inventory_open() {
        return;
    }
    let Ok(button) = buttons.get(activate.entity) else {
        return;
    };
    if button.recipe.is_empty() {
        return;
    }
    let Ok(recipe) = RecipeId::parse(&button.recipe) else {
        return;
    };
    let workstation = button.workbench.then_some(HOST_WORKBENCH_CONTAINER);
    let _receipt = spine.craft_recipe(&recipe, workstation);
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ItemBrowserEntryV1 {
    item: ItemId,
    name: String,
}

#[allow(clippy::needless_pass_by_value)] // Bevy observers receive SystemParams by value.
#[allow(clippy::too_many_arguments)] // Queries preserve disjoint typed widget boundaries.
pub(super) fn item_browser_activated(
    activate: On<'_, '_, Activate>,
    pause: Res<'_, ProductionSessionPause>,
    mut surfaces: ResMut<'_, ProductionHudSurfaces>,
    spine: Res<'_, ProductionSpine>,
    categories: Query<'_, '_, &ProductionItemCategoryButton, With<Button>>,
    items: Query<'_, '_, &ProductionItemBrowserButton, With<Button>>,
    search: Query<'_, '_, (), (With<ProductionItemBrowserSearch>, With<Button>)>,
    pages: Query<'_, '_, &ProductionItemBrowserPageButton, With<Button>>,
) {
    if pause.is_paused() || !surfaces.inventory_panel_open() {
        return;
    }
    if let Ok(button) = categories.get(activate.entity) {
        let category = if button.category.is_empty() {
            None
        } else {
            ItemCategoryId::parse(&button.category).ok()
        };
        surfaces
            .browser
            .apply(ItemBrowserTransitionV1::SelectCategory(category));
        return;
    }
    if search.contains(activate.entity) {
        surfaces
            .browser
            .apply(ItemBrowserTransitionV1::FocusSearch(true));
        return;
    }
    if let Ok(button) = pages.get(activate.entity) {
        let Some(catalog) = spine.gameplay_catalog() else {
            return;
        };
        let item_count = filtered_item_browser_entries(&spine, &catalog, &surfaces.browser).len();
        surfaces
            .browser
            .apply(ItemBrowserTransitionV1::NavigatePage {
                delta: button.0,
                item_count,
            });
        return;
    }
    let Ok(button) = items.get(activate.entity) else {
        return;
    };
    surfaces
        .browser
        .apply(ItemBrowserTransitionV1::FocusSearch(false));
    if spine.gameplay_mode() == Some(GameplayModeV1::Creative)
        && let Ok(item) = ItemId::parse(&button.item)
    {
        let _ = spine.creative_pick_item(item);
    }
}

#[allow(clippy::needless_pass_by_value)] // Bevy systems receive SystemParams by value.
pub(super) fn capture_item_browser_search(
    mut keyboard: MessageReader<'_, '_, KeyboardInput>,
    ownership: Res<'_, ClientInputOwnership>,
    mut surfaces: ResMut<'_, ProductionHudSurfaces>,
) {
    if *ownership == ClientInputOwnership::Released
        || !surfaces.inventory_panel_open()
        || !surfaces.item_browser_search_focused()
    {
        keyboard.clear();
        return;
    }
    for event in keyboard.read() {
        if event.state != ButtonState::Pressed {
            continue;
        }
        match &event.logical_key {
            Key::Backspace => surfaces.browser.apply(ItemBrowserTransitionV1::Backspace),
            Key::Delete => surfaces.browser.apply(ItemBrowserTransitionV1::ClearSearch),
            Key::Enter | Key::Escape => surfaces
                .browser
                .apply(ItemBrowserTransitionV1::FocusSearch(false)),
            _ => {
                if let Some(text) = &event.text {
                    surfaces
                        .browser
                        .apply(ItemBrowserTransitionV1::AppendSearch(text.to_string()));
                }
            }
        }
    }
}

#[allow(clippy::needless_pass_by_value)] // Bevy systems receive SystemParams by value.
#[allow(clippy::type_complexity)] // One bounded item-browser projection updates related widgets.
#[allow(clippy::too_many_arguments)] // Queries preserve disjoint typed widget boundaries.
#[allow(clippy::too_many_lines)] // One dirty projection keeps browser widget updates atomic.
pub(super) fn sync_item_browser(
    spine: Res<'_, ProductionSpine>,
    voxel_icons: Option<Res<'_, ProductionVoxelIconCache>>,
    mut surfaces: ResMut<'_, ProductionHudSurfaces>,
    mut category_buttons: Query<
        '_,
        '_,
        (
            &mut ProductionItemCategoryButton,
            &mut Node,
            &mut BackgroundColor,
            &Children,
        ),
        (
            With<Button>,
            Without<ProductionItemBrowserButton>,
            Without<ProductionItemBrowserSearch>,
        ),
    >,
    mut item_buttons: Query<
        '_,
        '_,
        (&mut ProductionItemBrowserButton, &mut Node, &Children),
        (With<Button>, Without<ProductionItemCategoryButton>),
    >,
    mut item_icons: Query<'_, '_, &mut ImageNode, With<ProductionItemBrowserVoxelIcon>>,
    mut search: Query<
        '_,
        '_,
        (&mut BackgroundColor, &Children),
        (
            With<ProductionItemBrowserSearch>,
            Without<ProductionItemBrowserButton>,
            Without<ProductionItemCategoryButton>,
        ),
    >,
    page_label: Query<'_, '_, Entity, With<ProductionItemBrowserPageLabel>>,
    mode_hint: Query<'_, '_, Entity, With<ProductionItemBrowserModeHint>>,
    mut labels: Query<'_, '_, &mut Text>,
) {
    if !surfaces.inventory_panel_open() || !surfaces.browser.projection_dirty {
        return;
    }
    let Some(catalog) = spine.gameplay_catalog() else {
        return;
    };
    let mut categories = catalog.categories().values().collect::<Vec<_>>();
    categories
        .sort_by(|left, right| (left.sort_order, &left.id).cmp(&(right.sort_order, &right.id)));
    for (mut button, mut node, mut background, children) in &mut category_buttons {
        let row = if button.index == 0 {
            Some((None, "All"))
        } else {
            categories
                .get(button.index - 1)
                .map(|category| (Some(&category.id), category.display_name.as_str()))
        };
        let Some((category, label)) = row else {
            button.category.clear();
            node.display = Display::None;
            continue;
        };
        button.category = category.map_or_else(String::new, ToString::to_string);
        node.display = Display::Flex;
        background.0 = if surfaces.browser.category.as_ref() == category {
            selected_slot_color()
        } else {
            Color::srgb(0.12, 0.16, 0.14)
        };
        set_child_text(children, label, &mut labels);
    }

    let entries = filtered_item_browser_entries(&spine, &catalog, &surfaces.browser);
    surfaces
        .browser
        .apply(ItemBrowserTransitionV1::NavigatePage {
            delta: 0,
            item_count: entries.len(),
        });
    let first = surfaces.browser.first_item_index;
    for (mut button, mut node, children) in &mut item_buttons {
        let Some(entry) = entries.get(first.saturating_add(button.index)) else {
            button.item.clear();
            node.display = Display::None;
            continue;
        };
        button.item = entry.item.to_string();
        node.display = Display::Flex;
        set_child_image(
            children,
            voxel_icons.as_deref().map(|icons| icons.item(&entry.item)),
            &mut item_icons,
        );
        let label = compact_item_label(&entry.name);
        set_child_text(children, &label, &mut labels);
    }

    let page_count = entries.len().div_ceil(ITEM_BROWSER_CAPACITY).max(1);
    let page = first / ITEM_BROWSER_CAPACITY + 1;
    if let Ok(entity) = page_label.single()
        && let Ok(mut text) = labels.get_mut(entity)
    {
        let label = format!("{page} / {page_count}");
        if text.0 != label {
            *text = Text::new(label);
        }
    }
    if let Ok((mut background, children)) = search.single_mut() {
        background.0 = if surfaces.browser.search_focused {
            Color::srgb(0.18, 0.23, 0.20)
        } else {
            Color::srgb(0.09, 0.12, 0.105)
        };
        let label = if surfaces.browser.query.is_empty() {
            "Search items or #tags".to_owned()
        } else if surfaces.browser.search_focused {
            format!("> {}_", surfaces.browser.query)
        } else {
            format!("> {}", surfaces.browser.query)
        };
        set_child_text(children, &label, &mut labels);
    }
    if let Ok(entity) = mode_hint.single()
        && let Ok(mut text) = labels.get_mut(entity)
    {
        let label = if spine.gameplay_mode() == Some(GameplayModeV1::Creative) {
            "Creative: click an item to fill the selected hotbar slot"
        } else {
            "Survival: browse and search registered items"
        };
        if text.0 != label {
            *text = Text::new(label);
        }
    }
    surfaces.browser.projection_dirty = false;
}

fn filtered_item_browser_entries(
    spine: &ProductionSpine,
    catalog: &latticeaxiom_gameplay::GameplayCatalog,
    state: &ItemBrowserStateV1,
) -> Vec<ItemBrowserEntryV1> {
    let query = state.query.to_lowercase();
    let tokens = query.split_whitespace().collect::<Vec<_>>();
    catalog
        .items()
        .keys()
        .filter(|item| {
            state
                .category
                .as_ref()
                .is_none_or(|category| catalog.category_for_item(item) == Some(category))
        })
        .filter_map(|item| {
            let display = spine.content_display(item.as_str());
            item_matches_search(catalog, item, &display.name, &tokens).then(|| ItemBrowserEntryV1 {
                item: item.clone(),
                name: display.name,
            })
        })
        .collect()
}

fn item_matches_search(
    catalog: &latticeaxiom_gameplay::GameplayCatalog,
    item: &ItemId,
    display_name: &str,
    tokens: &[&str],
) -> bool {
    let item_id = item.as_str().to_lowercase();
    let display_name = display_name.to_lowercase();
    let tags = catalog
        .tags_for_item(item)
        .into_iter()
        .map(|tag| tag.as_str().to_lowercase())
        .collect::<Vec<_>>();
    tokens.iter().all(|token| {
        if let Some(tag) = token.strip_prefix('#') {
            !tag.is_empty() && tags.iter().any(|candidate| candidate.contains(tag))
        } else {
            item_id.contains(token)
                || display_name.contains(token)
                || tags.iter().any(|candidate| candidate.contains(token))
        }
    })
}

fn compact_item_label(display_name: &str) -> String {
    let words = display_name
        .split(|character: char| character == '-' || character.is_whitespace())
        .filter(|word| !word.is_empty())
        .collect::<Vec<_>>();
    if words.len() > 1 {
        return words
            .iter()
            .filter_map(|word| word.chars().next())
            .take(4)
            .flat_map(char::to_uppercase)
            .collect();
    }
    display_name.chars().take(6).collect()
}

#[allow(clippy::needless_pass_by_value)] // Bevy systems receive SystemParams by value.
pub(super) fn sync_hand_recipe_list(
    spine: Res<'_, ProductionSpine>,
    voxel_icons: Option<Res<'_, ProductionVoxelIconCache>>,
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
    mut recipe_icons: Query<'_, '_, &mut ImageNode, With<ProductionRecipeVoxelIcon>>,
    mut labels: Query<'_, '_, &mut Text>,
) {
    if !surfaces.inventory_panel_open() {
        return;
    }
    let recipes = spine
        .recipe_inspect(None)
        .into_iter()
        .filter(|fragment| fragment.craftable() && fragment.workstation().is_none())
        .map(|fragment| fragment.recipe().clone())
        .collect::<Vec<_>>();
    let Some(children) = lists.iter().next() else {
        return;
    };
    let catalog = spine.gameplay_catalog();
    sync_recipe_buttons(
        children,
        &mut buttons,
        &mut recipe_icons,
        &mut labels,
        &recipes,
        RecipeProjectionV1 {
            catalog: catalog.as_ref(),
            voxel_icons: voxel_icons.as_deref(),
            workbench: false,
        },
    );
}

#[allow(clippy::needless_pass_by_value)] // Bevy systems receive SystemParams by value.
pub(super) fn sync_workbench_recipe_list(
    spine: Res<'_, ProductionSpine>,
    voxel_icons: Option<Res<'_, ProductionVoxelIconCache>>,
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
    mut recipe_icons: Query<'_, '_, &mut ImageNode, With<ProductionRecipeVoxelIcon>>,
    mut labels: Query<'_, '_, &mut Text>,
) {
    if !surfaces.workbench_open() {
        return;
    }
    let recipes = spine
        .recipe_inspect(Some(&crafting_workstation()))
        .into_iter()
        .filter(latticeaxiom_gameplay::RecipeInspectV1::craftable)
        .map(|fragment| fragment.recipe().clone())
        .collect::<Vec<_>>();
    let Some(children) = lists.iter().next() else {
        return;
    };
    let catalog = spine.gameplay_catalog();
    sync_recipe_buttons(
        children,
        &mut buttons,
        &mut recipe_icons,
        &mut labels,
        &recipes,
        RecipeProjectionV1 {
            catalog: catalog.as_ref(),
            voxel_icons: voxel_icons.as_deref(),
            workbench: true,
        },
    );
}

#[derive(Clone, Copy)]
struct RecipeProjectionV1<'a> {
    catalog: Option<&'a latticeaxiom_gameplay::GameplayCatalog>,
    voxel_icons: Option<&'a ProductionVoxelIconCache>,
    workbench: bool,
}

fn sync_recipe_buttons<F: QueryFilter>(
    children: &Children,
    buttons: &mut Query<'_, '_, (&mut ProductionRecipeButton, &mut Node, &Children), F>,
    recipe_icons: &mut Query<'_, '_, &mut ImageNode, With<ProductionRecipeVoxelIcon>>,
    labels: &mut Query<'_, '_, &mut Text>,
    recipes: &[RecipeId],
    projection: RecipeProjectionV1<'_>,
) {
    for (index, child) in children.iter().enumerate() {
        let Ok((mut button, mut node, row_children)) = buttons.get_mut(*child) else {
            continue;
        };
        if let Some(recipe) = recipes.get(index) {
            let id = recipe.as_str().to_owned();
            let label = recipe_row_label(recipe);
            button.recipe = id;
            button.workbench = projection.workbench;
            node.display = Display::Flex;
            let output = projection
                .catalog
                .and_then(|catalog| catalog.recipe(recipe).map(|recipe| (catalog, recipe)))
                .and_then(|(catalog, recipe)| catalog.bindings().get(&recipe.output.role));
            set_child_image(
                row_children,
                output.and_then(|item| projection.voxel_icons.map(|icons| icons.item(item))),
                recipe_icons,
            );
            set_child_text(row_children, &label, labels);
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
    voxel_icons: Option<Res<'_, ProductionVoxelIconCache>>,
    mut readout: Query<'_, '_, &mut Text, With<ProductionInspectReadout>>,
    mut icon: Query<'_, '_, &mut ImageNode, With<ProductionInspectIcon>>,
) {
    let Ok(mut text) = readout.single_mut() else {
        return;
    };
    let target = spine.current_target();
    let label = inspect_overlay_label(target.as_ref());
    if text.0 != label {
        *text = Text::new(label);
    }
    if let Ok(mut image) = icon.single_mut() {
        let desired = target.as_ref().and_then(|hit| {
            voxel_icons
                .as_deref()
                .map(|icons| icons.block(&hit.block_id))
        });
        set_image_node(&mut image, desired);
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
    let reject = spine.last_reject();
    let mining = mining_status_line(reject.as_ref());
    let durability = spine.inventory_view().and_then(|view| {
        let stack = view.selected()?;
        match stack.state() {
            latticeaxiom_gameplay::ItemStateV1::ToolDurability { remaining } => {
                Some(remaining.get())
            }
            latticeaxiom_gameplay::ItemStateV1::Plain => None,
        }
    });
    let label = status_line(&mining, durability);
    if text.0 != label {
        *text = Text::new(label);
    }
}

fn status_line(mining: &str, durability: Option<u32>) -> String {
    let tool = durability.map_or_else(|| "Tool —".to_owned(), |left| format!("Tool {left}"));
    format!("{mining}  {tool}")
}

fn mining_status_line(reject: Option<&BlockEditRejectV1>) -> String {
    match reject {
        Some(BlockEditRejectV1::RequiresTool { required }) => {
            format!("Mine needs {required}")
        }
        Some(BlockEditRejectV1::ToolBroken) => "Mine tool broken".to_owned(),
        Some(BlockEditRejectV1::NotBreakable) => "Mine unbreakable".to_owned(),
        _ => "Mine —".to_owned(),
    }
}

#[allow(clippy::needless_pass_by_value)] // Bevy systems receive SystemParams by value.
pub(super) fn sync_production_hotbar_hud(
    spine: Res<'_, ProductionSpine>,
    voxel_icons: Option<Res<'_, ProductionVoxelIconCache>>,
    surfaces: Res<'_, ProductionHudSurfaces>,
    slots: Query<'_, '_, (&ProductionHotbarSlot, &Children)>,
    mut slot_icons: Query<'_, '_, &mut ImageNode, With<ProductionSlotVoxelIcon>>,
    mut labels: Query<'_, '_, &mut Text>,
    mut selectors: Query<'_, '_, (&mut Node, &mut BorderColor), With<ProductionSlotSelector>>,
) {
    let view = spine.inventory_view();
    let selected = view
        .as_ref()
        .map_or(0, ProductionInventoryView::hotbar_slot);
    for (slot, children) in &slots {
        let selected_slot = slot.0 == selected;
        let latched = surfaces.cursor_slot() == Some(slot.0);
        let stack = view
            .as_ref()
            .and_then(|view| view.slots().get(usize::from(slot.0))?.as_ref());
        let label = stack.map_or_else(String::new, |stack| stack.quantity().to_string());
        set_child_image(
            children,
            stack.and_then(|stack| voxel_icons.as_deref().map(|icons| icons.item(stack.item()))),
            &mut slot_icons,
        );
        set_slot_selector(
            children,
            latched || selected_slot,
            if latched {
                latched_slot_color()
            } else {
                selected_slot_color()
            },
            &mut selectors,
        );
        set_child_text(children, &label, &mut labels);
    }
}

#[allow(clippy::needless_pass_by_value)] // Bevy systems receive SystemParams by value.
pub(super) fn sync_production_inventory_hud(
    spine: Res<'_, ProductionSpine>,
    voxel_icons: Option<Res<'_, ProductionVoxelIconCache>>,
    surfaces: Res<'_, ProductionHudSurfaces>,
    slots: Query<'_, '_, (&ProductionInventorySlot, &Children)>,
    mut slot_icons: Query<'_, '_, &mut ImageNode, With<ProductionSlotVoxelIcon>>,
    mut labels: Query<'_, '_, &mut Text>,
    mut selectors: Query<'_, '_, (&mut Node, &mut BorderColor), With<ProductionSlotSelector>>,
) {
    let view = spine.inventory_view();
    let selected = view
        .as_ref()
        .map_or(0, ProductionInventoryView::hotbar_slot);
    for (slot, children) in &slots {
        let stack = view
            .as_ref()
            .and_then(|view| view.slots().get(usize::from(slot.0))?.as_ref());
        let label = stack.map_or_else(String::new, |stack| stack.quantity().to_string());
        let selected_hotbar = slot.0 < HOTBAR_SLOTS && slot.0 == selected;
        let latched = surfaces.cursor_slot() == Some(slot.0);
        set_child_image(
            children,
            stack.and_then(|stack| voxel_icons.as_deref().map(|icons| icons.item(stack.item()))),
            &mut slot_icons,
        );
        set_slot_selector(
            children,
            latched || selected_hotbar,
            if latched {
                latched_slot_color()
            } else {
                selected_slot_color()
            },
            &mut selectors,
        );
        set_child_text(children, &label, &mut labels);
    }
}

fn set_slot_selector(
    children: &Children,
    visible: bool,
    color: Color,
    selectors: &mut Query<'_, '_, (&mut Node, &mut BorderColor), With<ProductionSlotSelector>>,
) {
    for child in children {
        if let Ok((mut node, mut border)) = selectors.get_mut(*child) {
            node.display = if visible {
                Display::Flex
            } else {
                Display::None
            };
            *border = BorderColor::all(if visible { color } else { Color::NONE });
        }
    }
}

fn set_child_image<F: QueryFilter>(
    children: &Children,
    desired: Option<&Handle<Image>>,
    images: &mut Query<'_, '_, &mut ImageNode, F>,
) {
    for child in children {
        if let Ok(mut image) = images.get_mut(*child) {
            set_image_node(&mut image, desired);
        }
    }
}

fn set_image_node(image: &mut ImageNode, desired: Option<&Handle<Image>>) {
    let desired = desired.cloned().unwrap_or(TRANSPARENT_IMAGE_HANDLE);
    if image.image != desired {
        image.image = desired;
    }
}

fn set_child_text(children: &Children, label: &str, labels: &mut Query<'_, '_, &mut Text>) {
    for child in children {
        if let Ok(mut text) = labels.get_mut(*child) {
            if text.0 != label {
                *text = Text::new(label);
            }
            return;
        }
    }
}

fn empty_slot_color(_selected: bool) -> Color {
    Color::srgb(0.12, 0.14, 0.13)
}

fn selected_slot_color() -> Color {
    Color::srgb(0.94, 0.94, 0.86)
}

fn latched_slot_color() -> Color {
    Color::srgb(0.62, 0.52, 0.28)
}

/// Digit keys that select hotbar slots; used by tests of the shipped binding table.
#[cfg(test)]
#[must_use]
fn hotbar_key_slot(code: bevy::input::keyboard::KeyCode) -> Option<u16> {
    use bevy::input::keyboard::KeyCode;
    match code {
        KeyCode::Digit1 | KeyCode::Numpad1 => Some(0),
        KeyCode::Digit2 | KeyCode::Numpad2 => Some(1),
        KeyCode::Digit3 | KeyCode::Numpad3 => Some(2),
        KeyCode::Digit4 | KeyCode::Numpad4 => Some(3),
        KeyCode::Digit5 | KeyCode::Numpad5 => Some(4),
        KeyCode::Digit6 | KeyCode::Numpad6 => Some(5),
        KeyCode::Digit7 | KeyCode::Numpad7 => Some(6),
        KeyCode::Digit8 | KeyCode::Numpad8 => Some(7),
        KeyCode::Digit9 | KeyCode::Numpad9 => Some(8),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        HOTBAR_SLOTS, ITEM_BROWSER_CAPACITY, ITEM_BROWSER_QUERY_CHARS, ItemBrowserStateV1,
        ItemBrowserTransitionV1, ProductionHudSurfaces, hotbar_key_slot, mining_status_line,
        spawn_production_hud, status_line,
    };
    use bevy::{
        app::{App, Startup},
        input::keyboard::KeyCode,
        prelude::{Display, Node, With},
    };
    use latticeaxiom_player::BlockEditRejectV1;

    use super::super::mining_ring::{ProductionMiningRing, ProductionMiningRingSegment};

    #[test]
    fn crosshair_spawns_a_hidden_thirty_two_segment_ring() {
        let mut app = App::new();
        app.add_systems(Startup, spawn_production_hud);
        app.update();

        let world = app.world_mut();
        let segment_count = world
            .query::<&ProductionMiningRingSegment>()
            .iter(world)
            .count();
        assert_eq!(segment_count, 32);
        let mut roots = world.query_filtered::<&Node, With<ProductionMiningRing>>();
        let root = roots.single(world).expect("one mining ring is spawned");
        assert_eq!(root.display, Display::None);
    }

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
    fn status_line_keeps_runtime_distance_telemetry_off_the_normal_hud() {
        let idle_mining = mining_status_line(None);
        let idle = status_line(&idle_mining, None);
        assert!(!idle.contains("Vitality"), "{idle}");
        assert!(!idle.contains("Render"), "{idle}");
        assert!(!idle.contains("View"), "{idle}");
        let progress = BlockEditRejectV1::RequiresProgress {
            progress: latticeaxiom_player::MiningProgressV1::new(3, 10)
                .expect("fixture progress is incomplete"),
        };
        let mining_label = mining_status_line(Some(&progress));
        let mining = status_line(&mining_label, Some(12));
        assert!(mining.contains("Mine —"), "{mining}");
        assert!(
            !mining.contains("left"),
            "progress belongs to the ring: {mining}"
        );
        assert!(mining.contains("Tool 12"), "{mining}");

        let required = BlockEditRejectV1::RequiresTool {
            required: "latticeaxiom:tool-class/pickaxe@1"
                .parse()
                .expect("pickaxe tool class is canonical"),
        };
        assert!(
            mining_status_line(Some(&required)).contains("pickaxe"),
            "tool requirements must be visible"
        );
    }

    #[test]
    fn item_browser_reducer_resets_filters_and_clamps_pages() {
        let mut browser = ItemBrowserStateV1::default();
        browser.apply(ItemBrowserTransitionV1::NavigatePage {
            delta: 1,
            item_count: ITEM_BROWSER_CAPACITY * 2 + 1,
        });
        assert_eq!(browser.first_item_index, ITEM_BROWSER_CAPACITY);
        browser.apply(ItemBrowserTransitionV1::NavigatePage {
            delta: 1,
            item_count: ITEM_BROWSER_CAPACITY * 2 + 1,
        });
        assert_eq!(browser.first_item_index, ITEM_BROWSER_CAPACITY * 2);
        browser.apply(ItemBrowserTransitionV1::NavigatePage {
            delta: 1,
            item_count: ITEM_BROWSER_CAPACITY * 2 + 1,
        });
        assert_eq!(browser.first_item_index, ITEM_BROWSER_CAPACITY * 2);

        browser.apply(ItemBrowserTransitionV1::AppendSearch("stone".to_owned()));
        assert_eq!(browser.first_item_index, 0);
        assert_eq!(browser.query, "stone");
        browser.apply(ItemBrowserTransitionV1::Backspace);
        assert_eq!(browser.query, "ston");
    }

    #[test]
    fn item_browser_reducer_bounds_search_input() {
        let mut browser = ItemBrowserStateV1::default();
        browser.apply(ItemBrowserTransitionV1::AppendSearch(
            "x".repeat(ITEM_BROWSER_QUERY_CHARS + 8),
        ));
        assert_eq!(browser.query.chars().count(), ITEM_BROWSER_QUERY_CHARS);
        browser.apply(ItemBrowserTransitionV1::ClearSearch);
        assert!(browser.query.is_empty());
    }

    #[test]
    fn inventory_and_workbench_are_mutually_exclusive_states() {
        let mut surfaces = ProductionHudSurfaces::default();
        surfaces.set_inventory_open(true);
        assert!(surfaces.inventory_panel_open());
        assert!(!surfaces.workbench_open());
        surfaces.set_workbench_open(true);
        assert!(!surfaces.inventory_panel_open());
        assert!(surfaces.workbench_open());
        surfaces.set_workbench_open(false);
        assert!(!surfaces.inventory_open());
    }
}
