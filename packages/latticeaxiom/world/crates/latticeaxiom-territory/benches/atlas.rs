//! Reproducible D7 Atlas cold, cached, parallel, and size baselines.

use std::{hint::black_box, num::NonZeroU32, thread};

use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use latticeaxiom_core::{CanonicalHash, StableId, canonical_json_bytes};
use latticeaxiom_territory::{
    AtlasConfigV1, AtlasScaleV1, AxisV1, CavePortalV1, CaveTopologyDomainIdV1,
    CaveTopologyParentV1, CoordinatorOfferV1, HydrologyPlanV1, PlanningCellBoundsV1,
    PlanningCellCoordinateV1, PortalHydrologyContractV1, PrimaryProviderOfferV1,
    ProviderGenerationIdentityV1, SurfaceTerritoryCandidateV1, TerritoryDomainIdV1,
    TerritoryLimitsV1, TerritoryPlanInputV1, TerritoryPlanV1, UndergroundTerritoryV1,
    VerticalRangeV1, WorldSeedV1,
};

fn nz(value: u32) -> NonZeroU32 {
    NonZeroU32::new(value)
        .unwrap_or_else(|| panic!("benchmark fixture expected a non-zero value, got {value}"))
}

fn stable(value: &str) -> StableId {
    value
        .parse()
        .unwrap_or_else(|error| panic!("invalid benchmark stable ID {value}: {error}"))
}

fn terrain_domain(name: &str) -> TerritoryDomainIdV1 {
    format!("latticeaxiom:territory-domain/{name}")
        .parse()
        .unwrap_or_else(|error| panic!("invalid benchmark terrain domain {name}: {error}"))
}

fn cave_domain(name: &str) -> CaveTopologyDomainIdV1 {
    format!("latticeaxiom:cave-topology-domain/{name}")
        .parse()
        .unwrap_or_else(|error| panic!("invalid benchmark cave domain {name}: {error}"))
}

fn provider(name: &str, revision: u32) -> ProviderGenerationIdentityV1 {
    ProviderGenerationIdentityV1::new(
        stable(&format!("latticeaxiom:provider/{name}")),
        nz(1),
        revision,
        CanonicalHash::digest(format!("{name}:{revision}")),
    )
}

#[allow(
    clippy::too_many_lines,
    reason = "benchmark fixture keeps the persisted contract explicit"
)]
fn benchmark_input(candidate_count: usize) -> TerritoryPlanInputV1 {
    let limits = TerritoryLimitsV1::default();
    let default_terrain = terrain_domain("terrenia");
    let woodland = terrain_domain("woodland");
    let badlands = terrain_domain("badlands");
    let meadow = terrain_domain("meadow");
    let default_cave = cave_domain("default");
    let limestone = cave_domain("limestone");
    let crystal = cave_domain("crystal");
    let surface_candidates = (0..candidate_count.max(3))
        .map(|index| {
            let level = u8::try_from(index % 3)
                .unwrap_or_else(|error| panic!("benchmark level conversion failed: {error}"));
            let (domain, parent) = match level {
                0 => (woodland.clone(), default_terrain.clone()),
                1 => (badlands.clone(), woodland.clone()),
                2 => (meadow.clone(), badlands.clone()),
                _ => panic!("benchmark produced an impossible Atlas level"),
            };
            let coordinate = i64::try_from(index)
                .unwrap_or_else(|error| panic!("benchmark coordinate conversion failed: {error}"));
            SurfaceTerritoryCandidateV1::new(
                stable(&format!("latticeaxiom:territory-candidate/c{index}")),
                domain,
                parent,
                level,
                PlanningCellCoordinateV1::new(coordinate, -coordinate),
                nz(1 + u32::try_from(index % 7).unwrap_or_default()),
            )
            .unwrap_or_else(|error| panic!("benchmark candidate was rejected: {error}"))
        })
        .collect();
    let cave_portals = vec![
        CavePortalV1::new(
            limestone.clone(),
            PlanningCellCoordinateV1::new(0, 0),
            default_cave.clone(),
            PlanningCellCoordinateV1::new(-1, 0),
            [0, -32_000, 0],
            AxisV1::Y,
            2_000,
            3_000,
            PortalHydrologyContractV1::Dry,
        )
        .unwrap_or_else(|error| panic!("benchmark portal was rejected: {error}")),
        CavePortalV1::new(
            crystal.clone(),
            PlanningCellCoordinateV1::new(7, 1),
            default_cave.clone(),
            PlanningCellCoordinateV1::new(8, 1),
            [128_000, -32_000, 16_000],
            AxisV1::Y,
            2_000,
            3_000,
            PortalHydrologyContractV1::Sealed,
        )
        .unwrap_or_else(|error| panic!("benchmark portal was rejected: {error}")),
    ];
    TerritoryPlanInputV1 {
        dimension: "latticeaxiom:dimension/terrenia"
            .parse()
            .unwrap_or_else(|error| panic!("invalid benchmark dimension: {error}")),
        world_seed: WorldSeedV1::from_text("Terrenia D7 benchmark"),
        atlas: AtlasConfigV1::new(
            vec![
                AtlasScaleV1::new(0, nz(64)),
                AtlasScaleV1::new(1, nz(8)),
                AtlasScaleV1::new(2, nz(1)),
            ],
            nz(2),
        )
        .unwrap_or_else(|error| panic!("benchmark Atlas config was rejected: {error}")),
        default_terrain_domain: default_terrain.clone(),
        default_cave_domain: default_cave.clone(),
        coordinators: vec![CoordinatorOfferV1::new(provider("coordinator", 1))],
        primary_offers: vec![
            PrimaryProviderOfferV1::terrain(default_terrain, provider("terrain", 1)),
            PrimaryProviderOfferV1::cave(default_cave, provider("cave", 1)),
        ],
        surface_candidates,
        underground_territories: vec![
            UndergroundTerritoryV1::new(
                limestone,
                CaveTopologyParentV1::DimensionDefault,
                PlanningCellBoundsV1::new(0, 0, 4, 4)
                    .unwrap_or_else(|error| panic!("benchmark bounds were rejected: {error}")),
                VerticalRangeV1::new(-64, -16)
                    .unwrap_or_else(|error| panic!("benchmark range was rejected: {error}")),
            ),
            UndergroundTerritoryV1::new(
                crystal,
                CaveTopologyParentV1::DimensionDefault,
                PlanningCellBoundsV1::new(4, 0, 8, 4)
                    .unwrap_or_else(|error| panic!("benchmark bounds were rejected: {error}")),
                VerticalRangeV1::new(-64, -16)
                    .unwrap_or_else(|error| panic!("benchmark range was rejected: {error}")),
            ),
        ],
        contributions: Vec::new(),
        cave_portals,
        hydrology: HydrologyPlanV1::compile(Vec::new(), Vec::new(), limits)
            .unwrap_or_else(|error| panic!("benchmark hydrology was rejected: {error}")),
        limits,
    }
}

fn compile_benchmark_plan() -> TerritoryPlanV1 {
    TerritoryPlanV1::compile(benchmark_input(1_000))
        .unwrap_or_else(|error| panic!("benchmark plan was rejected: {error}"))
}

fn atlas_benchmarks(criterion: &mut Criterion) {
    let mut cold = criterion.benchmark_group("atlas_cold");
    cold.throughput(Throughput::Elements(1_000));
    cold.bench_function("compile_1000_candidates", |bencher| {
        bencher.iter(|| {
            black_box(
                TerritoryPlanV1::compile(black_box(benchmark_input(1_000)))
                    .unwrap_or_else(|error| panic!("benchmark compile failed: {error}")),
            );
        });
    });
    cold.finish();

    let plan = compile_benchmark_plan();
    let mut cached = criterion.benchmark_group("atlas_cached");
    cached.throughput(Throughput::Elements(4_096));
    cached.bench_function("query_4096_cells", |bencher| {
        bencher.iter(|| {
            for index in 0_i64..4_096 {
                black_box(plan.query(PlanningCellCoordinateV1::new(index, -index)));
            }
        });
    });
    cached.bench_function("parallel_query_4096_cells", |bencher| {
        bencher.iter(|| {
            thread::scope(|scope| {
                for worker in 0_i64..4 {
                    let plan = &plan;
                    scope.spawn(move || {
                        for index in 0_i64..1_024 {
                            let coordinate = worker * 1_024 + index;
                            black_box(
                                plan.query(PlanningCellCoordinateV1::new(coordinate, -coordinate)),
                            );
                        }
                    });
                }
            });
        });
    });
    cached.finish();

    let serialized = canonical_json_bytes(&plan)
        .unwrap_or_else(|error| panic!("benchmark plan serialization failed: {error}"));
    let serialized_size = u64::try_from(serialized.len())
        .unwrap_or_else(|error| panic!("serialized-size conversion failed: {error}"));
    let mut size = criterion.benchmark_group("atlas_size");
    size.throughput(Throughput::Bytes(serialized_size));
    size.bench_function("canonical_plan_bytes", |bencher| {
        bencher.iter(|| black_box(serialized.as_slice()));
    });
    size.finish();
}

criterion_group!(benches, atlas_benchmarks);
criterion_main!(benches);
