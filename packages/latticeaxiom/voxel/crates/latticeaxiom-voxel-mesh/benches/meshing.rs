//! Deterministic Criterion coverage for the reusable CPU greedy-meshing path.

#![expect(
    clippy::expect_used,
    reason = "fixed benchmark fixture invariants must abort the run when violated"
)]

use std::{hint::black_box, sync::Arc};

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use latticeaxiom_voxel_mesh::{
    ChunkCoordinate, Face, FaceDescriptor, FaceOcclusion, FluidMeshFlow, FluidMeshIdentity,
    FluidMeshLevel, FluidSurfaceDescriptor, GreedyMesher, MeshBuffer, MeshGroup, MeshSource,
    PaddedChunk, SourceEpoch, SourceFingerprint, SourceRevision, Voxel,
};

const EDGE: usize = 32;
type BlockMeshShape = block_mesh::ndshape::ConstShape3u32<34, 34, 34>;

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

#[derive(Clone, Copy, Debug, Default)]
struct BenchFluidVoxel {
    level: Option<FluidMeshLevel>,
    flow: FluidMeshFlow,
}

impl BenchFluidVoxel {
    fn water(level: u8, flow: FluidMeshFlow) -> Self {
        Self {
            level: Some(FluidMeshLevel::new(level).expect("benchmark fluid level is valid")),
            flow,
        }
    }
}

impl Voxel for BenchFluidVoxel {
    type MergeKey = u8;

    fn face(&self, _face: Face) -> Option<FaceDescriptor<Self::MergeKey>> {
        None
    }

    fn fluid_surface(&self, face: Face) -> Option<FluidSurfaceDescriptor<Self::MergeKey>> {
        self.level.map(|level| {
            FluidSurfaceDescriptor::new(
                FluidMeshIdentity::new(1),
                level,
                self.flow,
                u8::try_from(face.index()).expect("six face indices fit u8"),
            )
        })
    }
}

impl block_mesh::Voxel for BenchVoxel {
    fn get_visibility(&self) -> block_mesh::VoxelVisibility {
        if self.material.is_some() {
            block_mesh::VoxelVisibility::Opaque
        } else {
            block_mesh::VoxelVisibility::Empty
        }
    }
}

impl block_mesh::MergeVoxel for BenchVoxel {
    type MergeValue = (Option<u8>, MeshGroup);

    fn merge_value(&self) -> Self::MergeValue {
        (self.material, self.group)
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
            BenchmarkId::new("latticeaxiom", name),
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

        let shape = BlockMeshShape {};
        let mut upstream = block_mesh::GreedyQuadsBuffer::new(voxels.len());
        block_mesh::greedy_quads(
            &voxels,
            &shape,
            [0; 3],
            [33; 3],
            &block_mesh::RIGHT_HANDED_Y_UP_CONFIG.faces,
            &mut upstream,
        );
        assert_eq!(
            upstream.quads.num_quads(),
            expected_quads,
            "upstream {name} guard"
        );
        group.bench_with_input(
            BenchmarkId::new("block-mesh", name),
            &voxels,
            |bencher, input| {
                bencher.iter(|| {
                    block_mesh::greedy_quads(
                        black_box(input.as_slice()),
                        &shape,
                        [0; 3],
                        [33; 3],
                        &block_mesh::RIGHT_HANDED_Y_UP_CONFIG.faces,
                        &mut upstream,
                    );
                    black_box(upstream.quads.num_quads());
                });
            },
        );
        assert_eq!(
            upstream.quads.num_quads(),
            expected_quads,
            "upstream {name} guard"
        );
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

fn benchmark_fluid_meshing(criterion: &mut Criterion) {
    let dimensions = PaddedChunk::new([EDGE; 3]).expect("benchmark dimensions are valid");
    let source = MeshSource::new(
        ChunkCoordinate::new(-5, 2, -7),
        SourceEpoch::new(3),
        SourceRevision::new(11),
        SourceFingerprint::new([19; 32]),
    );
    let mut voxels = vec![BenchFluidVoxel::default(); dimensions.volume_len()];
    for z in 0..EDGE {
        for x in 0..EDGE {
            let index = dimensions
                .linearize([x + 1, 1, z + 1])
                .expect("benchmark surface coordinate is padded in bounds");
            let level = u8::try_from((x + 3 * z) % 8).expect("modulo eight fits u8");
            let flow = match (x + z) % 4 {
                0 => FluidMeshFlow::East,
                1 => FluidMeshFlow::South,
                2 => FluidMeshFlow::West,
                _ => FluidMeshFlow::North,
            };
            voxels[index] = BenchFluidVoxel::water(level, flow);
        }
    }

    let mut mesher = GreedyMesher::new();
    let mut output = MeshBuffer::default();
    mesher
        .mesh_into(&voxels, dimensions, source, &mut output)
        .expect("benchmark corpus length matches dimensions");
    assert_eq!(output.quad_count(), 1_152, "water surface guard");
    assert_eq!(output.vertex_count(), 4_608, "water vertex guard");
    assert_eq!(output.index_count(), 6_912, "water index guard");

    let cells = u64::try_from(EDGE * EDGE * EDGE).expect("benchmark cell count fits u64");
    let mut group = criterion.benchmark_group("fluid_meshing_32_cubed_plus_halo");
    group.throughput(Throughput::Elements(cells));
    group.bench_function("level_and_flow_surface", |bencher| {
        bencher.iter(|| {
            let receipt = mesher
                .mesh_into(
                    black_box(voxels.as_slice()),
                    dimensions,
                    source,
                    &mut output,
                )
                .expect("prevalidated benchmark input remains valid");
            black_box(receipt);
            black_box(output.quad_count());
        });
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

criterion_group!(
    benches,
    benchmark_meshing,
    benchmark_geometry_handoff,
    benchmark_fluid_meshing
);
criterion_main!(benches);
