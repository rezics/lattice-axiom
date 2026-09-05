//! Reproducible commit-cost diagnostics for the in-memory reference kernel.

#![expect(
    clippy::expect_used,
    reason = "fixed benchmark fixture invariants must abort the run when violated"
)]

use std::{collections::BTreeMap, hint::black_box, str::FromStr};

use criterion::{Criterion, criterion_group, criterion_main};
use latticeaxiom_core::{SchemaId, WorldId};
use latticeaxiom_storage::{
    AuthoritativeTransactionKernel, ChangedDomains, ChunkCoordinate, ChunkData, ChunkKey,
    ChunkMutation, ChunkRevision, ChunkRevisionExpectation, DimensionId, MemoryTransactionKernel,
    PayloadSchemaVersion, TransactionId, VersionedPayload, WorldRevision, WorldTransaction,
};

const CHUNK_COUNT: usize = 128;
const PUBLICATION_BATCH_CHUNKS: usize = 8;
const VOXEL_PAYLOAD_BYTES: usize = 32 * 32 * 32 * 4;

fn memory_commit_benchmark(criterion: &mut Criterion) {
    let (kernel, world, dimension, schema, mut frontier) = seeded_kernel();
    let mut revisions = [ChunkRevision::new(1); CHUNK_COUNT];
    let mut iteration = 0_u64;

    criterion.bench_function("memory_commit_128_x_32_cubic_payloads", |bencher| {
        bencher.iter(|| {
            let index = usize::try_from(iteration % CHUNK_COUNT as u64)
                .expect("bounded benchmark index fits usize");
            let next_byte =
                u8::try_from(iteration % 251 + 1).expect("bounded benchmark byte fits u8");
            let key = ChunkKey::new(
                world,
                dimension.clone(),
                ChunkCoordinate::new(i32::try_from(index).expect("chunk index fits i32"), 0, 0),
            );
            let mutation = ChunkMutation::new(
                key,
                ChunkRevisionExpectation::Exact(revisions[index]),
                ChangedDomains::VOXELS,
                chunk_data(&schema, next_byte),
            );
            let receipt = kernel
                .commit(WorldTransaction::new(
                    TransactionId::from_u128(u128::from(iteration) + 10_000),
                    world,
                    frontier,
                    vec![mutation],
                ))
                .expect("benchmark update satisfies current revisions");
            frontier = receipt.world_revision();
            revisions[index] = receipt.chunks()[0].chunk_revision();
            iteration = iteration.saturating_add(1);
            black_box(receipt);
        });
    });

    let (kernel, world, dimension, schema, mut frontier) = seeded_kernel();
    let mut revisions = [ChunkRevision::new(1); CHUNK_COUNT];
    let mut iteration = 0_u64;
    criterion.bench_function("memory_publish_128_x_32_cubic_payloads", |bencher| {
        bencher.iter(|| {
            let index = usize::try_from(iteration % CHUNK_COUNT as u64)
                .expect("bounded benchmark index fits usize");
            let next_byte =
                u8::try_from(iteration % 251 + 1).expect("bounded benchmark byte fits u8");
            let key = ChunkKey::new(
                world,
                dimension.clone(),
                ChunkCoordinate::new(i32::try_from(index).expect("chunk index fits i32"), 0, 0),
            );
            let mutation = ChunkMutation::new(
                key,
                ChunkRevisionExpectation::Exact(revisions[index]),
                ChangedDomains::VOXELS,
                chunk_data(&schema, next_byte),
            );
            let receipt = kernel
                .publish(WorldTransaction::new(
                    TransactionId::from_u128(u128::from(iteration) + 20_000),
                    world,
                    frontier,
                    vec![mutation],
                ))
                .expect("benchmark publication satisfies current revisions");
            frontier = receipt.world_revision();
            revisions[index] = receipt.chunks()[0].chunk_revision();
            iteration = iteration.saturating_add(1);
            black_box(receipt);
        });
    });

    sequential_publication_benchmark(criterion);
    batched_publication_benchmark(criterion);
}

fn sequential_publication_benchmark(criterion: &mut Criterion) {
    let (kernel, world, dimension, schema, mut frontier) = seeded_kernel();
    let mut revisions = [ChunkRevision::new(1); CHUNK_COUNT];
    let mut iteration = 0_u64;
    criterion.bench_function(
        "memory_publish_8_x_32_cubic_sequential_transactions",
        |bencher| {
            bencher.iter(|| {
                let group = publication_group(iteration);
                for offset in 0..PUBLICATION_BATCH_CHUNKS {
                    let index = group + offset;
                    let mutation = update_mutation(
                        world,
                        &dimension,
                        &schema,
                        index,
                        revisions[index],
                        publication_byte(iteration, offset),
                    );
                    let transaction = iteration
                        .saturating_mul(
                            u64::try_from(PUBLICATION_BATCH_CHUNKS).expect("batch size fits u64"),
                        )
                        .saturating_add(u64::try_from(offset).expect("offset fits u64"));
                    let receipt = kernel
                        .publish(WorldTransaction::new(
                            TransactionId::from_u128(u128::from(transaction) + 30_000),
                            world,
                            frontier,
                            vec![mutation],
                        ))
                        .expect("sequential benchmark publication satisfies current revisions");
                    frontier = receipt.world_revision();
                    revisions[index] = receipt.chunks()[0].chunk_revision();
                    black_box(receipt);
                }
                iteration = iteration.saturating_add(1);
            });
        },
    );
}

fn batched_publication_benchmark(criterion: &mut Criterion) {
    let (kernel, world, dimension, schema, mut frontier) = seeded_kernel();
    let mut revisions = [ChunkRevision::new(1); CHUNK_COUNT];
    let mut iteration = 0_u64;
    criterion.bench_function(
        "memory_publish_8_x_32_cubic_batched_transaction",
        |bencher| {
            bencher.iter(|| {
                let group = publication_group(iteration);
                let mutations = (0..PUBLICATION_BATCH_CHUNKS)
                    .map(|offset| {
                        let index = group + offset;
                        update_mutation(
                            world,
                            &dimension,
                            &schema,
                            index,
                            revisions[index],
                            publication_byte(iteration, offset),
                        )
                    })
                    .collect::<Vec<_>>();
                let receipt = kernel
                    .publish(WorldTransaction::new(
                        TransactionId::from_u128(u128::from(iteration) + 40_000),
                        world,
                        frontier,
                        mutations,
                    ))
                    .expect("batched benchmark publication satisfies current revisions");
                frontier = receipt.world_revision();
                for (offset, chunk) in receipt.chunks().iter().enumerate() {
                    revisions[group + offset] = chunk.chunk_revision();
                }
                iteration = iteration.saturating_add(1);
                black_box(receipt);
            });
        },
    );
}

fn publication_group(iteration: u64) -> usize {
    usize::try_from(iteration)
        .expect("benchmark iteration fits usize")
        .wrapping_mul(PUBLICATION_BATCH_CHUNKS)
        % CHUNK_COUNT
}

fn publication_byte(iteration: u64, offset: usize) -> u8 {
    u8::try_from(iteration.wrapping_add(u64::try_from(offset).expect("offset fits u64")) % 251 + 1)
        .expect("bounded benchmark byte fits u8")
}

fn update_mutation(
    world: WorldId,
    dimension: &DimensionId,
    schema: &SchemaId,
    index: usize,
    revision: ChunkRevision,
    byte: u8,
) -> ChunkMutation {
    ChunkMutation::new(
        ChunkKey::new(
            world,
            dimension.clone(),
            ChunkCoordinate::new(i32::try_from(index).expect("chunk index fits i32"), 0, 0),
        ),
        ChunkRevisionExpectation::Exact(revision),
        ChangedDomains::VOXELS,
        chunk_data(schema, byte),
    )
}

fn seeded_kernel() -> (
    MemoryTransactionKernel,
    WorldId,
    DimensionId,
    SchemaId,
    WorldRevision,
) {
    let kernel = MemoryTransactionKernel::new();
    let world = WorldId::from_str("00000000-0000-4000-8000-0000000000b1")
        .expect("benchmark world UUID is canonical");
    let dimension = DimensionId::from_str("terrenia:dimension/overworld")
        .expect("benchmark dimension ID is valid");
    let schema = SchemaId::from_str("latticeaxiom:schema/chunk-voxels@1")
        .expect("benchmark schema ID is valid");
    let mut frontier = WorldRevision::ZERO;
    let mut transaction = 1_u128;
    for batch_start in (0..CHUNK_COUNT).step_by(32) {
        let mutations = (batch_start..batch_start + 32)
            .map(|index| {
                ChunkMutation::new(
                    ChunkKey::new(
                        world,
                        dimension.clone(),
                        ChunkCoordinate::new(
                            i32::try_from(index).expect("chunk index fits i32"),
                            0,
                            0,
                        ),
                    ),
                    ChunkRevisionExpectation::Absent,
                    ChangedDomains::ALL,
                    chunk_data(
                        &schema,
                        u8::try_from(index % 251).expect("chunk seed fits u8"),
                    ),
                )
            })
            .collect();
        let receipt = kernel
            .commit(WorldTransaction::new(
                TransactionId::from_u128(transaction),
                world,
                frontier,
                mutations,
            ))
            .expect("benchmark seed batch satisfies kernel limits");
        frontier = receipt.world_revision();
        transaction = transaction.saturating_add(1);
    }
    (kernel, world, dimension, schema, frontier)
}

fn chunk_data(schema: &SchemaId, byte: u8) -> ChunkData {
    ChunkData::new(
        VersionedPayload::new(
            schema.clone(),
            PayloadSchemaVersion::new(1).expect("benchmark schema version is positive"),
            vec![byte; VOXEL_PAYLOAD_BYTES],
        ),
        BTreeMap::new(),
        BTreeMap::new(),
        BTreeMap::new(),
    )
}

criterion_group!(benches, memory_commit_benchmark);
criterion_main!(benches);
