//! Terrenia-owned world-generation profiles and policy.
//!
//! The generic world-generation crate validates and executes resolved plans.
//! This package owns Terrenia profile identities, defaults, and the concrete
//! parameter rows selected by its New World surface.

use std::num::NonZeroU32;

use latticeaxiom_core::{CanonicalHash, StableId};
use latticeaxiom_worldgen::{
    BiomeSelectionRuleV1, ClimateConfigV2, LandmassConfigV2, ProviderGenerationIdentityV1,
    ReliefConfigV2, SurfaceBiomeIdV1, SurfaceBiomeTerrainProgramV1, SurfaceTerrainDomainV1,
    SurfaceWaterConfigV2, TerrainBaseAlgorithmV1, TerrainConfigV2, TerrainStyleV1,
    UndergroundConfigV2, WorldBoundsV2, WorldgenError, WorldgenResult,
};
use serde::{Deserialize, Serialize};

/// Terrenia's named world-creation terrain directions.
///
/// A preset is authoring policy owned by this package. Generation consumes the
/// fully resolved [`TerrainConfigV2`] returned by [`Self::resolve`].
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

    /// Returns the stable authoring name.
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

    /// Resolves a stable creation-profile identity to a Terrenia preset.
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
            Self::Balanced => balanced(),
            Self::Continental => continental(),
            Self::Archipelago => archipelago(),
            Self::Alpine => alpine(),
            Self::Eroded => eroded(),
            Self::Wild => wild(),
        }
    }
}

/// Resolves Terrenia's surface biomes to their owned terrain providers.
///
/// `implementation_fingerprint` is the verified hash of the selected package
/// realization. It becomes part of generation identity and receipts.
///
/// # Errors
///
/// Returns an invalid-program error if a package constant violates the stable
/// identifier grammar or expected biome kind.
pub fn surface_biome_terrain_programs(
    implementation_fingerprint: CanonicalHash,
) -> WorldgenResult<Vec<SurfaceBiomeTerrainProgramV1>> {
    [
        (
            "open-ocean",
            TerrainStyleV1::Marine,
            SurfaceTerrainDomainV1::Marine,
            u16::MAX,
            BiomeSelectionRuleV1::Fallback,
            TerrainBaseAlgorithmV1::MarineBasin,
        ),
        (
            "temperate-woodland",
            TerrainStyleV1::TemperateWoodland,
            SurfaceTerrainDomainV1::Land,
            u16::MAX,
            BiomeSelectionRuleV1::Fallback,
            TerrainBaseAlgorithmV1::TemperateRelief,
        ),
        (
            "arid-badlands",
            TerrainStyleV1::AridBadlands,
            SurfaceTerrainDomainV1::Land,
            20,
            BiomeSelectionRuleV1::AridityOrDry {
                min_aridity: 96,
                max_humidity: -384,
            },
            TerrainBaseAlgorithmV1::AridHighlands,
        ),
        (
            "boreal-wetland",
            TerrainStyleV1::BorealWetland,
            SurfaceTerrainDomainV1::Land,
            10,
            BiomeSelectionRuleV1::ClimateRange {
                min_temperature: -1_024,
                max_temperature: -97,
                min_humidity: -319,
                max_humidity: 1_024,
            },
            TerrainBaseAlgorithmV1::BorealLowlands,
        ),
    ]
    .into_iter()
    .map(|(path, style, domain, priority, selection, algorithm)| {
        terrain_program(
            path,
            style,
            domain,
            priority,
            selection,
            algorithm,
            implementation_fingerprint,
        )
    })
    .collect()
}

fn terrain_program(
    path: &str,
    style: TerrainStyleV1,
    domain: SurfaceTerrainDomainV1,
    selection_priority: u16,
    selection: BiomeSelectionRuleV1,
    algorithm: TerrainBaseAlgorithmV1,
    implementation_fingerprint: CanonicalHash,
) -> WorldgenResult<SurfaceBiomeTerrainProgramV1> {
    let biome = parse_program_identity(&format!("terrenia:biome/{path}"))?;
    let provider =
        parse_program_identity(&format!("terrenia:worldgen-provider/terrain-base/{path}@1"))?;
    Ok(SurfaceBiomeTerrainProgramV1::new(
        SurfaceBiomeIdV1::new(biome)?,
        style,
        domain,
        selection_priority,
        selection,
        algorithm,
        ProviderGenerationIdentityV1::new(
            provider,
            NonZeroU32::MIN,
            11,
            implementation_fingerprint,
        ),
    ))
}

fn parse_program_identity(value: &str) -> WorldgenResult<StableId> {
    value
        .parse::<StableId>()
        .map_err(|error| WorldgenError::InvalidTerrainProgram {
            field: "package_identity",
            reason: error.to_string(),
        })
}

const fn balanced() -> TerrainConfigV2 {
    preset(
        8_192, -96, 192, 224, 1_536, 96, 22, 54, 28, 176, 2_048, 330, 210, 36, 520, 260, 18, 384,
        9, 6, 220, 180, true, 4_096, 820, 820, 48, 500, 150, 24, 360, 30, 18,
    )
}

const fn continental() -> TerrainConfigV2 {
    preset(
        13_312, -230, 160, 96, 2_048, 88, 26, 66, 30, 184, 2_560, 360, 250, 42, 450, 230, 14, 512,
        10, 7, 180, 130, true, 5_120, 760, 780, 56, 540, 142, 28, 330, 34, 18,
    )
}

const fn archipelago() -> TerrainConfigV2 {
    preset(
        5_120, 190, 256, 620, 1_024, 144, 16, 36, 24, 126, 1_536, 270, 130, 28, 590, 300, 28, 256,
        8, 5, 350, 100, true, 3_072, 900, 940, 36, 420, 155, 22, 420, 28, 16,
    )
}

const fn alpine() -> TerrainConfigV2 {
    preset(
        8_192, -120, 176, 150, 1_536, 104, 28, 62, 34, 248, 2_304, 540, 180, 30, 360, 320, 20, 384,
        8, 8, 120, 90, true, 3_584, 860, 720, 42, 720, 175, 30, 300, 24, 20,
    )
}

const fn eroded() -> TerrainConfigV2 {
    preset(
        9_216, -140, 220, 180, 1_792, 78, 18, 42, 18, 92, 2_816, 230, 310, 24, 830, 120, 8, 448,
        12, 4, 420, 520, true, 5_120, 700, 920, 72, 360, 118, 30, 470, 38, 14,
    )
}

const fn wild() -> TerrainConfigV2 {
    let mut config = preset(
        7_168, -100, 160, 260, 1_280, 176, 32, 86, 48, 360, 1_536, 610, 360, 74, 280, 580, 72, 320,
        8, 9, 160, 100, true, 3_072, 920, 900, 32, 800, 220, 20, 500, 44, 26,
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
    reason = "preset rows keep resolved package policy auditable"
)]
const fn preset(
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
) -> TerrainConfigV2 {
    TerrainConfigV2::from_parts(
        WorldBoundsV2 {
            floor_y: -128,
            ceiling_y: 383,
            sea_level_y: 64,
        },
        LandmassConfigV2 {
            continent_scale_voxels,
            ocean_bias_per_1024,
            coast_width_per_1024,
            island_weight_per_1024,
            island_scale_voxels,
            ocean_depth_voxels,
        },
        ReliefConfigV2 {
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
        SurfaceWaterConfigV2 {
            river_spacing_voxels,
            river_width_voxels,
            river_depth_voxels,
            lake_amount_per_1024,
            wetland_amount_per_1024,
            underground_rivers,
        },
        ClimateConfigV2 {
            scale_voxels: climate_scale_voxels,
            temperature_variance_per_1024,
            humidity_variance_per_1024,
            transition_width_voxels: climate_transition_width_voxels,
            altitude_cooling_per_1024,
        },
        UndergroundConfigV2 {
            cave_amount_per_1024,
            cave_scale_voxels,
            aquifer_amount_per_1024,
            aquifer_depth_voxels,
            lava_depth_voxels,
        },
    )
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use latticeaxiom_worldgen::WorldgenConfigV1;

    use super::*;

    #[test]
    fn every_package_preset_is_closed_and_valid() {
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
                .expect("package profile identity is valid");
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
        let wild_world = TerrainPresetV2::Wild.resolve().world;
        assert_eq!(wild_world.ceiling_y - wild_world.floor_y + 1, 1_024);
    }

    #[test]
    fn package_profiles_have_distinct_resolved_hashes() {
        let hashes = TerrainPresetV2::ALL
            .into_iter()
            .map(|preset| {
                preset
                    .resolve()
                    .canonical_hash()
                    .expect("package profile canonicalizes")
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

    #[test]
    fn every_surface_biome_owns_one_terrain_provider() {
        let programs = surface_biome_terrain_programs(CanonicalHash::digest("package"))
            .expect("package terrain programs are valid");
        assert_eq!(programs.len(), 4);
        assert_eq!(
            programs
                .iter()
                .map(SurfaceBiomeTerrainProgramV1::material_style)
                .collect::<BTreeSet<_>>(),
            TerrainStyleV1::ALL.into_iter().collect()
        );
    }
}
