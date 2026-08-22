//! Settings surface vocabulary, transaction, and accessibility contracts.

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
    SettingsControlKind, SettingsReadOnlyReason, SettingsSurfaceAuthority, SettingsSurfaceError,
    SettingsSurfaceModel, SettingsSurfaceScope, SettingsSurfaceTransactionRequest,
};
use serde_json::json;

fn package_data(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../packages/latticeaxiom/settings-ui/data")
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
fn preview_cancel_uses_registry_rollback_order() {
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
    assert_eq!(
        SettingsSurfaceTransactionRequest::Rollback,
        SettingsSurfaceTransactionRequest::Rollback
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
    let model = SettingsSurfaceModel::from_snapshot(
        &catalog.as_catalog().runtime,
        snapshot.snapshot(),
        SettingsSurfaceAuthority {
            has_world_writer: true,
            writer: Some(SettingWriter::WorldOwner),
        },
    )
    .expect("model");
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
fn controls_rows_use_capture_keys_and_ui_never_persists() {
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
    assert_eq!(
        SettingsSurfaceTransactionRequest::Persist,
        SettingsSurfaceTransactionRequest::Persist
    );
}
