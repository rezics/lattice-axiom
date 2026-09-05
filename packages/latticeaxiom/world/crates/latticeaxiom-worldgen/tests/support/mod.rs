use std::num::NonZeroU32;

use latticeaxiom_core::CanonicalHash;
use latticeaxiom_worldgen::{
    BiomeSelectionRuleV1, ProviderGenerationIdentityV1, SurfaceBiomeIdV1,
    SurfaceBiomeTerrainProgramV1, SurfaceTerrainDomainV1, TerrainBaseAlgorithmV1, TerrainStyleV1,
};

pub(crate) fn surface_terrain_programs(
    include_boreal: bool,
    reverse: bool,
) -> Vec<SurfaceBiomeTerrainProgramV1> {
    let mut rows = vec![
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
                min_aridity: 64,
                max_humidity: -384,
            },
            TerrainBaseAlgorithmV1::AridHighlands,
        ),
    ];
    if include_boreal {
        rows.push((
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
        ));
    }
    let mut programs = rows
        .into_iter()
        .map(|(path, style, domain, priority, selection, algorithm)| {
            SurfaceBiomeTerrainProgramV1::new(
                SurfaceBiomeIdV1::new(
                    format!("fixture:biome/{path}")
                        .parse()
                        .expect("fixture biome identity is valid"),
                )
                .expect("fixture identity has biome kind"),
                style,
                domain,
                priority,
                selection,
                algorithm,
                ProviderGenerationIdentityV1::new(
                    format!("fixture:worldgen-provider/terrain-base/{path}@1")
                        .parse()
                        .expect("fixture terrain provider identity is valid"),
                    NonZeroU32::MIN,
                    10,
                    CanonicalHash::digest(format!("{path}-terrain-implementation-v10")),
                ),
            )
        })
        .collect::<Vec<_>>();
    if reverse {
        programs.reverse();
    }
    programs
}
