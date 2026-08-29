//! First-consumer user settings owned by `@latticeaxiom/settings`.

use std::collections::BTreeSet;

use latticeaxiom_compose::{
    RuntimeApplyImpact, SettingAuthority, SettingScope, SettingSensitivity, SettingSpec, ValueType,
};
use latticeaxiom_core::{PackageName, StableId};
use serde_json::{Number, Value};

use super::binding::BindingProfileV1;
use super::catalog::{SettingsCatalogError, ValidatedSettingsCatalog};
use super::overlay::EffectiveSettingsSnapshot;

/// Logical package that must own the graph-selected settings registry.
pub const SETTINGS_PACKAGE_NAME: &str = "@latticeaxiom/settings";

/// Stable ID for the user UI-scale setting.
pub const UI_SCALE_SETTING_ID: &str = "latticeaxiom:setting/ui-scale";

/// Stable ID for the user view-distance setting.
pub const VIEW_DISTANCE_SETTING_ID: &str = "latticeaxiom:setting/view-distance";

/// Stable ID for vertical synchronization.
pub const VIDEO_VSYNC_SETTING_ID: &str = "latticeaxiom:setting/video/vsync";

/// Stable ID for the focused-window frame cap.
pub const VIDEO_FRAME_RATE_LIMIT_SETTING_ID: &str = "latticeaxiom:setting/video/frame-rate-limit";

/// Stable ID for the unfocused-window frame cap.
pub const VIDEO_BACKGROUND_FRAME_RATE_LIMIT_SETTING_ID: &str =
    "latticeaxiom:setting/video/background-frame-rate-limit";

/// Stable ID for the authoritative simulation frequency request.
pub const SIMULATION_TICK_RATE_SETTING_ID: &str =
    "latticeaxiom:setting/gameplay/simulation-tick-rate";

/// Accessibility category for UI scale.
pub const ACCESSIBILITY_SETTING_CATEGORY_ID: &str = "latticeaxiom:setting-category/accessibility";

/// Video category for view distance.
pub const VIDEO_SETTING_CATEGORY_ID: &str = "latticeaxiom:setting-category/video";

/// Gameplay category for simulation timing.
pub const GAMEPLAY_SETTING_CATEGORY_ID: &str = "latticeaxiom:setting-category/gameplay";

/// Authored default UI scale.
pub const DEFAULT_UI_SCALE: &str = "1.0";

/// Inclusive authored maximum UI scale.
pub const MAX_UI_SCALE: &str = "2.0";
/// Inclusive authored minimum view-distance request, in chunks.
pub const MIN_VIEW_DISTANCE_CHUNKS: u32 = 2;

/// Authored default view-distance request, in chunks.
pub const DEFAULT_VIEW_DISTANCE_CHUNKS: u32 = 8;

/// Authored maximum view-distance request before host clamping, in chunks.
pub const AUTHORED_MAX_VIEW_DISTANCE_CHUNKS: u32 = 32;

/// Default authoritative simulation frequency in hertz.
pub const DEFAULT_SIMULATION_TICK_RATE_HZ: u16 = 60;

/// User-facing projection of the first-consumer settings.
#[derive(Clone, Debug, PartialEq)]
pub struct UserSettingsProjection {
    ui_scale: Number,
    requested_view_distance: u32,
    clamped_view_distance: u32,
    binding_profile: BindingProfileV1,
}

impl UserSettingsProjection {
    /// Returns the effective UI scale.
    #[must_use]
    pub const fn ui_scale(&self) -> &Number {
        &self.ui_scale
    }

    /// Returns the stored requested view distance, before host clamping.
    #[must_use]
    pub const fn requested_view_distance(&self) -> u32 {
        self.requested_view_distance
    }

    /// Returns the host-clamped view distance used by streaming.
    #[must_use]
    pub const fn clamped_view_distance(&self) -> u32 {
        self.clamped_view_distance
    }

    /// Returns the persisted user binding profile.
    #[must_use]
    pub const fn binding_profile(&self) -> &BindingProfileV1 {
        &self.binding_profile
    }
}

/// Clamps a requested view-distance to the authored minimum and host maximum.
#[must_use]
pub fn clamp_view_distance_chunks(requested: i64, hard_max_chunks: u32) -> u32 {
    let hard_max = hard_max_chunks.max(MIN_VIEW_DISTANCE_CHUNKS);
    if requested < i64::from(MIN_VIEW_DISTANCE_CHUNKS) {
        MIN_VIEW_DISTANCE_CHUNKS
    } else if requested > i64::from(hard_max) {
        hard_max
    } else {
        match u32::try_from(requested) {
            Ok(value) => value,
            Err(_) => hard_max,
        }
    }
}

/// Returns the package that must own foundation setting IDs.
///
/// # Panics
///
/// Panics only if the compile-time package name is no longer a valid
/// [`PackageName`]. That is a programmer error in this crate.
#[must_use]
pub fn settings_package_name() -> PackageName {
    parse_package(SETTINGS_PACKAGE_NAME)
}

/// Returns the UI-scale setting ID.
///
/// # Panics
///
/// Panics only if [`UI_SCALE_SETTING_ID`] is no longer a valid [`StableId`].
#[must_use]
pub fn ui_scale_setting_id() -> StableId {
    parse_id(UI_SCALE_SETTING_ID)
}

/// Returns the view-distance setting ID.
///
/// # Panics
///
/// Panics only if [`VIEW_DISTANCE_SETTING_ID`] is no longer a valid [`StableId`].
#[must_use]
pub fn view_distance_setting_id() -> StableId {
    parse_id(VIEW_DISTANCE_SETTING_ID)
}

/// Returns the vertical-synchronization setting ID.
#[must_use]
pub fn video_vsync_setting_id() -> StableId {
    parse_id(VIDEO_VSYNC_SETTING_ID)
}

/// Returns the focused-window frame-limit setting ID.
#[must_use]
pub fn video_frame_rate_limit_setting_id() -> StableId {
    parse_id(VIDEO_FRAME_RATE_LIMIT_SETTING_ID)
}

/// Returns the unfocused-window frame-limit setting ID.
#[must_use]
pub fn video_background_frame_rate_limit_setting_id() -> StableId {
    parse_id(VIDEO_BACKGROUND_FRAME_RATE_LIMIT_SETTING_ID)
}

/// Returns the simulation-frequency setting ID.
#[must_use]
pub fn simulation_tick_rate_setting_id() -> StableId {
    parse_id(SIMULATION_TICK_RATE_SETTING_ID)
}

/// Returns the first-consumer declarations owned by the settings package.
#[must_use]
pub fn foundation_setting_specs() -> Vec<SettingSpec> {
    vec![
        ui_scale_spec(),
        view_distance_spec(),
        video_vsync_spec(),
        video_frame_rate_limit_spec(),
        video_background_frame_rate_limit_spec(),
        simulation_tick_rate_spec(),
    ]
}

/// Projects first-consumer values from an effective snapshot and user profile.
///
/// Missing rows fall back to authored defaults. View distance is clamped by
/// `hard_max_chunks` without rewriting the stored request.
///
/// # Errors
///
/// Returns [`SettingsCatalogError`] when a present value has the wrong JSON type.
pub fn project_user_settings(
    snapshot: &EffectiveSettingsSnapshot,
    binding_profile: BindingProfileV1,
    hard_max_chunks: u32,
) -> Result<UserSettingsProjection, SettingsCatalogError> {
    let ui_scale = match snapshot.values().get(&ui_scale_setting_id()) {
        Some(row) => number_value(&row.value, UI_SCALE_SETTING_ID)?,
        None => number_literal(DEFAULT_UI_SCALE),
    };
    let requested = match snapshot.values().get(&view_distance_setting_id()) {
        Some(row) => integer_value(&row.value, VIEW_DISTANCE_SETTING_ID)?,
        None => i64::from(DEFAULT_VIEW_DISTANCE_CHUNKS),
    };
    let requested_view_distance =
        clamp_view_distance_chunks(requested, AUTHORED_MAX_VIEW_DISTANCE_CHUNKS);
    Ok(UserSettingsProjection {
        ui_scale,
        requested_view_distance,
        clamped_view_distance: clamp_view_distance_chunks(
            i64::from(requested_view_distance),
            hard_max_chunks,
        ),
        binding_profile,
    })
}

/// Verifies that a compiled catalog owns the first-consumer rows exactly once.
///
/// # Errors
///
/// Returns [`SettingsCatalogError`] when a reserved ID is missing, has the
/// wrong owner, or has an unexpected value schema.
pub fn assert_foundation_catalog(
    catalog: &ValidatedSettingsCatalog,
) -> Result<(), SettingsCatalogError> {
    let runtime = &catalog.as_catalog().runtime;
    for expected in foundation_setting_specs() {
        let Some(actual) = runtime.get(&expected.id) else {
            return Err(SettingsCatalogError::InvalidCatalogDocument {
                reason: format!("missing foundation setting `{}`", expected.id),
            });
        };
        if actual.declared_by.as_str() != SETTINGS_PACKAGE_NAME {
            return Err(SettingsCatalogError::FoundationOwnerMismatch {
                setting: actual.id.clone(),
                declared_by: actual.declared_by.clone(),
            });
        }
        if actual.value_type != expected.value_type
            || actual.default != expected.default
            || actual.authority != expected.authority
            || actual.apply_impact != expected.apply_impact
        {
            return Err(SettingsCatalogError::InvalidCatalogDocument {
                reason: format!(
                    "foundation setting `{}` drifted from the owned contract",
                    actual.id
                ),
            });
        }
    }
    Ok(())
}

fn ui_scale_spec() -> SettingSpec {
    SettingSpec {
        id: ui_scale_setting_id(),
        declared_by: settings_package_name(),
        schema_version: 1,
        value_type: ValueType::Number {
            min: Some(number_literal(DEFAULT_UI_SCALE)),
            max: Some(number_literal(MAX_UI_SCALE)),
            step: Some(number_literal("1.0")),
        },
        default: Value::Number(number_literal(DEFAULT_UI_SCALE)),
        allowed_scopes: BTreeSet::from([SettingScope::User, SettingScope::Session]),
        default_scope: SettingScope::User,
        authority: SettingAuthority::LocalUser,
        apply_impact: RuntimeApplyImpact::Immediate,
        category: parse_id(ACCESSIBILITY_SETTING_CATEGORY_ID),
        order: 10,
        label_key: "latticeaxiom.settings.ui-scale.label".to_owned(),
        description_key: "latticeaxiom.settings.ui-scale.description".to_owned(),
        visibility: None,
        enabled_when: None,
        sensitivity: SettingSensitivity::Ordinary,
        replacement: None,
    }
}

fn view_distance_spec() -> SettingSpec {
    SettingSpec {
        id: view_distance_setting_id(),
        declared_by: settings_package_name(),
        schema_version: 1,
        value_type: ValueType::Integer {
            min: Some(i64::from(MIN_VIEW_DISTANCE_CHUNKS)),
            max: Some(i64::from(AUTHORED_MAX_VIEW_DISTANCE_CHUNKS)),
            step: Some(1),
        },
        default: Value::from(DEFAULT_VIEW_DISTANCE_CHUNKS),
        allowed_scopes: BTreeSet::from([SettingScope::User, SettingScope::Session]),
        default_scope: SettingScope::User,
        authority: SettingAuthority::LocalUser,
        apply_impact: RuntimeApplyImpact::Immediate,
        category: parse_id(VIDEO_SETTING_CATEGORY_ID),
        order: 20,
        label_key: "latticeaxiom.settings.view-distance.label".to_owned(),
        description_key: "latticeaxiom.settings.view-distance.description".to_owned(),
        visibility: None,
        enabled_when: None,
        sensitivity: SettingSensitivity::Ordinary,
        replacement: None,
    }
}

fn video_vsync_spec() -> SettingSpec {
    local_user_spec(
        video_vsync_setting_id(),
        ValueType::Bool,
        Value::Bool(false),
        VIDEO_SETTING_CATEGORY_ID,
        60,
    )
}

fn video_frame_rate_limit_spec() -> SettingSpec {
    local_user_spec(
        video_frame_rate_limit_setting_id(),
        ValueType::Enum {
            values: [
                "30",
                "60",
                "90",
                "120",
                "144",
                "165",
                "240",
                "300",
                "360",
                "unlimited",
            ]
            .into_iter()
            .map(str::to_owned)
            .collect(),
        },
        Value::String("unlimited".to_owned()),
        VIDEO_SETTING_CATEGORY_ID,
        70,
    )
}

fn video_background_frame_rate_limit_spec() -> SettingSpec {
    local_user_spec(
        video_background_frame_rate_limit_setting_id(),
        ValueType::Integer {
            min: Some(5),
            max: Some(60),
            step: Some(5),
        },
        Value::from(30),
        VIDEO_SETTING_CATEGORY_ID,
        80,
    )
}

fn simulation_tick_rate_spec() -> SettingSpec {
    local_user_spec(
        simulation_tick_rate_setting_id(),
        ValueType::Integer {
            min: Some(1),
            max: Some(10_000),
            step: Some(1),
        },
        Value::from(DEFAULT_SIMULATION_TICK_RATE_HZ),
        GAMEPLAY_SETTING_CATEGORY_ID,
        10,
    )
}

fn local_user_spec(
    id: StableId,
    value_type: ValueType,
    default: Value,
    category: &'static str,
    order: i32,
) -> SettingSpec {
    let label_key = format!("{id}.label");
    let description_key = format!("{id}.description");
    SettingSpec {
        id,
        declared_by: settings_package_name(),
        schema_version: 1,
        value_type,
        default,
        allowed_scopes: BTreeSet::from([SettingScope::User, SettingScope::Session]),
        default_scope: SettingScope::User,
        authority: SettingAuthority::LocalUser,
        apply_impact: RuntimeApplyImpact::Immediate,
        category: parse_id(category),
        order,
        label_key,
        description_key,
        visibility: None,
        enabled_when: None,
        sensitivity: SettingSensitivity::Ordinary,
        replacement: None,
    }
}

fn number_value(value: &Value, setting: &str) -> Result<Number, SettingsCatalogError> {
    value
        .as_number()
        .cloned()
        .ok_or_else(|| SettingsCatalogError::InvalidCatalogDocument {
            reason: format!("setting `{setting}` is not an exact finite decimal"),
        })
}

fn integer_value(value: &Value, setting: &str) -> Result<i64, SettingsCatalogError> {
    value
        .as_i64()
        .ok_or_else(|| SettingsCatalogError::InvalidCatalogDocument {
            reason: format!("setting `{setting}` is not a signed JSON integer"),
        })
}

fn number_literal(value: &'static str) -> Number {
    match value.parse() {
        Ok(number) => number,
        Err(error) => panic!("{value} is a compile-time-valid exact decimal: {error}"),
    }
}

fn parse_id(value: &'static str) -> StableId {
    match value.parse() {
        Ok(id) => id,
        Err(error) => panic!("{value} is a compile-time-valid stable ID: {error}"),
    }
}

fn parse_package(value: &'static str) -> PackageName {
    match value.parse() {
        Ok(name) => name,
        Err(error) => panic!("{value} is a compile-time-valid package name: {error}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn view_distance_clamps_zero_and_over_host_max() {
        assert_eq!(clamp_view_distance_chunks(0, 8), 2);
        assert_eq!(clamp_view_distance_chunks(1, 8), 2);
        assert_eq!(clamp_view_distance_chunks(8, 8), 8);
        assert_eq!(clamp_view_distance_chunks(32, 8), 8);
        assert_eq!(clamp_view_distance_chunks(4, 0), 2);
    }

    #[test]
    fn foundation_specs_use_the_settings_package_and_immediate_impact() {
        for spec in foundation_setting_specs() {
            assert_eq!(spec.declared_by.as_str(), SETTINGS_PACKAGE_NAME);
            assert_eq!(spec.apply_impact, RuntimeApplyImpact::Immediate);
            assert_eq!(spec.authority, SettingAuthority::LocalUser);
            assert!(spec.allowed_scopes.contains(&SettingScope::User));
        }
    }
}
