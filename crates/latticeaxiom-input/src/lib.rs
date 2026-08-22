//! Headless-first input foundation for Lattice Axiom.
//!
//! This crate owns action catalog compilation, user binding-profile overlay,
//! same-context conflict detection, context-stack policy, Leafwing-facing
//! compiled maps, and headless logical-input APIs. It does not read Bevy
//! physical input, does not replace Leafwing, and does not `include_str!` a
//! production catalog. Callers must supply the lock-selected package document.

mod action;
mod binding;
mod catalog;
mod compile;
mod conflict;
mod context;
mod headless;
mod ids;
mod profile;

pub use action::{
    ActionKindV1, ActionSpecV1, AuthoritativePlayerActionV1, ClientSurfaceActionV1,
    EscSafetyActionV1, InputContextV1, RebindablePolicyV1,
};
pub use binding::{
    GamepadStickV1, InputBindingV1, KeyModifierV1, MouseButtonV1, MouseWheelAxisV1,
    OccupancyTokenV1,
};
pub use catalog::{
    ActionCatalogDocumentV1, InputActionsProviderV1, parse_package_name,
    select_exactly_one_input_actions_provider,
};
pub use compile::{
    CompiledActionIndex, CompiledActionV1, CompiledInputCatalogV1, CompiledLeafwingEntryV1,
    CompiledLeafwingMapV1, ControlsSettingRowV1, LeafwingRecipeV1, compile_input_catalog,
};
pub use conflict::{BindingConflictV1, detect_same_context_conflicts};
pub use context::{
    ActiveInputContextStack, CapturePolicyV1, ContextPolicyV1, ContextTransitionReceiptV1,
    CursorFocusPolicyV1, FocusOwnerV1, GameplayPolicyV1, HudOverlayKindV1, action_allowed,
    surface_action_on_hud_allowlist,
};
pub use headless::{Axis2ValueV1, HeadlessInputSession, LogicalInputSampleV1};
pub use ids::{
    ACTION_CATALOG_SCHEMA, BINDING_PROFILE_SCHEMA, INPUT_ACTIONS_CAPABILITY, INPUT_BINDING_SCHEMA,
    INPUT_PACKAGE_NAME, SHIPPED_ACTION_CATALOG_PACKAGE_PATH,
};
pub use profile::BindingProfileV1;

use latticeaxiom_core::{PackageName, StableId};
use thiserror::Error;

/// Typed failure for catalog selection, compile, profile, and context APIs.
#[derive(Debug, Error)]
pub enum InputError {
    /// The graph selected no input-actions provider.
    #[error("the product lock selected no input-actions provider")]
    MissingProvider,
    /// The graph selected more than one input-actions provider.
    #[error("the product lock selected multiple input-actions providers: {packages:?}")]
    DuplicateProviders {
        /// Conflicting package names in stable order.
        packages: Vec<PackageName>,
    },
    /// Catalog JSON or identity failed validation.
    #[error("invalid action catalog: {reason}")]
    InvalidCatalog {
        /// Validation diagnostic.
        reason: String,
    },
    /// Profile JSON failed validation.
    #[error("invalid binding profile: {reason}")]
    InvalidProfile {
        /// Validation diagnostic.
        reason: String,
    },
    /// A binding object failed validation.
    #[error("invalid input binding: {reason}")]
    InvalidBinding {
        /// Validation diagnostic.
        reason: String,
    },
    /// An action row failed validation.
    #[error("invalid action `{action}`: {reason}")]
    InvalidAction {
        /// Rejected action.
        action: StableId,
        /// Validation diagnostic.
        reason: &'static str,
    },
    /// Two rows declared the same stable ID.
    #[error("duplicate action `{action}`")]
    DuplicateAction {
        /// Duplicated action.
        action: StableId,
    },
    /// Package catalog and static client/player enums disagree.
    #[error("input catalog/enum mismatch: {detail}")]
    CatalogEnumMismatch {
        /// Mismatch diagnostic.
        detail: String,
    },
    /// A required schema major is newer than this crate understands.
    #[error("unknown required major for `{schema}`: {reason}")]
    UnknownRequiredMajor {
        /// Rejected schema identifier.
        schema: String,
        /// Reason the major cannot be accepted.
        reason: &'static str,
    },
    /// Same-context occupancy was claimed by multiple live actions.
    #[error("same-context binding conflicts: {conflicts:?}")]
    BindingConflicts {
        /// Occupying-action diagnostics in stable order.
        conflicts: Vec<BindingConflictV1>,
    },
    /// A rebind targeted an action missing from the compiled catalog.
    #[error("cannot rebind unknown action `{action}`")]
    UnknownRebindTarget {
        /// Missing action.
        action: StableId,
    },
    /// The action does not accept user keyboard/mouse overrides.
    #[error("action `{action}` is not rebindable")]
    ActionNotRebindable {
        /// Rejected action.
        action: StableId,
    },
    /// First-version gamepad mappings are package-fixed.
    #[error("gamepad rebinding is unavailable for `{action}`")]
    GamepadRebindUnavailable {
        /// Rejected action.
        action: StableId,
    },
    /// Context push/pop order violated the first-version stack grammar.
    #[error("illegal input context transition: {reason}")]
    IllegalContextTransition {
        /// Transition diagnostic.
        reason: String,
    },
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeSet, path::PathBuf, str::FromStr};

    use latticeaxiom_core::{CanonicalHash, PackageName, StableId, canonical_json_hash};

    use super::*;

    fn shipped_catalog_bytes() -> Vec<u8> {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../packages/latticeaxiom/input/data/action-catalog-v1.json");
        std::fs::read(path).expect("the shipped action catalog must exist")
    }

    fn shipped_catalog() -> ActionCatalogDocumentV1 {
        ActionCatalogDocumentV1::from_bytes(&shipped_catalog_bytes())
            .expect("the shipped action catalog must parse")
    }

    fn shipped_provider() -> InputActionsProviderV1 {
        let catalog = shipped_catalog();
        InputActionsProviderV1::new(
            catalog.owner_package.clone(),
            catalog.capability.clone(),
            catalog,
        )
        .expect("the shipped catalog must match its provider identity")
    }

    fn compile_defaults() -> CompiledInputCatalogV1 {
        compile_input_catalog([shipped_provider()], &BindingProfileV1::empty())
            .expect("the shipped catalog must compile against an empty profile")
    }

    fn action(id: &str) -> StableId {
        StableId::from_str(id).expect("test action IDs are valid")
    }

    #[test]
    fn player_discriminants_match_player_crate_contract() {
        assert_eq!(AuthoritativePlayerActionV1::Move as u8, 1);
        assert_eq!(AuthoritativePlayerActionV1::Look as u8, 2);
        assert_eq!(AuthoritativePlayerActionV1::Jump as u8, 3);
        assert_eq!(AuthoritativePlayerActionV1::BreakBlock as u8, 4);
        assert_eq!(AuthoritativePlayerActionV1::PlaceBlock as u8, 5);
        assert_eq!(AuthoritativePlayerActionV1::Inspect as u8, 6);
        assert_eq!(AuthoritativePlayerActionV1::Pause as u8, 7);
        assert_eq!(AuthoritativePlayerActionV1::PickBlock as u8, 9);
    }

    #[test]
    fn shipped_catalog_matches_static_enums_and_compiles() {
        let compiled = compile_defaults();
        assert_eq!(compiled.provider().as_str(), INPUT_PACKAGE_NAME);
        for surface in ClientSurfaceActionV1::ALL {
            let row = compiled
                .action_by_surface(surface)
                .expect("every surface action has a catalog row");
            assert_eq!(row.id.as_str(), surface.stable_id());
            assert_eq!(row.context, surface.declared_context());
        }
        for player in AuthoritativePlayerActionV1::ALL {
            let row = compiled
                .action_by_player(player)
                .expect("every player action has a catalog row");
            assert_eq!(row.id.as_str(), player.stable_id());
        }
        assert!(
            compiled
                .gameplay_map()
                .contains_action(&action("latticeaxiom:action/gameplay/move@1"))
        );
        assert!(
            compiled
                .surface_map()
                .contains_action(&action("latticeaxiom:action/ui/back@1"))
        );
        assert!(
            compiled
                .inventory_overlay_map()
                .contains_action(&action("latticeaxiom:action/hud/hotbar-slot-1@1"))
        );
        assert!(
            !compiled
                .inventory_overlay_map()
                .contains_action(&action("latticeaxiom:action/gameplay/move@1"))
        );
    }

    #[test]
    fn provider_order_does_not_change_effective_catalog() {
        let provider = shipped_provider();
        let profile = BindingProfileV1::empty();
        let first = compile_input_catalog([provider.clone()], &profile)
            .expect("compile")
            .evidence_hash()
            .expect("hash");
        let second = compile_input_catalog([provider], &profile)
            .expect("compile")
            .evidence_hash()
            .expect("hash");
        assert_eq!(first, second);
        assert_ne!(first, CanonicalHash::from_bytes([0; 32]));
    }

    #[test]
    fn duplicate_providers_fail_before_compile() {
        let left = shipped_provider();
        let mut right_catalog = shipped_catalog();
        right_catalog.owner_package =
            PackageName::from_str("@example/input").expect("valid package");
        let right = InputActionsProviderV1 {
            package: right_catalog.owner_package.clone(),
            capability: right_catalog.capability.clone(),
            catalog: right_catalog,
        };
        let error = compile_input_catalog([right, left], &BindingProfileV1::empty())
            .expect_err("duplicate providers must fail");
        assert!(matches!(error, InputError::DuplicateProviders { .. }));
    }

    #[test]
    fn missing_provider_fails() {
        let error = compile_input_catalog([], &BindingProfileV1::empty())
            .expect_err("zero providers must fail");
        assert!(matches!(error, InputError::MissingProvider));
    }

    #[test]
    fn binding_profile_round_trips_and_preserves_unknown_actions() {
        let unknown = action("latticeaxiom:action/mod/temporarily-missing@1");
        let mut profile = BindingProfileV1::empty();
        profile.set_override(
            unknown.clone(),
            vec![InputBindingV1::Keyboard {
                usage: "KeyQ".to_owned(),
                modifiers: [KeyModifierV1::Shift].into_iter().collect(),
            }],
        );
        let bytes = profile.to_canonical_bytes().expect("canonical bytes");
        let restored = BindingProfileV1::from_bytes(&bytes).expect("round-trip");
        assert_eq!(profile, restored);
        let compiled = compile_input_catalog([shipped_provider()], &restored).expect("compile");
        assert!(compiled.orphan_overrides().contains_key(&unknown));
        let again = BindingProfileV1::from_bytes(
            &restored
                .to_canonical_bytes()
                .expect("second canonical encode"),
        )
        .expect("second round-trip");
        assert_eq!(
            canonical_json_hash(&restored).expect("hash"),
            canonical_json_hash(&again).expect("hash")
        );
    }

    #[test]
    fn missing_override_inherits_and_empty_list_unbinds_keyboard_mouse() {
        let compiled_default = compile_defaults();
        let jump = action("latticeaxiom:action/gameplay/jump@1");
        let default_jump = compiled_default
            .action_by_id(&jump)
            .expect("jump")
            .effective_bindings
            .clone();
        assert!(default_jump.iter().any(
            |binding| matches!(binding, InputBindingV1::Keyboard { usage, .. } if usage == "Space")
        ));

        let mut unbind = BindingProfileV1::empty();
        unbind.set_override(jump.clone(), Vec::new());
        let compiled_unbind =
            compile_input_catalog([shipped_provider()], &unbind).expect("unbind compile");
        let unbound = &compiled_unbind
            .action_by_id(&jump)
            .expect("jump")
            .effective_bindings;
        assert!(unbound.iter().all(InputBindingV1::is_gamepad));
        assert!(
            unbound
                .iter()
                .any(|binding| matches!(binding, InputBindingV1::GamepadButton { button } if button == "South"))
        );
    }

    #[test]
    fn same_context_conflict_lists_occupying_actions_in_stable_order() {
        let mut profile = BindingProfileV1::empty();
        profile.set_override(
            action("latticeaxiom:action/hud/toggle-inventory@1"),
            vec![InputBindingV1::Keyboard {
                usage: "KeyC".to_owned(),
                modifiers: BTreeSet::default(),
            }],
        );
        let error = compile_input_catalog([shipped_provider()], &profile)
            .expect_err("KeyC is already workbench");
        let InputError::BindingConflicts { conflicts } = error else {
            panic!("expected binding conflicts, got {error}");
        };
        assert_eq!(conflicts.len(), 1);
        let occupying: Vec<_> = conflicts[0]
            .occupying_actions()
            .iter()
            .map(StableId::as_str)
            .collect();
        assert_eq!(
            occupying,
            [
                "latticeaxiom:action/hud/toggle-inventory@1",
                "latticeaxiom:action/hud/toggle-workbench@1"
            ]
        );
    }

    #[test]
    fn escape_may_be_reused_across_gameplay_and_surface() {
        let compiled = compile_defaults();
        let pause = compiled
            .action_by_surface(ClientSurfaceActionV1::Pause)
            .expect("pause");
        let back = compiled
            .action_by_surface(ClientSurfaceActionV1::Back)
            .expect("back");
        assert!(pause.effective_bindings.iter().any(
            |binding| matches!(binding, InputBindingV1::Keyboard { usage, .. } if usage == "Escape")
        ));
        assert!(back.effective_bindings.iter().any(
            |binding| matches!(binding, InputBindingV1::Keyboard { usage, .. } if usage == "Escape")
        ));
        assert_eq!(pause.context, InputContextV1::Gameplay);
        assert_eq!(back.context, InputContextV1::Surface);
    }

    #[test]
    fn virtual_dpad_occupies_component_keys() {
        let mut profile = BindingProfileV1::empty();
        profile.set_override(
            action("latticeaxiom:action/gameplay/jump@1"),
            vec![InputBindingV1::Keyboard {
                usage: "KeyW".to_owned(),
                modifiers: BTreeSet::default(),
            }],
        );
        let error = compile_input_catalog([shipped_provider()], &profile)
            .expect_err("KeyW is occupied by move");
        assert!(matches!(error, InputError::BindingConflicts { .. }));
    }

    #[test]
    fn inventory_overlay_blocks_gameplay_and_keeps_hotbar_allowlist() {
        let mut session = HeadlessInputSession::new(compile_defaults());
        session
            .set_axis(AuthoritativePlayerActionV1::Move, 1.0, 0.0)
            .expect("move axis");
        session
            .press_player(AuthoritativePlayerActionV1::BreakBlock)
            .expect("press break");
        session
            .push_context(
                InputContextV1::HudOverlay,
                Some(HudOverlayKindV1::Inventory),
            )
            .expect("open inventory");
        let opened = session.sample();
        assert!(opened.gameplay_suppressed);
        assert!(opened.axes.is_empty());
        assert!(
            !opened
                .held_player
                .contains(&AuthoritativePlayerActionV1::BreakBlock)
        );
        assert!(
            session
                .press_surface(ClientSurfaceActionV1::HotbarSlot3)
                .expect("hotbar allowed")
        );
        assert!(
            !session
                .press_player(AuthoritativePlayerActionV1::PlaceBlock)
                .expect("place is recorded physically but not dispatched")
        );
        let overlay = session.sample();
        assert!(
            overlay
                .held_surface
                .contains(&ClientSurfaceActionV1::HotbarSlot3)
        );
        assert!(
            !overlay
                .held_player
                .contains(&AuthoritativePlayerActionV1::PlaceBlock)
        );
        assert!(
            !overlay
                .started_player
                .contains(&AuthoritativePlayerActionV1::PlaceBlock)
        );
    }

    #[test]
    fn closing_a_surface_does_not_leak_the_consumed_toggle() {
        let mut session = HeadlessInputSession::new(compile_defaults());
        assert!(
            session
                .press_surface(ClientSurfaceActionV1::ToggleInventory)
                .expect("toggle from gameplay")
        );
        session
            .push_context(
                InputContextV1::HudOverlay,
                Some(HudOverlayKindV1::Inventory),
            )
            .expect("open");
        assert!(session.sample().started_surface.is_empty());
        session
            .release_surface(ClientSurfaceActionV1::ToggleInventory)
            .expect("release");
        assert!(
            session
                .press_surface(ClientSurfaceActionV1::ToggleInventory)
                .expect("close toggle is allowed")
        );
        session.pop_context().expect("close inventory");
        let after = session.sample();
        assert!(!after.gameplay_suppressed);
        assert!(
            !after
                .started_surface
                .contains(&ClientSurfaceActionV1::ToggleInventory)
        );
        assert!(
            !after
                .held_surface
                .contains(&ClientSurfaceActionV1::ToggleInventory)
        );
    }

    #[test]
    fn pause_and_settings_do_not_pass_gameplay_actions() {
        let mut session = HeadlessInputSession::new(compile_defaults());
        session
            .push_context(InputContextV1::Surface, None)
            .expect("pause/settings");
        assert!(
            !session
                .press_player(AuthoritativePlayerActionV1::Jump)
                .expect("jump suppressed")
        );
        assert!(
            session
                .press_surface(ClientSurfaceActionV1::NavDown)
                .expect("nav allowed")
        );
        assert!(
            session
                .press_surface(ClientSurfaceActionV1::Back)
                .expect("back allowed")
        );
        let sample = session.sample();
        assert!(sample.gameplay_suppressed);
        assert!(sample.held_player.is_empty());
        assert!(sample.started_player.is_empty());
        assert!(
            sample
                .held_surface
                .contains(&ClientSurfaceActionV1::NavDown)
        );
        session
            .push_context(InputContextV1::BindingCapture, None)
            .expect("capture");
        assert!(
            !session
                .press_surface(ClientSurfaceActionV1::NavDown)
                .expect("capture swallows surface nav")
        );
        assert_eq!(session.esc_safety(), EscSafetyActionV1::Cancel);
        assert!(session.sample().held_surface.is_empty());
    }

    #[test]
    fn esc_safety_survives_a_missing_profile_and_never_mutates_the_world() {
        let compiled = compile_defaults();
        let mut session = HeadlessInputSession::new(compiled);
        assert_eq!(session.esc_safety(), EscSafetyActionV1::Pause);
        session
            .push_context(
                InputContextV1::HudOverlay,
                Some(HudOverlayKindV1::Workbench),
            )
            .expect("workbench");
        assert_eq!(session.esc_safety(), EscSafetyActionV1::Back);
        session
            .push_context(InputContextV1::Surface, None)
            .expect("settings");
        assert_eq!(session.esc_safety(), EscSafetyActionV1::Back);
        session
            .push_context(InputContextV1::BindingCapture, None)
            .expect("capture");
        assert_eq!(session.esc_safety(), EscSafetyActionV1::Cancel);
    }

    #[test]
    fn unknown_required_major_fails_closed() {
        let error = BindingProfileV1::from_bytes(
            br#"{"schema":"latticeaxiom:schema/binding-profile@2","overrides":{}}"#,
        )
        .expect_err("major 2 is unknown");
        assert!(matches!(error, InputError::UnknownRequiredMajor { .. }));
    }

    #[test]
    fn catalog_enum_mismatch_fails_before_maps_are_built() {
        let mut catalog = shipped_catalog();
        catalog
            .actions
            .retain(|spec| spec.client_surface_action != Some(ClientSurfaceActionV1::NavUp));
        let provider = InputActionsProviderV1::new(
            catalog.owner_package.clone(),
            catalog.capability.clone(),
            catalog,
        )
        .expect("identity still matches");
        let error = compile_input_catalog([provider], &BindingProfileV1::empty())
            .expect_err("missing nav-up");
        assert!(matches!(error, InputError::CatalogEnumMismatch { .. }));
    }

    #[test]
    fn rebind_preview_is_atomic_and_immediate() {
        let compiled = compile_defaults();
        let inventory = action("latticeaxiom:action/hud/toggle-inventory@1");
        let profile = BindingProfileV1::empty();
        let preview = compiled
            .preview_rebind(
                &inventory,
                InputBindingV1::Keyboard {
                    usage: "KeyI".to_owned(),
                    modifiers: BTreeSet::default(),
                },
                &profile,
            )
            .expect("KeyI is free");
        let applied = compile_input_catalog([shipped_provider()], &preview).expect("apply");
        let bindings = &applied
            .action_by_id(&inventory)
            .expect("inventory")
            .effective_bindings;
        assert!(bindings.iter().any(
            |binding| matches!(binding, InputBindingV1::Keyboard { usage, .. } if usage == "KeyI")
        ));
        assert!(!bindings.iter().any(
            |binding| matches!(binding, InputBindingV1::Keyboard { usage, .. } if usage == "KeyE")
        ));
        compiled
            .preview_rebind(
                &inventory,
                InputBindingV1::Keyboard {
                    usage: "KeyC".to_owned(),
                    modifiers: BTreeSet::default(),
                },
                &profile,
            )
            .expect_err("conflict must not half-apply");
        assert!(profile.overrides.is_empty());
    }

    #[test]
    fn leafwing_maps_encode_wasd_mouse_and_standard_gamepad() {
        let compiled = compile_defaults();
        let move_entry = compiled
            .gameplay_map()
            .entries
            .iter()
            .find(|entry| entry.action_id.as_str() == "latticeaxiom:action/gameplay/move@1")
            .expect("move entry");
        assert!(move_entry.recipes.iter().any(|recipe| matches!(
            recipe,
            LeafwingRecipeV1::VirtualDPad {
                up,
                down,
                left,
                right,
                ..
            } if up == "KeyW" && down == "KeyS" && left == "KeyA" && right == "KeyD"
        )));
        assert!(move_entry.recipes.iter().any(|recipe| matches!(
            recipe,
            LeafwingRecipeV1::GamepadStick {
                stick: GamepadStickV1::Left,
                ..
            }
        )));
        let look_entry = compiled
            .gameplay_map()
            .entries
            .iter()
            .find(|entry| entry.action_id.as_str() == "latticeaxiom:action/gameplay/look@1")
            .expect("look entry");
        assert!(
            look_entry
                .recipes
                .iter()
                .any(|recipe| matches!(recipe, LeafwingRecipeV1::MouseMove { .. }))
        );
        assert!(
            compiled.controls_rows().iter().any(|row| {
                row.action_id.as_str() == "latticeaxiom:action/hud/toggle-inventory@1"
            })
        );
    }

    #[test]
    fn context_policies_are_not_a_single_boolean() {
        let gameplay = InputContextV1::Gameplay.policy();
        let overlay = InputContextV1::HudOverlay.policy();
        let surface = InputContextV1::Surface.policy();
        assert_eq!(gameplay.authoritative_gameplay, GameplayPolicyV1::Allow);
        assert_eq!(overlay.authoritative_gameplay, GameplayPolicyV1::Suppress);
        assert_eq!(overlay.capture, CapturePolicyV1::Overlay);
        assert_eq!(surface.capture, CapturePolicyV1::Exclusive);
        assert_eq!(surface.authoritative_gameplay, GameplayPolicyV1::Suppress);
        assert!(surface_action_on_hud_allowlist(
            ClientSurfaceActionV1::HotbarSlot1
        ));
        assert!(!surface_action_on_hud_allowlist(
            ClientSurfaceActionV1::NavUp
        ));
    }
}
