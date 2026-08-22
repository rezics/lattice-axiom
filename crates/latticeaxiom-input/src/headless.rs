//! Headless logical-input session used by command fixtures.

use std::collections::{BTreeMap, BTreeSet};

use crate::{
    ActiveInputContextStack, AuthoritativePlayerActionV1, ClientSurfaceActionV1,
    CompiledActionIndex, CompiledInputCatalogV1, ContextTransitionReceiptV1, CursorFocusPolicyV1,
    EscSafetyActionV1, FocusOwnerV1, HudOverlayKindV1, InputContextV1, InputError, action_allowed,
};

/// Finite two-axis value used by headless injection.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Axis2ValueV1 {
    /// Horizontal component.
    pub x: f32,
    /// Vertical component.
    pub y: f32,
}

impl Axis2ValueV1 {
    /// Replaces non-finite components with zero.
    #[must_use]
    pub fn finite_or_zero(x: f32, y: f32) -> Self {
        Self {
            x: if x.is_finite() { x } else { 0.0 },
            y: if y.is_finite() { y } else { 0.0 },
        }
    }
}

/// Sampled logical input after context arbitration.
#[derive(Clone, Debug, PartialEq)]
pub struct LogicalInputSampleV1 {
    /// Pressed-state generation.
    pub generation: u64,
    /// Context stack, base first.
    pub stack: Vec<InputContextV1>,
    /// Live gameplay suppression.
    pub gameplay_suppressed: bool,
    /// Cursor policy.
    pub cursor: CursorFocusPolicyV1,
    /// Focus owner.
    pub focus: FocusOwnerV1,
    /// Esc safety action.
    pub esc_safety: EscSafetyActionV1,
    /// Held client surface actions.
    pub held_surface: BTreeSet<ClientSurfaceActionV1>,
    /// Rising-edge client surface actions for this generation.
    pub started_surface: BTreeSet<ClientSurfaceActionV1>,
    /// Held authoritative player buttons.
    pub held_player: BTreeSet<AuthoritativePlayerActionV1>,
    /// Rising-edge authoritative player buttons for this generation.
    pub started_player: BTreeSet<AuthoritativePlayerActionV1>,
    /// Live axes. Empty while gameplay is suppressed.
    pub axes: BTreeMap<AuthoritativePlayerActionV1, Axis2ValueV1>,
}

/// Headless pump that injects logical actions against compiled maps.
#[derive(Clone, Debug)]
pub struct HeadlessInputSession {
    catalog: CompiledInputCatalogV1,
    stack: ActiveInputContextStack,
    physical_held: BTreeSet<CompiledActionIndex>,
    started: BTreeMap<CompiledActionIndex, u64>,
    axes: BTreeMap<CompiledActionIndex, Axis2ValueV1>,
    held_snapshots: Vec<BTreeSet<CompiledActionIndex>>,
}

impl HeadlessInputSession {
    /// Creates a session on the gameplay base context.
    #[must_use]
    pub fn new(catalog: CompiledInputCatalogV1) -> Self {
        Self {
            catalog,
            stack: ActiveInputContextStack::new(),
            physical_held: BTreeSet::new(),
            started: BTreeMap::new(),
            axes: BTreeMap::new(),
            held_snapshots: Vec::new(),
        }
    }

    /// Compiled catalog used by this session.
    #[must_use]
    pub const fn catalog(&self) -> &CompiledInputCatalogV1 {
        &self.catalog
    }

    /// Active context stack.
    #[must_use]
    pub const fn stack(&self) -> &ActiveInputContextStack {
        &self.stack
    }

    /// Pushes a context and clears just-pressed edges.
    ///
    /// # Errors
    ///
    /// Returns [`InputError::IllegalContextTransition`] for an illegal push.
    pub fn push_context(
        &mut self,
        context: InputContextV1,
        overlay: Option<HudOverlayKindV1>,
    ) -> Result<ContextTransitionReceiptV1, InputError> {
        self.held_snapshots.push(self.physical_held.clone());
        let receipt = self.stack.push(context, overlay)?;
        self.started.clear();
        Ok(receipt)
    }

    /// Pops the top context and releases actions it consumed.
    ///
    /// # Errors
    ///
    /// Returns [`InputError::IllegalContextTransition`] when popping gameplay.
    pub fn pop_context(&mut self) -> Result<ContextTransitionReceiptV1, InputError> {
        let receipt = self.stack.pop()?;
        let snapshot = self.held_snapshots.pop().unwrap_or_default();
        let mut release = receipt.released.clone();
        for index in self.physical_held.difference(&snapshot) {
            release.insert(*index);
        }
        for index in &release {
            self.physical_held.remove(index);
            self.axes.remove(index);
        }
        self.started.clear();
        Ok(receipt)
    }

    /// Injects a logical press. Suppressed actions stay physical-only.
    ///
    /// # Errors
    ///
    /// Returns [`InputError`] when the action is not in the compiled catalog.
    pub fn press_surface(&mut self, action: ClientSurfaceActionV1) -> Result<bool, InputError> {
        let index = self.surface_index(action)?;
        Ok(self.press_index(index))
    }

    /// Releases a logical surface action.
    ///
    /// # Errors
    ///
    /// Returns [`InputError`] when the action is not in the compiled catalog.
    pub fn release_surface(&mut self, action: ClientSurfaceActionV1) -> Result<(), InputError> {
        let index = self.surface_index(action)?;
        self.release_index(index);
        Ok(())
    }

    /// Injects a logical player-button press.
    ///
    /// # Errors
    ///
    /// Returns [`InputError`] when the action is not in the compiled catalog.
    pub fn press_player(
        &mut self,
        action: AuthoritativePlayerActionV1,
    ) -> Result<bool, InputError> {
        let index = self.player_index(action)?;
        Ok(self.press_index(index))
    }

    /// Releases a logical player button.
    ///
    /// # Errors
    ///
    /// Returns [`InputError`] when the action is not in the compiled catalog.
    pub fn release_player(
        &mut self,
        action: AuthoritativePlayerActionV1,
    ) -> Result<(), InputError> {
        let index = self.player_index(action)?;
        self.release_index(index);
        Ok(())
    }

    /// Sets a live axis. Suppressed gameplay samples as zero.
    ///
    /// # Errors
    ///
    /// Returns [`InputError`] when the action is not an axis in the catalog.
    pub fn set_axis(
        &mut self,
        action: AuthoritativePlayerActionV1,
        x: f32,
        y: f32,
    ) -> Result<(), InputError> {
        let index = self.player_index(action)?;
        self.axes.insert(index, Axis2ValueV1::finite_or_zero(x, y));
        Ok(())
    }

    /// Native Esc fallback for the current top context.
    #[must_use]
    pub fn esc_safety(&self) -> EscSafetyActionV1 {
        self.stack.esc_safety()
    }

    /// Samples allowed logical state for this generation.
    #[must_use]
    pub fn sample(&self) -> LogicalInputSampleV1 {
        let mut held_surface = BTreeSet::new();
        let mut started_surface = BTreeSet::new();
        let mut held_player = BTreeSet::new();
        let mut started_player = BTreeSet::new();
        let mut axes = BTreeMap::new();
        let gameplay_suppressed = self.stack.gameplay_suppressed();
        for index in &self.physical_held {
            if !action_allowed(&self.catalog, &self.stack, *index) {
                continue;
            }
            let Some(action) = self.catalog.action_by_index(*index) else {
                continue;
            };
            let started = self.started.get(index).copied() == Some(self.stack.generation());
            if let Some(surface) = action.client_surface_action {
                held_surface.insert(surface);
                if started {
                    started_surface.insert(surface);
                }
            }
            if let Some(player) = action.authoritative_player_action {
                if gameplay_suppressed && player.suppressed_by_hud_overlay() {
                    continue;
                }
                if gameplay_suppressed
                    && !matches!(
                        self.stack.top(),
                        InputContextV1::HudOverlay | InputContextV1::Gameplay
                    )
                {
                    continue;
                }
                held_player.insert(player);
                if started {
                    started_player.insert(player);
                }
            }
        }
        if !gameplay_suppressed {
            for (index, value) in &self.axes {
                if let Some(action) = self.catalog.action_by_index(*index)
                    && let Some(player) = action.authoritative_player_action
                {
                    axes.insert(player, *value);
                }
            }
        }
        LogicalInputSampleV1 {
            generation: self.stack.generation(),
            stack: self.stack.contexts(),
            gameplay_suppressed,
            cursor: self.stack.cursor_policy(),
            focus: self.stack.focus_owner(),
            esc_safety: self.stack.esc_safety(),
            held_surface,
            started_surface,
            held_player,
            started_player,
            axes,
        }
    }

    fn press_index(&mut self, index: CompiledActionIndex) -> bool {
        let allowed = action_allowed(&self.catalog, &self.stack, index);
        let inserted = self.physical_held.insert(index);
        if inserted {
            self.started.insert(index, self.stack.generation());
        }
        if allowed {
            self.stack.record_consumed(index);
        }
        allowed
    }

    fn release_index(&mut self, index: CompiledActionIndex) {
        self.physical_held.remove(&index);
        self.started.remove(&index);
    }

    fn surface_index(
        &self,
        action: ClientSurfaceActionV1,
    ) -> Result<CompiledActionIndex, InputError> {
        self.catalog
            .action_by_surface(action)
            .map(|compiled| compiled.index)
            .ok_or_else(|| InputError::CatalogEnumMismatch {
                detail: format!("compiled catalog is missing {}", action.stable_id()),
            })
    }

    fn player_index(
        &self,
        action: AuthoritativePlayerActionV1,
    ) -> Result<CompiledActionIndex, InputError> {
        self.catalog
            .action_by_player(action)
            .map(|compiled| compiled.index)
            .ok_or_else(|| InputError::CatalogEnumMismatch {
                detail: format!("compiled catalog is missing {}", action.stable_id()),
            })
    }
}
