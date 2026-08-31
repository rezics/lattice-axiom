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

/// Stable ID for the render-distance setting.
///
/// The identifier intentionally retains the original `view-distance` spelling
/// so existing persisted user values migrate without rewriting their storage
/// lane.
pub const RENDER_DISTANCE_SETTING_ID: &str = "latticeaxiom:setting/view-distance";

/// Stable ID for the authoritative simulation-distance setting.
pub const SIMULATION_DISTANCE_SETTING_ID: &str =
    "latticeaxiom:setting/gameplay/simulation-distance";

/// Stable ID for the full-resolution near-terrain distance setting.
pub const FULL_DETAIL_DISTANCE_SETTING_ID: &str = "latticeaxiom:setting/video/full-detail-distance";

/// Stable ID for the distant-terrain geometric-error policy.
pub const DISTANT_TERRAIN_QUALITY_SETTING_ID: &str =
    "latticeaxiom:setting/video/distant-terrain-quality";

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

/// Video category for terrain presentation settings.
pub const VIDEO_SETTING_CATEGORY_ID: &str = "latticeaxiom:setting-category/video";

/// Gameplay category for simulation timing.
pub const GAMEPLAY_SETTING_CATEGORY_ID: &str = "latticeaxiom:setting-category/gameplay";

/// Authored default UI scale.
pub const DEFAULT_UI_SCALE: &str = "1.0";

/// Inclusive authored maximum UI scale.
pub const MAX_UI_SCALE: &str = "2.0";
/// Inclusive authored minimum render-distance request, in chunks.
pub const MIN_RENDER_DISTANCE_CHUNKS: u32 = 2;

/// Authored default render-distance request, in chunks.
pub const DEFAULT_RENDER_DISTANCE_CHUNKS: u32 = 8;

/// Authored maximum render-distance request before host admission, in chunks.
pub const AUTHORED_MAX_RENDER_DISTANCE_CHUNKS: u32 = 32;

/// Inclusive authored minimum simulation-distance request, in chunks.
pub const MIN_SIMULATION_DISTANCE_CHUNKS: u32 = 2;

/// Authored default simulation-distance request, in chunks.
pub const DEFAULT_SIMULATION_DISTANCE_CHUNKS: u32 = 4;

/// Authored maximum simulation-distance request before host admission, in chunks.
pub const AUTHORED_MAX_SIMULATION_DISTANCE_CHUNKS: u32 = 32;

/// Inclusive authored minimum full-detail distance, in chunks.
pub const MIN_FULL_DETAIL_DISTANCE_CHUNKS: u32 = 2;

/// Authored default full-detail distance, in chunks.
pub const DEFAULT_FULL_DETAIL_DISTANCE_CHUNKS: u32 = 6;

/// Currently certified maximum full-detail distance, in chunks.
pub const AUTHORED_MAX_FULL_DETAIL_DISTANCE_CHUNKS: u32 = 6;

/// Authored default distant-terrain quality value.
pub const DEFAULT_DISTANT_TERRAIN_QUALITY: &str = "balanced";

/// Default authoritative simulation frequency in hertz.
pub const DEFAULT_SIMULATION_TICK_RATE_HZ: u16 = 60;

macro_rules! chunk_distance_type {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
        pub struct $name(u32);

        impl $name {
            /// Creates a non-zero chunk distance.
            #[must_use]
            pub const fn new(chunks: u32) -> Option<Self> {
                if chunks == 0 { None } else { Some(Self(chunks)) }
            }

            /// Returns the horizontal radius in chunks.
            #[must_use]
            pub const fn chunks(self) -> u32 {
                self.0
            }
        }
    };
}

chunk_distance_type!(
    /// Validated user render-distance request before host admission.
    RequestedRenderDistanceChunksV1
);
chunk_distance_type!(
    /// Render-distance target admitted by the active world and host profile.
    TargetRenderDistanceChunksV1
);
chunk_distance_type!(
    /// Validated user full-detail-distance request before host admission.
    RequestedFullDetailDistanceChunksV1
);
chunk_distance_type!(
    /// Radius retaining complete authoritative voxel chunks and near geometry.
    FullDetailDistanceChunksV1
);
chunk_distance_type!(
    /// Validated user simulation-distance request before host admission.
    RequestedSimulationDistanceChunksV1
);
chunk_distance_type!(
    /// Radius in which authoritative gameplay activation and ticking may occur.
    SimulationDistanceChunksV1
);
chunk_distance_type!(
    /// Largest contiguous near-plus-far radius ready for presentation.
    PresentedRenderDistanceChunksV1
);

impl Default for RequestedRenderDistanceChunksV1 {
    fn default() -> Self {
        Self(DEFAULT_RENDER_DISTANCE_CHUNKS)
    }
}

impl Default for TargetRenderDistanceChunksV1 {
    fn default() -> Self {
        Self(DEFAULT_RENDER_DISTANCE_CHUNKS)
    }
}

impl Default for RequestedFullDetailDistanceChunksV1 {
    fn default() -> Self {
        Self(DEFAULT_FULL_DETAIL_DISTANCE_CHUNKS)
    }
}

impl Default for FullDetailDistanceChunksV1 {
    fn default() -> Self {
        Self(DEFAULT_FULL_DETAIL_DISTANCE_CHUNKS)
    }
}

impl Default for RequestedSimulationDistanceChunksV1 {
    fn default() -> Self {
        Self(DEFAULT_SIMULATION_DISTANCE_CHUNKS)
    }
}

impl Default for SimulationDistanceChunksV1 {
    fn default() -> Self {
        Self(DEFAULT_SIMULATION_DISTANCE_CHUNKS)
    }
}

impl Default for PresentedRenderDistanceChunksV1 {
    fn default() -> Self {
        Self(DEFAULT_FULL_DETAIL_DISTANCE_CHUNKS)
    }
}

/// Validated user requests for the three independent terrain-distance domains.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct TerrainDistanceRequestsV1 {
    render: RequestedRenderDistanceChunksV1,
    simulation: RequestedSimulationDistanceChunksV1,
    full_detail: RequestedFullDetailDistanceChunksV1,
}

impl TerrainDistanceRequestsV1 {
    /// Creates one request set from values already validated by their setting domains.
    #[must_use]
    pub const fn new(
        render: RequestedRenderDistanceChunksV1,
        simulation: RequestedSimulationDistanceChunksV1,
        full_detail: RequestedFullDetailDistanceChunksV1,
    ) -> Self {
        Self {
            render,
            simulation,
            full_detail,
        }
    }

    /// Returns the total terrain-horizon request.
    #[must_use]
    pub const fn render(self) -> RequestedRenderDistanceChunksV1 {
        self.render
    }

    /// Returns the authoritative gameplay-radius request.
    #[must_use]
    pub const fn simulation(self) -> RequestedSimulationDistanceChunksV1 {
        self.simulation
    }

    /// Returns the complete voxel-terrain-radius request.
    #[must_use]
    pub const fn full_detail(self) -> RequestedFullDetailDistanceChunksV1 {
        self.full_detail
    }
}

/// Distant-terrain geometric-error policy, independent from its radius.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum FarTerrainQualityV1 {
    /// Prefer fewer and coarser far tiles.
    Performance,
    /// Balance distant silhouette fidelity and work.
    #[default]
    Balanced,
    /// Prefer lower geometric error at greater work and memory cost.
    Quality,
}

impl FarTerrainQualityV1 {
    /// Parses a catalog value.
    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "performance" => Some(Self::Performance),
            "balanced" => Some(Self::Balanced),
            "quality" => Some(Self::Quality),
            _ => None,
        }
    }

    /// Returns the canonical catalog value.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Performance => "performance",
            Self::Balanced => "balanced",
            Self::Quality => "quality",
        }
    }
}

/// User-facing projection of the first-consumer settings.
#[derive(Clone, Debug, PartialEq)]
pub struct UserSettingsProjection {
    ui_scale: Number,
    requested_render_distance: RequestedRenderDistanceChunksV1,
    target_render_distance: TargetRenderDistanceChunksV1,
    requested_simulation_distance: RequestedSimulationDistanceChunksV1,
    requested_full_detail_distance: RequestedFullDetailDistanceChunksV1,
    far_terrain_quality: FarTerrainQualityV1,
    binding_profile: BindingProfileV1,
}

impl UserSettingsProjection {
    /// Returns the effective UI scale.
    #[must_use]
    pub const fn ui_scale(&self) -> &Number {
        &self.ui_scale
    }

    /// Returns the stored render-distance request, before host admission.
    #[must_use]
    pub const fn requested_render_distance(&self) -> RequestedRenderDistanceChunksV1 {
        self.requested_render_distance
    }

    /// Returns the host-admitted total terrain horizon.
    #[must_use]
    pub const fn target_render_distance(&self) -> TargetRenderDistanceChunksV1 {
        self.target_render_distance
    }

    /// Returns the requested authoritative simulation radius.
    #[must_use]
    pub const fn requested_simulation_distance(&self) -> RequestedSimulationDistanceChunksV1 {
        self.requested_simulation_distance
    }

    /// Returns the requested full-resolution terrain radius.
    #[must_use]
    pub const fn requested_full_detail_distance(&self) -> RequestedFullDetailDistanceChunksV1 {
        self.requested_full_detail_distance
    }

    /// Returns the distant-terrain geometric-error policy.
    #[must_use]
    pub const fn far_terrain_quality(&self) -> FarTerrainQualityV1 {
        self.far_terrain_quality
    }

    /// Returns the persisted user binding profile.
    #[must_use]
    pub const fn binding_profile(&self) -> &BindingProfileV1 {
        &self.binding_profile
    }
}

/// Clamps a raw render-distance value to the authored request domain.
#[must_use]
pub fn clamp_requested_render_distance_chunks(requested: i64) -> RequestedRenderDistanceChunksV1 {
    RequestedRenderDistanceChunksV1(clamp_integer_distance(
        requested,
        MIN_RENDER_DISTANCE_CHUNKS,
        AUTHORED_MAX_RENDER_DISTANCE_CHUNKS,
    ))
}

/// Admits a validated render-distance request against a host limit.
#[must_use]
pub fn admit_render_distance_chunks(
    requested: RequestedRenderDistanceChunksV1,
    host_max_chunks: u32,
) -> TargetRenderDistanceChunksV1 {
    let host_max = host_max_chunks.clamp(
        MIN_RENDER_DISTANCE_CHUNKS,
        AUTHORED_MAX_RENDER_DISTANCE_CHUNKS,
    );
    TargetRenderDistanceChunksV1(requested.chunks().min(host_max))
}

/// Clamps a raw simulation-distance value to the authored request domain.
#[must_use]
pub fn clamp_simulation_distance_chunks(requested: i64) -> RequestedSimulationDistanceChunksV1 {
    RequestedSimulationDistanceChunksV1(clamp_integer_distance(
        requested,
        MIN_SIMULATION_DISTANCE_CHUNKS,
        AUTHORED_MAX_SIMULATION_DISTANCE_CHUNKS,
    ))
}

/// Clamps a raw full-detail value to the currently certified request domain.
#[must_use]
pub fn clamp_full_detail_distance_chunks(requested: i64) -> RequestedFullDetailDistanceChunksV1 {
    RequestedFullDetailDistanceChunksV1(clamp_integer_distance(
        requested,
        MIN_FULL_DETAIL_DISTANCE_CHUNKS,
        AUTHORED_MAX_FULL_DETAIL_DISTANCE_CHUNKS,
    ))
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

/// Returns the render-distance setting ID.
///
/// # Panics
///
/// Panics only if [`RENDER_DISTANCE_SETTING_ID`] is no longer a valid [`StableId`].
#[must_use]
pub fn render_distance_setting_id() -> StableId {
    parse_id(RENDER_DISTANCE_SETTING_ID)
}

/// Returns the simulation-distance setting ID.
#[must_use]
pub fn simulation_distance_setting_id() -> StableId {
    parse_id(SIMULATION_DISTANCE_SETTING_ID)
}

/// Returns the full-detail-distance setting ID.
#[must_use]
pub fn full_detail_distance_setting_id() -> StableId {
    parse_id(FULL_DETAIL_DISTANCE_SETTING_ID)
}

/// Returns the distant-terrain-quality setting ID.
#[must_use]
pub fn distant_terrain_quality_setting_id() -> StableId {
    parse_id(DISTANT_TERRAIN_QUALITY_SETTING_ID)
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
        render_distance_spec(),
        simulation_distance_spec(),
        full_detail_distance_spec(),
        distant_terrain_quality_spec(),
        video_vsync_spec(),
        video_frame_rate_limit_spec(),
        video_background_frame_rate_limit_spec(),
        simulation_tick_rate_spec(),
    ]
}

/// Projects first-consumer values from an effective snapshot and user profile.
///
/// Missing rows fall back to authored defaults. Render distance is admitted by
/// `host_max_render_chunks` without rewriting the stored request.
///
/// # Errors
///
/// Returns [`SettingsCatalogError`] when a present value has the wrong JSON type.
pub fn project_user_settings(
    snapshot: &EffectiveSettingsSnapshot,
    binding_profile: BindingProfileV1,
    host_max_render_chunks: u32,
) -> Result<UserSettingsProjection, SettingsCatalogError> {
    let ui_scale = match snapshot.values().get(&ui_scale_setting_id()) {
        Some(row) => number_value(&row.value, UI_SCALE_SETTING_ID)?,
        None => number_literal(DEFAULT_UI_SCALE),
    };
    let requested_render = match snapshot.values().get(&render_distance_setting_id()) {
        Some(row) => integer_value(&row.value, RENDER_DISTANCE_SETTING_ID)?,
        None => i64::from(DEFAULT_RENDER_DISTANCE_CHUNKS),
    };
    let requested_render_distance = clamp_requested_render_distance_chunks(requested_render);
    let requested_simulation_distance =
        match snapshot.values().get(&simulation_distance_setting_id()) {
            Some(row) => clamp_simulation_distance_chunks(integer_value(
                &row.value,
                SIMULATION_DISTANCE_SETTING_ID,
            )?),
            None => clamp_simulation_distance_chunks(i64::from(DEFAULT_SIMULATION_DISTANCE_CHUNKS)),
        };
    let requested_full_detail_distance = match snapshot
        .values()
        .get(&full_detail_distance_setting_id())
    {
        Some(row) => clamp_full_detail_distance_chunks(integer_value(
            &row.value,
            FULL_DETAIL_DISTANCE_SETTING_ID,
        )?),
        None => clamp_full_detail_distance_chunks(i64::from(DEFAULT_FULL_DETAIL_DISTANCE_CHUNKS)),
    };
    let far_terrain_quality = match snapshot.values().get(&distant_terrain_quality_setting_id()) {
        Some(row) => far_terrain_quality_value(&row.value)?,
        None => FarTerrainQualityV1::Balanced,
    };
    Ok(UserSettingsProjection {
        ui_scale,
        requested_render_distance,
        target_render_distance: admit_render_distance_chunks(
            requested_render_distance,
            host_max_render_chunks,
        ),
        requested_simulation_distance,
        requested_full_detail_distance,
        far_terrain_quality,
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

fn render_distance_spec() -> SettingSpec {
    SettingSpec {
        id: render_distance_setting_id(),
        declared_by: settings_package_name(),
        schema_version: 1,
        value_type: ValueType::Integer {
            min: Some(i64::from(MIN_RENDER_DISTANCE_CHUNKS)),
            max: Some(i64::from(AUTHORED_MAX_RENDER_DISTANCE_CHUNKS)),
            step: Some(1),
        },
        default: Value::from(DEFAULT_RENDER_DISTANCE_CHUNKS),
        allowed_scopes: BTreeSet::from([SettingScope::User, SettingScope::Session]),
        default_scope: SettingScope::User,
        authority: SettingAuthority::LocalUser,
        apply_impact: RuntimeApplyImpact::Immediate,
        category: parse_id(VIDEO_SETTING_CATEGORY_ID),
        order: 20,
        label_key: "latticeaxiom.settings.render-distance.label".to_owned(),
        description_key: "latticeaxiom.settings.render-distance.description".to_owned(),
        visibility: None,
        enabled_when: None,
        sensitivity: SettingSensitivity::Ordinary,
        replacement: None,
    }
}

fn simulation_distance_spec() -> SettingSpec {
    local_user_spec(
        simulation_distance_setting_id(),
        ValueType::Integer {
            min: Some(i64::from(MIN_SIMULATION_DISTANCE_CHUNKS)),
            max: Some(i64::from(AUTHORED_MAX_SIMULATION_DISTANCE_CHUNKS)),
            step: Some(1),
        },
        Value::from(DEFAULT_SIMULATION_DISTANCE_CHUNKS),
        GAMEPLAY_SETTING_CATEGORY_ID,
        5,
    )
}

fn full_detail_distance_spec() -> SettingSpec {
    local_user_spec(
        full_detail_distance_setting_id(),
        ValueType::Integer {
            min: Some(i64::from(MIN_FULL_DETAIL_DISTANCE_CHUNKS)),
            max: Some(i64::from(AUTHORED_MAX_FULL_DETAIL_DISTANCE_CHUNKS)),
            step: Some(1),
        },
        Value::from(DEFAULT_FULL_DETAIL_DISTANCE_CHUNKS),
        VIDEO_SETTING_CATEGORY_ID,
        30,
    )
}

fn distant_terrain_quality_spec() -> SettingSpec {
    local_user_spec(
        distant_terrain_quality_setting_id(),
        ValueType::Enum {
            values: ["performance", "balanced", "quality"]
                .into_iter()
                .map(str::to_owned)
                .collect(),
        },
        Value::String(DEFAULT_DISTANT_TERRAIN_QUALITY.to_owned()),
        VIDEO_SETTING_CATEGORY_ID,
        40,
    )
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

fn far_terrain_quality_value(value: &Value) -> Result<FarTerrainQualityV1, SettingsCatalogError> {
    value
        .as_str()
        .and_then(FarTerrainQualityV1::parse)
        .ok_or_else(|| SettingsCatalogError::InvalidCatalogDocument {
            reason: format!(
                "setting `{DISTANT_TERRAIN_QUALITY_SETTING_ID}` is not a supported quality value"
            ),
        })
}

fn clamp_integer_distance(requested: i64, authored_min: u32, authored_max: u32) -> u32 {
    if requested < i64::from(authored_min) {
        authored_min
    } else if requested > i64::from(authored_max) {
        authored_max
    } else {
        match u32::try_from(requested) {
            Ok(value) => value,
            Err(_) => authored_max,
        }
    }
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
    fn terrain_distance_types_clamp_independently() {
        assert_eq!(clamp_requested_render_distance_chunks(0).chunks(), 2);
        assert_eq!(clamp_requested_render_distance_chunks(32).chunks(), 32);
        assert_eq!(
            admit_render_distance_chunks(clamp_requested_render_distance_chunks(21), 8).chunks(),
            8
        );
        assert_eq!(clamp_simulation_distance_chunks(12).chunks(), 12);
        assert_eq!(clamp_full_detail_distance_chunks(12).chunks(), 6);
        assert_eq!(
            FarTerrainQualityV1::parse("balanced"),
            Some(FarTerrainQualityV1::Balanced)
        );
        assert_eq!(FarTerrainQualityV1::parse("ultra"), None);
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
