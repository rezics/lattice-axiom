//! Numeric 800×600 layout evidence for UI scale 1.0 and 2.0.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::semantic::SemanticKey;
use crate::theme::{ThemeTokens, UiScale};

/// Integer logical viewport used by headless layout verification.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct LogicalViewport {
    /// Logical width.
    pub width: u32,
    /// Logical height.
    pub height: u32,
}

/// Scroll position proving a focusable control can be brought into view.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ReachableControl {
    /// Stable semantic control.
    pub key: SemanticKey,
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
    /// Returns whether every essential action is reachable.
    #[must_use]
    pub fn essentials_reachable(&self, essentials: &[SemanticKey]) -> bool {
        essentials
            .iter()
            .all(|key| self.reachable.iter().any(|row| &row.key == key))
    }
}

/// Calculates the fixed header/footer plus scroll-region contract.
///
/// # Errors
///
/// Returns [`LayoutContractError`] if the supported minimum viewport is not
/// met, chrome leaves no scroll region, or focus keys are duplicated.
pub fn verify_scroll_layout(
    viewport: LogicalViewport,
    scale: UiScale,
    tokens: &ThemeTokens,
    focus_order: &[SemanticKey],
) -> Result<LayoutEvidence, LayoutContractError> {
    if viewport.width < 800 || viewport.height < 600 {
        return Err(LayoutContractError::ViewportBelowMinimum);
    }
    let chrome = tokens.chrome.scaled(scale);
    let scroll_viewport_height = viewport
        .height
        .saturating_sub(chrome.header.saturating_add(chrome.footer));
    if scroll_viewport_height < 88 {
        return Err(LayoutContractError::NoUsableScrollRegion);
    }
    let unique = focus_order.iter().collect::<BTreeSet<_>>();
    if unique.len() != focus_order.len() {
        return Err(LayoutContractError::DuplicateStableKey);
    }
    let control_count =
        u32::try_from(focus_order.len()).map_err(|_| LayoutContractError::TooManyControls)?;
    let content_height = chrome.row.saturating_mul(control_count);
    let maximum_offset = content_height.saturating_sub(scroll_viewport_height);
    let reachable = focus_order
        .iter()
        .enumerate()
        .map(|(index, key)| {
            let index = u32::try_from(index).map_err(|_| LayoutContractError::TooManyControls)?;
            let row_bottom = chrome.row.saturating_mul(index.saturating_add(1));
            Ok(ReachableControl {
                key: key.clone(),
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::semantic::SemanticKey;

    fn key(value: &str) -> SemanticKey {
        SemanticKey::new(value).expect("static layout key")
    }

    #[test]
    fn scale_one_and_two_keep_primary_actions_reachable_at_800x600() {
        let tokens = ThemeTokens::plain_v2();
        let viewport = LogicalViewport {
            width: 800,
            height: 600,
        };
        let focus = [
            key("home/settings"),
            key("home/worlds"),
            key("settings/apply"),
        ];
        for scale in [UiScale::One, UiScale::Two] {
            let evidence = verify_scroll_layout(viewport, scale, &tokens, &focus).expect("layout");
            assert!(evidence.essentials_reachable(&focus));
        }
    }
}
