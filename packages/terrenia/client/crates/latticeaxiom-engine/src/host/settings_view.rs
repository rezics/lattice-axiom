//! Interactive Bevy settings page hosted under the pause overlay.

use accesskit::{Node as AccessKitNode, Role as AccessKitRole};
use bevy::{
    a11y::AccessibilityNode,
    ecs::observer::On,
    input_focus::tab_navigation::TabIndex,
    picking::hover::Hovered,
    prelude::{
        AlignItems, BackgroundColor, BorderColor, BorderRadius, Color, Commands, Component,
        Display, Entity, FlexDirection, JustifyContent, Name, Node, Overflow, Query, Res, ResMut,
        Text, TextColor, UiRect, Val, Window, With, Without,
    },
    ui::FocusPolicy,
    ui_widgets::{
        Activate, Button, ScrollArea, Slider, SliderPrecision, SliderRange, SliderStep,
        SliderThumb, SliderValue, TrackClick, ValueChange,
    },
    window::PrimaryWindow,
};
use latticeaxiom_client_ui::desktop_style as style;
use latticeaxiom_core::StableId;
use latticeaxiom_runtime_contracts::{
    ValueType, distant_terrain_quality_setting_id, full_detail_distance_setting_id,
    render_distance_setting_id, simulation_distance_setting_id,
};
use latticeaxiom_settings_ui::{
    SettingsCategoryV1, SettingsControlKind, SettingsPageCommand, SettingsPageOperation,
    SettingsSectionV1, SettingsSurfaceRow, category_display_name, section_display_name,
    setting_display_name,
};
use serde_json::Value;

use super::{
    ADR_0026_CHUNK_EDGE_VOXELS,
    pause::{
        PauseMenuAction, PauseOverlay, ProductionSettingsState, RenderDistanceSlider,
        RenderDistanceSliderThumb,
    },
};
use crate::ui_font::ui_text_font;

/// Marker on the live settings page panel.
#[derive(Clone, Copy, Component, Debug, Default, Eq, PartialEq)]
pub(super) struct SettingsPageRoot {
    generation: u64,
}

/// Focusable settings-page control.
#[derive(Clone, Component, Debug, Eq, PartialEq)]
pub(super) struct SettingsPageControl {
    action: SettingsPageAction,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum SettingsPageAction {
    Category(SettingsCategoryV1),
    Section(SettingsSectionV1),
    Select(StableId),
    Toggle(StableId),
    Cycle(StableId),
    Binding(StableId),
    Operation(SettingsPageOperation),
    ResetRow,
}

/// Integer slider owned by one catalog setting.
#[derive(Clone, Component, Debug, Eq, PartialEq)]
pub(super) struct SettingsIntegerRow {
    setting: StableId,
}

const PAGE_NAV_WIDTH: f32 = 168.0;
const PAGE_DETAIL_WIDTH: f32 = 280.0;
const TRANSACTION_BUTTON_HEIGHT: f32 = 40.0;

/// Spawns the hidden settings page under the pause overlay.
pub(super) fn spawn_settings_page(parent: &mut bevy::ecs::hierarchy::ChildSpawnerCommands<'_>) {
    parent.spawn((
        SettingsPageRoot {
            generation: u64::MAX,
        },
        Name::new("Settings page"),
        Node {
            width: Val::Percent(94.0),
            max_width: Val::Px(1180.0),
            height: Val::Percent(78.0),
            max_height: Val::Px(760.0),
            min_height: Val::Px(320.0),
            display: Display::None,
            flex_direction: FlexDirection::Column,
            row_gap: Val::Px(8.0),
            padding: UiRect::all(Val::Px(12.0)),
            border: UiRect::all(Val::Px(1.0)),
            ..Node::default()
        },
        BackgroundColor(style::SURFACE),
        BorderColor::all(style::BORDER),
        FocusPolicy::Block,
    ));
}

/// Rebuilds the settings page when its generation changes.
#[allow(clippy::needless_pass_by_value)] // Bevy systems receive SystemParams by value.
pub(super) fn sync_settings_page(
    mut commands: Commands<'_, '_>,
    overlay: Query<'_, '_, Entity, With<PauseOverlay>>,
    roots: Query<'_, '_, (Entity, &SettingsPageRoot)>,
    mut settings: Option<ResMut<'_, ProductionSettingsState>>,
    windows: Query<'_, '_, &Window, With<PrimaryWindow>>,
) {
    let Some(settings) = settings.as_mut() else {
        return;
    };
    if let Ok(window) = windows.single() {
        settings.set_compact(window.width() < 900.0);
    }
    let Ok((entity, root)) = roots.single() else {
        return;
    };
    if root.generation == settings.page_generation() {
        return;
    }
    commands.entity(entity).despawn();
    let Ok(parent) = overlay.single() else {
        return;
    };
    commands.entity(parent).with_children(|overlay| {
        spawn_populated_page(overlay, settings);
    });
}

fn spawn_populated_page(
    parent: &mut bevy::ecs::hierarchy::ChildSpawnerCommands<'_>,
    settings: &ProductionSettingsState,
) {
    let page = settings.page();
    parent
        .spawn((
            SettingsPageRoot {
                generation: settings.page_generation(),
            },
            Name::new("Settings page"),
            Node {
                width: Val::Percent(94.0),
                max_width: Val::Px(1180.0),
                height: Val::Percent(78.0),
                max_height: Val::Px(760.0),
                min_height: Val::Px(320.0),
                display: Display::Flex,
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(8.0),
                padding: UiRect::all(Val::Px(12.0)),
                border: UiRect::all(Val::Px(1.0)),
                ..Node::default()
            },
            BackgroundColor(style::SURFACE),
            BorderColor::all(style::BORDER),
            FocusPolicy::Block,
        ))
        .with_children(|page_root| {
            page_root.spawn((
                Name::new("Settings title"),
                Text::new("Settings"),
                ui_text_font(28.0),
                TextColor(style::TEXT),
            ));
            page_root
                .spawn((
                    Name::new("Settings body"),
                    Node {
                        width: Val::Percent(100.0),
                        flex_grow: 1.0,
                        min_height: Val::Px(0.0),
                        flex_direction: FlexDirection::Row,
                        column_gap: Val::Px(12.0),
                        overflow: Overflow::clip(),
                        ..Node::default()
                    },
                ))
                .with_children(|body| {
                    spawn_category_nav(body, page);
                    spawn_content(body, settings);
                    if !page.compact() {
                        spawn_detail(body, settings);
                    }
                });
            spawn_transaction_bar(page_root);
        });
}

fn spawn_category_nav(
    parent: &mut bevy::ecs::hierarchy::ChildSpawnerCommands<'_>,
    page: &latticeaxiom_settings_ui::SettingsPageSession,
) {
    parent
        .spawn((
            Name::new("Settings categories"),
            ScrollArea,
            Node {
                width: Val::Px(PAGE_NAV_WIDTH),
                height: Val::Percent(100.0),
                min_height: Val::Px(0.0),
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(4.0),
                overflow: Overflow::scroll_y(),
                ..Node::default()
            },
        ))
        .with_children(|nav| {
            for (tab, category) in page.visible_categories().into_iter().enumerate() {
                spawn_text_button(
                    nav,
                    SettingsPageAction::Category(category),
                    category_display_name(category),
                    category == page.selected_category(),
                    i32::try_from(tab).unwrap_or(i32::MAX),
                );
            }
        });
}

fn spawn_content(
    parent: &mut bevy::ecs::hierarchy::ChildSpawnerCommands<'_>,
    settings: &ProductionSettingsState,
) {
    let page = settings.page();
    parent
        .spawn((
            Name::new("Settings content"),
            ScrollArea,
            Node {
                flex_grow: 1.0,
                height: Val::Percent(100.0),
                min_height: Val::Px(0.0),
                min_width: Val::Px(0.0),
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(8.0),
                overflow: Overflow::scroll_y(),
                ..Node::default()
            },
        ))
        .with_children(|content| {
            content
                .spawn((
                    Name::new("Settings sections"),
                    Node {
                        flex_direction: FlexDirection::Row,
                        flex_wrap: bevy::prelude::FlexWrap::Wrap,
                        column_gap: Val::Px(6.0),
                        row_gap: Val::Px(6.0),
                        ..Node::default()
                    },
                ))
                .with_children(|sections| {
                    for (tab, section) in page.visible_sections().into_iter().enumerate() {
                        spawn_text_button(
                            sections,
                            SettingsPageAction::Section(section),
                            section_display_name(section),
                            section == page.selected_section(),
                            40 + i32::try_from(tab).unwrap_or(i32::MAX),
                        );
                    }
                });
            for (tab, row) in page.visible_rows().into_iter().enumerate() {
                spawn_setting_row(
                    content,
                    settings,
                    row,
                    80 + i32::try_from(tab).unwrap_or(i32::MAX),
                );
            }
            let binding_base = 80 + i32::try_from(page.visible_rows().len()).unwrap_or(80);
            for (tab, binding) in page.visible_bindings().iter().enumerate() {
                spawn_text_button(
                    content,
                    SettingsPageAction::Binding(binding.action.clone()),
                    &format!(
                        "{}  ·  {}",
                        setting_display_name(&binding.action),
                        binding.effective_label
                    ),
                    false,
                    binding_base + i32::try_from(tab).unwrap_or(i32::MAX),
                );
            }
            let operation_base =
                binding_base + i32::try_from(page.visible_bindings().len()).unwrap_or(0);
            for (tab, operation) in page.visible_operations().iter().enumerate() {
                spawn_text_button(
                    content,
                    SettingsPageAction::Operation(*operation),
                    operation.label(),
                    false,
                    operation_base + i32::try_from(tab).unwrap_or(i32::MAX),
                );
            }
            if page.compact() {
                spawn_detail(content, settings);
            }
        });
}

fn spawn_detail(
    parent: &mut bevy::ecs::hierarchy::ChildSpawnerCommands<'_>,
    settings: &ProductionSettingsState,
) {
    let Some(detail) = settings.page().selected_detail(settings.page_host()) else {
        return;
    };
    let mut lines = vec![
        setting_display_name(&detail.id),
        setting_help_text(&detail.id, &detail.draft),
        String::new(),
        format!(
            "Default: {}",
            setting_value_text(&detail.id, &detail.default)
        ),
        format!(
            "Applied: {}",
            setting_value_text(&detail.id, &detail.applied)
        ),
        format!("Pending: {}", setting_value_text(&detail.id, &detail.draft)),
    ];
    if detail.admission.effective != detail.draft {
        lines.push(format!(
            "Currently available: {}",
            setting_value_text(&detail.id, &detail.admission.effective)
        ));
    }
    if let Some(reason) = detail.admission.clamp_reason.as_deref() {
        lines.push(format!(
            "Availability limit: {}",
            player_facing_reason(reason)
        ));
    }
    if !detail.editable {
        lines.push("Editing is unavailable in the current session.".to_owned());
    }
    let text = lines.join("\n");
    parent
        .spawn((
            Name::new("Settings detail"),
            ScrollArea,
            Node {
                width: Val::Px(PAGE_DETAIL_WIDTH),
                min_width: Val::Px(220.0),
                height: Val::Percent(100.0),
                min_height: Val::Px(0.0),
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(8.0),
                padding: UiRect::all(Val::Px(10.0)),
                overflow: Overflow::scroll_y(),
                ..Node::default()
            },
            BackgroundColor(style::SURFACE),
        ))
        .with_children(|detail_root| {
            detail_root.spawn((Text::new(text), ui_text_font(14.0), TextColor(style::MUTED)));
            if detail.editable {
                spawn_text_button(
                    detail_root,
                    SettingsPageAction::ResetRow,
                    "Reset row",
                    false,
                    200,
                );
            }
        });
}

fn spawn_setting_row(
    parent: &mut bevy::ecs::hierarchy::ChildSpawnerCommands<'_>,
    settings: &ProductionSettingsState,
    row: &SettingsSurfaceRow,
    tab: i32,
) {
    let selected = settings
        .page()
        .selected_detail(settings.page_host())
        .is_some_and(|detail| detail.id == row.id);
    let label = setting_value_label(&row.id, &row.value);
    match row.control {
        SettingsControlKind::IntegerSlider => {
            spawn_integer_slider(parent, settings, row, &label, selected, tab);
        }
        SettingsControlKind::Toggle => spawn_text_button(
            parent,
            SettingsPageAction::Toggle(row.id.clone()),
            &label,
            selected,
            tab,
        ),
        SettingsControlKind::EnumCycle => spawn_text_button(
            parent,
            SettingsPageAction::Cycle(row.id.clone()),
            &label,
            selected,
            tab,
        ),
        _ => spawn_text_button(
            parent,
            SettingsPageAction::Select(row.id.clone()),
            &label,
            selected,
            tab,
        ),
    }
}

#[allow(
    clippy::too_many_lines,
    reason = "Bevy slider hierarchy is one catalog-row widget"
)]
fn spawn_integer_slider(
    parent: &mut bevy::ecs::hierarchy::ChildSpawnerCommands<'_>,
    settings: &ProductionSettingsState,
    row: &SettingsSurfaceRow,
    label: &str,
    selected: bool,
    tab: i32,
) {
    let Ok(state) = settings.page().surface().integer_slider(&row.id) else {
        spawn_text_button(
            parent,
            SettingsPageAction::Select(row.id.clone()),
            label,
            selected,
            tab,
        );
        return;
    };
    let Some((value, min, max, step)) = slider_values(state) else {
        spawn_text_button(
            parent,
            SettingsPageAction::Select(row.id.clone()),
            label,
            selected,
            tab,
        );
        return;
    };
    let render_distance = row.id == render_distance_setting_id();
    parent
        .spawn((
            Name::new(label.to_owned()),
            Node {
                width: Val::Percent(100.0),
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(4.0),
                ..Node::default()
            },
        ))
        .with_children(|block| {
            spawn_text_button(
                block,
                SettingsPageAction::Select(row.id.clone()),
                label,
                selected,
                tab,
            );
            let mut slider = block.spawn((
                SettingsIntegerRow {
                    setting: row.id.clone(),
                },
                SettingsPageControl {
                    action: SettingsPageAction::Select(row.id.clone()),
                },
                Name::new(format!("{} slider", row.id)),
                Node {
                    position_type: bevy::prelude::PositionType::Relative,
                    width: Val::Percent(100.0),
                    height: Val::Px(22.0),
                    border: UiRect::all(Val::Px(2.0)),
                    flex_direction: FlexDirection::Column,
                    justify_content: JustifyContent::Center,
                    ..Node::default()
                },
                Slider {
                    track_click: TrackClick::Snap,
                    ..Slider::default()
                },
                SliderValue(value),
                SliderRange::new(min, max),
                SliderStep(step),
                SliderPrecision(0),
                TabIndex(tab + 500),
                setting_accessibility_node(&row.id, &row.value),
                BorderColor::all(Color::NONE),
            ));
            if render_distance {
                slider.insert(RenderDistanceSlider);
            }
            slider.with_children(|slider| {
                slider.spawn((
                    Name::new("Setting rail"),
                    Node {
                        width: Val::Percent(100.0),
                        height: Val::Px(8.0),
                        border_radius: BorderRadius::all(Val::Px(4.0)),
                        ..Node::default()
                    },
                    BackgroundColor(style::BORDER),
                ));
                slider
                    .spawn((
                        Name::new("Setting thumb track"),
                        Node {
                            position_type: bevy::prelude::PositionType::Absolute,
                            width: Val::Percent(100.0),
                            height: Val::Px(16.0),
                            ..Node::default()
                        },
                    ))
                    .with_children(|track| {
                        let mut thumb = track.spawn((
                            Name::new("Setting thumb"),
                            SliderThumb,
                            Node {
                                position_type: bevy::prelude::PositionType::Absolute,
                                left: Val::Percent(0.0),
                                width: Val::Px(16.0),
                                height: Val::Px(16.0),
                                border_radius: BorderRadius::MAX,
                                ..Node::default()
                            },
                            BackgroundColor(Color::srgb(0.32, 0.78, 0.63)),
                        ));
                        if render_distance {
                            thumb.insert(RenderDistanceSliderThumb);
                        }
                    });
            });
        });
}

fn spawn_transaction_bar(parent: &mut bevy::ecs::hierarchy::ChildSpawnerCommands<'_>) {
    parent
        .spawn((
            Name::new("Settings transaction"),
            Node {
                width: Val::Percent(100.0),
                flex_direction: FlexDirection::Row,
                justify_content: JustifyContent::FlexEnd,
                align_items: AlignItems::Center,
                column_gap: Val::Px(8.0),
                flex_shrink: 0.0,
                ..Node::default()
            },
        ))
        .with_children(|bar| {
            spawn_transaction_button(bar, PauseMenuAction::Undo, "Undo", 1001);
            spawn_transaction_button(bar, PauseMenuAction::Apply, "Apply", 1000);
            spawn_transaction_button(bar, PauseMenuAction::Back, "Back", 1002);
        });
}

fn spawn_transaction_button(
    parent: &mut bevy::ecs::hierarchy::ChildSpawnerCommands<'_>,
    action: PauseMenuAction,
    label: &'static str,
    tab: i32,
) {
    parent
        .spawn((
            Button,
            action,
            Name::new(label),
            Hovered::default(),
            TabIndex(tab),
            accessibility_node(AccessKitRole::Button, label),
            Node {
                min_width: Val::Px(112.0),
                height: Val::Px(TRANSACTION_BUTTON_HEIGHT),
                padding: UiRect::axes(Val::Px(16.0), Val::Px(8.0)),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                border_radius: BorderRadius::all(Val::Px(6.0)),
                ..Node::default()
            },
            BackgroundColor(style::SURFACE),
        ))
        .with_children(|button| {
            button.spawn((Text::new(label), ui_text_font(16.0), TextColor(style::TEXT)));
        });
}

fn spawn_text_button(
    parent: &mut bevy::ecs::hierarchy::ChildSpawnerCommands<'_>,
    action: SettingsPageAction,
    label: &str,
    selected: bool,
    tab: i32,
) {
    parent
        .spawn((
            Button,
            SettingsPageControl { action },
            Name::new(label.to_owned()),
            Hovered::default(),
            TabIndex(tab),
            accessibility_node(AccessKitRole::Button, label),
            Node {
                min_height: Val::Px(36.0),
                width: Val::Percent(100.0),
                padding: UiRect::axes(Val::Px(10.0), Val::Px(6.0)),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::FlexStart,
                ..Node::default()
            },
            BackgroundColor(if selected {
                Color::srgb(0.16, 0.32, 0.28)
            } else {
                style::SURFACE
            }),
        ))
        .with_children(|button| {
            button.spawn((
                Text::new(label.to_owned()),
                ui_text_font(16.0),
                TextColor(style::TEXT),
            ));
        });
}

fn accessibility_node(role: AccessKitRole, label: &str) -> AccessibilityNode {
    let mut node = AccessKitNode::new(role);
    node.set_label(label);
    node.into()
}

fn setting_accessibility_node(id: &StableId, value: &Value) -> AccessibilityNode {
    let mut node = AccessKitNode::new(AccessKitRole::Slider);
    node.set_label(setting_display_name(id));
    node.set_description(setting_help_text(id, value));
    node.into()
}

fn slider_values(
    state: latticeaxiom_settings_ui::SettingsIntegerSliderState,
) -> Option<(f32, f32, f32, f32)> {
    let value = f32::from(i16::try_from(state.draft).ok()?);
    let min = f32::from(i16::try_from(state.min).ok()?);
    let max = f32::from(i16::try_from(state.max).ok()?);
    let step = f32::from(u16::try_from(state.step).ok()?);
    (step > 0.0 && min <= max).then_some((value, min, max, step))
}

fn slider_json_value(value: f32) -> Option<Value> {
    let rounded = value.round();
    if !rounded.is_finite() {
        return None;
    }
    #[allow(
        clippy::cast_possible_truncation,
        reason = "Bevy slider widgets are bounded to the i16 setting domain"
    )]
    let as_i16 = i16::try_from(rounded as i32).ok()?;
    Some(Value::from(i64::from(as_i16)))
}

fn format_value(value: &Value) -> String {
    match value {
        Value::Bool(true) => "On".to_owned(),
        Value::Bool(false) => "Off".to_owned(),
        Value::String(text) => text.clone(),
        other => other.to_string().trim_matches('"').to_owned(),
    }
}

fn setting_value_label(id: &StableId, value: &Value) -> String {
    format!(
        "{}  ·  {}",
        setting_display_name(id),
        setting_value_text(id, value)
    )
}

fn setting_value_text(id: &StableId, value: &Value) -> String {
    let chunks = value.as_u64().and_then(|value| u32::try_from(value).ok());
    if id == &render_distance_setting_id()
        && let Some(chunks) = chunks
    {
        let meters = chunks.saturating_mul(u32::from(ADR_0026_CHUNK_EDGE_VOXELS));
        return format!("{chunks} chunks radius ({meters} m)");
    }
    if (id == &simulation_distance_setting_id() || id == &full_detail_distance_setting_id())
        && let Some(chunks) = chunks
    {
        return format!("{chunks} chunks radius");
    }
    if id == &distant_terrain_quality_setting_id()
        && let Some(value) = value.as_str()
    {
        return match value {
            "performance" => "Performance".to_owned(),
            "balanced" => "Balanced".to_owned(),
            "quality" => "Quality".to_owned(),
            other => other.to_owned(),
        };
    }
    format_value(value)
}

fn setting_help_text(id: &StableId, value: &Value) -> String {
    if id == &render_distance_setting_id() {
        let current = setting_value_text(id, value);
        return format!(
            "Total terrain draw radius around the player. One chunk is {ADR_0026_CHUNK_EDGE_VOXELS} m; the current value is {current}. Simulation Distance and Full Detail Distance are configured separately."
        );
    }
    if id == &simulation_distance_setting_id() {
        return "Radius for authoritative world simulation around the player. It does not change the terrain draw radius.".to_owned();
    }
    if id == &full_detail_distance_setting_id() {
        return "Radius rendered as full voxel geometry. Beyond it, simplified terrain continues until Render Distance.".to_owned();
    }
    if id == &distant_terrain_quality_setting_id() {
        return "Controls the fidelity of simplified terrain beyond Full Detail Distance. It changes distant quality, not the draw radius.".to_owned();
    }
    format!("Controls {}.", setting_display_name(id))
}

fn player_facing_reason(reason: &str) -> String {
    let readable = reason.replace(['-', '_'], " ");
    let mut chars = readable.chars();
    chars.next().map_or_else(String::new, |first| {
        first.to_uppercase().collect::<String>() + chars.as_str()
    })
}

fn cycle_enum(current: &Value, value_type: &ValueType) -> Option<Value> {
    let ValueType::Enum { values } = value_type else {
        return None;
    };
    let current = current.as_str().unwrap_or_default();
    let ordered = values.iter().cloned().collect::<Vec<_>>();
    if ordered.is_empty() {
        return None;
    }
    let index = ordered
        .iter()
        .position(|value| value == current)
        .unwrap_or(0);
    let next = (index + 1) % ordered.len();
    Some(Value::String(ordered[next].clone()))
}

/// Activates category, section, toggle, enum, and reset controls.
#[allow(clippy::needless_pass_by_value)] // Bevy observers receive SystemParams by value.
pub(super) fn settings_page_activated(
    activate: On<'_, '_, Activate>,
    controls: Query<'_, '_, &SettingsPageControl, With<Button>>,
    mut settings: Option<ResMut<'_, ProductionSettingsState>>,
) {
    let Ok(control) = controls.get(activate.entity) else {
        return;
    };
    let Some(settings) = settings.as_mut() else {
        return;
    };
    let command = match &control.action {
        SettingsPageAction::Category(category) => SettingsPageCommand::SelectCategory(*category),
        SettingsPageAction::Section(section) => SettingsPageCommand::SelectSection(*section),
        SettingsPageAction::Select(setting) => SettingsPageCommand::SelectSetting(setting.clone()),
        SettingsPageAction::Toggle(setting) => {
            let Some(current) = settings.page().surface().draft_value(setting) else {
                return;
            };
            SettingsPageCommand::SetValue {
                setting: setting.clone(),
                value: Value::Bool(!current.as_bool().unwrap_or(false)),
            }
        }
        SettingsPageAction::Cycle(setting) => {
            let Some(current) = settings.page().surface().draft_value(setting).cloned() else {
                return;
            };
            let Some(spec) = settings.page().spec(setting) else {
                return;
            };
            let Some(value) = cycle_enum(&current, &spec.value_type) else {
                return;
            };
            SettingsPageCommand::SetValue {
                setting: setting.clone(),
                value,
            }
        }
        SettingsPageAction::Binding(action) => SettingsPageCommand::SelectBinding(action.clone()),
        SettingsPageAction::Operation(_) => return,
        SettingsPageAction::ResetRow => {
            let Some(detail) = settings.page().selected_detail(settings.page_host()) else {
                return;
            };
            SettingsPageCommand::ResetRow(detail.id)
        }
    };
    settings.handle_page(command);
}

/// Copies integer-slider widgets into the settings-page draft.
#[allow(clippy::needless_pass_by_value)] // Bevy observers receive SystemParams by value.
pub(super) fn settings_integer_slider_changed(
    change: On<'_, '_, ValueChange<f32>>,
    sliders: Query<'_, '_, &SettingsIntegerRow, Without<RenderDistanceSlider>>,
    mut settings: Option<ResMut<'_, ProductionSettingsState>>,
    mut commands: Commands<'_, '_>,
) {
    let Ok(row) = sliders.get(change.source) else {
        return;
    };
    let Some(settings) = settings.as_mut() else {
        return;
    };
    let Some(value) = slider_json_value(change.value) else {
        return;
    };
    settings.handle_page(SettingsPageCommand::SetValue {
        setting: row.setting.clone(),
        value,
    });
    if let Ok(state) = settings.page().surface().integer_slider(&row.setting)
        && let Some((value, _, _, _)) = slider_values(state)
    {
        commands.entity(change.source).insert(SliderValue(value));
    }
}

/// Shows or hides the settings page with the Settings modal.
#[allow(clippy::needless_pass_by_value)] // Bevy systems receive SystemParams by value.
pub(super) fn sync_settings_page_visibility(
    pause: Res<'_, super::ProductionSessionPause>,
    router: Option<Res<'_, super::ProductionSurfaceRouter>>,
    mut roots: Query<'_, '_, &mut Node, With<SettingsPageRoot>>,
) {
    let showing = pause.is_paused()
        && router.as_ref().is_some_and(|router| {
            router.inner().route().modal() == latticeaxiom_client_ui::GameModalV1::Settings
        });
    for mut node in &mut roots {
        let display = if showing {
            Display::Flex
        } else {
            Display::None
        };
        if node.display != display {
            node.display = display;
        }
    }
}

#[cfg(test)]
mod tests {
    use latticeaxiom_runtime_contracts::{
        distant_terrain_quality_setting_id, full_detail_distance_setting_id,
        render_distance_setting_id, simulation_distance_setting_id,
    };
    use serde_json::json;

    use super::{setting_help_text, setting_value_label, setting_value_text};

    #[test]
    fn transaction_buttons_are_a_bottom_right_row_and_content_scrolls() {
        let source = include_str!("settings_view.rs");
        assert!(source.contains("Name::new(\"Settings transaction\")"));
        assert!(source.contains("JustifyContent::FlexEnd"));
        assert!(source.contains("FlexDirection::Row"));
        assert!(source.contains("PauseMenuAction::Apply"));
        assert!(source.contains("PauseMenuAction::Undo"));
        assert!(source.contains("PauseMenuAction::Back"));
        assert!(source.contains("ScrollArea"));
        assert!(source.contains("Overflow::scroll_y()"));
        assert!(source.contains("min_height: Val::Px(0.0)"));
        assert!(source.contains("spawn_transaction_button"));
        assert!(source.contains("\"Undo\""));
        assert!(source.contains("\"Apply\""));
        assert!(source.contains("\"Back\""));
        assert!(!source.contains(concat!("spawn_transaction_", "icon")));
    }

    #[test]
    fn terrain_distance_labels_expose_radius_units_and_independent_meanings() {
        let render = render_distance_setting_id();
        assert_eq!(
            setting_value_label(&render, &json!(21)),
            "Render Distance  ·  21 chunks radius (672 m)"
        );
        let render_help = setting_help_text(&render, &json!(21));
        assert!(render_help.contains("One chunk is 32 m"), "{render_help}");
        assert!(
            render_help
                .contains("Simulation Distance and Full Detail Distance are configured separately"),
            "{render_help}"
        );

        let simulation = simulation_distance_setting_id();
        assert_eq!(
            setting_value_text(&simulation, &json!(6)),
            "6 chunks radius"
        );
        assert!(
            setting_help_text(&simulation, &json!(6))
                .contains("does not change the terrain draw radius")
        );

        let full_detail = full_detail_distance_setting_id();
        assert!(
            setting_help_text(&full_detail, &json!(6)).contains("simplified terrain continues")
        );

        let quality = distant_terrain_quality_setting_id();
        assert_eq!(setting_value_text(&quality, &json!("balanced")), "Balanced");
        assert!(
            setting_help_text(&quality, &json!("balanced"))
                .contains("changes distant quality, not the draw radius")
        );
    }
}
