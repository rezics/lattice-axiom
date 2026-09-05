//! Declared localization, input, and accessibility support evidence.

use serde::{Deserialize, Serialize};

/// Verification state for one client-surface capability.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum CapabilityEvidenceStatus {
    /// Covered by the headless semantic contract and automated evidence.
    HeadlessVerified,
    /// Declared for the future Bevy adapter but not yet verified there.
    AdapterPending,
    /// Deliberately unavailable in this implementation stage.
    Unavailable,
}

/// Evidence status for screenshot and GPU pixel comparison.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum VisualEvidenceStatus {
    /// No screenshot, pixel golden, or GPU visual comparison was collected.
    NotCollected,
}

/// Honest capability report exposed to diagnostics and CI.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct StartSurfaceCapabilityReport {
    /// CJK fallback font bundle requirement.
    pub cjk_fallback_font: CapabilityEvidenceStatus,
    /// Composition and committed-text event handling.
    pub ime_composition: CapabilityEvidenceStatus,
    /// Role/name/value/description/state/action semantic tree.
    pub accessibility_tree: CapabilityEvidenceStatus,
    /// Keyboard semantic-command injection.
    pub keyboard_navigation: CapabilityEvidenceStatus,
    /// Standard-controller semantic-command injection.
    pub controller_navigation: CapabilityEvidenceStatus,
    /// Current visual evidence state.
    pub visual_evidence: VisualEvidenceStatus,
}

impl Default for StartSurfaceCapabilityReport {
    fn default() -> Self {
        Self {
            cjk_fallback_font: CapabilityEvidenceStatus::AdapterPending,
            ime_composition: CapabilityEvidenceStatus::HeadlessVerified,
            accessibility_tree: CapabilityEvidenceStatus::HeadlessVerified,
            keyboard_navigation: CapabilityEvidenceStatus::HeadlessVerified,
            controller_navigation: CapabilityEvidenceStatus::HeadlessVerified,
            visual_evidence: VisualEvidenceStatus::NotCollected,
        }
    }
}

/// Presentation-neutral editable text state driven by IME events.
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
