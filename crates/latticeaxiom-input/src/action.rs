//! Stable action identities, kinds, and first-version static enums.

use latticeaxiom_core::StableId;
use serde::{Deserialize, Serialize};

use crate::InputError;

/// Button versus two-axis action kind.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ActionKindV1 {
    /// Digital button or edge.
    Button,
    /// Finite two-axis value.
    Axis2,
}

/// Owning input context declared on an action spec.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum InputContextV1 {
    /// Stack-base live gameplay.
    Gameplay,
    /// Inventory or workbench overlay.
    HudOverlay,
    /// Exclusive shell, pause, or settings surface.
    Surface,
    /// Exclusive key-capture child of settings.
    BindingCapture,
}

/// First-version rebind policy.
///
/// Keyboard and mouse bindings may be replaced. Standard gamepad mappings stay
/// package-fixed until a later UI acceptance gate.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RebindablePolicyV1 {
    /// Keyboard and mouse bindings may be overridden by the user profile.
    KeyboardMouse,
    /// No user override is accepted for this action.
    None,
}

/// Authoritative fixed-tick player actions owned by `PlayerActionV1`.
///
/// Numeric discriminants match `latticeaxiom-player::PlayerActionV1` and must
/// not be reassigned. `SurfaceActivate` remains a player-crate discriminant and
/// is not catalog-mapped; surface activate is [`ClientSurfaceActionV1::Activate`].
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
#[repr(u8)]
pub enum AuthoritativePlayerActionV1 {
    /// Walk axes; `+x` right and `+y` forward.
    Move = 1,
    /// Look delta in radians.
    Look = 2,
    /// Jump button.
    Jump = 3,
    /// Break the targeted voxel.
    BreakBlock = 4,
    /// Place beside the targeted voxel.
    PlaceBlock = 5,
    /// Record an inspect receipt.
    Inspect = 6,
    /// Request pause.
    Pause = 7,
    /// Select a placement stack for the aimed block.
    PickBlock = 9,
}

impl AuthoritativePlayerActionV1 {
    /// Frozen first-version player actions compiled from the catalog.
    pub const ALL: [Self; 8] = [
        Self::Move,
        Self::Look,
        Self::Jump,
        Self::BreakBlock,
        Self::PlaceBlock,
        Self::Inspect,
        Self::Pause,
        Self::PickBlock,
    ];

    /// Returns the catalog stable ID for this player action.
    #[must_use]
    pub const fn stable_id(self) -> &'static str {
        match self {
            Self::Move => "latticeaxiom:action/gameplay/move@1",
            Self::Look => "latticeaxiom:action/gameplay/look@1",
            Self::Jump => "latticeaxiom:action/gameplay/jump@1",
            Self::BreakBlock => "latticeaxiom:action/gameplay/break-block@1",
            Self::PlaceBlock => "latticeaxiom:action/gameplay/place-block@1",
            Self::Inspect => "latticeaxiom:action/gameplay/inspect@1",
            Self::Pause => "latticeaxiom:action/gameplay/pause@1",
            Self::PickBlock => "latticeaxiom:action/gameplay/pick-block@1",
        }
    }

    /// Returns whether this action is suppressed while a HUD overlay is open.
    #[must_use]
    pub const fn suppressed_by_hud_overlay(self) -> bool {
        matches!(
            self,
            Self::Move
                | Self::Look
                | Self::Jump
                | Self::BreakBlock
                | Self::PlaceBlock
                | Self::Inspect
                | Self::PickBlock
        )
    }
}

/// Client-only shell, HUD, and UI actions.
///
/// These values do not enter the authoritative fixed tick, save, or portable
/// ABI. Package data must match this enum exactly.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ClientSurfaceActionV1 {
    /// Move focus up.
    NavUp,
    /// Move focus down.
    NavDown,
    /// Move focus left.
    NavLeft,
    /// Move focus right.
    NavRight,
    /// Advance focus.
    NavNext,
    /// Move focus to the previous target.
    NavPrevious,
    /// Activate the focused control.
    Activate,
    /// Dismiss or return from the current surface.
    Back,
    /// Open or close pause.
    Pause,
    /// Toggle the inventory overlay.
    ToggleInventory,
    /// Toggle the workbench overlay.
    ToggleWorkbench,
    /// Select hotbar slot 1.
    #[serde(rename = "hotbar-slot-1")]
    HotbarSlot1,
    /// Select hotbar slot 2.
    #[serde(rename = "hotbar-slot-2")]
    HotbarSlot2,
    /// Select hotbar slot 3.
    #[serde(rename = "hotbar-slot-3")]
    HotbarSlot3,
    /// Select hotbar slot 4.
    #[serde(rename = "hotbar-slot-4")]
    HotbarSlot4,
    /// Select hotbar slot 5.
    #[serde(rename = "hotbar-slot-5")]
    HotbarSlot5,
    /// Select hotbar slot 6.
    #[serde(rename = "hotbar-slot-6")]
    HotbarSlot6,
    /// Select hotbar slot 7.
    #[serde(rename = "hotbar-slot-7")]
    HotbarSlot7,
    /// Select hotbar slot 8.
    #[serde(rename = "hotbar-slot-8")]
    HotbarSlot8,
    /// Select hotbar slot 9.
    #[serde(rename = "hotbar-slot-9")]
    HotbarSlot9,
    /// Advance the hotbar selection.
    HotbarNext,
    /// Move the hotbar selection backward.
    HotbarPrevious,
}

impl ClientSurfaceActionV1 {
    /// Frozen first-version client surface actions.
    pub const ALL: [Self; 22] = [
        Self::NavUp,
        Self::NavDown,
        Self::NavLeft,
        Self::NavRight,
        Self::NavNext,
        Self::NavPrevious,
        Self::Activate,
        Self::Back,
        Self::Pause,
        Self::ToggleInventory,
        Self::ToggleWorkbench,
        Self::HotbarSlot1,
        Self::HotbarSlot2,
        Self::HotbarSlot3,
        Self::HotbarSlot4,
        Self::HotbarSlot5,
        Self::HotbarSlot6,
        Self::HotbarSlot7,
        Self::HotbarSlot8,
        Self::HotbarSlot9,
        Self::HotbarNext,
        Self::HotbarPrevious,
    ];

    /// Returns the catalog stable ID for this surface action.
    #[must_use]
    pub const fn stable_id(self) -> &'static str {
        match self {
            Self::NavUp => "latticeaxiom:action/ui/nav-up@1",
            Self::NavDown => "latticeaxiom:action/ui/nav-down@1",
            Self::NavLeft => "latticeaxiom:action/ui/nav-left@1",
            Self::NavRight => "latticeaxiom:action/ui/nav-right@1",
            Self::NavNext => "latticeaxiom:action/ui/nav-next@1",
            Self::NavPrevious => "latticeaxiom:action/ui/nav-previous@1",
            Self::Activate => "latticeaxiom:action/ui/activate@1",
            Self::Back => "latticeaxiom:action/ui/back@1",
            Self::Pause => "latticeaxiom:action/gameplay/pause@1",
            Self::ToggleInventory => "latticeaxiom:action/hud/toggle-inventory@1",
            Self::ToggleWorkbench => "latticeaxiom:action/hud/toggle-workbench@1",
            Self::HotbarSlot1 => "latticeaxiom:action/hud/hotbar-slot-1@1",
            Self::HotbarSlot2 => "latticeaxiom:action/hud/hotbar-slot-2@1",
            Self::HotbarSlot3 => "latticeaxiom:action/hud/hotbar-slot-3@1",
            Self::HotbarSlot4 => "latticeaxiom:action/hud/hotbar-slot-4@1",
            Self::HotbarSlot5 => "latticeaxiom:action/hud/hotbar-slot-5@1",
            Self::HotbarSlot6 => "latticeaxiom:action/hud/hotbar-slot-6@1",
            Self::HotbarSlot7 => "latticeaxiom:action/hud/hotbar-slot-7@1",
            Self::HotbarSlot8 => "latticeaxiom:action/hud/hotbar-slot-8@1",
            Self::HotbarSlot9 => "latticeaxiom:action/hud/hotbar-slot-9@1",
            Self::HotbarNext => "latticeaxiom:action/hud/hotbar-next@1",
            Self::HotbarPrevious => "latticeaxiom:action/hud/hotbar-previous@1",
        }
    }

    /// Returns the owning input context declared by ADR 0033.
    #[must_use]
    pub const fn declared_context(self) -> InputContextV1 {
        match self {
            Self::NavUp
            | Self::NavDown
            | Self::NavLeft
            | Self::NavRight
            | Self::NavNext
            | Self::NavPrevious
            | Self::Activate
            | Self::Back => InputContextV1::Surface,
            Self::Pause
            | Self::ToggleInventory
            | Self::ToggleWorkbench
            | Self::HotbarSlot1
            | Self::HotbarSlot2
            | Self::HotbarSlot3
            | Self::HotbarSlot4
            | Self::HotbarSlot5
            | Self::HotbarSlot6
            | Self::HotbarSlot7
            | Self::HotbarSlot8
            | Self::HotbarSlot9
            | Self::HotbarNext
            | Self::HotbarPrevious => InputContextV1::Gameplay,
        }
    }

    /// Returns the hotbar slot index in `1..=9`.
    #[must_use]
    pub const fn hotbar_slot(self) -> Option<u8> {
        match self {
            Self::HotbarSlot1 => Some(1),
            Self::HotbarSlot2 => Some(2),
            Self::HotbarSlot3 => Some(3),
            Self::HotbarSlot4 => Some(4),
            Self::HotbarSlot5 => Some(5),
            Self::HotbarSlot6 => Some(6),
            Self::HotbarSlot7 => Some(7),
            Self::HotbarSlot8 => Some(8),
            Self::HotbarSlot9 => Some(9),
            _ => None,
        }
    }
}

/// Native Esc fallback that cannot be removed by a missing profile.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum EscSafetyActionV1 {
    /// Close or return from a surface or overlay.
    Back,
    /// Open pause from live gameplay.
    Pause,
    /// Cancel binding capture without applying.
    Cancel,
}

/// Authored action row before catalog compilation.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ActionSpecV1 {
    /// Versioned stable action identifier.
    pub id: StableId,
    /// Button or two-axis kind.
    pub kind: ActionKindV1,
    /// Owning input context.
    pub context: InputContextV1,
    /// Whether keyboard and mouse bindings may be overridden.
    pub rebindable: RebindablePolicyV1,
    /// Optional mapping onto the frozen client surface enum.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_surface_action: Option<ClientSurfaceActionV1>,
    /// Optional mapping onto the frozen player-action discriminants.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub authoritative_player_action: Option<AuthoritativePlayerActionV1>,
    /// Platform default bindings in stable authored order.
    pub defaults: Vec<crate::InputBindingV1>,
}

impl ActionSpecV1 {
    pub(crate) fn validate(&self) -> Result<(), InputError> {
        if self.id.kind() != "action" {
            return Err(InputError::InvalidAction {
                action: self.id.clone(),
                reason: "stable ID kind must be `action`",
            });
        }
        if self.id.major().map(std::num::NonZeroU64::get) != Some(1) {
            return Err(InputError::InvalidAction {
                action: self.id.clone(),
                reason: "first-version catalog actions require major `@1`",
            });
        }
        if self.client_surface_action.is_none() && self.authoritative_player_action.is_none() {
            return Err(InputError::InvalidAction {
                action: self.id.clone(),
                reason: "v1 actions must map to a static player or surface enum",
            });
        }
        if let Some(surface) = self.client_surface_action
            && surface.stable_id() != self.id.as_str()
        {
            return Err(InputError::CatalogEnumMismatch {
                detail: format!(
                    "action {} maps to surface variant {}",
                    self.id,
                    surface.stable_id()
                ),
            });
        }
        if let Some(player) = self.authoritative_player_action
            && player.stable_id() != self.id.as_str()
        {
            return Err(InputError::CatalogEnumMismatch {
                detail: format!(
                    "action {} maps to player variant {}",
                    self.id,
                    player.stable_id()
                ),
            });
        }
        if let Some(surface) = self.client_surface_action
            && surface.declared_context() != self.context
        {
            return Err(InputError::InvalidAction {
                action: self.id.clone(),
                reason: "declared context does not match the frozen surface enum",
            });
        }
        let expects_axis = matches!(
            self.authoritative_player_action,
            Some(AuthoritativePlayerActionV1::Move | AuthoritativePlayerActionV1::Look)
        );
        if expects_axis != matches!(self.kind, ActionKindV1::Axis2) {
            return Err(InputError::InvalidAction {
                action: self.id.clone(),
                reason: "axis player actions must use kind axis2; all other v1 actions are buttons",
            });
        }
        for binding in &self.defaults {
            binding.validate_for_kind(self.kind)?;
        }
        Ok(())
    }
}
