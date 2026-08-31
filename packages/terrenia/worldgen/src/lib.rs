//! Terrenia-owned world-generation profiles and policy.
//!
//! The generic world-generation crate validates and executes resolved plans.
//! This package owns Terrenia profile identities, defaults, and the concrete
//! parameter rows selected by its New World surface.

use std::num::NonZeroU32;

use latticeaxiom_core::{CanonicalHash, StableId};
use latticeaxiom_worldgen::{
    BiomeSelectionRuleV1, ClimateConfigV2, ClosedSplinePointV1, ClosedSplineV1, LandmassConfigV2,
    ProviderGenerationIdentityV1, ReliefConfigV2, SemanticFieldSpecV1, SemanticTerrainPolicyV1,
    SurfaceBiomeIdV1, SurfaceBiomeTerrainProgramV1, SurfaceTerrainDomainV1, SurfaceWaterConfigV2,
    TerrainBaseAlgorithmV1, TerrainConfigV2, TerrainStyleV1, UndergroundConfigV2, WorldBoundsV2,
    WorldgenError, WorldgenResult,
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

/// Resolves Terrenia's new-world semantic terrain revision.
///
/// The returned provider identities use registration major `@2` and
/// algorithm revision 13. The package-owned policy is serialized into every
/// program and therefore participates in the generation epoch.
///
/// # Errors
///
/// Returns an invalid-program or invalid-config error if package constants,
/// resolved frequencies, or closed spline rows violate their contracts.
pub fn semantic_surface_biome_terrain_programs(
    terrain: TerrainConfigV2,
    implementation_fingerprint: CanonicalHash,
) -> WorldgenResult<Vec<SurfaceBiomeTerrainProgramV1>> {
    let policy = semantic_policy(terrain)?;
    [
        (
            "open-ocean",
            TerrainStyleV1::Marine,
            SurfaceTerrainDomainV1::Marine,
            u16::MAX,
            BiomeSelectionRuleV1::Fallback,
            TerrainBaseAlgorithmV1::SemanticMarineBasin,
        ),
        (
            "temperate-woodland",
            TerrainStyleV1::TemperateWoodland,
            SurfaceTerrainDomainV1::Land,
            u16::MAX,
            BiomeSelectionRuleV1::Fallback,
            TerrainBaseAlgorithmV1::SemanticTemperateRelief,
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
            TerrainBaseAlgorithmV1::SemanticAridHighlands,
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
            TerrainBaseAlgorithmV1::SemanticBorealLowlands,
        ),
    ]
    .into_iter()
    .map(|(path, style, domain, priority, selection, algorithm)| {
        semantic_terrain_program(
            path,
            style,
            domain,
            priority,
            selection,
            algorithm,
            implementation_fingerprint,
            policy.clone(),
        )
    })
    .collect()
}

#[allow(
    clippy::too_many_arguments,
    reason = "package terrain rows keep ownership, selection, implementation identity, and semantic policy explicit"
)]
fn semantic_terrain_program(
    path: &str,
    style: TerrainStyleV1,
    domain: SurfaceTerrainDomainV1,
    selection_priority: u16,
    selection: BiomeSelectionRuleV1,
    algorithm: TerrainBaseAlgorithmV1,
    implementation_fingerprint: CanonicalHash,
    policy: SemanticTerrainPolicyV1,
) -> WorldgenResult<SurfaceBiomeTerrainProgramV1> {
    let biome = parse_program_identity(&format!("terrenia:biome/{path}"))?;
    let provider =
        parse_program_identity(&format!("terrenia:worldgen-provider/terrain-base/{path}@2"))?;
    Ok(SurfaceBiomeTerrainProgramV1::new_semantic(
        SurfaceBiomeIdV1::new(biome)?,
        style,
        domain,
        selection_priority,
        selection,
        algorithm,
        ProviderGenerationIdentityV1::new(
            provider,
            NonZeroU32::MIN,
            13,
            implementation_fingerprint,
        ),
        policy,
    ))
}

fn semantic_policy(terrain: TerrainConfigV2) -> WorldgenResult<SemanticTerrainPolicyV1> {
    let scale = |value: u32| {
        NonZeroU32::new(value.max(1)).map(|scale| SemanticFieldSpecV1::new(scale, 1_024))
    };
    let continentalness = scale(terrain.landmass.continent_scale_voxels);
    let uplift = scale(terrain.relief.mountain_scale_voxels);
    let lithology = scale(terrain.relief.mountain_scale_voxels.div_ceil(2));
    let temperature = scale(terrain.climate.scale_voxels);
    let precipitation = scale(terrain.climate.scale_voxels.saturating_mul(3).div_ceil(4));
    let infiltration = scale(terrain.climate.scale_voxels.div_ceil(2));
    let detail = scale(terrain.relief.mountain_scale_voxels.div_ceil(8).max(64));
    let terrain_volume = scale(u32::from(terrain.underground.cave_scale_voxels).saturating_mul(4));
    let geologic_volume = scale(u32::from(terrain.underground.cave_scale_voxels).saturating_mul(8));
    let field = |value: Option<SemanticFieldSpecV1>, name| {
        value.ok_or_else(|| WorldgenError::InvalidConfig {
            field: name,
            reason: "resolved semantic field scale must be nonzero".to_owned(),
        })
    };
    SemanticTerrainPolicyV1::new(
        field(continentalness, "terrenia.semantic.continentalness")?,
        field(uplift, "terrenia.semantic.uplift")?,
        field(lithology, "terrenia.semantic.lithology")?,
        field(temperature, "terrenia.semantic.temperature")?,
        field(precipitation, "terrenia.semantic.precipitation")?,
        field(infiltration, "terrenia.semantic.infiltration")?,
        field(detail, "terrenia.semantic.detail")?,
        field(terrain_volume, "terrenia.semantic.terrain_volume")?,
        field(geologic_volume, "terrenia.semantic.geologic_volume")?,
        closed_spline(&[
            (-1_024, -1_024),
            (-256, -192),
            (0, 0),
            (256, 160),
            (1_024, 1_024),
        ])?,
        closed_spline(&[(-1_024, 0), (0, 48), (384, 280), (768, 800), (1_024, 1_024)])?,
        closed_spline(&[(-1_024, 96), (0, 320), (512, 720), (1_024, 1_024)])?,
        closed_spline(&[(-1_024, 0), (256, 0), (768, 768), (1_024, 1_024)])?,
        4 * 256,
        6 * 256,
        2 * 256,
        terrain
            .water
            .river_width_voxels
            .saturating_add(terrain.water.river_depth_voxels)
            .saturating_mul(256),
    )
}

fn closed_spline(points: &[(i16, i32)]) -> WorldgenResult<ClosedSplineV1> {
    ClosedSplineV1::new(
        points
            .iter()
            .map(|&(input, output)| ClosedSplinePointV1::new(input, output))
            .collect(),
    )
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
            12,
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

    use latticeaxiom_worldgen::{
        SemanticTerrainFieldV1, WorldSeedV1, WorldgenConfigV1, WorldgenSeedRootV2,
    };

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

    #[test]
    fn semantic_provider_revision_is_distinct_and_closed() {
        let fingerprint = CanonicalHash::digest("package");
        let legacy = surface_biome_terrain_programs(fingerprint)
            .expect("legacy programs remain available for frozen epochs");
        let semantic = semantic_surface_biome_terrain_programs(
            TerrainPresetV2::Balanced.resolve(),
            fingerprint,
        )
        .expect("semantic programs are valid");
        assert_eq!(legacy.len(), semantic.len());
        for (old, new) in legacy.iter().zip(&semantic) {
            assert_eq!(old.provider().algorithm_revision(), 12);
            assert_eq!(new.provider().algorithm_revision(), 13);
            assert!(old.provider().provider_stable_id().as_str().ends_with("@1"));
            assert!(new.provider().provider_stable_id().as_str().ends_with("@2"));
            assert!(old.semantic_policy().is_none());
            assert!(new.semantic_policy().is_some());
        }
        let hashes = semantic
            .iter()
            .filter_map(SurfaceBiomeTerrainProgramV1::semantic_policy)
            .map(|policy| policy.canonical_hash().expect("policy canonicalizes"))
            .collect::<BTreeSet<_>>();
        assert_eq!(hashes.len(), 1);
        assert!(semantic.iter().all(|program| {
            program
                .semantic_policy()
                .is_some_and(|policy| policy.maximum_density_displacement_voxels() == 8)
        }));
    }

    #[test]
    fn semantic_default_has_land_ocean_relief_and_no_axis_lock() {
        let terrain = TerrainPresetV2::Balanced.resolve();
        let policy = semantic_policy(terrain).expect("balanced semantic policy is valid");
        let field = SemanticTerrainFieldV1::new(
            WorldgenSeedRootV2::from_world_seed(WorldSeedV1::from_integer(0x51_7a)),
            terrain,
            policy,
        )
        .expect("semantic field compiles");
        let edge = 48_i64;
        let spacing = 128_i64;
        let mut heights = Vec::with_capacity(
            usize::try_from(edge * edge).expect("quality corpus size fits usize"),
        );
        let mut golden_bytes = Vec::with_capacity(
            usize::try_from(edge * edge * 16).expect("golden corpus size fits usize"),
        );
        let mut land = 0_usize;
        for z in 0..edge {
            for x in 0..edge {
                let sample = field
                    .sample((x - edge / 2) * spacing, (z - edge / 2) * spacing)
                    .expect("quality corpus stays in the fixed-field envelope");
                land += usize::from(sample.continentalness_per_1024() >= 0);
                heights.push(i64::from(sample.initial_surface_y_q8()));
                golden_bytes.extend_from_slice(&sample.continentalness_per_1024().to_be_bytes());
                golden_bytes.extend_from_slice(&sample.uplift_per_1024().to_be_bytes());
                golden_bytes.extend_from_slice(&sample.lithology_per_1024().to_be_bytes());
                golden_bytes.extend_from_slice(&sample.effective_runoff_q16().to_be_bytes());
                golden_bytes.extend_from_slice(&sample.initial_surface_y_q8().to_be_bytes());
            }
        }
        let count = heights.len();
        assert!((count / 5..=count * 4 / 5).contains(&land));
        let mut axis_energy = 0_u128;
        let mut diagonal_energy = 0_u128;
        for z in 0..edge - 1 {
            for x in 0..edge - 1 {
                let index = usize::try_from(z * edge + x).expect("index fits");
                let east = usize::try_from(z * edge + x + 1).expect("index fits");
                let south = usize::try_from((z + 1) * edge + x).expect("index fits");
                let diagonal = usize::try_from((z + 1) * edge + x + 1).expect("index fits");
                axis_energy = axis_energy
                    .saturating_add(u128::from(heights[index].abs_diff(heights[east])))
                    .saturating_add(u128::from(heights[index].abs_diff(heights[south])));
                diagonal_energy = diagonal_energy
                    .saturating_add(u128::from(heights[index].abs_diff(heights[diagonal])));
            }
        }
        assert!(heights.iter().min() < heights.iter().max());
        assert!(axis_energy > diagonal_energy / 2);
        assert!(axis_energy < diagonal_energy.saturating_mul(4));
        assert_eq!(
            CanonicalHash::digest(golden_bytes).to_string(),
            "389e76008dbf36ce25a9684150e0310d4e464f51fb93472e886b5d15940a7020"
        );
    }
}
