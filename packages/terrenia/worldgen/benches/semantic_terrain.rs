//! Reproducible optimized semantic-field and density-composition diagnostics.

#![allow(
    clippy::expect_used,
    reason = "malformed package benchmark constants must abort before measurement"
)]

use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use latticeaxiom_core::CanonicalHash;
use latticeaxiom_terrenia_worldgen::{TerrainPresetV2, semantic_surface_biome_terrain_programs};
use latticeaxiom_worldgen::{SemanticTerrainFieldV1, WorldSeedV1, WorldgenSeedRootV2};

fn semantic_field_and_density(c: &mut Criterion) {
    let terrain = TerrainPresetV2::Balanced.resolve();
    let programs = semantic_surface_biome_terrain_programs(
        terrain,
        CanonicalHash::digest("semantic-terrain-benchmark"),
    )
    .expect("benchmark package policy is valid");
    let policy = programs[0]
        .semantic_policy()
        .expect("semantic program carries policy")
        .clone();
    let field = SemanticTerrainFieldV1::new(
        WorldgenSeedRootV2::from_world_seed(WorldSeedV1::from_integer(0x51_7a)),
        terrain,
        policy,
    )
    .expect("benchmark field compiles");
    let mut group = c.benchmark_group("terrenia_semantic_terrain_v1");
    group.throughput(Throughput::Elements(1));
    group.bench_function("column", |bencher| {
        let mut coordinate = 0_i64;
        bencher.iter(|| {
            coordinate = coordinate.wrapping_add(17);
            field
                .sample(coordinate, coordinate.wrapping_mul(-3))
                .expect("benchmark coordinate stays in the fixed envelope")
        });
    });
    group.throughput(Throughput::Elements(32 * 32 * 32));
    group.bench_function("density_32_cube", |bencher| {
        bencher.iter(|| {
            let mut solid = 0_u32;
            for y in 48..80 {
                for z in 0..32 {
                    for x in 0..32 {
                        solid = solid.saturating_add(u32::from(
                            field
                                .density(x, y, z, 64, false)
                                .expect("benchmark coordinate stays in the fixed envelope")
                                .final_density_q8()
                                >= 0,
                        ));
                    }
                }
            }
            solid
        });
    });
    group.finish();
}

criterion_group!(benches, semantic_field_and_density);
criterion_main!(benches);
