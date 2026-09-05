//! Settings surface vocabulary, state-machine, and accessibility contracts.

#![allow(
    clippy::expect_used,
    reason = "integration fixtures state their construction invariants"
)]

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::PathBuf;

use latticeaxiom_client_ui::{
    ControlVocabularyRef, SemanticKey, check_accesskit_tree, validate_control_vocabulary,
};
use latticeaxiom_core::{CanonicalHash, PackageName, StableId};
use latticeaxiom_runtime_contracts::{
    InputBindingV1, KnownInputBindingV1, RuntimeApplyImpact, ScopeOverlay, SettingAuthority,
    SettingScope, SettingSensitivity, SettingSpec, SettingTransactionRevision, SettingWriter,
    SettingsCatalogFragment, SettingsCatalogPolicy, StoreRevision, ValidatedSettingsCatalog,
    ValueType, resolve_effective_settings,
};
use latticeaxiom_settings_ui::{
    SettingsControlError, SettingsControlKind, SettingsReadOnlyReason, SettingsSliderConstraint,
    SettingsSliderDirection, SettingsSurfaceApplyResolution, SettingsSurfaceAuthority,
    SettingsSurfaceCommand, SettingsSurfaceError, SettingsSurfaceModel, SettingsSurfaceOutcome,
    SettingsSurfaceScope, SettingsSurfaceState,
};
use serde_json::json;

fn package_data(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../data")
        .join(name)
}

fn package(value: &str) -> PackageName {
    value.parse().expect("package fixture")
}

fn stable_id(value: &str) -> StableId {
    value.parse().expect("stable ID fixture")
}

fn setting(
    path: &str,
    category: &str,
    value_type: ValueType,
    default: serde_json::Value,
) -> SettingSpec {
    SettingSpec {
        id: stable_id(&format!("example:setting/{path}")),
        declared_by: package("@example/settings"),
        schema_version: 1,
        value_type,
        default,
        allowed_scopes: BTreeSet::from([SettingScope::User]),
        default_scope: SettingScope::User,
        authority: SettingAuthority::LocalUser,
        apply_impact: RuntimeApplyImpact::Immediate,
        category: stable_id(category),
        order: 0,
        label_key: path.to_owned(),
        description_key: format!("{path}.description"),
        visibility: None,
        enabled_when: None,
        sensitivity: SettingSensitivity::Ordinary,
        replacement: None,
    }
}

#[test]
fn package_vocabulary_matches_the_crate() {
    let categories: latticeaxiom_settings_ui::SettingsCategoryDocument = serde_json::from_str(
        &fs::read_to_string(package_data("categories.json")).expect("categories"),
    )
    .expect("categories json");
    categories.validate().expect("category vocabulary");
    let controls: latticeaxiom_settings_ui::SettingsControlDocument = serde_json::from_str(
        &fs::read_to_string(package_data("control-vocabulary.json")).expect("controls"),
    )
    .expect("controls json");
    controls.validate().expect("control vocabulary");
    validate_control_vocabulary(ControlVocabularyRef {
        major: controls.vocabulary_major,
        required: true,
    })
    .expect("shared control major");
}

#[test]
fn integer_slider_carries_catalog_bounds_and_step() {
    let spec = setting(
        "view-distance",
        "latticeaxiom:setting-category/video",
        ValueType::Integer {
            min: Some(2),
            max: Some(64),
            step: Some(2),
        },
        json!(12),
    );
    let catalog = ValidatedSettingsCatalog::compile(
        [SettingsCatalogFragment::new(
            package("@example/settings"),
            vec![spec],
        )],
        SettingsCatalogPolicy::default(),
    )
    .expect("bounded integer catalog");
    let snapshot = resolve_effective_settings(&catalog, [], CanonicalHash::digest(b"lock"))
        .expect("bounded integer snapshot");
    let model = SettingsSurfaceModel::from_snapshot(
        &catalog.as_catalog().runtime,
        snapshot.snapshot(),
        SettingsSurfaceAuthority::shell(),
    )
    .expect("bounded integer surface");

    let row = model.rows().first().expect("view-distance row");
    assert_eq!(row.control, SettingsControlKind::IntegerSlider);
    assert_eq!(
        row.slider,
        Some(SettingsSliderConstraint::Integer {
            min: 2,
            max: 64,
            step: 2,
        })
    );
}

#[test]
fn package_model_owns_slider_snap_apply_undo_and_cancel() {
    let spec = setting(
        "view-distance",
        "latticeaxiom:setting-category/video",
        ValueType::Integer {
            min: Some(2),
            max: Some(32),
            step: Some(2),
        },
        json!(8),
    );
    let setting_id = spec.id.clone();
    let catalog = ValidatedSettingsCatalog::compile(
        [SettingsCatalogFragment::new(
            package("@example/settings"),
            vec![spec],
        )],
        SettingsCatalogPolicy::default(),
    )
    .expect("view-distance catalog");
    let snapshot =
        resolve_effective_settings(&catalog, [], CanonicalHash::digest(b"lock")).expect("snapshot");
    let mut model = SettingsSurfaceModel::from_snapshot(
        &catalog.as_catalog().runtime,
        snapshot.snapshot(),
        SettingsSurfaceAuthority::shell(),
    )
    .expect("surface");

    assert!(!model.is_editing());
    assert!(matches!(
        model.handle(SettingsSurfaceCommand::BeginEdit),
        Ok(SettingsSurfaceOutcome::EditingBegan { .. })
    ));
    assert!(model.is_editing());
    assert_eq!(model.integer_slider(&setting_id).expect("slider").draft, 8);
    assert!(matches!(
        model.handle(SettingsSurfaceCommand::SetIntegerSliderValue {
            setting: setting_id.clone(),
            value: 2.6,
        }),
        Ok(SettingsSurfaceOutcome::DraftChanged { value, .. }) if value == json!(2)
    ));
    assert!(matches!(
        model.handle(SettingsSurfaceCommand::SetIntegerSliderValue {
            setting: setting_id.clone(),
            value: 3.4,
        }),
        Ok(SettingsSurfaceOutcome::DraftChanged { value, .. }) if value == json!(4)
    ));
    assert!(matches!(
        model.handle(SettingsSurfaceCommand::StepIntegerSlider {
            setting: setting_id.clone(),
            direction: SettingsSliderDirection::Increment,
        }),
        Ok(SettingsSurfaceOutcome::DraftChanged { value, .. }) if value == json!(6)
    ));
    let request = match model.handle(SettingsSurfaceCommand::Apply) {
        Ok(SettingsSurfaceOutcome::ApplyRequested(request)) => request,
        other => panic!("apply request: {other:?}"),
    };
    assert_eq!(request.proposed.get(&setting_id), Some(&json!(6)));
    assert!(matches!(
        model.finish_apply(SettingsSurfaceApplyResolution::Committed),
        Ok(SettingsSurfaceOutcome::ApplyCompleted {
            resolution: SettingsSurfaceApplyResolution::Committed
        })
    ));
    assert_eq!(
        model.applied_value(&setting_id),
        model.draft_value(&setting_id)
    );
    assert!(model.is_editing());
    assert_eq!(model.state(), &SettingsSurfaceState::Editing);

    model
        .handle(SettingsSurfaceCommand::SetIntegerSliderValue {
            setting: setting_id.clone(),
            value: 32.0,
        })
        .expect("new draft");
    assert!(matches!(
        model.handle(SettingsSurfaceCommand::Undo),
        Ok(SettingsSurfaceOutcome::DraftRestored { discarded }) if discarded == vec![setting_id.clone()]
    ));
    model
        .handle(SettingsSurfaceCommand::SetIntegerSliderValue {
            setting: setting_id.clone(),
            value: 12.0,
        })
        .expect("cancel draft");
    assert!(matches!(
        model.handle(SettingsSurfaceCommand::Cancel),
        Ok(SettingsSurfaceOutcome::Cancelled { discarded }) if discarded == vec![setting_id]
    ));
    assert!(!model.is_dirty());
    assert!(!model.is_editing());
    assert_eq!(
        model.lock_for_safe_restart(),
        SettingsSurfaceOutcome::SafeRestartLocked
    );
    assert_eq!(model.state(), &SettingsSurfaceState::SafeRestartLocked);
}

#[test]
fn non_aligned_schema_max_projects_the_last_reachable_integer() {
    let spec = setting(
        "odd-maximum",
        "latticeaxiom:setting-category/video",
        ValueType::Integer {
            min: Some(0),
            max: Some(5),
            step: Some(2),
        },
        json!(0),
    );
    let setting_id = spec.id.clone();
    let catalog = ValidatedSettingsCatalog::compile(
        [SettingsCatalogFragment::new(
            package("@example/settings"),
            vec![spec],
        )],
        SettingsCatalogPolicy::default(),
    )
    .expect("non-aligned integer catalog");
    let snapshot =
        resolve_effective_settings(&catalog, [], CanonicalHash::digest(b"lock")).expect("snapshot");
    let mut model = SettingsSurfaceModel::from_snapshot(
        &catalog.as_catalog().runtime,
        snapshot.snapshot(),
        SettingsSurfaceAuthority::shell(),
    )
    .expect("surface");

    assert_eq!(
        model.rows()[0].slider,
        Some(SettingsSliderConstraint::Integer {
            min: 0,
            max: 4,
            step: 2,
        })
    );
    assert_eq!(model.integer_slider(&setting_id).expect("slider").max, 4);
    model
        .handle(SettingsSurfaceCommand::BeginEdit)
        .expect("begin edit");
    assert!(matches!(
        model.handle(SettingsSurfaceCommand::SetIntegerSliderValue {
            setting: setting_id.clone(),
            value: 5.0,
        }),
        Ok(SettingsSurfaceOutcome::DraftChanged { value, .. }) if value == json!(4)
    ));
    assert_eq!(
        model.handle(SettingsSurfaceCommand::StepIntegerSlider {
            setting: setting_id.clone(),
            direction: SettingsSliderDirection::Increment,
        }),
        Ok(SettingsSurfaceOutcome::Unchanged)
    );
    assert!(matches!(
        model.handle(SettingsSurfaceCommand::StepIntegerSlider {
            setting: setting_id.clone(),
            direction: SettingsSliderDirection::Decrement,
        }),
        Ok(SettingsSurfaceOutcome::DraftChanged { value, .. }) if value == json!(2)
    ));
    assert!(matches!(
        model.handle(SettingsSurfaceCommand::SetValue {
            setting: setting_id,
            value: json!(5),
        }),
        Err(SettingsSurfaceError::InvalidDraftValue { .. })
    ));
}

#[test]
fn safe_restart_lock_is_terminal_for_every_draft_command() {
    let spec = setting(
        "locked",
        "latticeaxiom:setting-category/video",
        ValueType::Integer {
            min: Some(0),
            max: Some(8),
            step: Some(2),
        },
        json!(0),
    );
    let setting_id = spec.id.clone();
    let catalog = ValidatedSettingsCatalog::compile(
        [SettingsCatalogFragment::new(
            package("@example/settings"),
            vec![spec],
        )],
        SettingsCatalogPolicy::default(),
    )
    .expect("locked integer catalog");
    let snapshot =
        resolve_effective_settings(&catalog, [], CanonicalHash::digest(b"lock")).expect("snapshot");
    let mut model = SettingsSurfaceModel::from_snapshot(
        &catalog.as_catalog().runtime,
        snapshot.snapshot(),
        SettingsSurfaceAuthority::shell(),
    )
    .expect("surface");
    assert_eq!(model.state(), &SettingsSurfaceState::Closed);
    assert_eq!(
        model.handle(SettingsSurfaceCommand::Apply),
        Err(SettingsSurfaceError::NotEditing)
    );
    model
        .handle(SettingsSurfaceCommand::BeginEdit)
        .expect("begin edit");
    model
        .handle(SettingsSurfaceCommand::SetIntegerSliderValue {
            setting: setting_id.clone(),
            value: 4.0,
        })
        .expect("dirty draft");
    model
        .handle(SettingsSurfaceCommand::Apply)
        .expect("pending apply");
    assert!(model.apply_pending());
    assert_eq!(
        model.lock_for_safe_restart(),
        SettingsSurfaceOutcome::SafeRestartLocked
    );
    assert!(model.is_safe_restart_locked());
    assert!(!model.is_editing());
    assert!(!model.apply_pending());

    let commands = [
        SettingsSurfaceCommand::BeginEdit,
        SettingsSurfaceCommand::SetValue {
            setting: setting_id.clone(),
            value: json!(6),
        },
        SettingsSurfaceCommand::SetIntegerSliderValue {
            setting: setting_id.clone(),
            value: 6.0,
        },
        SettingsSurfaceCommand::StepIntegerSlider {
            setting: setting_id.clone(),
            direction: SettingsSliderDirection::Increment,
        },
        SettingsSurfaceCommand::Apply,
        SettingsSurfaceCommand::Undo,
        SettingsSurfaceCommand::Cancel,
    ];
    for command in commands {
        assert_eq!(
            model.handle(command),
            Err(SettingsSurfaceError::SafeRestartLocked)
        );
        assert_eq!(model.state(), &SettingsSurfaceState::SafeRestartLocked);
        assert_eq!(model.draft_value(&setting_id), Some(&json!(4)));
    }
    assert_eq!(
        model.finish_apply(SettingsSurfaceApplyResolution::Committed),
        Err(SettingsSurfaceError::NoPendingApply)
    );
    assert_eq!(model.state(), &SettingsSurfaceState::SafeRestartLocked);

    let fragment = model.semantic_fragment().expect("locked fragment");
    for key in ["settings/back", "settings/undo", "settings/apply"] {
        let node = fragment
            .find(&SemanticKey::new(key).expect("semantic key"))
            .expect("locked control");
        assert!(node.state.disabled);
        assert!(node.actions.is_empty());
    }
    let row = fragment
        .find(&SemanticKey::new("settings/row/example:setting/locked").expect("row key"))
        .expect("locked row");
    assert!(row.state.disabled);
    assert!(row.actions.is_empty());
}

#[test]
fn restart_required_completion_enters_the_typed_safe_restart_lock() {
    let spec = setting(
        "uncertain",
        "latticeaxiom:setting-category/video",
        ValueType::Bool,
        json!(false),
    );
    let setting_id = spec.id.clone();
    let catalog = ValidatedSettingsCatalog::compile(
        [SettingsCatalogFragment::new(
            package("@example/settings"),
            vec![spec],
        )],
        SettingsCatalogPolicy::default(),
    )
    .expect("uncertain catalog");
    let snapshot =
        resolve_effective_settings(&catalog, [], CanonicalHash::digest(b"lock")).expect("snapshot");
    let mut model = SettingsSurfaceModel::from_snapshot(
        &catalog.as_catalog().runtime,
        snapshot.snapshot(),
        SettingsSurfaceAuthority::shell(),
    )
    .expect("surface");
    model
        .handle(SettingsSurfaceCommand::BeginEdit)
        .expect("begin edit");
    model
        .handle(SettingsSurfaceCommand::SetValue {
            setting: setting_id,
            value: json!(true),
        })
        .expect("dirty draft");
    model
        .handle(SettingsSurfaceCommand::Apply)
        .expect("apply request");

    assert!(matches!(
        model.finish_apply(SettingsSurfaceApplyResolution::PublicationUnknownRestartRequired),
        Ok(SettingsSurfaceOutcome::ApplyCompleted {
            resolution: SettingsSurfaceApplyResolution::PublicationUnknownRestartRequired
        })
    ));
    assert_eq!(model.state(), &SettingsSurfaceState::SafeRestartLocked);
    assert_eq!(
        model.handle(SettingsSurfaceCommand::BeginEdit),
        Err(SettingsSurfaceError::SafeRestartLocked)
    );
}

#[test]
fn unbounded_number_slider_is_rejected_by_surface_projection() {
    let min = json!(0.5).as_number().expect("number minimum").clone();
    let step = json!(0.25).as_number().expect("number step").clone();
    let spec = setting(
        "gamma",
        "latticeaxiom:setting-category/video",
        ValueType::Number {
            min: Some(min),
            max: None,
            step: Some(step),
        },
        json!(1.0),
    );
    let catalog = ValidatedSettingsCatalog::compile(
        [SettingsCatalogFragment::new(
            package("@example/settings"),
            vec![spec],
        )],
        SettingsCatalogPolicy::default(),
    )
    .expect("catalog permits a generic unbounded number");
    let snapshot = resolve_effective_settings(&catalog, [], CanonicalHash::digest(b"lock"))
        .expect("unbounded number snapshot");

    let error = SettingsSurfaceModel::from_snapshot(
        &catalog.as_catalog().runtime,
        snapshot.snapshot(),
        SettingsSurfaceAuthority::shell(),
    )
    .expect_err("the required slider vocabulary must fail closed");
    assert_eq!(
        error,
        SettingsSurfaceError::Control(SettingsControlError::UnboundedSlider {
            setting: "example:setting/gamma".to_owned(),
        })
    );
}

#[test]
fn world_rows_are_read_only_without_writer_authority() {
    let mut spec = setting(
        "rule",
        "latticeaxiom:setting-category/world",
        ValueType::Bool,
        json!(false),
    );
    spec.authority = SettingAuthority::WorldOwner;
    spec.allowed_scopes = BTreeSet::from([SettingScope::World]);
    spec.default_scope = SettingScope::World;
    let catalog = ValidatedSettingsCatalog::compile(
        [SettingsCatalogFragment::new(
            package("@example/settings"),
            vec![spec],
        )],
        SettingsCatalogPolicy::default(),
    )
    .expect("catalog");
    let snapshot =
        resolve_effective_settings(&catalog, [], CanonicalHash::digest(b"lock")).expect("snapshot");
    let model = SettingsSurfaceModel::from_snapshot(
        &catalog.as_catalog().runtime,
        snapshot.snapshot(),
        SettingsSurfaceAuthority::shell(),
    )
    .expect("model");
    assert_eq!(model.rows()[0].control, SettingsControlKind::Toggle);
    assert!(!model.rows()[0].editable);
    assert_eq!(
        model.rows()[0].read_only_reason,
        Some(SettingsReadOnlyReason::MissingWriterAuthority)
    );
}

#[test]
fn runtime_contract_owns_preview_rollback_order() {
    let alpha = setting(
        "alpha",
        "latticeaxiom:setting-category/interface",
        ValueType::Bool,
        json!(false),
    );
    let beta = setting(
        "beta",
        "latticeaxiom:setting-category/interface",
        ValueType::Bool,
        json!(false),
    );
    let catalog = ValidatedSettingsCatalog::compile(
        [SettingsCatalogFragment::new(
            package("@example/settings"),
            vec![alpha.clone(), beta.clone()],
        )],
        SettingsCatalogPolicy::default(),
    )
    .expect("catalog");
    let lock = CanonicalHash::digest(b"lock");
    let before = resolve_effective_settings(&catalog, [], lock).expect("before");
    let proposed = resolve_effective_settings(
        &catalog,
        [ScopeOverlay::new(
            SettingScope::User,
            StoreRevision::new(1),
            SettingTransactionRevision::new(1),
            SettingWriter::LocalUser,
            None,
            BTreeMap::from([(alpha.id, json!(true)), (beta.id, json!(true))]),
        )],
        lock,
    )
    .expect("proposed");
    let rollback = before
        .snapshot()
        .diff(proposed.snapshot())
        .pre_persist_rollback_plan();
    assert_eq!(
        rollback
            .steps()
            .iter()
            .map(|step| step.id.to_string())
            .collect::<Vec<_>>(),
        vec![
            "example:setting/beta".to_owned(),
            "example:setting/alpha".to_owned()
        ]
    );
}

#[test]
fn mixed_durability_domains_cannot_share_one_apply() {
    let user = setting(
        "scale",
        "latticeaxiom:setting-category/interface",
        ValueType::Integer {
            min: Some(1),
            max: Some(2),
            step: Some(1),
        },
        json!(1),
    );
    let mut world = setting(
        "difficulty",
        "latticeaxiom:setting-category/world",
        ValueType::Bool,
        json!(false),
    );
    world.allowed_scopes = BTreeSet::from([SettingScope::World]);
    world.default_scope = SettingScope::World;
    world.authority = SettingAuthority::WorldOwner;
    let user_id = user.id.clone();
    let world_id = world.id.clone();
    let catalog = ValidatedSettingsCatalog::compile(
        [SettingsCatalogFragment::new(
            package("@example/settings"),
            vec![user, world],
        )],
        SettingsCatalogPolicy::default(),
    )
    .expect("catalog");
    let snapshot =
        resolve_effective_settings(&catalog, [], CanonicalHash::digest(b"lock")).expect("snapshot");
    let mut model = SettingsSurfaceModel::from_snapshot(
        &catalog.as_catalog().runtime,
        snapshot.snapshot(),
        SettingsSurfaceAuthority {
            has_world_writer: true,
            writer: Some(SettingWriter::WorldOwner),
        },
    )
    .expect("model");
    model
        .handle(SettingsSurfaceCommand::BeginEdit)
        .expect("begin edit");
    assert!(matches!(
        model.handle(SettingsSurfaceCommand::SetIntegerSliderValue {
            setting: user_id,
            value: 2.0,
        }),
        Ok(SettingsSurfaceOutcome::DraftChanged { .. })
    ));
    assert!(matches!(
        model.handle(SettingsSurfaceCommand::SetValue {
            setting: world_id,
            value: json!(true),
        }),
        Ok(SettingsSurfaceOutcome::DraftChanged { .. })
    ));
    assert_eq!(
        model.require_single_domain_apply(),
        Err(SettingsSurfaceError::MixedDurabilityDomains)
    );
}

#[test]
fn settings_tree_has_one_accesskit_root_and_apply_action() {
    let spec = setting(
        "scale",
        "latticeaxiom:setting-category/interface",
        ValueType::Integer {
            min: Some(1),
            max: Some(2),
            step: Some(1),
        },
        json!(1),
    );
    let catalog = ValidatedSettingsCatalog::compile(
        [SettingsCatalogFragment::new(
            package("@example/settings"),
            vec![spec],
        )],
        SettingsCatalogPolicy::default(),
    )
    .expect("catalog");
    let snapshot =
        resolve_effective_settings(&catalog, [], CanonicalHash::digest(b"lock")).expect("snapshot");
    let model = SettingsSurfaceModel::from_snapshot(
        &catalog.as_catalog().runtime,
        snapshot.snapshot(),
        SettingsSurfaceAuthority::shell(),
    )
    .expect("model");
    let tree = model.semantic_tree().expect("tree");
    let a11y = check_accesskit_tree(&tree).expect("a11y");
    assert_eq!(
        a11y.role,
        latticeaxiom_client_ui::AccessKitRole::Application
    );
    assert!(
        tree.find(&SemanticKey::new("settings/apply").expect("apply"))
            .is_some()
    );
    assert!(
        tree.find(&SemanticKey::new("settings/search").expect("search"))
            .is_some()
    );
}

#[test]
fn in_game_scope_hides_packages_and_discloses_restart_impact() {
    let mut scale = setting(
        "scale",
        "latticeaxiom:setting-category/interface",
        ValueType::Integer {
            min: Some(1),
            max: Some(2),
            step: Some(1),
        },
        json!(1),
    );
    scale.apply_impact = latticeaxiom_runtime_contracts::RuntimeApplyImpact::ProcessRestart;
    let scale_id = scale.id.clone();
    let mut packages = setting(
        "lock",
        "latticeaxiom:setting-category/packages",
        ValueType::Bool,
        json!(false),
    );
    packages.apply_impact = latticeaxiom_runtime_contracts::RuntimeApplyImpact::Immediate;
    let catalog = ValidatedSettingsCatalog::compile(
        [SettingsCatalogFragment::new(
            package("@example/settings"),
            vec![scale, packages],
        )],
        SettingsCatalogPolicy::default(),
    )
    .expect("catalog");
    let snapshot =
        resolve_effective_settings(&catalog, [], CanonicalHash::digest(b"lock")).expect("snapshot");
    let mut model = SettingsSurfaceModel::from_snapshot(
        &catalog.as_catalog().runtime,
        snapshot.snapshot(),
        SettingsSurfaceAuthority::shell(),
    )
    .expect("model");
    assert_eq!(model.rows().len(), 2);
    model.apply_scope_filter(SettingsSurfaceScope::InGame);
    assert_eq!(model.rows().len(), 1);
    assert_eq!(
        model.rows()[0].category,
        latticeaxiom_settings_ui::SettingsCategoryV1::Interface
    );
    model
        .handle(SettingsSurfaceCommand::BeginEdit)
        .expect("begin edit");
    assert!(matches!(
        model.handle(SettingsSurfaceCommand::SetIntegerSliderValue {
            setting: scale_id,
            value: 2.0,
        }),
        Ok(SettingsSurfaceOutcome::DraftChanged { .. })
    ));
    assert!(model.restart_impact().process_restart);
    let fragment = model.semantic_fragment().expect("fragment");
    assert_eq!(fragment.role, latticeaxiom_client_ui::SemanticRole::Group);
    let apply = fragment
        .find(&SemanticKey::new("settings/apply").expect("apply"))
        .expect("apply");
    assert!(
        apply
            .description
            .as_deref()
            .is_some_and(|text| text.contains("Process restart"))
    );
}

#[test]
fn controls_rows_use_capture_keys() {
    let default = serde_json::to_value(InputBindingV1::Known(KnownInputBindingV1::Keyboard {
        usage: "Escape".to_owned(),
        modifiers: latticeaxiom_runtime_contracts::KeyboardModifiersV1::default(),
    }))
    .expect("binding default");
    let spec = setting(
        "pause",
        "latticeaxiom:setting-category/controls",
        ValueType::KeyBinding,
        default,
    );
    let catalog = ValidatedSettingsCatalog::compile(
        [SettingsCatalogFragment::new(
            package("@example/settings"),
            vec![spec],
        )],
        SettingsCatalogPolicy::default(),
    )
    .expect("catalog");
    let snapshot =
        resolve_effective_settings(&catalog, [], CanonicalHash::digest(b"lock")).expect("snapshot");
    let model = SettingsSurfaceModel::from_snapshot(
        &catalog.as_catalog().runtime,
        snapshot.snapshot(),
        SettingsSurfaceAuthority::shell(),
    )
    .expect("model");
    assert_eq!(model.rows()[0].control, SettingsControlKind::KeyBinding);
    let tree = model.semantic_tree().expect("tree");
    assert!(
        tree.find(&SemanticKey::new("settings/controls/example:setting/pause").expect("controls"))
            .is_some()
    );
}
