//! Shell semantic views: Home, create, continue, loading, recovery, and quit.

use std::collections::BTreeSet;

use crate::semantic::{SemanticKey, SemanticNode, SemanticRole, SemanticState};
use crate::views::keys::{surface_key, try_surface_key};
use crate::widgets::{ButtonWidget, ListItemWidget, ListWidget, ModalWidget, TextInputWidget};

/// Home continue/review enablement owned by the world-library contract.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum HomeContinueV1 {
    /// No recent world.
    None,
    /// Exact-ready recent world.
    Continue {
        /// Display label.
        label: String,
        /// Health / lock / durability summary.
        summary: String,
    },
    /// Recent world requires review before any write.
    Review {
        /// Display label.
        label: String,
        /// Actionable health reason.
        reason: String,
    },
}

/// Home information architecture: Continue, Worlds, New World, Packages/Profiles,
/// Settings, Diagnostics/About, Quit.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HomeViewV1 {
    /// Continue or Review enablement.
    pub continue_action: HomeContinueV1,
}

impl HomeViewV1 {
    /// Projects Home in frozen order.
    #[must_use]
    pub fn semantic_node(&self, focused: Option<&SemanticKey>) -> SemanticNode {
        let mut children = Vec::new();
        match &self.continue_action {
            HomeContinueV1::None => {}
            HomeContinueV1::Continue { label, summary } => children.push(
                ButtonWidget::new(
                    surface_key("home/continue"),
                    format!("Continue — {label}"),
                    Some(summary.clone()),
                    true,
                )
                .semantic_node(focused == Some(&surface_key("home/continue"))),
            ),
            HomeContinueV1::Review { label, reason } => children.push(
                ButtonWidget::new(
                    surface_key("home/review"),
                    format!("Review {label}"),
                    Some(reason.clone()),
                    true,
                )
                .semantic_node(focused == Some(&surface_key("home/review"))),
            ),
        }
        for (key, name, description) in [
            ("home/worlds", "Worlds", "Browse, recover, or manage worlds"),
            (
                "home/new-world",
                "New World",
                "Create from current profile safe defaults",
            ),
            (
                "home/packages-profiles",
                "Packages and Profiles",
                "Inspect the frozen lock and profile drafts",
            ),
            (
                "home/settings",
                "Settings",
                "Accessibility remains reachable before and during errors",
            ),
            (
                "home/diagnostics-about",
                "Diagnostics and About",
                "Lock, health, and product identity",
            ),
            ("home/quit", "Quit", "Confirm leaving the product"),
        ] {
            children.push(
                ButtonWidget::new(surface_key(key), name, Some(description.to_owned()), true)
                    .semantic_node(focused == Some(&surface_key(key))),
            );
        }
        SemanticNode {
            key: surface_key("shell/home"),
            role: SemanticRole::Navigation,
            name: "Home".to_owned(),
            value: None,
            description: Some("Package-driven client shell".to_owned()),
            state: SemanticState::default(),
            actions: BTreeSet::new(),
            children,
        }
    }
}

/// New-world create form. IME/CJK committed text is held beside this view.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NewWorldViewV1 {
    /// Committed world name.
    pub name: String,
}

impl NewWorldViewV1 {
    /// Projects Back, IME name field, and Quick Create.
    #[must_use]
    pub fn semantic_node(&self, focused: Option<&SemanticKey>) -> SemanticNode {
        SemanticNode {
            key: surface_key("shell/new-world"),
            role: SemanticRole::Group,
            name: "New World".to_owned(),
            value: None,
            description: Some("Create from current profile safe defaults".to_owned()),
            state: SemanticState::default(),
            actions: BTreeSet::new(),
            children: vec![
                ButtonWidget::new(
                    surface_key("new-world/back"),
                    "Back",
                    Some("Return to home".to_owned()),
                    true,
                )
                .semantic_node(focused == Some(&surface_key("new-world/back"))),
                TextInputWidget {
                    key: surface_key("new-world/name"),
                    name: "World name".to_owned(),
                    description: Some(
                        "Accepts composition and IME committed-text events".to_owned(),
                    ),
                    value: self.name.clone(),
                    enabled: true,
                }
                .semantic_node(focused == Some(&surface_key("new-world/name"))),
                ButtonWidget::new(
                    surface_key("new-world/quick-create"),
                    "Quick Create",
                    Some("Resolve and publish a transaction from current safe defaults".to_owned()),
                    true,
                )
                .semantic_node(focused == Some(&surface_key("new-world/quick-create"))),
            ],
        }
    }
}

/// One catalog world or recovery row.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorldRowV1 {
    /// Stable semantic key text, for example `world:1/<uuid>`.
    pub key: String,
    /// Display or recovery label.
    pub name: String,
    /// Card state value.
    pub value: String,
    /// Actionable description, never a generic cannot-open string.
    pub description: String,
    /// Whether the row is an alert (corrupt/recovery).
    pub alert: bool,
    /// Whether the row is currently focused in the catalog.
    pub enabled: bool,
}

impl WorldRowV1 {
    fn semantic_item(&self) -> Result<ListItemWidget, crate::SemanticKeyError> {
        Ok(ListItemWidget {
            key: try_surface_key(self.key.clone())?,
            name: self.name.clone(),
            value: Some(self.value.clone()),
            enabled: self.enabled,
        })
    }
}

/// Worlds library including recovery alerts.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct WorldsViewV1 {
    /// Visible rows including failed headers.
    pub rows: Vec<WorldRowV1>,
    /// Focused world key.
    pub focused_world: Option<String>,
}

impl WorldsViewV1 {
    /// Projects Back, Trash, and the catalog list.
    ///
    /// # Errors
    ///
    /// Returns [`crate::SemanticKeyError`] if a world key is not a stable path.
    pub fn semantic_node(
        &self,
        focused: Option<&SemanticKey>,
    ) -> Result<SemanticNode, crate::SemanticKeyError> {
        let items = self
            .rows
            .iter()
            .map(WorldRowV1::semantic_item)
            .collect::<Result<Vec<_>, _>>()?;
        let selected = self
            .focused_world
            .as_ref()
            .and_then(|key| try_surface_key(key.clone()).ok());
        let list = ListWidget {
            key: surface_key("worlds"),
            name: "Worlds".to_owned(),
            items,
            selected,
        }
        .semantic_node(focused);
        Ok(SemanticNode {
            key: surface_key("shell/worlds"),
            role: SemanticRole::Group,
            name: "World library".to_owned(),
            value: None,
            description: Some("The shell does not open a writer".to_owned()),
            state: SemanticState::default(),
            actions: BTreeSet::new(),
            children: vec![
                ButtonWidget::new(
                    surface_key("worlds/back"),
                    "Back",
                    Some("Return to home".to_owned()),
                    true,
                )
                .semantic_node(focused == Some(&surface_key("worlds/back"))),
                ButtonWidget::new(
                    surface_key("worlds/trash"),
                    "Trash",
                    Some("Review managed-trash restore actions".to_owned()),
                    true,
                )
                .semantic_node(focused == Some(&surface_key("worlds/trash"))),
                list,
            ],
        })
    }
}

/// Managed-trash recovery list.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct RecoveryViewV1 {
    /// Trash rows.
    pub rows: Vec<WorldRowV1>,
}

impl RecoveryViewV1 {
    /// Projects restore / restore-as-clone.
    ///
    /// # Errors
    ///
    /// Returns [`crate::SemanticKeyError`] if a trash key is not a stable path.
    pub fn semantic_node(
        &self,
        focused: Option<&SemanticKey>,
    ) -> Result<SemanticNode, crate::SemanticKeyError> {
        let mut children = vec![
            ButtonWidget::new(
                surface_key("recovery/back"),
                "Back",
                Some("Return to the world library".to_owned()),
                true,
            )
            .semantic_node(focused == Some(&surface_key("recovery/back"))),
        ];
        for row in &self.rows {
            let key = try_surface_key(row.key.clone())?;
            children.push(SemanticNode {
                key: key.clone(),
                role: if row.alert {
                    SemanticRole::Alert
                } else {
                    SemanticRole::ListItem
                },
                name: row.name.clone(),
                value: Some(row.value.clone()),
                description: Some(row.description.clone()),
                state: SemanticState {
                    focusable: row.enabled,
                    focused: focused == Some(&key) && row.enabled,
                    disabled: !row.enabled,
                    expanded: None,
                    busy: false,
                },
                actions: if row.enabled {
                    [crate::SemanticAction::Activate].into_iter().collect()
                } else {
                    BTreeSet::new()
                },
                children: Vec::new(),
            });
        }
        Ok(SemanticNode {
            key: surface_key("shell/recovery"),
            role: SemanticRole::Group,
            name: "Recovery".to_owned(),
            value: None,
            description: Some(
                "Crash marker, read-only recovery, checkpoint, clone, trash, and restore"
                    .to_owned(),
            ),
            state: SemanticState::default(),
            actions: BTreeSet::new(),
            children,
        })
    }
}

/// Loading stages. Cancel after writer open requires the shutdown barrier.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LoadingViewV1 {
    /// Stage label.
    pub stage: String,
    /// Honest progress text.
    pub progress: String,
    /// Optional current item.
    pub current_item: Option<String>,
    /// Whether a world writer has opened.
    pub writer_opened: bool,
}

impl LoadingViewV1 {
    /// Projects status and a cancel control that fail-closes after writer open.
    #[must_use]
    pub fn semantic_node(&self, focused: Option<&SemanticKey>) -> SemanticNode {
        let cancel_enabled = !self.writer_opened;
        SemanticNode {
            key: surface_key("shell/loading"),
            role: SemanticRole::Group,
            name: "Loading".to_owned(),
            value: Some(self.stage.clone()),
            description: self.current_item.clone(),
            state: SemanticState {
                busy: true,
                ..SemanticState::default()
            },
            actions: BTreeSet::new(),
            children: vec![
                SemanticNode {
                    key: surface_key("loading/status"),
                    role: SemanticRole::Status,
                    name: self.stage.clone(),
                    value: Some(self.progress.clone()),
                    description: self.current_item.clone(),
                    state: SemanticState {
                        busy: true,
                        ..SemanticState::default()
                    },
                    actions: BTreeSet::new(),
                    children: Vec::new(),
                },
                ButtonWidget::new(
                    surface_key("loading/cancel"),
                    if cancel_enabled {
                        "Cancel loading"
                    } else {
                        "Cancel requires shutdown"
                    },
                    Some(if cancel_enabled {
                        "Cancel before a writer opens".to_owned()
                    } else {
                        "Writer is open. Cancel must enter the shutdown barrier.".to_owned()
                    }),
                    cancel_enabled,
                )
                .semantic_node(focused == Some(&surface_key("loading/cancel"))),
            ],
        }
    }
}

/// Packages/profiles placeholder. Graph edits are not runtime settings.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PackagesProfilesViewV1;

impl PackagesProfilesViewV1 {
    /// Projects a read-only lock identity surface.
    #[must_use]
    pub fn semantic_node(focused: Option<&SemanticKey>) -> SemanticNode {
        SemanticNode {
            key: surface_key("shell/packages-profiles"),
            role: SemanticRole::Group,
            name: "Packages and Profiles".to_owned(),
            value: None,
            description: Some(
                "Composition parameters require a candidate lock. They are not live settings."
                    .to_owned(),
            ),
            state: SemanticState::default(),
            actions: BTreeSet::new(),
            children: vec![
                ButtonWidget::new(
                    surface_key("packages-profiles/back"),
                    "Back",
                    Some("Return to home".to_owned()),
                    true,
                )
                .semantic_node(focused == Some(&surface_key("packages-profiles/back"))),
            ],
        }
    }
}

/// Diagnostics/about.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DiagnosticsAboutViewV1 {
    /// Product or lock identity summary.
    pub summary: String,
}

impl DiagnosticsAboutViewV1 {
    /// Projects diagnostics and about.
    #[must_use]
    pub fn semantic_node(&self, focused: Option<&SemanticKey>) -> SemanticNode {
        SemanticNode {
            key: surface_key("shell/diagnostics-about"),
            role: SemanticRole::Group,
            name: "Diagnostics and About".to_owned(),
            value: Some(self.summary.clone()),
            description: Some("Lock, health, and product identity".to_owned()),
            state: SemanticState::default(),
            actions: BTreeSet::new(),
            children: vec![
                ButtonWidget::new(
                    surface_key("diagnostics-about/back"),
                    "Back",
                    Some("Return to home".to_owned()),
                    true,
                )
                .semantic_node(focused == Some(&surface_key("diagnostics-about/back"))),
            ],
        }
    }
}

/// Quit confirmation. Shell exit without intent ends the product.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct QuitConfirmViewV1;

impl QuitConfirmViewV1 {
    /// Projects confirm/cancel.
    #[must_use]
    pub fn semantic_node(focused: Option<&SemanticKey>) -> SemanticNode {
        ModalWidget {
            key: surface_key("modal/quit"),
            name: "Quit".to_owned(),
            description: Some("Leave the product. No launch intent is published.".to_owned()),
            confirm: ButtonWidget::new(
                surface_key("modal/quit/confirm"),
                "Quit",
                Some("End the product without a world handoff".to_owned()),
                true,
            ),
            cancel: ButtonWidget::new(
                surface_key("modal/quit/cancel"),
                "Cancel",
                Some("Return to home".to_owned()),
                true,
            ),
        }
        .semantic_node(focused)
    }
}
