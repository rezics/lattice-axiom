//! Deterministic Criterion coverage for the reusable CPU greedy-meshing path.

#![expect(
    clippy::expect_used,
    reason = "fixed benchmark fixture invariants must abort the run when violated"
)]

use std::{hint::black_box, sync::Arc};

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use latticeaxiom_voxel_mesh::{
    ChunkCoordinate, Face, FaceDescriptor, FaceOcclusion, GreedyMesher, MeshBuffer, MeshGroup,
    MeshSource, PaddedChunk, SourceEpoch, SourceFingerprint, SourceRevision, Voxel,
};

const EDGE: usize = 32;

#[derive(Clone, Copy, Debug)]
enum Corpus {
    Solid,
    Layered,
    Checker,
}

#[derive(Clone, Copy, Debug)]
struct BenchVoxel {
    material: Option<u8>,
    group: MeshGroup,
    occlusion: FaceOcclusion,
}

impl BenchVoxel {
    const AIR: Self = Self {
        material: None,
        group: MeshGroup::Opaque,
        occlusion: FaceOcclusion::None,
    };

    const fn solid(material: u8, group: MeshGroup) -> Self {
        Self {
            material: Some(material),
            group,
            occlusion: FaceOcclusion::Full,
        }
    }
}

impl Voxel for BenchVoxel {
    type MergeKey = u8;

    fn face(&self, _face: Face) -> Option<FaceDescriptor<Self::MergeKey>> {
        self.material
            .map(|material| FaceDescriptor::new(self.group, material, self.occlusion))
    }
}

fn benchmark_meshing(criterion: &mut Criterion) {
    let dimensions = PaddedChunk::new([EDGE; 3]).expect("benchmark dimensions are valid");
    let source = MeshSource::new(
        ChunkCoordinate::new(-5, 2, -7),
        SourceEpoch::new(3),
        SourceRevision::new(11),
        SourceFingerprint::new([19; 32]),
    );
    let cells = u64::try_from(EDGE * EDGE * EDGE).expect("benchmark cell count fits u64");
    let mut group = criterion.benchmark_group("greedy_meshing_32_cubed_plus_halo");
    group.throughput(Throughput::Elements(cells));

    for (name, corpus, expected_quads) in [
        ("solid", Corpus::Solid, 6),
        ("layered", Corpus::Layered, 18),
        ("checker", Corpus::Checker, 98_304),
    ] {
        // Input construction and the first capacity warm-up stay outside the
        // timed closure. Both scratch and output allocations are then reused.
        let voxels = build_corpus(dimensions, corpus);
        let mut mesher = GreedyMesher::new();
        let mut output = MeshBuffer::default();
        mesher
            .mesh_into(&voxels, dimensions, source, &mut output)
            .expect("benchmark corpus length matches dimensions");
        assert_eq!(output.quad_count(), expected_quads, "{name} guard");

        group.bench_with_input(
            BenchmarkId::from_parameter(name),
            &voxels,
            |bencher, input| {
                bencher.iter(|| {
                    let receipt = mesher
                        .mesh_into(black_box(input.as_slice()), dimensions, source, &mut output)
                        .expect("prevalidated benchmark input remains valid");
                    black_box(receipt);
                    black_box(output.quad_count());
                });
            },
        );
        assert_eq!(output.quad_count(), expected_quads, "{name} guard");
    }

    group.finish();
}

fn benchmark_geometry_handoff(criterion: &mut Criterion) {
    let dimensions = PaddedChunk::new([EDGE; 3]).expect("benchmark dimensions are valid");
    let source = MeshSource::new(
        ChunkCoordinate::new(-5, 2, -7),
        SourceEpoch::new(3),
        SourceRevision::new(11),
        SourceFingerprint::new([19; 32]),
    );
    let voxels = build_corpus(dimensions, Corpus::Checker);
    let mut mesher = GreedyMesher::new();
    let mut geometry = MeshBuffer::default();
    mesher
        .mesh_into(&voxels, dimensions, source, &mut geometry)
        .expect("benchmark corpus length matches dimensions");
    assert_eq!(geometry.quad_count(), 98_304, "checker guard");

    // These are the old owned and current shared presentation handoffs over
    // identical worst-case chunk geometry; fixture construction is untimed.
    let shared = Arc::new(geometry.clone());
    let mut group = criterion.benchmark_group("mesh_presentation_handoff/checker_98304_quads");
    group.throughput(Throughput::Elements(98_304));
    group.bench_function("owned_deep_clone", |bencher| {
        bencher.iter(|| black_box(geometry.clone()));
    });
    group.bench_function("shared_arc_clone", |bencher| {
        bencher.iter(|| black_box(Arc::clone(&shared)));
    });
    group.finish();
}

fn build_corpus(dimensions: PaddedChunk, corpus: Corpus) -> Vec<BenchVoxel> {
    let mut voxels = vec![BenchVoxel::AIR; dimensions.volume_len()];
    for z in 0..EDGE {
        for y in 0..EDGE {
            for x in 0..EDGE {
                let voxel = match corpus {
                    Corpus::Solid => BenchVoxel::solid(1, MeshGroup::Opaque),
                    Corpus::Layered => {
                        let layer = y / 8;
                        BenchVoxel::solid(
                            u8::try_from(layer + 1).expect("four layers fit u8"),
                            MeshGroup::ALL[layer],
                        )
                    }
                    Corpus::Checker if (x + y + z) % 2 == 0 => {
                        BenchVoxel::solid(1, MeshGroup::Opaque)
                    }
                    Corpus::Checker => BenchVoxel::AIR,
                };
                let index = dimensions
                    .linearize([x + 1, y + 1, z + 1])
                    .expect("benchmark interior coordinate is padded in bounds");
                voxels[index] = voxel;
            }
        }
    }
    voxels
}

criterion_group!(benches, benchmark_meshing, benchmark_geometry_handoff);
criterion_main!(benches);
