//! CPU benchmark for the revision gate and upstream enum conversion.

use std::{convert::Infallible, hint::black_box};

use criterion::{BatchSize, Criterion, Throughput, criterion_group, criterion_main};
use latticeaxiom_storage::ChunkRevision;
use latticeaxiom_voxel_playground::{
    PresentationCoordinate, PresentationSink, PresentationVoxel, ProjectionBatch, ProjectionWrite,
    RevisionGate, VoxelChunkEdge, to_upstream_voxel,
};

#[derive(Debug, Default)]
struct CountingSink {
    writes: usize,
}

impl PresentationSink<u8> for CountingSink {
    type Error = Infallible;

    fn set_voxel(
        &mut self,
        coordinate: PresentationCoordinate,
        voxel: PresentationVoxel<u8>,
    ) -> Result<(), Self::Error> {
        black_box((coordinate, to_upstream_voxel(voxel)));
        self.writes = self.writes.saturating_add(1);
        Ok(())
    }
}

fn projection_benchmark(criterion: &mut Criterion) {
    const EDITS: usize = 4_096;
    let edge = VoxelChunkEdge::D2;
    let coordinate = PresentationCoordinate::new(-1, 12, -1);
    let chunk = coordinate.chunk(edge);
    let mut batches = Vec::with_capacity(EDITS);
    for revision in 1..=EDITS {
        let result = ProjectionBatch::new(
            chunk,
            ChunkRevision::new(revision as u64),
            edge,
            vec![ProjectionWrite::new(
                coordinate,
                PresentationVoxel::Solid(revision.to_le_bytes()[0]),
            )],
        );
        let Ok(batch) = result else {
            panic!("benchmark edit must remain inside its declared chunk");
        };
        batches.push(batch);
    }

    let mut group = criterion.benchmark_group("upstream_projection_adapter");
    group.throughput(Throughput::Elements(EDITS as u64));
    group.bench_function("apply_4096_revision_checked_edits", |bencher| {
        bencher.iter_batched(
            || (RevisionGate::default(), CountingSink::default()),
            |(mut gate, mut sink)| {
                for batch in &batches {
                    let result = gate.apply(batch, &mut sink);
                    black_box(result);
                }
                black_box(sink.writes);
            },
            BatchSize::SmallInput,
        );
    });
    group.finish();
}

criterion_group!(benches, projection_benchmark);
criterion_main!(benches);
