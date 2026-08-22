#![allow(
    clippy::expect_used,
    reason = "test fixtures state construction and dispatch invariants"
)]

use std::{cell::Cell, collections::BTreeMap, mem};

use latticeaxiom_core::{SchemaId, WorldId};
use latticeaxiom_storage::{
    AuthoritativeTransactionKernel, ChangedDomains, ChunkCoordinate, ChunkData, ChunkKey,
    ChunkMutation, ChunkRevisionExpectation, CommitReceipt, DimensionId, FaultPoint,
    MemoryTransactionKernel, PayloadSchemaVersion, StoredChunk, TransactionId, VersionedPayload,
    WorldTransaction,
};
use latticeaxiom_voxel_mesh::Face;
use proptest::prelude::*;

use crate::{
    ApplyAdmission, ApplyByteDeclaration, BackpressureReason, CancellationAckOutcome,
    CellSelection, ColliderFailure, ColliderSafetyState, ColliderSemanticFingerprint,
    CommittedChunkProjection, CompletionOutcome, DdaOrigin, DdaOutcome, DdaQuery,
    DerivedApplyBudget, DerivedApplySlice, DerivedInput, DerivedKind, DerivedMemoryBudget,
    DerivedOwner, DerivedPriority, DerivedQueueLimits, DerivedRequest, DerivedRequestSet,
    DispatchOutcome, EvictionLeaseGeneration, ExecutorFinish, ExecutorOutcome, FixedTick,
    InterestWindow, MemoryStage, MeshSemanticFingerprint, ProjectionDecision, ProjectionEvidence,
    RetainedBytes, RuntimeError, RuntimeGeneration, RuntimeLimits, StaleReason, VoxelCoordinate,
    VoxelRuntime, WallClockNanos, WorkerAbortOutcome, WorkingSetScope, WorldEpoch,
    cpu_heavy_concurrency, host_parallelism,
};

const EDGE: u16 = 4;
const CELL_COUNT: usize = 64;

fn scope() -> WorkingSetScope {
    let world: WorldId = "00000000-0000-4000-8000-000000000001"
        .parse()
        .expect("fixture is a canonical version-4 world UUID");
    let dimension: DimensionId = "terrenia:dimension/terrenia"
        .parse()
        .expect("fixture is a canonical dimension stable ID");
    WorkingSetScope::new(world, dimension, WorldEpoch::new(7))
}

fn queue_limits(
    max_pending: usize,
    max_in_flight: usize,
    max_reserved_bytes: u64,
) -> DerivedQueueLimits {
    DerivedQueueLimits::new(max_pending, max_in_flight, max_reserved_bytes)
        .expect("fixture limits are nonzero")
}

fn limits() -> RuntimeLimits {
    let queue = queue_limits(128, 32, 1024 * 1024);
    RuntimeLimits::new(64, 2 * 1024 * 1024, queue, queue).expect("fixture limits are nonzero")
}

fn runtime() -> VoxelRuntime<u8> {
    VoxelRuntime::new(scope(), RuntimeGeneration::new(11), EDGE, 0, limits())
        .expect("fixture runtime is valid")
}

fn request(owner: u64, priority: u16, result_bytes: u64, apply_bytes: u64) -> DerivedRequest {
    DerivedRequest::new(
        DerivedPriority::new(priority),
        DerivedOwner::new(owner),
        DerivedMemoryBudget::new(result_bytes, apply_bytes),
    )
}

fn requests() -> DerivedRequestSet {
    DerivedRequestSet::new(request(1, 0, 512, 256), request(2, 0, 512, 256))
}

fn mesh_fingerprint(value: u8) -> MeshSemanticFingerprint {
    MeshSemanticFingerprint::new([value; 32])
}

fn collider_fingerprint(value: u8) -> ColliderSemanticFingerprint {
    ColliderSemanticFingerprint::new([value; 32])
}

fn voxel_data(cells: Vec<u8>) -> ChunkData {
    let schema: SchemaId = "latticeaxiom:schema/chunk-voxels@1"
        .parse()
        .expect("fixture schema is canonical and versioned");
    let schema_version =
        PayloadSchemaVersion::new(1).expect("fixture payload schema version is positive");
    ChunkData::new(
        VersionedPayload::new(schema, schema_version, cells),
        BTreeMap::new(),
        BTreeMap::new(),
        BTreeMap::new(),
    )
}

fn build_transaction(
    storage: &MemoryTransactionKernel,
    runtime_scope: &WorkingSetScope,
    coordinate: ChunkCoordinate,
    cells: Vec<u8>,
    transaction: u128,
) -> WorldTransaction {
    let snapshot = storage
        .reference_snapshot(runtime_scope.world())
        .expect("reference snapshot remains available");
    let key = ChunkKey::new(
        runtime_scope.world(),
        runtime_scope.dimension().clone(),
        coordinate,
    );
    let (expectation, changed_domains) = snapshot.chunk(&key).map_or(
        (ChunkRevisionExpectation::Absent, ChangedDomains::ALL),
        |stored| {
            (
                ChunkRevisionExpectation::Exact(stored.revision()),
                ChangedDomains::VOXELS,
            )
        },
    );
    WorldTransaction::new(
        TransactionId::from_u128(transaction),
        runtime_scope.world(),
        snapshot.revision(),
        vec![ChunkMutation::new(
            key,
            expectation,
            changed_domains,
            voxel_data(cells),
        )],
    )
}

fn commit_chunk(
    storage: &MemoryTransactionKernel,
    runtime_scope: &WorkingSetScope,
    coordinate: ChunkCoordinate,
    cells: Vec<u8>,
    transaction: u128,
) -> (CommitReceipt, StoredChunk) {
    let receipt = storage
        .commit(build_transaction(
            storage,
            runtime_scope,
            coordinate,
            cells,
            transaction,
        ))
        .expect("fixture transaction commits atomically");
    let snapshot = storage
        .reference_snapshot(runtime_scope.world())
        .expect("committed snapshot remains available");
    let key = ChunkKey::new(
        runtime_scope.world(),
        runtime_scope.dimension().clone(),
        coordinate,
    );
    let stored = snapshot
        .chunk(&key)
        .expect("committed chunk is present")
        .clone();
    (receipt, stored)
}

fn projection_from_stored(
    stored: &StoredChunk,
    mesh: u8,
    collider: u8,
) -> CommittedChunkProjection<u8> {
    CommittedChunkProjection::from_stored_chunk(
        stored,
        EDGE,
        stored.data().voxels().bytes().to_vec(),
        mesh_fingerprint(mesh),
        collider_fingerprint(collider),
    )
    .expect("stored fixture decodes to one cubic chunk")
}

fn project_stored(
    runtime: &mut VoxelRuntime<u8>,
    stored: &StoredChunk,
    tick: u64,
    mesh: u8,
    collider: u8,
) {
    runtime
        .project_committed(
            projection_from_stored(stored, mesh, collider),
            FixedTick::new(tick),
            requests(),
        )
        .expect("storage-issued projection fits the runtime");
}

fn index(x: usize, y: usize, z: usize) -> usize {
    let edge = usize::from(EDGE);
    x + edge * (z + edge * y)
}

fn dispatch_target(
    runtime: &mut VoxelRuntime<u8>,
    kind: DerivedKind,
    target: ChunkCoordinate,
) -> DerivedInput<u8> {
    loop {
        match runtime
            .dispatch_next(kind)
            .expect("fixture job identity remains in range")
        {
            DispatchOutcome::Started(input) if input.ticket().key().coordinate() == target => {
                return input;
            }
            DispatchOutcome::Started(input) => {
                let outcome = runtime.complete_derived(
                    input,
                    0_u8,
                    ApplyByteDeclaration::new(0),
                    FixedTick::new(1),
                    |_| Ok::<(), ()>(()),
                );
                assert!(matches!(outcome, CompletionOutcome::Applied { .. }));
            }
            other => panic!("target must remain dispatchable, got {other:?}"),
        }
    }
}

#[derive(Debug, Eq, PartialEq)]
struct HeapResult(Vec<u8>);

impl RetainedBytes for HeapResult {
    fn retained_bytes(&self) -> u64 {
        u64::try_from(mem::size_of::<Self>())
            .ok()
            .and_then(|inline| {
                u64::try_from(self.0.capacity())
                    .ok()
                    .and_then(|heap| inline.checked_add(heap))
            })
            .unwrap_or(u64::MAX)
    }
}

#[derive(Debug, Eq, PartialEq)]
struct ExpandingCell {
    cloned: bool,
}

impl Clone for ExpandingCell {
    fn clone(&self) -> Self {
        Self { cloned: true }
    }
}

impl RetainedBytes for ExpandingCell {
    fn retained_bytes(&self) -> u64 {
        if self.cloned { 4_096 } else { 1 }
    }
}

impl crate::CollisionSemantics for ExpandingCell {
    fn collision_occupied(&self) -> bool {
        true
    }
}

#[test]
fn receipt_projection_is_gated_replayable_and_fault_safe() {
    let runtime_scope = scope();
    let storage = MemoryTransactionKernel::new();
    let coordinate = ChunkCoordinate::new(0, 0, 0);
    let failed = build_transaction(&storage, &runtime_scope, coordinate, vec![1; CELL_COUNT], 7);
    storage
        .inject_fault_once(FaultPoint::AfterStagingBeforePublish)
        .expect("fixture installs one failpoint");
    assert!(storage.commit(failed).is_err());

    let mut runtime = runtime();
    assert!(!runtime.is_resident(coordinate));
    assert_eq!(
        storage
            .reference_snapshot(runtime_scope.world())
            .expect("failed commit leaves a readable snapshot")
            .len(),
        0
    );

    let cells = vec![3; CELL_COUNT];
    let (receipt, _) = commit_chunk(&storage, &runtime_scope, coordinate, cells.clone(), 8);
    let key = ChunkKey::new(
        runtime_scope.world(),
        runtime_scope.dimension().clone(),
        coordinate,
    );
    let projection = CommittedChunkProjection::from_commit_receipt(
        &receipt,
        key,
        EDGE,
        cells,
        mesh_fingerprint(1),
        collider_fingerprint(2),
    )
    .expect("receipt includes the committed chunk");
    assert!(matches!(
        projection.evidence(),
        ProjectionEvidence::CommitReceipt {
            transaction,
            durability: latticeaxiom_storage::ReferenceDurability::Queued,
        } if transaction == receipt.transaction_id()
    ));
    let admitted = runtime
        .project_committed(projection.clone(), FixedTick::new(4), requests())
        .expect("committed projection is admitted");
    assert_eq!(admitted.decision(), ProjectionDecision::Admitted);
    assert_eq!(admitted.world_revision(), receipt.world_revision());
    assert_eq!(
        runtime
            .cell(VoxelCoordinate::new(1, 1, 1))
            .expect("committed cell is resident"),
        &3
    );
    let replay = runtime
        .project_committed(projection, FixedTick::new(5), requests())
        .expect("exact projection replay is idempotent");
    assert_eq!(replay.decision(), ProjectionDecision::Replay);
    assert!(replay.derived().is_empty());
}

#[test]
fn projection_rejects_scope_capacity_conflict_and_regression() {
    let runtime_scope = scope();
    let storage = MemoryTransactionKernel::new();
    let first = ChunkCoordinate::new(0, 0, 0);
    let second = ChunkCoordinate::new(1, 0, 0);
    let (receipt, stored_first) =
        commit_chunk(&storage, &runtime_scope, first, vec![1; CELL_COUNT], 1);
    let (_, stored_second) = commit_chunk(&storage, &runtime_scope, second, vec![2; CELL_COUNT], 2);
    let absent_key = ChunkKey::new(
        runtime_scope.world(),
        runtime_scope.dimension().clone(),
        second,
    );
    assert!(matches!(
        CommittedChunkProjection::from_commit_receipt(
            &receipt,
            absent_key,
            EDGE,
            vec![2; CELL_COUNT],
            mesh_fingerprint(1),
            collider_fingerprint(1),
        ),
        Err(RuntimeError::ChunkMissingFromCommit { .. })
    ));

    let queue = queue_limits(8, 2, 1024 * 1024);
    let one =
        RuntimeLimits::new(1, 2 * 1024 * 1024, queue, queue).expect("fixture limits are positive");
    let mut bounded = VoxelRuntime::new(
        runtime_scope.clone(),
        RuntimeGeneration::new(1),
        EDGE,
        0_u8,
        one,
    )
    .expect("runtime is valid");
    project_stored(&mut bounded, &stored_first, 0, 1, 1);
    assert!(matches!(
        bounded.project_committed(
            projection_from_stored(&stored_second, 1, 1),
            FixedTick::new(1),
            requests(),
        ),
        Err(RuntimeError::ResidentLimitExceeded { limit: 1 })
    ));
    let conflicting = CommittedChunkProjection::from_stored_chunk(
        &stored_first,
        EDGE,
        vec![9; CELL_COUNT],
        mesh_fingerprint(1),
        collider_fingerprint(1),
    )
    .expect("fixture preserves cubic shape");
    assert!(matches!(
        bounded.project_committed(conflicting, FixedTick::new(1), requests()),
        Err(RuntimeError::ConflictingProjection { .. })
    ));

    let other_world: WorldId = "00000000-0000-4000-8000-000000000002"
        .parse()
        .expect("fixture is a canonical version-4 world UUID");
    let mut other = VoxelRuntime::new(
        WorkingSetScope::new(
            other_world,
            runtime_scope.dimension().clone(),
            WorldEpoch::new(7),
        ),
        RuntimeGeneration::new(2),
        EDGE,
        0_u8,
        limits(),
    )
    .expect("runtime is valid");
    assert!(matches!(
        other.project_committed(
            projection_from_stored(&stored_first, 1, 1),
            FixedTick::new(0),
            requests(),
        ),
        Err(RuntimeError::ScopeMismatch { .. })
    ));

    let (_, newer) = commit_chunk(&storage, &runtime_scope, first, vec![4; CELL_COUNT], 3);
    let mut revision_runtime = runtime();
    project_stored(&mut revision_runtime, &newer, 2, 1, 1);
    assert!(matches!(
        revision_runtime.project_committed(
            projection_from_stored(&stored_first, 1, 1),
            FixedTick::new(3),
            requests(),
        ),
        Err(RuntimeError::StaleProjection { .. })
    ));
}

#[test]
fn semantic_fingerprints_are_separate_and_stale_work_never_applies() {
    let runtime_scope = scope();
    let storage = MemoryTransactionKernel::new();
    let coordinate = ChunkCoordinate::new(0, 0, 0);
    let (_, first) = commit_chunk(&storage, &runtime_scope, coordinate, vec![1; CELL_COUNT], 1);
    let mut runtime = runtime();
    let initial = runtime
        .project_committed(
            projection_from_stored(&first, 1, 2),
            FixedTick::new(0),
            requests(),
        )
        .expect("initial projection is admitted");
    let mesh = initial
        .derived()
        .iter()
        .find(|receipt| receipt.key().kind() == DerivedKind::Mesh)
        .expect("mesh is scheduled")
        .key()
        .source_fingerprint();
    let collider = initial
        .derived()
        .iter()
        .find(|receipt| receipt.key().kind() == DerivedKind::Collider)
        .expect("collider is scheduled")
        .key()
        .source_fingerprint();
    assert_ne!(mesh, collider);
    let input = dispatch_target(&mut runtime, DerivedKind::Mesh, coordinate);

    let mesh_only = runtime
        .project_committed(
            projection_from_stored(&first, 3, 2),
            FixedTick::new(1),
            requests(),
        )
        .expect("mesh semantics update independently");
    assert_eq!(mesh_only.derived().len(), 1);
    assert_eq!(mesh_only.derived()[0].key().kind(), DerivedKind::Mesh);
    let collider_only = runtime
        .project_committed(
            projection_from_stored(&first, 3, 4),
            FixedTick::new(2),
            requests(),
        )
        .expect("collider semantics update independently");
    assert_eq!(collider_only.derived().len(), 1);
    assert_eq!(
        collider_only.derived()[0].key().kind(),
        DerivedKind::Collider
    );

    let (_, newer) = commit_chunk(&storage, &runtime_scope, coordinate, vec![2; CELL_COUNT], 2);
    project_stored(&mut runtime, &newer, 4, 3, 4);
    let applied = Cell::new(false);
    let outcome = runtime.complete_derived(
        input,
        0_u8,
        ApplyByteDeclaration::new(0),
        FixedTick::new(6),
        |_| {
            applied.set(true);
            Ok::<(), ()>(())
        },
    );
    assert!(matches!(
        outcome,
        CompletionOutcome::StaleRejected {
            reason: StaleReason::WorldRevision,
            ..
        }
    ));
    assert!(!applied.get());
    assert_eq!(runtime.diagnostics().combined_reserved_bytes(), 0);
}
#[test]
fn eviction_permit_retains_cancelled_resources_until_executor_ack() {
    let runtime_scope = scope();
    let storage = MemoryTransactionKernel::new();
    let coordinate = ChunkCoordinate::new(0, 0, 0);
    let (_, stored) = commit_chunk(&storage, &runtime_scope, coordinate, vec![1; CELL_COUNT], 1);
    let mut runtime = runtime();
    project_stored(&mut runtime, &stored, 0, 1, 1);
    let input = dispatch_target(&mut runtime, DerivedKind::Mesh, coordinate);
    let ticket = input.ticket().clone();
    let reserved = ticket.reserved_bytes();
    let permit = runtime
        .prepare_eviction(coordinate, EvictionLeaseGeneration::new(9))
        .expect("resident projection is clean by construction");
    let receipt = runtime
        .evict_committed(permit, FixedTick::new(2), requests())
        .expect("exact permit evicts");
    assert!(!runtime.is_resident(coordinate));
    assert_eq!(receipt.cancellations().len(), 1);
    assert_eq!(receipt.cancellations()[0].job(), ticket.id());
    assert_eq!(receipt.cancellations()[0].owner(), ticket.owner());
    assert_eq!(
        receipt.cancellations()[0].token(),
        ticket.cancellation_token()
    );
    assert_eq!(runtime.diagnostics().combined_reserved_bytes(), reserved);
    assert_eq!(runtime.diagnostics().mesh().cancel_requested(), 1);

    let result = HeapResult(vec![0; 32]);
    let result_bytes = result.retained_bytes();
    let acknowledgement = runtime.acknowledge_cancelled(input, Some(result));
    let CancellationAckOutcome::Released(release) = acknowledgement else {
        panic!("cancel acknowledgement must release owned buffers");
    };
    assert_eq!(release.input_bytes(), ticket.input_bytes());
    assert_eq!(release.result_bytes(), result_bytes);
    assert_eq!(release.released_reserved_bytes(), reserved);
    assert_eq!(runtime.diagnostics().combined_reserved_bytes(), 0);
    assert_eq!(runtime.diagnostics().mesh().in_flight(), 0);
}

#[test]
fn stale_eviction_permit_and_foreign_runtime_cannot_release_owner_state() {
    let runtime_scope = scope();
    let storage = MemoryTransactionKernel::new();
    let coordinate = ChunkCoordinate::new(0, 0, 0);
    let (_, first) = commit_chunk(&storage, &runtime_scope, coordinate, vec![1; CELL_COUNT], 1);
    let mut owner = runtime();
    project_stored(&mut owner, &first, 0, 1, 1);
    let permit = owner
        .prepare_eviction(coordinate, EvictionLeaseGeneration::new(1))
        .expect("projection is resident");
    let input = dispatch_target(&mut owner, DerivedKind::Mesh, coordinate);
    let reserved = input.ticket().reserved_bytes();

    let (_, newer) = commit_chunk(&storage, &runtime_scope, coordinate, vec![2; CELL_COUNT], 2);
    project_stored(&mut owner, &newer, 1, 1, 1);
    assert!(matches!(
        owner.evict_committed(permit, FixedTick::new(2), requests()),
        Err(RuntimeError::StaleEvictionPermit { .. })
    ));

    let mut foreign = VoxelRuntime::new(
        runtime_scope,
        RuntimeGeneration::new(12),
        EDGE,
        0_u8,
        limits(),
    )
    .expect("foreign runtime is valid");
    project_stored(&mut foreign, &newer, 1, 1, 1);
    let outcome = foreign.complete_derived(
        input,
        7_u8,
        ApplyByteDeclaration::new(0),
        FixedTick::new(2),
        |_| Ok::<(), ()>(()),
    );
    let CompletionOutcome::UnknownJob { input, result, .. } = outcome else {
        panic!("foreign runtime must return resources");
    };
    assert_eq!(owner.diagnostics().combined_reserved_bytes(), reserved);
    let accepted = owner.complete_derived(
        input,
        result,
        ApplyByteDeclaration::new(0),
        FixedTick::new(3),
        |_| Ok::<(), ()>(()),
    );
    assert!(matches!(
        accepted,
        CompletionOutcome::StaleRejected {
            reason: StaleReason::WorldRevision,
            ..
        }
    ));
    assert_eq!(owner.diagnostics().combined_reserved_bytes(), 0);
}

fn seeded_cells(seed: u8, coordinate: ChunkCoordinate) -> Vec<u8> {
    (0..CELL_COUNT)
        .map(|index| {
            seed.wrapping_add(coordinate.x.to_le_bytes()[0])
                .wrapping_add(coordinate.y.to_le_bytes()[0])
                .wrapping_add(coordinate.z.to_le_bytes()[0])
                .wrapping_add(u8::try_from(index).expect("cell index fits u8"))
        })
        .collect()
}

#[test]
fn dirty_edited_chunk_cannot_be_evicted() {
    let runtime_scope = scope();
    let storage = MemoryTransactionKernel::new();
    let coordinate = ChunkCoordinate::new(0, 0, 0);
    let (_, stored) = commit_chunk(
        &storage,
        &runtime_scope,
        coordinate,
        seeded_cells(1, coordinate),
        1,
    );
    let mut runtime = runtime();
    project_stored(&mut runtime, &stored, 0, 1, 1);
    assert!(matches!(
        runtime.evict_clean_outside_interest(
            EvictionLeaseGeneration::new(2),
            FixedTick::new(0),
            requests(),
        ),
        Err(RuntimeError::InterestWindowRequired)
    ));
    runtime
        .mark_dirty(coordinate)
        .expect("resident generated chunk can be pinned");
    assert!(runtime.is_dirty(coordinate));
    assert!(matches!(
        runtime.prepare_eviction(coordinate, EvictionLeaseGeneration::new(1)),
        Err(RuntimeError::DirtyChunkPinned { .. })
    ));
}

#[test]
fn voxel_changing_update_pins_the_edited_projection() {
    let runtime_scope = scope();
    let storage = MemoryTransactionKernel::new();
    let coordinate = ChunkCoordinate::new(0, 0, 0);
    let (_, first) = commit_chunk(
        &storage,
        &runtime_scope,
        coordinate,
        seeded_cells(1, coordinate),
        1,
    );
    let mut runtime = runtime();
    project_stored(&mut runtime, &first, 0, 1, 1);
    assert!(!runtime.is_dirty(coordinate));
    let (_, edited) = commit_chunk(
        &storage,
        &runtime_scope,
        coordinate,
        seeded_cells(2, coordinate),
        2,
    );
    project_stored(&mut runtime, &edited, 1, 1, 1);
    assert!(runtime.is_dirty(coordinate));
    assert!(matches!(
        runtime.prepare_eviction(coordinate, EvictionLeaseGeneration::new(1)),
        Err(RuntimeError::DirtyChunkPinned { .. })
    ));
}

#[test]
#[allow(
    clippy::too_many_lines,
    reason = "one test covers cap, dirty pin, eviction, and identical regeneration"
)]
fn bounded_residency_evicts_clean_chunks_and_regenerates_identically() {
    let runtime_scope = scope();
    let storage = MemoryTransactionKernel::new();
    let seed = 7_u8;
    let cap = 4;
    let max_in_flight = 2;
    let queue = queue_limits(32, max_in_flight, 1024 * 1024);
    let limits = RuntimeLimits::new(cap, 2 * 1024 * 1024, queue, queue)
        .expect("fixture limits are positive");
    let mut runtime = VoxelRuntime::new(
        runtime_scope.clone(),
        RuntimeGeneration::new(1),
        EDGE,
        0_u8,
        limits,
    )
    .expect("runtime is valid");
    let origin = ChunkCoordinate::new(0, 0, 0);
    runtime.set_interest(InterestWindow::new(origin, 0));

    let origin_cells = seeded_cells(seed, origin);
    let (_, origin_stored) =
        commit_chunk(&storage, &runtime_scope, origin, origin_cells.clone(), 1);
    project_stored(&mut runtime, &origin_stored, 0, 1, 1);
    runtime.mark_dirty(origin).expect("origin edit is pinned");
    assert_eq!(
        runtime.dirty_coordinates().collect::<Vec<_>>(),
        vec![origin]
    );

    let mut generated = BTreeMap::new();
    let mut stored_by_coordinate = BTreeMap::new();
    generated.insert(origin, origin_cells);
    stored_by_coordinate.insert(origin, origin_stored);
    let mut transaction = 2_u128;
    for x in 1_i32..=8 {
        let coordinate = ChunkCoordinate::new(x, 0, 0);
        let cells = seeded_cells(seed, coordinate);
        generated.insert(coordinate, cells.clone());
        let (_, stored) = commit_chunk(&storage, &runtime_scope, coordinate, cells, transaction);
        transaction = transaction.saturating_add(1);
        stored_by_coordinate.insert(coordinate, stored.clone());
        project_stored(
            &mut runtime,
            &stored,
            u64::from(u32::try_from(x).expect("x fits u32")),
            1,
            1,
        );
        assert!(runtime.diagnostics().resident_chunks() <= cap);
        assert!(runtime.diagnostics().mesh().in_flight() <= max_in_flight);
        assert!(runtime.diagnostics().collider().in_flight() <= max_in_flight);
        assert!(runtime.diagnostics().in_flight_chunks() <= max_in_flight.saturating_mul(2));
        assert!(runtime.diagnostics().active_chunks() <= runtime.diagnostics().resident_chunks());
        assert!(runtime.diagnostics().visible_chunks() <= runtime.diagnostics().resident_chunks());
        assert_eq!(runtime.diagnostics().saving_chunks(), 0);
        assert_eq!(runtime.diagnostics().byte_budget(), 2 * 1024 * 1024);
    }

    assert_eq!(runtime.diagnostics().resident_high_water(), cap);
    assert!(runtime.diagnostics().resident_high_water() <= cap);
    assert!(runtime.is_resident(origin));
    assert!(runtime.is_dirty(origin));
    assert_eq!(runtime.diagnostics().dirty_chunks(), 1);
    assert_eq!(runtime.diagnostics().saving_chunks(), 0);
    assert_eq!(runtime.diagnostics().byte_budget(), 2 * 1024 * 1024);
    assert!(!runtime.is_resident(ChunkCoordinate::new(3, 0, 0)));

    let mut started = Vec::new();
    loop {
        match runtime
            .dispatch_next(DerivedKind::Mesh)
            .expect("job identity remains in range")
        {
            DispatchOutcome::Started(input) => {
                assert!(runtime.diagnostics().mesh().in_flight() <= max_in_flight);
                started.push(input);
            }
            DispatchOutcome::Empty | DispatchOutcome::Backpressured { .. } => break,
            DispatchOutcome::MemoryContractViolation { .. } => {
                panic!("fixture halo estimate must cover actual input bytes")
            }
        }
    }
    assert!(runtime.diagnostics().mesh().in_flight_high_water() <= max_in_flight);
    for input in started {
        assert!(matches!(
            runtime.complete_derived(
                input,
                0_u8,
                ApplyByteDeclaration::new(0),
                FixedTick::new(20),
                |_| Ok::<(), ()>(()),
            ),
            CompletionOutcome::Applied { .. }
                | CompletionOutcome::Cancelled { .. }
                | CompletionOutcome::StaleRejected { .. }
        ));
    }

    let evicted = runtime
        .evict_clean_outside_interest(
            EvictionLeaseGeneration::new(99),
            FixedTick::new(21),
            requests(),
        )
        .expect("interest window is set");
    assert!(!evicted.is_empty());
    assert_eq!(runtime.diagnostics().resident_chunks(), 1);
    assert!(runtime.is_resident(origin));
    assert!(runtime.is_dirty(origin));
    assert!(
        runtime
            .prepare_eviction(origin, EvictionLeaseGeneration::new(100))
            .is_err()
    );

    let regenerated = ChunkCoordinate::new(5, 0, 0);
    let original = generated
        .get(&regenerated)
        .expect("original generated bytes were captured")
        .clone();
    let again = seeded_cells(seed, regenerated);
    assert_eq!(original, again);
    let restored = stored_by_coordinate
        .get(&regenerated)
        .expect("first publication remains available after runtime eviction");
    project_stored(&mut runtime, restored, 30, 1, 1);
    assert!(runtime.is_resident(regenerated));
    assert!(!runtime.is_dirty(regenerated));
    assert_eq!(
        runtime
            .cell(VoxelCoordinate::new(
                i64::from(regenerated.x) * i64::from(EDGE),
                0,
                0
            ))
            .expect("regenerated cell is resident"),
        &again[0]
    );
}

#[test]
fn in_interest_clean_chunks_are_not_auto_evicted_at_capacity() {
    let runtime_scope = scope();
    let storage = MemoryTransactionKernel::new();
    let queue = queue_limits(8, 2, 1024 * 1024);
    let limits =
        RuntimeLimits::new(2, 2 * 1024 * 1024, queue, queue).expect("fixture limits are positive");
    let mut runtime = VoxelRuntime::new(
        runtime_scope.clone(),
        RuntimeGeneration::new(1),
        EDGE,
        0_u8,
        limits,
    )
    .expect("runtime is valid");
    let origin = ChunkCoordinate::new(0, 0, 0);
    runtime.set_interest(InterestWindow::new(origin, 1));
    for (index, coordinate) in [origin, ChunkCoordinate::new(1, 0, 0)]
        .into_iter()
        .enumerate()
    {
        let (_, stored) = commit_chunk(
            &storage,
            &runtime_scope,
            coordinate,
            seeded_cells(3, coordinate),
            u128::try_from(index).expect("index fits") + 1,
        );
        project_stored(
            &mut runtime,
            &stored,
            u64::try_from(index).expect("index fits u64"),
            1,
            1,
        );
    }
    let outside = ChunkCoordinate::new(2, 0, 0);
    let (_, stored) = commit_chunk(
        &storage,
        &runtime_scope,
        outside,
        seeded_cells(3, outside),
        3,
    );
    assert!(matches!(
        runtime.project_committed(
            projection_from_stored(&stored, 1, 1),
            FixedTick::new(3),
            requests(),
        ),
        Err(RuntimeError::ResidentLimitExceeded { limit: 2 })
    ));
    assert!(runtime.is_resident(origin));
    assert!(runtime.is_resident(ChunkCoordinate::new(1, 0, 0)));
    assert!(!runtime.is_resident(outside));
}

#[test]
#[allow(
    clippy::too_many_lines,
    reason = "one regression covers the three stages of the combined memory contract"
)]
fn complete_input_result_apply_ledger_fails_closed() {
    let runtime_scope = scope();
    let storage = MemoryTransactionKernel::new();
    let coordinate = ChunkCoordinate::new(0, 0, 0);
    let (_, stored) = commit_chunk(&storage, &runtime_scope, coordinate, vec![1; CELL_COUNT], 1);
    let tiny = DerivedRequestSet::new(request(1, 0, 8, 4), request(2, 0, 8, 4));
    let mut runtime = runtime();
    runtime
        .project_committed(
            projection_from_stored(&stored, 1, 1),
            FixedTick::new(0),
            tiny,
        )
        .expect("projection fits");
    let input = dispatch_target(&mut runtime, DerivedKind::Collider, coordinate);
    let called = Cell::new(false);
    let outcome = runtime.complete_derived(
        input,
        HeapResult(vec![0; 64]),
        ApplyByteDeclaration::new(0),
        FixedTick::new(1),
        |_| {
            called.set(true);
            Ok::<(), ()>(())
        },
    );
    assert!(matches!(
        outcome,
        CompletionOutcome::MemoryContractViolation {
            stage: MemoryStage::Result,
            ..
        }
    ));
    assert!(!called.get());
    assert!(matches!(
        runtime.collider_safety(coordinate),
        Some(ColliderSafetyState::FailedConservative {
            failure: ColliderFailure::MemoryContractViolation,
            ..
        })
    ));
    assert_eq!(runtime.diagnostics().combined_reserved_bytes(), 0);

    runtime
        .request_derived(
            coordinate,
            DerivedKind::Collider,
            FixedTick::new(2),
            request(2, 0, 128, 4),
        )
        .expect("retry is queued");
    let input = dispatch_target(&mut runtime, DerivedKind::Collider, coordinate);
    let called = Cell::new(false);
    let outcome = runtime.complete_derived(
        input,
        0_u8,
        ApplyByteDeclaration::new(5),
        FixedTick::new(3),
        |_| {
            called.set(true);
            Ok::<(), ()>(())
        },
    );
    assert!(matches!(
        outcome,
        CompletionOutcome::MemoryContractViolation {
            stage: MemoryStage::Apply,
            budget: 4,
            actual: 5,
            ..
        }
    ));
    assert!(!called.get());
    assert_eq!(runtime.diagnostics().combined_reserved_bytes(), 0);

    let custom_cells = (0..CELL_COUNT)
        .map(|_| ExpandingCell { cloned: false })
        .collect();
    let projection = CommittedChunkProjection::from_stored_chunk(
        &stored,
        EDGE,
        custom_cells,
        mesh_fingerprint(1),
        collider_fingerprint(1),
    )
    .expect("custom decode preserves cubic shape");
    let mut custom = VoxelRuntime::new(
        scope(),
        RuntimeGeneration::new(1),
        EDGE,
        ExpandingCell { cloned: false },
        limits(),
    )
    .expect("custom runtime is valid");
    custom
        .project_committed(projection, FixedTick::new(0), requests())
        .expect("projection is admitted");
    assert!(matches!(
        custom
            .dispatch_next(DerivedKind::Collider)
            .expect("job identity remains in range"),
        DispatchOutcome::MemoryContractViolation {
            actual_input_bytes,
            reserved_input_bytes,
            ..
        } if actual_input_bytes > reserved_input_bytes
    ));
    assert_eq!(custom.diagnostics().collider().in_flight(), 0);
    assert_eq!(custom.diagnostics().combined_reserved_bytes(), 0);
}

#[test]
fn collider_ready_follows_host_success_and_failures_remain_conservative() {
    let runtime_scope = scope();
    let storage = MemoryTransactionKernel::new();
    let coordinate = ChunkCoordinate::new(0, 0, 0);
    let (_, stored) = commit_chunk(&storage, &runtime_scope, coordinate, vec![1; CELL_COUNT], 1);
    let mut runtime = runtime();
    project_stored(&mut runtime, &stored, 10, 1, 1);
    assert!(matches!(
        runtime.collider_safety(coordinate),
        Some(ColliderSafetyState::PendingConservative { .. })
    ));
    assert!(
        runtime
            .projected_collision_occupied(VoxelCoordinate::new(0, 0, 0))
            .expect("committed occupancy is resident")
    );

    let failed = dispatch_target(&mut runtime, DerivedKind::Collider, coordinate);
    let outcome = runtime.complete_derived(
        failed,
        0_u8,
        ApplyByteDeclaration::new(1),
        FixedTick::new(11),
        |_| Err::<(), _>("backend rejected"),
    );
    assert!(matches!(
        outcome,
        CompletionOutcome::ApplyFailed {
            error: "backend rejected",
            ..
        }
    ));
    assert!(matches!(
        runtime.collider_safety(coordinate),
        Some(ColliderSafetyState::FailedConservative {
            failure: ColliderFailure::ApplyRejected,
            ..
        })
    ));
    runtime
        .request_derived(
            coordinate,
            DerivedKind::Collider,
            FixedTick::new(12),
            request(2, 0, 512, 256),
        )
        .expect("retry is queued");
    let successful = dispatch_target(&mut runtime, DerivedKind::Collider, coordinate);
    let successful_key = successful.ticket().key().clone();
    let host_applied = Cell::new(false);
    let outcome = runtime.complete_derived(
        successful,
        0_u8,
        ApplyByteDeclaration::new(1),
        FixedTick::new(14),
        |_| {
            host_applied.set(true);
            Ok::<_, ()>("collider handle")
        },
    );
    let CompletionOutcome::Applied { receipt, value } = outcome else {
        panic!("successful host apply must produce a receipt");
    };
    assert!(host_applied.get());
    assert_eq!(value, "collider handle");
    assert_eq!(receipt.key(), &successful_key);
    assert_eq!(receipt.commit_to_apply_ticks(), 2);
    assert!(matches!(
        runtime.collider_safety(coordinate),
        Some(ColliderSafetyState::Ready {
            source_fingerprint,
            world_revision,
            voxel_revision,
        }) if *source_fingerprint == successful_key.source_fingerprint()
            && *world_revision == successful_key.world_revision()
            && *voxel_revision == successful_key.voxel_revision()
    ));
}

#[test]
fn host_apply_panic_is_contained_without_marking_applied() {
    let runtime_scope = scope();
    let storage = MemoryTransactionKernel::new();
    let coordinate = ChunkCoordinate::new(0, 0, 0);
    let (_, stored) = commit_chunk(&storage, &runtime_scope, coordinate, vec![1; CELL_COUNT], 1);
    let mut runtime = runtime();
    project_stored(&mut runtime, &stored, 0, 1, 1);
    let input = dispatch_target(&mut runtime, DerivedKind::Collider, coordinate);
    let outcome = runtime.complete_derived(
        input,
        0_u8,
        ApplyByteDeclaration::new(0),
        FixedTick::new(1),
        |_| -> Result<(), ()> { panic!("fixture backend panic") },
    );
    assert!(matches!(outcome, CompletionOutcome::ApplyPanicked { .. }));
    assert!(matches!(
        runtime.collider_safety(coordinate),
        Some(ColliderSafetyState::FailedConservative {
            failure: ColliderFailure::ApplyPanicked,
            ..
        })
    ));
    assert!(
        runtime
            .last_applied_key(coordinate, DerivedKind::Collider)
            .is_none()
    );
    assert_eq!(runtime.diagnostics().combined_reserved_bytes(), 0);
}

#[test]
fn combined_ledger_backpressures_cross_kind_dispatch() {
    let runtime_scope = scope();
    let storage = MemoryTransactionKernel::new();
    let coordinate = ChunkCoordinate::new(0, 0, 0);
    let (_, stored) = commit_chunk(&storage, &runtime_scope, coordinate, vec![1; CELL_COUNT], 1);
    let input_bytes = u64::try_from(mem::size_of::<DerivedInput<u8>>())
        .expect("type size fits u64")
        + u64::try_from((usize::from(EDGE) + 2).pow(3)).expect("halo volume fits u64");
    let job_bytes = input_bytes + 1;
    let queue = queue_limits(8, 2, job_bytes * 4);
    let global_budget = job_bytes * 2 - 1;
    let limits =
        RuntimeLimits::new(4, global_budget, queue, queue).expect("calculated limits are positive");
    let no_extra = DerivedRequestSet::new(request(1, 0, 1, 0), request(2, 0, 1, 0));
    let mut runtime =
        VoxelRuntime::new(runtime_scope, RuntimeGeneration::new(1), EDGE, 0_u8, limits)
            .expect("runtime is valid");
    runtime
        .project_committed(
            projection_from_stored(&stored, 1, 1),
            FixedTick::new(0),
            no_extra,
        )
        .expect("each individual job fits");
    let DispatchOutcome::Started(mesh) = runtime
        .dispatch_next(DerivedKind::Mesh)
        .expect("job identity remains in range")
    else {
        panic!("mesh must acquire the first reservation");
    };
    assert_eq!(mesh.ticket().reserved_bytes(), job_bytes);
    assert!(matches!(
        runtime
            .dispatch_next(DerivedKind::Collider)
            .expect("job identity remains in range"),
        DispatchOutcome::Backpressured {
            reason: BackpressureReason::CombinedReservedBytes
        }
    ));
    assert!(matches!(
        runtime.complete_derived(
            mesh,
            0_u8,
            ApplyByteDeclaration::new(0),
            FixedTick::new(1),
            |_| Ok::<(), ()>(()),
        ),
        CompletionOutcome::Applied { .. }
    ));
    assert!(matches!(
        runtime
            .dispatch_next(DerivedKind::Collider)
            .expect("released bytes admit collider"),
        DispatchOutcome::Started(_)
    ));
}
#[test]
fn dda_uses_committed_y_up_revisions_and_missing_is_not_air() {
    let runtime_scope = scope();
    let storage = MemoryTransactionKernel::new();
    let coordinate = ChunkCoordinate::new(0, 0, 0);
    let mut cells = vec![0; CELL_COUNT];
    cells[index(2, 1, 1)] = 7;
    cells[index(1, 2, 1)] = 8;
    let (_, stored) = commit_chunk(&storage, &runtime_scope, coordinate, cells, 1);
    let mut runtime = runtime();
    project_stored(&mut runtime, &stored, 0, 1, 1);

    let outcome = runtime
        .raycast_committed(
            DdaQuery::new(
                DdaOrigin::new(coordinate, [0.5, 1.5, 1.5]),
                [1.0, 0.0, 0.0],
                5.0,
            ),
            |value| {
                if *value == 7 {
                    CellSelection::Target
                } else {
                    CellSelection::PassThrough
                }
            },
        )
        .expect("valid bounded query succeeds");
    let DdaOutcome::Hit(hit) = outcome else {
        panic!("committed target must be selected");
    };
    assert_eq!(hit.coordinate(), VoxelCoordinate::new(2, 1, 1));
    assert_eq!(hit.entered_face(), Some(Face::NegX));
    assert_eq!(
        hit.adjacent_placement(),
        Some(VoxelCoordinate::new(1, 1, 1))
    );
    let revisions = runtime
        .chunk_revisions(coordinate)
        .expect("projection is resident");
    assert_eq!(
        (hit.world_revision(), hit.revision(), hit.voxel_revision()),
        revisions
    );

    let upward = runtime
        .raycast_committed(
            DdaQuery::new(
                DdaOrigin::new(coordinate, [1.5, 0.5, 1.5]),
                [0.0, 1.0, 0.0],
                5.0,
            ),
            |value| {
                if *value == 8 {
                    CellSelection::Target
                } else {
                    CellSelection::PassThrough
                }
            },
        )
        .expect("native +Y query succeeds");
    assert!(matches!(
        upward,
        DdaOutcome::Hit(cell)
            if cell.coordinate() == VoxelCoordinate::new(1, 2, 1)
                && cell.entered_face() == Some(Face::NegY)
    ));

    let unavailable = runtime
        .raycast_committed(
            DdaQuery::new(
                DdaOrigin::new(coordinate, [3.5, 0.5, 0.5]),
                [1.0, 0.0, 0.0],
                1.0,
            ),
            |_| CellSelection::PassThrough,
        )
        .expect("bounded query succeeds");
    assert!(matches!(
        unavailable,
        DdaOutcome::Unavailable(missing)
            if missing.coordinate() == VoxelCoordinate::new(4, 0, 0)
                && (missing.distance() - 0.5).abs() < f64::EPSILON
    ));
}

#[test]
fn dda_validates_reach_and_negative_boundary_ownership() {
    let runtime_scope = scope();
    let storage = MemoryTransactionKernel::new();
    let coordinate = ChunkCoordinate::new(-1, 0, 0);
    let mut cells = vec![0; CELL_COUNT];
    cells[index(3, 0, 0)] = 1;
    let (_, stored) = commit_chunk(&storage, &runtime_scope, coordinate, cells, 1);
    let mut runtime = runtime();
    project_stored(&mut runtime, &stored, 0, 1, 1);
    let hit = runtime
        .raycast_committed(
            DdaQuery::new(
                DdaOrigin::new(ChunkCoordinate::new(0, 0, 0), [0.0, 0.5, 0.5]),
                [-1.0, 0.0, 0.0],
                1.0,
            ),
            |value| {
                if *value == 1 {
                    CellSelection::Target
                } else {
                    CellSelection::PassThrough
                }
            },
        )
        .expect("negative boundary query is valid");
    assert!(matches!(
        hit,
        DdaOutcome::Hit(cell)
            if cell.coordinate() == VoxelCoordinate::new(-1, 0, 0)
                && cell.distance() == 0.0
                && cell.entered_face().is_none()
    ));
    assert!(matches!(
        runtime.raycast_committed(
            DdaQuery::new(
                DdaOrigin::new(coordinate, [1.0, 1.0, 1.0]),
                [1.0, 0.0, 0.0],
                5.001,
            ),
            |_| CellSelection::PassThrough,
        ),
        Err(RuntimeError::ReachExceeded {
            maximum_millimeters: 5_000,
            ..
        })
    ));
}

#[test]
fn cpu_heavy_concurrency_reserves_two_cores() {
    assert_eq!(cpu_heavy_concurrency(0), 1);
    assert_eq!(cpu_heavy_concurrency(1), 1);
    assert_eq!(cpu_heavy_concurrency(2), 1);
    assert_eq!(cpu_heavy_concurrency(6), 4);
    assert_eq!(cpu_heavy_concurrency(8), 6);
    assert!(host_parallelism() >= 1);
}

#[test]
fn dispatch_is_split_from_apply_and_respects_cpu_heavy_cap() {
    let runtime_scope = scope();
    let storage = MemoryTransactionKernel::new();
    let first = ChunkCoordinate::new(0, 0, 0);
    let second = ChunkCoordinate::new(1, 0, 0);
    let (_, stored_first) = commit_chunk(&storage, &runtime_scope, first, vec![1; CELL_COUNT], 1);
    let (_, stored_second) = commit_chunk(&storage, &runtime_scope, second, vec![2; CELL_COUNT], 2);
    let queue = queue_limits(8, 4, 1024 * 1024);
    let limits = RuntimeLimits::new(4, 2 * 1024 * 1024, queue, queue)
        .expect("fixture limits are positive")
        .with_cpu_heavy_concurrency(1)
        .expect("cpu-heavy cap is positive");
    let mut runtime =
        VoxelRuntime::new(runtime_scope, RuntimeGeneration::new(1), EDGE, 0_u8, limits)
            .expect("runtime is valid");
    project_stored(&mut runtime, &stored_first, 0, 1, 1);
    project_stored(&mut runtime, &stored_second, 1, 1, 1);

    let mesh = match runtime
        .dispatch_next(DerivedKind::Mesh)
        .expect("job identity remains in range")
    {
        DispatchOutcome::Started(input) => input,
        other => panic!("first mesh job must start, got {other:?}"),
    };
    assert_eq!(runtime.in_flight_jobs(), 1);
    assert!(matches!(
        runtime
            .dispatch_next(DerivedKind::Mesh)
            .expect("job identity remains in range"),
        DispatchOutcome::Backpressured {
            reason: BackpressureReason::InFlightJobs
        }
    ));
    assert!(matches!(
        runtime
            .dispatch_next(DerivedKind::Collider)
            .expect("job identity remains in range"),
        DispatchOutcome::Backpressured {
            reason: BackpressureReason::InFlightJobs
        }
    ));

    runtime.record_waiting_to_apply(32);
    assert_eq!(runtime.diagnostics().waiting_to_apply_jobs(), 1);
    assert_eq!(runtime.diagnostics().waiting_to_apply_bytes(), 32);
    let snapshot = runtime.admission_snapshot();
    assert_eq!(snapshot.in_flight_jobs(), 1);
    assert_eq!(snapshot.in_flight_bytes(), runtime.in_flight_bytes());
    assert_eq!(snapshot.waiting_to_apply_bytes(), 32);
    assert_eq!(snapshot.cpu_heavy_slots_remaining(), 0);
    runtime.consume_waiting_to_apply(32);
    assert_eq!(runtime.diagnostics().waiting_to_apply_jobs(), 0);
    assert_eq!(runtime.diagnostics().waiting_to_apply_bytes(), 0);

    assert!(matches!(
        runtime.complete_derived(
            mesh,
            0_u8,
            ApplyByteDeclaration::new(0),
            FixedTick::new(2),
            |_| Ok::<(), ()>(()),
        ),
        CompletionOutcome::Applied { .. }
    ));
    assert_eq!(runtime.in_flight_jobs(), 0);
    assert!(matches!(
        runtime
            .dispatch_next(DerivedKind::Mesh)
            .expect("released cap admits the next job"),
        DispatchOutcome::Started(_)
    ));
}

#[test]
fn desktop_reference_queue_caps_and_soft_high_water_match_adr_0026() {
    let mesh =
        DerivedQueueLimits::mesh_desktop_reference_v1().expect("accepted mesh caps are nonzero");
    let collider = DerivedQueueLimits::collider_desktop_reference_v1()
        .expect("accepted collider caps are nonzero");
    assert_eq!(mesh.max_pending(), 128);
    assert_eq!(mesh.max_reserved_bytes(), 128 * 1024 * 1024);
    assert_eq!(collider.max_pending(), 64);
    assert_eq!(collider.max_reserved_bytes(), 64 * 1024 * 1024);
    assert_eq!(mesh.soft_high_water_jobs(), 96);
    assert_eq!(collider.soft_high_water_jobs(), 48);
    assert!(mesh.at_soft_high_water(96, 0));
    assert!(!mesh.at_soft_high_water(95, 0));
    let limits = RuntimeLimits::new(64, RuntimeLimits::COMBINED_BYTE_CAP, mesh, collider)
        .expect("accepted combined caps are nonzero");
    assert_eq!(limits.max_combined_in_flight(), 192);
    assert_eq!(limits.max_combined_reserved_bytes(), 384 * 1024 * 1024);
    assert_eq!(limits.combined_soft_high_water_jobs(), 144);
    assert_eq!(
        limits.combined_soft_high_water_bytes(),
        384 * 1024 * 1024 * 75 / 100
    );
    assert_eq!(RuntimeLimits::MAIN_WORLD_APPLY_JOB_CAP, 16);
    assert_eq!(RuntimeLimits::MAIN_WORLD_APPLY_BYTE_CAP, 16 * 1024 * 1024);
    assert_eq!(RuntimeLimits::MAIN_WORLD_APPLY_WALL_CLOCK_NANOS, 2_000_000);
    let apply = RuntimeLimits::main_world_apply_budget();
    assert_eq!(apply, DerivedApplyBudget::desktop_reference_v1());
    assert_eq!(apply.max_jobs(), 16);
    assert_eq!(apply.max_bytes(), 16 * 1024 * 1024);
    assert_eq!(apply.max_wall_clock_nanos(), 2_000_000);
}

#[test]
fn pending_and_in_flight_byte_ledgers_are_deterministic() {
    let runtime_scope = scope();
    let storage = MemoryTransactionKernel::new();
    let first = ChunkCoordinate::new(0, 0, 0);
    let second = ChunkCoordinate::new(1, 0, 0);
    let (_, stored_first) = commit_chunk(&storage, &runtime_scope, first, vec![1; CELL_COUNT], 1);
    let (_, stored_second) = commit_chunk(&storage, &runtime_scope, second, vec![2; CELL_COUNT], 2);
    let mut runtime = runtime();
    project_stored(&mut runtime, &stored_first, 0, 1, 1);
    project_stored(&mut runtime, &stored_second, 1, 1, 1);

    let pending_jobs = runtime.pending_jobs();
    let pending_bytes = runtime.pending_bytes();
    assert_eq!(pending_jobs, 4);
    assert!(pending_bytes > 0);
    assert_eq!(runtime.in_flight_jobs(), 0);
    assert_eq!(runtime.in_flight_bytes(), 0);
    assert_eq!(
        runtime.diagnostics().combined_pending_bytes(),
        pending_bytes
    );

    let mesh = match runtime
        .dispatch_next(DerivedKind::Mesh)
        .expect("job identity remains in range")
    {
        DispatchOutcome::Started(input) => input,
        other => panic!("first mesh job must start, got {other:?}"),
    };
    assert_eq!(runtime.pending_jobs(), pending_jobs - 1);
    assert_eq!(
        runtime.pending_bytes(),
        pending_bytes.saturating_sub(mesh.ticket().reserved_bytes())
    );
    assert_eq!(runtime.in_flight_jobs(), 1);
    assert_eq!(runtime.in_flight_bytes(), mesh.ticket().reserved_bytes());
    assert!(!runtime.admission_snapshot().at_soft_high_water());

    assert!(matches!(
        runtime.complete_derived(
            mesh,
            0_u8,
            ApplyByteDeclaration::new(0),
            FixedTick::new(2),
            |_| Ok::<(), ()>(()),
        ),
        CompletionOutcome::Applied { .. }
    ));
    assert_eq!(runtime.in_flight_jobs(), 0);
    assert_eq!(runtime.in_flight_bytes(), 0);
}

#[test]
fn executor_panic_and_lost_ticket_release_reservations() {
    let runtime_scope = scope();
    let storage = MemoryTransactionKernel::new();
    let coordinate = ChunkCoordinate::new(0, 0, 0);
    let (_, stored) = commit_chunk(&storage, &runtime_scope, coordinate, vec![1; CELL_COUNT], 1);
    let mut runtime = runtime();
    project_stored(&mut runtime, &stored, 0, 1, 1);

    let panicked = dispatch_target(&mut runtime, DerivedKind::Collider, coordinate);
    let reserved = panicked.ticket().reserved_bytes();
    assert_eq!(runtime.in_flight_bytes(), reserved);
    let abort = runtime.complete_executor::<u8, (), ()>(
        ExecutorOutcome::Panicked { input: panicked },
        ApplyByteDeclaration::new(0),
        FixedTick::new(1),
        |_| Ok(()),
    );
    let ExecutorFinish::Aborted(WorkerAbortOutcome::Panicked(receipt)) = abort else {
        panic!("executor panic must abort without applying");
    };
    assert_eq!(receipt.released_reserved_bytes(), reserved);
    assert_eq!(runtime.in_flight_bytes(), 0);
    assert_eq!(runtime.diagnostics().collider().executor_panicked(), 1);
    assert!(matches!(
        runtime.collider_safety(coordinate),
        Some(ColliderSafetyState::FailedConservative {
            failure: ColliderFailure::ExecutorPanicked,
            ..
        })
    ));
    assert!(
        runtime
            .last_applied_key(coordinate, DerivedKind::Collider)
            .is_none()
    );

    runtime
        .request_derived(
            coordinate,
            DerivedKind::Mesh,
            FixedTick::new(2),
            request(1, 0, 512, 256),
        )
        .expect("retry is queued");
    let lost = dispatch_target(&mut runtime, DerivedKind::Mesh, coordinate);
    let ticket = lost.ticket().clone();
    let lost_bytes = ticket.reserved_bytes();
    drop(lost);
    let recovered = runtime.recover_lost_ticket(ticket.clone());
    assert!(matches!(
        recovered,
        WorkerAbortOutcome::Lost(receipt) if receipt.job() == ticket.id()
            && receipt.released_reserved_bytes() == lost_bytes
    ));
    assert_eq!(runtime.in_flight_bytes(), 0);
    assert_eq!(runtime.diagnostics().mesh().lost_tickets(), 1);
    assert!(matches!(
        runtime.recover_lost_ticket(ticket),
        WorkerAbortOutcome::UnknownTicket { .. }
    ));
}

#[test]
fn apply_slice_wall_clock_and_byte_hooks_stop_later_jobs() {
    let budget = DerivedApplyBudget::desktop_reference_v1();
    let mut slice = DerivedApplySlice::new();
    assert_eq!(
        slice.admission(budget.max_bytes().saturating_add(1), budget),
        ApplyAdmission::Admit
    );
    slice.commit_applied(budget.max_bytes().saturating_add(1), WallClockNanos::new(0));
    assert_eq!(slice.admission(1, budget), ApplyAdmission::StopBytes);

    let mut wall = DerivedApplySlice::new();
    wall.commit_applied(1, WallClockNanos::new(0));
    wall.set_elapsed(WallClockNanos::main_world_apply_cap());
    assert_eq!(wall.admission(1, budget), ApplyAdmission::StopWallClock);

    let mut jobs = DerivedApplySlice::new();
    for _ in 0..budget.max_jobs() {
        assert_eq!(jobs.admission(1, budget), ApplyAdmission::Admit);
        jobs.commit_applied(1, WallClockNanos::new(0));
    }
    assert_eq!(jobs.admission(1, budget), ApplyAdmission::StopJobs);

    let mut runtime = runtime();
    runtime.record_apply_budget_stop(ApplyAdmission::StopJobs);
    runtime.record_apply_budget_stop(ApplyAdmission::StopBytes);
    runtime.record_apply_budget_stop(ApplyAdmission::StopWallClock);
    let diagnostics = runtime.diagnostics();
    assert_eq!(diagnostics.apply_stopped_jobs(), 1);
    assert_eq!(diagnostics.apply_stopped_bytes(), 1);
    assert_eq!(diagnostics.apply_stopped_wall_clock(), 1);
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]

    #[test]
    fn committed_cell_addressing_round_trips_negative_chunks(
        chunk_x in -32_i32..32,
        chunk_y in -16_i32..16,
        chunk_z in -32_i32..32,
        local_x in 0_usize..usize::from(EDGE),
        local_y in 0_usize..usize::from(EDGE),
        local_z in 0_usize..usize::from(EDGE),
        value in any::<u8>(),
    ) {
        let runtime_scope = scope();
        let storage = MemoryTransactionKernel::new();
        let coordinate = ChunkCoordinate::new(chunk_x, chunk_y, chunk_z);
        let mut cells = vec![0; CELL_COUNT];
        cells[index(local_x, local_y, local_z)] = value;
        let (_, stored) = commit_chunk(&storage, &runtime_scope, coordinate, cells, 1);
        let mut runtime = runtime();
        project_stored(&mut runtime, &stored, 0, 1, 1);
        let edge = i64::from(EDGE);
        let world = VoxelCoordinate::new(
            i64::from(chunk_x) * edge + i64::try_from(local_x).expect("local X fits i64"),
            i64::from(chunk_y) * edge + i64::try_from(local_y).expect("local Y fits i64"),
            i64::from(chunk_z) * edge + i64::try_from(local_z).expect("local Z fits i64"),
        );
        prop_assert_eq!(runtime.cell(world), Ok(&value));
    }
}
