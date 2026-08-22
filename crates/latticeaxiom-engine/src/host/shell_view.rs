//! Bevy Text/Button adapter for the package-driven start shell.
//!
//! This process hosts only the start-ui semantic tree. Continue/Play of a
//! `ReadyExact` world seals [`LaunchHandoff::for_ready_exact`] and requests
//! [`AppExit`]. An external supervisor must persist that intent and spawn the
//! replacement game process. This App does not install
//! [`super::ProductionSpine`] and does not enter Playing.

use bevy::{
    app::{App, AppExit, Plugin, Startup, Update},
    ecs::schedule::IntoScheduleConfigs,
    input::{ButtonInput, keyboard::KeyCode},
    prelude::{
        AlignItems, BackgroundColor, Button, Camera2d, Changed, ClearColor, Color, Commands,
        Component, Entity, FlexDirection, Interaction, JustifyContent, MessageWriter, Name, Node,
        Query, Res, ResMut, Resource, Text, TextColor, TextFont, UiRect, Val, With,
    },
};
use latticeaxiom_core::WorldId;
use latticeaxiom_launcher::{FreshClientAppLeaseProof, FreshClientAppLeaseToken};
use latticeaxiom_start_ui::{
    InputSource, LaunchHandoff, MemoryStartEffect, SemanticActionId, SemanticCommand, SemanticNode,
    SemanticNodeId, SemanticRole, ShellEffect,
};

use super::{ProductionHostError, ProductionMemoryStart, start::unix_now_ms};
use crate::{EngineInstance, LockVerifiedComposeImages, VerifiedProductLockHash};

/// Sealed replacement-process handoff published immediately before [`AppExit`].
///
/// Persistence and spawning the game process remain the external supervisor's
/// responsibility.
#[derive(Clone, Debug, Resource)]
#[allow(dead_code)] // Held on the world until AppExit; persistence is the supervisor gap.
pub(crate) struct SealedLaunchHandoff(pub LaunchHandoff);

#[derive(Debug, Resource)]
struct ClientShellSession {
    start: ProductionMemoryStart,
    focused: Option<SemanticNodeId>,
    tree_epoch: u64,
}

#[derive(Clone, Copy, Component, Debug, Eq, PartialEq)]
struct ShellViewRoot {
    epoch: u64,
}

#[derive(Clone, Component, Debug)]
struct ShellControl {
    id: SemanticNodeId,
}

/// Bevy plugin that projects the start-ui semantic tree as Text/Button nodes.
#[derive(Clone, Copy, Debug, Default)]
struct ClientShellPlugin;

impl Plugin for ClientShellPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_shell_camera).add_systems(
            Update,
            (shell_button_clicks, shell_keyboard, rebuild_shell_view).chain(),
        );
    }
}

impl EngineInstance {
    /// Builds the process's sole interactive client as a package-driven start shell.
    ///
    /// The App renders [`latticeaxiom_start_ui::StartShellModel::semantic_tree`]
    /// with Bevy Text/Button nodes. Continue/Play of a `ReadyExact` world seals
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
    ) -> Result<(Self, FreshClientAppLeaseProof), crate::ProductionMemoryStartError> {
        let product_lock_hash = VerifiedProductLockHash::new(images.product_lock_hash());
        let mut start = ProductionMemoryStart::from_lock_images(images.clone())?;
        start.set_now_ms(unix_now_ms());
        if let Ok(intent) = start.quick_create_intent("New World") {
            start.set_draft(intent);
        }
        let instance = Self::new_client_with_setup(images.into_images(), move |app| {
            install_client_shell(app, product_lock_hash, start);
        })
        .map_err(ProductionHostError::from)?;
        Ok((instance, lease.into_app_created_proof()))
    }
}

fn install_client_shell(
    app: &mut App,
    product_lock_hash: VerifiedProductLockHash,
    start: ProductionMemoryStart,
) {
    let focused = first_focusable(&start);
    app.insert_resource(product_lock_hash)
        .insert_resource(ClearColor(Color::srgb(0.05, 0.06, 0.07)))
        .insert_resource(ClientShellSession {
            start,
            focused,
            tree_epoch: 0,
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

fn spawn_shell_root(commands: &mut Commands<'_, '_>, session: &ClientShellSession) {
    let tree = session.start.flow().shell().semantic_tree();
    commands
        .spawn((
            ShellViewRoot {
                epoch: session.tree_epoch,
            },
            Name::new("Client shell"),
            Node {
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                row_gap: Val::Px(12.0),
                padding: UiRect::all(Val::Px(24.0)),
                ..Node::default()
            },
        ))
        .with_children(|root| {
            spawn_semantic_node(root, &tree, session.focused.as_ref());
        });
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
                TextFont::from_font_size(if node.role == SemanticRole::Application {
                    36.0
                } else {
                    22.0
                }),
                TextColor(Color::srgb(0.94, 0.95, 0.90)),
            ));
            if node.role == SemanticRole::Application
                && let Some(description) = &node.description
            {
                parent.spawn((
                    Text::new(description.clone()),
                    TextFont::from_font_size(16.0),
                    TextColor(Color::srgb(0.72, 0.74, 0.68)),
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
        SemanticRole::Button | SemanticRole::ListItem => {
            spawn_control(parent, node, focused);
        }
        SemanticRole::TextInput | SemanticRole::Status | SemanticRole::Alert => {
            let label = node.value.as_ref().map_or_else(
                || node.name.clone(),
                |value| format!("{}: {value}", node.name),
            );
            parent.spawn((
                Name::new(node.name.clone()),
                Text::new(label),
                TextFont::from_font_size(18.0),
                TextColor(if node.role == SemanticRole::Alert {
                    Color::srgb(0.92, 0.62, 0.45)
                } else {
                    Color::srgb(0.86, 0.88, 0.82)
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
    let label = node.value.as_ref().map_or_else(
        || node.name.clone(),
        |value| format!("{} — {value}", node.name),
    );
    parent
        .spawn((
            Button,
            ShellControl {
                id: node.id.clone(),
            },
            Name::new(node.name.clone()),
            Node {
                width: Val::Px(480.0),
                min_height: Val::Px(44.0),
                padding: UiRect::axes(Val::Px(16.0), Val::Px(10.0)),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                ..Node::default()
            },
            BackgroundColor(control_color(if is_focused {
                Interaction::Hovered
            } else {
                Interaction::None
            })),
        ))
        .with_children(|button| {
            button.spawn((
                Text::new(label),
                TextFont::from_font_size(20.0),
                TextColor(Color::srgb(0.94, 0.95, 0.90)),
            ));
        });
}

const fn control_color(interaction: Interaction) -> Color {
    match interaction {
        Interaction::Pressed => Color::srgb(0.18, 0.42, 0.36),
        Interaction::Hovered => Color::srgb(0.16, 0.22, 0.20),
        Interaction::None => Color::srgb(0.08, 0.11, 0.10),
    }
}

#[allow(clippy::needless_pass_by_value)] // Bevy systems receive SystemParams by value.
#[allow(clippy::type_complexity)] // Button interaction query is one shell-control mapping.
fn shell_button_clicks(
    mut commands: Commands<'_, '_>,
    mut session: ResMut<'_, ClientShellSession>,
    mut exits: MessageWriter<'_, AppExit>,
    mut interactions: Query<
        '_,
        '_,
        (&Interaction, &ShellControl, &mut BackgroundColor),
        (Changed<Interaction>, With<Button>),
    >,
) {
    let mut pressed = None;
    for (interaction, control, mut background) in &mut interactions {
        let focused = session.focused.as_ref() == Some(&control.id);
        background.0 = control_color(if *interaction == Interaction::None && focused {
            Interaction::Hovered
        } else {
            *interaction
        });
        if *interaction == Interaction::Pressed {
            pressed = Some(control.id.clone());
        }
    }
    if let Some(target) = pressed {
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
}

#[allow(clippy::needless_pass_by_value)] // Bevy systems receive SystemParams by value.
fn shell_keyboard(
    mut commands: Commands<'_, '_>,
    mut session: ResMut<'_, ClientShellSession>,
    mut exits: MessageWriter<'_, AppExit>,
    keyboard: Res<'_, ButtonInput<KeyCode>>,
) {
    if keyboard.just_pressed(KeyCode::Tab) {
        let reverse = keyboard.pressed(KeyCode::ShiftLeft) || keyboard.pressed(KeyCode::ShiftRight);
        traverse_focus(&mut session, reverse);
        return;
    }
    if keyboard.just_pressed(KeyCode::ArrowDown) {
        traverse_focus(&mut session, false);
        return;
    }
    if keyboard.just_pressed(KeyCode::ArrowUp) {
        traverse_focus(&mut session, true);
        return;
    }
    if keyboard.just_pressed(KeyCode::Enter) || keyboard.just_pressed(KeyCode::Space) {
        if let Some(target) = session.focused.clone() {
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
        return;
    }
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

fn apply_shell_command(
    commands: &mut Commands<'_, '_>,
    session: &mut ClientShellSession,
    exits: &mut MessageWriter<'_, AppExit>,
    command: &SemanticCommand,
) {
    session.start.set_now_ms(unix_now_ms());
    let Ok(effect) = session.start.inject(command) else {
        return;
    };
    match effect {
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
    session: &ClientShellSession,
    exits: &mut MessageWriter<'_, AppExit>,
    world_id: WorldId,
) -> bool {
    let Ok(handoff) = session
        .start
        .launch_handoff_for_ready_exact(world_id, unix_now_ms())
    else {
        return false;
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
        if let Ok(report) = latticeaxiom_launcher::ChildExitReportV1::seal(
            latticeaxiom_launcher::ChildExitReportDraftV1 {
                child_generation: generation,
                process_epoch: epoch,
                role: latticeaxiom_launcher::ChildRoleV1::Shell,
                exit_kind: latticeaxiom_launcher::ChildExitKindV1::Handoff,
                intent_generation: Some(handoff.intent.generation()),
                intent_checksum: Some(handoff.intent.checksum()),
                confirmed_setting_transaction_revision:
                    latticeaxiom_launcher::SettingTransactionRevision::new(0),
                last_written_world: None,
                last_durable_world: None,
                shell_lock_hash: handoff.intent.shell_lock_hash(),
                world_lock_hash: handoff.intent.world_lock_hash(),
                world_open_plan_hash: handoff.intent.world_open_plan_hash(),
                diagnostic_ref: None,
            },
        ) {
            let _ = crate::supervisor::publish_child_exit(&root, &report, Some(&handoff.intent));
        }
    }
    commands.insert_resource(SealedLaunchHandoff(handoff));
    exits.write(AppExit::Success);
    true
}

fn first_focusable(start: &ProductionMemoryStart) -> Option<SemanticNodeId> {
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
