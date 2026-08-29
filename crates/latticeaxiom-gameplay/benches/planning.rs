//! Reproducible optimized benchmark for one bounded mining-plan hot path.

use std::{fmt::Debug, hint::black_box, num::NonZeroU32, str::FromStr};

use criterion::{Criterion, criterion_group, criterion_main};
use latticeaxiom_gameplay::{
    BlockDefinitionV1, BlockId, BlockKey, BlockPosition, CatalogLimits, ChunkRevision,
    CommandEnvelopeV1, DimensionChunkKey, DimensionId, GameplayCatalog, GameplayCatalogSourceV1,
    GameplayCommandV1, GameplayEditTarget, GameplayKernel, GameplayLimits, GameplayStorageDomain,
    InventoryStateV1, ItemDefinitionV1, ItemId, ItemStackV1, MineCommandV1, MiningRuleV1, PlayerId,
    ReferenceGameplayState, TransactionId,
};

fn parsed<T>(value: &str) -> T
where
    T: FromStr,
    T::Err: Debug,
{
    match value.parse() {
        Ok(value) => value,
        Err(error) => panic!("benchmark fixture ID failed: {error:?}"),
    }
}

fn non_zero(value: u32) -> NonZeroU32 {
    match NonZeroU32::new(value) {
        Some(value) => value,
        None => panic!("benchmark fixture value was zero"),
    }
}

fn planning_benchmark(criterion: &mut Criterion) {
    let item: ItemId = parsed("example:item/benchmark-drop");
    let block: BlockId = parsed("example:block/benchmark-target");
    let drop = match ItemStackV1::plain(item.clone(), 1) {
        Ok(value) => value,
        Err(error) => panic!("benchmark drop failed: {error}"),
    };
    let catalog = match GameplayCatalog::compile(
        GameplayCatalogSourceV1 {
            items: vec![ItemDefinitionV1 {
                id: item,
                stack_limit: non_zero(64),
                placement_block: None,
                durability: None,
            }],
            blocks: vec![BlockDefinitionV1 {
                id: block.clone(),
                mining: MiningRuleV1 {
                    hardness: non_zero(100),
                    tool: None,
                },
                drop,
            }],
            ..GameplayCatalogSourceV1::default()
        },
        CatalogLimits::default(),
    ) {
        Ok(value) => value,
        Err(error) => panic!("benchmark catalog failed: {error}"),
    };
    let mut state = match ReferenceGameplayState::new(GameplayLimits::default()) {
        Ok(value) => value,
        Err(error) => panic!("benchmark state failed: {error}"),
    };
    let dimension = parsed::<DimensionId>("example:dimension/benchmark");
    let position = BlockPosition { x: -1, y: 8, z: 1 };
    let chunk = DimensionChunkKey::new(dimension.clone(), position.chunk());
    if let Err(error) = state.seed_loaded_chunk(chunk.clone(), ChunkRevision::ZERO) {
        panic!("benchmark loaded chunk failed: {error}");
    }
    if let Err(error) = state.seed_player(
        PlayerId::new(1),
        match InventoryStateV1::empty(
            GameplayEditTarget::new(chunk, GameplayStorageDomain::PersistentEntities),
            8,
        ) {
            Ok(value) => value,
            Err(error) => panic!("benchmark inventory failed: {error}"),
        },
    ) {
        panic!("benchmark player failed: {error}");
    }
    let target = BlockKey::new(dimension, position);
    if let Err(error) = state.seed_block(target.clone(), block) {
        panic!("benchmark block failed: {error}");
    }
    let command = CommandEnvelopeV1 {
        transaction_id: TransactionId::from_u128(1),
        expected_world_revision: state.observed_world_revision(),
        command: GameplayCommandV1::Mine(MineCommandV1 {
            reserved_drop: latticeaxiom_gameplay::DropEntityId::new(2),
            player: PlayerId::new(1),
            target,
            expected_chunk_revision: ChunkRevision::ZERO,
            tool_slot: None,
            steps: latticeaxiom_gameplay::MiningStepCountV1::ONE,
        }),
    };
    let kernel = GameplayKernel::new(&catalog);
    criterion.bench_function("plan_mining_progress", |bencher| {
        bencher.iter(|| {
            let result = kernel.plan(black_box(&state), black_box(&command));
            black_box(result)
        });
    });
}

criterion_group!(benches, planning_benchmark);
criterion_main!(benches);
