use std::num::NonZeroU32;

use latticeaxiom_core::CanonicalHash;
use latticeaxiom_worldgen::{
    ProviderGenerationIdentityV1, SurfaceBiomeIdV1, SurfaceBiomeTerrainProgramV1,
    TerrainBaseAlgorithmV1, TerrainStyleV1,
};

pub(crate) fn surface_terrain_programs(
    include_boreal: bool,
    reverse: bool,
) -> Vec<SurfaceBiomeTerrainProgramV1> {
    let mut rows = vec![
        ("temperate-woodland", TerrainStyleV1::TemperateWoodland),
        ("arid-badlands", TerrainStyleV1::AridBadlands),
    ];
    if include_boreal {
        rows.push(("boreal-wetland", TerrainStyleV1::BorealWetland));
    }
    let mut programs = rows
        .into_iter()
        .map(|(path, style)| {
            SurfaceBiomeTerrainProgramV1::new(
                SurfaceBiomeIdV1::new(
                    format!("fixture:biome/{path}")
                        .parse()
                        .expect("fixture biome identity is valid"),
                )
                .expect("fixture identity has biome kind"),
                style,
                TerrainBaseAlgorithmV1::ContinentalComposite,
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
