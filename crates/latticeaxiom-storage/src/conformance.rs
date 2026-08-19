//! Shared conformance suite for every [`crate::WorldStorage`] backend.

use std::collections::BTreeMap;

use crate::{
    ArtifactId, ArtifactKey, ArtifactReceipt, BoundaryContractId, ChunkCommit, ChunkKey,
    ChunkSnapshot, CommitCondition, CommitId, ContentHash, DimensionId, Durability, EntityId,
    GenerationEpoch, GenerationProvenance, ImplementationId, OwnedPayload, OwnerId, ReceiptDomain,
    ReceiptQuery, SchemaVersion, StorageError, WorkKind, WorldId, WorldStorage,
};

/// Runs all storage semantic checks, constructing a fresh backend per check.
///
/// # Panics
///
/// Panics when the backend violates atomic replacement, revision, idempotency,
/// collision, round-trip, or deterministic scan requirements.
pub fn run_all<S, F>(mut make_storage: F)
where
    S: WorldStorage,
    F: FnMut() -> S,
{
    round_trips_all_categories(&make_storage());
    exact_retries_are_idempotent(&make_storage());
    failed_conditions_leave_state_unchanged(&make_storage());
    replacement_removes_old_categories_atomically(&make_storage());
    artifact_ownership_is_exclusive(&make_storage());
    receipt_scans_are_stably_ordered(&make_storage());
}

/// Creates a complete deterministic commit used by facade and backend tests.
///
/// # Panics
///
/// Panics only if a compile-time test fixture identifier is invalid.
#[must_use]
pub fn sample_commit(commit_id: u128) -> ChunkCommit {
    let mut implementations = BTreeMap::new();
    implementations.insert(
        implementation("latticeaxiom.official/terrain.base@demo"),
        hash(1),
    );
    let mut spatial_entities = BTreeMap::new();
    spatial_entities.insert(
        EntityId::from_u128(7),
        OwnedPayload {
            owner: owner("latticeaxiom.official/chest"),
            schema: SchemaVersion::new(1, 0),
            payload: vec![5, 6, 7],
        },
    );
    let mut continuation = BTreeMap::new();
    continuation.insert(
        work_kind("scheduled_tick"),
        OwnedPayload {
            owner: owner("latticeaxiom.lib/simulation"),
            schema: SchemaVersion::new(1, 1),
            payload: vec![8, 9],
        },
    );
    let mut artifact_receipts = BTreeMap::new();
    artifact_receipts.insert(
        artifact("generation.chunk", 12),
        ArtifactReceipt {
            producer: implementation("latticeaxiom.official/terrain.base@demo"),
            configuration_hash: hash(2),
            input_hash: hash(3),
            content_hash: hash(4),
        },
    );

    ChunkCommit {
        id: CommitId::from_u128(commit_id),
        key: ChunkKey::new(
            WorldId::from_u128(1),
            DimensionId::from_u32(0),
            crate::ChunkPos::new(-1, 2, 3),
        ),
        condition: CommitCondition::IfAbsent,
        durability: Durability::Sync,
        snapshot: ChunkSnapshot {
            schema: SchemaVersion::new(1, 0),
            provenance: GenerationProvenance {
                created_at_unix_millis: 1_787_097_600_000,
                epoch: GenerationEpoch::from_u64(1),
                configuration_hash: hash(5),
                implementations,
                upstream_plan_hash: hash(6),
                boundary_contract: BoundaryContractId::new("chunk-neighbors/v1")
                    .expect("static boundary contract is valid"),
            },
            payload: vec![0, 1, 1, 2, 3, 5, 8],
        },
        spatial_entities,
        continuation,
        artifact_receipts,
    }
}

/// Checks a complete round trip of every authoritative chunk category.
///
/// # Panics
///
/// Panics when commit or load fails, or reconstructed contents differ.
pub fn round_trips_all_categories(storage: &impl WorldStorage) {
    let commit = sample_commit(1);
    let receipt = storage
        .commit_chunk(commit.clone())
        .expect("complete sample commit must succeed");
    assert_eq!(receipt.revision, 1);
    assert!(!receipt.replayed);
    let loaded = storage
        .load_chunk(commit.key)
        .expect("committed chunk must load")
        .expect("committed chunk must exist");
    assert_eq!(loaded.key, commit.key);
    assert_eq!(loaded.revision, 1);
    assert_eq!(loaded.snapshot, commit.snapshot);
    assert_eq!(loaded.spatial_entities, commit.spatial_entities);
    assert_eq!(loaded.continuation, commit.continuation);
    assert_eq!(loaded.artifact_receipts, commit.artifact_receipts);
}

/// Checks exact idempotent replay and conflicting commit-ID reuse.
///
/// # Panics
///
/// Panics when an exact retry changes state or a different retry is accepted.
pub fn exact_retries_are_idempotent(storage: &impl WorldStorage) {
    let commit = sample_commit(2);
    let first = storage
        .commit_chunk(commit.clone())
        .expect("initial commit must succeed");
    let replay = storage
        .commit_chunk(commit.clone())
        .expect("exact retry must succeed");
    assert_eq!(first.revision, replay.revision);
    assert!(replay.replayed);

    let mut reused = commit;
    reused.snapshot.payload.push(13);
    assert!(matches!(
        storage.commit_chunk(reused),
        Err(StorageError::CommitIdReuse { .. })
    ));
}

/// Checks failed optimistic commits leave the entire prior chunk unchanged.
///
/// # Panics
///
/// Panics when a stale write succeeds or changes any stored category.
pub fn failed_conditions_leave_state_unchanged(storage: &impl WorldStorage) {
    let commit = sample_commit(3);
    storage
        .commit_chunk(commit.clone())
        .expect("initial commit must succeed");
    let before = storage
        .load_chunk(commit.key)
        .expect("read before conflict must succeed");

    let mut stale = commit.clone();
    stale.id = CommitId::from_u128(4);
    stale.condition = CommitCondition::IfRevision(0);
    stale.snapshot.payload = vec![99];
    stale.spatial_entities.clear();
    stale.continuation.clear();
    stale.artifact_receipts.clear();
    assert!(matches!(
        storage.commit_chunk(stale),
        Err(StorageError::RevisionConflict { .. })
    ));
    assert_eq!(
        storage
            .load_chunk(commit.key)
            .expect("read after conflict must succeed"),
        before
    );
}

/// Checks one update replaces every category and removes stale receipt keys.
///
/// # Panics
///
/// Panics when a replacement is partial or leaves old receipts visible.
pub fn replacement_removes_old_categories_atomically(storage: &impl WorldStorage) {
    let initial = sample_commit(5);
    storage
        .commit_chunk(initial.clone())
        .expect("initial commit must succeed");

    let mut replacement = sample_commit(6);
    replacement.condition = CommitCondition::IfRevision(1);
    replacement.snapshot.payload = vec![21, 34];
    replacement.spatial_entities.clear();
    replacement.continuation.clear();
    replacement.artifact_receipts.clear();
    let receipt = storage
        .commit_chunk(replacement.clone())
        .expect("replacement commit must succeed");
    assert_eq!(receipt.revision, 2);

    let loaded = storage
        .load_chunk(replacement.key)
        .expect("replacement must load")
        .expect("replacement must exist");
    assert_eq!(loaded.revision, 2);
    assert_eq!(loaded.snapshot, replacement.snapshot);
    assert!(loaded.spatial_entities.is_empty());
    assert!(loaded.continuation.is_empty());
    assert!(loaded.artifact_receipts.is_empty());
    assert!(
        storage
            .scan_receipts(&ReceiptQuery {
                world: replacement.key.world,
                domain: None,
            })
            .expect("receipt scan must succeed")
            .is_empty()
    );
}

/// Checks that one artifact key cannot be claimed by two chunks.
///
/// # Panics
///
/// Panics when a colliding commit succeeds or alters the original chunk.
pub fn artifact_ownership_is_exclusive(storage: &impl WorldStorage) {
    let first = sample_commit(7);
    storage
        .commit_chunk(first.clone())
        .expect("first artifact owner must commit");
    let mut second = sample_commit(8);
    second.key.position.x += 1;
    assert!(matches!(
        storage.commit_chunk(second),
        Err(StorageError::ArtifactCollision { .. })
    ));
    assert!(
        storage
            .load_chunk(first.key)
            .expect("original owner must remain readable")
            .is_some()
    );
}

/// Checks receipt scans use stable `(domain, artifact)` key order and filters.
///
/// # Panics
///
/// Panics when scans are unstable or domain filtering is inexact.
pub fn receipt_scans_are_stably_ordered(storage: &impl WorldStorage) {
    let mut commit = sample_commit(9);
    commit.artifact_receipts.clear();
    for (domain, id, byte) in [("zeta", 2, 2), ("alpha", 9, 9), ("alpha", 1, 1)] {
        commit.artifact_receipts.insert(
            artifact(domain, id),
            ArtifactReceipt {
                producer: implementation("latticeaxiom.official/terrain.base@demo"),
                configuration_hash: hash(byte),
                input_hash: hash(byte),
                content_hash: hash(byte),
            },
        );
    }
    storage
        .commit_chunk(commit.clone())
        .expect("receipt sample must commit");
    let all = storage
        .scan_receipts(&ReceiptQuery {
            world: commit.key.world,
            domain: None,
        })
        .expect("full receipt scan must succeed");
    let keys: Vec<_> = all.iter().map(|(_, key, _)| key.clone()).collect();
    let mut sorted = keys.clone();
    sorted.sort();
    assert_eq!(keys, sorted);

    let alpha = storage
        .scan_receipts(&ReceiptQuery {
            world: commit.key.world,
            domain: Some(receipt_domain("alpha")),
        })
        .expect("filtered receipt scan must succeed");
    assert_eq!(alpha.len(), 2);
    assert!(
        alpha
            .iter()
            .all(|(_, key, _)| key.domain.as_str() == "alpha")
    );
}

fn artifact(domain: &str, id: u128) -> ArtifactKey {
    ArtifactKey::new(receipt_domain(domain), ArtifactId::from_u128(id))
}

fn receipt_domain(value: &str) -> ReceiptDomain {
    ReceiptDomain::new(value).expect("static receipt domain is valid")
}

fn implementation(value: &str) -> ImplementationId {
    ImplementationId::new(value).expect("static implementation id is valid")
}

fn owner(value: &str) -> OwnerId {
    OwnerId::new(value).expect("static owner id is valid")
}

fn work_kind(value: &str) -> WorkKind {
    WorkKind::new(value).expect("static work kind is valid")
}

fn hash(byte: u8) -> ContentHash {
    ContentHash::from_bytes([byte; 32])
}
