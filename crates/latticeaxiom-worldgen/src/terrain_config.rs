//! Resolved, bounded Worldgen V2 terrain presets.

use latticeaxiom_core::{CanonicalHash, StableId, canonical_json_bytes};
use serde::{Deserialize, Serialize};

use crate::{TerrainConfigHashV2, WorldgenConfigV1, WorldgenError, WorldgenResult};

const MAX_RATIO: u16 = 1_024;

/// Named world-creation terrain directions.
///
/// A preset is an authoring convenience. Generation consumes the fully
/// resolved [`TerrainConfigV2`] returned by [`Self::resolve`].
#[derive(
    Clone, Copy, Debug, Default, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize,
)]
#[serde(rename_all = "kebab-case")]
pub enum TerrainPresetV2 {
    /// Broadly varied continents, relief, water, and climates.
    #[default]
    Balanced,
    /// Large landmasses with long rivers and inland mountain chains.
    Continental,
    /// Ocean-dominant island chains with deeper basins.
    Archipelago,
    /// High relief dominated by continuous mountain systems.
    Alpine,
    /// Strongly eroded low relief, broad valleys, lakes, and wetlands.
    Eroded,
    /// Extreme vertical relief and rare unusual landforms.
    Wild,
}

impl TerrainPresetV2 {
    /// Built-in presets in stable user-facing order.
    pub const ALL: [Self; 6] = [
        Self::Balanced,
        Self::Continental,
        Self::Archipelago,
        Self::Alpine,
        Self::Eroded,
        Self::Wild,
    ];

    /// Returns the stable persisted preset identifier.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Balanced => "balanced",
            Self::Continental => "continental",
            Self::Archipelago => "archipelago",
            Self::Alpine => "alpine",
            Self::Eroded => "eroded",
            Self::Wild => "wild",
        }
    }

    /// Returns the stable profile identity persisted by world creation.
    #[must_use]
    pub const fn profile_id_str(self) -> &'static str {
        match self {
            Self::Balanced => "latticeaxiom:worldgen-profile/balanced@2",
            Self::Continental => "latticeaxiom:worldgen-profile/continental@2",
            Self::Archipelago => "latticeaxiom:worldgen-profile/archipelago@2",
            Self::Alpine => "latticeaxiom:worldgen-profile/alpine@2",
            Self::Eroded => "latticeaxiom:worldgen-profile/eroded@2",
            Self::Wild => "latticeaxiom:worldgen-profile/wild@2",
        }
    }

    /// Resolves a stable creation-profile identity to a built-in preset.
    #[must_use]
    pub fn from_profile_id(profile: &StableId) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|preset| preset.profile_id_str() == profile.as_str())
    }

    /// Resolves this preset into every output-affecting terrain parameter.
    #[must_use]
    pub const fn resolve(self) -> TerrainConfigV2 {
        match self {
            Self::Balanced => TerrainConfigV2::balanced(),
            Self::Continental => TerrainConfigV2::continental(),
            Self::Archipelago => TerrainConfigV2::archipelago(),
            Self::Alpine => TerrainConfigV2::alpine(),
            Self::Eroded => TerrainConfigV2::eroded(),
            Self::Wild => TerrainConfigV2::wild(),
        }
    }
}

/// Immutable vertical world contract resolved at creation time.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WorldBoundsV2 {
    /// Inclusive lowest materializable Y.
    pub floor_y: i32,
    /// Inclusive highest materializable Y.
    pub ceiling_y: i32,
    /// Global surface-water datum.
    pub sea_level_y: i32,
}

/// Large-scale continent, coast, island, and ocean controls.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct LandmassConfigV2 {
    /// Approximate wavelength of primary continental fields.
    pub continent_scale_voxels: u32,
    /// Signed land threshold; larger values produce more ocean.
    pub ocean_bias_per_1024: i16,
    /// Width of the smooth coast transition in fixed-point field units.
    pub coast_width_per_1024: u16,
    /// Contribution of secondary island fields.
    pub island_weight_per_1024: u16,
    /// Approximate wavelength of secondary islands.
    pub island_scale_voxels: u32,
    /// Maximum depth of deep ocean below sea level.
    pub ocean_depth_voxels: u16,
}

/// Plains, hills, plateaus, mountains, erosion, and detail controls.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ReliefConfigV2 {
    /// Nominal inland surface above sea level.
    pub base_height_voxels: u16,
    /// Additional lift from continental interior.
    pub continental_lift_voxels: u16,
    /// Maximum rolling-hill displacement.
    pub hill_height_voxels: u16,
    /// Maximum mountain-chain displacement.
    pub mountain_height_voxels: u16,
    /// Approximate wavelength of mountain chains.
    pub mountain_scale_voxels: u32,
    /// Mountain coverage in `0..=1024` units.
    pub mountain_amount_per_1024: u16,
    /// Plateau coverage in `0..=1024` units.
    pub plateau_amount_per_1024: u16,
    /// Plateau lift above surrounding terrain.
    pub plateau_height_voxels: u16,
    /// Erosion attenuation in `0..=1024` units.
    pub erosion_strength_per_1024: u16,
    /// Local detail amplitude in `0..=1024` units.
    pub roughness_per_1024: u16,
    /// Rare volcanic uplift coverage in `0..=1024` units.
    pub volcano_amount_per_1024: u16,
}

/// Surface drainage controls shared by terrain and occupancy generation.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SurfaceWaterConfigV2 {
    /// Approximate spacing between primary river corridors.
    pub river_spacing_voxels: u16,
    /// Half-width of primary river beds.
    pub river_width_voxels: u16,
    /// River-bed incision below the uncarved surface.
    pub river_depth_voxels: u16,
    /// Lake occurrence in `0..=1024` units.
    pub lake_amount_per_1024: u16,
    /// Wetland occurrence in flat lowlands in `0..=1024` units.
    pub wetland_amount_per_1024: u16,
    /// Whether surface rivers may continue through mountains underground.
    pub underground_rivers: bool,
}

/// Climate-field controls kept independent from terrain shape.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ClimateConfigV2 {
    /// Approximate wavelength of temperature and humidity fields.
    pub scale_voxels: u32,
    /// Temperature variation in `0..=1024` units.
    pub temperature_variance_per_1024: u16,
    /// Humidity variation in `0..=1024` units.
    pub humidity_variance_per_1024: u16,
    /// Width of climate material transitions in voxels.
    pub transition_width_voxels: u16,
    /// Strength of altitude cooling in `0..=1024` units.
    pub altitude_cooling_per_1024: u16,
}

/// Cave, aquifer, and lava controls resolved with the surface preset.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct UndergroundConfigV2 {
    /// Cave occurrence in `0..=1024` units.
    pub cave_amount_per_1024: u16,
    /// Coarse three-dimensional cave field cell edge.
    pub cave_scale_voxels: u16,
    /// Aquifer occurrence in `0..=1024` units.
    pub aquifer_amount_per_1024: u16,
    /// Aquifer table depth below the surface.
    pub aquifer_depth_voxels: u16,
    /// Height of the lava-bearing layer above the world floor.
    pub lava_depth_voxels: u16,
}

/// Fully resolved, integer-only Worldgen V2 terrain configuration.
///
/// This record, rather than the display preset alone, is hashed into the
/// generation contract and persisted by generation receipts.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TerrainConfigV2 {
    /// Vertical world bounds and sea datum.
    pub world: WorldBoundsV2,
    /// Continental and ocean controls.
    pub landmass: LandmassConfigV2,
    /// Surface-relief controls.
    pub relief: ReliefConfigV2,
    /// Rivers, lakes, and wetlands.
    pub water: SurfaceWaterConfigV2,
    /// Climate controls independent from shape.
    pub climate: ClimateConfigV2,
    /// Underground controls.
    pub underground: UndergroundConfigV2,
}

impl TerrainConfigV2 {
    /// Returns the standard 512-voxel balanced profile.
    #[must_use]
    pub const fn balanced() -> Self {
        Self::new(
            8_192, -96, 192, 224, 1_536, 96, 22, 54, 28, 176, 2_048, 330, 210, 36, 520, 260, 18,
            384, 9, 6, 220, 180, true, 4_096, 820, 820, 48, 500, 150, 24, 360, 30, 18,
        )
    }

    /// Returns a large-continent profile with long inland terrain systems.
    #[must_use]
    pub const fn continental() -> Self {
        Self::new(
            13_312, -230, 160, 96, 2_048, 88, 26, 66, 30, 184, 2_560, 360, 250, 42, 450, 230, 14,
            512, 10, 7, 180, 130, true, 5_120, 760, 780, 56, 540, 142, 28, 330, 34, 18,
        )
    }

    /// Returns an ocean-dominant island-chain profile.
    #[must_use]
    pub const fn archipelago() -> Self {
        Self::new(
            5_120, 190, 256, 620, 1_024, 144, 16, 36, 24, 126, 1_536, 270, 130, 28, 590, 300, 28,
            256, 8, 5, 350, 100, true, 3_072, 900, 940, 36, 420, 155, 22, 420, 28, 16,
        )
    }

    /// Returns a mountain-chain and deep-valley profile.
    #[must_use]
    pub const fn alpine() -> Self {
        Self::new(
            8_192, -120, 176, 150, 1_536, 104, 28, 62, 34, 248, 2_304, 540, 180, 30, 360, 320, 20,
            384, 8, 8, 120, 90, true, 3_584, 860, 720, 42, 720, 175, 30, 300, 24, 20,
        )
    }

    /// Returns an old, strongly eroded landscape with broad wetlands.
    #[must_use]
    pub const fn eroded() -> Self {
        Self::new(
            9_216, -140, 220, 180, 1_792, 78, 18, 42, 18, 92, 2_816, 230, 310, 24, 830, 120, 8,
            448, 12, 4, 420, 520, true, 5_120, 700, 920, 72, 360, 118, 30, 470, 38, 14,
        )
    }

    /// Returns a 1024-voxel extreme profile with uncommon high relief.
    #[must_use]
    pub const fn wild() -> Self {
        let mut config = Self::new(
            7_168, -100, 160, 260, 1_280, 176, 32, 86, 48, 360, 1_536, 610, 360, 74, 280, 580, 72,
            320, 8, 9, 160, 100, true, 3_072, 920, 900, 32, 800, 220, 20, 500, 44, 26,
        );
        config.world = WorldBoundsV2 {
            floor_y: -256,
            ceiling_y: 767,
            sea_level_y: 64,
        };
        config
    }

    #[allow(
        clippy::too_many_arguments,
        reason = "preset rows keep resolved values auditable"
    )]
    const fn new(
        continent_scale_voxels: u32,
        ocean_bias_per_1024: i16,
        coast_width_per_1024: u16,
        island_weight_per_1024: u16,
        island_scale_voxels: u32,
        ocean_depth_voxels: u16,
        base_height_voxels: u16,
        continental_lift_voxels: u16,
        hill_height_voxels: u16,
        mountain_height_voxels: u16,
        mountain_scale_voxels: u32,
        mountain_amount_per_1024: u16,
        plateau_amount_per_1024: u16,
        plateau_height_voxels: u16,
        erosion_strength_per_1024: u16,
        roughness_per_1024: u16,
        volcano_amount_per_1024: u16,
        river_spacing_voxels: u16,
        river_width_voxels: u16,
        river_depth_voxels: u16,
        lake_amount_per_1024: u16,
        wetland_amount_per_1024: u16,
        underground_rivers: bool,
        climate_scale_voxels: u32,
        temperature_variance_per_1024: u16,
        humidity_variance_per_1024: u16,
        climate_transition_width_voxels: u16,
        altitude_cooling_per_1024: u16,
        cave_amount_per_1024: u16,
        cave_scale_voxels: u16,
        aquifer_amount_per_1024: u16,
        aquifer_depth_voxels: u16,
        lava_depth_voxels: u16,
    ) -> Self {
        Self {
            world: WorldBoundsV2 {
                floor_y: -128,
                ceiling_y: 383,
                sea_level_y: 64,
            },
            landmass: LandmassConfigV2 {
                continent_scale_voxels,
                ocean_bias_per_1024,
                coast_width_per_1024,
                island_weight_per_1024,
                island_scale_voxels,
                ocean_depth_voxels,
            },
            relief: ReliefConfigV2 {
                base_height_voxels,
                continental_lift_voxels,
                hill_height_voxels,
                mountain_height_voxels,
                mountain_scale_voxels,
                mountain_amount_per_1024,
                plateau_amount_per_1024,
                plateau_height_voxels,
                erosion_strength_per_1024,
                roughness_per_1024,
                volcano_amount_per_1024,
            },
            water: SurfaceWaterConfigV2 {
                river_spacing_voxels,
                river_width_voxels,
                river_depth_voxels,
                lake_amount_per_1024,
                wetland_amount_per_1024,
                underground_rivers,
            },
            climate: ClimateConfigV2 {
                scale_voxels: climate_scale_voxels,
                temperature_variance_per_1024,
                humidity_variance_per_1024,
                transition_width_voxels: climate_transition_width_voxels,
                altitude_cooling_per_1024,
            },
            underground: UndergroundConfigV2 {
                cave_amount_per_1024,
                cave_scale_voxels,
                aquifer_amount_per_1024,
                aquifer_depth_voxels,
                lava_depth_voxels,
            },
        }
    }

    /// Creates a bounded compatibility profile for callers still supplying
    /// only the version-one spine record.
    #[must_use]
    pub fn for_legacy_spine(spine: &WorldgenConfigV1) -> Self {
        let mut config = Self::balanced();
        config.world.floor_y = spine.world_floor_y;
        config.world.ceiling_y = spine.world_ceiling_y;
        config.world.sea_level_y = spine
            .temperate_base_height
            .min(spine.arid_base_height)
            .saturating_sub(8)
            .clamp(
                spine.world_floor_y.saturating_add(1),
                spine.world_ceiling_y.saturating_sub(1),
            );
        config.relief.base_height_voxels = 8;
        config.relief.continental_lift_voxels = spine
            .temperate_relief
            .min(spine.arid_relief)
            .saturating_div(2)
            .max(1);
        config.relief.hill_height_voxels = spine
            .temperate_relief
            .min(spine.arid_relief)
            .saturating_div(3)
            .max(1);
        config.relief.mountain_height_voxels = spine.temperate_relief.max(spine.arid_relief).max(1);
        // Compatibility plans historically had no inland-lake occupancy.
        // Explicit V2 configs opt into lake basins and their standing water.
        config.water.lake_amount_per_1024 = 0;
        config.underground.cave_amount_per_1024 = spine.cave_threshold_per_1024;
        config.underground.cave_scale_voxels = spine.cave_cell_edge_voxels;
        config.climate.transition_width_voxels = spine.transition_width_voxels;
        config
    }

    /// Validates every bound and its agreement with the chunk-generation spine.
    ///
    /// # Errors
    ///
    /// Returns [`WorldgenError::InvalidConfig`] for a numeric violation or a
    /// vertical contract that differs from the supplied spine.
    pub fn validate_against(&self, spine: &WorldgenConfigV1) -> WorldgenResult<()> {
        self.validate_against_inner(spine, true)
    }

    pub(crate) fn validate_legacy_against(&self, spine: &WorldgenConfigV1) -> WorldgenResult<()> {
        self.validate_against_inner(spine, false)
    }

    fn validate_against_inner(
        &self,
        spine: &WorldgenConfigV1,
        require_standard_height: bool,
    ) -> WorldgenResult<()> {
        validate_world(self.world, spine, require_standard_height)?;
        validate_landmass(self.landmass)?;
        validate_relief(self.relief)?;
        validate_surface_water(self.water)?;
        validate_climate(self.climate)?;
        validate_underground(self.underground)?;
        Ok(())
    }

    /// Returns canonical JSON for the fully resolved configuration.
    ///
    /// # Errors
    ///
    /// Returns a canonical encoding error when serialization fails.
    pub fn canonical_bytes(&self) -> WorldgenResult<Vec<u8>> {
        canonical_json_bytes(self).map_err(|error| WorldgenError::CanonicalEncoding {
            kind: "TerrainConfigV2",
            reason: error.to_string(),
        })
    }

    /// Returns the canonical resolved terrain-configuration hash.
    ///
    /// # Errors
    ///
    /// Returns a canonical encoding error when serialization fails.
    pub fn canonical_hash(&self) -> WorldgenResult<TerrainConfigHashV2> {
        self.canonical_bytes()
            .map(|bytes| TerrainConfigHashV2::from_hash(CanonicalHash::digest(bytes)))
    }
}

fn validate_world(
    world: WorldBoundsV2,
    spine: &WorldgenConfigV1,
    require_standard_height: bool,
) -> WorldgenResult<()> {
    if world.floor_y != spine.world_floor_y {
        return Err(invalid("terrain.world.floor_y", "must equal world_floor_y"));
    }
    if world.ceiling_y != spine.world_ceiling_y {
        return Err(invalid(
            "terrain.world.ceiling_y",
            "must equal world_ceiling_y",
        ));
    }
    let height = i64::from(world.ceiling_y)
        .saturating_sub(i64::from(world.floor_y))
        .saturating_add(1);
    let power_of_two = u64::try_from(height).is_ok_and(u64::is_power_of_two);
    if require_standard_height && (height < 384 || !power_of_two) {
        return Err(invalid(
            "terrain.world",
            "height must be a power of two and at least 384 voxels",
        ));
    }
    if !(world.floor_y..=world.ceiling_y).contains(&world.sea_level_y) {
        return Err(invalid(
            "terrain.world.sea_level_y",
            "must stay inside the materializable column",
        ));
    }
    Ok(())
}

fn validate_landmass(config: LandmassConfigV2) -> WorldgenResult<()> {
    bounded_u32(
        "terrain.landmass.continent_scale_voxels",
        config.continent_scale_voxels,
        256,
        65_536,
    )?;
    bounded_i16(
        "terrain.landmass.ocean_bias_per_1024",
        config.ocean_bias_per_1024,
        -768,
        768,
    )?;
    ratio(
        "terrain.landmass.coast_width_per_1024",
        config.coast_width_per_1024,
        16,
    )?;
    ratio(
        "terrain.landmass.island_weight_per_1024",
        config.island_weight_per_1024,
        0,
    )?;
    bounded_u32(
        "terrain.landmass.island_scale_voxels",
        config.island_scale_voxels,
        128,
        16_384,
    )?;
    bounded_u16(
        "terrain.landmass.ocean_depth_voxels",
        config.ocean_depth_voxels,
        8,
        512,
    )
}

fn validate_relief(config: ReliefConfigV2) -> WorldgenResult<()> {
    bounded_u16(
        "terrain.relief.base_height_voxels",
        config.base_height_voxels,
        1,
        256,
    )?;
    bounded_u16(
        "terrain.relief.continental_lift_voxels",
        config.continental_lift_voxels,
        1,
        512,
    )?;
    bounded_u16(
        "terrain.relief.hill_height_voxels",
        config.hill_height_voxels,
        1,
        256,
    )?;
    bounded_u16(
        "terrain.relief.mountain_height_voxels",
        config.mountain_height_voxels,
        1,
        640,
    )?;
    bounded_u32(
        "terrain.relief.mountain_scale_voxels",
        config.mountain_scale_voxels,
        128,
        16_384,
    )?;
    ratio(
        "terrain.relief.mountain_amount_per_1024",
        config.mountain_amount_per_1024,
        0,
    )?;
    ratio(
        "terrain.relief.plateau_amount_per_1024",
        config.plateau_amount_per_1024,
        0,
    )?;
    bounded_u16(
        "terrain.relief.plateau_height_voxels",
        config.plateau_height_voxels,
        0,
        256,
    )?;
    ratio(
        "terrain.relief.erosion_strength_per_1024",
        config.erosion_strength_per_1024,
        0,
    )?;
    ratio(
        "terrain.relief.roughness_per_1024",
        config.roughness_per_1024,
        0,
    )?;
    ratio(
        "terrain.relief.volcano_amount_per_1024",
        config.volcano_amount_per_1024,
        0,
    )
}

fn validate_surface_water(config: SurfaceWaterConfigV2) -> WorldgenResult<()> {
    bounded_u16(
        "terrain.water.river_spacing_voxels",
        config.river_spacing_voxels,
        64,
        4_096,
    )?;
    bounded_u16(
        "terrain.water.river_width_voxels",
        config.river_width_voxels,
        1,
        64,
    )?;
    bounded_u16(
        "terrain.water.river_depth_voxels",
        config.river_depth_voxels,
        1,
        64,
    )?;
    if config.river_width_voxels.saturating_mul(4) >= config.river_spacing_voxels {
        return Err(invalid(
            "terrain.water.river_width_voxels",
            "four widths must fit inside river spacing",
        ));
    }
    ratio(
        "terrain.water.lake_amount_per_1024",
        config.lake_amount_per_1024,
        0,
    )?;
    ratio(
        "terrain.water.wetland_amount_per_1024",
        config.wetland_amount_per_1024,
        0,
    )
}

fn validate_climate(config: ClimateConfigV2) -> WorldgenResult<()> {
    bounded_u32(
        "terrain.climate.scale_voxels",
        config.scale_voxels,
        256,
        65_536,
    )?;
    ratio(
        "terrain.climate.temperature_variance_per_1024",
        config.temperature_variance_per_1024,
        0,
    )?;
    ratio(
        "terrain.climate.humidity_variance_per_1024",
        config.humidity_variance_per_1024,
        0,
    )?;
    bounded_u16(
        "terrain.climate.transition_width_voxels",
        config.transition_width_voxels,
        1,
        512,
    )?;
    ratio(
        "terrain.climate.altitude_cooling_per_1024",
        config.altitude_cooling_per_1024,
        0,
    )
}

fn validate_underground(config: UndergroundConfigV2) -> WorldgenResult<()> {
    ratio(
        "terrain.underground.cave_amount_per_1024",
        config.cave_amount_per_1024,
        0,
    )?;
    bounded_u16(
        "terrain.underground.cave_scale_voxels",
        config.cave_scale_voxels,
        2,
        128,
    )?;
    ratio(
        "terrain.underground.aquifer_amount_per_1024",
        config.aquifer_amount_per_1024,
        0,
    )?;
    bounded_u16(
        "terrain.underground.aquifer_depth_voxels",
        config.aquifer_depth_voxels,
        4,
        256,
    )?;
    bounded_u16(
        "terrain.underground.lava_depth_voxels",
        config.lava_depth_voxels,
        1,
        256,
    )
}

fn ratio(field: &'static str, value: u16, minimum: u16) -> WorldgenResult<()> {
    bounded_u16(field, value, minimum, MAX_RATIO)
}

fn bounded_u16(field: &'static str, value: u16, minimum: u16, maximum: u16) -> WorldgenResult<()> {
    if (minimum..=maximum).contains(&value) {
        Ok(())
    } else {
        Err(invalid(
            field,
            format!("must be in {minimum}..={maximum}, got {value}"),
        ))
    }
}

fn bounded_u32(field: &'static str, value: u32, minimum: u32, maximum: u32) -> WorldgenResult<()> {
    if (minimum..=maximum).contains(&value) {
        Ok(())
    } else {
        Err(invalid(
            field,
            format!("must be in {minimum}..={maximum}, got {value}"),
        ))
    }
}

fn bounded_i16(field: &'static str, value: i16, minimum: i16, maximum: i16) -> WorldgenResult<()> {
    if (minimum..=maximum).contains(&value) {
        Ok(())
    } else {
        Err(invalid(
            field,
            format!("must be in {minimum}..={maximum}, got {value}"),
        ))
    }
}

fn invalid(field: &'static str, reason: impl Into<String>) -> WorldgenError {
    WorldgenError::InvalidConfig {
        field,
        reason: reason.into(),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;

    #[test]
    fn every_builtin_preset_is_closed_and_valid() {
        for preset in TerrainPresetV2::ALL {
            let config = preset.resolve();
            let spine = WorldgenConfigV1 {
                world_floor_y: config.world.floor_y,
                world_ceiling_y: config.world.ceiling_y,
                ..WorldgenConfigV1::default()
            };
            assert!(config.validate_against(&spine).is_ok(), "{preset:?}");
            assert!(config.canonical_hash().is_ok(), "{preset:?}");
            let profile = preset
                .profile_id_str()
                .parse::<StableId>()
                .expect("built-in profile identity is valid");
            assert_eq!(TerrainPresetV2::from_profile_id(&profile), Some(preset));
        }
    }

    #[test]
    fn standard_profiles_use_a_512_voxel_column() {
        for preset in [
            TerrainPresetV2::Balanced,
            TerrainPresetV2::Continental,
            TerrainPresetV2::Archipelago,
            TerrainPresetV2::Alpine,
            TerrainPresetV2::Eroded,
        ] {
            let world = preset.resolve().world;
            assert_eq!(world.floor_y, -128);
            assert_eq!(world.ceiling_y, 383);
            assert_eq!(world.ceiling_y - world.floor_y + 1, 512);
        }
        let wild = TerrainPresetV2::Wild.resolve().world;
        assert_eq!(wild.ceiling_y - wild.floor_y + 1, 1_024);
    }

    #[test]
    fn builtin_profiles_have_distinct_resolved_hashes() {
        let hashes = TerrainPresetV2::ALL
            .into_iter()
            .map(|preset| {
                preset
                    .resolve()
                    .canonical_hash()
                    .expect("built-in profile canonicalizes")
            })
            .collect::<BTreeSet<_>>();
        assert_eq!(hashes.len(), TerrainPresetV2::ALL.len());
    }

    #[test]
    fn resolved_config_rejects_unknown_fields() {
        let mut value = serde_json::to_value(TerrainPresetV2::Balanced.resolve())
            .unwrap_or(serde_json::Value::Null);
        if let Some(object) = value.as_object_mut() {
            object.insert("mystery".to_owned(), serde_json::Value::Bool(true));
        }
        assert!(serde_json::from_value::<TerrainConfigV2>(value).is_err());
    }
}
