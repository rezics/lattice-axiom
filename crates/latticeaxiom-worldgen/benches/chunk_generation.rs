//! Reproducible CPU diagnostics for deterministic D4 chunk generation.

#![allow(
    clippy::expect_used,
    reason = "a malformed benchmark fixture must abort before measurement"
)]

use std::{hint::black_box, num::NonZeroU32};

use criterion::{Criterion, criterion_group, criterion_main};
use latticeaxiom_core::{CanonicalHash, StableId};
use latticeaxiom_worldgen::{
    AdjacentEpochSnapshotV1, AuthoredWorldgenBindingsV1, CellEpochStateV1, ChunkCoordinate,
    ChunkGenerationRequestV1, D4BlockCatalogClosureV1, D4MaterialRoleV1, D4RoleVocabularyV1,
    DimensionId, FrozenRoleBindingsV1, GenerationPlanInputV1, GenerationPlanV1,
    NaturalLayerConfigV1, NaturalLayerInputV1, PlanActivationIdV1, PlanningCellCoordinateV1,
    ProviderGenerationIdentityV1, ProviderOfferV1, ProviderSlotV1, WorldSeedV1, WorldgenConfigV1,
    WorldgenLimitsV1,
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
    let adjacent = AdjacentEpochSnapshotV1::all_unassigned(PlanningCellCoordinateV1::new(-1, 0))
        .expect("benchmark adjacency is representable");
    c.bench_function("d4_chunk_16_cubic_snapshot_candidate", |bencher| {
        bencher.iter(|| {
            plan.generate(ChunkGenerationRequestV1::new(
                black_box(ChunkCoordinate::new(-3, 1, 5)),
                None,
                CellEpochStateV1::Unassigned,
                adjacent.clone(),
                Vec::new(),
            ))
            .expect("benchmark chunk must remain valid")
        });
    });
    let natural = natural_fixture_plan();
    c.bench_function("v5_natural_chunk_16_cubic_snapshot_candidate", |bencher| {
        bencher.iter(|| {
            natural
                .generate(ChunkGenerationRequestV1::new(
                    black_box(ChunkCoordinate::new(-3, 1, 5)),
                    None,
                    CellEpochStateV1::Unassigned,
                    adjacent.clone(),
                    Vec::new(),
                ))
                .expect("natural benchmark chunk must remain valid")
        });
    });
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
}

fn fixture_plan() -> GenerationPlanV1 {
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
    GenerationPlanV1::compile(GenerationPlanInputV1::new(
        dimension,
        WorldSeedV1::from_integer(42),
        WorldgenConfigV1::default(),
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
    GenerationPlanV1::compile(
        GenerationPlanInputV1::new(
            "terrenia:dimension/terrenia"
                .parse()
                .expect("benchmark dimension is valid"),
            WorldSeedV1::from_integer(42),
            WorldgenConfigV1::default(),
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
            NaturalLayerConfigV1::default(),
            bindings.natural_vocabulary().expect("natural vocabulary"),
            natural_offers,
        )),
    )
    .expect("natural benchmark plan is valid")
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
