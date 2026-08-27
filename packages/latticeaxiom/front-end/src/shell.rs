//! Shell routing, semantic-tree projection, and replacement-process handoff.

use std::collections::BTreeSet;

use latticeaxiom_core::{
    CanonicalHash, CanonicalJsonError, StableId, WorldId, canonical_json_hash,
};
use latticeaxiom_launcher::{
    LaunchAttempt, LaunchGeneration, LaunchIntentDraftV1, LaunchIntentV1, LaunchModelError,
    LaunchTargetV1, SettingTransactionRevision,
};
use latticeaxiom_world_catalog::{WorldOpenAction, WorldOpenStatus};
use thiserror::Error;

use crate::{
    ClientShellGraph, HomePrimaryAction, LoadingState, RecoveryCue, SemanticActionId,
    SemanticCommand, SemanticCommandError, SemanticNode, SemanticNodeId, SemanticRole,
    SemanticState, TrashedWorldRecord, WorldCardAction, WorldListModel, WorldShellRecord,
    WorldgenProfileOption, validate_semantic_command,
};

/// Package-driven client-shell route.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ShellScreen {
    /// High-frequency home actions.
    Home,
    /// Bounded world catalog and recovery actions.
    Worlds,
    /// Quick and advanced creation entry.
    NewWorld,
    /// Reserved settings route. The start process does not advertise this
    /// route until it owns a transactional settings host.
    Settings,
    /// World activation progress.
    Loading,
    /// Managed-trash recovery list.
    Trash,
    /// Packages and profiles. Composition edits are not live settings.
    PackagesProfiles,
    /// Diagnostics and about.
    DiagnosticsAbout,
    /// Quit confirmation without a launch intent.
    QuitConfirm,
    /// Live world session; the start library is not the front surface.
    Playing,
    /// In-session pause overlay. Save and Exit are explicit; this is not a checkpoint.
    Pause,
}

/// Headless state used by both CI and the future Bevy client adapter.
#[derive(Clone, Debug)]
pub struct StartShellModel {
    graph: ClientShellGraph,
    /// Current route.
    pub screen: ShellScreen,
    /// Visible world library.
    pub worlds: WorldListModel,
    /// Managed-trash records in structural order.
    pub trash: Vec<TrashedWorldRecord>,
    /// World selected for recovery review.
    pub selected: Option<WorldId>,
    /// Current loading state when routed to Loading.
    pub loading: Option<LoadingState>,
    worldgen_profiles: Vec<WorldgenProfileOption>,
    selected_worldgen_profile: Option<StableId>,
}

impl StartShellModel {
    /// Creates a shell only from a validated package closure.
    #[must_use]
    pub const fn new(graph: ClientShellGraph, worlds: WorldListModel) -> Self {
        Self {
            graph,
            screen: ShellScreen::Home,
            worlds,
            trash: Vec::new(),
            selected: None,
            loading: None,
            worldgen_profiles: Vec::new(),
            selected_worldgen_profile: None,
        }
    }

    /// Installs the bounded generation-profile catalog shown during creation.
    ///
    /// # Errors
    ///
    /// Returns [`ShellCommandError`] for duplicate identities, an empty
    /// catalog, or a selected identity that is not present.
    pub fn set_worldgen_profiles(
        &mut self,
        profiles: Vec<WorldgenProfileOption>,
        selected: &StableId,
    ) -> Result<(), ShellCommandError> {
        if profiles.is_empty() || profiles.len() > 32 {
            return Err(ShellCommandError::InvalidWorldgenProfiles);
        }
        let unique = profiles
            .iter()
            .map(|profile| &profile.id)
            .collect::<BTreeSet<_>>();
        if unique.len() != profiles.len() || !unique.contains(selected) {
            return Err(ShellCommandError::InvalidWorldgenProfiles);
        }
        self.worldgen_profiles = profiles;
        self.selected_worldgen_profile = Some(selected.clone());
        Ok(())
    }

    /// Returns the currently selected generation profile, when configured.
    #[must_use]
    pub const fn selected_worldgen_profile(&self) -> Option<&StableId> {
        self.selected_worldgen_profile.as_ref()
    }

    /// Returns the resolved package graph that owns this shell.
    #[must_use]
    pub const fn graph(&self) -> &ClientShellGraph {
        &self.graph
    }

    /// Builds role/name/value/description/state/action semantics in visual and
    /// focus order, with no Bevy entity identities.
    #[must_use]
    pub fn semantic_tree(&self) -> SemanticNode {
        let children = match self.screen {
            ShellScreen::Home => self.home_nodes(),
            ShellScreen::Worlds => self.world_nodes(),
            ShellScreen::NewWorld => self.new_world_nodes(),
            ShellScreen::Settings => Self::settings_nodes(),
            ShellScreen::Loading => self.loading_nodes(),
            ShellScreen::Trash => self.trash_nodes(),
            ShellScreen::PackagesProfiles => Self::packages_profiles_nodes(),
            ShellScreen::DiagnosticsAbout => Self::diagnostics_about_nodes(),
            ShellScreen::QuitConfirm => Self::quit_confirm_nodes(),
            ShellScreen::Playing => Self::playing_nodes(),
            ShellScreen::Pause => Self::pause_nodes(),
        };
        SemanticNode {
            id: node_id("shell"),
            role: SemanticRole::Application,
            name: "Lattice Axiom".to_owned(),
            value: None,
            description: Some("Package-driven client shell".to_owned()),
            state: SemanticState::default(),
            actions: BTreeSet::new(),
            children,
        }
    }

    /// Validates and applies a semantic command from keyboard, controller, or
    /// a headless injector.
    ///
    /// # Errors
    ///
    /// Returns [`ShellCommandError`] if the current tree rejects the command or
    /// the command does not map to the current route.
    #[allow(clippy::too_many_lines)]
    pub fn inject(&mut self, command: &SemanticCommand) -> Result<ShellEffect, ShellCommandError> {
        let tree = self.semantic_tree();
        validate_semantic_command(&tree, command)?;
        let target = command.target.as_str();
        let action = command.action;
        let effect = if matches!(
            action,
            SemanticActionId::FocusNext | SemanticActionId::FocusPrevious
        ) {
            ShellEffect::FocusTraversal(action)
        } else if self.screen == ShellScreen::Playing
            && (target == "playing/pause" || action == SemanticActionId::PauseWorld)
        {
            self.screen = ShellScreen::Pause;
            ShellEffect::Navigate(ShellScreen::Pause)
        } else if self.screen == ShellScreen::Pause
            && (target == "pause/resume"
                || action == SemanticActionId::ResumeWorld
                || action == SemanticActionId::Back)
        {
            self.screen = ShellScreen::Playing;
            ShellEffect::Navigate(ShellScreen::Playing)
        } else if self.screen == ShellScreen::Pause
            && (target == "pause/save" || action == SemanticActionId::SaveWorld)
        {
            ShellEffect::RequestSaveWorld
        } else if self.screen == ShellScreen::Pause
            && (target == "pause/exit" || action == SemanticActionId::ExitWorld)
        {
            self.screen = ShellScreen::Home;
            ShellEffect::RequestExitWorld
        } else if target == "home/worlds" || action == SemanticActionId::OpenWorlds {
            self.screen = ShellScreen::Worlds;
            ShellEffect::Navigate(ShellScreen::Worlds)
        } else if target == "home/new-world"
            || (action == SemanticActionId::QuickCreate && self.screen != ShellScreen::NewWorld)
        {
            self.screen = ShellScreen::NewWorld;
            ShellEffect::Navigate(ShellScreen::NewWorld)
        } else if target == "home/packages-profiles" {
            self.screen = ShellScreen::PackagesProfiles;
            ShellEffect::Navigate(ShellScreen::PackagesProfiles)
        } else if target == "home/diagnostics-about" {
            self.screen = ShellScreen::DiagnosticsAbout;
            ShellEffect::Navigate(ShellScreen::DiagnosticsAbout)
        } else if target == "home/quit" {
            self.screen = ShellScreen::QuitConfirm;
            ShellEffect::Navigate(ShellScreen::QuitConfirm)
        } else if target == "modal/quit/confirm" {
            ShellEffect::RequestQuitProduct
        } else if action == SemanticActionId::Back {
            self.screen = ShellScreen::Home;
            ShellEffect::Navigate(ShellScreen::Home)
        } else if target == "home/continue" || action == SemanticActionId::ContinueWorld {
            match self.worlds.home_primary_action() {
                HomePrimaryAction::Continue { world_id, .. } => {
                    ShellEffect::RequestExactWorldLaunch(world_id)
                }
                _ => return Err(ShellCommandError::NoExactContinue),
            }
        } else if target == "home/review" || action == SemanticActionId::ReviewWorld {
            let HomePrimaryAction::Review { world_id, .. } = self.worlds.home_primary_action()
            else {
                return Err(ShellCommandError::NoReviewTarget);
            };
            self.selected = Some(world_id);
            self.screen = ShellScreen::Worlds;
            ShellEffect::ReviewWorld(world_id)
        } else if target == "worlds/trash" {
            self.screen = ShellScreen::Trash;
            ShellEffect::Navigate(ShellScreen::Trash)
        } else if target == "trash/back" {
            self.screen = ShellScreen::Worlds;
            ShellEffect::Navigate(ShellScreen::Worlds)
        } else if target.starts_with("world:") {
            self.world_command_effect(command)?
        } else if target.starts_with("trash:") {
            let record = self
                .trash
                .iter()
                .find(|record| {
                    let id = trash_semantic_id(record);
                    id == command.target
                        || command
                            .target
                            .as_str()
                            .starts_with(&format!("{}/", id.as_str()))
                })
                .ok_or(ShellCommandError::UnknownWorld)?;
            if command.target.as_str().ends_with("/clone")
                || action == SemanticActionId::RestoreWorldAsClone
            {
                ShellEffect::RequestRestoreTrashAsClone(record.tombstone.world_id)
            } else if matches!(
                action,
                SemanticActionId::Activate | SemanticActionId::RestoreWorld
            ) {
                ShellEffect::RequestRestoreTrash(record.tombstone.world_id)
            } else {
                return Err(ShellCommandError::UnmappedCommand);
            }
        } else if target == "loading/cancel" {
            let loading = self
                .loading
                .as_ref()
                .ok_or(ShellCommandError::NoLoadingState)?;
            ShellEffect::CancelLoading(loading.cancel_disposition())
        } else if self.screen == ShellScreen::NewWorld
            && (target.starts_with("new-world/profile/")
                || action == SemanticActionId::SelectWorldgenProfile)
        {
            let selected = self
                .worldgen_profiles
                .iter()
                .enumerate()
                .find(|(index, _)| target == format!("new-world/profile/{index}"))
                .map(|(_, profile)| profile.id.clone())
                .ok_or(ShellCommandError::UnmappedCommand)?;
            self.selected_worldgen_profile = Some(selected);
            ShellEffect::WorldgenProfileSelected
        } else if target == "new-world/quick-create" || action == SemanticActionId::QuickCreate {
            ShellEffect::RequestQuickCreate
        } else {
            return Err(ShellCommandError::UnmappedCommand);
        };
        Ok(effect)
    }

    fn world_command_effect(
        &mut self,
        command: &SemanticCommand,
    ) -> Result<ShellEffect, ShellCommandError> {
        let record = self
            .worlds
            .records()
            .iter()
            .find(|record| {
                record.semantic_id() == command.target
                    || command
                        .target
                        .as_str()
                        .starts_with(&format!("{}/", record.semantic_id().as_str()))
            })
            .ok_or(ShellCommandError::UnknownWorld)?;
        let world_id = record.world_id();
        self.selected = Some(world_id);
        let suffix = command
            .target
            .as_str()
            .strip_prefix(&format!("{}/", record.semantic_id().as_str()));
        Ok(match (suffix, command.action) {
            (None, SemanticActionId::Activate | SemanticActionId::ReviewWorld)
            | (Some("details" | "rename" | "storage"), SemanticActionId::Activate) => {
                ShellEffect::ReviewWorld(world_id)
            }
            (Some("play"), SemanticActionId::Activate | SemanticActionId::PlayExact)
            | (None, SemanticActionId::PlayExact) => ShellEffect::RequestExactWorldLaunch(world_id),
            (Some("preflight" | "prepare" | "compatible" | "repair" | "read-only"), _)
            | (_, SemanticActionId::RunPreflight) => ShellEffect::RequestRunPreflight(world_id),
            (Some("checkpoint"), _) | (_, SemanticActionId::CreateCheckpoint) => {
                ShellEffect::RequestCheckpoint(world_id)
            }
            (Some("clone" | "migrate"), _) | (_, SemanticActionId::CloneWorld) => {
                ShellEffect::RequestClone(world_id)
            }
            (Some("export"), _) | (_, SemanticActionId::ExportWorld) => {
                ShellEffect::RequestExport(world_id)
            }
            (Some("trash"), _) | (_, SemanticActionId::MoveToTrash) => {
                ShellEffect::RequestMoveToTrash(world_id)
            }
            (Some("restore-checkpoint"), _) | (_, SemanticActionId::RestoreCheckpoint) => {
                ShellEffect::RequestRestoreCheckpoint(world_id)
            }
            (Some("lease"), _) | (_, SemanticActionId::RecoverStaleLease) => {
                ShellEffect::RequestRecoverStaleLease(world_id)
            }
            (Some("inspect"), _) | (_, SemanticActionId::InspectRecovery) => {
                ShellEffect::RequestInspectRecovery(world_id)
            }
            _ => return Err(ShellCommandError::UnmappedCommand),
        })
    }

    fn home_nodes(&self) -> Vec<SemanticNode> {
        let mut nodes = Vec::new();
        match self.worlds.home_primary_action() {
            HomePrimaryAction::Continue { label, world_id } => nodes.push(button(
                "home/continue",
                format!("Continue — {label}"),
                format!(
                    "ReadyExact; world {world_id}; health/lock/durability summaries come from the catalog"
                ),
                [SemanticActionId::Activate, SemanticActionId::ContinueWorld],
            )),
            HomePrimaryAction::Review { label, health, .. } => nodes.push(button(
                "home/review",
                format!("Review {label}"),
                format!("Recent world requires review: {health:?}"),
                [SemanticActionId::Activate, SemanticActionId::ReviewWorld],
            )),
            HomePrimaryAction::Worlds => {}
        }
        nodes.extend([
            button(
                "home/worlds",
                "Worlds",
                "Browse, recover, or manage worlds",
                [SemanticActionId::Activate, SemanticActionId::OpenWorlds],
            ),
            button(
                "home/new-world",
                "New World",
                "Create from current profile safe defaults",
                [SemanticActionId::Activate, SemanticActionId::QuickCreate],
            ),
            button(
                "home/packages-profiles",
                "Packages and Profiles",
                "Inspect the frozen lock and profile drafts",
                [SemanticActionId::Activate],
            ),
            button(
                "home/diagnostics-about",
                "Diagnostics and About",
                "Lock, health, and product identity",
                [SemanticActionId::Activate],
            ),
            button(
                "home/quit",
                "Quit",
                "Confirm leaving the product",
                [SemanticActionId::Activate],
            ),
        ]);
        nodes
    }

    fn world_nodes(&self) -> Vec<SemanticNode> {
        let mut nodes = vec![
            button(
                "worlds/back",
                "Back",
                "Return to home",
                [SemanticActionId::Back],
            ),
            button(
                "worlds/trash",
                "Trash",
                "Review managed-trash restore actions",
                [SemanticActionId::Activate],
            ),
        ];
        nodes.extend(self.worlds.records().iter().map(|record| {
            let cues = record.recovery_cues();
            let description = if cues.is_empty() {
                format!(
                    "Catalog card {:?}; the shell does not open a writer",
                    record.card_state()
                )
            } else {
                cues.iter()
                    .map(|cue| format!("{}: {}", cue.title, cue.description))
                    .collect::<Vec<_>>()
                    .join(" ")
            };
            let mut children = cues
                .iter()
                .enumerate()
                .filter_map(|(index, cue)| recovery_alert_node(record, index, cue))
                .collect::<Vec<_>>();
            children.extend(
                record
                    .actions()
                    .iter()
                    .filter_map(|action| world_action_node(record, action)),
            );
            SemanticNode {
                id: record.semantic_id(),
                role: if matches!(record.health(), crate::WorldHealth::CatalogFailure(_))
                    || !cues.is_empty()
                {
                    SemanticRole::Alert
                } else {
                    SemanticRole::ListItem
                },
                name: record.display_label(),
                value: Some(format!("{:?}", record.card_state())),
                description: Some(description),
                state: SemanticState {
                    focusable: true,
                    focused: self.worlds.focused() == Some(&record.semantic_id()),
                    ..SemanticState::default()
                },
                actions: BTreeSet::from([
                    SemanticActionId::Activate,
                    SemanticActionId::ReviewWorld,
                ]),
                children,
            }
        }));
        nodes
    }

    fn trash_nodes(&self) -> Vec<SemanticNode> {
        let mut nodes = vec![button(
            "trash/back",
            "Back",
            "Return to the world library",
            [SemanticActionId::Back],
        )];
        nodes.extend(self.trash.iter().map(|record| {
            let id = trash_semantic_id(record);
            SemanticNode {
                id: id.clone(),
                role: SemanticRole::ListItem,
                name: record.tombstone.display_name.as_str().to_owned(),
                value: Some(record.tombstone.world_id.to_string()),
                description: Some(
                    "Restore original identity or restore-as-clone. Live WorldId overwrite is blocked."
                        .to_owned(),
                ),
                state: SemanticState {
                    focusable: true,
                    ..SemanticState::default()
                },
                actions: BTreeSet::from([
                    SemanticActionId::Activate,
                    SemanticActionId::RestoreWorld,
                    SemanticActionId::RestoreWorldAsClone,
                ]),
                children: vec![
                    button(
                        &format!("{}/restore", id.as_str()),
                        "Restore",
                        "Restore the original identity when it is not live",
                        [SemanticActionId::Activate, SemanticActionId::RestoreWorld],
                    ),
                    button(
                        &format!("{}/clone", id.as_str()),
                        "Restore as clone",
                        "Publish a new WorldId and re-key; never overwrite a live world",
                        [
                            SemanticActionId::Activate,
                            SemanticActionId::RestoreWorldAsClone,
                        ],
                    ),
                ],
            }
        }));
        nodes
    }

    fn new_world_nodes(&self) -> Vec<SemanticNode> {
        let mut nodes = vec![
            button(
                "new-world/back",
                "Back",
                "Return to home",
                [SemanticActionId::Back],
            ),
            SemanticNode {
                id: node_id("new-world/name"),
                role: SemanticRole::TextInput,
                name: "World name".to_owned(),
                value: None,
                description: Some("Accepts composition and IME committed-text events".to_owned()),
                state: SemanticState {
                    focusable: true,
                    ..SemanticState::default()
                },
                actions: BTreeSet::new(),
                children: Vec::new(),
            },
        ];
        if let Some(selected) = self.selected_worldgen_profile.as_ref()
            && let Some(profile) = self
                .worldgen_profiles
                .iter()
                .find(|profile| &profile.id == selected)
        {
            nodes.push(SemanticNode {
                id: node_id("new-world/profile-status"),
                role: SemanticRole::Status,
                name: "Terrain profile".to_owned(),
                value: Some(profile.label.clone()),
                description: Some(profile.description.clone()),
                state: SemanticState::default(),
                actions: BTreeSet::new(),
                children: Vec::new(),
            });
        }
        nodes.extend(
            self.worldgen_profiles
                .iter()
                .enumerate()
                .map(|(index, profile)| {
                    button(
                        &format!("new-world/profile/{index}"),
                        profile.label.clone(),
                        profile.description.clone(),
                        [
                            SemanticActionId::Activate,
                            SemanticActionId::SelectWorldgenProfile,
                        ],
                    )
                }),
        );
        nodes.push(button(
            "new-world/quick-create",
            "Create World",
            "Publish a transaction with the selected resolved terrain profile",
            [SemanticActionId::Activate, SemanticActionId::QuickCreate],
        ));
        nodes
    }

    fn settings_nodes() -> Vec<SemanticNode> {
        vec![
            button(
            "settings/back",
            "Back",
            "Return to home",
            [SemanticActionId::Back],
            ),
            SemanticNode {
                id: node_id("settings/status"),
                role: SemanticRole::Status,
                name: "Settings".to_owned(),
                value: None,
                description: Some(
                    "Settings are owned by the in-session pause surface; the start shell does not edit them"
                        .to_owned(),
                ),
                state: SemanticState::default(),
                actions: BTreeSet::new(),
                children: Vec::new(),
            },
        ]
    }

    fn loading_nodes(&self) -> Vec<SemanticNode> {
        let (name, value, busy) = self.loading.as_ref().map_or_else(
            || ("Loading".to_owned(), None, false),
            |loading| {
                (
                    loading.stage.fallback_label().to_owned(),
                    Some(format!("{:?}", loading.progress)),
                    loading.stage != crate::LoadingStage::Playing,
                )
            },
        );
        vec![
            SemanticNode {
                id: node_id("loading/status"),
                role: SemanticRole::Status,
                name,
                value,
                description: self
                    .loading
                    .as_ref()
                    .and_then(|loading| loading.current_item.clone()),
                state: SemanticState {
                    busy,
                    ..SemanticState::default()
                },
                actions: BTreeSet::new(),
                children: Vec::new(),
            },
            button(
                "loading/cancel",
                "Cancel loading",
                "Cancellation uses the writer-safe boundary for the current stage",
                [SemanticActionId::Activate, SemanticActionId::CancelLoading],
            ),
        ]
    }

    fn playing_nodes() -> Vec<SemanticNode> {
        vec![button(
            "playing/pause",
            "Pause",
            "Open the pause overlay without mutating world state",
            [SemanticActionId::Activate, SemanticActionId::PauseWorld],
        )]
    }

    fn pause_nodes() -> Vec<SemanticNode> {
        vec![
            button(
                "pause/resume",
                "Resume",
                "Return to the live world session without writing",
                [SemanticActionId::Activate, SemanticActionId::ResumeWorld],
            ),
            button(
                "pause/save",
                "Save",
                "Flush dirty chunks through the sealed writer and close it",
                [SemanticActionId::Activate, SemanticActionId::SaveWorld],
            ),
            button(
                "pause/exit",
                "Exit",
                "Leave the world session and return to the start shell",
                [SemanticActionId::Activate, SemanticActionId::ExitWorld],
            ),
        ]
    }

    fn packages_profiles_nodes() -> Vec<SemanticNode> {
        vec![button(
            "packages-profiles/back",
            "Back",
            "Return to home. Composition edits require a candidate lock.",
            [SemanticActionId::Back],
        )]
    }

    fn diagnostics_about_nodes() -> Vec<SemanticNode> {
        vec![button(
            "diagnostics-about/back",
            "Back",
            "Return to home",
            [SemanticActionId::Back],
        )]
    }

    fn quit_confirm_nodes() -> Vec<SemanticNode> {
        vec![
            button(
                "modal/quit/confirm",
                "Quit",
                "End the product without a world handoff",
                [SemanticActionId::Activate],
            ),
            button(
                "modal/quit/cancel",
                "Cancel",
                "Return to home",
                [SemanticActionId::Activate, SemanticActionId::Back],
            ),
        ]
    }
}

fn button<const N: usize>(
    id: &str,
    name: impl Into<String>,
    description: impl Into<String>,
    actions: [SemanticActionId; N],
) -> SemanticNode {
    SemanticNode {
        id: node_id(id),
        role: SemanticRole::Button,
        name: name.into(),
        value: None,
        description: Some(description.into()),
        state: SemanticState {
            focusable: true,
            ..SemanticState::default()
        },
        actions: BTreeSet::from(actions),
        children: Vec::new(),
    }
}

fn node_id(value: &str) -> SemanticNodeId {
    match SemanticNodeId::new(value) {
        Ok(id) => id,
        Err(error) => unreachable!("validated static shell semantic ID: {error}"),
    }
}

fn recovery_alert_node(
    record: &WorldShellRecord,
    index: usize,
    cue: &RecoveryCue,
) -> Option<SemanticNode> {
    let id = SemanticNodeId::new(format!(
        "{}/recovery-{index}",
        record.semantic_id().as_str()
    ))
    .ok()?;
    Some(SemanticNode {
        id,
        role: SemanticRole::Alert,
        name: cue.title.clone(),
        value: Some(format!("{:?}", cue.code)),
        description: Some(cue.description.clone()),
        state: SemanticState::default(),
        actions: BTreeSet::new(),
        children: Vec::new(),
    })
}

fn world_action_node(record: &WorldShellRecord, action: &WorldCardAction) -> Option<SemanticNode> {
    let id = record.action_semantic_id(action)?;
    let name = match action {
        WorldCardAction::PlayExact => "Play".to_owned(),
        WorldCardAction::RunPreflight => "Run preflight".to_owned(),
        WorldCardAction::CreateCheckpoint => "Create checkpoint".to_owned(),
        WorldCardAction::Duplicate => "Clone".to_owned(),
        WorldCardAction::Export | WorldCardAction::Preflight(WorldOpenAction::Export) => {
            "Export".to_owned()
        }
        WorldCardAction::MoveToTrash => "Move to trash".to_owned(),
        WorldCardAction::InspectRecovery => "Inspect recovery".to_owned(),
        WorldCardAction::Rename => "Rename".to_owned(),
        WorldCardAction::OpenStorageLocation => "Open storage location".to_owned(),
        WorldCardAction::Details => "Details".to_owned(),
        WorldCardAction::Preflight(WorldOpenAction::UseFrozenLock) => "Play exact".to_owned(),
        WorldCardAction::Preflight(WorldOpenAction::OpenReadOnly) => "Open read-only".to_owned(),
        WorldCardAction::Preflight(WorldOpenAction::RestoreCheckpoint { .. }) => {
            "Restore checkpoint".to_owned()
        }
        WorldCardAction::Preflight(WorldOpenAction::RepairHeader { .. }) => {
            "Repair header".to_owned()
        }
        WorldCardAction::Preflight(WorldOpenAction::PreparePackage { .. }) => {
            "Prepare package".to_owned()
        }
        WorldCardAction::Preflight(WorldOpenAction::ResolveCompatibleGraph) => {
            "Resolve compatible graph".to_owned()
        }
        WorldCardAction::Preflight(WorldOpenAction::CloneAndMigrate { .. }) => {
            "Clone and migrate".to_owned()
        }
        WorldCardAction::Preflight(WorldOpenAction::RecoverStaleLease) => {
            "Recover stale lease".to_owned()
        }
    };
    Some(button(
        id.as_str(),
        name,
        format!("Catalog action {action:?}; the shell does not open a writer"),
        [SemanticActionId::Activate, action.semantic_action()],
    ))
}

fn trash_semantic_id(record: &TrashedWorldRecord) -> SemanticNodeId {
    match SemanticNodeId::new(format!(
        "trash:{}/{}/{}",
        record.location.root.0, record.location.world_id, record.location.entry_id
    )) {
        Ok(id) => id,
        Err(error) => unreachable!("validated trash semantic ID: {error}"),
    }
}

/// Observable result of one accepted shell command.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ShellEffect {
    /// Route changed.
    Navigate(ShellScreen),
    /// Input adapter should move focus using current semantic order.
    FocusTraversal(SemanticActionId),
    /// Exact-ready world is selected for launch handoff.
    RequestExactWorldLaunch(WorldId),
    /// World is selected for compatibility/recovery review.
    ReviewWorld(WorldId),
    /// Quick-create form should emit its typed intent.
    RequestQuickCreate,
    /// The creation form selected a different generation profile.
    WorldgenProfileSelected,
    /// Loading cancellation policy derived from the writer boundary.
    CancelLoading(crate::LoadingCancelDisposition),
    /// Host should flush dirty chunks through the sealed writer and close it.
    RequestSaveWorld,
    /// Host should drop the live world session and return to the start shell.
    RequestExitWorld,
    /// Plan a catalog checkpoint without opening a writer.
    RequestCheckpoint(WorldId),
    /// Plan a clone with a new world identity.
    RequestClone(WorldId),
    /// Plan a move into managed trash.
    RequestMoveToTrash(WorldId),
    /// Plan a bounded export that excludes secrets.
    RequestExport(WorldId),
    /// Run metadata-only preflight for the selected world.
    RequestRunPreflight(WorldId),
    /// Inspect a corrupt catalog row.
    RequestInspectRecovery(WorldId),
    /// Restore a verified checkpoint as the next safe step.
    RequestRestoreCheckpoint(WorldId),
    /// Restore a managed-trash entry without overwriting a live world.
    RequestRestoreTrash(WorldId),
    /// Restore a managed-trash entry as a clone with a new world identity.
    RequestRestoreTrashAsClone(WorldId),
    /// Plan stale exclusive-lease recovery without opening a writer.
    RequestRecoverStaleLease(WorldId),
    /// Confirm product quit without publishing a launch intent.
    RequestQuitProduct,
}

/// Invalid shell command injection.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum ShellCommandError {
    /// Accessibility tree rejected the command.
    #[error(transparent)]
    Semantic(#[from] SemanticCommandError),
    /// Current recent world does not allow Continue.
    #[error("no exact-ready recent world is available for Continue")]
    NoExactContinue,
    /// No recent world needs review.
    #[error("no recent world is available for review")]
    NoReviewTarget,
    /// World row disappeared during async refresh.
    #[error("world command target disappeared")]
    UnknownWorld,
    /// Loading route has no loading state.
    #[error("loading command has no loading state")]
    NoLoadingState,
    /// Advertised command has no route mapping.
    #[error("semantic command is not mapped on the current route")]
    UnmappedCommand,
    /// Host-contributed world-generation profiles are empty, duplicated, or mis-selected.
    #[error("world-generation profile catalog is invalid")]
    InvalidWorldgenProfiles,
}

/// Inputs needed to seal a replacement-process world launch intent.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LaunchHandoffContext {
    /// Monotonic launcher generation.
    pub generation: LaunchGeneration,
    /// Intent issue time.
    pub issued_at_ms: u64,
    /// Intent expiry within launcher policy.
    pub expires_at_ms: u64,
    /// Exact client-shell lock.
    pub shell_lock_hash: CanonicalHash,
    /// Exact frozen world lock.
    pub world_lock_hash: CanonicalHash,
    /// Last settings transaction confirmed by the shutdown barrier.
    pub confirmed_setting_transaction_revision: SettingTransactionRevision,
}

/// Handoff that must be atomically published before the current process exits.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LaunchHandoff {
    /// Authenticated cross-process envelope.
    pub intent: LaunchIntentV1,
    /// Explicit client lifecycle requirement.
    pub disposition: ClientProcessDisposition,
}

impl LaunchHandoff {
    /// Builds a world launch only from a `ReadyExact` plan offering the frozen lock.
    ///
    /// No Bevy `App` is created here. The launcher atomically persists this
    /// intent after shutdown barriers and starts a replacement process whose
    /// one fresh application enters the world.
    ///
    /// # Errors
    ///
    /// Returns [`LaunchHandoffError`] if the world is not exact-ready, plan
    /// hashing fails, or launcher intent validation fails.
    pub fn for_ready_exact(
        record: &WorldShellRecord,
        context: LaunchHandoffContext,
    ) -> Result<Self, LaunchHandoffError> {
        let plan = record
            .open_plan
            .as_ref()
            .ok_or(LaunchHandoffError::NotReadyExact)?;
        if plan.status != WorldOpenStatus::ReadyExact
            || !plan.actions.contains(&WorldOpenAction::UseFrozenLock)
        {
            return Err(LaunchHandoffError::NotReadyExact);
        }
        let world_open_plan_hash = canonical_json_hash(plan)?;
        let intent = LaunchIntentV1::seal(LaunchIntentDraftV1 {
            generation: context.generation,
            attempt: LaunchAttempt::FIRST,
            issued_at_ms: context.issued_at_ms,
            expires_at_ms: context.expires_at_ms,
            target: LaunchTargetV1::World {
                world_id: record.world_id(),
            },
            shell_lock_hash: context.shell_lock_hash,
            world_lock_hash: Some(context.world_lock_hash),
            world_open_plan_hash: Some(world_open_plan_hash),
            confirmed_setting_transaction_revision: context.confirmed_setting_transaction_revision,
        })?;
        Ok(Self {
            intent,
            disposition: ClientProcessDisposition::ExitAfterAtomicIntentPublish,
        })
    }
}

/// Client lifecycle after a launch handoff.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ClientProcessDisposition {
    /// Exit current process; external launcher starts the replacement process.
    ExitAfterAtomicIntentPublish,
}

/// Failure to build replacement-process handoff.
#[derive(Debug, Error)]
pub enum LaunchHandoffError {
    /// Continue/launch is not allowed for this record.
    #[error("world launch requires a ReadyExact plan offering UseFrozenLock")]
    NotReadyExact,
    /// Canonical plan hashing failed.
    #[error(transparent)]
    Canonical(#[from] CanonicalJsonError),
    /// Launcher rejected the intent shape or lifetime.
    #[error(transparent)]
    Launcher(#[from] LaunchModelError),
}
