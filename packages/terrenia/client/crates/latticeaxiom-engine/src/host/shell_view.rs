//! Bevy `ui_widgets` adapter for the package-driven start shell.
//!
//! This process hosts only the start-ui semantic tree. Continue/Play of a
//! `ReadyExact` world seals [`LaunchHandoff::for_ready_exact`] and requests
//! [`AppExit`]. An external supervisor must persist that intent and spawn the
//! replacement game process. This App does not install
//! [`super::ProductionSpine`] and does not enter Playing.

use bevy::{
    app::{App, AppExit, Plugin, Startup, Update},
    ecs::observer::On,
    ecs::query::Has,
    ecs::schedule::IntoScheduleConfigs,
    input::{ButtonInput, keyboard::KeyCode},
    input_focus::{
        AutoFocus, InputFocus,
        tab_navigation::{TabGroup, TabIndex},
    },
    picking::hover::Hovered,
    prelude::{
        AlignItems, BackgroundColor, BorderColor, BorderRadius, Camera2d, ClearColor, Color,
        Commands, Component, Entity, FlexDirection, JustifyContent, MessageWriter, Name, Node,
        Overflow, Query, Res, ResMut, Resource, Text, TextColor, UiRect, Val, With,
    },
    ui::Pressed,
    ui_widgets::{Activate, Button, ScrollArea},
};
use latticeaxiom_client_ui::desktop_style as style;
use latticeaxiom_core::WorldId;
use latticeaxiom_launcher::{
    FreshClientAppLeaseProof, FreshClientAppLeaseToken, SettingTransactionRevision,
};
use latticeaxiom_start_ui::{
    InputSource, LaunchHandoff, MemoryStartEffect, SemanticActionId, SemanticCommand, SemanticNode,
    SemanticNodeId, SemanticRole, ShellEffect,
};

use super::{ProductionHostError, ProductionMemoryStart, start::unix_now_ms};
use crate::{
    EngineInstance, LockVerifiedComposeImages, VerifiedProductLockHash, ui_font::ui_text_font,
};

/// Sealed replacement-process handoff published immediately before [`AppExit`].
///
/// Persistence and spawning the game process remain the external supervisor's
/// responsibility.
#[derive(Clone, Debug, Resource)]
#[allow(dead_code)] // Held on the world until AppExit; persistence is the supervisor gap.
pub(crate) struct SealedLaunchHandoff(pub LaunchHandoff);

#[derive(Clone, Debug, Default, Resource)]
pub(crate) struct ShellHandoffState(pub std::sync::Arc<std::sync::atomic::AtomicBool>);

#[derive(Debug, Resource)]
struct ClientShellSession {
    start: ProductionMemoryStart,
    confirmed_setting_transaction_revision: SettingTransactionRevision,
    focused: Option<SemanticNodeId>,
    tree_epoch: u64,
    world_name: String,
    message: Option<String>,
    editing_name: bool,
    select_name: bool,
    composition: String,
    handoff: ShellHandoffState,
}

#[derive(Clone, Copy, Component, Debug, Eq, PartialEq)]
struct ShellViewRoot {
    epoch: u64,
}

#[derive(Component, Debug)]
struct ShellScrollView;

#[derive(Clone, Component, Debug)]
struct ShellControl {
    id: SemanticNodeId,
    primary: bool,
}

/// Bevy plugin that projects the start-ui semantic tree as `ui_widgets` nodes.
#[derive(Clone, Copy, Debug, Default)]
struct ClientShellPlugin;

impl Plugin for ClientShellPlugin {
    fn build(&self, app: &mut App) {
        app.add_observer(shell_control_activated)
            .add_systems(Startup, spawn_shell_camera)
            .add_systems(
                Update,
                (
                    shell_keyboard,
                    shell_text_input,
                    rebuild_shell_view,
                    sync_shell_control_visuals,
                )
                    .chain(),
            )
            .add_systems(
                bevy::app::PostUpdate,
                reveal_shell_focus.after(bevy::ui::UiSystems::Layout),
            );
    }
}

impl EngineInstance {
    /// Builds the process's sole interactive client as a package-driven start shell.
    ///
    /// The App renders [`latticeaxiom_start_ui::StartShellModel::semantic_tree`]
    /// with Bevy `ui_widgets` nodes. Continue/Play of a `ReadyExact` world seals
    /// [`LaunchHandoff::for_ready_exact`] and exits. This constructor does not
    /// spawn [`super::ProductionSpine`] and does not enter Playing in this App.
    ///
    /// An external supervisor must persist the sealed intent and spawn the
    /// replacement game process (a lock whose roots include `terrenia`). This
    /// host does not spawn that process.
    ///
    /// # Errors
    ///
    /// Returns [`crate::ProductionMemoryStartError`] when the lock does not
    /// resolve a shell-package closure or the process event loop is already
    /// reserved.
    pub fn new_client_shell_from_lock(
        images: LockVerifiedComposeImages,
        lease: FreshClientAppLeaseToken,
        confirmed_setting_transaction_revision: SettingTransactionRevision,
    ) -> Result<(Self, FreshClientAppLeaseProof), crate::ProductionMemoryStartError> {
        let start = ProductionMemoryStart::from_lock_images(images.clone())?;
        Self::new_client_shell_with_start(
            images,
            lease,
            confirmed_setting_transaction_revision,
            start,
        )
    }

    /// Builds the shell around a host-supplied persistent world library.
    ///
    /// # Errors
    /// Returns a start error when the client App cannot be created.
    pub fn new_client_shell_with_start(
        images: LockVerifiedComposeImages,
        lease: FreshClientAppLeaseToken,
        confirmed_setting_transaction_revision: SettingTransactionRevision,
        mut start: ProductionMemoryStart,
    ) -> Result<(Self, FreshClientAppLeaseProof), crate::ProductionMemoryStartError> {
        let product_lock_hash = VerifiedProductLockHash::new(images.product_lock_hash());
        start.set_now_ms(unix_now_ms());
        if let Ok(intent) = start.quick_create_intent("New World") {
            start.set_draft(intent);
        }
        let instance = Self::new_client_with_setup(images.into_images(), move |app| {
            install_client_shell(
                app,
                product_lock_hash,
                start,
                confirmed_setting_transaction_revision,
            );
        })
        .map_err(ProductionHostError::from)?;
        Ok((instance, lease.into_app_created_proof()))
    }
}

fn install_client_shell(
    app: &mut App,
    product_lock_hash: VerifiedProductLockHash,
    mut start: ProductionMemoryStart,
    confirmed_setting_transaction_revision: SettingTransactionRevision,
) {
    if std::env::var_os("LATTICEAXIOM_CAPTURE_PATH").is_some() {
        let action = match std::env::var("LATTICEAXIOM_CAPTURE_ROUTE").as_deref() {
            Ok("new-world") => Some(SemanticActionId::QuickCreate),
            Ok("worlds") => Some(SemanticActionId::OpenWorlds),
            Ok("settings") => Some(SemanticActionId::OpenSettings),
            _ => None,
        };
        if let Some(action) = action {
            let tree = start.flow().shell().semantic_tree();
            if let Some(node) = tree
                .children
                .iter()
                .find(|node| node.actions.contains(&action))
            {
                let _ = start.inject(&SemanticCommand {
                    target: node.id.clone(),
                    action,
                    source: InputSource::Headless,
                });
            }
        }
    }
    let focused = first_focusable(&start);
    let handoff = ShellHandoffState::default();
    app.insert_resource(handoff.clone())
        .insert_resource(product_lock_hash)
        .insert_resource(ClearColor(style::CANVAS))
        .insert_resource(ClientShellSession {
            start,
            confirmed_setting_transaction_revision,
            focused,
            tree_epoch: 0,
            world_name: "New World".to_owned(),
            message: None,
            editing_name: false,
            select_name: false,
            composition: String::new(),
            handoff,
        })
        .add_plugins(ClientShellPlugin);
}

#[allow(clippy::needless_pass_by_value)] // Bevy systems receive SystemParams by value.
fn spawn_shell_camera(mut commands: Commands<'_, '_>) {
    commands.spawn((Name::new("Shell Camera"), Camera2d));
}

#[allow(clippy::needless_pass_by_value)] // Bevy systems receive SystemParams by value.
fn rebuild_shell_view(
    mut commands: Commands<'_, '_>,
    session: Res<'_, ClientShellSession>,
    roots: Query<'_, '_, (Entity, &ShellViewRoot)>,
) {
    match roots.single() {
        Ok((_, root)) if root.epoch == session.tree_epoch => {}
        Ok((entity, _)) => {
            commands.entity(entity).despawn();
            spawn_shell_root(&mut commands, &session);
        }
        Err(_) => spawn_shell_root(&mut commands, &session),
    }
}

fn shell_heading(screen: latticeaxiom_start_ui::ShellScreen) -> &'static str {
    use latticeaxiom_start_ui::ShellScreen;
    match screen {
        ShellScreen::Home => "Choose where your story continues.",
        ShellScreen::Worlds => "Your worlds",
        ShellScreen::NewWorld => "Create a world",
        ShellScreen::Trash => "Recently removed worlds",
        ShellScreen::QuitConfirm => "Leave for now?",
        ShellScreen::Loading => "Preparing your world",
        ShellScreen::PackagesProfiles => "Packages and profiles",
        ShellScreen::DiagnosticsAbout => "About Lattice Axiom",
        ShellScreen::Settings => "Settings",
        ShellScreen::Playing | ShellScreen::Pause => "Your journey",
    }
}

fn spawn_shell_root(commands: &mut Commands<'_, '_>, session: &ClientShellSession) {
    let mut tree = session.start.flow().shell().semantic_tree();
    project_name_value(
        &mut tree,
        &format!("{}{}", session.world_name, session.composition),
    );
    commands
        .spawn((
            ShellViewRoot {
                epoch: session.tree_epoch,
            },
            Name::new("Client shell"),
            TabGroup::default(),
            Node {
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                row_gap: Val::Px(20.0),
                padding: UiRect::axes(Val::Px(36.0), Val::Px(24.0)),
                ..Node::default()
            },
            style::backdrop(),
        ))
        .with_children(|root| {
            spawn_shell_header(root);
            root.spawn((
                Node {
                    width: Val::Percent(100.0),
                    max_width: Val::Px(1080.0),
                    flex_grow: 1.0,
                    min_height: Val::Px(0.0),
                    flex_direction: FlexDirection::Column,
                    row_gap: Val::Px(12.0),
                    overflow: Overflow::scroll_y(),
                    padding: UiRect::right(Val::Px(12.0)),
                    ..Node::default()
                },
                ScrollArea,
                ShellScrollView,
            ))
            .with_children(|body| {
                spawn_shell_content(body, &tree, session);
            });
            if session.start.flow().shell().screen == latticeaxiom_start_ui::ShellScreen::NewWorld {
                root.spawn(Node {
                    width: Val::Percent(100.0),
                    max_width: Val::Px(1080.0),
                    column_gap: Val::Px(12.0),
                    ..Node::default()
                })
                .with_children(|actions| {
                    for node in tree
                        .children
                        .iter()
                        .filter(|node| is_creation_action(&node.id))
                    {
                        spawn_control(actions, node, session.focused.as_ref());
                    }
                });
            }
            root.spawn((
                Text::new("Tab  Move focus     Enter  Select     Esc  Back"),
                ui_text_font(15.0),
                TextColor(style::MUTED),
                Node {
                    width: Val::Percent(100.0),
                    max_width: Val::Px(1080.0),
                    ..Node::default()
                },
            ));
        });
}

fn spawn_shell_header(root: &mut bevy::ecs::hierarchy::ChildSpawnerCommands<'_>) {
    root.spawn((
        Node {
            width: Val::Percent(100.0),
            max_width: Val::Px(1080.0),
            min_height: Val::Px(60.0),
            align_items: AlignItems::Center,
            justify_content: JustifyContent::SpaceBetween,
            border: UiRect::bottom(Val::Px(1.0)),
            padding: UiRect::bottom(Val::Px(16.0)),
            ..Node::default()
        },
        BorderColor::all(style::BORDER),
    ))
    .with_children(|header| {
        header.spawn((
            Text::new("Lattice Axiom"),
            ui_text_font(30.0),
            TextColor(style::TEXT),
        ));
        header.spawn((
            Text::new("Single player"),
            ui_text_font(16.0),
            TextColor(style::MUTED),
        ));
    });
}

fn is_creation_action(id: &SemanticNodeId) -> bool {
    matches!(id.as_str(), "new-world/back" | "new-world/quick-create")
}

fn spawn_shell_content(
    body: &mut bevy::ecs::hierarchy::ChildSpawnerCommands<'_>,
    tree: &SemanticNode,
    session: &ClientShellSession,
) {
    body.spawn((
        Text::new(shell_heading(session.start.flow().shell().screen)),
        ui_text_font(36.0),
        TextColor(style::TEXT),
        Node {
            margin: UiRect::bottom(Val::Px(8.0)),
            ..Node::default()
        },
    ));
    if let Some(message) = &session.message {
        body.spawn((
            Text::new(message.clone()),
            ui_text_font(18.0),
            TextColor(style::DANGER),
        ));
    }
    for child in &tree.children {
        if !is_creation_action(&child.id) && !child.id.as_str().starts_with("new-world/profile/") {
            spawn_semantic_node(body, child, session.focused.as_ref());
        }
    }
    if session.start.flow().shell().screen == latticeaxiom_start_ui::ShellScreen::NewWorld {
        body.spawn(Node {
            width: Val::Percent(100.0),
            flex_wrap: bevy::prelude::FlexWrap::Wrap,
            column_gap: Val::Px(10.0),
            row_gap: Val::Px(10.0),
            ..Node::default()
        })
        .with_children(|profiles| {
            for child in tree
                .children
                .iter()
                .filter(|node| node.id.as_str().starts_with("new-world/profile/"))
            {
                spawn_control(profiles, child, session.focused.as_ref());
            }
        });
    }
}

fn project_name_value(node: &mut SemanticNode, name: &str) {
    if node.id.as_str() == "new-world/name" {
        node.value = Some(name.to_owned());
    }
    for child in &mut node.children {
        project_name_value(child, name);
    }
}

fn spawn_semantic_node(
    parent: &mut bevy::ecs::hierarchy::ChildSpawnerCommands<'_>,
    node: &SemanticNode,
    focused: Option<&SemanticNodeId>,
) {
    match node.role {
        SemanticRole::Application | SemanticRole::Navigation | SemanticRole::Group => {
            parent.spawn((
                Name::new(node.name.clone()),
                Text::new(node.name.clone()),
                ui_text_font(if node.role == SemanticRole::Application {
                    36.0
                } else {
                    22.0
                }),
                TextColor(style::TEXT),
            ));
            if node.role == SemanticRole::Application
                && let Some(description) = &node.description
            {
                parent.spawn((
                    Text::new(description.clone()),
                    ui_text_font(16.0),
                    TextColor(style::MUTED),
                    Node {
                        margin: UiRect::bottom(Val::Px(8.0)),
                        ..Node::default()
                    },
                ));
            }
            for child in &node.children {
                spawn_semantic_node(parent, child, focused);
            }
        }
        SemanticRole::Button | SemanticRole::ListItem | SemanticRole::TextInput => {
            spawn_control(parent, node, focused);
        }
        SemanticRole::Status | SemanticRole::Alert => {
            let label = node.value.as_ref().map_or_else(
                || node.name.clone(),
                |value| format!("{}: {value}", node.name),
            );
            parent.spawn((
                Name::new(node.name.clone()),
                Text::new(label),
                ui_text_font(18.0),
                TextColor(if node.role == SemanticRole::Alert {
                    style::DANGER
                } else {
                    style::MUTED
                }),
            ));
        }
    }
}

fn spawn_control(
    parent: &mut bevy::ecs::hierarchy::ChildSpawnerCommands<'_>,
    node: &SemanticNode,
    focused: Option<&SemanticNodeId>,
) {
    let is_focused = focused.is_some_and(|focused| focused == &node.id);
    let primary = node.actions.contains(&SemanticActionId::QuickCreate)
        || node.actions.contains(&SemanticActionId::ContinueWorld)
        || node.actions.contains(&SemanticActionId::PlayExact);
    let label = node.value.as_ref().map_or_else(
        || node.name.clone(),
        |value| format!("{} — {value}", node.name),
    );
    let profile = node.id.as_str().starts_with("new-world/profile/");
    let width = if profile {
        Val::Percent(31.0)
    } else if node.id.as_str() == "new-world/back" {
        Val::Px(160.0)
    } else {
        Val::Percent(100.0)
    };
    let mut control = parent.spawn((
        Button,
        TabIndex(0),
        ShellControl {
            id: node.id.clone(),
            primary,
        },
        Name::new(node.name.clone()),
        Hovered::default(),
        Node {
            width,
            min_width: if profile {
                Val::Px(180.0)
            } else {
                Val::Px(0.0)
            },
            flex_grow: f32::from(u8::from(profile)),
            max_width: Val::Px(720.0),
            min_height: Val::Px(52.0),
            border: UiRect::all(Val::Px(2.0)),
            border_radius: BorderRadius::all(Val::Px(6.0)),
            padding: UiRect::axes(Val::Px(16.0), Val::Px(10.0)),
            align_items: AlignItems::Center,
            justify_content: JustifyContent::FlexStart,
            ..Node::default()
        },
        BackgroundColor(control_color(false, is_focused, primary)),
        BorderColor::all(style::focus_border(is_focused)),
    ));
    if is_focused {
        control.insert(AutoFocus);
    }
    if node.state.disabled {
        control.insert(bevy::ui::InteractionDisabled);
    }
    if node.role == SemanticRole::TextInput {
        let mut accessible = accesskit::Node::new(accesskit::Role::TextInput);
        accessible.set_label(node.name.clone());
        accessible.set_value(node.value.clone().unwrap_or_default());
        control.insert(bevy::a11y::AccessibilityNode(accessible));
    }
    control.with_children(|button| {
        button.spawn((
            Text::new(label),
            ui_text_font(20.0),
            TextColor(if primary { style::CANVAS } else { style::TEXT }),
        ));
    });
}

const fn control_color(pressed: bool, hovered: bool, primary: bool) -> Color {
    if primary {
        style::ACCENT
    } else if pressed {
        style::BORDER
    } else if hovered {
        style::RAISED
    } else {
        style::SURFACE
    }
}

#[allow(clippy::needless_pass_by_value)] // Bevy observers receive SystemParams by value.
fn shell_control_activated(
    activate: On<'_, '_, Activate>,
    mut commands: Commands<'_, '_>,
    mut session: ResMut<'_, ClientShellSession>,
    mut exits: MessageWriter<'_, AppExit>,
    controls: Query<'_, '_, &ShellControl, With<Button>>,
) {
    let Ok(control) = controls.get(activate.entity) else {
        return;
    };
    if control.id.as_str() == "new-world/name" {
        return;
    }
    let target = control.id.clone();
    apply_shell_command(
        &mut commands,
        &mut session,
        &mut exits,
        &SemanticCommand {
            target,
            action: SemanticActionId::Activate,
            source: InputSource::Keyboard,
        },
    );
}

type ShellControlVisualQuery<'w, 's> = Query<
    'w,
    's,
    (
        Entity,
        &'static ShellControl,
        &'static Hovered,
        Has<Pressed>,
        &'static mut BackgroundColor,
        &'static mut BorderColor,
    ),
    With<Button>,
>;

#[allow(clippy::needless_pass_by_value)] // Bevy systems receive SystemParams by value.
fn sync_shell_control_visuals(
    mut session: ResMut<'_, ClientShellSession>,
    focus: Res<'_, InputFocus>,
    mut controls: ShellControlVisualQuery<'_, '_>,
) {
    for (entity, control, hovered, pressed, mut background, mut border) in &mut controls {
        let focused = focus.get() == Some(entity);
        if focused {
            session.focused = Some(control.id.clone());
        }
        border.set_all(if control.primary && focused {
            style::TEXT
        } else {
            style::focus_border(focused)
        });
        let desired = control_color(pressed, hovered.get() || focused, control.primary);
        if background.0 != desired {
            background.0 = desired;
        }
    }
}

#[allow(clippy::needless_pass_by_value)] // Bevy systems receive SystemParams by value.
fn shell_keyboard(
    mut commands: Commands<'_, '_>,
    mut session: ResMut<'_, ClientShellSession>,
    mut exits: MessageWriter<'_, AppExit>,
    keyboard: Res<'_, ButtonInput<KeyCode>>,
) {
    if keyboard.just_pressed(KeyCode::Escape)
        && let Some(target) = back_target(&session.start.flow().shell().semantic_tree())
    {
        apply_shell_command(
            &mut commands,
            &mut session,
            &mut exits,
            &SemanticCommand {
                target,
                action: SemanticActionId::Back,
                source: InputSource::Keyboard,
            },
        );
    }
}

#[allow(clippy::needless_pass_by_value)]
fn reveal_shell_focus(
    focus: Res<'_, InputFocus>,
    mut last_revealed: bevy::prelude::Local<'_, Option<Entity>>,
    elements: Query<
        '_,
        '_,
        (
            &bevy::ui::ComputedNode,
            &bevy::ui::UiGlobalTransform,
            &ShellControl,
        ),
        With<ShellControl>,
    >,
    mut scrolls: Query<
        '_,
        '_,
        (
            &bevy::ui::ComputedNode,
            &bevy::ui::UiGlobalTransform,
            &mut bevy::ui::ScrollPosition,
        ),
        With<ShellScrollView>,
    >,
) {
    let Some(entity) = focus.get() else {
        return;
    };
    if *last_revealed == Some(entity) {
        return;
    }
    let Ok((item, position, control)) = elements.get(entity) else {
        return;
    };
    if is_creation_action(&control.id) {
        return;
    }
    for (viewport, origin, mut scroll) in &mut scrolls {
        if viewport.size().y <= 0.0 {
            continue;
        }
        let top = origin.translation.y - viewport.size().y / 2.0;
        let bottom = top + viewport.size().y;
        let item_top = position.translation.y - item.size().y / 2.0;
        let item_bottom = item_top + item.size().y;
        let delta = if item_top < top {
            item_top - top
        } else if item_bottom > bottom {
            item_bottom - bottom
        } else {
            0.0
        };
        if delta != 0.0 {
            scroll.y = (scroll.y + delta * viewport.inverse_scale_factor()).max(0.0);
        }
        *last_revealed = Some(entity);
    }
}

#[allow(clippy::needless_pass_by_value)]
fn shell_text_input(
    mut session: ResMut<'_, ClientShellSession>,
    keyboard: Res<'_, ButtonInput<KeyCode>>,
    mut input: bevy::prelude::MessageReader<'_, '_, bevy::input::keyboard::KeyboardInput>,
    mut ime: bevy::prelude::MessageReader<'_, '_, bevy::window::Ime>,
    mut windows: Query<'_, '_, &mut bevy::window::Window, With<bevy::window::PrimaryWindow>>,
) {
    let editing = session
        .focused
        .as_ref()
        .is_some_and(|id| id.as_str() == "new-world/name");
    for mut window in &mut windows {
        window.ime_enabled = editing;
    }
    if !editing {
        session.editing_name = false;
        input.clear();
        ime.clear();
        return;
    }
    if !session.editing_name {
        session.select_name = true;
        session.editing_name = true;
    }
    let before = (session.world_name.clone(), session.composition.clone());
    let mut committed_ime = false;
    for event in ime.read() {
        match event {
            bevy::window::Ime::Preedit { value, .. } => session.composition.clone_from(value),
            bevy::window::Ime::Commit { value, .. } => {
                committed_ime = true;
                if session.select_name {
                    session.world_name.clear();
                    session.select_name = false;
                }
                append_world_name(&mut session.world_name, value);
                session.composition.clear();
            }
            _ => {}
        }
    }
    let control = keyboard.pressed(KeyCode::ControlLeft) || keyboard.pressed(KeyCode::ControlRight);
    for event in input
        .read()
        .filter(|event| event.state == bevy::input::ButtonState::Pressed)
    {
        if control && event.key_code == KeyCode::KeyA {
            session.select_name = true;
            continue;
        }
        if event.key_code == KeyCode::Backspace {
            if session.select_name {
                session.world_name.clear();
                session.select_name = false;
            } else {
                session.world_name.pop();
            }
        } else if !control
            && !committed_ime
            && session.composition.is_empty()
            && let Some(text) = &event.text
            && text.chars().any(|ch| !ch.is_control())
        {
            if session.select_name {
                session.world_name.clear();
                session.select_name = false;
            }
            append_world_name(&mut session.world_name, text);
        }
    }
    if before != (session.world_name.clone(), session.composition.clone()) {
        session.tree_epoch = session.tree_epoch.saturating_add(1);
    }
}

fn append_world_name(name: &mut String, text: &str) {
    let remaining = 64_usize.saturating_sub(name.chars().count());
    name.extend(text.chars().filter(|ch| !ch.is_control()).take(remaining));
}

fn apply_shell_command(
    commands: &mut Commands<'_, '_>,
    session: &mut ClientShellSession,
    exits: &mut MessageWriter<'_, AppExit>,
    command: &SemanticCommand,
) {
    session.message = None;
    if command.target.as_str() == "new-world/quick-create" {
        match session.start.quick_create_intent(&session.world_name) {
            Ok(intent) => session.start.set_draft(intent),
            Err(error) => {
                session.message = Some(error.to_string());
                session.tree_epoch = session.tree_epoch.saturating_add(1);
                return;
            }
        }
    }
    session.start.set_now_ms(unix_now_ms());
    let effect = match session.start.inject(command) {
        Ok(effect) => effect,
        Err(error) => {
            session.message = Some(error.to_string());
            session.tree_epoch = session.tree_epoch.saturating_add(1);
            return;
        }
    };
    match effect {
        MemoryStartEffect::Shell(ShellEffect::RequestQuitProduct) => {
            exits.write(AppExit::Success);
        }
        MemoryStartEffect::Shell(ShellEffect::RequestExactWorldLaunch(world_id)) => {
            exit_with_ready_exact_handoff(commands, session, exits, world_id);
        }
        MemoryStartEffect::Shell(ShellEffect::ReviewWorld(world_id)) => {
            if exit_with_ready_exact_handoff(commands, session, exits, world_id) {
                return;
            }
            session.focused = first_focusable(&session.start);
            session.tree_epoch = session.tree_epoch.saturating_add(1);
        }
        MemoryStartEffect::Created(_) => {
            if let Ok(intent) = session.start.quick_create_intent("New World") {
                session.start.set_draft(intent);
            }
            session.focused = first_focusable(&session.start);
            session.tree_epoch = session.tree_epoch.saturating_add(1);
        }
        MemoryStartEffect::Shell(ShellEffect::FocusTraversal(_)) => {
            traverse_focus(session, command.action == SemanticActionId::FocusPrevious);
        }
        MemoryStartEffect::Shell(_) => {
            session.focused = first_focusable(&session.start);
            session.tree_epoch = session.tree_epoch.saturating_add(1);
        }
    }
}

fn traverse_focus(session: &mut ClientShellSession, reverse: bool) {
    let order = session.start.flow().shell().semantic_tree().focus_order();
    if order.is_empty() {
        session.focused = None;
        session.tree_epoch = session.tree_epoch.saturating_add(1);
        return;
    }
    let current = session
        .focused
        .as_ref()
        .and_then(|focused| order.iter().position(|id| id == focused));
    let next = match (current, reverse) {
        (Some(index), false) => (index + 1) % order.len(),
        (Some(index), true) => index.checked_sub(1).unwrap_or(order.len() - 1),
        (None, true) => order.len() - 1,
        (None, false) => 0,
    };
    session.focused = order.get(next).cloned();
    session.tree_epoch = session.tree_epoch.saturating_add(1);
}

fn exit_with_ready_exact_handoff(
    commands: &mut Commands<'_, '_>,
    session: &mut ClientShellSession,
    exits: &mut MessageWriter<'_, AppExit>,
    world_id: WorldId,
) -> bool {
    let mut handoff = match session.start.launch_handoff_for_ready_exact(
        world_id,
        unix_now_ms(),
        session.confirmed_setting_transaction_revision,
    ) {
        Ok(handoff) => handoff,
        Err(error) => {
            session.message = Some(error.to_string());
            session.tree_epoch = session.tree_epoch.saturating_add(1);
            return false;
        }
    };
    if let Some(root) = std::env::var_os(crate::supervisor::ENV_LAUNCH_ROOT) {
        let root = std::path::PathBuf::from(root);
        let epoch = std::env::var(crate::supervisor::ENV_PROCESS_EPOCH)
            .ok()
            .and_then(|value| value.parse().ok())
            .and_then(|value| latticeaxiom_launcher::ProcessEpoch::new(value).ok())
            .unwrap_or(latticeaxiom_launcher::ProcessEpoch::FIRST);
        let generation = std::env::var(crate::supervisor::ENV_GENERATION)
            .ok()
            .and_then(|value| value.parse().ok())
            .and_then(|value| latticeaxiom_launcher::LaunchGeneration::new(value).ok())
            .unwrap_or(latticeaxiom_launcher::LaunchGeneration::FIRST);
        match crate::supervisor::persist_supervised_intent(&root, &handoff.intent, generation) {
            Ok(durable) => handoff.intent = durable,
            Err(error) => {
                session.message = Some(error.to_string());
                session.tree_epoch = session.tree_epoch.saturating_add(1);
                return false;
            }
        }
        let report = match latticeaxiom_launcher::ChildExitReportV1::seal(
            latticeaxiom_launcher::ChildExitReportDraftV1 {
                child_generation: generation,
                process_epoch: epoch,
                role: crate::supervisor::shell_like_child_role(),
                exit_kind: latticeaxiom_launcher::ChildExitKindV1::Handoff,
                intent_generation: Some(handoff.intent.generation()),
                intent_checksum: Some(handoff.intent.checksum()),
                confirmed_setting_transaction_revision: handoff
                    .intent
                    .confirmed_setting_transaction_revision(),
                last_written_world: None,
                last_durable_world: None,
                shell_lock_hash: handoff.intent.shell_lock_hash(),
                world_lock_hash: None,
                world_open_plan_hash: None,
                diagnostic_ref: None,
            },
        ) {
            Ok(report) => report,
            Err(error) => {
                session.message = Some(error.to_string());
                session.tree_epoch = session.tree_epoch.saturating_add(1);
                return false;
            }
        };
        if let Err(error) =
            crate::supervisor::publish_child_exit(&root, &report, Some(&handoff.intent))
        {
            session.message = Some(error.to_string());
            session.tree_epoch = session.tree_epoch.saturating_add(1);
            return false;
        }
    }
    session
        .handoff
        .0
        .store(true, std::sync::atomic::Ordering::Release);
    commands.insert_resource(SealedLaunchHandoff(handoff));
    exits.write(AppExit::Success);
    true
}

fn first_focusable(start: &ProductionMemoryStart) -> Option<SemanticNodeId> {
    if start.flow().shell().screen == latticeaxiom_start_ui::ShellScreen::NewWorld {
        return SemanticNodeId::new("new-world/name").ok();
    }
    start
        .flow()
        .shell()
        .semantic_tree()
        .focus_order()
        .into_iter()
        .next()
}

fn back_target(tree: &SemanticNode) -> Option<SemanticNodeId> {
    if tree.actions.contains(&SemanticActionId::Back) {
        return Some(tree.id.clone());
    }
    tree.children.iter().find_map(back_target)
}

#[cfg(test)]
mod name_editor_tests {
    use super::append_world_name;

    #[test]
    fn name_input_preserves_unicode_and_bounds_committed_text() {
        let mut name = String::new();
        append_world_name(&mut name, "晶格世界\n");
        assert_eq!(name, "晶格世界");
        append_world_name(&mut name, &"界".repeat(100));
        assert_eq!(name.chars().count(), 64);
        name.pop();
        assert_eq!(name.chars().count(), 63);
    }
}
