//! Stable semantic keys, nodes, and commands independent of Bevy entities.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use thiserror::Error;
use unicode_normalization::is_nfc;

/// Maximum semantic-key byte length.
pub const MAX_SEMANTIC_KEY_BYTES: usize = 256;

/// Stable semantic identity independent of Bevy entities and row indices.
#[derive(Clone, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct SemanticKey(String);

impl SemanticKey {
    /// Creates a non-empty path-like stable key.
    ///
    /// # Errors
    ///
    /// Returns [`SemanticKeyError`] when the key is empty, oversized, not NFC,
    /// or contains characters outside lowercase ASCII, digits, `/`, `-`, `_`,
    /// `:`.
    pub fn new(value: impl Into<String>) -> Result<Self, SemanticKeyError> {
        let value = value.into();
        if value.is_empty() {
            return Err(SemanticKeyError::Empty);
        }
        if value.len() > MAX_SEMANTIC_KEY_BYTES {
            return Err(SemanticKeyError::TooLong {
                actual: value.len(),
            });
        }
        if !is_nfc(&value) {
            return Err(SemanticKeyError::NotNfc);
        }
        if value.bytes().any(|byte| {
            !(byte.is_ascii_lowercase()
                || byte.is_ascii_digit()
                || matches!(byte, b'/' | b'-' | b'_' | b':'))
        }) {
            return Err(SemanticKeyError::ForbiddenCharacter);
        }
        Ok(Self(value))
    }

    /// Returns the stable string form.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Invalid semantic key.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum SemanticKeyError {
    /// Empty keys are not stable.
    #[error("semantic key is empty")]
    Empty,
    /// Key exceeds its bounded representation.
    #[error("semantic key has {actual} bytes; limit is 256")]
    TooLong {
        /// Actual byte count.
        actual: usize,
    },
    /// Key is not Unicode NFC.
    #[error("semantic key is not Unicode NFC")]
    NotNfc,
    /// Key contains a character outside the stable path alphabet.
    #[error("semantic key contains a forbidden character")]
    ForbiddenCharacter,
}

/// Accessibility role projected to AccessKit.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum SemanticRole {
    /// Unique application root. Surfaces may not spawn a second root.
    Application,
    /// Navigation region.
    Navigation,
    /// Group of related controls.
    Group,
    /// Action button.
    Button,
    /// Toggle or switch.
    Switch,
    /// Bounded numeric slider.
    Slider,
    /// List container.
    List,
    /// Selectable list row.
    ListItem,
    /// Editable text field.
    TextInput,
    /// Modal dialog.
    Dialog,
    /// Status or progress output.
    Status,
    /// Alert requiring attention.
    Alert,
}

/// Stable logical action advertised by a node.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum SemanticAction {
    /// Activate the focused control.
    Activate,
    /// Navigate to the previous surface.
    Back,
    /// Confirm a modal.
    Confirm,
    /// Cancel a modal, capture, or draft.
    Cancel,
    /// Move focus forward.
    FocusNext,
    /// Move focus backward.
    FocusPrevious,
    /// Move focus up.
    NavUp,
    /// Move focus down.
    NavDown,
    /// Move focus left.
    NavLeft,
    /// Move focus right.
    NavRight,
    /// Clear a captured binding.
    Clear,
}

/// First-version client surface actions shared by keyboard and gamepad maps.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ClientSurfaceActionV1 {
    /// Directional up.
    NavUp,
    /// Directional down.
    NavDown,
    /// Directional left.
    NavLeft,
    /// Directional right.
    NavRight,
    /// Next focus.
    NavNext,
    /// Previous focus.
    NavPrevious,
    /// Activate.
    Activate,
    /// Back / Escape safety.
    Back,
}

impl ClientSurfaceActionV1 {
    /// Returns the matching semantic action.
    #[must_use]
    pub const fn semantic_action(self) -> SemanticAction {
        match self {
            Self::NavUp => SemanticAction::NavUp,
            Self::NavDown => SemanticAction::NavDown,
            Self::NavLeft => SemanticAction::NavLeft,
            Self::NavRight => SemanticAction::NavRight,
            Self::NavNext => SemanticAction::FocusNext,
            Self::NavPrevious => SemanticAction::FocusPrevious,
            Self::Activate => SemanticAction::Activate,
            Self::Back => SemanticAction::Back,
        }
    }
}

/// Accessibility state expressed without renderer-specific types.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "Accessibility state is an orthogonal AccessKit projection, not a state machine"
)]
pub struct SemanticState {
    /// Whether this node may receive focus.
    pub focusable: bool,
    /// Whether it is currently focused.
    pub focused: bool,
    /// Whether actions are disabled.
    pub disabled: bool,
    /// Whether a disclosure group is expanded.
    pub expanded: Option<bool>,
    /// Whether a status is busy.
    pub busy: bool,
}

/// One renderer-neutral accessibility node.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SemanticNode {
    /// Stable key used for async focus restoration.
    pub key: SemanticKey,
    /// Accessibility role.
    pub role: SemanticRole,
    /// Localized accessible name.
    pub name: String,
    /// Current value, when meaningful.
    pub value: Option<String>,
    /// Explanatory accessible description.
    pub description: Option<String>,
    /// Dynamic accessibility state.
    pub state: SemanticState,
    /// Supported logical actions in stable order.
    pub actions: BTreeSet<SemanticAction>,
    /// Child nodes in visual and focus order.
    pub children: Vec<SemanticNode>,
}

impl SemanticNode {
    /// Finds a node by stable identity.
    #[must_use]
    pub fn find(&self, target: &SemanticKey) -> Option<&Self> {
        if &self.key == target {
            return Some(self);
        }
        self.children.iter().find_map(|child| child.find(target))
    }

    /// Returns focusable node keys in semantic/visual traversal order.
    #[must_use]
    pub fn focus_order(&self) -> Vec<SemanticKey> {
        let mut order = Vec::new();
        self.append_focus_order(&mut order);
        order
    }

    fn append_focus_order(&self, order: &mut Vec<SemanticKey>) {
        if self.state.focusable && !self.state.disabled {
            order.push(self.key.clone());
        }
        for child in &self.children {
            child.append_focus_order(order);
        }
    }

    /// Returns whether this tree contains more than one application root.
    #[must_use]
    pub fn has_second_application_root(&self) -> bool {
        self.children.iter().any(Self::contains_application)
    }

    fn contains_application(&self) -> bool {
        self.role == SemanticRole::Application
            || self.children.iter().any(Self::contains_application)
    }
}

/// Physical source translated into a semantic command.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum InputSource {
    /// Keyboard or keyboard-only accessibility flow.
    Keyboard,
    /// Standard game controller.
    Gamepad,
    /// Pointer.
    Mouse,
    /// Headless test or tool injection.
    Headless,
}

/// Presentation-neutral command injected into a surface.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SemanticCommand {
    /// Stable target node.
    pub target: SemanticKey,
    /// Advertised action to invoke.
    pub action: SemanticAction,
    /// Physical or headless source.
    pub source: InputSource,
}

/// Validates semantic command injection against the current tree.
///
/// # Errors
///
/// Returns [`SemanticCommandError`] if the target vanished, is disabled, or
/// does not advertise the requested action.
pub fn validate_semantic_command(
    tree: &SemanticNode,
    command: &SemanticCommand,
) -> Result<(), SemanticCommandError> {
    let node = tree
        .find(&command.target)
        .ok_or(SemanticCommandError::UnknownTarget)?;
    if node.state.disabled {
        return Err(SemanticCommandError::DisabledTarget);
    }
    if !node.actions.contains(&command.action) {
        return Err(SemanticCommandError::UnsupportedAction);
    }
    Ok(())
}

/// Rejected semantic command.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum SemanticCommandError {
    /// Target no longer exists in the current semantic tree.
    #[error("semantic command target does not exist")]
    UnknownTarget,
    /// Target is visible but disabled.
    #[error("semantic command target is disabled")]
    DisabledTarget,
    /// Requested action is not advertised by the target.
    #[error("semantic command action is not supported by the target")]
    UnsupportedAction,
}

/// Presentation-neutral editable text driven by IME events.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ImeTextState {
    committed: String,
    composing: Option<String>,
}

impl ImeTextState {
    /// Applies a composition event without prematurely committing the text.
    pub fn set_composition(&mut self, text: impl Into<String>) {
        self.composing = Some(text.into());
    }

    /// Commits text and clears any active composition.
    pub fn commit(&mut self, text: &str) {
        self.committed.push_str(text);
        self.composing = None;
    }

    /// Cancels the active composition while preserving committed text.
    pub fn cancel_composition(&mut self) {
        self.composing = None;
    }

    /// Returns committed UTF-8 text.
    #[must_use]
    pub fn committed(&self) -> &str {
        &self.committed
    }

    /// Returns the uncommitted composition, when present.
    #[must_use]
    pub fn composing(&self) -> Option<&str> {
        self.composing.as_deref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keyboard_and_gamepad_share_the_same_logical_actions() {
        assert_eq!(
            ClientSurfaceActionV1::Activate.semantic_action(),
            SemanticAction::Activate
        );
        assert_eq!(
            ClientSurfaceActionV1::Back.semantic_action(),
            SemanticAction::Back
        );
    }

    #[test]
    fn ime_composition_does_not_commit_until_explicit_commit() {
        let mut ime = ImeTextState::default();
        ime.set_composition("你");
        assert_eq!(ime.committed(), "");
        assert_eq!(ime.composing(), Some("你"));
        ime.commit("你好");
        assert_eq!(ime.committed(), "你好");
        assert_eq!(ime.composing(), None);
    }
}
