//! Deterministic in-memory reference implementation of [`crate::AuthoritativeTransactionKernel`].

use std::{
    collections::{BTreeMap, VecDeque},
    sync::{RwLock, RwLockReadGuard, RwLockWriteGuard},
};

use latticeaxiom_core::{CanonicalHash, WorldId};

use crate::model::PayloadByteMeter;
use crate::{
    AuthoritativeTransactionKernel, ChangedDomains, ChunkCommitReceipt, ChunkData, ChunkKey,
    ChunkMutation, ChunkRevision, ChunkRevisionExpectation, CommitReceipt, ContinuationRevision,
    DomainRevisions, FaultPoint, PersistentEntityRevision, ReferenceDurability,
    ReferenceWorldSnapshot, RevisionCounter, StorageError, StorageResult, StoredChunk,
    TransactionId, TransactionKernelLimits, VoxelRevision, WorldRevision, WorldTransaction,
    canonical_hash::{hash_materialized_chunk_state, hash_transaction},
};

#[derive(Clone, Debug)]
struct CommittedTransaction {
    fingerprint: CanonicalHash,
    receipt: CommitReceipt,
}

#[derive(Clone, Debug, Default)]
struct MemoryWorld {
    revision: WorldRevision,
    chunks: BTreeMap<ChunkKey, StoredChunk>,
    entity_locations: BTreeMap<crate::PersistentEntityId, ChunkKey>,
    committed_transactions: BTreeMap<TransactionId, CommittedTransaction>,
    receipt_order: VecDeque<TransactionId>,
}

#[derive(Debug, Default)]
struct MemoryState {
    worlds: BTreeMap<WorldId, MemoryWorld>,
    next_fault: Option<FaultPoint>,
}

/// Atomic in-memory reference backend for tests and headless conformance.
///
/// Its whole-world staging clone, global lock, and full-state hash are
/// intentionally reference-scale behavior, not a production persistence path.
///
/// A commit clones one world's authoritative state, applies every mutation to
/// that private staging copy, and publishes the copy under one write lock.
/// Readers therefore observe the complete old or new transaction, never an
/// intermediate chunk set.
#[derive(Debug)]
pub struct MemoryTransactionKernel {
    limits: TransactionKernelLimits,
    state: RwLock<MemoryState>,
}

impl crate::storage::sealed::Sealed for MemoryTransactionKernel {}

impl MemoryTransactionKernel {
    /// Creates an empty instance with non-normative reference safety limits.
    #[must_use]
    pub fn new() -> Self {
        Self::with_limits(TransactionKernelLimits::default())
    }

    /// Creates an empty instance with explicit transaction and replay ceilings.
    #[must_use]
    pub fn with_limits(limits: TransactionKernelLimits) -> Self {
        Self {
            limits,
            state: RwLock::new(MemoryState::default()),
        }
    }

    /// Installs a deterministic one-shot failure for the next valid commit.
    ///
    /// Natural validation, idempotent replay, and optimistic conflict happen
    /// before the failpoint is consumed.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError::FaultAlreadyPending`] if a failpoint is already
    /// installed, or [`StorageError::LockPoisoned`] if the state lock is
    /// unavailable.
    pub fn inject_fault_once(&self, point: FaultPoint) -> StorageResult<()> {
        let mut state = self.write_state("install failpoint")?;
        if state.next_fault.is_some() {
            return Err(StorageError::FaultAlreadyPending);
        }
        state.next_fault = Some(point);
        Ok(())
    }

    fn read_state(
        &self,
        operation: &'static str,
    ) -> StorageResult<RwLockReadGuard<'_, MemoryState>> {
        self.state
            .read()
            .map_err(|_| StorageError::LockPoisoned { operation })
    }

    fn write_state(
        &self,
        operation: &'static str,
    ) -> StorageResult<RwLockWriteGuard<'_, MemoryState>> {
        self.state
            .write()
            .map_err(|_| StorageError::LockPoisoned { operation })
    }

    fn commit_prepared(&self, prepared: PreparedTransaction) -> StorageResult<CommitReceipt> {
        let mut state = self.write_state("commit authoritative transaction")?;
        let current_world = state
            .worlds
            .get(&prepared.world)
            .cloned()
            .unwrap_or_default();

        if let Some(receipt) = replay_receipt(&current_world, &prepared)? {
            return Ok(receipt);
        }
        ensure_retry_not_expired(&current_world, &prepared)?;
        ensure_world_revision(&current_world, &prepared)?;
        let new_world_revision = next_world_revision(current_world.revision)?;
        let plans = prepare_mutation_plans(&current_world, prepared.mutations)?;
        let entity_locations = project_entity_locations(&current_world, &plans)?;
        let fault = state.next_fault.take();
        fail_if(fault, FaultPoint::AfterValidation)?;

        let mut staged_world = current_world;
        staged_world.revision = new_world_revision;
        staged_world.entity_locations = entity_locations;
        let mut chunk_receipts = Vec::with_capacity(plans.len());
        for (index, plan) in plans.into_iter().enumerate() {
            apply_plan(
                &mut staged_world,
                new_world_revision,
                plan,
                &mut chunk_receipts,
            );
            if index == 0 {
                fail_if(fault, FaultPoint::AfterFirstStagedMutation)?;
            }
        }

        let materialized_chunk_state_hash = hash_materialized_chunk_state(
            prepared.world,
            new_world_revision,
            &staged_world.chunks,
        )?;
        let receipt = CommitReceipt {
            transaction_id: prepared.id,
            world: prepared.world,
            world_revision: new_world_revision,
            chunks: chunk_receipts,
            durability: ReferenceDurability::Queued,
            materialized_chunk_state_hash,
            replayed: false,
        };
        retain_receipt(
            &mut staged_world,
            prepared.id,
            prepared.fingerprint,
            receipt.clone(),
            self.limits.max_retained_receipts_per_world(),
            prepared.world,
        )?;
        fail_if(fault, FaultPoint::AfterStagingBeforePublish)?;

        state.worlds.insert(prepared.world, staged_world);
        fail_if(fault, FaultPoint::AfterPublishBeforeReceipt)?;
        Ok(receipt)
    }
}

impl Default for MemoryTransactionKernel {
    fn default() -> Self {
        Self::new()
    }
}

impl AuthoritativeTransactionKernel for MemoryTransactionKernel {
    fn limits(&self) -> TransactionKernelLimits {
        self.limits
    }

    fn reference_snapshot(&self, world: WorldId) -> StorageResult<ReferenceWorldSnapshot> {
        let state = self.read_state("capture read snapshot")?;
        let (revision, chunks, entity_locations) = state.worlds.get(&world).map_or_else(
            || (WorldRevision::ZERO, BTreeMap::new(), BTreeMap::new()),
            |stored| {
                (
                    stored.revision,
                    stored.chunks.clone(),
                    stored.entity_locations.clone(),
                )
            },
        );
        let materialized_chunk_state_hash =
            hash_materialized_chunk_state(world, revision, &chunks)?;
        Ok(ReferenceWorldSnapshot {
            world,
            revision,
            chunks,
            entity_locations,
            materialized_chunk_state_hash,
        })
    }
    fn commit(&self, transaction: WorldTransaction) -> StorageResult<CommitReceipt> {
        let prepared = validate_transaction(transaction, self.limits)?;
        self.commit_prepared(prepared)
    }
}

struct PreparedTransaction {
    id: TransactionId,
    world: WorldId,
    base_world_revision: WorldRevision,
    mutations: Vec<ChunkMutation>,
    fingerprint: CanonicalHash,
}

fn validate_transaction(
    mut transaction: WorldTransaction,
    limits: TransactionKernelLimits,
) -> StorageResult<PreparedTransaction> {
    if transaction.mutations.is_empty() {
        return Err(StorageError::EmptyTransaction);
    }
    validate_count_limit(&transaction, limits)?;
    validate_payload_limit(&transaction, limits)?;
    transaction
        .mutations
        .sort_by(|left, right| left.key.cmp(&right.key));
    validate_keys(&transaction)?;
    let fingerprint = hash_transaction(&transaction)?;
    Ok(PreparedTransaction {
        id: transaction.id,
        world: transaction.world,
        base_world_revision: transaction.base_world_revision,
        mutations: transaction.mutations,
        fingerprint,
    })
}

fn validate_count_limit(
    transaction: &WorldTransaction,
    limits: TransactionKernelLimits,
) -> StorageResult<()> {
    let actual_chunks = u64::try_from(transaction.mutations.len()).map_err(|_| {
        StorageError::PayloadSizeOverflow {
            what: "transaction mutation count",
        }
    })?;
    if actual_chunks > u64::from(limits.max_chunks_per_transaction()) {
        return Err(StorageError::TransactionChunkLimitExceeded {
            actual_chunks,
            max_chunks: limits.max_chunks_per_transaction(),
        });
    }
    Ok(())
}

fn validate_payload_limit(
    transaction: &WorldTransaction,
    limits: TransactionKernelLimits,
) -> StorageResult<()> {
    let mut meter = PayloadByteMeter::new(limits.max_payload_bytes_per_transaction());
    for mutation in &transaction.mutations {
        mutation.data.measure_encoded_len(&mut meter)?;
    }
    Ok(())
}
fn validate_keys(transaction: &WorldTransaction) -> StorageResult<()> {
    let mut previous: Option<&ChunkKey> = None;
    for mutation in &transaction.mutations {
        if mutation.key.world != transaction.world {
            return Err(StorageError::ChunkWorldMismatch {
                transaction_world: transaction.world,
                key: Box::new(mutation.key.clone()),
            });
        }
        if previous == Some(&mutation.key) {
            return Err(StorageError::DuplicateChunk {
                key: Box::new(mutation.key.clone()),
            });
        }
        previous = Some(&mutation.key);
    }
    Ok(())
}
fn replay_receipt(
    world: &MemoryWorld,
    prepared: &PreparedTransaction,
) -> StorageResult<Option<CommitReceipt>> {
    let Some(committed) = world.committed_transactions.get(&prepared.id) else {
        return Ok(None);
    };
    if committed.fingerprint != prepared.fingerprint {
        return Err(StorageError::TransactionIdReuse {
            world: prepared.world,
            transaction_id: prepared.id,
        });
    }
    let mut receipt = committed.receipt.clone();
    receipt.replayed = true;
    Ok(Some(receipt))
}

fn ensure_retry_not_expired(
    world: &MemoryWorld,
    prepared: &PreparedTransaction,
) -> StorageResult<()> {
    if prepared.base_world_revision >= world.revision {
        return Ok(());
    }
    let Some(oldest_id) = world.receipt_order.front() else {
        return Ok(());
    };
    let oldest = world.committed_transactions.get(oldest_id).ok_or(
        StorageError::ReceiptHistoryInvariant {
            world: prepared.world,
        },
    )?;
    let oldest_replayable_base = oldest
        .receipt
        .world_revision
        .get()
        .checked_sub(1)
        .map(WorldRevision::new)
        .ok_or(StorageError::ReceiptHistoryInvariant {
            world: prepared.world,
        })?;
    if prepared.base_world_revision < oldest_replayable_base {
        return Err(StorageError::RetryWindowExpired {
            world: prepared.world,
            transaction_id: prepared.id,
            transaction_base: prepared.base_world_revision,
            oldest_replayable_base,
        });
    }
    Ok(())
}
fn ensure_world_revision(world: &MemoryWorld, prepared: &PreparedTransaction) -> StorageResult<()> {
    if world.revision != prepared.base_world_revision {
        return Err(StorageError::WorldRevisionConflict {
            world: prepared.world,
            expected: prepared.base_world_revision,
            actual: world.revision,
        });
    }
    Ok(())
}

fn next_world_revision(current: WorldRevision) -> StorageResult<WorldRevision> {
    current
        .get()
        .checked_add(1)
        .map(WorldRevision::new)
        .ok_or(StorageError::RevisionOverflow {
            counter: RevisionCounter::World,
            key: None,
        })
}

struct MutationPlan {
    mutation: ChunkMutation,
    chunk_revision: ChunkRevision,
    domain_revisions: DomainRevisions,
}

fn prepare_mutation_plans(
    world: &MemoryWorld,
    mutations: Vec<ChunkMutation>,
) -> StorageResult<Vec<MutationPlan>> {
    mutations
        .into_iter()
        .map(|mutation| prepare_mutation_plan(world, mutation))
        .collect()
}

fn prepare_mutation_plan(
    world: &MemoryWorld,
    mutation: ChunkMutation,
) -> StorageResult<MutationPlan> {
    let current = world.chunks.get(&mutation.key);
    ensure_chunk_revision(current, &mutation)?;
    let actual_domains = changed_domains(current.map(StoredChunk::data), &mutation.data);
    if actual_domains.is_empty() {
        return Err(StorageError::NoopMutation {
            key: Box::new(mutation.key.clone()),
        });
    }
    if actual_domains != mutation.changed_domains {
        return Err(StorageError::ChangedDomainsMismatch {
            key: Box::new(mutation.key.clone()),
            declared: mutation.changed_domains,
            actual: actual_domains,
        });
    }

    let current_chunk_revision = current.map_or(ChunkRevision::ZERO, StoredChunk::revision);
    let current_domain_revisions =
        current.map_or_else(DomainRevisions::default, StoredChunk::domain_revisions);
    Ok(MutationPlan {
        chunk_revision: next_chunk_revision(current_chunk_revision, &mutation.key)?,
        domain_revisions: next_domain_revisions(
            current_domain_revisions,
            actual_domains,
            &mutation.key,
        )?,
        mutation,
    })
}

fn ensure_chunk_revision(
    current: Option<&StoredChunk>,
    mutation: &ChunkMutation,
) -> StorageResult<()> {
    let actual = current.map(StoredChunk::revision);
    let matches = match mutation.expected_revision {
        ChunkRevisionExpectation::Absent => actual.is_none(),
        ChunkRevisionExpectation::Exact(expected) => actual == Some(expected),
    };
    if !matches {
        return Err(StorageError::ChunkRevisionConflict {
            key: Box::new(mutation.key.clone()),
            expected: mutation.expected_revision,
            actual,
        });
    }
    Ok(())
}

fn project_entity_locations(
    world: &MemoryWorld,
    plans: &[MutationPlan],
) -> StorageResult<BTreeMap<crate::PersistentEntityId, ChunkKey>> {
    let mut locations = world.entity_locations.clone();
    for plan in plans {
        let Some(current) = world.chunks.get(&plan.mutation.key) else {
            continue;
        };
        for entity in current.data.persistent_entities.keys() {
            match locations.remove(entity) {
                Some(indexed_key) if indexed_key == plan.mutation.key => {}
                _ => {
                    return Err(StorageError::EntityIndexInvariant {
                        world: plan.mutation.key.world,
                        entity: *entity,
                    });
                }
            }
        }
    }
    for plan in plans {
        for entity in plan.mutation.data.persistent_entities.keys() {
            if let Some(existing) = locations.insert(*entity, plan.mutation.key.clone())
                && existing != plan.mutation.key
            {
                return Err(StorageError::PersistentEntityCollision {
                    world: plan.mutation.key.world,
                    entity: *entity,
                    existing: Box::new(existing),
                    attempted: Box::new(plan.mutation.key.clone()),
                });
            }
        }
    }
    Ok(locations)
}
fn changed_domains(current: Option<&ChunkData>, replacement: &ChunkData) -> ChangedDomains {
    let Some(current) = current else {
        return ChangedDomains::ALL;
    };
    let mut changed = ChangedDomains::NONE;
    if current.voxels != replacement.voxels {
        changed = changed.union(ChangedDomains::VOXELS);
    }
    if current.persistent_entities != replacement.persistent_entities {
        changed = changed.union(ChangedDomains::PERSISTENT_ENTITIES);
    }
    if current.continuations != replacement.continuations {
        changed = changed.union(ChangedDomains::CONTINUATIONS);
    }
    if current.provenance != replacement.provenance {
        changed = changed.union(ChangedDomains::PROVENANCE);
    }
    changed
}

fn next_chunk_revision(current: ChunkRevision, key: &ChunkKey) -> StorageResult<ChunkRevision> {
    current
        .get()
        .checked_add(1)
        .map(ChunkRevision::new)
        .ok_or_else(|| StorageError::RevisionOverflow {
            counter: RevisionCounter::Chunk,
            key: Some(Box::new(key.clone())),
        })
}

fn next_domain_revisions(
    current: DomainRevisions,
    changed: ChangedDomains,
    key: &ChunkKey,
) -> StorageResult<DomainRevisions> {
    Ok(DomainRevisions {
        voxels: next_voxel_revision(current.voxels, changed, key)?,
        persistent_entities: next_entity_revision(current.persistent_entities, changed, key)?,
        continuation: next_continuation_revision(current.continuation, changed, key)?,
    })
}

fn next_voxel_revision(
    current: VoxelRevision,
    changed: ChangedDomains,
    key: &ChunkKey,
) -> StorageResult<VoxelRevision> {
    if !changed.contains(ChangedDomains::VOXELS) {
        return Ok(current);
    }
    current
        .get()
        .checked_add(1)
        .map(VoxelRevision::new)
        .ok_or_else(|| StorageError::RevisionOverflow {
            counter: RevisionCounter::Voxels,
            key: Some(Box::new(key.clone())),
        })
}

fn next_entity_revision(
    current: PersistentEntityRevision,
    changed: ChangedDomains,
    key: &ChunkKey,
) -> StorageResult<PersistentEntityRevision> {
    if !changed.contains(ChangedDomains::PERSISTENT_ENTITIES) {
        return Ok(current);
    }
    current
        .get()
        .checked_add(1)
        .map(PersistentEntityRevision::new)
        .ok_or_else(|| StorageError::RevisionOverflow {
            counter: RevisionCounter::PersistentEntities,
            key: Some(Box::new(key.clone())),
        })
}

fn next_continuation_revision(
    current: ContinuationRevision,
    changed: ChangedDomains,
    key: &ChunkKey,
) -> StorageResult<ContinuationRevision> {
    if !changed.contains(ChangedDomains::CONTINUATIONS) {
        return Ok(current);
    }
    current
        .get()
        .checked_add(1)
        .map(ContinuationRevision::new)
        .ok_or_else(|| StorageError::RevisionOverflow {
            counter: RevisionCounter::Continuation,
            key: Some(Box::new(key.clone())),
        })
}

fn apply_plan(
    world: &mut MemoryWorld,
    world_revision: WorldRevision,
    plan: MutationPlan,
    receipts: &mut Vec<ChunkCommitReceipt>,
) {
    let key = plan.mutation.key;
    world.chunks.insert(
        key.clone(),
        StoredChunk {
            key: key.clone(),
            captured_world_revision: world_revision,
            revision: plan.chunk_revision,
            domain_revisions: plan.domain_revisions,
            data: plan.mutation.data,
        },
    );
    receipts.push(ChunkCommitReceipt {
        key,
        chunk_revision: plan.chunk_revision,
        domain_revisions: plan.domain_revisions,
        changed_domains: plan.mutation.changed_domains,
    });
}

fn retain_receipt(
    world: &mut MemoryWorld,
    id: TransactionId,
    fingerprint: CanonicalHash,
    receipt: CommitReceipt,
    max_retained: u32,
    world_id: WorldId,
) -> StorageResult<()> {
    world.committed_transactions.insert(
        id,
        CommittedTransaction {
            fingerprint,
            receipt,
        },
    );
    world.receipt_order.push_back(id);
    let maximum = usize::try_from(max_retained).map_err(|_| StorageError::PayloadSizeOverflow {
        what: "retained receipt limit",
    })?;
    while world.receipt_order.len() > maximum {
        let Some(expired) = world.receipt_order.pop_front() else {
            return Err(StorageError::ReceiptHistoryInvariant { world: world_id });
        };
        world.committed_transactions.remove(&expired);
    }
    Ok(())
}
fn fail_if(actual: Option<FaultPoint>, expected: FaultPoint) -> StorageResult<()> {
    if actual == Some(expected) {
        Err(StorageError::InjectedFault { point: expected })
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::expect_used,
        clippy::too_many_lines,
        reason = "fault and boundary tests name the invariant behind every fixture access"
    )]

    use std::{sync::Barrier, thread};

    use super::*;
    use crate::model::{payload_measure_visits, reset_payload_measure_visits};
    use crate::{ChunkCoordinate, PersistentEntityId, conformance};

    #[test]
    fn conforms_to_materialized_chunk_transaction_kernel() {
        conformance::run_all(MemoryTransactionKernel::new);
    }

    #[test]
    fn every_pre_publish_fault_preserves_the_old_world() {
        for point in [
            FaultPoint::AfterValidation,
            FaultPoint::AfterFirstStagedMutation,
            FaultPoint::AfterStagingBeforePublish,
        ] {
            let storage = MemoryTransactionKernel::new();
            let transaction = conformance::sample_create_transaction(90);
            let world = transaction.world();
            storage
                .inject_fault_once(point)
                .expect("a fresh storage must accept one failpoint");
            assert_eq!(
                storage.commit(transaction),
                Err(StorageError::InjectedFault { point })
            );
            let snapshot = storage
                .reference_snapshot(world)
                .expect("a pre-publish fault must preserve readable state");
            assert_eq!(snapshot.revision(), WorldRevision::ZERO);
            assert!(snapshot.is_empty());
        }
    }

    #[test]
    fn lost_receipt_retry_is_idempotent() {
        let storage = MemoryTransactionKernel::new();
        let transaction = conformance::sample_create_transaction(91);
        let world = transaction.world();
        storage
            .inject_fault_once(FaultPoint::AfterPublishBeforeReceipt)
            .expect("a fresh storage must accept one failpoint");
        assert_eq!(
            storage.commit(transaction.clone()),
            Err(StorageError::InjectedFault {
                point: FaultPoint::AfterPublishBeforeReceipt,
            })
        );
        let published = storage
            .reference_snapshot(world)
            .expect("an after-publish fault must leave a complete readable world");
        assert_eq!(published.revision(), WorldRevision::new(1));
        assert_eq!(published.len(), 2);

        let retry = storage
            .commit(transaction)
            .expect("an exact retry must recover the lost receipt");
        assert!(retry.replayed());
        assert_eq!(retry.world_revision(), WorldRevision::new(1));
        assert_eq!(
            storage
                .reference_snapshot(world)
                .expect("replay must leave state readable")
                .revision(),
            WorldRevision::new(1)
        );
    }
    #[test]
    fn materialized_chunk_reads_return_committed_state_and_none_when_absent() {
        let storage = MemoryTransactionKernel::new();
        let transaction = conformance::sample_create_transaction(93);
        let world = transaction.world();
        let present = transaction.mutations[0].key().clone();
        let absent = conformance::sample_key(world, ChunkCoordinate::new(9, -4, 2));
        let before = storage
            .reference_snapshot(world)
            .expect("an empty world must still produce a snapshot");
        assert!(before.chunk(&present).is_none());
        assert!(before.chunk(&absent).is_none());

        storage
            .commit(transaction)
            .expect("the sample materialization must commit");
        let after = storage
            .reference_snapshot(world)
            .expect("a committed world must be readable before generation");
        assert!(after.chunk(&present).is_some());
        assert!(after.chunk(&absent).is_none());
        assert_eq!(after.revision(), WorldRevision::new(1));
    }

    #[test]
    fn memory_receipts_report_only_queued_durability() {
        let receipt = MemoryTransactionKernel::new()
            .commit(conformance::sample_create_transaction(92))
            .expect("the valid reference transaction must commit");
        assert_eq!(receipt.durability(), ReferenceDurability::Queued);
    }

    #[test]
    fn payload_preflight_stops_before_visiting_oversized_tail_payloads() {
        let world = conformance::sample_world();
        let mut data = conformance::sample_data(93);
        let template = data
            .persistent_entities
            .values()
            .next()
            .expect("sample data contains one entity payload")
            .clone();
        for id in 1_000_u128..2_024 {
            data.persistent_entities
                .insert(PersistentEntityId::from_u128(id), template.clone());
        }
        let transaction = WorldTransaction::new(
            TransactionId::from_u128(93),
            world,
            WorldRevision::ZERO,
            vec![ChunkMutation::new(
                conformance::sample_key(world, ChunkCoordinate::new(0, 0, 0)),
                ChunkRevisionExpectation::Absent,
                ChangedDomains::ALL,
                data,
            )],
        );
        let storage = MemoryTransactionKernel::with_limits(
            TransactionKernelLimits::new(1, 1, 4).expect("positive reference limits must be valid"),
        );

        reset_payload_measure_visits();
        assert!(matches!(
            storage.commit(transaction),
            Err(StorageError::TransactionPayloadLimitExceeded {
                minimum_payload_bytes,
                max_payload_bytes: 1,
            }) if minimum_payload_bytes > 1
        ));
        assert_eq!(payload_measure_visits(), 1);
    }
    #[test]
    fn receipt_history_is_bounded_and_old_retries_expire() {
        let storage = MemoryTransactionKernel::with_limits(
            TransactionKernelLimits::new(1, 1024 * 1024, 3)
                .expect("positive reference limits must be valid"),
        );
        let first = commit_voxel_step(&storage, 100, 1);
        commit_voxel_step(&storage, 101, 2);
        commit_voxel_step(&storage, 102, 3);
        assert_receipt_history_len(&storage, first.world(), 3);
        assert!(
            storage
                .commit(first.clone())
                .expect("the horizon boundary receipt must remain replayable")
                .replayed()
        );

        commit_voxel_step(&storage, 103, 4);
        assert_receipt_history_len(&storage, first.world(), 3);
        assert!(matches!(
            storage.commit(first),
            Err(StorageError::RetryWindowExpired {
                transaction_base: WorldRevision::ZERO,
                oldest_replayable_base,
                ..
            }) if oldest_replayable_base == WorldRevision::new(1)
        ));

        for step in 5_u8..=100 {
            commit_voxel_step(&storage, 100 + u128::from(step), step);
            assert_receipt_history_len(&storage, conformance::sample_world(), 3);
        }
    }

    #[test]
    fn world_revision_overflow_rolls_back() {
        let (storage, world, key) = materialized_storage(200);
        {
            let mut state = storage
                .write_state("prepare world overflow fixture")
                .expect("the isolated test lock cannot be poisoned");
            state
                .worlds
                .get_mut(&world)
                .expect("the committed fixture world must exist")
                .revision = WorldRevision::new(u64::MAX);
        }
        let before = storage
            .reference_snapshot(world)
            .expect("the overflow fixture must be readable");
        let mutation = voxel_mutation(&before, &key);
        let result = storage.commit(WorldTransaction::new(
            TransactionId::from_u128(201),
            world,
            before.revision(),
            vec![mutation],
        ));
        assert_eq!(
            result,
            Err(StorageError::RevisionOverflow {
                counter: RevisionCounter::World,
                key: None,
            })
        );
        assert_eq!(
            storage
                .reference_snapshot(world)
                .expect("world overflow rejection must preserve state"),
            before
        );
    }

    #[test]
    fn chunk_revision_overflow_rolls_back() {
        let (storage, world, key) = materialized_storage(210);
        {
            let mut state = storage
                .write_state("prepare chunk overflow fixture")
                .expect("the isolated test lock cannot be poisoned");
            state
                .worlds
                .get_mut(&world)
                .expect("the committed fixture world must exist")
                .chunks
                .get_mut(&key)
                .expect("the committed fixture chunk must exist")
                .revision = ChunkRevision::new(u64::MAX);
        }
        assert_counter_overflow_rolls_back(
            &storage,
            world,
            &key,
            211,
            RevisionCounter::Chunk,
            voxel_mutation,
        );
    }

    #[test]
    fn every_domain_revision_overflow_rolls_back() {
        let (voxel_storage, world, key) = materialized_storage(220);
        {
            let mut state = voxel_storage
                .write_state("prepare voxel overflow fixture")
                .expect("the isolated test lock cannot be poisoned");
            state
                .worlds
                .get_mut(&world)
                .expect("the committed fixture world must exist")
                .chunks
                .get_mut(&key)
                .expect("the committed fixture chunk must exist")
                .domain_revisions
                .voxels = VoxelRevision::new(u64::MAX);
        }
        assert_counter_overflow_rolls_back(
            &voxel_storage,
            world,
            &key,
            221,
            RevisionCounter::Voxels,
            voxel_mutation,
        );

        let (entity_storage, world, key) = materialized_storage(230);
        {
            let mut state = entity_storage
                .write_state("prepare entity overflow fixture")
                .expect("the isolated test lock cannot be poisoned");
            state
                .worlds
                .get_mut(&world)
                .expect("the committed fixture world must exist")
                .chunks
                .get_mut(&key)
                .expect("the committed fixture chunk must exist")
                .domain_revisions
                .persistent_entities = PersistentEntityRevision::new(u64::MAX);
        }
        assert_counter_overflow_rolls_back(
            &entity_storage,
            world,
            &key,
            231,
            RevisionCounter::PersistentEntities,
            entity_mutation,
        );

        let (continuation_storage, world, key) = materialized_storage(240);
        {
            let mut state = continuation_storage
                .write_state("prepare continuation overflow fixture")
                .expect("the isolated test lock cannot be poisoned");
            state
                .worlds
                .get_mut(&world)
                .expect("the committed fixture world must exist")
                .chunks
                .get_mut(&key)
                .expect("the committed fixture chunk must exist")
                .domain_revisions
                .continuation = ContinuationRevision::new(u64::MAX);
        }
        assert_counter_overflow_rolls_back(
            &continuation_storage,
            world,
            &key,
            241,
            RevisionCounter::Continuation,
            continuation_mutation,
        );
    }

    #[test]
    fn concurrent_snapshot_observes_only_old_or_new_world() {
        for iteration in 0_u128..32 {
            let storage = MemoryTransactionKernel::new();
            let transaction = conformance::sample_create_transaction(300 + iteration);
            let world = transaction.world();
            let barrier = Barrier::new(3);
            thread::scope(|scope| {
                let writer = scope.spawn(|| {
                    barrier.wait();
                    storage
                        .commit(transaction)
                        .expect("the concurrent reference transaction must commit")
                });
                let reader = scope.spawn(|| {
                    barrier.wait();
                    storage
                        .reference_snapshot(world)
                        .expect("the concurrent reference snapshot must be readable")
                });
                barrier.wait();
                let receipt = writer
                    .join()
                    .expect("the scoped writer thread must not panic");
                let snapshot = reader
                    .join()
                    .expect("the scoped reader thread must not panic");
                match snapshot.revision() {
                    WorldRevision::ZERO => assert!(snapshot.is_empty()),
                    revision if revision == WorldRevision::new(1) => {
                        assert_eq!(snapshot.len(), 2);
                        assert_eq!(
                            snapshot.materialized_chunk_state_hash(),
                            receipt.materialized_chunk_state_hash()
                        );
                    }
                    revision => panic!("snapshot observed partial revision {revision:?}"),
                }
            });
        }
    }

    fn commit_voxel_step(
        storage: &MemoryTransactionKernel,
        transaction_id: u128,
        value: u8,
    ) -> WorldTransaction {
        let world = conformance::sample_world();
        let snapshot = storage
            .reference_snapshot(world)
            .expect("the sequential reference world must be readable");
        let mutation = if snapshot.is_empty() {
            ChunkMutation::new(
                conformance::sample_key(world, ChunkCoordinate::new(0, 0, 0)),
                ChunkRevisionExpectation::Absent,
                ChangedDomains::ALL,
                conformance::sample_data(value),
            )
        } else {
            let (key, chunk) = snapshot
                .chunks()
                .next()
                .expect("the non-empty sequential world contains its chunk");
            let mut data = chunk.data().clone();
            data.voxels.bytes = vec![value, value ^ 0x5a];
            ChunkMutation::new(
                key.clone(),
                ChunkRevisionExpectation::Exact(chunk.revision()),
                ChangedDomains::VOXELS,
                data,
            )
        };
        let transaction = WorldTransaction::new(
            TransactionId::from_u128(transaction_id),
            world,
            snapshot.revision(),
            vec![mutation],
        );
        storage
            .commit(transaction.clone())
            .expect("the sequential voxel transaction must commit");
        transaction
    }

    fn assert_receipt_history_len(
        storage: &MemoryTransactionKernel,
        world: WorldId,
        expected: usize,
    ) {
        let state = storage
            .read_state("inspect bounded receipt history")
            .expect("the isolated test lock cannot be poisoned");
        let stored = state
            .worlds
            .get(&world)
            .expect("the sequential fixture world must exist");
        assert_eq!(stored.committed_transactions.len(), expected);
        assert_eq!(stored.receipt_order.len(), expected);
    }

    fn materialized_storage(id: u128) -> (MemoryTransactionKernel, WorldId, ChunkKey) {
        let storage = MemoryTransactionKernel::new();
        let transaction = conformance::sample_create_transaction(id);
        let world = transaction.world();
        storage
            .commit(transaction)
            .expect("the overflow fixture transaction must commit");
        let key = storage
            .reference_snapshot(world)
            .expect("the overflow fixture must be readable")
            .chunks()
            .next()
            .map(|(key, _)| key.clone())
            .expect("the overflow fixture contains two chunks");
        (storage, world, key)
    }

    fn voxel_mutation(snapshot: &ReferenceWorldSnapshot, key: &ChunkKey) -> ChunkMutation {
        let chunk = snapshot
            .chunk(key)
            .expect("the overflow fixture chunk must exist");
        let mut data = chunk.data().clone();
        data.voxels.bytes.push(0xa1);
        ChunkMutation::new(
            key.clone(),
            ChunkRevisionExpectation::Exact(chunk.revision()),
            ChangedDomains::VOXELS,
            data,
        )
    }

    fn entity_mutation(snapshot: &ReferenceWorldSnapshot, key: &ChunkKey) -> ChunkMutation {
        let chunk = snapshot
            .chunk(key)
            .expect("the overflow fixture chunk must exist");
        let mut data = chunk.data().clone();
        data.persistent_entities
            .values_mut()
            .next()
            .expect("sample data contains one entity")
            .bytes
            .push(0xe1);
        ChunkMutation::new(
            key.clone(),
            ChunkRevisionExpectation::Exact(chunk.revision()),
            ChangedDomains::PERSISTENT_ENTITIES,
            data,
        )
    }

    fn continuation_mutation(snapshot: &ReferenceWorldSnapshot, key: &ChunkKey) -> ChunkMutation {
        let chunk = snapshot
            .chunk(key)
            .expect("the overflow fixture chunk must exist");
        let mut data = chunk.data().clone();
        data.continuations
            .values_mut()
            .next()
            .expect("sample data contains one continuation")
            .bytes
            .push(0xc1);
        ChunkMutation::new(
            key.clone(),
            ChunkRevisionExpectation::Exact(chunk.revision()),
            ChangedDomains::CONTINUATIONS,
            data,
        )
    }

    fn assert_counter_overflow_rolls_back(
        storage: &MemoryTransactionKernel,
        world: WorldId,
        key: &ChunkKey,
        transaction_id: u128,
        expected_counter: RevisionCounter,
        mutation: fn(&ReferenceWorldSnapshot, &ChunkKey) -> ChunkMutation,
    ) {
        let before = storage
            .reference_snapshot(world)
            .expect("the overflow fixture must be readable");
        let result = storage.commit(WorldTransaction::new(
            TransactionId::from_u128(transaction_id),
            world,
            before.revision(),
            vec![mutation(&before, key)],
        ));
        assert!(matches!(
            result,
            Err(StorageError::RevisionOverflow {
                counter,
                key: Some(_),
            }) if counter == expected_counter
        ));
        assert_eq!(
            storage
                .reference_snapshot(world)
                .expect("counter overflow rejection must preserve state"),
            before
        );
    }
}
