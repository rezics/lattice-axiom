//! Active input context stack, policy derivation, and pressed-state cleanup.

use std::collections::BTreeSet;

use crate::{
    ClientSurfaceActionV1, CompiledActionIndex, CompiledInputCatalogV1, EscSafetyActionV1,
    InputContextV1, InputError,
};

/// Capture policy for one context layer.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CapturePolicyV1 {
    /// Lower maps are not live.
    Exclusive,
    /// Lower maps may contribute an explicit allowlist.
    Overlay,
}

/// Whether live authoritative gameplay frames may be produced.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GameplayPolicyV1 {
    /// Live gameplay frames are allowed.
    Allow,
    /// Live gameplay frames are suppressed.
    Suppress,
}

/// Cursor and focus ownership derived from the top context.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CursorFocusPolicyV1 {
    /// Cursor locked to gameplay look.
    LockedGameplay,
    /// Visible cursor owned by a surface.
    VisibleSurface,
}

/// Focus owner derived from the top context.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FocusOwnerV1 {
    /// World/gameplay focus.
    GameplayWorld,
    /// Surface widget focus.
    Surface,
    /// Binding-capture focus.
    BindingCapture,
}

/// Three explicit policies for one context. Capture and gameplay suppression
/// are independent and must not be inferred from a single boolean.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ContextPolicyV1 {
    /// Capture mode.
    pub capture: CapturePolicyV1,
    /// Authoritative gameplay permission.
    pub authoritative_gameplay: GameplayPolicyV1,
    /// Cursor and focus policy.
    pub cursor_focus: CursorFocusPolicyV1,
}

impl InputContextV1 {
    /// Returns the frozen first-version policy for this context.
    #[must_use]
    pub const fn policy(self) -> ContextPolicyV1 {
        match self {
            Self::Gameplay => ContextPolicyV1 {
                capture: CapturePolicyV1::Overlay,
                authoritative_gameplay: GameplayPolicyV1::Allow,
                cursor_focus: CursorFocusPolicyV1::LockedGameplay,
            },
            Self::HudOverlay => ContextPolicyV1 {
                capture: CapturePolicyV1::Overlay,
                authoritative_gameplay: GameplayPolicyV1::Suppress,
                cursor_focus: CursorFocusPolicyV1::VisibleSurface,
            },
            Self::Surface | Self::BindingCapture => ContextPolicyV1 {
                capture: CapturePolicyV1::Exclusive,
                authoritative_gameplay: GameplayPolicyV1::Suppress,
                cursor_focus: CursorFocusPolicyV1::VisibleSurface,
            },
        }
    }

    /// Native Esc fallback for this context. Never mutates the world.
    #[must_use]
    pub const fn esc_safety(self) -> EscSafetyActionV1 {
        match self {
            Self::Gameplay => EscSafetyActionV1::Pause,
            Self::HudOverlay | Self::Surface => EscSafetyActionV1::Back,
            Self::BindingCapture => EscSafetyActionV1::Cancel,
        }
    }
}

/// Inventory versus workbench overlay identity.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HudOverlayKindV1 {
    /// Inventory overlay.
    Inventory,
    /// Workbench overlay.
    Workbench,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ContextLayerV1 {
    context: InputContextV1,
    overlay: Option<HudOverlayKindV1>,
    consumed: BTreeSet<CompiledActionIndex>,
}

/// Receipt emitted after an atomic push or pop.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContextTransitionReceiptV1 {
    /// Pressed-state generation after the transition.
    pub generation: u64,
    /// Resulting stack, base first.
    pub stack: Vec<InputContextV1>,
    /// Whether live gameplay is suppressed.
    pub gameplay_suppressed: bool,
    /// Cursor policy.
    pub cursor: CursorFocusPolicyV1,
    /// Focus owner.
    pub focus: FocusOwnerV1,
    /// Esc safety action for the new top context.
    pub esc_safety: EscSafetyActionV1,
    /// Action indices released because the popped surface consumed them.
    pub released: BTreeSet<CompiledActionIndex>,
}

/// Unique client input arbitration resource.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ActiveInputContextStack {
    layers: Vec<ContextLayerV1>,
    generation: u64,
}

impl Default for ActiveInputContextStack {
    fn default() -> Self {
        Self::new()
    }
}

impl ActiveInputContextStack {
    /// Creates a stack whose only layer is gameplay.
    #[must_use]
    pub fn new() -> Self {
        Self {
            layers: vec![ContextLayerV1 {
                context: InputContextV1::Gameplay,
                overlay: None,
                consumed: BTreeSet::new(),
            }],
            generation: 1,
        }
    }

    /// Current layers, base first.
    #[must_use]
    pub fn contexts(&self) -> Vec<InputContextV1> {
        self.layers.iter().map(|layer| layer.context).collect()
    }

    /// Top context.
    #[must_use]
    pub fn top(&self) -> InputContextV1 {
        match self.layers.last() {
            Some(layer) => layer.context,
            None => InputContextV1::Gameplay,
        }
    }

    /// Pressed-state generation. Transitions always increment it.
    #[must_use]
    pub const fn generation(&self) -> u64 {
        self.generation
    }

    /// Returns whether live authoritative gameplay is currently suppressed.
    #[must_use]
    pub fn gameplay_suppressed(&self) -> bool {
        self.top().policy().authoritative_gameplay == GameplayPolicyV1::Suppress
    }

    /// Cursor policy of the top context.
    #[must_use]
    pub fn cursor_policy(&self) -> CursorFocusPolicyV1 {
        self.top().policy().cursor_focus
    }

    /// Focus owner of the top context.
    #[must_use]
    pub fn focus_owner(&self) -> FocusOwnerV1 {
        match self.top() {
            InputContextV1::Gameplay => FocusOwnerV1::GameplayWorld,
            InputContextV1::BindingCapture => FocusOwnerV1::BindingCapture,
            InputContextV1::HudOverlay | InputContextV1::Surface => FocusOwnerV1::Surface,
        }
    }

    /// Native Esc safety action for the current top context.
    #[must_use]
    pub fn esc_safety(&self) -> EscSafetyActionV1 {
        self.top().esc_safety()
    }

    /// Overlay kind when a HUD overlay is on the stack.
    #[must_use]
    pub fn hud_overlay(&self) -> Option<HudOverlayKindV1> {
        self.layers.iter().find_map(|layer| layer.overlay)
    }

    /// Pushes `context` and bumps pressed-state generation.
    ///
    /// # Errors
    ///
    /// Returns [`InputError::IllegalContextTransition`] when the push order is
    /// illegal for first-version stacks.
    pub fn push(
        &mut self,
        context: InputContextV1,
        overlay: Option<HudOverlayKindV1>,
    ) -> Result<ContextTransitionReceiptV1, InputError> {
        if !self.can_push(context, overlay) {
            return Err(InputError::IllegalContextTransition {
                reason: format!(
                    "cannot push {context:?} overlay={overlay:?} onto {:?}",
                    self.contexts()
                ),
            });
        }
        self.generation = self.generation.saturating_add(1);
        self.layers.push(ContextLayerV1 {
            context,
            overlay,
            consumed: BTreeSet::new(),
        });
        Ok(self.receipt(BTreeSet::new()))
    }

    /// Pops the top context, releasing actions it consumed.
    ///
    /// # Errors
    ///
    /// Returns [`InputError::IllegalContextTransition`] when gameplay is the
    /// only remaining layer.
    pub fn pop(&mut self) -> Result<ContextTransitionReceiptV1, InputError> {
        if self.layers.len() <= 1 {
            return Err(InputError::IllegalContextTransition {
                reason: "cannot pop the gameplay base context".to_owned(),
            });
        }
        let Some(popped) = self.layers.pop() else {
            return Err(InputError::IllegalContextTransition {
                reason: "cannot pop the gameplay base context".to_owned(),
            });
        };
        self.generation = self.generation.saturating_add(1);
        Ok(self.receipt(popped.consumed))
    }

    pub(crate) fn record_consumed(&mut self, index: CompiledActionIndex) {
        if let Some(top) = self.layers.last_mut() {
            top.consumed.insert(index);
        }
    }

    fn can_push(&self, context: InputContextV1, overlay: Option<HudOverlayKindV1>) -> bool {
        matches!(
            (self.top(), context, overlay),
            (
                InputContextV1::Gameplay,
                InputContextV1::HudOverlay,
                Some(_)
            ) | (
                InputContextV1::Gameplay | InputContextV1::HudOverlay,
                InputContextV1::Surface,
                None
            ) | (
                InputContextV1::Surface,
                InputContextV1::BindingCapture,
                None
            )
        )
    }

    fn receipt(&self, released: BTreeSet<CompiledActionIndex>) -> ContextTransitionReceiptV1 {
        ContextTransitionReceiptV1 {
            generation: self.generation,
            stack: self.contexts(),
            gameplay_suppressed: self.gameplay_suppressed(),
            cursor: self.cursor_policy(),
            focus: self.focus_owner(),
            esc_safety: self.esc_safety(),
            released,
        }
    }
}

/// Returns whether `action` is allowed by the current stack and catalog maps.
#[must_use]
pub fn action_allowed(
    catalog: &CompiledInputCatalogV1,
    stack: &ActiveInputContextStack,
    index: CompiledActionIndex,
) -> bool {
    let Some(action) = catalog.action_by_index(index) else {
        return false;
    };
    match stack.top() {
        InputContextV1::Gameplay => action.context == InputContextV1::Gameplay,
        InputContextV1::HudOverlay => match stack.hud_overlay() {
            Some(HudOverlayKindV1::Inventory) => {
                catalog.inventory_overlay_map().contains_action(&action.id)
            }
            Some(HudOverlayKindV1::Workbench) => {
                catalog.workbench_overlay_map().contains_action(&action.id)
            }
            None => false,
        },
        InputContextV1::Surface => action.context == InputContextV1::Surface,
        InputContextV1::BindingCapture => false,
    }
}

/// Returns whether a HUD overlay still permits this surface action.
#[must_use]
pub fn surface_action_on_hud_allowlist(action: ClientSurfaceActionV1) -> bool {
    matches!(
        action,
        ClientSurfaceActionV1::Back
            | ClientSurfaceActionV1::ToggleInventory
            | ClientSurfaceActionV1::ToggleWorkbench
            | ClientSurfaceActionV1::HotbarSlot1
            | ClientSurfaceActionV1::HotbarSlot2
            | ClientSurfaceActionV1::HotbarSlot3
            | ClientSurfaceActionV1::HotbarSlot4
            | ClientSurfaceActionV1::HotbarSlot5
            | ClientSurfaceActionV1::HotbarSlot6
            | ClientSurfaceActionV1::HotbarSlot7
            | ClientSurfaceActionV1::HotbarSlot8
            | ClientSurfaceActionV1::HotbarSlot9
            | ClientSurfaceActionV1::HotbarNext
            | ClientSurfaceActionV1::HotbarPrevious
    )
}
