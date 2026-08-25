//! Official settings-page information architecture and mock-host contracts.

#![allow(
    clippy::expect_used,
    reason = "integration fixtures state their construction invariants"
)]

use latticeaxiom_client_ui::{SemanticKey, check_accesskit_tree};
use latticeaxiom_core::StableId;
use latticeaxiom_runtime_contracts::{ui_scale_setting_id, view_distance_setting_id};
use latticeaxiom_settings_ui::{
    MemorySettingsHost, SettingsCategoryV1, SettingsPageCatalogKind, SettingsPageCommand,
    SettingsPageHost, SettingsPageOpen, SettingsPageOutcome, SettingsPageSession,
    SettingsSectionV1, compile_baseline_page_catalog,
};
use serde_json::json;

fn setting(id: &str) -> StableId {
    id.parse().expect("setting fixture")
}

fn open_page(open: SettingsPageOpen) -> (SettingsPageSession, MemorySettingsHost) {
    let host = MemorySettingsHost::new();
    let page = SettingsPageSession::open(open, &host).expect("open settings page");
    (page, host)
}

#[test]
fn playable_catalog_compiles_without_developer_or_feature_conditional_rows() {
    let catalog =
        compile_baseline_page_catalog(SettingsPageCatalogKind::Playable).expect("playable catalog");
    let runtime = &catalog.as_catalog().runtime;
    assert!(runtime.contains_key(&ui_scale_setting_id()));
    assert!(runtime.contains_key(&view_distance_setting_id()));
    assert!(runtime.contains_key(&setting("latticeaxiom:setting/accessibility/high-contrast")));
    assert!(runtime.contains_key(&setting("latticeaxiom:setting/video/field-of-view")));
    assert!(!runtime.contains_key(&setting("latticeaxiom:setting/developer/overlay-enabled")));
    assert!(!runtime.contains_key(&setting("latticeaxiom:setting/audio/weather-volume")));
    assert!(!runtime.contains_key(&setting("terrenia:setting/world/difficulty")));
}

#[test]
fn developer_catalog_adds_overlay_rows() {
    let catalog = compile_baseline_page_catalog(SettingsPageCatalogKind::Developer)
        .expect("developer catalog");
    assert!(
        catalog
            .as_catalog()
            .runtime
            .contains_key(&setting("latticeaxiom:setting/developer/log-level"))
    );
}

#[test]
fn shell_page_has_all_nine_categories_and_transaction_bar() {
    let (page, host) = open_page(SettingsPageOpen::shell());
    assert_eq!(
        page.visible_categories(),
        vec![
            SettingsCategoryV1::Accessibility,
            SettingsCategoryV1::Controls,
            SettingsCategoryV1::Audio,
            SettingsCategoryV1::Video,
            SettingsCategoryV1::Interface,
            SettingsCategoryV1::Gameplay,
            SettingsCategoryV1::World,
            SettingsCategoryV1::Packages,
        ]
    );
    let tree = page.semantic_tree(&host).expect("tree");
    check_accesskit_tree(&tree).expect("a11y");
    assert!(tree.find(&key("settings/nav")).is_some());
    assert!(tree.find(&key("settings/sections")).is_some());
    assert!(tree.find(&key("settings/content")).is_some());
    assert!(tree.find(&key("settings/detail")).is_some());
    assert!(tree.find(&key("settings/transaction")).is_some());
    assert!(tree.find(&key("settings/apply")).is_some());
    assert!(tree.find(&key("settings/search")).is_some());
    assert!(tree.find(&key("settings/back")).is_some());
}

#[test]
fn in_game_page_hides_packages_and_keeps_controls() {
    let (page, host) = open_page(SettingsPageOpen::in_game(false));
    assert!(
        !page
            .visible_categories()
            .contains(&SettingsCategoryV1::Packages)
    );
    assert!(
        page.visible_categories()
            .contains(&SettingsCategoryV1::Controls)
    );
    let fragment = page.semantic_fragment(&host).expect("fragment");
    assert_eq!(fragment.key, key("modal/settings"));
    assert!(fragment.find(&key("settings/operation/export")).is_none());
}

#[test]
fn category_section_and_row_navigation_keep_breadcrumbs() {
    let (mut page, mut host) = open_page(SettingsPageOpen::shell());
    page.handle(
        SettingsPageCommand::SelectCategory(SettingsCategoryV1::Video),
        &mut host,
    )
    .expect("select video");
    assert_eq!(page.selected_category(), SettingsCategoryV1::Video);
    assert_eq!(page.selected_section(), SettingsSectionV1::VideoGeneral);
    page.handle(
        SettingsPageCommand::SelectSection(SettingsSectionV1::VideoQuality),
        &mut host,
    )
    .expect("select quality");
    assert!(!page.visible_rows().is_empty());
    let fog = setting("latticeaxiom:setting/video/fog-quality");
    page.handle(SettingsPageCommand::SelectSetting(fog.clone()), &mut host)
        .expect("select fog");
    let detail = page.selected_detail(&host).expect("detail");
    assert_eq!(detail.id, fog);
    assert_eq!(detail.owner, "@latticeaxiom/client-presentation");
    assert!(detail.description_key.contains("fog-quality"));
}

#[test]
fn advanced_video_is_hidden_until_the_interface_toggle() {
    let (mut page, mut host) = open_page(SettingsPageOpen::shell());
    page.handle(
        SettingsPageCommand::SelectCategory(SettingsCategoryV1::Video),
        &mut host,
    )
    .expect("video");
    assert!(
        !page
            .visible_sections()
            .contains(&SettingsSectionV1::VideoAdvanced)
    );
    page.handle(
        SettingsPageCommand::SetValue {
            setting: setting("latticeaxiom:setting/interface/show-advanced-settings"),
            value: json!(true),
        },
        &mut host,
    )
    .expect("show advanced");
    page.handle(
        SettingsPageCommand::SelectCategory(SettingsCategoryV1::Video),
        &mut host,
    )
    .expect("video again");
    assert!(
        page.visible_sections()
            .contains(&SettingsSectionV1::VideoAdvanced)
    );
}

#[test]
fn subtitle_rows_appear_only_when_subtitles_are_enabled() {
    let (mut page, mut host) = open_page(SettingsPageOpen::shell());
    page.handle(
        SettingsPageCommand::SelectSection(SettingsSectionV1::AccessibilityHearing),
        &mut host,
    )
    .expect("hearing");
    assert!(
        !page
            .visible_rows()
            .iter()
            .any(|row| { row.id.as_str() == "latticeaxiom:setting/accessibility/subtitle-scale" })
    );
    page.handle(
        SettingsPageCommand::SetValue {
            setting: setting("latticeaxiom:setting/accessibility/subtitles"),
            value: json!(true),
        },
        &mut host,
    )
    .expect("enable subtitles");
    page.handle(
        SettingsPageCommand::SelectSection(SettingsSectionV1::AccessibilityHearing),
        &mut host,
    )
    .expect("hearing after enable");
    assert!(
        page.visible_rows()
            .iter()
            .any(|row| { row.id.as_str() == "latticeaxiom:setting/accessibility/subtitle-scale" })
    );
}

#[test]
fn mock_host_apply_persists_user_draft_and_clamps_view_distance() {
    let (mut page, mut host) = open_page(SettingsPageOpen::shell());
    page.handle(
        SettingsPageCommand::SelectCategory(SettingsCategoryV1::Video),
        &mut host,
    )
    .expect("video");
    page.handle(
        SettingsPageCommand::SetValue {
            setting: view_distance_setting_id(),
            value: json!(24),
        },
        &mut host,
    )
    .expect("set view distance");
    assert!(page.surface().is_dirty());
    let outcome = page
        .handle(SettingsPageCommand::Apply, &mut host)
        .expect("apply");
    assert!(matches!(
        outcome,
        SettingsPageOutcome::Surface(
            latticeaxiom_settings_ui::SettingsSurfaceOutcome::ApplyCompleted { .. }
        )
    ));
    assert_eq!(
        host.values().get(&view_distance_setting_id()),
        Some(&json!(24))
    );
    assert!(!page.surface().is_dirty());
    let admission = host.admission(&view_distance_setting_id(), &json!(64));
    assert_eq!(admission.admitted, json!(32));
    assert!(admission.clamp_reason.is_some());
}

#[test]
fn mixed_user_and_world_draft_cannot_share_one_apply() {
    let (mut page, mut host) = open_page(SettingsPageOpen::in_game(true));
    page.handle(
        SettingsPageCommand::SetValue {
            setting: setting("latticeaxiom:setting/interface/text-scale"),
            value: json!(125),
        },
        &mut host,
    )
    .expect("text scale");
    page.handle(
        SettingsPageCommand::SetValue {
            setting: setting("latticeaxiom:setting/world/autosave-interval"),
            value: json!(10),
        },
        &mut host,
    )
    .expect("autosave");
    assert!(page.surface().mixed_durability_domains());
    let err = page
        .handle(SettingsPageCommand::Apply, &mut host)
        .expect_err("mixed domains");
    assert!(err.to_string().contains("durability"));
}

#[test]
fn world_rows_are_read_only_without_a_writer() {
    let (page, host) = open_page(SettingsPageOpen::in_game(false));
    let autosave = page
        .surface()
        .rows()
        .iter()
        .find(|row| row.id.as_str() == "latticeaxiom:setting/world/autosave-interval")
        .expect("autosave");
    assert!(!autosave.editable);
    let _ = host;
}

#[test]
fn controls_keyboard_lists_rebindable_actions() {
    let (mut page, mut host) = open_page(SettingsPageOpen::shell());
    page.handle(
        SettingsPageCommand::SelectCategory(SettingsCategoryV1::Controls),
        &mut host,
    )
    .expect("controls");
    page.handle(
        SettingsPageCommand::SelectSection(SettingsSectionV1::ControlsKeyboard),
        &mut host,
    )
    .expect("keyboard");
    assert!(page.visible_bindings().len() >= 20);
    let tree = page.semantic_tree(&host).expect("tree");
    assert!(
        tree.find(&key(
            "settings/controls/latticeaxiom:action/gameplay/pause-1"
        ))
        .is_some()
    );
}

#[test]
fn packages_operations_are_not_persisted_values() {
    let (mut page, mut host) = open_page(SettingsPageOpen::shell());
    page.handle(
        SettingsPageCommand::SelectCategory(SettingsCategoryV1::Packages),
        &mut host,
    )
    .expect("packages");
    assert_eq!(page.visible_operations().len(), 6);
    let tree = page.semantic_tree(&host).expect("tree");
    assert!(
        tree.find(&key("settings/operation/profile-draft"))
            .is_some()
    );
    assert!(page.visible_rows().is_empty());
}

#[test]
fn compact_layout_keeps_detail_and_transaction_reachable() {
    let mut open = SettingsPageOpen::shell();
    open.compact = true;
    let (page, host) = open_page(open);
    let fragment = page.semantic_fragment(&host).expect("compact fragment");
    let content = fragment.find(&key("settings/content")).expect("content");
    assert!(content.find(&key("settings/detail")).is_some());
    assert!(fragment.find(&key("settings/transaction")).is_some());
    assert!(fragment.find(&key("settings/apply")).is_some());
}

#[test]
fn search_matches_stable_id_and_keeps_section_context() {
    let (mut page, mut host) = open_page(SettingsPageOpen::shell());
    page.handle(
        SettingsPageCommand::SetSearch("view-distance".to_owned()),
        &mut host,
    )
    .expect("search");
    page.handle(
        SettingsPageCommand::SelectCategory(SettingsCategoryV1::Video),
        &mut host,
    )
    .expect("video");
    assert!(
        page.visible_rows()
            .iter()
            .any(|row| row.id == view_distance_setting_id())
    );
}

fn key(value: &str) -> SemanticKey {
    SemanticKey::new(value).expect("semantic key fixture")
}
