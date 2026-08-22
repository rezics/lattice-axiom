//! Foundation catalog, atomic persistence, and apply/preview/rollback fixtures.

#![allow(
    clippy::expect_used,
    reason = "integration fixtures state their construction invariants"
)]

use std::collections::BTreeMap;
use std::fs;
use std::sync::atomic::{AtomicU64, Ordering};

use latticeaxiom_compose::SettingScope;
use latticeaxiom_core::{CanonicalHash, StableId};
use latticeaxiom_runtime_contracts::{
    BindingProfileV1, DeterministicLocalSettingsStore, FilesystemLocalSettingsStore,
    InputBindingV1, IsolationReason, KnownInputBindingV1, LatticeLocalSettingsV1,
    LocalSettingsOrigin, LocalSettingsStore, PreviewPolicyV1, SettingsApplyTransaction,
    SettingsCatalogFragment, SettingsCatalogPolicy, SettingsTransactionPhase, StoredSettingEntryV1,
    ValidatedSettingsCatalog, assert_foundation_catalog, clamp_view_distance_chunks,
    decode_local_settings, foundation_setting_specs, project_user_settings,
    resolve_effective_settings, settings_package_name, ui_scale_setting_id,
    view_distance_setting_id,
};

const PACKAGE_CATALOG: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../packages/latticeaxiom/settings/data/user-catalog-v1.json"
);
const COMPLETE_ENVELOPE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/settings/complete-user-settings.json"
);
const CORRUPT_ENVELOPE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/settings/corrupt-user-settings.json"
);
const NEWER_ENVELOPE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/settings/newer-user-settings.json"
);

static NEXT_TEST_DIRECTORY: AtomicU64 = AtomicU64::new(0);

fn read(path: &str) -> Vec<u8> {
    fs::read(path).unwrap_or_else(|error| panic!("fixture `{path}` must be readable: {error}"))
}

fn compile_package_catalog() -> ValidatedSettingsCatalog {
    let fragment = SettingsCatalogFragment::from_canonical_json(&read(PACKAGE_CATALOG))
        .unwrap_or_else(|error| panic!("package catalog fragment: {error}"));
    assert_eq!(fragment.owner(), &settings_package_name());
    ValidatedSettingsCatalog::compile([fragment], SettingsCatalogPolicy::default())
        .unwrap_or_else(|error| panic!("package catalog compile: {error}"))
}

fn keyboard(usage: &str) -> InputBindingV1 {
    InputBindingV1::Known(KnownInputBindingV1::Keyboard {
        usage: usage.to_owned(),
        modifiers: latticeaxiom_runtime_contracts::KeyboardModifiersV1::default(),
    })
}

#[test]
fn shipped_package_catalog_matches_owned_foundation_specs() {
    let catalog = compile_package_catalog();
    assert_foundation_catalog(&catalog)
        .unwrap_or_else(|error| panic!("foundation catalog: {error}"));
    let compiled = catalog.as_catalog();
    for expected in foundation_setting_specs() {
        let actual = compiled
            .runtime
            .get(&expected.id)
            .unwrap_or_else(|| panic!("missing {}", expected.id));
        assert_eq!(actual, &expected);
    }
}

#[test]
fn complete_envelope_round_trips_scale_view_distance_and_orphan_bindings() {
    let catalog = compile_package_catalog();
    let envelope = decode_local_settings(&read(COMPLETE_ENVELOPE))
        .unwrap_or_else(|reason| panic!("complete envelope: {reason:?}"));
    assert_eq!(envelope.user().len(), 2);
    let resolution = resolve_effective_settings(
        &catalog,
        [envelope.device_overlay(), envelope.user_overlay()],
        CanonicalHash::digest(b"lock"),
    )
    .unwrap_or_else(|error| panic!("resolve: {error}"));
    let projection =
        project_user_settings(resolution.snapshot(), envelope.binding_profile().clone(), 8)
            .unwrap_or_else(|error| panic!("project: {error}"));
    assert_eq!(projection.ui_scale().to_string(), "2.0");
    assert_eq!(projection.requested_view_distance(), 12);
    assert_eq!(projection.clamped_view_distance(), 8);
    assert_eq!(clamp_view_distance_chunks(12, 8), 8);

    let defaults = BTreeMap::from([(
        "latticeaxiom:action/gameplay/pause@1"
            .parse()
            .unwrap_or_else(|error| panic!("pause id: {error}")),
        vec![keyboard("Escape")],
    )]);
    let effective = envelope.binding_profile().overlay_defaults(&defaults);
    assert_eq!(effective.orphans().len(), 1);
    assert!(
        effective
            .orphans()
            .keys()
            .any(|id| id.as_str() == "latticeaxiom:action/gameplay/sprint@1")
    );
}

fn foundation_user_values() -> BTreeMap<StableId, serde_json::Value> {
    BTreeMap::from([
        (ui_scale_setting_id(), serde_json::json!(2.0)),
        (view_distance_setting_id(), serde_json::json!(12)),
    ])
}

#[test]
fn preview_cancel_rolls_back_foundation_rows_in_reverse_stable_order() {
    let catalog = compile_package_catalog();
    let lock = CanonicalHash::digest(b"shell-lock");
    let current = LatticeLocalSettingsV1::empty();
    let before = resolve_effective_settings(&catalog, [], lock)
        .unwrap_or_else(|error| panic!("before: {error}"));
    let proposed = foundation_user_values();
    let mut draft = SettingsApplyTransaction::user_draft(
        &catalog,
        before.snapshot(),
        &current,
        &proposed,
        BindingProfileV1::empty(),
        PreviewPolicyV1::Reversible {
            timeout_ms: 8_000,
            participants: vec![settings_package_name()],
        },
        lock,
    )
    .unwrap_or_else(|error| panic!("draft: {error}"));
    draft
        .prepare()
        .unwrap_or_else(|error| panic!("prepare: {error}"));
    assert_eq!(draft.begin_preview(), Ok(8_000));
    let rollback = draft
        .rollback_before_persist()
        .unwrap_or_else(|error| panic!("rollback: {error}"));
    assert_eq!(
        rollback
            .steps()
            .iter()
            .map(|step| step.id.to_string())
            .collect::<Vec<_>>(),
        vec![
            "latticeaxiom:setting/view-distance".to_owned(),
            "latticeaxiom:setting/ui-scale".to_owned()
        ]
    );
    draft
        .finish_rollback(true)
        .unwrap_or_else(|error| panic!("finish rollback: {error}"));
    assert_eq!(draft.phase(), SettingsTransactionPhase::RolledBack);
}

#[test]
fn persist_survives_replacement_process_and_cannot_roll_back() {
    let catalog = compile_package_catalog();
    let lock = CanonicalHash::digest(b"shell-lock");
    let current = LatticeLocalSettingsV1::empty();
    let before = resolve_effective_settings(&catalog, [], lock)
        .unwrap_or_else(|error| panic!("before: {error}"));
    let proposed = foundation_user_values();
    let mut apply = SettingsApplyTransaction::user_draft(
        &catalog,
        before.snapshot(),
        &current,
        &proposed,
        BindingProfileV1::new(
            1,
            BTreeMap::from([(
                "latticeaxiom:action/gameplay/pause@1"
                    .parse()
                    .unwrap_or_else(|error| panic!("pause id: {error}")),
                vec![keyboard("KeyP")],
            )]),
        )
        .unwrap_or_else(|error| panic!("profile: {error}")),
        PreviewPolicyV1::None,
        lock,
    )
    .unwrap_or_else(|error| panic!("apply draft: {error}"));
    apply
        .prepare()
        .unwrap_or_else(|error| panic!("apply prepare: {error}"));
    let serial = NEXT_TEST_DIRECTORY.fetch_add(1, Ordering::Relaxed);
    let root = std::env::temp_dir().join(format!(
        "latticeaxiom-settings-foundation-{}-{serial}",
        std::process::id()
    ));
    let store =
        FilesystemLocalSettingsStore::open(&root).unwrap_or_else(|error| panic!("open: {error}"));
    let (persisted, batch) = apply
        .persist(&store, &current, lock)
        .unwrap_or_else(|error| panic!("persist: {error}"));
    assert!(batch.restart_impact().live_immediate);
    assert_eq!(apply.phase(), SettingsTransactionPhase::Committed);
    assert!(apply.rollback_before_persist().is_err());

    let reopened =
        FilesystemLocalSettingsStore::open(&root).unwrap_or_else(|error| panic!("reopen: {error}"));
    let loaded = reopened
        .load()
        .unwrap_or_else(|error| panic!("reload: {error}"));
    assert_eq!(loaded.origin(), &LocalSettingsOrigin::Complete);
    assert_eq!(loaded.envelope(), &persisted);
    assert_eq!(
        loaded
            .envelope()
            .user()
            .get(&ui_scale_setting_id())
            .map(StoredSettingEntryV1::value),
        Some(&serde_json::json!(2.0))
    );
    assert_eq!(
        loaded
            .envelope()
            .user()
            .get(&view_distance_setting_id())
            .map(StoredSettingEntryV1::value),
        Some(&serde_json::json!(12))
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
fn corrupt_and_newer_fixtures_are_isolated_from_defaults() {
    let store = DeterministicLocalSettingsStore::new();
    store
        .install_visible(read(CORRUPT_ENVELOPE))
        .unwrap_or_else(|error| panic!("install corrupt: {error}"));
    let loaded = store
        .load()
        .unwrap_or_else(|error| panic!("load corrupt: {error}"));
    assert!(matches!(
        loaded.origin(),
        LocalSettingsOrigin::Isolated {
            reason: IsolationReason::Corrupt { .. },
            ..
        }
    ));
    assert_eq!(loaded.envelope(), &LatticeLocalSettingsV1::empty());

    let store = DeterministicLocalSettingsStore::new();
    store
        .install_visible(read(NEWER_ENVELOPE))
        .unwrap_or_else(|error| panic!("install newer: {error}"));
    let loaded = store
        .load()
        .unwrap_or_else(|error| panic!("load newer: {error}"));
    assert!(matches!(
        loaded.origin(),
        LocalSettingsOrigin::Isolated {
            reason: IsolationReason::NewerRequired {
                found: 2,
                supported: 1
            },
            ..
        }
    ));
    let isolated = store
        .isolated()
        .unwrap_or_else(|error| panic!("isolated: {error}"));
    assert_eq!(isolated.values().next(), Some(&read(NEWER_ENVELOPE)));
}

#[test]
fn user_scope_is_the_owning_persistence_lane_for_foundation_rows() {
    let catalog = compile_package_catalog();
    for id in [ui_scale_setting_id(), view_distance_setting_id()] {
        let spec = catalog
            .as_catalog()
            .runtime
            .get(&id)
            .unwrap_or_else(|| panic!("missing {id}"));
        assert!(spec.allowed_scopes.contains(&SettingScope::User));
        assert_eq!(spec.default_scope, SettingScope::User);
    }
}

#[test]
fn leftover_tmp_is_not_accepted_as_complete() {
    let store = DeterministicLocalSettingsStore::new();
    let envelope = decode_local_settings(&read(COMPLETE_ENVELOPE))
        .unwrap_or_else(|reason| panic!("complete: {reason:?}"));
    store
        .persist(&envelope)
        .unwrap_or_else(|error| panic!("persist complete: {error}"));
    store
        .install_temporary(read(CORRUPT_ENVELOPE))
        .unwrap_or_else(|error| panic!("install tmp: {error}"));
    let loaded = store
        .load()
        .unwrap_or_else(|error| panic!("load with tmp: {error}"));
    assert_eq!(loaded.origin(), &LocalSettingsOrigin::Complete);
    assert_eq!(loaded.envelope(), &envelope);
}
