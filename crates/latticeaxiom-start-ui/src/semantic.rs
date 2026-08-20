//! Stable accessibility tree, logical input, and numeric layout contracts.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Stable semantic identity independent of Bevy entities and row indices.
#[derive(Clone, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct SemanticNodeId(String);

impl SemanticNodeId {
    /// Creates a non-empty path-like stable ID.
    ///
    /// # Errors
    ///
    /// Returns [`SemanticNodeIdError`] when the ID is empty, oversized, or
    /// contains characters outside lowercase ASCII, digits, `/`, `-`, `_`, `:`.
    pub fn new(value: impl Into<String>) -> Result<Self, SemanticNodeIdError> {
        let value = value.into();
        if value.is_empty() {
            return Err(SemanticNodeIdError::Empty);
        }
        if value.len() > 256 {
            return Err(SemanticNodeIdError::TooLong {
                actual: value.len(),
            });
        }
        if value.bytes().any(|byte| {
            !(byte.is_ascii_lowercase()
                || byte.is_ascii_digit()
                || matches!(byte, b'/' | b'-' | b'_' | b':'))
        }) {
            return Err(SemanticNodeIdError::ForbiddenCharacter);
        }
        Ok(Self(value))
    }

    /// Returns the stable string form.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Invalid semantic node identity.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum SemanticNodeIdError {
    /// Empty IDs are not stable keys.
    #[error("semantic node ID is empty")]
    Empty,
    /// ID exceeds its bounded wire representation.
    #[error("semantic node ID has {actual} bytes; limit is 256")]
    TooLong {
        /// Actual byte count.
        actual: usize,
    },
    /// ID could not be used as a stable semantic path.
    #[error("semantic node ID contains a forbidden character")]
    ForbiddenCharacter,
}

/// Accessibility role projected by the future Bevy/AccessKit adapter.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum SemanticRole {
    /// Root application surface.
    Application,
    /// Navigation region.
    Navigation,
    /// Group of related controls.
    Group,
    /// Action button.
    Button,
    /// Selectable world-list row.
    ListItem,
    /// Editable text field.
    TextInput,
    /// Status or progress output.
    Status,
    /// Alert requiring attention.
    Alert,
}

/// Stable logical action advertised in accessibility nodes and input maps.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum SemanticActionId {
    /// Activate the focused control.
    Activate,
    /// Navigate to the previous surface.
    Back,
    /// Move focus forward.
    FocusNext,
    /// Move focus backward.
    FocusPrevious,
    /// Open the world library.
    OpenWorlds,
    /// Open settings.
    OpenSettings,
    /// Submit a quick-create intent.
    QuickCreate,
    /// Continue the exact-ready recent world.
    ContinueWorld,
    /// Review a recent world that is not exact-ready.
    ReviewWorld,
    /// Cancel a loading stage at a safe boundary.
    CancelLoading,
    /// Apply a validated settings draft.
    ApplySettings,
    /// Roll back prepared settings or a reversible preview.
    RollbackSettings,
    /// Move a world into managed trash.
    MoveToTrash,
    /// Restore an entry from managed trash.
    RestoreWorld,
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
    pub id: SemanticNodeId,
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
    pub actions: BTreeSet<SemanticActionId>,
    /// Child nodes in visual and focus order.
    pub children: Vec<SemanticNode>,
}

impl SemanticNode {
    /// Finds a node by stable identity.
    #[must_use]
    pub fn find(&self, target: &SemanticNodeId) -> Option<&Self> {
        if &self.id == target {
            return Some(self);
        }
        self.children.iter().find_map(|child| child.find(target))
    }

    /// Returns focusable node IDs in semantic/visual traversal order.
    #[must_use]
    pub fn focus_order(&self) -> Vec<SemanticNodeId> {
        let mut order = Vec::new();
        self.append_focus_order(&mut order);
        order
    }

    fn append_focus_order(&self, order: &mut Vec<SemanticNodeId>) {
        if self.state.focusable && !self.state.disabled {
            order.push(self.id.clone());
        }
        for child in &self.children {
            child.append_focus_order(order);
        }
    }
}

/// Physical source translated into a semantic command.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum InputSource {
    /// Keyboard or keyboard-only accessibility flow.
    Keyboard,
    /// Standard game controller.
    Controller,
    /// Headless test or tool injection.
    Headless,
}

/// Presentation-neutral command injected into the shell state machine.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SemanticCommand {
    /// Stable target node.
    pub target: SemanticNodeId,
    /// Advertised action to invoke.
    pub action: SemanticActionId,
    /// Physical or headless source.
    pub source: InputSource,
}

/// Validates semantic command injection against the current tree.
///
/// # Errors
///
/// Returns [`SemanticCommandError`] if the target vanished during an async
/// refresh, is disabled, or does not advertise the requested action.
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

/// Keys with stable shell-navigation meaning.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum KeyboardSemanticInput {
    /// Enter or Space.
    Activate,
    /// Escape.
    Back,
    /// Tab.
    Next,
    /// Shift+Tab.
    Previous,
}

/// Standard-controller buttons with stable shell-navigation meaning.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ControllerSemanticInput {
    /// South face button.
    Activate,
    /// East face button.
    Back,
    /// D-pad down/right.
    Next,
    /// D-pad up/left.
    Previous,
}

/// Maps keyboard input to the same logical IDs used by accessibility actions.
#[must_use]
pub const fn keyboard_action(input: KeyboardSemanticInput) -> SemanticActionId {
    match input {
        KeyboardSemanticInput::Activate => SemanticActionId::Activate,
        KeyboardSemanticInput::Back => SemanticActionId::Back,
        KeyboardSemanticInput::Next => SemanticActionId::FocusNext,
        KeyboardSemanticInput::Previous => SemanticActionId::FocusPrevious,
    }
}

/// Maps controller input to the same logical IDs used by accessibility actions.
#[must_use]
pub const fn controller_action(input: ControllerSemanticInput) -> SemanticActionId {
    match input {
        ControllerSemanticInput::Activate => SemanticActionId::Activate,
        ControllerSemanticInput::Back => SemanticActionId::Back,
        ControllerSemanticInput::Next => SemanticActionId::FocusNext,
        ControllerSemanticInput::Previous => SemanticActionId::FocusPrevious,
    }
}

/// Integer logical viewport used by headless layout verification.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct LogicalViewport {
    /// Logical width.
    pub width: u32,
    /// Logical height.
    pub height: u32,
}

/// Supported first-version UI scale.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum UiScale {
    /// 100% UI scale.
    One,
    /// 200% UI scale.
    Two,
}

impl UiScale {
    const fn factor(self) -> u32 {
        match self {
            Self::One => 1,
            Self::Two => 2,
        }
    }
}

/// Scroll position proving a focusable control can be brought into view.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ReachableControl {
    /// Stable semantic control.
    pub id: SemanticNodeId,
    /// Minimum content scroll offset that reveals the control.
    pub reveal_scroll_offset: u32,
}

/// Machine-readable numeric layout evidence.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct LayoutEvidence {
    /// Viewport used by the calculation.
    pub viewport: LogicalViewport,
    /// UI scale used by the calculation.
    pub scale: UiScale,
    /// Height reserved for scrollable content.
    pub scroll_viewport_height: u32,
    /// Total content height.
    pub content_height: u32,
    /// Focusable controls and a valid revealing scroll offset.
    pub reachable: Vec<ReachableControl>,
}

impl LayoutEvidence {
    /// Returns whether every essential primary/recovery action is reachable.
    #[must_use]
    pub fn essentials_reachable(&self, essentials: &[SemanticNodeId]) -> bool {
        essentials
            .iter()
            .all(|id| self.reachable.iter().any(|row| &row.id == id))
    }
}

/// Calculates the fixed header/footer plus scroll-region contract.
///
/// # Errors
///
/// Returns [`LayoutContractError`] if the supported minimum viewport is not
/// met, chrome leaves no scroll region, or focus IDs are duplicated.
pub fn verify_scroll_layout(
    viewport: LogicalViewport,
    scale: UiScale,
    focus_order: &[SemanticNodeId],
) -> Result<LayoutEvidence, LayoutContractError> {
    if viewport.width < 800 || viewport.height < 600 {
        return Err(LayoutContractError::ViewportBelowMinimum);
    }
    let factor = scale.factor();
    let header = 72_u32.saturating_mul(factor);
    let footer = 64_u32.saturating_mul(factor);
    let scroll_viewport_height = viewport
        .height
        .saturating_sub(header.saturating_add(footer));
    if scroll_viewport_height < 88 {
        return Err(LayoutContractError::NoUsableScrollRegion);
    }
    let unique = focus_order.iter().collect::<BTreeSet<_>>();
    if unique.len() != focus_order.len() {
        return Err(LayoutContractError::DuplicateStableKey);
    }
    let row_height = 44_u32.saturating_mul(factor);
    let control_count =
        u32::try_from(focus_order.len()).map_err(|_| LayoutContractError::TooManyControls)?;
    let content_height = row_height.saturating_mul(control_count);
    let maximum_offset = content_height.saturating_sub(scroll_viewport_height);
    let reachable = focus_order
        .iter()
        .enumerate()
        .map(|(index, id)| {
            let index = u32::try_from(index).map_err(|_| LayoutContractError::TooManyControls)?;
            let row_bottom = row_height.saturating_mul(index.saturating_add(1));
            Ok(ReachableControl {
                id: id.clone(),
                reveal_scroll_offset: row_bottom
                    .saturating_sub(scroll_viewport_height)
                    .min(maximum_offset),
            })
        })
        .collect::<Result<Vec<_>, LayoutContractError>>()?;
    Ok(LayoutEvidence {
        viewport,
        scale,
        scroll_viewport_height,
        content_height,
        reachable,
    })
}

/// Numeric layout contract violation.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum LayoutContractError {
    /// The first-version minimum is 800 by 600 logical units.
    #[error("logical viewport is smaller than 800 by 600")]
    ViewportBelowMinimum,
    /// Fixed chrome consumed the scroll viewport.
    #[error("scaled shell chrome leaves no usable scroll region")]
    NoUsableScrollRegion,
    /// Focus restoration would be ambiguous.
    #[error("focus traversal contains a duplicate stable key")]
    DuplicateStableKey,
    /// The focus traversal does not fit the versioned numeric layout contract.
    #[error("focus traversal contains more controls than the layout contract can index")]
    TooManyControls,
}
