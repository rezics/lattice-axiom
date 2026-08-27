//! Deterministic catalog compilation, overlay, and Leafwing-facing maps.

use std::collections::BTreeMap;

use latticeaxiom_core::{CanonicalHash, PackageName, StableId, canonical_json_hash};

use crate::{
    ActionKindV1, ActionSpecV1, AuthoritativePlayerActionV1, BindingProfileV1,
    ClientSurfaceActionV1, GamepadStickV1, InputActionsProviderV1, InputBindingV1, InputContextV1,
    InputError, KeyModifierV1, MouseButtonV1, MouseWheelAxisV1, MouseWheelDirectionV1,
    RebindablePolicyV1,
    catalog::{assert_static_enum_goldens, unique_sorted_actions},
    conflict::detect_same_context_conflicts,
    select_exactly_one_input_actions_provider,
};

/// Dense compiled action index used on hot paths instead of stable ID strings.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct CompiledActionIndex(u16);

impl CompiledActionIndex {
    /// Returns the dense index integer.
    #[must_use]
    pub const fn get(self) -> u16 {
        self.0
    }
}

/// One compiled action after default/profile overlay.
#[derive(Clone, Debug, PartialEq)]
pub struct CompiledActionV1 {
    /// Dense index assigned in stable ID order.
    pub index: CompiledActionIndex,
    /// Stable action identifier.
    pub id: StableId,
    /// Button or axis kind.
    pub kind: ActionKindV1,
    /// Owning context.
    pub context: InputContextV1,
    /// Rebind policy.
    pub rebindable: RebindablePolicyV1,
    /// Optional client surface mapping.
    pub client_surface_action: Option<ClientSurfaceActionV1>,
    /// Optional authoritative player mapping.
    pub authoritative_player_action: Option<AuthoritativePlayerActionV1>,
    /// Effective bindings after overlay.
    pub effective_bindings: Vec<InputBindingV1>,
}

/// Leafwing-facing recipe that a client adapter can turn into an `InputMap`.
#[derive(Clone, Debug, PartialEq)]
pub enum LeafwingRecipeV1 {
    /// Keyboard usage with modifiers.
    Keyboard {
        /// Physical usage.
        usage: String,
        /// Modifiers.
        modifiers: Vec<KeyModifierV1>,
    },
    /// WASD-style virtual d-pad.
    VirtualDPad {
        /// Forward.
        up: String,
        /// Back.
        down: String,
        /// Left.
        left: String,
        /// Right.
        right: String,
        /// Modifiers.
        modifiers: Vec<KeyModifierV1>,
    },
    /// Mouse button.
    MouseButton {
        /// Button.
        button: MouseButtonV1,
    },
    /// Mouse motion look.
    MouseMove {
        /// Sensitivity.
        sensitivity: f32,
    },
    /// Mouse wheel.
    MouseWheel {
        /// Axis.
        axis: MouseWheelAxisV1,
    },
    /// Button-like mouse-wheel direction.
    MouseWheelDirection {
        /// Direction.
        direction: MouseWheelDirectionV1,
    },
    /// Standard gamepad button.
    GamepadButton {
        /// Button name.
        button: String,
    },
    /// Standard analog stick.
    GamepadStick {
        /// Stick.
        stick: GamepadStickV1,
        /// Deadzone.
        deadzone: f32,
        /// Sensitivity.
        sensitivity: f32,
    },
}

/// One compiled Leafwing map entry.
#[derive(Clone, Debug, PartialEq)]
pub struct CompiledLeafwingEntryV1 {
    /// Dense action index.
    pub index: CompiledActionIndex,
    /// Stable action ID.
    pub action_id: StableId,
    /// Control kind.
    pub kind: ActionKindV1,
    /// Leafwing-facing recipes in stable order.
    pub recipes: Vec<LeafwingRecipeV1>,
}

/// Compiled Leafwing-facing map for one context filter.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CompiledLeafwingMapV1 {
    /// Entries in dense index order.
    pub entries: Vec<CompiledLeafwingEntryV1>,
}

impl CompiledLeafwingMapV1 {
    /// Returns whether the map contains `action`.
    #[must_use]
    pub fn contains_action(&self, action: &StableId) -> bool {
        self.entries.iter().any(|entry| entry.action_id == *action)
    }
}

/// Mechanically generated Controls row for a rebindable action.
#[derive(Clone, Debug, PartialEq)]
pub struct ControlsSettingRowV1 {
    /// Action identifier.
    pub action_id: StableId,
    /// Owning context.
    pub context: InputContextV1,
    /// Rebind policy.
    pub rebindable: RebindablePolicyV1,
    /// Effective keyboard/mouse bindings shown to the user.
    pub keyboard_mouse_bindings: Vec<InputBindingV1>,
}

/// Fully compiled input catalog ready for headless use or a client adapter.
#[derive(Clone, Debug, PartialEq)]
pub struct CompiledInputCatalogV1 {
    provider: PackageName,
    specs: Vec<ActionSpecV1>,
    actions: Vec<CompiledActionV1>,
    by_id: BTreeMap<StableId, CompiledActionIndex>,
    orphan_overrides: BTreeMap<StableId, Vec<InputBindingV1>>,
    gameplay_map: CompiledLeafwingMapV1,
    surface_map: CompiledLeafwingMapV1,
    inventory_overlay_map: CompiledLeafwingMapV1,
    workbench_overlay_map: CompiledLeafwingMapV1,
}

impl CompiledInputCatalogV1 {
    /// Package that provided the compiled catalog.
    #[must_use]
    pub const fn provider(&self) -> &PackageName {
        &self.provider
    }

    /// Compiled actions in dense index order.
    #[must_use]
    pub fn actions(&self) -> &[CompiledActionV1] {
        &self.actions
    }

    /// Unknown profile entries retained after overlay.
    #[must_use]
    pub const fn orphan_overrides(&self) -> &BTreeMap<StableId, Vec<InputBindingV1>> {
        &self.orphan_overrides
    }

    /// Leafwing-facing gameplay map.
    #[must_use]
    pub const fn gameplay_map(&self) -> &CompiledLeafwingMapV1 {
        &self.gameplay_map
    }

    /// Leafwing-facing exclusive surface map.
    #[must_use]
    pub const fn surface_map(&self) -> &CompiledLeafwingMapV1 {
        &self.surface_map
    }

    /// HUD overlay allowlist while inventory is open.
    #[must_use]
    pub const fn inventory_overlay_map(&self) -> &CompiledLeafwingMapV1 {
        &self.inventory_overlay_map
    }

    /// HUD overlay allowlist while workbench is open.
    #[must_use]
    pub const fn workbench_overlay_map(&self) -> &CompiledLeafwingMapV1 {
        &self.workbench_overlay_map
    }

    /// Looks up a compiled action by stable ID.
    #[must_use]
    pub fn action_by_id(&self, id: &StableId) -> Option<&CompiledActionV1> {
        self.by_id
            .get(id)
            .and_then(|index| self.actions.get(usize::from(index.0)))
    }

    /// Looks up a compiled action by dense index.
    #[must_use]
    pub fn action_by_index(&self, index: CompiledActionIndex) -> Option<&CompiledActionV1> {
        self.actions.get(usize::from(index.0))
    }

    /// Looks up the compiled row for a client surface action.
    #[must_use]
    pub fn action_by_surface(&self, action: ClientSurfaceActionV1) -> Option<&CompiledActionV1> {
        parse_action_id(action.stable_id()).and_then(|id| self.action_by_id(&id))
    }

    /// Looks up the compiled row for an authoritative player action.
    #[must_use]
    pub fn action_by_player(
        &self,
        action: AuthoritativePlayerActionV1,
    ) -> Option<&CompiledActionV1> {
        parse_action_id(action.stable_id()).and_then(|id| self.action_by_id(&id))
    }

    /// Returns Controls rows for rebindable actions in stable ID order.
    #[must_use]
    pub fn controls_rows(&self) -> Vec<ControlsSettingRowV1> {
        self.actions
            .iter()
            .filter(|action| action.rebindable == RebindablePolicyV1::KeyboardMouse)
            .map(|action| ControlsSettingRowV1 {
                action_id: action.id.clone(),
                context: action.context,
                rebindable: action.rebindable,
                keyboard_mouse_bindings: action
                    .effective_bindings
                    .iter()
                    .filter(|binding| binding.is_keyboard_mouse())
                    .cloned()
                    .collect(),
            })
            .collect()
    }

    /// Canonical hash of provider, action IDs, and effective bindings.
    ///
    /// # Errors
    ///
    /// Returns [`latticeaxiom_core::CanonicalJsonError`] when encoding fails.
    pub fn evidence_hash(&self) -> Result<CanonicalHash, latticeaxiom_core::CanonicalJsonError> {
        let rows: Vec<_> = self
            .actions
            .iter()
            .map(|action| {
                (
                    action.id.as_str().to_owned(),
                    action.effective_bindings.clone(),
                )
            })
            .collect();
        canonical_json_hash(&(self.provider.as_str().to_owned(), rows))
    }

    /// Returns a profile with `candidate` applied to `action` if it is legal.
    ///
    /// # Errors
    ///
    /// Returns [`InputError`] when the action is unknown, not rebindable, the
    /// candidate is not keyboard/mouse, or the result conflicts.
    pub fn preview_rebind(
        &self,
        action: &StableId,
        candidate: InputBindingV1,
        profile: &BindingProfileV1,
    ) -> Result<BindingProfileV1, InputError> {
        let compiled =
            self.action_by_id(action)
                .ok_or_else(|| InputError::UnknownRebindTarget {
                    action: action.clone(),
                })?;
        if compiled.rebindable != RebindablePolicyV1::KeyboardMouse {
            return Err(InputError::ActionNotRebindable {
                action: action.clone(),
            });
        }
        if !candidate.is_keyboard_mouse() {
            return Err(InputError::GamepadRebindUnavailable {
                action: action.clone(),
            });
        }
        let mut preview = profile.clone();
        preview.set_override(action.clone(), vec![candidate]);
        compile_selected_catalog(self.provider.clone(), self.specs.clone(), &preview)?;
        Ok(preview)
    }

    /// Returns a profile that explicitly unbinds keyboard/mouse for `action`.
    ///
    /// # Errors
    ///
    /// Returns [`InputError`] when the action is unknown or not rebindable.
    pub fn preview_clear(
        &self,
        action: &StableId,
        profile: &BindingProfileV1,
    ) -> Result<BindingProfileV1, InputError> {
        let compiled =
            self.action_by_id(action)
                .ok_or_else(|| InputError::UnknownRebindTarget {
                    action: action.clone(),
                })?;
        if compiled.rebindable != RebindablePolicyV1::KeyboardMouse {
            return Err(InputError::ActionNotRebindable {
                action: action.clone(),
            });
        }
        let mut preview = profile.clone();
        preview.set_override(action.clone(), Vec::new());
        compile_selected_catalog(self.provider.clone(), self.specs.clone(), &preview)?;
        Ok(preview)
    }
}

/// Compiles graph-selected providers and a user profile into one catalog.
///
/// # Errors
///
/// Returns [`InputError`] when provider cardinality, catalog/enum goldens,
/// binding overlay, or same-context conflicts fail.
pub fn compile_input_catalog<I>(
    providers: I,
    profile: &BindingProfileV1,
) -> Result<CompiledInputCatalogV1, InputError>
where
    I: IntoIterator<Item = InputActionsProviderV1>,
{
    let selected = select_exactly_one_input_actions_provider(providers)?;
    compile_selected_catalog(selected.package, selected.catalog.actions, profile)
}

fn compile_selected_catalog(
    provider: PackageName,
    actions: Vec<ActionSpecV1>,
    profile: &BindingProfileV1,
) -> Result<CompiledInputCatalogV1, InputError> {
    let sorted = unique_sorted_actions(actions)?;
    assert_static_enum_goldens(&sorted)?;
    let specs = sorted.clone();
    let mut compiled_actions = Vec::with_capacity(sorted.len());
    let mut by_id = BTreeMap::new();
    let mut conflict_rows = Vec::new();
    let mut remaining_overrides = profile.overrides.clone();
    for (index, spec) in sorted.into_iter().enumerate() {
        let index =
            CompiledActionIndex(
                u16::try_from(index).map_err(|_| InputError::InvalidCatalog {
                    reason: "catalog exceeds the u16 dense index space".to_owned(),
                })?,
            );
        let user = remaining_overrides.remove(&spec.id);
        let effective = overlay_bindings(&spec, user.as_deref())?;
        conflict_rows.push((spec.id.clone(), spec.context, effective.clone()));
        by_id.insert(spec.id.clone(), index);
        compiled_actions.push(CompiledActionV1 {
            index,
            id: spec.id,
            kind: spec.kind,
            context: spec.context,
            rebindable: spec.rebindable,
            client_surface_action: spec.client_surface_action,
            authoritative_player_action: spec.authoritative_player_action,
            effective_bindings: effective,
        });
    }
    let conflict_refs: Vec<(StableId, InputContextV1, &[InputBindingV1])> = conflict_rows
        .iter()
        .map(|(id, context, bindings)| (id.clone(), *context, bindings.as_slice()))
        .collect();
    let conflicts = detect_same_context_conflicts(&conflict_refs);
    if !conflicts.is_empty() {
        return Err(InputError::BindingConflicts { conflicts });
    }
    let gameplay_map = leafwing_map(&compiled_actions, MapFilter::Gameplay);
    let surface_map = leafwing_map(&compiled_actions, MapFilter::Surface);
    let inventory_overlay_map = leafwing_map(&compiled_actions, MapFilter::InventoryOverlay);
    let workbench_overlay_map = leafwing_map(&compiled_actions, MapFilter::WorkbenchOverlay);
    Ok(CompiledInputCatalogV1 {
        provider,
        specs,
        actions: compiled_actions,
        by_id,
        orphan_overrides: remaining_overrides,
        gameplay_map,
        surface_map,
        inventory_overlay_map,
        workbench_overlay_map,
    })
}

fn overlay_bindings(
    spec: &ActionSpecV1,
    user: Option<&[InputBindingV1]>,
) -> Result<Vec<InputBindingV1>, InputError> {
    let mut gamepad = Vec::new();
    let mut keyboard_mouse = Vec::new();
    for binding in &spec.defaults {
        if binding.is_gamepad() {
            gamepad.push(binding.clone());
        } else if binding.is_keyboard_mouse() {
            keyboard_mouse.push(binding.clone());
        } else {
            return Err(InputError::UnknownRequiredMajor {
                schema: crate::ids::INPUT_BINDING_SCHEMA.to_owned(),
                reason: "catalog defaults cannot include unknown binding kinds",
            });
        }
    }
    match user {
        None => {
            let mut combined = keyboard_mouse;
            combined.extend(gamepad);
            Ok(combined)
        }
        Some(overrides) => {
            if spec.rebindable == RebindablePolicyV1::None {
                return Err(InputError::ActionNotRebindable {
                    action: spec.id.clone(),
                });
            }
            let mut applied = Vec::new();
            for binding in overrides {
                if binding.is_keyboard_mouse() {
                    binding.validate_for_kind(spec.kind)?;
                    applied.push(binding.clone());
                } else if binding.is_gamepad() {
                    return Err(InputError::GamepadRebindUnavailable {
                        action: spec.id.clone(),
                    });
                }
            }
            applied.extend(gamepad);
            Ok(applied)
        }
    }
}

#[derive(Clone, Copy)]
enum MapFilter {
    Gameplay,
    Surface,
    InventoryOverlay,
    WorkbenchOverlay,
}

fn leafwing_map(actions: &[CompiledActionV1], filter: MapFilter) -> CompiledLeafwingMapV1 {
    let entries = actions
        .iter()
        .filter(|action| matches_filter(action, filter))
        .map(|action| CompiledLeafwingEntryV1 {
            index: action.index,
            action_id: action.id.clone(),
            kind: action.kind,
            recipes: action
                .effective_bindings
                .iter()
                .filter_map(to_recipe)
                .collect(),
        })
        .filter(|entry| !entry.recipes.is_empty())
        .collect();
    CompiledLeafwingMapV1 { entries }
}

fn matches_filter(action: &CompiledActionV1, filter: MapFilter) -> bool {
    match filter {
        MapFilter::Gameplay => action.context == InputContextV1::Gameplay,
        MapFilter::Surface => action.context == InputContextV1::Surface,
        MapFilter::InventoryOverlay => is_hud_allowlisted(action, true),
        MapFilter::WorkbenchOverlay => is_hud_allowlisted(action, false),
    }
}

fn is_hud_allowlisted(action: &CompiledActionV1, inventory: bool) -> bool {
    match action.client_surface_action {
        Some(ClientSurfaceActionV1::ToggleInventory) => inventory,
        Some(ClientSurfaceActionV1::ToggleWorkbench) => !inventory,
        Some(
            ClientSurfaceActionV1::Back
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
            | ClientSurfaceActionV1::HotbarPrevious,
        ) => true,
        _ => false,
    }
}

fn to_recipe(binding: &InputBindingV1) -> Option<LeafwingRecipeV1> {
    match binding {
        InputBindingV1::Keyboard { usage, modifiers } => Some(LeafwingRecipeV1::Keyboard {
            usage: usage.clone(),
            modifiers: modifiers.iter().copied().collect(),
        }),
        InputBindingV1::KeyboardVirtualDPad {
            up,
            down,
            left,
            right,
            modifiers,
        } => Some(LeafwingRecipeV1::VirtualDPad {
            up: up.clone(),
            down: down.clone(),
            left: left.clone(),
            right: right.clone(),
            modifiers: modifiers.iter().copied().collect(),
        }),
        InputBindingV1::MouseButton { button } => {
            Some(LeafwingRecipeV1::MouseButton { button: *button })
        }
        InputBindingV1::MouseMotion { sensitivity } => Some(LeafwingRecipeV1::MouseMove {
            sensitivity: *sensitivity,
        }),
        InputBindingV1::MouseWheel { axis } => Some(LeafwingRecipeV1::MouseWheel { axis: *axis }),
        InputBindingV1::MouseWheelDirection { direction } => {
            Some(LeafwingRecipeV1::MouseWheelDirection {
                direction: *direction,
            })
        }
        InputBindingV1::GamepadButton { button } => Some(LeafwingRecipeV1::GamepadButton {
            button: button.clone(),
        }),
        InputBindingV1::GamepadStick {
            stick,
            deadzone,
            sensitivity,
        } => Some(LeafwingRecipeV1::GamepadStick {
            stick: *stick,
            deadzone: *deadzone,
            sensitivity: *sensitivity,
        }),
        InputBindingV1::Unknown { .. } => None,
    }
}

fn parse_action_id(value: &str) -> Option<StableId> {
    value.parse().ok()
}
