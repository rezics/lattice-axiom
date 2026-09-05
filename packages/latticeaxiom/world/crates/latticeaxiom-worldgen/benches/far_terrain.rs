//! Reproducible CPU and retained-memory evidence for derived far-terrain tiles.

#![allow(
    clippy::expect_used,
    reason = "a malformed benchmark fixture must abort before measurement"
)]

use std::hint::black_box;

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use latticeaxiom_core::CanonicalHash;
use latticeaxiom_storage::{ChunkRevision, DimensionId};
use latticeaxiom_worldgen::{
    D4MaterialRoleV1, FarTerrainCommittedProvenanceV1, FarTerrainLodLevelV1,
    FarTerrainSourceProvenanceV1, FarTerrainSurfaceSampleV1, FarTerrainSurfaceSourceV1,
    FarTerrainTileAddressV1, FarTerrainTileCoordinateV1, SnapshotChecksumV1, WorldgenResult,
    build_far_terrain_tile_v1,
};

struct BenchmarkSurface;

impl FarTerrainSurfaceSourceV1 for BenchmarkSurface {
    fn sample_far_terrain_surface(
        &self,
        world_x: i64,
        world_z: i64,
    ) -> WorldgenResult<FarTerrainSurfaceSampleV1> {
        let ridge = if world_x.rem_euclid(96) >= 48 { 18 } else { 0 };
        let height = world_x
            .div_euclid(17)
            .saturating_add(world_z.div_euclid(23))
            .saturating_add(ridge);
        let height = i32::try_from(height).unwrap_or_else(|_| {
            if height.is_negative() {
                i32::MIN
            } else {
                i32::MAX
            }
        });
        Ok(FarTerrainSurfaceSampleV1::new(
            height,
            if ridge == 0 {
                D4MaterialRoleV1::TemperateSurface
            } else {
                D4MaterialRoleV1::TemperateBaseRock
            },
            (height < 4).then_some(4),
        ))
    }
}

fn provenance() -> FarTerrainSourceProvenanceV1 {
    FarTerrainSourceProvenanceV1::Committed(FarTerrainCommittedProvenanceV1::new(
        "terrenia:dimension/terrenia"
            .parse::<DimensionId>()
            .expect("benchmark dimension is valid"),
        ChunkRevision::new(1),
        SnapshotChecksumV1::from_hash(CanonicalHash::digest(b"benchmark-surface-v1")),
    ))
}

fn far_tile_generation(c: &mut Criterion) {
    let mut group = c.benchmark_group("far_terrain_tile_v1");
    for lod in [0_u8, 1, 2] {
        let level = FarTerrainLodLevelV1::new(lod).expect("benchmark LOD is bounded");
        let address = FarTerrainTileAddressV1::new(FarTerrainTileCoordinateV1::new(-3, 2), level);
        let representative =
            build_far_terrain_tile_v1(&BenchmarkSurface, provenance(), address, 32)
                .expect("representative far tile builds");
        let full_chunk_bytes = representative
            .covered_columns()
            .saturating_mul(7)
            .saturating_mul(32)
            .saturating_mul(2);
        let retained_bytes = representative.retained_vector_bytes();
        assert!(
            u64::try_from(retained_bytes).unwrap_or(u64::MAX) < full_chunk_bytes,
            "far tile retained vectors must remain smaller than two-byte indices for seven full chunks per covered column"
        );
        group.throughput(Throughput::Elements(representative.covered_columns()));
        group.bench_with_input(
            BenchmarkId::from_parameter(format!(
                "lod{lod}-retained{retained_bytes}B-baseline{full_chunk_bytes}B"
            )),
            &address,
            |bencher, address| {
                bencher.iter(|| {
                    black_box(
                        build_far_terrain_tile_v1(
                            &BenchmarkSurface,
                            provenance(),
                            black_box(*address),
                            32,
                        )
                        .expect("benchmark far tile builds"),
                    )
                });
            },
        );
    }
    group.finish();
}

criterion_group!(benches, far_tile_generation);
criterion_main!(benches);
