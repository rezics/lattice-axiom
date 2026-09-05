//! Shared surface epoch, cursor, and input-context projections.

use serde::{Deserialize, Serialize};

/// Monotonic epoch incremented on every accepted route transition.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct SurfaceEpoch(u64);

impl SurfaceEpoch {
    /// Epoch assigned to a freshly constructed router.
    pub const FIRST: Self = Self(1);

    /// Creates an epoch from a raw counter.
    #[must_use]
    pub const fn from_raw(value: u64) -> Self {
        Self(value)
    }

    /// Returns the raw counter.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }

    /// Returns the next epoch.
    #[must_use]
    pub const fn next(self) -> Self {
        Self(self.0.saturating_add(1))
    }
}

impl core::fmt::Display for SurfaceEpoch {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        self.0.fmt(formatter)
    }
}

/// Cursor policy mechanically derived from the active route.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum CursorPolicy {
    /// Gameplay camera lock.
    LockedGameplay,
    /// Visible surface cursor owned by the unique focus owner.
    VisibleSurface,
}

/// Capture policy for one input-context layer.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum CapturePolicy {
    /// Base gameplay layer.
    Base,
    /// Overlay that may forward an allowlist.
    Overlay,
    /// Exclusive capture.
    Exclusive,
}

/// Authoritative gameplay admission for one context layer.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum GameplayAdmission {
    /// Live authoritative frames are admitted.
    Allow,
    /// Live authoritative frames are suppressed.
    Suppress,
}

/// Named input context owned by the surface router projection.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum InputContextKind {
    /// Gameplay base.
    Gameplay,
    /// Inventory/workbench overlay.
    HudOverlay,
    /// Pause, settings, shell, and confirm.
    Surface,
    /// Exclusive binding capture.
    BindingCapture,
}

impl InputContextKind {
    /// Returns the frozen capture, gameplay, and cursor policy for this layer.
    #[must_use]
    pub const fn policy(self) -> ContextPolicy {
        match self {
            Self::Gameplay => ContextPolicy {
                capture: CapturePolicy::Base,
                gameplay: GameplayAdmission::Allow,
                cursor: CursorPolicy::LockedGameplay,
            },
            Self::HudOverlay => ContextPolicy {
                capture: CapturePolicy::Overlay,
                gameplay: GameplayAdmission::Suppress,
                cursor: CursorPolicy::VisibleSurface,
            },
            Self::Surface | Self::BindingCapture => ContextPolicy {
                capture: CapturePolicy::Exclusive,
                gameplay: GameplayAdmission::Suppress,
                cursor: CursorPolicy::VisibleSurface,
            },
        }
    }
}

/// Frozen context policy triple. Exclusive and gameplay suppression are distinct.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ContextPolicy {
    /// Capture mode.
    pub capture: CapturePolicy,
    /// Authoritative gameplay admission.
    pub gameplay: GameplayAdmission,
    /// Cursor ownership.
    pub cursor: CursorPolicy,
}

/// Derived input-context stack. Gameplay is always the base of a game process.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ActiveInputContextStack {
    layers: Vec<InputContextKind>,
}

impl ActiveInputContextStack {
    /// Creates a stack from bottom to top.
    #[must_use]
    pub fn from_layers(layers: Vec<InputContextKind>) -> Self {
        Self { layers }
    }

    /// Shell-only exclusive surface stack. It does not create a game route.
    #[must_use]
    pub fn shell_surface() -> Self {
        Self {
            layers: vec![InputContextKind::Surface],
        }
    }

    /// Returns layers from bottom to top.
    #[must_use]
    pub fn layers(&self) -> &[InputContextKind] {
        &self.layers
    }

    /// Returns whether live authoritative gameplay is suppressed.
    #[must_use]
    pub fn suppresses_gameplay(&self) -> bool {
        self.layers
            .iter()
            .any(|layer| layer.policy().gameplay == GameplayAdmission::Suppress)
    }

    /// Returns the winning cursor policy from the top layer.
    #[must_use]
    pub fn cursor_policy(&self) -> CursorPolicy {
        self.layers
            .last()
            .map_or(CursorPolicy::VisibleSurface, |layer| layer.policy().cursor)
    }

    /// Returns the exclusive top context, when present.
    #[must_use]
    pub fn top(&self) -> Option<InputContextKind> {
        self.layers.last().copied()
    }
}

/// Maximum route stack depth: base, overlay, modal.
pub const MAX_ROUTE_DEPTH: u8 = 3;

/// Version of the typed route vocabulary.
pub const ROUTE_VOCABULARY_MAJOR: u32 = 1;
