//! Reproducible cold, cached, and Bevy task-pool domain-planning diagnostics.

#![allow(
    clippy::expect_used,
    reason = "a malformed benchmark fixture must abort before measurement"
)]

use std::{num::NonZeroU16, num::NonZeroU32, str::FromStr, sync::Arc};

use bevy::tasks::TaskPoolBuilder;
use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use latticeaxiom_core::CanonicalHash;
use latticeaxiom_worldgen::{
    DimensionId, GenerationEpochIdV1, HydrologicDomainCacheV1, HydrologicDomainConfigV1,
    HydrologicDomainGridV1, HydrologicDomainInputV1, plan_hydrologic_domain_v1,
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

criterion_group!(benches, cold_and_cached, task_pool_scaling);
criterion_main!(benches);
