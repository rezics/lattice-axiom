//! Typed button, list, and modal primitives that emit semantic commands.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::semantic::{SemanticAction, SemanticKey, SemanticNode, SemanticRole, SemanticState};

/// Versioned control vocabulary major supported by this crate.
pub const CONTROL_VOCABULARY_MAJOR: u32 = 1;

/// Widget kinds that packages may request through typed rows.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum WidgetKind {
    /// Activate-only button.
    Button,
    /// Boolean toggle.
    Toggle,
    /// Bounded integer or float slider.
    Slider,
    /// Closed enum cycle.
    Cycle,
    /// Stable-key list.
    List,
    /// Editable text.
    Text,
    /// Modal dialog.
    Modal,
    /// Transient status toast.
    Toast,
    /// Read-only value row.
    ReadOnly,
    /// Command row that emits a typed action.
    Command,
    /// Key-binding capture row.
    KeyBinding,
}

impl WidgetKind {
    /// Returns the vocabulary identity for this kind at the crate major.
    #[must_use]
    pub const fn vocabulary_id(self) -> &'static str {
        match self {
            Self::Button => "latticeaxiom:ui-control/button@1",
            Self::Toggle => "latticeaxiom:ui-control/toggle@1",
            Self::Slider => "latticeaxiom:ui-control/slider@1",
            Self::Cycle => "latticeaxiom:ui-control/cycle@1",
            Self::List => "latticeaxiom:ui-control/list@1",
            Self::Text => "latticeaxiom:ui-control/text@1",
            Self::Modal => "latticeaxiom:ui-control/modal@1",
            Self::Toast => "latticeaxiom:ui-control/toast@1",
            Self::ReadOnly => "latticeaxiom:ui-control/read-only@1",
            Self::Command => "latticeaxiom:ui-control/command@1",
            Self::KeyBinding => "latticeaxiom:ui-control/key-binding@1",
        }
    }
}

/// Requested control vocabulary identity.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ControlVocabularyRef {
    /// Requested major.
    pub major: u32,
    /// Whether the row is required for surface construction.
    pub required: bool,
}

/// Validates that a required control major is supported.
///
/// # Errors
///
/// Returns [`WidgetError::UnsupportedRequiredMajor`] when a required control
/// requests a newer vocabulary major than this crate implements.
pub fn validate_control_vocabulary(requested: ControlVocabularyRef) -> Result<(), WidgetError> {
    if requested.required && requested.major > CONTROL_VOCABULARY_MAJOR {
        return Err(WidgetError::UnsupportedRequiredMajor {
            requested: requested.major,
            supported: CONTROL_VOCABULARY_MAJOR,
        });
    }
    Ok(())
}

/// Button semantics projected into the unique UI root.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ButtonWidget {
    /// Stable key.
    pub key: SemanticKey,
    /// Accessible name.
    pub name: String,
    /// Accessible description.
    pub description: Option<String>,
    /// Whether the button may be activated.
    pub enabled: bool,
}

impl ButtonWidget {
    /// Constructs an enabled or disabled button.
    #[must_use]
    pub fn new(
        key: SemanticKey,
        name: impl Into<String>,
        description: Option<String>,
        enabled: bool,
    ) -> Self {
        Self {
            key,
            name: name.into(),
            description,
            enabled,
        }
    }

    /// Projects button role, name, and activate/back actions.
    #[must_use]
    pub fn semantic_node(&self, focused: bool) -> SemanticNode {
        let mut actions = BTreeSet::from([SemanticAction::Activate]);
        if !self.enabled {
            actions.clear();
        }
        SemanticNode {
            key: self.key.clone(),
            role: SemanticRole::Button,
            name: self.name.clone(),
            value: None,
            description: self.description.clone(),
            state: SemanticState {
                focusable: self.enabled,
                focused: focused && self.enabled,
                disabled: !self.enabled,
                expanded: None,
                busy: false,
            },
            actions,
            children: Vec::new(),
        }
    }
}

/// One list row addressed by a stable key rather than a visual index.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ListItemWidget {
    /// Stable item key.
    pub key: SemanticKey,
    /// Accessible name.
    pub name: String,
    /// Optional value.
    pub value: Option<String>,
    /// Whether the row may be activated.
    pub enabled: bool,
}

/// List semantics that restore focus by stable item key.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ListWidget {
    /// List container key.
    pub key: SemanticKey,
    /// Accessible name.
    pub name: String,
    /// Ordered items.
    pub items: Vec<ListItemWidget>,
    /// Selected item key, when present.
    pub selected: Option<SemanticKey>,
}

impl ListWidget {
    /// Projects a list and its items in visual order.
    #[must_use]
    pub fn semantic_node(&self, focused: Option<&SemanticKey>) -> SemanticNode {
        let children = self
            .items
            .iter()
            .map(|item| {
                let selected = self.selected.as_ref() == Some(&item.key);
                SemanticNode {
                    key: item.key.clone(),
                    role: SemanticRole::ListItem,
                    name: item.name.clone(),
                    value: item.value.clone(),
                    description: selected.then(|| "selected".to_owned()),
                    state: SemanticState {
                        focusable: item.enabled,
                        focused: focused == Some(&item.key) && item.enabled,
                        disabled: !item.enabled,
                        expanded: None,
                        busy: false,
                    },
                    actions: if item.enabled {
                        BTreeSet::from([SemanticAction::Activate])
                    } else {
                        BTreeSet::new()
                    },
                    children: Vec::new(),
                }
            })
            .collect();
        SemanticNode {
            key: self.key.clone(),
            role: SemanticRole::List,
            name: self.name.clone(),
            value: self.selected.as_ref().map(|key| key.as_str().to_owned()),
            description: None,
            state: SemanticState::default(),
            actions: BTreeSet::new(),
            children,
        }
    }

    /// Restores selection after an async refresh using a stable key.
    #[must_use]
    pub fn restore_selection(&self, previous: Option<&SemanticKey>) -> Option<SemanticKey> {
        previous
            .and_then(|key| {
                self.items
                    .iter()
                    .find(|item| item.enabled && &item.key == key)
                    .map(|item| item.key.clone())
            })
            .or_else(|| {
                self.items
                    .iter()
                    .find(|item| item.enabled)
                    .map(|item| item.key.clone())
            })
    }
}

/// Modal dialog that owns focus while it is open.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ModalWidget {
    /// Dialog key.
    pub key: SemanticKey,
    /// Accessible name.
    pub name: String,
    /// Accessible description.
    pub description: Option<String>,
    /// Primary confirm button.
    pub confirm: ButtonWidget,
    /// Cancel / Back safety button.
    pub cancel: ButtonWidget,
}

impl ModalWidget {
    /// Projects a dialog that traps focus in confirm/cancel.
    #[must_use]
    pub fn semantic_node(&self, focused: Option<&SemanticKey>) -> SemanticNode {
        SemanticNode {
            key: self.key.clone(),
            role: SemanticRole::Dialog,
            name: self.name.clone(),
            value: None,
            description: self.description.clone(),
            state: SemanticState {
                focusable: false,
                focused: false,
                disabled: false,
                expanded: Some(true),
                busy: false,
            },
            actions: BTreeSet::from([SemanticAction::Cancel, SemanticAction::Back]),
            children: vec![
                self.confirm
                    .semantic_node(focused == Some(&self.confirm.key)),
                self.cancel.semantic_node(focused == Some(&self.cancel.key)),
            ],
        }
    }
}

/// Boolean toggle projected as a switch.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ToggleWidget {
    /// Stable key.
    pub key: SemanticKey,
    /// Accessible name.
    pub name: String,
    /// Accessible description.
    pub description: Option<String>,
    /// Current value.
    pub on: bool,
    /// Whether the switch may be activated.
    pub enabled: bool,
}

impl ToggleWidget {
    /// Projects switch role and activate action.
    #[must_use]
    pub fn semantic_node(&self, focused: bool) -> SemanticNode {
        SemanticNode {
            key: self.key.clone(),
            role: SemanticRole::Switch,
            name: self.name.clone(),
            value: Some(if self.on {
                "on".to_owned()
            } else {
                "off".to_owned()
            }),
            description: self.description.clone(),
            state: SemanticState {
                focusable: self.enabled,
                focused: focused && self.enabled,
                disabled: !self.enabled,
                expanded: None,
                busy: false,
            },
            actions: if self.enabled {
                BTreeSet::from([SemanticAction::Activate])
            } else {
                BTreeSet::new()
            },
            children: Vec::new(),
        }
    }
}

/// Bounded slider projected for integer or float rows.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SliderWidget {
    /// Stable key.
    pub key: SemanticKey,
    /// Accessible name.
    pub name: String,
    /// Accessible description.
    pub description: Option<String>,
    /// Current value as text.
    pub value: String,
    /// Whether the slider may be activated.
    pub enabled: bool,
}

impl SliderWidget {
    /// Projects slider role and activate action.
    #[must_use]
    pub fn semantic_node(&self, focused: bool) -> SemanticNode {
        SemanticNode {
            key: self.key.clone(),
            role: SemanticRole::Slider,
            name: self.name.clone(),
            value: Some(self.value.clone()),
            description: self.description.clone(),
            state: SemanticState {
                focusable: self.enabled,
                focused: focused && self.enabled,
                disabled: !self.enabled,
                expanded: None,
                busy: false,
            },
            actions: if self.enabled {
                BTreeSet::from([
                    SemanticAction::Activate,
                    SemanticAction::NavLeft,
                    SemanticAction::NavRight,
                ])
            } else {
                BTreeSet::new()
            },
            children: Vec::new(),
        }
    }
}

/// Editable text, including IME composition.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TextInputWidget {
    /// Stable key.
    pub key: SemanticKey,
    /// Accessible name.
    pub name: String,
    /// Accessible description.
    pub description: Option<String>,
    /// Committed value.
    pub value: String,
    /// Whether the field may receive focus.
    pub enabled: bool,
}

impl TextInputWidget {
    /// Projects text-input role. Composition is held in [`crate::ImeTextState`].
    #[must_use]
    pub fn semantic_node(&self, focused: bool) -> SemanticNode {
        SemanticNode {
            key: self.key.clone(),
            role: SemanticRole::TextInput,
            name: self.name.clone(),
            value: Some(self.value.clone()),
            description: self.description.clone(),
            state: SemanticState {
                focusable: self.enabled,
                focused: focused && self.enabled,
                disabled: !self.enabled,
                expanded: None,
                busy: false,
            },
            actions: if self.enabled {
                BTreeSet::from([SemanticAction::Activate])
            } else {
                BTreeSet::new()
            },
            children: Vec::new(),
        }
    }
}

/// Transient status toast.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ToastWidget {
    /// Stable key.
    pub key: SemanticKey,
    /// Accessible name.
    pub name: String,
    /// Accessible description.
    pub description: Option<String>,
}

impl ToastWidget {
    /// Projects a non-focusable status toast.
    #[must_use]
    pub fn semantic_node(&self) -> SemanticNode {
        SemanticNode {
            key: self.key.clone(),
            role: SemanticRole::Status,
            name: self.name.clone(),
            value: None,
            description: self.description.clone(),
            state: SemanticState::default(),
            actions: BTreeSet::new(),
            children: Vec::new(),
        }
    }
}

/// Widget command emitted instead of mutating route, cursor, or focus.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum WidgetCommand {
    /// Activate a button, list item, or command row.
    Activate {
        /// Target key.
        key: SemanticKey,
    },
    /// Confirm the open modal.
    Confirm {
        /// Modal key.
        key: SemanticKey,
    },
    /// Dismiss the open modal through Cancel or Back.
    Dismiss {
        /// Modal key.
        key: SemanticKey,
    },
}

/// Widget contract violation.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum WidgetError {
    /// A required control vocabulary major is newer than this crate.
    #[error(
        "required UI control vocabulary major {requested} is unsupported; crate supports {supported}"
    )]
    UnsupportedRequiredMajor {
        /// Requested major.
        requested: u32,
        /// Supported major.
        supported: u32,
    },
}

/// Creates the unique application root. Additional application roles are forbidden.
///
/// # Errors
///
/// Returns [`UiRootError::SecondApplicationRoot`] when `children` already
/// contain an application role.
pub fn application_root(
    key: SemanticKey,
    name: impl Into<String>,
    children: Vec<SemanticNode>,
) -> Result<SemanticNode, UiRootError> {
    let root = SemanticNode {
        key,
        role: SemanticRole::Application,
        name: name.into(),
        value: None,
        description: Some("Package-driven client surface".to_owned()),
        state: SemanticState::default(),
        actions: BTreeSet::new(),
        children,
    };
    if root.has_second_application_root() {
        return Err(UiRootError::SecondApplicationRoot);
    }
    Ok(root)
}

/// Attempt to create a private second UI root.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum UiRootError {
    /// Packages and widgets may not spawn a second application root.
    #[error("client UI forbids a second application root")]
    SecondApplicationRoot,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(value: &str) -> SemanticKey {
        SemanticKey::new(value).expect("static widget key")
    }

    #[test]
    fn required_newer_control_major_fails_closed() {
        assert_eq!(
            validate_control_vocabulary(ControlVocabularyRef {
                major: 2,
                required: true,
            }),
            Err(WidgetError::UnsupportedRequiredMajor {
                requested: 2,
                supported: 1,
            })
        );
        assert_eq!(
            validate_control_vocabulary(ControlVocabularyRef {
                major: 2,
                required: false,
            }),
            Ok(())
        );
    }

    #[test]
    fn list_restores_selection_by_stable_key_not_index() {
        let list = ListWidget {
            key: key("list"),
            name: "Worlds".to_owned(),
            items: vec![
                ListItemWidget {
                    key: key("world:b"),
                    name: "B".to_owned(),
                    value: None,
                    enabled: true,
                },
                ListItemWidget {
                    key: key("world:a"),
                    name: "A".to_owned(),
                    value: None,
                    enabled: true,
                },
            ],
            selected: None,
        };
        assert_eq!(
            list.restore_selection(Some(&key("world:a"))),
            Some(key("world:a"))
        );
    }

    #[test]
    fn application_root_rejects_a_nested_application() {
        let nested = SemanticNode {
            key: key("private-root"),
            role: SemanticRole::Application,
            name: "private".to_owned(),
            value: None,
            description: None,
            state: SemanticState::default(),
            actions: BTreeSet::new(),
            children: Vec::new(),
        };
        assert_eq!(
            application_root(key("shell"), "Lattice Axiom", vec![nested]),
            Err(UiRootError::SecondApplicationRoot)
        );
    }
}
