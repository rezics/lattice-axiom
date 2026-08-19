//! Persistence-format and atomic-write support for storage backend crates.
//!
//! This module is public only so dedicated backend crates can share the exact
//! key/value format and transaction preparation logic. Game, ECS, and content
//! crates should use [`crate::WorldStorage`] instead.

use std::collections::{BTreeMap, BTreeSet};

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::{
    ArtifactKey, ArtifactReceipt, CheckpointId, ChunkCommit, ChunkKey, CommitCondition,
    CommitReceipt, ReceiptQuery, StorageError, StorageResult, StoredChunk, WorldId,
};

const ENVELOPE_MAGIC: [u8; 4] = *b"LAXE";
const ENVELOPE_FORMAT_VERSION: u16 = 1;
const RECORD_SCHEMA_VERSION: u32 = 1;
const ENVELOPE_HEADER_LENGTH: usize = 20;
const MAX_IDENTIFIER_BYTES: usize = 1_024;

const SNAPSHOT_KEY_SPACE: u8 = 0x10;
const ENTITIES_KEY_SPACE: u8 = 0x11;
const CONTINUATION_KEY_SPACE: u8 = 0x12;
const RECEIPT_INDEX_KEY_SPACE: u8 = 0x13;
const RECEIPT_KEY_SPACE: u8 = 0x20;
const COMMIT_MARKER_KEY_SPACE: u8 = 0x30;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
enum RecordKind {
    Snapshot = 1,
    Entities = 2,
    Continuation = 3,
    ReceiptIndex = 4,
    Receipt = 5,
    CommitMarker = 6,
    MemoryCheckpoint = 7,
}

impl RecordKind {
    const fn name(self) -> &'static str {
        match self {
            Self::Snapshot => "chunk snapshot",
            Self::Entities => "spatial entities",
            Self::Continuation => "simulation continuation",
            Self::ReceiptIndex => "chunk receipt index",
            Self::Receipt => "artifact receipt",
            Self::CommitMarker => "commit marker",
            Self::MemoryCheckpoint => "memory checkpoint",
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct SnapshotRecord {
    revision: u64,
    snapshot: crate::ChunkSnapshot,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct EntitiesRecord {
    revision: u64,
    entities: BTreeMap<crate::EntityId, crate::OwnedPayload>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct ContinuationRecord {
    revision: u64,
    continuation: BTreeMap<crate::WorkKind, crate::OwnedPayload>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct ReceiptIndexRecord {
    revision: u64,
    artifacts: BTreeSet<ArtifactKey>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct ReceiptRecord {
    revision: u64,
    chunk: ChunkKey,
    artifact: ArtifactKey,
    receipt: ArtifactReceipt,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct CommitMarkerRecord {
    canonical_commit: Vec<u8>,
    receipt: CommitReceipt,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct MemoryCheckpointRecord {
    id: CheckpointId,
    records: BTreeMap<Vec<u8>, Vec<u8>>,
}

/// Backend-neutral result of preparing an idempotent atomic commit.
#[derive(Debug)]
pub enum CommitPreparation {
    /// The exact commit was acknowledged previously; no write is necessary.
    Replay(CommitReceipt),
    /// The backend must apply the complete write set atomically.
    Write(PreparedWrite),
}

/// Fully encoded puts and deletes that must be applied as one atomic unit.
#[derive(Debug)]
pub struct PreparedWrite {
    puts: Vec<(Vec<u8>, Vec<u8>)>,
    deletes: Vec<Vec<u8>>,
    receipt: CommitReceipt,
}

/// Owned components of an encoded atomic backend write.
#[derive(Debug)]
pub struct PreparedWriteParts {
    /// Key/value records to insert or replace.
    pub puts: Vec<(Vec<u8>, Vec<u8>)>,
    /// Persistent keys to delete.
    pub deletes: Vec<Vec<u8>>,
    /// Receipt to return only after the whole write succeeds.
    pub receipt: CommitReceipt,
}

/// Decoded contents of an independently restorable memory checkpoint.
#[derive(Debug)]
pub struct DecodedMemoryCheckpoint {
    /// Caller-selected checkpoint identifier.
    pub id: CheckpointId,
    /// Complete versioned persistent record map.
    pub records: BTreeMap<Vec<u8>, Vec<u8>>,
}

impl PreparedWrite {
    /// Consumes the prepared transaction into puts, deletes, and its receipt.
    #[must_use]
    pub fn into_parts(self) -> PreparedWriteParts {
        PreparedWriteParts {
            puts: self.puts,
            deletes: self.deletes,
            receipt: self.receipt,
        }
    }
}

/// Encodes an `i32` so lexicographic byte order equals signed numeric order.
///
/// This is the persistent-key rule from ADR 0011: flip the sign bit and emit
/// the resulting unsigned value in big-endian order.
#[must_use]
pub const fn encode_ordered_i32(value: i32) -> [u8; 4] {
    (value.cast_unsigned() ^ 0x8000_0000).to_be_bytes()
}

/// Returns the stable chunk key for the selected logical key space.
fn chunk_record_key(space: u8, key: ChunkKey) -> Vec<u8> {
    let mut encoded = Vec::with_capacity(33);
    encoded.push(space);
    encoded.extend_from_slice(&key.world.to_u128().to_be_bytes());
    encoded.extend_from_slice(&key.dimension.to_u32().to_be_bytes());
    encoded.extend_from_slice(&encode_ordered_i32(key.position.x));
    encoded.extend_from_slice(&encode_ordered_i32(key.position.y));
    encoded.extend_from_slice(&encode_ordered_i32(key.position.z));
    encoded
}

/// Returns the stable snapshot-record key of a chunk.
#[must_use]
pub fn snapshot_key(key: ChunkKey) -> Vec<u8> {
    chunk_record_key(SNAPSHOT_KEY_SPACE, key)
}

fn entities_key(key: ChunkKey) -> Vec<u8> {
    chunk_record_key(ENTITIES_KEY_SPACE, key)
}

fn continuation_key(key: ChunkKey) -> Vec<u8> {
    chunk_record_key(CONTINUATION_KEY_SPACE, key)
}

fn receipt_index_key(key: ChunkKey) -> Vec<u8> {
    chunk_record_key(RECEIPT_INDEX_KEY_SPACE, key)
}

fn receipt_key(world: WorldId, artifact: &ArtifactKey) -> StorageResult<Vec<u8>> {
    validate_identifier(artifact.domain.as_str())?;
    let domain = artifact.domain.as_str().as_bytes();
    let mut encoded = Vec::with_capacity(1 + 16 + domain.len() * 2 + 1 + 16);
    encoded.push(RECEIPT_KEY_SPACE);
    encoded.extend_from_slice(&world.to_u128().to_be_bytes());
    encode_ordered_bytes(domain, &mut encoded);
    encoded.extend_from_slice(&artifact.artifact.to_u128().to_be_bytes());
    Ok(encoded)
}

fn commit_marker_key(world: WorldId, commit: crate::CommitId) -> Vec<u8> {
    let mut encoded = Vec::with_capacity(33);
    encoded.push(COMMIT_MARKER_KEY_SPACE);
    encoded.extend_from_slice(&world.to_u128().to_be_bytes());
    encoded.extend_from_slice(&commit.to_u128().to_be_bytes());
    encoded
}

/// Returns the ordered key prefix used to scan a receipt query.
///
/// # Errors
///
/// Returns an error if a deserialized query contains an invalid domain.
pub fn receipt_scan_prefix(query: &ReceiptQuery) -> StorageResult<Vec<u8>> {
    let mut prefix = Vec::with_capacity(19);
    prefix.push(RECEIPT_KEY_SPACE);
    prefix.extend_from_slice(&query.world.to_u128().to_be_bytes());
    if let Some(domain) = &query.domain {
        validate_identifier(domain.as_str())?;
        encode_ordered_bytes(domain.as_str().as_bytes(), &mut prefix);
    }
    Ok(prefix)
}

fn encode_ordered_bytes(bytes: &[u8], output: &mut Vec<u8>) {
    // Prefix every data byte with one and terminate with zero. This makes the
    // encoding self-delimiting while preserving bytewise lexical order,
    // including the rule that a shorter prefix sorts before its extension.
    for byte in bytes {
        output.push(1);
        output.push(*byte);
    }
    output.push(0);
}

/// Decodes and validates one key/value entry found during a receipt scan.
///
/// # Errors
///
/// Returns an error when the envelope is malformed or its stable key does not
/// match the decoded receipt.
pub fn decode_scanned_receipt(
    key: &[u8],
    value: &[u8],
) -> StorageResult<(ChunkKey, ArtifactKey, ArtifactReceipt)> {
    let record: ReceiptRecord = decode_record(RecordKind::Receipt, value)?;
    let expected_key = receipt_key(record.chunk.world, &record.artifact)?;
    if expected_key != key {
        return Err(StorageError::CorruptRecord {
            reason: "artifact receipt key does not match its versioned value".to_owned(),
        });
    }
    validate_receipt(&record.artifact, &record.receipt)?;
    Ok((record.chunk, record.artifact, record.receipt))
}

/// Loads one chunk through a backend-provided point-read function.
///
/// The callback must read every key from one consistent view.
///
/// # Errors
///
/// Returns an error from the callback, envelope decoding, or cross-record
/// revision validation.
pub fn load_chunk_with<F>(key: ChunkKey, mut get: F) -> StorageResult<Option<StoredChunk>>
where
    F: FnMut(&[u8]) -> StorageResult<Option<Vec<u8>>>,
{
    let Some(snapshot_bytes) = get(&snapshot_key(key))? else {
        return Ok(None);
    };
    let snapshot: SnapshotRecord = decode_record(RecordKind::Snapshot, &snapshot_bytes)?;

    let entities_bytes = required_record(&mut get, &entities_key(key), "spatial entities", key)?;
    let entities: EntitiesRecord = decode_record(RecordKind::Entities, &entities_bytes)?;
    ensure_revision(
        key,
        snapshot.revision,
        entities.revision,
        "spatial entities",
    )?;

    let continuation_bytes = required_record(
        &mut get,
        &continuation_key(key),
        "simulation continuation",
        key,
    )?;
    let continuation: ContinuationRecord =
        decode_record(RecordKind::Continuation, &continuation_bytes)?;
    ensure_revision(
        key,
        snapshot.revision,
        continuation.revision,
        "simulation continuation",
    )?;

    let index_bytes = required_record(
        &mut get,
        &receipt_index_key(key),
        "artifact receipt index",
        key,
    )?;
    let index: ReceiptIndexRecord = decode_record(RecordKind::ReceiptIndex, &index_bytes)?;
    ensure_revision(
        key,
        snapshot.revision,
        index.revision,
        "artifact receipt index",
    )?;

    let mut artifact_receipts = BTreeMap::new();
    for artifact in index.artifacts {
        let persistent_key = receipt_key(key.world, &artifact)?;
        let receipt_bytes = required_record(&mut get, &persistent_key, "artifact receipt", key)?;
        let receipt: ReceiptRecord = decode_record(RecordKind::Receipt, &receipt_bytes)?;
        ensure_revision(key, snapshot.revision, receipt.revision, "artifact receipt")?;
        if receipt.chunk != key || receipt.artifact != artifact {
            return Err(StorageError::CorruptRecord {
                reason: format!(
                    "artifact receipt indexed by {key:?} points to {:?}",
                    receipt.chunk
                ),
            });
        }
        validate_receipt(&artifact, &receipt.receipt)?;
        artifact_receipts.insert(artifact, receipt.receipt);
    }

    let stored = StoredChunk {
        key,
        revision: snapshot.revision,
        snapshot: snapshot.snapshot,
        spatial_entities: entities.entities,
        continuation: continuation.continuation,
        artifact_receipts,
    };
    validate_stored_chunk(&stored)?;
    Ok(Some(stored))
}

/// Validates an immutable commit and prepares its atomic persistent write set.
///
/// The callback is invoked only while the backend's single-writer critical
/// section is held. Exact idempotent replays return [`CommitPreparation::Replay`].
///
/// # Errors
///
/// Returns an error from validation, the callback, optimistic concurrency,
/// artifact ownership checks, encoding, or decoding existing records.
pub fn prepare_commit_with<F>(commit: &ChunkCommit, mut get: F) -> StorageResult<CommitPreparation>
where
    F: FnMut(&[u8]) -> StorageResult<Option<Vec<u8>>>,
{
    validate_commit(commit)?;
    let canonical_commit = encode_payload(RecordKind::CommitMarker, commit)?;
    let marker_key = commit_marker_key(commit.key.world, commit.id);
    if let Some(marker_bytes) = get(&marker_key)? {
        let marker: CommitMarkerRecord = decode_record(RecordKind::CommitMarker, &marker_bytes)?;
        if marker.canonical_commit != canonical_commit {
            return Err(StorageError::CommitIdReuse {
                world: commit.key.world,
                commit_id: commit.id,
            });
        }
        let mut receipt = marker.receipt;
        receipt.replayed = true;
        return Ok(CommitPreparation::Replay(receipt));
    }

    let existing_snapshot = get(&snapshot_key(commit.key))?;
    let current_revision = existing_snapshot
        .as_deref()
        .map(|bytes| decode_record::<SnapshotRecord>(RecordKind::Snapshot, bytes))
        .transpose()?
        .map(|record| record.revision);
    check_condition(commit, current_revision)?;
    let revision = current_revision
        .unwrap_or(0)
        .checked_add(1)
        .ok_or_else(|| StorageError::InvalidCommit {
            reason: format!("chunk {:?} exhausted its revision counter", commit.key),
        })?;

    let old_index = load_old_receipt_index(commit.key, current_revision, &mut get)?;
    ensure_artifact_ownership(commit, &mut get)?;
    let write = build_prepared_write(commit, revision, &old_index, marker_key, canonical_commit)?;
    Ok(CommitPreparation::Write(write))
}

fn load_old_receipt_index<F>(
    key: ChunkKey,
    current_revision: Option<u64>,
    get: &mut F,
) -> StorageResult<BTreeSet<ArtifactKey>>
where
    F: FnMut(&[u8]) -> StorageResult<Option<Vec<u8>>>,
{
    if let Some(expected_revision) = current_revision {
        let bytes = required_record(get, &receipt_index_key(key), "artifact receipt index", key)?;
        let index: ReceiptIndexRecord = decode_record(RecordKind::ReceiptIndex, &bytes)?;
        ensure_revision(
            key,
            expected_revision,
            index.revision,
            "artifact receipt index",
        )?;
        Ok(index.artifacts)
    } else if get(&receipt_index_key(key))?.is_some() {
        Err(StorageError::CorruptRecord {
            reason: format!("absent chunk {key:?} has an orphan artifact receipt index"),
        })
    } else {
        Ok(BTreeSet::new())
    }
}

fn ensure_artifact_ownership<F>(commit: &ChunkCommit, get: &mut F) -> StorageResult<()>
where
    F: FnMut(&[u8]) -> StorageResult<Option<Vec<u8>>>,
{
    for artifact in commit.artifact_receipts.keys() {
        let key = receipt_key(commit.key.world, artifact)?;
        if let Some(bytes) = get(&key)? {
            let existing: ReceiptRecord = decode_record(RecordKind::Receipt, &bytes)?;
            if existing.chunk != commit.key {
                return Err(StorageError::ArtifactCollision {
                    world: commit.key.world,
                    artifact: Box::new(artifact.clone()),
                    existing: existing.chunk,
                    attempted: commit.key,
                });
            }
        }
    }
    Ok(())
}

fn build_prepared_write(
    commit: &ChunkCommit,
    revision: u64,
    old_index: &BTreeSet<ArtifactKey>,
    marker_key: Vec<u8>,
    canonical_commit: Vec<u8>,
) -> StorageResult<PreparedWrite> {
    let new_index: BTreeSet<_> = commit.artifact_receipts.keys().cloned().collect();
    let mut puts = encode_chunk_category_puts(commit, revision, &new_index)?;
    append_receipt_puts(&mut puts, commit, revision)?;
    let receipt = CommitReceipt {
        commit_id: commit.id,
        key: commit.key,
        revision,
        durability: commit.durability,
        replayed: false,
    };
    puts.push((
        marker_key,
        encode_record(
            RecordKind::CommitMarker,
            &CommitMarkerRecord {
                canonical_commit,
                receipt: receipt.clone(),
            },
        )?,
    ));
    let deletes = old_index
        .difference(&new_index)
        .map(|artifact| receipt_key(commit.key.world, artifact))
        .collect::<StorageResult<Vec<_>>>()?;
    Ok(PreparedWrite {
        puts,
        deletes,
        receipt,
    })
}

fn encode_chunk_category_puts(
    commit: &ChunkCommit,
    revision: u64,
    new_index: &BTreeSet<ArtifactKey>,
) -> StorageResult<Vec<(Vec<u8>, Vec<u8>)>> {
    let puts = vec![
        (
            snapshot_key(commit.key),
            encode_record(
                RecordKind::Snapshot,
                &SnapshotRecord {
                    revision,
                    snapshot: commit.snapshot.clone(),
                },
            )?,
        ),
        (
            entities_key(commit.key),
            encode_record(
                RecordKind::Entities,
                &EntitiesRecord {
                    revision,
                    entities: commit.spatial_entities.clone(),
                },
            )?,
        ),
        (
            continuation_key(commit.key),
            encode_record(
                RecordKind::Continuation,
                &ContinuationRecord {
                    revision,
                    continuation: commit.continuation.clone(),
                },
            )?,
        ),
        (
            receipt_index_key(commit.key),
            encode_record(
                RecordKind::ReceiptIndex,
                &ReceiptIndexRecord {
                    revision,
                    artifacts: new_index.clone(),
                },
            )?,
        ),
    ];
    Ok(puts)
}

fn append_receipt_puts(
    puts: &mut Vec<(Vec<u8>, Vec<u8>)>,
    commit: &ChunkCommit,
    revision: u64,
) -> StorageResult<()> {
    for (artifact, receipt) in &commit.artifact_receipts {
        puts.push((
            receipt_key(commit.key.world, artifact)?,
            encode_record(
                RecordKind::Receipt,
                &ReceiptRecord {
                    revision,
                    chunk: commit.key,
                    artifact: artifact.clone(),
                    receipt: receipt.clone(),
                },
            )?,
        ));
    }
    Ok(())
}

/// Encodes all memory-backend records as one independently restorable envelope.
///
/// # Errors
///
/// Returns an error if bincode cannot encode the record map.
pub fn encode_memory_checkpoint(
    id: CheckpointId,
    records: &BTreeMap<Vec<u8>, Vec<u8>>,
) -> StorageResult<Vec<u8>> {
    encode_record(
        RecordKind::MemoryCheckpoint,
        &MemoryCheckpointRecord {
            id,
            records: records.clone(),
        },
    )
}

/// Decodes all records from an independently restorable memory checkpoint.
///
/// # Errors
///
/// Returns an error for malformed, incompatible, or undecodable envelopes.
pub fn decode_memory_checkpoint(bytes: &[u8]) -> StorageResult<DecodedMemoryCheckpoint> {
    let checkpoint: MemoryCheckpointRecord = decode_record(RecordKind::MemoryCheckpoint, bytes)?;
    Ok(DecodedMemoryCheckpoint {
        id: checkpoint.id,
        records: checkpoint.records,
    })
}

fn validate_commit(commit: &ChunkCommit) -> StorageResult<()> {
    if commit.snapshot.schema.major == 0 {
        return Err(StorageError::InvalidCommit {
            reason: "snapshot schema major version must be non-zero".to_owned(),
        });
    }
    validate_identifier(commit.snapshot.provenance.boundary_contract.as_str())?;
    for implementation in commit.snapshot.provenance.implementations.keys() {
        validate_identifier(implementation.as_str())?;
    }
    for payload in commit.spatial_entities.values() {
        validate_owned_payload(payload)?;
    }
    for (kind, payload) in &commit.continuation {
        validate_identifier(kind.as_str())?;
        validate_owned_payload(payload)?;
    }
    for (artifact, receipt) in &commit.artifact_receipts {
        validate_receipt(artifact, receipt)?;
    }
    Ok(())
}

fn validate_stored_chunk(chunk: &StoredChunk) -> StorageResult<()> {
    if chunk.revision == 0 {
        return Err(StorageError::CorruptRecord {
            reason: format!("chunk {:?} has reserved revision zero", chunk.key),
        });
    }
    if chunk.snapshot.schema.major == 0 {
        return Err(StorageError::CorruptRecord {
            reason: format!("chunk {:?} has schema major version zero", chunk.key),
        });
    }
    validate_identifier(chunk.snapshot.provenance.boundary_contract.as_str())?;
    for implementation in chunk.snapshot.provenance.implementations.keys() {
        validate_identifier(implementation.as_str())?;
    }
    for payload in chunk.spatial_entities.values() {
        validate_owned_payload(payload)?;
    }
    for (kind, payload) in &chunk.continuation {
        validate_identifier(kind.as_str())?;
        validate_owned_payload(payload)?;
    }
    Ok(())
}

fn validate_owned_payload(payload: &crate::OwnedPayload) -> StorageResult<()> {
    validate_identifier(payload.owner.as_str())?;
    if payload.schema.major == 0 {
        return Err(StorageError::InvalidCommit {
            reason: format!(
                "payload owned by {:?} has schema major version zero",
                payload.owner
            ),
        });
    }
    Ok(())
}

fn validate_receipt(artifact: &ArtifactKey, receipt: &ArtifactReceipt) -> StorageResult<()> {
    validate_identifier(artifact.domain.as_str())?;
    validate_identifier(receipt.producer.as_str())?;
    Ok(())
}

fn validate_identifier(value: &str) -> StorageResult<()> {
    if value.is_empty() {
        return Err(crate::IdentifierError::Empty.into());
    }
    if value.len() > MAX_IDENTIFIER_BYTES {
        return Err(crate::IdentifierError::TooLong {
            length: value.len(),
            maximum: MAX_IDENTIFIER_BYTES,
        }
        .into());
    }
    Ok(())
}

fn check_condition(commit: &ChunkCommit, actual: Option<u64>) -> StorageResult<()> {
    let accepted = match commit.condition {
        CommitCondition::Any => true,
        CommitCondition::IfAbsent => actual.is_none(),
        CommitCondition::IfRevision(expected) => actual == Some(expected),
    };
    if accepted {
        Ok(())
    } else {
        Err(StorageError::RevisionConflict {
            key: commit.key,
            expected: commit.condition,
            actual,
        })
    }
}

fn required_record<F>(
    get: &mut F,
    persistent_key: &[u8],
    category: &'static str,
    chunk: ChunkKey,
) -> StorageResult<Vec<u8>>
where
    F: FnMut(&[u8]) -> StorageResult<Option<Vec<u8>>>,
{
    get(persistent_key)?.ok_or_else(|| StorageError::CorruptRecord {
        reason: format!("chunk {chunk:?} is missing its {category} record"),
    })
}

fn ensure_revision(
    key: ChunkKey,
    expected: u64,
    actual: u64,
    category: &'static str,
) -> StorageResult<()> {
    if expected == actual {
        Ok(())
    } else {
        Err(StorageError::CorruptRecord {
            reason: format!(
                "chunk {key:?} snapshot revision {expected} differs from {category} revision {actual}"
            ),
        })
    }
}

fn encode_payload<T: Serialize>(kind: RecordKind, value: &T) -> StorageResult<Vec<u8>> {
    bincode::serde::encode_to_vec(
        value,
        bincode::config::standard()
            .with_fixed_int_encoding()
            .with_little_endian(),
    )
    .map_err(|error| StorageError::Codec {
        operation: "encode",
        record: kind.name(),
        message: error.to_string(),
    })
}

fn encode_record<T: Serialize>(kind: RecordKind, value: &T) -> StorageResult<Vec<u8>> {
    let payload = encode_payload(kind, value)?;
    let payload_length = u64::try_from(payload.len()).map_err(|_| StorageError::Codec {
        operation: "encode",
        record: kind.name(),
        message: "payload length does not fit u64".to_owned(),
    })?;
    let mut envelope = Vec::with_capacity(ENVELOPE_HEADER_LENGTH + payload.len());
    envelope.extend_from_slice(&ENVELOPE_MAGIC);
    envelope.extend_from_slice(&ENVELOPE_FORMAT_VERSION.to_be_bytes());
    envelope.push(kind as u8);
    envelope.push(0);
    envelope.extend_from_slice(&RECORD_SCHEMA_VERSION.to_be_bytes());
    envelope.extend_from_slice(&payload_length.to_be_bytes());
    envelope.extend_from_slice(&payload);
    Ok(envelope)
}

fn decode_record<T: DeserializeOwned>(kind: RecordKind, bytes: &[u8]) -> StorageResult<T> {
    if bytes.len() < ENVELOPE_HEADER_LENGTH {
        return Err(StorageError::CorruptRecord {
            reason: format!("{} envelope is only {} bytes", kind.name(), bytes.len()),
        });
    }
    if bytes[0..4] != ENVELOPE_MAGIC {
        return Err(StorageError::CorruptRecord {
            reason: format!("{} envelope has invalid magic", kind.name()),
        });
    }
    let envelope_version = u16::from_be_bytes([bytes[4], bytes[5]]);
    if envelope_version != ENVELOPE_FORMAT_VERSION {
        return Err(StorageError::UnsupportedEnvelope {
            found: envelope_version,
            supported: ENVELOPE_FORMAT_VERSION,
        });
    }
    if bytes[6] != kind as u8 {
        return Err(StorageError::CorruptRecord {
            reason: format!(
                "expected {} envelope kind {}, found {}",
                kind.name(),
                kind as u8,
                bytes[6]
            ),
        });
    }
    if bytes[7] != 0 {
        return Err(StorageError::CorruptRecord {
            reason: format!("{} envelope has unsupported flags", kind.name()),
        });
    }
    let schema_version = u32::from_be_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]);
    if schema_version != RECORD_SCHEMA_VERSION {
        return Err(StorageError::UnsupportedSchema {
            record: kind.name(),
            found: schema_version,
            supported: RECORD_SCHEMA_VERSION,
        });
    }
    let declared_length = u64::from_be_bytes([
        bytes[12], bytes[13], bytes[14], bytes[15], bytes[16], bytes[17], bytes[18], bytes[19],
    ]);
    let declared_length =
        usize::try_from(declared_length).map_err(|_| StorageError::CorruptRecord {
            reason: format!("{} envelope length exceeds this platform", kind.name()),
        })?;
    let payload = &bytes[ENVELOPE_HEADER_LENGTH..];
    if payload.len() != declared_length {
        return Err(StorageError::CorruptRecord {
            reason: format!(
                "{} envelope declares {declared_length} payload bytes but contains {}",
                kind.name(),
                payload.len()
            ),
        });
    }
    let (value, consumed) = bincode::serde::decode_from_slice(
        payload,
        bincode::config::standard()
            .with_fixed_int_encoding()
            .with_little_endian(),
    )
    .map_err(|error| StorageError::Codec {
        operation: "decode",
        record: kind.name(),
        message: error.to_string(),
    })?;
    if consumed != payload.len() {
        return Err(StorageError::CorruptRecord {
            reason: format!(
                "{} decoder consumed {consumed} of {} payload bytes",
                kind.name(),
                payload.len()
            ),
        });
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use latticeaxiom_core::ChunkPos;
    use proptest::prelude::*;

    use super::*;
    use crate::{DimensionId, WorldId};

    #[test]
    fn signed_key_boundaries_sort_in_numeric_order() {
        let values = [i32::MIN, -33, -1, 0, 1, 32, i32::MAX];
        let encoded: Vec<_> = values.into_iter().map(encode_ordered_i32).collect();
        assert!(encoded.windows(2).all(|pair| pair[0] < pair[1]));
    }

    #[test]
    fn chunk_keys_follow_world_dimension_xyz_order() {
        let world = WorldId::from_u128(7);
        let dimension = DimensionId::from_u32(2);
        let mut positions = [
            ChunkPos::new(0, 0, 0),
            ChunkPos::new(-1, 12, 3),
            ChunkPos::new(-1, -2, 9),
            ChunkPos::new(0, -1, 100),
        ];
        positions.sort();
        let encoded: Vec<_> = positions
            .iter()
            .map(|position| snapshot_key(ChunkKey::new(world, dimension, *position)))
            .collect();
        assert!(encoded.windows(2).all(|pair| pair[0] < pair[1]));
    }

    #[test]
    fn envelope_rejects_newer_schema_before_decoding_payload() {
        let record = MemoryCheckpointRecord {
            id: CheckpointId::from_u128(1),
            records: BTreeMap::new(),
        };
        let mut encoded = encode_record(RecordKind::MemoryCheckpoint, &record)
            .expect("test checkpoint envelope must encode");
        encoded[8..12].copy_from_slice(&2_u32.to_be_bytes());
        assert!(matches!(
            decode_record::<MemoryCheckpointRecord>(RecordKind::MemoryCheckpoint, &encoded),
            Err(StorageError::UnsupportedSchema {
                found: 2,
                supported: 1,
                ..
            })
        ));
    }

    #[test]
    fn envelope_rejects_truncation_and_trailing_bytes() {
        let record = MemoryCheckpointRecord {
            id: CheckpointId::from_u128(2),
            records: BTreeMap::new(),
        };
        let encoded = encode_record(RecordKind::MemoryCheckpoint, &record)
            .expect("test checkpoint envelope must encode");
        assert!(matches!(
            decode_record::<MemoryCheckpointRecord>(
                RecordKind::MemoryCheckpoint,
                &encoded[..ENVELOPE_HEADER_LENGTH - 1]
            ),
            Err(StorageError::CorruptRecord { .. })
        ));
        let mut with_trailing_byte = encoded;
        with_trailing_byte.push(0);
        assert!(matches!(
            decode_record::<MemoryCheckpointRecord>(
                RecordKind::MemoryCheckpoint,
                &with_trailing_byte
            ),
            Err(StorageError::CorruptRecord { .. })
        ));
    }

    proptest! {
        #[test]
        fn ordered_i32_encoding_matches_numeric_comparison(left in any::<i32>(), right in any::<i32>()) {
            prop_assert_eq!(left.cmp(&right), encode_ordered_i32(left).cmp(&encode_ordered_i32(right)));
        }

        #[test]
        fn chunk_position_key_order_is_lexicographic_xyz(
            ax in any::<i32>(), ay in any::<i32>(), az in any::<i32>(),
            bx in any::<i32>(), by in any::<i32>(), bz in any::<i32>(),
        ) {
            let world = WorldId::from_u128(9);
            let dimension = DimensionId::from_u32(4);
            let a = ChunkKey::new(world, dimension, ChunkPos::new(ax, ay, az));
            let b = ChunkKey::new(world, dimension, ChunkPos::new(bx, by, bz));
            prop_assert_eq!(a.cmp(&b), snapshot_key(a).cmp(&snapshot_key(b)));
        }
    }
}
