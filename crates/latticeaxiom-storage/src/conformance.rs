//! Reusable authoritative storage conformance corpus.
//!
//! Enable the `conformance` feature for an implementation kept inside this crate and call [`run_all`]
//! with a factory that returns an empty transaction kernel.

use std::{collections::BTreeMap, str::FromStr};

use latticeaxiom_core::{CanonicalHash, SchemaId, StableId, WorldId};

use crate::{
    AuthoritativeTransactionKernel, ChangedDomains, ChunkCoordinate, ChunkData, ChunkKey,
    ChunkMutation, ChunkRevision, ChunkRevisionExpectation, ContinuationId, DimensionId,
    PayloadSchemaVersion, PersistentEntityId, StorageError, TransactionId, VersionedPayload,
    WorldRevision, WorldTransaction,
};

/// Runs the implementation-neutral transaction and snapshot conformance corpus.
///
/// `factory` must return a new empty and independent storage instance for each
/// call. The corpus verifies atomic multi-chunk publication, snapshot
/// isolation, idempotency, optimistic conflicts, bounded batches, domain
/// revisions, canonical ordering, and deterministic materialized-chunk hashes.
pub fn run_all<S, F>(factory: F)
where
    S: AuthoritativeTransactionKernel,
    F: Fn() -> S + Copy,
{
    atomic_multi_chunk_publication(&factory());
    idempotent_retry_and_reuse_rejection(&factory());
    optimistic_conflict_has_no_partial_publish(&factory());
    domain_revisions_advance_selectively(&factory());
    entity_index_moves_atomically_and_rejects_collisions(&factory());
    invalid_transactions_do_not_publish(&factory());
    bounded_reads_match_reference_snapshot(&factory());
    owned_snapshot_is_isolated(&factory());
    mutation_order_does_not_change_state(factory);
    changed_domain_mismatch_is_rejected(&factory());
    batch_count_is_bounded(&factory());
}

fn bounded_reads_match_reference_snapshot(storage: &impl AuthoritativeTransactionKernel) {
    let transaction = sample_create_transaction(11);
    let world = transaction.world();
    let keys = transaction
        .mutations()
        .iter()
        .map(|mutation| mutation.key().clone())
        .collect::<Vec<_>>();
    assert_eq!(
        storage
            .world_frontier(world)
            .expect("an empty world frontier must be readable"),
        WorldRevision::ZERO
    );
    assert!(
        storage
            .read_chunk(&keys[0])
            .expect("an absent bounded chunk read must succeed")
            .is_none()
    );

    let receipt = storage
        .commit(transaction)
        .expect("the bounded-read fixture transaction must commit");
    let snapshot = storage
        .reference_snapshot(world)
        .expect("the committed reference snapshot must remain readable");
    assert_eq!(
        storage
            .world_frontier(world)
            .expect("the committed world frontier must be readable"),
        receipt.world_revision()
    );
    for key in keys {
        assert_eq!(
            storage
                .read_chunk(&key)
                .expect("a committed bounded chunk read must succeed")
                .as_ref(),
            snapshot.chunk(&key)
        );
    }
}

/// Creates a canonical two-chunk materialization transaction for backend tests.
#[must_use]
pub fn sample_create_transaction(id: u128) -> WorldTransaction {
    let world = sample_world();
    let mutations = vec![
        ChunkMutation::new(
            sample_key(world, ChunkCoordinate::new(1, -2, 3)),
            ChunkRevisionExpectation::Absent,
            ChangedDomains::ALL,
            sample_data(11),
        ),
        ChunkMutation::new(
            sample_key(world, ChunkCoordinate::new(-1, 0, 4)),
            ChunkRevisionExpectation::Absent,
            ChangedDomains::ALL,
            sample_data(22),
        ),
    ];
    WorldTransaction::new(
        TransactionId::from_u128(id),
        world,
        WorldRevision::ZERO,
        mutations,
    )
}

pub(crate) fn sample_world() -> WorldId {
    WorldId::from_str("018f1e2d-3c4b-4a59-8c6d-7e8f9012abcd")
        .expect("the conformance world UUID literal is canonical")
}

pub(crate) fn sample_key(world: WorldId, coordinate: ChunkCoordinate) -> ChunkKey {
    ChunkKey::new(
        world,
        DimensionId::from_str("terrenia:dimension/terrenia")
            .expect("the conformance dimension literal follows the stable-ID grammar"),
        coordinate,
    )
}

pub(crate) fn sample_data(seed: u8) -> ChunkData {
    let schema = SchemaId::from_str("latticeaxiom:schema/chunk-voxels@1")
        .expect("the conformance schema literal follows the schema-ID grammar");
    let schema_version =
        PayloadSchemaVersion::new(1).expect("schema version one is a positive persistent version");
    let voxels = VersionedPayload::new(schema.clone(), schema_version, vec![seed, seed ^ 0x5a]);
    let mut entities = BTreeMap::new();
    entities.insert(
        PersistentEntityId::from_u128(u128::from(seed)),
        VersionedPayload::new(schema.clone(), schema_version, vec![seed.wrapping_add(1)]),
    );
    let mut continuations = BTreeMap::new();
    continuations.insert(
        ContinuationId::from_u128(u128::from(seed)),
        VersionedPayload::new(schema, schema_version, vec![seed.wrapping_add(2)]),
    );
    let mut provenance = BTreeMap::new();
    provenance.insert(
        StableId::from_str("latticeaxiom:provenance/worldgen@1")
            .expect("the conformance provenance literal follows the stable-ID grammar"),
        CanonicalHash::digest([seed]),
    );
    ChunkData::new(voxels, entities, continuations, provenance)
}

fn atomic_multi_chunk_publication(storage: &impl AuthoritativeTransactionKernel) {
    let transaction = sample_create_transaction(1);
    let world = transaction.world();
    let receipt = storage
        .commit(transaction)
        .expect("a valid initial multi-chunk transaction must commit");
    assert_eq!(receipt.world_revision(), WorldRevision::new(1));

    assert!(!receipt.replayed());
    assert_eq!(receipt.chunks().len(), 2);
    assert!(receipt.chunks()[0].key() < receipt.chunks()[1].key());

    let snapshot = storage
        .reference_snapshot(world)
        .expect("a committed world must produce a read snapshot");
    assert_eq!(snapshot.revision(), WorldRevision::new(1));
    assert_eq!(snapshot.len(), 2);
    assert_eq!(
        snapshot.materialized_chunk_state_hash(),
        receipt.materialized_chunk_state_hash()
    );
    for (_, chunk) in snapshot.chunks() {
        assert_eq!(chunk.captured_world_revision(), WorldRevision::new(1));
        assert_eq!(chunk.revision(), ChunkRevision::new(1));
        assert_eq!(chunk.domain_revisions().voxels().get(), 1);
        assert_eq!(chunk.domain_revisions().persistent_entities().get(), 1);
        assert_eq!(chunk.domain_revisions().continuation().get(), 1);
    }
}

fn idempotent_retry_and_reuse_rejection(storage: &impl AuthoritativeTransactionKernel) {
    let transaction = sample_create_transaction(2);
    let world = transaction.world();
    let first = storage
        .commit(transaction.clone())
        .expect("the original transaction must commit");
    let mut reordered_retry = transaction.clone();
    reordered_retry.mutations.reverse();
    let replay = storage
        .commit(reordered_retry)
        .expect("an exact transaction retry must replay its receipt");
    assert!(replay.replayed());
    assert_eq!(replay.world_revision(), first.world_revision());
    assert_eq!(
        replay.materialized_chunk_state_hash(),
        first.materialized_chunk_state_hash()
    );
    assert_eq!(replay.chunks(), first.chunks());

    let mut different = transaction;
    different.mutations[0].data.voxels.bytes.push(0xff);
    assert!(matches!(
        storage.commit(different),
        Err(StorageError::TransactionIdReuse { .. })
    ));
    assert_eq!(
        storage
            .reference_snapshot(world)
            .expect("transaction-ID reuse rejection must preserve the world")
            .revision(),
        WorldRevision::new(1)
    );
}

fn optimistic_conflict_has_no_partial_publish(storage: &impl AuthoritativeTransactionKernel) {
    let create = sample_create_transaction(3);
    let world = create.world();
    storage
        .commit(create)
        .expect("the initial transaction must commit");
    let before = storage
        .reference_snapshot(world)
        .expect("the committed world must be readable");
    let keys = before
        .chunks()
        .map(|(key, _)| key.clone())
        .collect::<Vec<_>>();
    let valid = replacement(
        &before,
        &keys[0],
        ChunkRevisionExpectation::Exact(ChunkRevision::new(1)),
        31,
    );
    let stale = replacement(&before, &keys[1], ChunkRevisionExpectation::Absent, 32);
    let transaction = WorldTransaction::new(
        TransactionId::from_u128(4),
        world,
        before.revision(),
        vec![valid, stale],
    );
    assert!(matches!(
        storage.commit(transaction),
        Err(StorageError::ChunkRevisionConflict { .. })
    ));
    let after = storage
        .reference_snapshot(world)
        .expect("a rejected transaction must leave a readable world");
    assert_eq!(after, before);

    let stale_world = WorldTransaction::new(
        TransactionId::from_u128(5),
        world,
        WorldRevision::ZERO,
        vec![replacement(
            &before,
            &keys[0],
            ChunkRevisionExpectation::Exact(ChunkRevision::new(1)),
            33,
        )],
    );
    assert!(matches!(
        storage.commit(stale_world),
        Err(StorageError::WorldRevisionConflict { .. })
    ));
    assert_eq!(
        storage
            .reference_snapshot(world)
            .expect("a world conflict must preserve the snapshot"),
        before
    );
}

#[allow(
    clippy::too_many_lines,
    reason = "the conformance sequence verifies all domain counters after each atomic step"
)]
fn domain_revisions_advance_selectively(storage: &impl AuthoritativeTransactionKernel) {
    let create = sample_create_transaction(6);
    let world = create.world();
    storage
        .commit(create)
        .expect("the initial transaction must commit");
    let before = storage
        .reference_snapshot(world)
        .expect("the initial world must be readable");
    let key = before
        .chunks()
        .next()
        .map(|(key, _)| key.clone())
        .expect("the sample transaction always creates two chunks");

    let mut entity_data = before
        .chunk(&key)
        .expect("the selected chunk must exist")
        .data()
        .clone();
    let entity = *entity_data
        .persistent_entities
        .keys()
        .next()
        .expect("sample data contains one persistent entity");
    entity_data
        .persistent_entities
        .get_mut(&entity)
        .expect("the selected entity must remain present")
        .bytes
        .push(0xe1);
    let entity_receipt = storage
        .commit(WorldTransaction::new(
            TransactionId::from_u128(7),
            world,
            before.revision(),
            vec![ChunkMutation::new(
                key.clone(),
                ChunkRevisionExpectation::Exact(ChunkRevision::new(1)),
                ChangedDomains::PERSISTENT_ENTITIES,
                entity_data,
            )],
        ))
        .expect("an entity-only replacement must commit");
    assert_eq!(
        entity_receipt.chunks()[0].changed_domains(),
        ChangedDomains::PERSISTENT_ENTITIES
    );
    let after_entity = storage
        .reference_snapshot(world)
        .expect("the entity update must be readable");
    let entity_chunk = after_entity
        .chunk(&key)
        .expect("the entity-updated chunk must exist");
    assert_eq!(entity_chunk.revision(), ChunkRevision::new(2));
    assert_eq!(entity_chunk.domain_revisions().voxels().get(), 1);
    assert_eq!(
        entity_chunk.domain_revisions().persistent_entities().get(),
        2
    );
    assert_eq!(entity_chunk.domain_revisions().continuation().get(), 1);

    let mut continuation_data = entity_chunk.data().clone();
    let continuation = *continuation_data
        .continuations
        .keys()
        .next()
        .expect("sample data contains one continuation");
    continuation_data
        .continuations
        .get_mut(&continuation)
        .expect("the selected continuation must remain present")
        .bytes
        .push(0xc1);
    let continuation_receipt = storage
        .commit(WorldTransaction::new(
            TransactionId::from_u128(8),
            world,
            after_entity.revision(),
            vec![ChunkMutation::new(
                key.clone(),
                ChunkRevisionExpectation::Exact(ChunkRevision::new(2)),
                ChangedDomains::CONTINUATIONS,
                continuation_data,
            )],
        ))
        .expect("a continuation-only replacement must commit");
    assert_eq!(
        continuation_receipt.chunks()[0].changed_domains(),
        ChangedDomains::CONTINUATIONS
    );
    let after_continuation = storage
        .reference_snapshot(world)
        .expect("the continuation update must be readable");
    let continuation_chunk = after_continuation
        .chunk(&key)
        .expect("the continuation-updated chunk must exist");
    assert_eq!(continuation_chunk.revision(), ChunkRevision::new(3));
    assert_eq!(continuation_chunk.domain_revisions().voxels().get(), 1);
    assert_eq!(
        continuation_chunk
            .domain_revisions()
            .persistent_entities()
            .get(),
        2
    );
    assert_eq!(
        continuation_chunk.domain_revisions().continuation().get(),
        2
    );

    let mut provenance_data = continuation_chunk.data().clone();
    let provenance = provenance_data
        .provenance
        .keys()
        .next()
        .cloned()
        .expect("sample data contains one provenance entry");
    provenance_data
        .provenance
        .insert(provenance, CanonicalHash::digest(b"updated provenance"));
    let provenance_receipt = storage
        .commit(WorldTransaction::new(
            TransactionId::from_u128(9),
            world,
            after_continuation.revision(),
            vec![ChunkMutation::new(
                key.clone(),
                ChunkRevisionExpectation::Exact(ChunkRevision::new(3)),
                ChangedDomains::PROVENANCE,
                provenance_data,
            )],
        ))
        .expect("a provenance-only replacement must commit");
    assert_eq!(
        provenance_receipt.chunks()[0].changed_domains(),
        ChangedDomains::PROVENANCE
    );
    let after_provenance = storage
        .reference_snapshot(world)
        .expect("the provenance update must be readable");
    let provenance_chunk = after_provenance
        .chunk(&key)
        .expect("the provenance-updated chunk must exist");
    assert_eq!(provenance_chunk.revision(), ChunkRevision::new(4));
    assert_eq!(provenance_chunk.domain_revisions().voxels().get(), 1);
    assert_eq!(
        provenance_chunk
            .domain_revisions()
            .persistent_entities()
            .get(),
        2
    );
    assert_eq!(provenance_chunk.domain_revisions().continuation().get(), 2);
}
fn owned_snapshot_is_isolated(storage: &impl AuthoritativeTransactionKernel) {
    let create = sample_create_transaction(8);
    let world = create.world();
    storage
        .commit(create)
        .expect("the initial transaction must commit");
    let old_snapshot = storage
        .reference_snapshot(world)
        .expect("the initial world must be readable");
    let key = old_snapshot
        .chunks()
        .next()
        .map(|(key, _)| key.clone())
        .expect("the sample transaction always creates chunks");
    let update = WorldTransaction::new(
        TransactionId::from_u128(9),
        world,
        old_snapshot.revision(),
        vec![replacement(
            &old_snapshot,
            &key,
            ChunkRevisionExpectation::Exact(ChunkRevision::new(1)),
            88,
        )],
    );
    storage
        .commit(update)
        .expect("the update transaction must commit");
    let new_snapshot = storage
        .reference_snapshot(world)
        .expect("the updated world must be readable");
    assert_eq!(old_snapshot.revision(), WorldRevision::new(1));
    assert_eq!(new_snapshot.revision(), WorldRevision::new(2));
    assert_ne!(
        old_snapshot.materialized_chunk_state_hash(),
        new_snapshot.materialized_chunk_state_hash()
    );
    assert_eq!(
        old_snapshot
            .chunk(&key)
            .expect("old snapshot owns its original chunk")
            .revision(),
        ChunkRevision::new(1)
    );
}

fn mutation_order_does_not_change_state<S, F>(factory: F)
where
    S: AuthoritativeTransactionKernel,
    F: Fn() -> S,
{
    let forward = sample_create_transaction(10);
    let mut reverse = forward.clone();
    reverse.mutations.reverse();
    let first = factory()
        .commit(forward)
        .expect("forward mutation order must commit");
    let second = factory()
        .commit(reverse)
        .expect("reverse mutation order must commit");
    assert_eq!(
        first.materialized_chunk_state_hash(),
        second.materialized_chunk_state_hash()
    );
    assert_eq!(first.chunks(), second.chunks());
}

#[allow(
    clippy::too_many_lines,
    reason = "the conformance scenario keeps collision rejection and the following move contiguous"
)]
fn entity_index_moves_atomically_and_rejects_collisions(
    storage: &impl AuthoritativeTransactionKernel,
) {
    let create = sample_create_transaction(13);
    let world = create.world();
    storage
        .commit(create)
        .expect("the initial entity-index transaction must commit");
    let before = storage
        .reference_snapshot(world)
        .expect("the entity-index fixture must be readable");
    let keys = before
        .chunks()
        .map(|(key, _)| key.clone())
        .collect::<Vec<_>>();
    let source = keys[0].clone();
    let destination = keys[1].clone();
    let entity = *before
        .chunk(&source)
        .expect("the source chunk must exist")
        .data()
        .persistent_entities
        .keys()
        .next()
        .expect("the source chunk must contain one entity");
    let payload = before
        .chunk(&source)
        .expect("the source chunk must exist")
        .data()
        .persistent_entities
        .get(&entity)
        .expect("the selected entity payload must exist")
        .clone();

    let mut collision_data = before
        .chunk(&destination)
        .expect("the destination chunk must exist")
        .data()
        .clone();
    collision_data
        .persistent_entities
        .insert(entity, payload.clone());
    let collision = WorldTransaction::new(
        TransactionId::from_u128(14),
        world,
        before.revision(),
        vec![ChunkMutation::new(
            destination.clone(),
            ChunkRevisionExpectation::Exact(ChunkRevision::new(1)),
            ChangedDomains::PERSISTENT_ENTITIES,
            collision_data.clone(),
        )],
    );
    assert!(matches!(
        storage.commit(collision),
        Err(StorageError::PersistentEntityCollision { .. })
    ));
    assert_eq!(
        storage
            .reference_snapshot(world)
            .expect("collision rejection must preserve the snapshot"),
        before
    );

    let mut source_data = before
        .chunk(&source)
        .expect("the source chunk must exist")
        .data()
        .clone();
    source_data
        .persistent_entities
        .remove(&entity)
        .expect("the moved entity must be removed from its source");
    let moved = WorldTransaction::new(
        TransactionId::from_u128(15),
        world,
        before.revision(),
        vec![
            ChunkMutation::new(
                source.clone(),
                ChunkRevisionExpectation::Exact(ChunkRevision::new(1)),
                ChangedDomains::PERSISTENT_ENTITIES,
                source_data,
            ),
            ChunkMutation::new(
                destination.clone(),
                ChunkRevisionExpectation::Exact(ChunkRevision::new(1)),
                ChangedDomains::PERSISTENT_ENTITIES,
                collision_data,
            ),
        ],
    );
    storage
        .commit(moved)
        .expect("one transaction may move an entity between affected chunks");
    let after = storage
        .reference_snapshot(world)
        .expect("the moved entity index must be readable");
    assert_eq!(after.entity_chunk(entity), Some(&destination));
    assert_eq!(after.entity_count(), before.entity_count());
    assert!(
        !after
            .chunk(&source)
            .expect("the source chunk must remain materialized")
            .data()
            .persistent_entities
            .contains_key(&entity)
    );
    assert!(
        after
            .chunk(&destination)
            .expect("the destination chunk must remain materialized")
            .data()
            .persistent_entities
            .contains_key(&entity)
    );
}

fn invalid_transactions_do_not_publish(storage: &impl AuthoritativeTransactionKernel) {
    let world = sample_world();
    assert_eq!(
        storage.commit(WorldTransaction::new(
            TransactionId::from_u128(16),
            world,
            WorldRevision::ZERO,
            Vec::new(),
        )),
        Err(StorageError::EmptyTransaction)
    );

    let other_world = WorldId::from_str("028f1e2d-3c4b-4a59-8c6d-7e8f9012abcd")
        .expect("the alternate world UUID literal is canonical");
    let wrong_world = WorldTransaction::new(
        TransactionId::from_u128(17),
        world,
        WorldRevision::ZERO,
        vec![ChunkMutation::new(
            sample_key(other_world, ChunkCoordinate::new(0, 0, 0)),
            ChunkRevisionExpectation::Absent,
            ChangedDomains::ALL,
            sample_data(1),
        )],
    );
    assert!(matches!(
        storage.commit(wrong_world),
        Err(StorageError::ChunkWorldMismatch { .. })
    ));

    let duplicate_key = sample_key(world, ChunkCoordinate::new(0, 0, 0));
    let duplicate = WorldTransaction::new(
        TransactionId::from_u128(18),
        world,
        WorldRevision::ZERO,
        vec![
            ChunkMutation::new(
                duplicate_key.clone(),
                ChunkRevisionExpectation::Absent,
                ChangedDomains::ALL,
                sample_data(2),
            ),
            ChunkMutation::new(
                duplicate_key,
                ChunkRevisionExpectation::Absent,
                ChangedDomains::ALL,
                sample_data(3),
            ),
        ],
    );
    assert!(matches!(
        storage.commit(duplicate),
        Err(StorageError::DuplicateChunk { .. })
    ));
    assert!(
        storage
            .reference_snapshot(world)
            .expect("invalid transactions must leave the world readable")
            .is_empty()
    );

    storage
        .commit(sample_create_transaction(19))
        .expect("the no-op fixture must first materialize its chunks");
    let before = storage
        .reference_snapshot(world)
        .expect("the no-op fixture must be readable");
    let (key, chunk) = before
        .chunks()
        .next()
        .expect("the sample transaction always creates chunks");
    let no_op = WorldTransaction::new(
        TransactionId::from_u128(20),
        world,
        before.revision(),
        vec![ChunkMutation::new(
            key.clone(),
            ChunkRevisionExpectation::Exact(chunk.revision()),
            ChangedDomains::NONE,
            chunk.data().clone(),
        )],
    );
    assert!(matches!(
        storage.commit(no_op),
        Err(StorageError::NoopMutation { .. })
    ));
    assert_eq!(
        storage
            .reference_snapshot(world)
            .expect("a no-op rejection must preserve the snapshot"),
        before
    );
}
fn changed_domain_mismatch_is_rejected(storage: &impl AuthoritativeTransactionKernel) {
    let world = sample_world();
    let key = sample_key(world, ChunkCoordinate::new(0, 0, 0));
    let mutation = ChunkMutation::new(
        key,
        ChunkRevisionExpectation::Absent,
        ChangedDomains::VOXELS,
        sample_data(44),
    );
    let transaction = WorldTransaction::new(
        TransactionId::from_u128(11),
        world,
        WorldRevision::ZERO,
        vec![mutation],
    );
    assert!(matches!(
        storage.commit(transaction),
        Err(StorageError::ChangedDomainsMismatch { .. })
    ));
    assert!(
        storage
            .reference_snapshot(world)
            .expect("domain mismatch must preserve readable state")
            .is_empty()
    );
}

fn batch_count_is_bounded(storage: &impl AuthoritativeTransactionKernel) {
    let world = sample_world();
    let limit = storage.limits().max_chunks_per_transaction();
    let duplicate_key = sample_key(world, ChunkCoordinate::new(0, 0, 0));
    let mutations = (0..=limit)
        .map(|index| {
            ChunkMutation::new(
                duplicate_key.clone(),
                ChunkRevisionExpectation::Absent,
                ChangedDomains::ALL,
                sample_data(index.to_le_bytes()[0]),
            )
        })
        .collect();
    let transaction = WorldTransaction::new(
        TransactionId::from_u128(21),
        world,
        WorldRevision::ZERO,
        mutations,
    );
    assert!(matches!(
        storage.commit(transaction),
        Err(StorageError::TransactionChunkLimitExceeded { .. })
    ));
    assert!(
        storage
            .reference_snapshot(world)
            .expect("over-limit rejection must preserve readable state")
            .is_empty()
    );
}
fn replacement(
    snapshot: &crate::ReferenceWorldSnapshot,
    key: &ChunkKey,
    expected_revision: ChunkRevisionExpectation,
    voxel: u8,
) -> ChunkMutation {
    let mut data = snapshot
        .chunk(key)
        .expect("replacement fixtures address a materialized chunk")
        .data()
        .clone();
    data.voxels.bytes = vec![voxel, voxel ^ 0x5a];
    ChunkMutation::new(key.clone(), expected_revision, ChangedDomains::VOXELS, data)
}
