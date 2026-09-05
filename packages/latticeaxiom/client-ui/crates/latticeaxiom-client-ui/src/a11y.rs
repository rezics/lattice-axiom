//! AccessKit helpers that check role, name, value, description, state, and action.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::semantic::{SemanticAction, SemanticNode, SemanticRole};

const fn action_name(action: SemanticAction) -> &'static str {
    match action {
        SemanticAction::Activate => "activate",
        SemanticAction::Back => "back",
        SemanticAction::Confirm => "confirm",
        SemanticAction::Cancel => "cancel",
        SemanticAction::FocusNext => "focus-next",
        SemanticAction::FocusPrevious => "focus-previous",
        SemanticAction::NavUp => "nav-up",
        SemanticAction::NavDown => "nav-down",
        SemanticAction::NavLeft => "nav-left",
        SemanticAction::NavRight => "nav-right",
        SemanticAction::Clear => "clear",
    }
}

/// AccessKit role name used by the future Bevy adapter.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "PascalCase")]
pub enum AccessKitRole {
    /// Application window root.
    Application,
    /// Navigation landmark.
    Navigation,
    /// Generic group.
    Group,
    /// Button.
    Button,
    /// Switch / toggle.
    Switch,
    /// Slider.
    Slider,
    /// List.
    List,
    /// List item.
    ListItem,
    /// Text input, including IME.
    TextInput,
    /// Modal dialog.
    Dialog,
    /// Status.
    Status,
    /// Alert.
    Alert,
}

impl AccessKitRole {
    /// Maps a semantic role onto the AccessKit vocabulary.
    #[must_use]
    pub const fn from_semantic(role: SemanticRole) -> Self {
        match role {
            SemanticRole::Application => Self::Application,
            SemanticRole::Navigation => Self::Navigation,
            SemanticRole::Group => Self::Group,
            SemanticRole::Button => Self::Button,
            SemanticRole::Switch => Self::Switch,
            SemanticRole::Slider => Self::Slider,
            SemanticRole::List => Self::List,
            SemanticRole::ListItem => Self::ListItem,
            SemanticRole::TextInput => Self::TextInput,
            SemanticRole::Dialog => Self::Dialog,
            SemanticRole::Status => Self::Status,
            SemanticRole::Alert => Self::Alert,
        }
    }
}

/// One AccessKit-shaped node used by headless automation.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AccessKitNode {
    /// Stable semantic key, never a Bevy entity.
    pub key: String,
    /// AccessKit role.
    pub role: AccessKitRole,
    /// Accessible name.
    pub name: String,
    /// Accessible value.
    pub value: Option<String>,
    /// Accessible description.
    pub description: Option<String>,
    /// Whether the node is focusable.
    pub focusable: bool,
    /// Whether the node is focused.
    pub focused: bool,
    /// Whether the node is disabled.
    pub disabled: bool,
    /// Advertised actions in stable order.
    pub actions: Vec<String>,
    /// Child nodes in visual order.
    pub children: Vec<AccessKitNode>,
}

impl AccessKitNode {
    /// Projects a semantic node into AccessKit-shaped fields.
    #[must_use]
    pub fn from_semantic(node: &SemanticNode) -> Self {
        Self {
            key: node.key.as_str().to_owned(),
            role: AccessKitRole::from_semantic(node.role),
            name: node.name.clone(),
            value: node.value.clone(),
            description: node.description.clone(),
            focusable: node.state.focusable,
            focused: node.state.focused,
            disabled: node.state.disabled,
            actions: node
                .actions
                .iter()
                .map(|action| action_name(*action).to_owned())
                .collect(),
            children: node.children.iter().map(Self::from_semantic).collect(),
        }
    }
}

/// Checks that a semantic tree satisfies the first-version AccessKit contract.
///
/// # Errors
///
/// Returns [`A11yError`] when names, focus, uniqueness, or root cardinality
/// fail.
pub fn check_accesskit_tree(root: &SemanticNode) -> Result<AccessKitNode, A11yError> {
    if root.role != SemanticRole::Application {
        return Err(A11yError::MissingApplicationRoot);
    }
    if root.has_second_application_root() {
        return Err(A11yError::SecondApplicationRoot);
    }
    let mut seen = BTreeSet::new();
    let mut focused = 0_usize;
    check_node(root, &mut seen, &mut focused)?;
    if focused > 1 {
        return Err(A11yError::MultipleFocusedNodes);
    }
    Ok(AccessKitNode::from_semantic(root))
}

fn check_node(
    node: &SemanticNode,
    seen: &mut BTreeSet<String>,
    focused: &mut usize,
) -> Result<(), A11yError> {
    if !seen.insert(node.key.as_str().to_owned()) {
        return Err(A11yError::DuplicateKey);
    }
    if node.name.trim().is_empty() {
        return Err(A11yError::EmptyName);
    }
    if node.state.focused {
        if !node.state.focusable || node.state.disabled {
            return Err(A11yError::FocusedUnfocusable);
        }
        *focused = focused.saturating_add(1);
    }
    if node.state.focusable && !node.state.disabled && node.actions.is_empty() {
        return Err(A11yError::FocusableWithoutAction);
    }
    for child in &node.children {
        check_node(child, seen, focused)?;
    }
    Ok(())
}

/// AccessKit contract violation.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum A11yError {
    /// The tree does not start at the unique application root.
    #[error("accessibility tree is missing the application root")]
    MissingApplicationRoot,
    /// A private second UI root was projected.
    #[error("accessibility tree contains a second application root")]
    SecondApplicationRoot,
    /// Two nodes share a stable key.
    #[error("accessibility tree contains a duplicate stable key")]
    DuplicateKey,
    /// A node has no accessible name.
    #[error("accessibility node has an empty name")]
    EmptyName,
    /// The focused node is not a legal focus target.
    #[error("focused accessibility node is not focusable")]
    FocusedUnfocusable,
    /// More than one node claims focus.
    #[error("accessibility tree has multiple focused nodes")]
    MultipleFocusedNodes,
    /// A focusable control advertises no actions.
    #[error("focusable accessibility node advertises no actions")]
    FocusableWithoutAction,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::semantic::SemanticKey;
    use crate::widgets::{ButtonWidget, application_root};

    fn key(value: &str) -> SemanticKey {
        SemanticKey::new(value).expect("static a11y key")
    }

    #[test]
    fn accesskit_projection_requires_named_focusable_actions() {
        let button = ButtonWidget {
            key: key("pause/resume"),
            name: "Resume".to_owned(),
            description: Some("Return to play".to_owned()),
            enabled: true,
        }
        .semantic_node(true);
        let root = application_root(key("shell"), "Lattice Axiom", vec![button]).expect("root");
        let node = check_accesskit_tree(&root).expect("a11y");
        assert_eq!(node.role, AccessKitRole::Application);
        assert_eq!(node.children[0].role, AccessKitRole::Button);
        assert!(node.children[0].actions.contains(&"activate".to_owned()));
    }
}
