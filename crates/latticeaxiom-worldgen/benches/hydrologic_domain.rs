//! Reproducible cold, cached, and Bevy task-pool domain-planning diagnostics.

#![allow(
    clippy::expect_used,
    reason = "a malformed benchmark fixture must abort before measurement"
)]

use std::{num::NonZeroU16, num::NonZeroU32, str::FromStr, sync::Arc};

use bevy::tasks::TaskPoolBuilder;
use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use latticeaxiom_core::{CanonicalHash, StableId};
use latticeaxiom_storage::ChunkCoordinate;
use latticeaxiom_worldgen::{
    DimensionId, GenerationEpochIdV1, HydrologicDomainCacheV1, HydrologicDomainConfigV1,
    HydrologicDomainGridV1, HydrologicDomainInputV1, HydrologicTopologyConfigV1,
    LandscapeEvolutionConfigV1, LandscapeEvolutionInputV1, StaticReservoirSamplerV1,
    build_hydrologic_topology_v1, evolve_hydrologic_landscape_v1, plan_hydrologic_domain_v1,
    plan_hydrologic_domains_parallel_v1,
};

fn fixture(edge: u16, spacing: u32, halo: u16, domain_x: i64) -> HydrologicDomainInputV1 {
    let mut elevations = Vec::with_capacity(usize::from(edge) * usize::from(edge));
    let center = i32::from(edge / 2);
    for z in 0..edge {
        for x in 0..edge {
            let dx = i32::from(x) - center;
            let dz = i32::from(z) - center;
            elevations.push(
                40_000 - i32::from(x) * 29 - i32::from(z) * 17
                    + (dx * dx + dz * dz).rem_euclid(257),
            );
        }
    }
    HydrologicDomainInputV1::new(
        DimensionId::from_str("latticeaxiom:dimension/terrenia")
            .expect("benchmark dimension identity is valid"),
        GenerationEpochIdV1::from_hash(CanonicalHash::digest("hydrology-benchmark-epoch")),
        domain_x,
        0,
        None,
        CanonicalHash::digest("hydrology-benchmark-provenance"),
        HydrologicDomainGridV1::new(
            domain_x.saturating_mul(i64::from(edge) * i64::from(spacing)),
            0,
            NonZeroU32::new(spacing).expect("benchmark spacing is nonzero"),
            NonZeroU16::new(edge).expect("benchmark edge is nonzero"),
            NonZeroU16::new(edge).expect("benchmark edge is nonzero"),
            halo,
        ),
        i32::MIN,
        elevations,
        vec![65_536; usize::from(edge) * usize::from(edge)],
        Vec::new(),
        HydrologicDomainConfigV1::default(),
    )
    .expect("benchmark domain input is valid")
}

fn cold_and_cached(c: &mut Criterion) {
    let mut group = c.benchmark_group("hydrologic_domain_v1");
    for (edge, spacing, halo) in [(64_u16, 8_u32, 8_u16), (96, 16, 16), (128, 32, 24)] {
        let input = fixture(edge, spacing, halo, 0);
        group.throughput(Throughput::Elements(u64::from(edge) * u64::from(edge)));
        group.bench_with_input(
            BenchmarkId::new("cold_priority_flood_mfd", edge),
            &input,
            |bencher, input| {
                bencher.iter(|| plan_hydrologic_domain_v1(input).expect("benchmark plan is valid"));
            },
        );

        let key = input.cache_key().expect("benchmark key canonicalizes");
        let plan = Arc::new(plan_hydrologic_domain_v1(&input).expect("benchmark plan is valid"));
        let bytes = u64::try_from(
            plan.canonical_bytes()
                .expect("benchmark plan canonicalizes")
                .len(),
        )
        .expect("benchmark byte count fits u64");
        let mut cache =
            HydrologicDomainCacheV1::new(1, bytes).expect("benchmark cache bounds are valid");
        cache
            .insert(key.clone(), plan)
            .expect("benchmark cache entry fits");
        group.bench_with_input(
            BenchmarkId::new("exact_cache_hit", edge),
            &key,
            |bencher, key| {
                bencher.iter(|| {
                    cache
                        .get(key)
                        .expect("benchmark cache entry remains resident")
                });
            },
        );
    }
    group.finish();
}

fn task_pool_scaling(c: &mut Criterion) {
    let inputs = (0..8_i64)
        .map(|domain_x| fixture(64, 16, 16, domain_x))
        .collect::<Vec<_>>();
    let mut group = c.benchmark_group("hydrologic_domain_parallel_v1");
    group.throughput(Throughput::Elements(8 * 64 * 64));
    for threads in [1_usize, 2, 4] {
        let pool = TaskPoolBuilder::new().num_threads(threads).build();
        group.bench_with_input(
            BenchmarkId::new("eight_domains", threads),
            &threads,
            |bencher, _| {
                bencher.iter(|| plan_hydrologic_domains_parallel_v1(&pool, inputs.clone()));
            },
        );
    }
    group.finish();
}

fn topology_and_materialization(c: &mut Criterion) {
    let domain =
        plan_hydrologic_domain_v1(&fixture(64, 8, 8, 0)).expect("benchmark domain plan is valid");
    let config = HydrologicTopologyConfigV1::default();
    let topology =
        build_hydrologic_topology_v1(&domain, &config).expect("benchmark topology is valid");
    let water =
        StableId::from_str("latticeaxiom:fluid/water").expect("benchmark water identity is valid");
    let sampler = StaticReservoirSamplerV1::new(&domain, &topology, &config)
        .expect("benchmark sampler binds");
    let mut group = c.benchmark_group("hydrologic_topology_v1");
    group.throughput(Throughput::Elements(64 * 64));
    group.bench_function("extract_64", |bencher| {
        bencher.iter(|| {
            build_hydrologic_topology_v1(&domain, &config)
                .expect("benchmark topology remains valid")
        });
    });
    group.throughput(Throughput::Elements(16 * 16));
    group.bench_function("materialize_16_cube", |bencher| {
        bencher.iter(|| {
            sampler
                .materialize(
                    ChunkCoordinate::new(0, 9, 0),
                    NonZeroU16::new(16).expect("benchmark chunk edge is nonzero"),
                    0,
                    255,
                    &water,
                )
                .expect("benchmark reservoir remains valid")
        });
    });
    group.finish();
}

fn landscape_evolution(c: &mut Criterion) {
    let input = LandscapeEvolutionInputV1::new(
        fixture(64, 8, 8, 0),
        HydrologicTopologyConfigV1::default(),
        LandscapeEvolutionConfigV1::new(
            2,
            1,
            2,
            65_536,
            0,
            1 << 22,
            4_096,
            0,
            100_000_000,
            512 * 1024 * 1024,
            1_024 * 1024 * 1024,
        ),
    );
    let mut group = c.benchmark_group("landscape_evolution_v1");
    group.throughput(Throughput::Elements(64 * 64));
    group.bench_function("two_iterations_64", |bencher| {
        bencher.iter(|| {
            evolve_hydrologic_landscape_v1(&input)
                .expect("benchmark landscape evolution remains valid")
        });
    });
    group.finish();
}

criterion_group!(
    benches,
    cold_and_cached,
    task_pool_scaling,
    topology_and_materialization,
    landscape_evolution
);
criterion_main!(benches);
