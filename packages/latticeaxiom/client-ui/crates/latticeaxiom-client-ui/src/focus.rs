//! Unique focus ownership derived from a surface epoch and stable keys.

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::semantic::{SemanticKey, SemanticNode};
use crate::surface::{CursorPolicy, SurfaceEpoch};

/// Unique focus owner for the current surface epoch.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FocusOwner {
    epoch: SurfaceEpoch,
    key: Option<SemanticKey>,
}

impl FocusOwner {
    /// Creates an unfocused owner bound to `epoch`.
    #[must_use]
    pub const fn empty(epoch: SurfaceEpoch) -> Self {
        Self { epoch, key: None }
    }

    /// Binds a stable key to `epoch`.
    #[must_use]
    pub const fn new(epoch: SurfaceEpoch, key: SemanticKey) -> Self {
        Self {
            epoch,
            key: Some(key),
        }
    }

    /// Returns the epoch that owns this focus.
    #[must_use]
    pub const fn epoch(&self) -> SurfaceEpoch {
        self.epoch
    }

    /// Returns the focused semantic key, if any.
    #[must_use]
    pub const fn key(&self) -> Option<&SemanticKey> {
        self.key.as_ref()
    }
}

/// Focus restoration and directional navigation over a semantic tree.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FocusController {
    owner: FocusOwner,
}

impl FocusController {
    /// Creates a controller for `owner`.
    #[must_use]
    pub const fn new(owner: FocusOwner) -> Self {
        Self { owner }
    }

    /// Returns the current owner.
    #[must_use]
    pub const fn owner(&self) -> &FocusOwner {
        &self.owner
    }

    /// Restores focus after a tree diff using a stable key, never a row index.
    ///
    /// When the previous key vanished, the nearest remaining focusable key in
    /// visual order is selected.
    #[must_use]
    pub fn restore(&mut self, tree: &SemanticNode, epoch: SurfaceEpoch) -> &FocusOwner {
        let order = tree.focus_order();
        let restored = self
            .owner
            .key
            .as_ref()
            .and_then(|previous| nearest_focusable(&order, previous));
        self.owner = FocusOwner {
            epoch,
            key: restored,
        };
        &self.owner
    }

    /// Moves focus to the next or previous focusable key.
    ///
    /// # Errors
    ///
    /// Returns [`FocusError::EmptyTraversal`] when no focusable node exists.
    pub fn move_linear(
        &mut self,
        tree: &SemanticNode,
        direction: LinearFocusDirection,
    ) -> Result<&FocusOwner, FocusError> {
        let order = tree.focus_order();
        if order.is_empty() {
            return Err(FocusError::EmptyTraversal);
        }
        let current = self
            .owner
            .key
            .as_ref()
            .and_then(|key| order.iter().position(|candidate| candidate == key));
        let next = match (current, direction) {
            (Some(index), LinearFocusDirection::Next) => {
                order.get(index.saturating_add(1)).or_else(|| order.first())
            }
            (Some(index), LinearFocusDirection::Previous) => {
                if index == 0 {
                    order.last()
                } else {
                    order.get(index.saturating_sub(1))
                }
            }
            (None, LinearFocusDirection::Next | LinearFocusDirection::Previous) => order.first(),
        }
        .cloned()
        .ok_or(FocusError::EmptyTraversal)?;
        self.owner.key = Some(next);
        Ok(&self.owner)
    }
}

/// Linear focus traversal direction.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum LinearFocusDirection {
    /// Tab / next.
    Next,
    /// Shift+Tab / previous.
    Previous,
}

/// Cursor and focus policy pair derived from the active route.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FocusCursorProjection {
    /// Unique focus owner.
    pub focus: FocusOwner,
    /// Cursor policy mechanically derived from the route.
    pub cursor: CursorPolicy,
}

fn nearest_focusable(order: &[SemanticKey], previous: &SemanticKey) -> Option<SemanticKey> {
    if let Some(exact) = order.iter().find(|key| *key == previous) {
        return Some(exact.clone());
    }
    order
        .iter()
        .rev()
        .find(|key| key.as_str() < previous.as_str())
        .cloned()
        .or_else(|| order.first().cloned())
}

/// Focus navigation failure.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum FocusError {
    /// The current tree has no focusable node.
    #[error("focus traversal is empty")]
    EmptyTraversal,
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;
    use crate::semantic::{SemanticRole, SemanticState};
    use crate::widgets::ButtonWidget;

    fn key(value: &str) -> SemanticKey {
        SemanticKey::new(value).expect("static focus key")
    }

    fn tree() -> SemanticNode {
        SemanticNode {
            key: key("root"),
            role: SemanticRole::Group,
            name: "root".to_owned(),
            value: None,
            description: None,
            state: SemanticState::default(),
            actions: BTreeSet::new(),
            children: vec![
                ButtonWidget {
                    key: key("a"),
                    name: "A".to_owned(),
                    description: None,
                    enabled: true,
                }
                .semantic_node(false),
                ButtonWidget {
                    key: key("c"),
                    name: "C".to_owned(),
                    description: None,
                    enabled: true,
                }
                .semantic_node(false),
            ],
        }
    }

    #[test]
    fn vanished_focus_restores_nearest_stable_key() {
        let mut controller = FocusController::new(FocusOwner::new(SurfaceEpoch::FIRST, key("b")));
        let owner = controller.restore(&tree(), SurfaceEpoch::from_raw(2));
        assert_eq!(owner.key(), Some(&key("a")));
        assert_eq!(owner.epoch().get(), 2);
    }
}
