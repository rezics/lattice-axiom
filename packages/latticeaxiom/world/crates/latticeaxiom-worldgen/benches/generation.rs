//! Reproducible CPU diagnostics for deterministic world-generation primitives.

use std::{hint::black_box, num::NonZeroU32};

use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use latticeaxiom_worldgen::{
    FixedCoordinateV1, WorldSeedV1, open_simplex_2f_3d_v1, open_simplex_2s_2d_v1,
};

fn seed_derivation(c: &mut Criterion) {
    c.bench_function("world_seed_integer_v1", |bencher| {
        bencher.iter(|| WorldSeedV1::from_integer(42));
    });
}

fn fixed_field_candidates(c: &mut Criterion) {
    let scale = NonZeroU32::new(64).unwrap_or(NonZeroU32::MIN);
    let mut points_2d = Vec::with_capacity(256 * 256);
    for z in -128_i64..128 {
        for x in -128_i64..128 {
            let Ok(x) = FixedCoordinateV1::from_voxel(x, scale) else {
                panic!("benchmark X coordinate must fit the fixed field envelope");
            };
            let Ok(z) = FixedCoordinateV1::from_voxel(z, scale) else {
                panic!("benchmark Z coordinate must fit the fixed field envelope");
            };
            points_2d.push((x, z));
        }
    }
    let mut group = c.benchmark_group("fixed_field_candidates_v1");
    group.throughput(Throughput::Elements(
        u64::try_from(points_2d.len()).unwrap_or(u64::MAX),
    ));
    group.bench_function("opensimplex2s_2d_256_squared", |bencher| {
        bencher.iter(|| {
            for &(x, z) in black_box(&points_2d) {
                let _ = black_box(open_simplex_2s_2d_v1(0x5eed, x, z));
            }
        });
    });

    let mut points_3d = Vec::with_capacity(32 * 32 * 32);
    for y in -16_i64..16 {
        for z in -16_i64..16 {
            for x in -16_i64..16 {
                let Ok(x) = FixedCoordinateV1::from_voxel(x, scale) else {
                    panic!("benchmark X coordinate must fit the fixed field envelope");
                };
                let Ok(y) = FixedCoordinateV1::from_voxel(y, scale) else {
                    panic!("benchmark Y coordinate must fit the fixed field envelope");
                };
                let Ok(z) = FixedCoordinateV1::from_voxel(z, scale) else {
                    panic!("benchmark Z coordinate must fit the fixed field envelope");
                };
                points_3d.push((x, y, z));
            }
        }
    }
    group.throughput(Throughput::Elements(
        u64::try_from(points_3d.len()).unwrap_or(u64::MAX),
    ));
    group.bench_function("opensimplex2f_3d_32_cubed", |bencher| {
        bencher.iter(|| {
            for &(x, y, z) in black_box(&points_3d) {
                let _ = black_box(open_simplex_2f_3d_v1(0x5eed, x, y, z));
            }
        });
    });
    group.finish();
}

criterion_group!(benches, seed_derivation, fixed_field_candidates);
criterion_main!(benches);
