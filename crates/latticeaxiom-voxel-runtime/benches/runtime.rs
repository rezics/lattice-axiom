//! Representative non-panicking throughput gates for committed projection work.

#![allow(
    clippy::expect_used,
    reason = "benchmark setup states storage and fixture construction invariants"
)]

use std::{collections::BTreeMap, hint::black_box};

use criterion::{BatchSize, Criterion, criterion_group, criterion_main};
use latticeaxiom_core::{SchemaId, WorldId};
use latticeaxiom_storage::{
    AuthoritativeTransactionKernel, ChangedDomains, ChunkCoordinate, ChunkData, ChunkKey,
    ChunkMutation, ChunkRevisionExpectation, DimensionId, MemoryTransactionKernel,
    PayloadSchemaVersion, StoredChunk, TransactionId, VersionedPayload, WorldTransaction,
};
use latticeaxiom_voxel_runtime::{
    CellSelection, ColliderSemanticFingerprint, CommittedChunkProjection, DdaOrigin, DdaQuery,
    DerivedKind, DerivedMemoryBudget, DerivedOwner, DerivedPriority, DerivedQueueLimits,
    DerivedRequest, DerivedRequestSet, FixedTick, MeshSemanticFingerprint, RuntimeGeneration,
    RuntimeLimits, VoxelRuntime, WorkingSetScope, WorldEpoch,
};

const EDGE: u16 = 32;
const CELL_COUNT: usize = 32 * 32 * 32;

struct Fixture {
    scope: WorkingSetScope,
    first: StoredChunk,
    second: StoredChunk,
}

fn scope() -> WorkingSetScope {
    let world: WorldId = "00000000-0000-4000-8000-000000000001"
        .parse()
        .expect("fixture is a canonical version-4 world UUID");
    let dimension: DimensionId = "terrenia:dimension/terrenia"
        .parse()
        .expect("fixture is a canonical dimension stable ID");
    WorkingSetScope::new(world, dimension, WorldEpoch::new(1))
}

fn data(cells: Vec<u8>) -> ChunkData {
    let schema: SchemaId = "latticeaxiom:schema/chunk-voxels@1"
        .parse()
        .expect("fixture schema is canonical and versioned");
    ChunkData::new(
        VersionedPayload::new(
            schema,
            PayloadSchemaVersion::new(1).expect("fixture schema version is positive"),
            cells,
        ),
        BTreeMap::new(),
        BTreeMap::new(),
        BTreeMap::new(),
    )
}

fn fixture() -> Fixture {
    let scope = scope();
    let storage = MemoryTransactionKernel::new();
    let coordinate = ChunkCoordinate::new(0, 0, 0);
    let key = ChunkKey::new(scope.world(), scope.dimension().clone(), coordinate);
    let mut first_cells = vec![0_u8; CELL_COUNT];
    for y in 0..usize::from(EDGE) {
        for z in 0..usize::from(EDGE) {
            for x in 0..usize::from(EDGE) {
                first_cells[x + usize::from(EDGE) * (z + usize::from(EDGE) * y)] =
                    u8::from(y < 12 || (x + y + z) % 17 == 0);
            }
        }
    }
    storage
        .commit(WorldTransaction::new(
            TransactionId::from_u128(1),
            scope.world(),
            latticeaxiom_storage::WorldRevision::ZERO,
            vec![ChunkMutation::new(
                key.clone(),
                ChunkRevisionExpectation::Absent,
                ChangedDomains::ALL,
                data(first_cells),
            )],
        ))
        .expect("first fixture commit succeeds");
    let first_snapshot = storage
        .reference_snapshot(scope.world())
        .expect("first fixture snapshot exists");
    let first = first_snapshot
        .chunk(&key)
        .expect("first fixture chunk exists")
        .clone();
    let mut second_cells = first.data().voxels().bytes().to_vec();
    for value in second_cells.iter_mut().step_by(257) {
        *value ^= 1;
    }
    storage
        .commit(WorldTransaction::new(
            TransactionId::from_u128(2),
            scope.world(),
            first_snapshot.revision(),
            vec![ChunkMutation::new(
                key.clone(),
                ChunkRevisionExpectation::Exact(first.revision()),
                ChangedDomains::VOXELS,
                data(second_cells),
            )],
        ))
        .expect("second fixture commit succeeds");
    let second = storage
        .reference_snapshot(scope.world())
        .expect("second fixture snapshot exists")
        .chunk(&key)
        .expect("second fixture chunk exists")
        .clone();
    Fixture {
        scope,
        first,
        second,
    }
}

fn limits() -> RuntimeLimits {
    let queue = DerivedQueueLimits::new(128, 8, 16 * 1024 * 1024)
        .expect("benchmark queue limits are nonzero");
    RuntimeLimits::new(64, 32 * 1024 * 1024, queue, queue)
        .expect("benchmark runtime limits are nonzero")
}

fn requests() -> DerivedRequestSet {
    let mesh = DerivedRequest::new(
        DerivedPriority::new(0),
        DerivedOwner::new(1),
        DerivedMemoryBudget::new(4 * 1024 * 1024, 4 * 1024 * 1024),
    );
    let collider = DerivedRequest::new(
        DerivedPriority::new(0),
        DerivedOwner::new(2),
        DerivedMemoryBudget::new(2 * 1024 * 1024, 2 * 1024 * 1024),
    );
    DerivedRequestSet::new(mesh, collider)
}

fn projection(stored: &StoredChunk) -> CommittedChunkProjection<u8> {
    CommittedChunkProjection::from_stored_chunk(
        stored,
        EDGE,
        stored.data().voxels().bytes().to_vec(),
        MeshSemanticFingerprint::new([3; 32]),
        ColliderSemanticFingerprint::new([5; 32]),
    )
    .expect("benchmark payload is exactly one cubic chunk")
}

fn prepared_runtime(fixture: &Fixture) -> VoxelRuntime<u8> {
    let mut runtime = VoxelRuntime::new(
        fixture.scope.clone(),
        RuntimeGeneration::new(1),
        EDGE,
        0_u8,
        limits(),
    )
    .expect("benchmark runtime is valid");
    runtime
        .project_committed(projection(&fixture.first), FixedTick::new(0), requests())
        .expect("first benchmark projection fits");
    runtime
}

fn runtime_benchmarks(criterion: &mut Criterion) {
    let fixture = fixture();

    criterion.bench_function("runtime/capture_32_cubed_plus_halo", |bencher| {
        bencher.iter_batched(
            || prepared_runtime(&fixture),
            |mut runtime| black_box(runtime.dispatch_next(DerivedKind::Mesh)),
            BatchSize::SmallInput,
        );
    });

    criterion.bench_function("runtime/project_committed_sparse_update", |bencher| {
        bencher.iter_batched(
            || prepared_runtime(&fixture),
            |mut runtime| {
                black_box(runtime.project_committed(
                    projection(&fixture.second),
                    FixedTick::new(1),
                    requests(),
                ))
            },
            BatchSize::SmallInput,
        );
    });

    criterion.bench_function("runtime/committed_dda_five_meters", |bencher| {
        let runtime = prepared_runtime(&fixture);
        let query = DdaQuery::new(
            DdaOrigin::new(ChunkCoordinate::new(0, 0, 0), [16.5, 16.5, 16.5]),
            [1.0, -0.3, -0.7],
            5.0,
        );
        bencher.iter(|| {
            black_box(runtime.raycast_committed(query, |value| {
                if *value == 0 {
                    CellSelection::PassThrough
                } else {
                    CellSelection::Target
                }
            }))
        });
    });
}

criterion_group!(benches, runtime_benchmarks);
criterion_main!(benches);
