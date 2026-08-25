//! Playable-profile baseline catalog projected by the official settings page.
//!
//! Foundation IDs `latticeaxiom:setting/ui-scale` and
//! `latticeaxiom:setting/view-distance` stay owned by `@latticeaxiom/settings`
//! so the lock-selected persist path remains valid. Remaining baseline rows
//! use the shipped catalog identities. Feature-conditional rows without a
//! runtime consumer are omitted. `RuntimeApplyImpact::Preview` is mapped to
//! `Immediate` because the catalog compiler rejects Preview.

use std::collections::BTreeSet;

use latticeaxiom_core::{PackageName, StableId};
use latticeaxiom_runtime_contracts::{
    RuntimeApplyImpact, SettingAuthority, SettingPredicate, SettingScope, SettingSensitivity,
    SettingSpec, SettingsCatalogFragment, SettingsCatalogPolicy, ValidatedSettingsCatalog,
    ValueType, foundation_setting_specs,
};
use serde_json::{Value, json};

use crate::error::SettingsPageError;
use crate::layout::SettingsSectionV1;

/// Whether developer overlay rows are compiled into the page catalog.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SettingsPageCatalogKind {
    /// Ordinary playable profile. Developer category is absent.
    Playable,
    /// Dev-tools profile. Developer overlay rows are registered.
    Developer,
}

/// Compiles the official settings-page baseline catalog.
///
/// # Errors
///
/// Returns [`SettingsPageError`] when a fixture identity is invalid or catalog
/// compilation fails.
pub fn compile_baseline_page_catalog(
    kind: SettingsPageCatalogKind,
) -> Result<ValidatedSettingsCatalog, SettingsPageError> {
    let mut fragments = vec![
        SettingsCatalogFragment::new(
            package("@latticeaxiom/settings"),
            foundation_setting_specs(),
        ),
        SettingsCatalogFragment::new(package("@latticeaxiom/settings-ui"), settings_ui_rows()),
        SettingsCatalogFragment::new(
            package("@latticeaxiom/client-presentation"),
            client_presentation_rows(),
        ),
        SettingsCatalogFragment::new(package("@latticeaxiom/input"), input_rows()),
        SettingsCatalogFragment::new(package("@latticeaxiom/inspect"), inspect_rows()),
        SettingsCatalogFragment::new(package("@latticeaxiom/front-end"), front_end_rows()),
        SettingsCatalogFragment::new(package("@latticeaxiom/world-library"), world_library_rows()),
    ];
    if kind == SettingsPageCatalogKind::Developer {
        fragments.push(SettingsCatalogFragment::new(
            package("@latticeaxiom/dev-tools"),
            developer_rows(),
        ));
    }
    ValidatedSettingsCatalog::compile(fragments, SettingsCatalogPolicy::default())
        .map_err(SettingsPageError::Catalog)
}

/// Returns the frozen section for a compiled setting identity.
#[must_use]
pub fn section_for_setting(id: &StableId) -> SettingsSectionV1 {
    match id.as_str() {
        "latticeaxiom:setting/ui-scale"
        | "latticeaxiom:setting/interface/text-scale"
        | "latticeaxiom:setting/interface/show-advanced-settings" => {
            SettingsSectionV1::InterfaceDisplay
        }
        "latticeaxiom:setting/interface/language" => SettingsSectionV1::InterfaceLanguage,
        "latticeaxiom:setting/accessibility/high-contrast"
        | "latticeaxiom:setting/accessibility/narrator-mode" => {
            SettingsSectionV1::AccessibilityVision
        }
        "latticeaxiom:setting/accessibility/reduce-motion"
        | "latticeaxiom:setting/accessibility/notification-duration"
        | "latticeaxiom:setting/accessibility/camera-shake-scale" => {
            SettingsSectionV1::AccessibilityMotion
        }
        "latticeaxiom:setting/accessibility/subtitles"
        | "latticeaxiom:setting/accessibility/subtitle-direction"
        | "latticeaxiom:setting/accessibility/subtitle-scale"
        | "latticeaxiom:setting/accessibility/subtitle-background-opacity" => {
            SettingsSectionV1::AccessibilityHearing
        }
        id if id.starts_with("latticeaxiom:setting/input/mouse-")
            || id.starts_with("latticeaxiom:setting/input/invert-mouse-")
            || id.starts_with("latticeaxiom:setting/input/raw-mouse-")
            || id.starts_with("latticeaxiom:setting/input/scroll-")
            || id.starts_with("latticeaxiom:setting/input/discrete-scroll")
            || id.starts_with("latticeaxiom:setting/input/attack-mode")
            || id.starts_with("latticeaxiom:setting/input/use-mode") =>
        {
            SettingsSectionV1::ControlsMouse
        }
        id if id.starts_with("latticeaxiom:setting/input/gamepad-") => {
            SettingsSectionV1::ControlsGamepad
        }
        id if id.starts_with("latticeaxiom:setting/audio/")
            && (id.ends_with("-volume") && !id.ends_with("background-volume")
                || id.ends_with("/master-volume")
                || id.ends_with("/music-volume")
                || id.ends_with("/ambient-volume")
                || id.ends_with("/blocks-volume")
                || id.ends_with("/players-volume")
                || id.ends_with("/ui-volume")
                || id.ends_with("/narrator-volume")
                || id.ends_with("/background-volume")) =>
        {
            if id.ends_with("background-volume") {
                SettingsSectionV1::AudioDevices
            } else {
                SettingsSectionV1::AudioVolumes
            }
        }
        "latticeaxiom:setting/audio/output-device"
        | "latticeaxiom:setting/audio/directional-audio"
        | "latticeaxiom:setting/audio/mono-audio" => SettingsSectionV1::AudioDevices,
        "latticeaxiom:setting/view-distance"
        | "latticeaxiom:setting/video/performance-preset"
        | "latticeaxiom:setting/video/window-mode"
        | "latticeaxiom:setting/video/monitor"
        | "latticeaxiom:setting/video/resolution"
        | "latticeaxiom:setting/video/refresh-rate"
        | "latticeaxiom:setting/video/vsync"
        | "latticeaxiom:setting/video/frame-rate-limit"
        | "latticeaxiom:setting/video/background-frame-rate-limit"
        | "latticeaxiom:setting/video/field-of-view"
        | "latticeaxiom:setting/video/fov-effects-scale"
        | "latticeaxiom:setting/video/brightness" => SettingsSectionV1::VideoGeneral,
        "latticeaxiom:setting/video/graphics-quality"
        | "latticeaxiom:setting/video/particles"
        | "latticeaxiom:setting/video/smooth-lighting"
        | "latticeaxiom:setting/video/biome-blend-radius"
        | "latticeaxiom:setting/video/entity-distance-scale"
        | "latticeaxiom:setting/video/entity-shadows"
        | "latticeaxiom:setting/video/fog-quality"
        | "latticeaxiom:setting/video/chunk-fade-duration"
        | "latticeaxiom:setting/video/mipmap-levels"
        | "latticeaxiom:setting/video/anisotropic-filtering"
        | "latticeaxiom:setting/video/texture-filtering" => SettingsSectionV1::VideoQuality,
        "latticeaxiom:setting/video/chunk-update-mode"
        | "latticeaxiom:setting/video/block-face-culling"
        | "latticeaxiom:setting/video/fog-occlusion"
        | "latticeaxiom:setting/video/entity-culling"
        | "latticeaxiom:setting/video/animate-visible-textures-only" => {
            SettingsSectionV1::VideoPerformance
        }
        "latticeaxiom:setting/video/chunk-build-threads"
        | "latticeaxiom:setting/video/cpu-render-ahead" => SettingsSectionV1::VideoAdvanced,
        "latticeaxiom:setting/interface/inspect-visible"
        | "latticeaxiom:setting/interface/inspect-detail"
        | "latticeaxiom:setting/interface/inspect-pin-mode" => SettingsSectionV1::InterfaceInspect,
        "latticeaxiom:setting/gameplay/pause-on-focus-loss" => SettingsSectionV1::GameplayGeneral,
        "latticeaxiom:setting/world/default-root"
        | "latticeaxiom:setting/world/backup-before-migration"
        | "latticeaxiom:setting/world/trash-retention-days"
        | "latticeaxiom:setting/world/confirm-destructive-actions" => {
            SettingsSectionV1::WorldLibrary
        }
        "latticeaxiom:setting/world/autosave-interval" => SettingsSectionV1::WorldRules,
        _ => SettingsSectionV1::DeveloperOverlay,
    }
}

fn settings_ui_rows() -> Vec<SettingSpec> {
    vec![
        row(
            "latticeaxiom:setting/interface/text-scale",
            "@latticeaxiom/settings-ui",
            "latticeaxiom:setting-category/interface",
            20,
            integer(75, 200, 5),
            json!(100),
            SettingScope::User,
            SettingAuthority::LocalUser,
            RuntimeApplyImpact::Immediate,
        ),
        row(
            "latticeaxiom:setting/interface/language",
            "@latticeaxiom/settings-ui",
            "latticeaxiom:setting-category/interface",
            30,
            enumeration(&["system", "en", "zh-Hans", "zh-Hant", "ja"]),
            json!("system"),
            SettingScope::User,
            SettingAuthority::LocalUser,
            RuntimeApplyImpact::Immediate,
        ),
        row(
            "latticeaxiom:setting/interface/show-advanced-settings",
            "@latticeaxiom/settings-ui",
            "latticeaxiom:setting-category/interface",
            40,
            ValueType::Bool,
            json!(false),
            SettingScope::User,
            SettingAuthority::LocalUser,
            RuntimeApplyImpact::Immediate,
        ),
        row(
            "latticeaxiom:setting/accessibility/high-contrast",
            "@latticeaxiom/settings-ui",
            "latticeaxiom:setting-category/accessibility",
            10,
            ValueType::Bool,
            json!(false),
            SettingScope::User,
            SettingAuthority::LocalUser,
            RuntimeApplyImpact::Immediate,
        ),
        row(
            "latticeaxiom:setting/accessibility/reduce-motion",
            "@latticeaxiom/settings-ui",
            "latticeaxiom:setting-category/accessibility",
            20,
            ValueType::Bool,
            json!(false),
            SettingScope::User,
            SettingAuthority::LocalUser,
            RuntimeApplyImpact::Immediate,
        ),
        row(
            "latticeaxiom:setting/accessibility/notification-duration",
            "@latticeaxiom/settings-ui",
            "latticeaxiom:setting-category/accessibility",
            30,
            enumeration(&["short", "normal", "long"]),
            json!("normal"),
            SettingScope::User,
            SettingAuthority::LocalUser,
            RuntimeApplyImpact::Immediate,
        ),
    ]
}

fn client_presentation_rows() -> Vec<SettingSpec> {
    let mut rows = vec![
        row(
            "latticeaxiom:setting/accessibility/narrator-mode",
            "@latticeaxiom/client-presentation",
            "latticeaxiom:setting-category/accessibility",
            40,
            enumeration(&["off", "system", "all"]),
            json!("off"),
            SettingScope::User,
            SettingAuthority::LocalUser,
            RuntimeApplyImpact::Immediate,
        ),
        row(
            "latticeaxiom:setting/accessibility/subtitles",
            "@latticeaxiom/client-presentation",
            "latticeaxiom:setting-category/accessibility",
            50,
            ValueType::Bool,
            json!(false),
            SettingScope::User,
            SettingAuthority::LocalUser,
            RuntimeApplyImpact::Immediate,
        ),
        visible_when(
            row(
                "latticeaxiom:setting/accessibility/subtitle-direction",
                "@latticeaxiom/client-presentation",
                "latticeaxiom:setting-category/accessibility",
                51,
                ValueType::Bool,
                json!(true),
                SettingScope::User,
                SettingAuthority::LocalUser,
                RuntimeApplyImpact::Immediate,
            ),
            "latticeaxiom:setting/accessibility/subtitles",
            json!(true),
        ),
        visible_when(
            row(
                "latticeaxiom:setting/accessibility/subtitle-scale",
                "@latticeaxiom/client-presentation",
                "latticeaxiom:setting-category/accessibility",
                52,
                integer(75, 200, 5),
                json!(100),
                SettingScope::User,
                SettingAuthority::LocalUser,
                RuntimeApplyImpact::Immediate,
            ),
            "latticeaxiom:setting/accessibility/subtitles",
            json!(true),
        ),
        visible_when(
            row(
                "latticeaxiom:setting/accessibility/subtitle-background-opacity",
                "@latticeaxiom/client-presentation",
                "latticeaxiom:setting-category/accessibility",
                53,
                integer(0, 100, 5),
                json!(50),
                SettingScope::User,
                SettingAuthority::LocalUser,
                RuntimeApplyImpact::Immediate,
            ),
            "latticeaxiom:setting/accessibility/subtitles",
            json!(true),
        ),
        row(
            "latticeaxiom:setting/accessibility/camera-shake-scale",
            "@latticeaxiom/client-presentation",
            "latticeaxiom:setting-category/accessibility",
            60,
            integer(0, 100, 5),
            json!(100),
            SettingScope::User,
            SettingAuthority::LocalUser,
            RuntimeApplyImpact::Immediate,
        ),
    ];
    rows.extend(audio_rows());
    rows.extend(video_rows());
    rows
}

fn audio_rows() -> Vec<SettingSpec> {
    let owner = "@latticeaxiom/client-presentation";
    let category = "latticeaxiom:setting-category/audio";
    let mut rows = vec![device_row(
        "latticeaxiom:setting/audio/master-volume",
        owner,
        category,
        10,
        integer(0, 100, 1),
        json!(100),
        RuntimeApplyImpact::Immediate,
    )];
    for (order, path, default) in [
        (20, "music-volume", 50),
        (30, "ambient-volume", 100),
        (40, "blocks-volume", 100),
        (50, "players-volume", 100),
        (60, "ui-volume", 100),
        (70, "narrator-volume", 100),
        (80, "background-volume", 25),
    ] {
        rows.push(row(
            &format!("latticeaxiom:setting/audio/{path}"),
            owner,
            category,
            order,
            integer(0, 100, 1),
            json!(default),
            SettingScope::User,
            SettingAuthority::LocalUser,
            RuntimeApplyImpact::Immediate,
        ));
    }
    rows.push(device_row(
        "latticeaxiom:setting/audio/output-device",
        owner,
        category,
        90,
        enumeration(&["system"]),
        json!("system"),
        RuntimeApplyImpact::ProcessRestart,
    ));
    rows.push(device_row(
        "latticeaxiom:setting/audio/directional-audio",
        owner,
        category,
        100,
        enumeration(&["auto", "off", "on"]),
        json!("auto"),
        RuntimeApplyImpact::ProcessRestart,
    ));
    rows.push(row(
        "latticeaxiom:setting/audio/mono-audio",
        owner,
        category,
        110,
        ValueType::Bool,
        json!(false),
        SettingScope::User,
        SettingAuthority::LocalUser,
        RuntimeApplyImpact::Immediate,
    ));
    rows
}

fn video_rows() -> Vec<SettingSpec> {
    let mut rows = video_general_rows();
    rows.extend(video_quality_rows());
    rows.extend(video_performance_rows());
    rows
}

#[allow(
    clippy::too_many_lines,
    reason = "video general rows are a closed catalog table"
)]
fn video_general_rows() -> Vec<SettingSpec> {
    let owner = "@latticeaxiom/client-presentation";
    let category = "latticeaxiom:setting-category/video";
    vec![
        device_row(
            "latticeaxiom:setting/video/performance-preset",
            owner,
            category,
            10,
            enumeration(&["safe", "balanced", "quality", "custom"]),
            json!("balanced"),
            RuntimeApplyImpact::Immediate,
        ),
        device_row(
            "latticeaxiom:setting/video/window-mode",
            owner,
            category,
            20,
            enumeration(&["windowed", "borderless", "exclusive"]),
            json!("windowed"),
            RuntimeApplyImpact::Immediate,
        ),
        device_row(
            "latticeaxiom:setting/video/monitor",
            owner,
            category,
            30,
            enumeration(&["primary"]),
            json!("primary"),
            RuntimeApplyImpact::Immediate,
        ),
        device_row(
            "latticeaxiom:setting/video/resolution",
            owner,
            category,
            40,
            enumeration(&["current"]),
            json!("current"),
            RuntimeApplyImpact::Immediate,
        ),
        device_row(
            "latticeaxiom:setting/video/refresh-rate",
            owner,
            category,
            50,
            enumeration(&["current"]),
            json!("current"),
            RuntimeApplyImpact::Immediate,
        ),
        device_row(
            "latticeaxiom:setting/video/vsync",
            owner,
            category,
            60,
            ValueType::Bool,
            json!(true),
            RuntimeApplyImpact::Immediate,
        ),
        device_row(
            "latticeaxiom:setting/video/frame-rate-limit",
            owner,
            category,
            70,
            enumeration(&["30", "60", "90", "120", "144", "165", "240", "unlimited"]),
            json!("120"),
            RuntimeApplyImpact::Immediate,
        ),
        device_row(
            "latticeaxiom:setting/video/background-frame-rate-limit",
            owner,
            category,
            80,
            integer(5, 60, 5),
            json!(30),
            RuntimeApplyImpact::Immediate,
        ),
        row(
            "latticeaxiom:setting/video/field-of-view",
            owner,
            category,
            90,
            integer(50, 110, 1),
            json!(75),
            SettingScope::User,
            SettingAuthority::LocalUser,
            RuntimeApplyImpact::Immediate,
        ),
        row(
            "latticeaxiom:setting/video/fov-effects-scale",
            owner,
            category,
            100,
            integer(0, 100, 5),
            json!(100),
            SettingScope::User,
            SettingAuthority::LocalUser,
            RuntimeApplyImpact::Immediate,
        ),
        row(
            "latticeaxiom:setting/video/brightness",
            owner,
            category,
            110,
            integer(0, 100, 5),
            json!(50),
            SettingScope::User,
            SettingAuthority::LocalUser,
            RuntimeApplyImpact::Immediate,
        ),
    ]
}

#[allow(
    clippy::too_many_lines,
    reason = "video quality rows are a closed catalog table"
)]
fn video_quality_rows() -> Vec<SettingSpec> {
    let owner = "@latticeaxiom/client-presentation";
    let category = "latticeaxiom:setting-category/video";
    vec![
        row(
            "latticeaxiom:setting/video/graphics-quality",
            owner,
            category,
            200,
            enumeration(&["fast", "balanced", "fancy"]),
            json!("balanced"),
            SettingScope::User,
            SettingAuthority::LocalUser,
            RuntimeApplyImpact::Immediate,
        ),
        row(
            "latticeaxiom:setting/video/particles",
            owner,
            category,
            210,
            enumeration(&["minimal", "decreased", "all"]),
            json!("all"),
            SettingScope::User,
            SettingAuthority::LocalUser,
            RuntimeApplyImpact::Immediate,
        ),
        row(
            "latticeaxiom:setting/video/smooth-lighting",
            owner,
            category,
            220,
            enumeration(&["off", "low", "high"]),
            json!("high"),
            SettingScope::User,
            SettingAuthority::LocalUser,
            RuntimeApplyImpact::WorldReactivate,
        ),
        row(
            "latticeaxiom:setting/video/biome-blend-radius",
            owner,
            category,
            230,
            integer(0, 7, 1),
            json!(2),
            SettingScope::User,
            SettingAuthority::LocalUser,
            RuntimeApplyImpact::WorldReactivate,
        ),
        row(
            "latticeaxiom:setting/video/entity-distance-scale",
            owner,
            category,
            240,
            integer(50, 200, 25),
            json!(100),
            SettingScope::User,
            SettingAuthority::LocalUser,
            RuntimeApplyImpact::Immediate,
        ),
        row(
            "latticeaxiom:setting/video/entity-shadows",
            owner,
            category,
            250,
            ValueType::Bool,
            json!(true),
            SettingScope::User,
            SettingAuthority::LocalUser,
            RuntimeApplyImpact::Immediate,
        ),
        row(
            "latticeaxiom:setting/video/fog-quality",
            owner,
            category,
            260,
            enumeration(&["off", "fast", "fancy"]),
            json!("fancy"),
            SettingScope::User,
            SettingAuthority::LocalUser,
            RuntimeApplyImpact::Immediate,
        ),
        row(
            "latticeaxiom:setting/video/chunk-fade-duration",
            owner,
            category,
            270,
            integer(0, 2000, 50),
            json!(500),
            SettingScope::User,
            SettingAuthority::LocalUser,
            RuntimeApplyImpact::Immediate,
        ),
        device_row(
            "latticeaxiom:setting/video/mipmap-levels",
            owner,
            category,
            280,
            integer(0, 4, 1),
            json!(4),
            RuntimeApplyImpact::WorldReactivate,
        ),
        device_row(
            "latticeaxiom:setting/video/anisotropic-filtering",
            owner,
            category,
            290,
            enumeration(&["off", "2x", "4x", "8x", "16x"]),
            json!("off"),
            RuntimeApplyImpact::WorldReactivate,
        ),
        row(
            "latticeaxiom:setting/video/texture-filtering",
            owner,
            category,
            300,
            enumeration(&["nearest", "linear"]),
            json!("nearest"),
            SettingScope::User,
            SettingAuthority::LocalUser,
            RuntimeApplyImpact::WorldReactivate,
        ),
    ]
}

fn video_performance_rows() -> Vec<SettingSpec> {
    let owner = "@latticeaxiom/client-presentation";
    let category = "latticeaxiom:setting-category/video";
    vec![
        row(
            "latticeaxiom:setting/video/chunk-update-mode",
            owner,
            category,
            400,
            enumeration(&["immediate", "soon", "deferred"]),
            json!("soon"),
            SettingScope::User,
            SettingAuthority::LocalUser,
            RuntimeApplyImpact::Immediate,
        ),
        device_row(
            "latticeaxiom:setting/video/block-face-culling",
            owner,
            category,
            410,
            ValueType::Bool,
            json!(true),
            RuntimeApplyImpact::WorldReactivate,
        ),
        device_row(
            "latticeaxiom:setting/video/fog-occlusion",
            owner,
            category,
            420,
            ValueType::Bool,
            json!(true),
            RuntimeApplyImpact::Immediate,
        ),
        device_row(
            "latticeaxiom:setting/video/entity-culling",
            owner,
            category,
            430,
            ValueType::Bool,
            json!(true),
            RuntimeApplyImpact::Immediate,
        ),
        device_row(
            "latticeaxiom:setting/video/animate-visible-textures-only",
            owner,
            category,
            440,
            ValueType::Bool,
            json!(true),
            RuntimeApplyImpact::Immediate,
        ),
        device_row(
            "latticeaxiom:setting/video/chunk-build-threads",
            owner,
            category,
            500,
            enumeration(&["auto", "1", "2", "4", "8"]),
            json!("auto"),
            RuntimeApplyImpact::WorldReactivate,
        ),
        device_row(
            "latticeaxiom:setting/video/cpu-render-ahead",
            owner,
            category,
            510,
            integer(1, 4, 1),
            json!(2),
            RuntimeApplyImpact::ProcessRestart,
        ),
    ]
}

fn input_rows() -> Vec<SettingSpec> {
    let mut rows = mouse_rows();
    rows.extend(gamepad_rows());
    rows
}

fn mouse_rows() -> Vec<SettingSpec> {
    let owner = "@latticeaxiom/input";
    let category = "latticeaxiom:setting-category/controls";
    vec![
        row(
            "latticeaxiom:setting/input/mouse-sensitivity",
            owner,
            category,
            10,
            integer(1, 200, 1),
            json!(100),
            SettingScope::User,
            SettingAuthority::LocalUser,
            RuntimeApplyImpact::Immediate,
        ),
        row(
            "latticeaxiom:setting/input/invert-mouse-x",
            owner,
            category,
            20,
            ValueType::Bool,
            json!(false),
            SettingScope::User,
            SettingAuthority::LocalUser,
            RuntimeApplyImpact::Immediate,
        ),
        row(
            "latticeaxiom:setting/input/invert-mouse-y",
            owner,
            category,
            30,
            ValueType::Bool,
            json!(false),
            SettingScope::User,
            SettingAuthority::LocalUser,
            RuntimeApplyImpact::Immediate,
        ),
        device_row(
            "latticeaxiom:setting/input/raw-mouse-input",
            owner,
            category,
            40,
            ValueType::Bool,
            json!(true),
            RuntimeApplyImpact::Immediate,
        ),
        row(
            "latticeaxiom:setting/input/scroll-sensitivity",
            owner,
            category,
            50,
            integer(25, 200, 5),
            json!(100),
            SettingScope::User,
            SettingAuthority::LocalUser,
            RuntimeApplyImpact::Immediate,
        ),
        row(
            "latticeaxiom:setting/input/discrete-scroll",
            owner,
            category,
            60,
            ValueType::Bool,
            json!(false),
            SettingScope::User,
            SettingAuthority::LocalUser,
            RuntimeApplyImpact::Immediate,
        ),
        row(
            "latticeaxiom:setting/input/attack-mode",
            owner,
            category,
            70,
            enumeration(&["hold", "toggle"]),
            json!("hold"),
            SettingScope::User,
            SettingAuthority::LocalUser,
            RuntimeApplyImpact::Immediate,
        ),
        row(
            "latticeaxiom:setting/input/use-mode",
            owner,
            category,
            80,
            enumeration(&["hold", "toggle"]),
            json!("hold"),
            SettingScope::User,
            SettingAuthority::LocalUser,
            RuntimeApplyImpact::Immediate,
        ),
    ]
}

fn gamepad_rows() -> Vec<SettingSpec> {
    let owner = "@latticeaxiom/input";
    let category = "latticeaxiom:setting-category/controls";
    vec![
        row(
            "latticeaxiom:setting/input/gamepad-look-x",
            owner,
            category,
            200,
            integer(1, 200, 1),
            json!(100),
            SettingScope::User,
            SettingAuthority::LocalUser,
            RuntimeApplyImpact::Immediate,
        ),
        row(
            "latticeaxiom:setting/input/gamepad-look-y",
            owner,
            category,
            210,
            integer(1, 200, 1),
            json!(100),
            SettingScope::User,
            SettingAuthority::LocalUser,
            RuntimeApplyImpact::Immediate,
        ),
        row(
            "latticeaxiom:setting/input/gamepad-invert-x",
            owner,
            category,
            220,
            ValueType::Bool,
            json!(false),
            SettingScope::User,
            SettingAuthority::LocalUser,
            RuntimeApplyImpact::Immediate,
        ),
        row(
            "latticeaxiom:setting/input/gamepad-invert-y",
            owner,
            category,
            230,
            ValueType::Bool,
            json!(false),
            SettingScope::User,
            SettingAuthority::LocalUser,
            RuntimeApplyImpact::Immediate,
        ),
        row(
            "latticeaxiom:setting/input/gamepad-deadzone",
            owner,
            category,
            240,
            integer(0, 50, 1),
            json!(10),
            SettingScope::User,
            SettingAuthority::LocalUser,
            RuntimeApplyImpact::Immediate,
        ),
        row(
            "latticeaxiom:setting/input/gamepad-response-curve",
            owner,
            category,
            250,
            enumeration(&["linear", "relaxed", "precise"]),
            json!("linear"),
            SettingScope::User,
            SettingAuthority::LocalUser,
            RuntimeApplyImpact::Immediate,
        ),
    ]
}

fn inspect_rows() -> Vec<SettingSpec> {
    vec![
        row(
            "latticeaxiom:setting/interface/inspect-visible",
            "@latticeaxiom/inspect",
            "latticeaxiom:setting-category/interface",
            50,
            ValueType::Bool,
            json!(true),
            SettingScope::User,
            SettingAuthority::LocalUser,
            RuntimeApplyImpact::Immediate,
        ),
        row(
            "latticeaxiom:setting/interface/inspect-detail",
            "@latticeaxiom/inspect",
            "latticeaxiom:setting-category/interface",
            60,
            enumeration(&["compact", "standard", "verbose"]),
            json!("compact"),
            SettingScope::User,
            SettingAuthority::LocalUser,
            RuntimeApplyImpact::Immediate,
        ),
        row(
            "latticeaxiom:setting/interface/inspect-pin-mode",
            "@latticeaxiom/inspect",
            "latticeaxiom:setting-category/interface",
            70,
            enumeration(&["hold", "toggle"]),
            json!("hold"),
            SettingScope::User,
            SettingAuthority::LocalUser,
            RuntimeApplyImpact::Immediate,
        ),
    ]
}

fn front_end_rows() -> Vec<SettingSpec> {
    vec![row(
        "latticeaxiom:setting/gameplay/pause-on-focus-loss",
        "@latticeaxiom/front-end",
        "latticeaxiom:setting-category/gameplay",
        10,
        ValueType::Bool,
        json!(true),
        SettingScope::User,
        SettingAuthority::LocalUser,
        RuntimeApplyImpact::Immediate,
    )]
}

fn world_library_rows() -> Vec<SettingSpec> {
    let mut default_root = device_row(
        "latticeaxiom:setting/world/default-root",
        "@latticeaxiom/world-library",
        "latticeaxiom:setting-category/world",
        10,
        ValueType::String {
            min_length: Some(1),
            max_length: Some(512),
        },
        json!("platform"),
        RuntimeApplyImpact::ProcessRestart,
    );
    default_root.sensitivity = SettingSensitivity::PrivatePath;
    vec![
        default_root,
        world_owner_row(
            "latticeaxiom:setting/world/autosave-interval",
            "@latticeaxiom/world-library",
            20,
            integer(1, 30, 1),
            json!(5),
        ),
        row(
            "latticeaxiom:setting/world/backup-before-migration",
            "@latticeaxiom/world-library",
            "latticeaxiom:setting-category/world",
            30,
            ValueType::Bool,
            json!(true),
            SettingScope::User,
            SettingAuthority::LocalUser,
            RuntimeApplyImpact::Immediate,
        ),
        row(
            "latticeaxiom:setting/world/trash-retention-days",
            "@latticeaxiom/world-library",
            "latticeaxiom:setting-category/world",
            40,
            integer(1, 90, 1),
            json!(30),
            SettingScope::User,
            SettingAuthority::LocalUser,
            RuntimeApplyImpact::Immediate,
        ),
        row(
            "latticeaxiom:setting/world/confirm-destructive-actions",
            "@latticeaxiom/world-library",
            "latticeaxiom:setting-category/world",
            50,
            ValueType::Bool,
            json!(true),
            SettingScope::User,
            SettingAuthority::LocalUser,
            RuntimeApplyImpact::Immediate,
        ),
    ]
}

fn developer_rows() -> Vec<SettingSpec> {
    let owner = "@latticeaxiom/dev-tools";
    let category = "latticeaxiom:setting-category/developer";
    vec![
        row(
            "latticeaxiom:setting/developer/overlay-enabled",
            owner,
            category,
            10,
            ValueType::Bool,
            json!(false),
            SettingScope::User,
            SettingAuthority::LocalUser,
            RuntimeApplyImpact::Immediate,
        ),
        row(
            "latticeaxiom:setting/developer/overlay-detail",
            owner,
            category,
            20,
            enumeration(&["minimal", "normal", "verbose"]),
            json!("normal"),
            SettingScope::User,
            SettingAuthority::LocalUser,
            RuntimeApplyImpact::Immediate,
        ),
        row(
            "latticeaxiom:setting/developer/sample-period-ms",
            owner,
            category,
            30,
            integer(100, 5000, 100),
            json!(500),
            SettingScope::User,
            SettingAuthority::LocalUser,
            RuntimeApplyImpact::Immediate,
        ),
        row(
            "latticeaxiom:setting/developer/history-seconds",
            owner,
            category,
            40,
            integer(5, 300, 1),
            json!(30),
            SettingScope::User,
            SettingAuthority::LocalUser,
            RuntimeApplyImpact::Immediate,
        ),
        row(
            "latticeaxiom:setting/developer/visualizer-radius",
            owner,
            category,
            50,
            integer(1, 32, 1),
            json!(8),
            SettingScope::User,
            SettingAuthority::LocalUser,
            RuntimeApplyImpact::Immediate,
        ),
        row(
            "latticeaxiom:setting/developer/visualizer-depth-mode",
            owner,
            category,
            60,
            enumeration(&["depth-tested", "x-ray"]),
            json!("depth-tested"),
            SettingScope::User,
            SettingAuthority::LocalUser,
            RuntimeApplyImpact::Immediate,
        ),
        row(
            "latticeaxiom:setting/developer/visualizer-labels",
            owner,
            category,
            70,
            ValueType::Bool,
            json!(true),
            SettingScope::User,
            SettingAuthority::LocalUser,
            RuntimeApplyImpact::Immediate,
        ),
        row(
            "latticeaxiom:setting/developer/log-level",
            owner,
            category,
            80,
            enumeration(&["error", "warn", "info", "debug", "trace"]),
            json!("info"),
            SettingScope::User,
            SettingAuthority::LocalUser,
            RuntimeApplyImpact::Immediate,
        ),
    ]
}

fn integer(min: i64, max: i64, step: u64) -> ValueType {
    ValueType::Integer {
        min: Some(min),
        max: Some(max),
        step: Some(step),
    }
}

fn enumeration(values: &[&str]) -> ValueType {
    ValueType::Enum {
        values: values.iter().map(|value| (*value).to_owned()).collect(),
    }
}

#[allow(
    clippy::too_many_arguments,
    reason = "baseline rows are a closed fixture table, not a public API"
)]
fn row(
    id: &str,
    owner: &str,
    category: &str,
    order: i32,
    value_type: ValueType,
    default: Value,
    scope: SettingScope,
    authority: SettingAuthority,
    apply_impact: RuntimeApplyImpact,
) -> SettingSpec {
    SettingSpec {
        id: setting_id(id),
        declared_by: package(owner),
        schema_version: 1,
        value_type,
        default,
        allowed_scopes: BTreeSet::from([scope]),
        default_scope: scope,
        authority,
        apply_impact,
        category: setting_id(category),
        order,
        label_key: format!("{id}.label"),
        description_key: format!("{id}.description"),
        visibility: None,
        enabled_when: None,
        sensitivity: SettingSensitivity::Ordinary,
        replacement: None,
    }
}

fn device_row(
    id: &str,
    owner: &str,
    category: &str,
    order: i32,
    value_type: ValueType,
    default: Value,
    apply_impact: RuntimeApplyImpact,
) -> SettingSpec {
    row(
        id,
        owner,
        category,
        order,
        value_type,
        default,
        SettingScope::Device,
        SettingAuthority::LocalUser,
        apply_impact,
    )
}

fn world_owner_row(
    id: &str,
    owner: &str,
    order: i32,
    value_type: ValueType,
    default: Value,
) -> SettingSpec {
    row(
        id,
        owner,
        "latticeaxiom:setting-category/world",
        order,
        value_type,
        default,
        SettingScope::World,
        SettingAuthority::WorldOwner,
        RuntimeApplyImpact::Immediate,
    )
}

fn visible_when(mut spec: SettingSpec, setting: &str, value: Value) -> SettingSpec {
    spec.visibility = Some(SettingPredicate::Equals {
        setting: setting_id(setting),
        value,
    });
    spec
}

fn package(value: &str) -> PackageName {
    match value.parse() {
        Ok(name) => name,
        Err(error) => panic!("{value} is a compile-time-valid package name: {error}"),
    }
}

fn setting_id(value: &str) -> StableId {
    match value.parse() {
        Ok(id) => id,
        Err(error) => panic!("{value} is a compile-time-valid stable ID: {error}"),
    }
}
