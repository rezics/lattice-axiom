//! Reproducible worker-throughput diagnostics for deterministic chunk generation.
//!
//! These measurements do not define the main-loop frame budget; the host runs
//! generation on Bevy's task pool and bounds result application separately.

#![allow(
    clippy::expect_used,
    reason = "a malformed benchmark fixture must abort before measurement"
)]

use std::{hint::black_box, num::NonZeroU32};

use criterion::{Criterion, criterion_group, criterion_main};
use latticeaxiom_core::{CanonicalHash, StableId};
use latticeaxiom_worldgen::{
    AdjacentEpochSnapshotV1, AuthoredWorldgenBindingsV1, CellEpochStateV1, ChunkCoordinate,
    ChunkGenerationOutcomeV1, ChunkGenerationRequestV1, D4BlockCatalogClosureV1, D4MaterialRoleV1,
    D4RoleVocabularyV1, DimensionId, FrozenRoleBindingsV1, GenerationPlanInputV1, GenerationPlanV1,
    HydrologyFluidBindingsV1, HydrologyOccupancyConfigV1, HydrologyOccupancyInputV1,
    NaturalLayerConfigV1, NaturalLayerInputV1, PlanActivationIdV1, PlanningCellCoordinateV1,
    ProviderGenerationIdentityV1, ProviderOfferV1, ProviderSlotV1, TerrainConfigV2,
    TerrainPresetV2, TerrainStyleV1, WorldSeedV1, WorldgenConfigV1, WorldgenLimitsV1,
};

const ROLE_TARGETS: [(D4MaterialRoleV1, &str); 16] = [
    (D4MaterialRoleV1::Empty, "air"),
    (D4MaterialRoleV1::TemperateSurface, "grass"),
    (D4MaterialRoleV1::TemperateSubsurface, "dirt"),
    (D4MaterialRoleV1::TemperateBaseRock, "stone"),
    (D4MaterialRoleV1::TemperateSecondaryRock, "limestone"),
    (D4MaterialRoleV1::TemperateClay, "clay"),
    (D4MaterialRoleV1::TemperateGravel, "gravel"),
    (D4MaterialRoleV1::WoodlandLog, "oak-log"),
    (D4MaterialRoleV1::WoodlandLeaves, "oak-leaves"),
    (D4MaterialRoleV1::WoodlandGroundCover, "tall-grass"),
    (D4MaterialRoleV1::AridSand, "sand"),
    (D4MaterialRoleV1::AridRedSand, "red-sand"),
    (D4MaterialRoleV1::AridSandstone, "sandstone"),
    (D4MaterialRoleV1::AridRedSandstone, "red-sandstone"),
    (D4MaterialRoleV1::AridBaseRock, "basalt"),
    (D4MaterialRoleV1::CopperResource, "copper-ore"),
];

fn generation_benchmarks(c: &mut Criterion) {
    let plan = fixture_plan();
    let plan_32 = fixture_plan_with_edge(32);
    let d4_coordinate = surface_chunk(&plan, -3, 5);
    let d4_coordinate_32 = surface_chunk(&plan_32, -3, 5);
    let adjacent = AdjacentEpochSnapshotV1::all_unassigned(PlanningCellCoordinateV1::new(-1, 0))
        .expect("benchmark adjacency is representable");
    c.bench_function("d4_chunk_16_cubic_snapshot_candidate", |bencher| {
        bencher.iter(|| {
            plan.generate(ChunkGenerationRequestV1::new(
                black_box(d4_coordinate),
                None,
                CellEpochStateV1::Unassigned,
                adjacent.clone(),
                Vec::new(),
            ))
            .expect("benchmark chunk must remain valid")
        });
    });
    c.bench_function("d4_chunk_32_cubic_snapshot_candidate", |bencher| {
        bencher.iter(|| {
            plan_32
                .generate(ChunkGenerationRequestV1::new(
                    black_box(d4_coordinate_32),
                    None,
                    CellEpochStateV1::Unassigned,
                    adjacent.clone(),
                    Vec::new(),
                ))
                .expect("32-cubic benchmark chunk must remain valid")
        });
    });
    let natural = natural_fixture_plan();
    let natural_32 = natural_fixture_plan_with_edge(32);
    let natural_coordinate = surface_chunk(&natural, -3, 5);
    let natural_coordinate_32 = surface_chunk(&natural_32, -3, 5);
    c.bench_function("v5_natural_chunk_16_cubic_snapshot_candidate", |bencher| {
        bencher.iter(|| {
            natural
                .generate(ChunkGenerationRequestV1::new(
                    black_box(natural_coordinate),
                    None,
                    CellEpochStateV1::Unassigned,
                    adjacent.clone(),
                    Vec::new(),
                ))
                .expect("natural benchmark chunk must remain valid")
        });
    });
    c.bench_function("v5_natural_chunk_32_cubic_snapshot_candidate", |bencher| {
        bencher.iter(|| {
            natural_32
                .generate(ChunkGenerationRequestV1::new(
                    black_box(natural_coordinate_32),
                    None,
                    CellEpochStateV1::Unassigned,
                    adjacent.clone(),
                    Vec::new(),
                ))
                .expect("32-cubic natural benchmark chunk must remain valid")
        });
    });
    production_boreal_benchmark(c);
    let hydrology_32 = hydrology_fixture_plan_with_edge(32);
    let hydrology_coordinate = surface_chunk(&hydrology_32, -3, 5);
    c.bench_function(
        "v6_hydrology_chunk_32_cubic_occupancy_candidate",
        |bencher| {
            bencher.iter(|| {
                hydrology_32
                    .hydrology_occupancy_candidate(black_box(hydrology_coordinate))
                    .expect("32-cubic hydrology benchmark chunk must remain valid")
            });
        },
    );
    let hydrology_snapshot = hydrology_32
        .generate(
            hydrology_32
                .vacant_generation_request(hydrology_coordinate)
                .expect("32-cubic hydrology benchmark request must remain valid"),
        )
        .expect("32-cubic hydrology benchmark snapshot must remain valid");
    let ChunkGenerationOutcomeV1::Prepared(hydrology_snapshot) = hydrology_snapshot else {
        panic!("benchmark request must prepare a snapshot candidate");
    };
    c.bench_function("v6_hydrology_chunk_32_cubic_snapshot_reuse", |bencher| {
        bencher.iter(|| {
            hydrology_32
                .hydrology_occupancy_candidate_for_snapshot(black_box(&hydrology_snapshot))
                .expect("32-cubic snapshot reuse must remain valid")
        });
    });
    terrain_query_benchmarks(c, &plan);
    v2_terrain_benchmarks(c);
}

fn production_boreal_benchmark(c: &mut Criterion) {
    let plan = production_boreal_fixture_plan();
    let coordinate = surface_chunk_with_style(&plan, TerrainStyleV1::BorealWetland);
    let adjacent = AdjacentEpochSnapshotV1::all_unassigned(PlanningCellCoordinateV1::from_chunk(
        coordinate,
        plan.config().planning_cell_edge_chunks,
    ))
    .expect("production boreal benchmark adjacency is representable");
    c.bench_function(
        "v5_production_boreal_chunk_32_cubic_snapshot_candidate",
        |bencher| {
            bencher.iter(|| {
                plan.generate(ChunkGenerationRequestV1::new(
                    black_box(coordinate),
                    None,
                    CellEpochStateV1::Unassigned,
                    adjacent.clone(),
                    Vec::new(),
                ))
                .expect("production boreal benchmark chunk must remain valid")
            });
        },
    );
}

fn terrain_query_benchmarks(c: &mut Criterion, plan: &GenerationPlanV1) {
    c.bench_function("d4_density_4096_samples", |bencher| {
        bencher.iter(|| {
            let mut accumulator = 0_i64;
            for index in 0_i64..4_096 {
                accumulator ^= plan.terrain_density(
                    black_box(index.rem_euclid(97) - 48),
                    black_box(index.rem_euclid(64) - 32),
                    black_box(index.rem_euclid(89) - 44),
                );
            }
            black_box(accumulator)
        });
    });
    let high_relief = natural_fixture_plan_with_config(
        WorldgenConfigV1 {
            chunk_edge_voxels: 32,
            transition_width_voxels: 16,
            height_noise_scale_voxels: 32,
            world_ceiling_y: 319,
            temperate_base_height: 80,
            temperate_relief: 112,
            arid_base_height: 88,
            arid_relief: 128,
            ..WorldgenConfigV1::default()
        },
        NaturalLayerConfigV1 {
            boreal_base_height: 80,
            boreal_relief: 112,
            ..NaturalLayerConfigV1::default()
        },
        false,
    );
    c.bench_function("v5_high_relief_4096_height_samples", |bencher| {
        bencher.iter(|| {
            let mut accumulator = 0_i64;
            for index in 0_i64..4_096 {
                let x = index.rem_euclid(64).saturating_mul(17).saturating_sub(544);
                let z = index.div_euclid(64).saturating_mul(17).saturating_sub(544);
                accumulator ^= i64::from(high_relief.terrain_height(black_box(x), black_box(z)));
            }
            black_box(accumulator)
        });
    });
}

fn v2_terrain_benchmarks(c: &mut Criterion) {
    let balanced = v2_fixture_plan(TerrainPresetV2::Balanced);
    let wild = v2_fixture_plan(TerrainPresetV2::Wild);
    let coordinate = surface_chunk(&balanced, -3, 5);
    let adjacent = AdjacentEpochSnapshotV1::all_unassigned(PlanningCellCoordinateV1::from_chunk(
        coordinate,
        balanced.config().planning_cell_edge_chunks,
    ))
    .expect("V2 benchmark adjacency is representable");
    c.bench_function("v2_balanced_chunk_32_cubic_snapshot_candidate", |bencher| {
        bencher.iter(|| {
            balanced
                .generate(ChunkGenerationRequestV1::new(
                    black_box(coordinate),
                    None,
                    CellEpochStateV1::Unassigned,
                    adjacent.clone(),
                    Vec::new(),
                ))
                .expect("V2 balanced benchmark chunk must remain valid")
        });
    });
    for (name, plan) in [
        ("v2_balanced_4096_terrain_columns", &balanced),
        ("v2_wild_4096_terrain_columns", &wild),
    ] {
        c.bench_function(name, |bencher| {
            bencher.iter(|| {
                let mut accumulator = 0_i64;
                for index in 0_i64..4_096 {
                    let x = index.rem_euclid(64).saturating_mul(17).saturating_sub(544);
                    let z = index.div_euclid(64).saturating_mul(17).saturating_sub(544);
                    let sample = plan.terrain_column(black_box(x), black_box(z));
                    accumulator ^= i64::from(sample.height());
                    accumulator ^= i64::from(sample.surface_water_y().unwrap_or_default());
                }
                black_box(accumulator)
            });
        });
    }
}

fn fixture_plan() -> GenerationPlanV1 {
    fixture_plan_with_edge(WorldgenConfigV1::default().chunk_edge_voxels)
}

fn fixture_plan_with_edge(chunk_edge_voxels: u16) -> GenerationPlanV1 {
    let vocabulary = D4RoleVocabularyV1::new(ROLE_TARGETS.iter().map(|(purpose, _)| {
        (
            *purpose,
            stable_id(&format!("terrenia:block-role/d4/{}@1", purpose.as_str())),
        )
    }))
    .expect("benchmark vocabulary is valid");
    let bindings = FrozenRoleBindingsV1::new(ROLE_TARGETS.iter().map(|(purpose, target)| {
        (
            stable_id(&format!("terrenia:block-role/d4/{}@1", purpose.as_str())),
            block_id(target),
        )
    }))
    .expect("benchmark role bindings are valid");
    let mut blocks = ROLE_TARGETS
        .iter()
        .map(|(_, path)| block_id(path))
        .collect::<Vec<_>>();
    blocks.extend([block_id("obsidian"), block_id("copper-block")]);
    let catalog = D4BlockCatalogClosureV1::new(blocks).expect("benchmark catalog is valid");
    let dimension = "terrenia:dimension/terrenia"
        .parse::<DimensionId>()
        .expect("benchmark dimension is valid");
    let config = WorldgenConfigV1 {
        chunk_edge_voxels,
        ..WorldgenConfigV1::default()
    };
    GenerationPlanV1::compile(GenerationPlanInputV1::new(
        dimension,
        WorldSeedV1::from_integer(42),
        config,
        7,
        PlanActivationIdV1::from_hash(CanonicalHash::digest(b"benchmark-activation")),
        provider_offers(),
        vocabulary,
        bindings,
        catalog,
        CanonicalHash::digest(b"benchmark-semantic-image"),
        vec![CanonicalHash::digest(b"benchmark-lock")],
        WorldgenLimitsV1::default(),
    ))
    .expect("benchmark plan is valid")
}

fn provider_offers() -> Vec<ProviderOfferV1> {
    [
        (ProviderSlotV1::GenerationCoordinator, "coordinator"),
        (ProviderSlotV1::StyleSelector, "selector"),
        (ProviderSlotV1::TemperateTerrain, "temperate"),
        (ProviderSlotV1::AridTerrain, "arid"),
        (ProviderSlotV1::TerrainTransition, "transition"),
        (ProviderSlotV1::CaveTopology, "cave"),
        (ProviderSlotV1::Materializer, "materializer"),
    ]
    .into_iter()
    .map(|(slot, path)| {
        let revision = if slot == ProviderSlotV1::CaveTopology {
            8
        } else {
            7
        };
        ProviderOfferV1::new(
            slot,
            ProviderGenerationIdentityV1::new(
                stable_id(&format!("fixture:worldgen-provider/{path}@1")),
                NonZeroU32::MIN,
                revision,
                CanonicalHash::digest(format!("{path}-benchmark-implementation-v{revision}")),
            ),
        )
    })
    .collect()
}

fn natural_fixture_plan() -> GenerationPlanV1 {
    natural_fixture_plan_with_edge(WorldgenConfigV1::default().chunk_edge_voxels)
}

fn natural_fixture_plan_with_edge(chunk_edge_voxels: u16) -> GenerationPlanV1 {
    natural_fixture_plan_with_options(chunk_edge_voxels, false)
}

fn hydrology_fixture_plan_with_edge(chunk_edge_voxels: u16) -> GenerationPlanV1 {
    natural_fixture_plan_with_options(chunk_edge_voxels, true)
}

fn natural_fixture_plan_with_options(
    chunk_edge_voxels: u16,
    include_hydrology_occupancy: bool,
) -> GenerationPlanV1 {
    natural_fixture_plan_with_config(
        WorldgenConfigV1 {
            chunk_edge_voxels,
            ..WorldgenConfigV1::default()
        },
        NaturalLayerConfigV1::default(),
        include_hydrology_occupancy,
    )
}

fn production_boreal_fixture_plan() -> GenerationPlanV1 {
    natural_fixture_plan_with_config(
        WorldgenConfigV1 {
            chunk_edge_voxels: 32,
            planning_cell_edge_chunks: 2,
            transition_width_voxels: 16,
            height_noise_scale_voxels: 32,
            world_floor_y: -64,
            world_ceiling_y: 319,
            temperate_base_height: 80,
            temperate_relief: 112,
            arid_base_height: 88,
            arid_relief: 128,
            ..WorldgenConfigV1::default()
        },
        NaturalLayerConfigV1 {
            boreal_base_height: 80,
            boreal_relief: 112,
            ..NaturalLayerConfigV1::default()
        },
        false,
    )
}

fn natural_fixture_plan_with_config(
    config: WorldgenConfigV1,
    natural_config: NaturalLayerConfigV1,
    include_hydrology_occupancy: bool,
) -> GenerationPlanV1 {
    natural_fixture_plan_with_config_and_terrain(
        config,
        natural_config,
        include_hydrology_occupancy,
        None,
    )
}

fn natural_fixture_plan_with_config_and_terrain(
    config: WorldgenConfigV1,
    natural_config: NaturalLayerConfigV1,
    include_hydrology_occupancy: bool,
    terrain_config: Option<TerrainConfigV2>,
) -> GenerationPlanV1 {
    let bindings = AuthoredWorldgenBindingsV1::from_json(
        include_str!("../../../packages/terrenia/worldgen/data/authored-block-bindings-v1.json")
            .as_bytes(),
    )
    .expect("benchmark authored bindings decode");
    let mut natural_offers = Vec::new();
    for (slot, path) in [
        (ProviderSlotV1::Geology, "geology"),
        (ProviderSlotV1::Hydrology, "hydrology"),
        (ProviderSlotV1::Resources, "resources"),
        (ProviderSlotV1::Vegetation, "vegetation"),
        (ProviderSlotV1::BorealTerrain, "boreal"),
    ] {
        natural_offers.push(ProviderOfferV1::new(
            slot,
            ProviderGenerationIdentityV1::new(
                stable_id(&format!("fixture:worldgen-provider/{path}@1")),
                NonZeroU32::MIN,
                1,
                CanonicalHash::digest(format!("{path}-benchmark-implementation-v1")),
            ),
        ));
    }
    let mut input = GenerationPlanInputV1::new(
        "terrenia:dimension/terrenia"
            .parse()
            .expect("benchmark dimension is valid"),
        WorldSeedV1::from_integer(42),
        config,
        7,
        PlanActivationIdV1::from_hash(CanonicalHash::digest(b"benchmark-activation")),
        provider_offers(),
        bindings.d4_vocabulary().expect("D4 vocabulary"),
        bindings.role_bindings().expect("role bindings"),
        bindings.catalog_closure().expect("catalog"),
        CanonicalHash::digest(b"benchmark-semantic-image"),
        vec![CanonicalHash::digest(b"benchmark-lock")],
        WorldgenLimitsV1::default(),
    )
    .with_natural_layer(NaturalLayerInputV1::new(
        natural_config,
        bindings.natural_vocabulary().expect("natural vocabulary"),
        natural_offers,
    ));
    if let Some(terrain_config) = terrain_config {
        input = input.with_terrain_config(terrain_config);
    }
    if include_hydrology_occupancy {
        input = input.with_hydrology_occupancy(HydrologyOccupancyInputV1::new(
            HydrologyOccupancyConfigV1::default(),
            HydrologyFluidBindingsV1::new(
                stable_id("terrenia:fluid/water"),
                stable_id("terrenia:fluid/lava"),
                bindings
                    .predicate("place-water")
                    .expect("place-water predicate")
                    .clone(),
                bindings
                    .predicate("place-lava")
                    .expect("place-lava predicate")
                    .clone(),
            )
            .expect("benchmark hydrology fluids are valid"),
        ));
    }
    GenerationPlanV1::compile(input).expect("natural benchmark plan is valid")
}

fn v2_fixture_plan(preset: TerrainPresetV2) -> GenerationPlanV1 {
    let terrain = preset.resolve();
    natural_fixture_plan_with_config_and_terrain(
        WorldgenConfigV1 {
            chunk_edge_voxels: 32,
            world_floor_y: terrain.world.floor_y,
            world_ceiling_y: terrain.world.ceiling_y,
            ..WorldgenConfigV1::default()
        },
        NaturalLayerConfigV1::default(),
        false,
        Some(terrain),
    )
}

fn surface_chunk(plan: &GenerationPlanV1, chunk_x: i32, chunk_z: i32) -> ChunkCoordinate {
    let edge = i64::from(plan.config().chunk_edge_voxels);
    let world_x = i64::from(chunk_x)
        .saturating_mul(edge)
        .saturating_add(edge / 2);
    let world_z = i64::from(chunk_z)
        .saturating_mul(edge)
        .saturating_add(edge / 2);
    ChunkCoordinate::new(
        chunk_x,
        plan.terrain_height(world_x, world_z)
            .div_euclid(i32::from(plan.config().chunk_edge_voxels)),
        chunk_z,
    )
}

fn surface_chunk_with_style(plan: &GenerationPlanV1, expected: TerrainStyleV1) -> ChunkCoordinate {
    let edge = i64::from(plan.config().chunk_edge_voxels);
    for chunk_z in -128_i32..=128 {
        for chunk_x in -128_i32..=128 {
            let world_x = i64::from(chunk_x)
                .saturating_mul(edge)
                .saturating_add(edge / 2);
            let world_z = i64::from(chunk_z)
                .saturating_mul(edge)
                .saturating_add(edge / 2);
            if plan.territory_query(world_x, world_z).winner() == expected {
                return surface_chunk(plan, chunk_x, chunk_z);
            }
        }
    }
    panic!("benchmark corpus has no {expected:?} surface chunk")
}

fn stable_id(value: &str) -> StableId {
    value
        .parse()
        .unwrap_or_else(|error| panic!("invalid benchmark stable ID `{value}`: {error}"))
}

fn block_id(path: &str) -> StableId {
    stable_id(&format!("terrenia:block/{path}"))
}

criterion_group!(benches, generation_benchmarks);
criterion_main!(benches);
