//! Reproducible optimized semantic-field and density-composition diagnostics.

#![allow(
    clippy::expect_used,
    reason = "malformed package benchmark constants must abort before measurement"
)]

use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use latticeaxiom_core::CanonicalHash;
use latticeaxiom_terrenia_worldgen::{
    TerrainPresetV2, semantic_surface_biome_terrain_programs,
    semantic_surface_biome_terrain_programs_v1,
};
use latticeaxiom_worldgen::{
    SemanticTerrainFieldV1, SurfaceBiomeTerrainProgramV1, TerrainConfigV2, WorldSeedV1,
    WorldgenResult, WorldgenSeedRootV2,
};

fn semantic_field_and_density(c: &mut Criterion) {
    let terrain = TerrainPresetV2::Balanced.resolve();
    let fingerprint = CanonicalHash::digest("semantic-terrain-benchmark");
    let legacy = semantic_field(
        terrain,
        semantic_surface_biome_terrain_programs_v1(terrain, fingerprint),
        "legacy benchmark package policy is valid",
    );
    let current = semantic_field(
        terrain,
        semantic_surface_biome_terrain_programs(terrain, fingerprint),
        "current benchmark package policy is valid",
    );
    benchmark_field(c, "terrenia_semantic_terrain_provider_2", &legacy);
    benchmark_field(c, "terrenia_semantic_terrain_provider_3", &current);
}

fn semantic_field(
    terrain: TerrainConfigV2,
    programs: WorldgenResult<Vec<SurfaceBiomeTerrainProgramV1>>,
    invariant: &'static str,
) -> SemanticTerrainFieldV1 {
    let programs = programs.expect(invariant);
    let policy = programs[0]
        .semantic_policy()
        .expect("semantic program carries policy")
        .clone();
    SemanticTerrainFieldV1::new(
        WorldgenSeedRootV2::from_world_seed(WorldSeedV1::from_integer(0x51_7a)),
        terrain,
        policy,
    )
    .expect("benchmark field compiles")
}

fn benchmark_field(c: &mut Criterion, name: &str, field: &SemanticTerrainFieldV1) {
    let mut group = c.benchmark_group(name);
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
            for z in 0..32 {
                for x in 0..32 {
                    let column = field
                        .prepare_density_column(x, z, 64, false)
                        .expect("benchmark coordinate stays in the fixed envelope");
                    for y in 48..80 {
                        solid = solid.saturating_add(u32::from(
                            column
                                .density_at(y)
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
