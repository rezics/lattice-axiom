//! Streaming reference hashes for materialized chunk state and transactions.

use std::{collections::BTreeMap, fmt};

use latticeaxiom_core::{CanonicalHash, WorldId};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{
    ChunkData, ChunkKey, ChunkRevisionExpectation, StorageError, StorageResult, StoredChunk,
    WorldRevision, WorldTransaction,
};

const REFERENCE_MATERIALIZED_CHUNK_STATE_DOMAIN: &[u8] =
    b"latticeaxiom:reference-materialized-chunk-state/v1\0";
const REFERENCE_MATERIALIZED_CHUNK_TRANSACTION_DOMAIN: &[u8] =
    b"latticeaxiom:reference-materialized-chunk-transaction/v1\0";

/// SHA-256 digest of the reference materialized-chunk projection.
///
/// The digest covers world identity and revision plus every materialized chunk
/// key, revision, entity, continuation, provenance entry, and payload byte. The
/// derived entity-location index is reconstructed from those chunk payloads and
/// is not encoded twice. World metadata, requirement closure, exact lock data,
/// receipts, presentation data, and backend metadata are deliberately absent,
/// so this is not a complete authoritative-world hash.
#[repr(transparent)]
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct MaterializedChunkStateHash(CanonicalHash);

impl MaterializedChunkStateHash {
    /// Returns the exact SHA-256 digest bytes.
    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; CanonicalHash::BYTE_LENGTH] {
        self.0.as_bytes()
    }

    /// Returns the shared canonical hash representation.
    #[must_use]
    pub const fn as_canonical_hash(self) -> CanonicalHash {
        self.0
    }

    pub(crate) const fn from_bytes(bytes: [u8; CanonicalHash::BYTE_LENGTH]) -> Self {
        Self(CanonicalHash::from_bytes(bytes))
    }
}

impl fmt::Display for MaterializedChunkStateHash {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

pub(crate) fn hash_materialized_chunk_state(
    world: WorldId,
    revision: WorldRevision,
    chunks: &BTreeMap<ChunkKey, StoredChunk>,
) -> StorageResult<MaterializedChunkStateHash> {
    let mut digest = CanonicalDigest::new(REFERENCE_MATERIALIZED_CHUNK_STATE_DOMAIN);
    digest.world_id(world);
    digest.u64(revision.get());
    digest.collection_len(chunks.len(), "world chunk count")?;
    for (key, chunk) in chunks {
        hash_chunk_entry(&mut digest, key, chunk)?;
    }
    Ok(MaterializedChunkStateHash::from_bytes(digest.finish()))
}

pub(crate) fn hash_projected_materialized_chunk_state(
    world: WorldId,
    revision: WorldRevision,
    chunks: &BTreeMap<ChunkKey, StoredChunk>,
    replacements: &BTreeMap<ChunkKey, StoredChunk>,
) -> StorageResult<MaterializedChunkStateHash> {
    let inserted = replacements
        .keys()
        .filter(|key| !chunks.contains_key(*key))
        .count();
    let projected_count =
        chunks
            .len()
            .checked_add(inserted)
            .ok_or(StorageError::PayloadSizeOverflow {
                what: "projected world chunk count",
            })?;
    let mut digest = CanonicalDigest::new(REFERENCE_MATERIALIZED_CHUNK_STATE_DOMAIN);
    digest.world_id(world);
    digest.u64(revision.get());
    digest.collection_len(projected_count, "world chunk count")?;

    let mut existing = chunks.iter().peekable();
    let mut replacement = replacements.iter().peekable();
    loop {
        match (existing.peek(), replacement.peek()) {
            (Some((existing_key, existing_chunk)), Some((replacement_key, replacement_chunk))) => {
                match existing_key.cmp(replacement_key) {
                    std::cmp::Ordering::Less => {
                        hash_chunk_entry(&mut digest, existing_key, existing_chunk)?;
                        existing.next();
                    }
                    std::cmp::Ordering::Equal => {
                        hash_chunk_entry(&mut digest, replacement_key, replacement_chunk)?;
                        existing.next();
                        replacement.next();
                    }
                    std::cmp::Ordering::Greater => {
                        hash_chunk_entry(&mut digest, replacement_key, replacement_chunk)?;
                        replacement.next();
                    }
                }
            }
            (Some((key, chunk)), None) => {
                hash_chunk_entry(&mut digest, key, chunk)?;
                existing.next();
            }
            (None, Some((key, chunk))) => {
                hash_chunk_entry(&mut digest, key, chunk)?;
                replacement.next();
            }
            (None, None) => break,
        }
    }
    Ok(MaterializedChunkStateHash::from_bytes(digest.finish()))
}

fn hash_chunk_entry(
    digest: &mut CanonicalDigest,
    key: &ChunkKey,
    chunk: &StoredChunk,
) -> StorageResult<()> {
    digest.chunk_key(key)?;
    digest.u64(chunk.captured_world_revision.get());
    digest.u64(chunk.revision.get());
    digest.u64(chunk.domain_revisions.voxels.get());
    digest.u64(chunk.domain_revisions.persistent_entities.get());
    digest.u64(chunk.domain_revisions.continuation.get());
    digest.chunk_data(&chunk.data)
}

pub(crate) fn hash_transaction(transaction: &WorldTransaction) -> StorageResult<CanonicalHash> {
    let mut digest = CanonicalDigest::new(REFERENCE_MATERIALIZED_CHUNK_TRANSACTION_DOMAIN);
    digest.world_id(transaction.world);
    digest.u64(transaction.base_world_revision.get());
    digest.collection_len(transaction.mutations.len(), "transaction mutation count")?;
    for mutation in &transaction.mutations {
        digest.chunk_key(&mutation.key)?;
        match mutation.expected_revision {
            ChunkRevisionExpectation::Absent => digest.u8(0),
            ChunkRevisionExpectation::Exact(revision) => {
                digest.u8(1);
                digest.u64(revision.get());
            }
        }
        digest.u8(mutation.changed_domains.bits());
        digest.chunk_data(&mutation.data)?;
    }
    Ok(CanonicalHash::from_bytes(digest.finish()))
}

struct CanonicalDigest(Sha256);

impl CanonicalDigest {
    fn new(domain: &[u8]) -> Self {
        let mut hasher = Sha256::new();
        hasher.update(domain);
        Self(hasher)
    }

    fn finish(self) -> [u8; CanonicalHash::BYTE_LENGTH] {
        let bytes = self.0.finalize();
        let mut output = [0_u8; CanonicalHash::BYTE_LENGTH];
        output.copy_from_slice(&bytes);
        output
    }

    fn raw(&mut self, bytes: &[u8]) {
        self.0.update(bytes);
    }

    fn u8(&mut self, value: u8) {
        self.raw(&[value]);
    }

    fn u32(&mut self, value: u32) {
        self.raw(&value.to_be_bytes());
    }

    fn u64(&mut self, value: u64) {
        self.raw(&value.to_be_bytes());
    }

    fn i32(&mut self, value: i32) {
        self.raw(&value.to_be_bytes());
    }

    fn collection_len(&mut self, length: usize, what: &'static str) -> StorageResult<()> {
        let length =
            u64::try_from(length).map_err(|_| StorageError::PayloadSizeOverflow { what })?;
        self.u64(length);
        Ok(())
    }

    fn bytes(&mut self, bytes: &[u8], what: &'static str) -> StorageResult<()> {
        self.collection_len(bytes.len(), what)?;
        self.raw(bytes);
        Ok(())
    }

    fn world_id(&mut self, world: WorldId) {
        self.raw(world.as_uuid().as_bytes());
    }

    fn chunk_key(&mut self, key: &ChunkKey) -> StorageResult<()> {
        self.world_id(key.world);
        self.bytes(key.dimension.as_str().as_bytes(), "dimension ID")?;
        self.i32(key.coordinate.x);
        self.i32(key.coordinate.y);
        self.i32(key.coordinate.z);
        Ok(())
    }

    fn versioned_payload(&mut self, payload: &crate::VersionedPayload) -> StorageResult<()> {
        self.bytes(payload.schema.as_str().as_bytes(), "payload schema ID")?;
        self.u32(payload.schema_version.get());
        self.bytes(&payload.bytes, "versioned payload")
    }

    fn chunk_data(&mut self, data: &ChunkData) -> StorageResult<()> {
        self.u8(1);
        self.versioned_payload(&data.voxels)?;

        self.u8(2);
        self.collection_len(data.persistent_entities.len(), "persistent entity count")?;
        for (id, payload) in &data.persistent_entities {
            self.raw(id.as_bytes());
            self.versioned_payload(payload)?;
        }

        self.u8(3);
        self.collection_len(data.continuations.len(), "continuation count")?;
        for (id, payload) in &data.continuations {
            self.raw(id.as_bytes());
            self.versioned_payload(payload)?;
        }

        self.u8(4);
        self.collection_len(data.provenance.len(), "provenance count")?;
        for (id, hash) in &data.provenance {
            self.bytes(id.as_str().as_bytes(), "provenance ID")?;
            self.raw(hash.as_bytes());
        }
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    #![allow(
        clippy::expect_used,
        clippy::too_many_lines,
        reason = "fixed canonical fixtures document their construction invariants"
    )]

    use std::str::FromStr;

    use latticeaxiom_core::{CanonicalHash, SchemaId, StableId};

    use super::*;
    use crate::{
        ChunkCoordinate, ChunkRevision, ContinuationId, ContinuationRevision, DimensionId,
        DomainRevisions, PayloadSchemaVersion, PersistentEntityId, PersistentEntityRevision,
        VoxelRevision,
        conformance::{sample_data, sample_key, sample_world},
    };

    #[derive(Clone)]
    struct CanonicalState {
        world: WorldId,
        revision: WorldRevision,
        chunks: BTreeMap<ChunkKey, StoredChunk>,
    }

    fn fixed_state() -> CanonicalState {
        let world = sample_world();
        let key = sample_key(world, ChunkCoordinate::new(1, -2, 3));
        let chunk = StoredChunk {
            key: key.clone(),
            captured_world_revision: WorldRevision::new(5),
            revision: ChunkRevision::new(6),
            domain_revisions: DomainRevisions {
                voxels: VoxelRevision::new(2),
                persistent_entities: PersistentEntityRevision::new(3),
                continuation: ContinuationRevision::new(4),
            },
            data: sample_data(11),
        };
        CanonicalState {
            world,
            revision: WorldRevision::new(7),
            chunks: BTreeMap::from([(key, chunk)]),
        }
    }

    fn materialized_chunk_state_hash(state: &CanonicalState) -> MaterializedChunkStateHash {
        hash_materialized_chunk_state(state.world, state.revision, &state.chunks)
            .expect("the fixed canonical state has representable lengths")
    }

    fn assert_hash_changes(label: &str, mutate: impl FnOnce(&mut CanonicalState)) {
        let mut state = fixed_state();
        let before = materialized_chunk_state_hash(&state);
        mutate(&mut state);
        let after = materialized_chunk_state_hash(&state);
        assert_ne!(before, after, "{label} must affect the canonical hash");
    }

    fn alternate_world() -> WorldId {
        WorldId::from_str("028f1e2d-3c4b-4a59-8c6d-7e8f9012abcd")
            .expect("the alternate world UUID literal is canonical")
    }

    fn mutate_key(state: &mut CanonicalState, mutate: impl FnOnce(&mut ChunkKey)) {
        let (mut key, mut chunk) = state
            .chunks
            .pop_first()
            .expect("the fixed state contains exactly one chunk");
        mutate(&mut key);
        chunk.key = key.clone();
        assert!(state.chunks.insert(key, chunk).is_none());
    }

    fn chunk_mut(state: &mut CanonicalState) -> &mut StoredChunk {
        state
            .chunks
            .values_mut()
            .next()
            .expect("the fixed state contains exactly one chunk")
    }

    fn data_mut(state: &mut CanonicalState) -> &mut ChunkData {
        &mut chunk_mut(state).data
    }

    #[test]
    fn materialized_chunk_state_hash_matches_known_answer() {
        assert_eq!(
            materialized_chunk_state_hash(&fixed_state()).as_bytes(),
            &[
                0xd0, 0x7e, 0x72, 0x5e, 0x65, 0x90, 0x9c, 0xde, 0x90, 0x8d, 0x95, 0xd8, 0x4c, 0x93,
                0x08, 0x87, 0x0d, 0xcd, 0x3f, 0x8f, 0xe1, 0x43, 0x5b, 0x6f, 0x93, 0x8d, 0x53, 0xa0,
                0x1f, 0x0d, 0x77, 0xd8,
            ]
        );
    }

    #[test]
    fn projected_chunk_hash_matches_fully_materialized_state() {
        let state = fixed_state();
        let template = state
            .chunks
            .values()
            .next()
            .expect("the fixed state contains exactly one chunk");
        let mut existing = BTreeMap::new();
        for x in [-3, 0, 3] {
            let key = sample_key(state.world, ChunkCoordinate::new(x, -2, 3));
            let mut chunk = template.clone();
            chunk.key = key.clone();
            chunk.data = sample_data(u8::try_from(x + 4).expect("fixture seed fits in u8"));
            existing.insert(key, chunk);
        }
        let mut replacements = BTreeMap::new();
        for x in [-4, 0, 4] {
            let key = sample_key(state.world, ChunkCoordinate::new(x, -2, 3));
            let mut chunk = template.clone();
            chunk.key = key.clone();
            chunk.captured_world_revision = WorldRevision::new(8);
            chunk.data = sample_data(u8::try_from(x + 8).expect("fixture seed fits in u8"));
            replacements.insert(key, chunk);
        }
        let mut materialized = existing.clone();
        materialized.extend(replacements.clone());

        let projected = hash_projected_materialized_chunk_state(
            state.world,
            WorldRevision::new(8),
            &existing,
            &replacements,
        )
        .expect("the bounded projected fixture has representable lengths");
        let expected =
            hash_materialized_chunk_state(state.world, WorldRevision::new(8), &materialized)
                .expect("the bounded materialized fixture has representable lengths");
        assert_eq!(projected, expected);
    }

    #[test]
    fn envelope_fields_affect_materialized_chunk_state_hash() {
        assert_hash_changes("world ID", |state| state.world = alternate_world());
        assert_hash_changes("world revision", |state| {
            state.revision = WorldRevision::new(8);
        });
        assert_hash_changes("chunk count", |state| state.chunks.clear());
        assert_hash_changes("chunk world", |state| {
            mutate_key(state, |key| key.world = alternate_world());
        });
        assert_hash_changes("dimension ID", |state| {
            mutate_key(state, |key| {
                key.dimension = DimensionId::from_str("terrenia:dimension/alternate")
                    .expect("the alternate dimension ID is valid");
            });
        });
        assert_hash_changes("chunk x", |state| {
            mutate_key(state, |key| key.coordinate.x += 1);
        });
        assert_hash_changes("chunk y", |state| {
            mutate_key(state, |key| key.coordinate.y += 1);
        });
        assert_hash_changes("chunk z", |state| {
            mutate_key(state, |key| key.coordinate.z += 1);
        });
        assert_hash_changes("captured world revision", |state| {
            chunk_mut(state).captured_world_revision = WorldRevision::new(8);
        });
        assert_hash_changes("chunk revision", |state| {
            chunk_mut(state).revision = ChunkRevision::new(7);
        });
        assert_hash_changes("voxel revision", |state| {
            chunk_mut(state).domain_revisions.voxels = VoxelRevision::new(3);
        });
        assert_hash_changes("entity revision", |state| {
            chunk_mut(state).domain_revisions.persistent_entities =
                PersistentEntityRevision::new(4);
        });
        assert_hash_changes("continuation revision", |state| {
            chunk_mut(state).domain_revisions.continuation = ContinuationRevision::new(5);
        });
    }

    #[test]
    fn payload_fields_affect_materialized_chunk_state_hash() {
        assert_hash_changes("voxel schema", |state| {
            data_mut(state).voxels.schema =
                SchemaId::from_str("latticeaxiom:schema/alternate-voxels@1")
                    .expect("the alternate voxel schema ID is valid");
        });
        assert_hash_changes("voxel schema version", |state| {
            data_mut(state).voxels.schema_version =
                PayloadSchemaVersion::new(2).expect("version two is positive");
        });
        assert_hash_changes("voxel bytes", |state| {
            data_mut(state).voxels.bytes.push(0xa1);
        });
        assert_hash_changes("entity count", |state| {
            data_mut(state).persistent_entities.clear();
        });
        assert_hash_changes("entity ID", |state| {
            let data = data_mut(state);
            let (_, payload) = data
                .persistent_entities
                .pop_first()
                .expect("sample data contains one entity");
            data.persistent_entities
                .insert(PersistentEntityId::from_u128(999), payload);
        });
        assert_hash_changes("entity schema", |state| {
            data_mut(state)
                .persistent_entities
                .values_mut()
                .next()
                .expect("sample data contains one entity")
                .schema = SchemaId::from_str("latticeaxiom:schema/alternate-entity@1")
                .expect("the alternate entity schema ID is valid");
        });
        assert_hash_changes("entity schema version", |state| {
            data_mut(state)
                .persistent_entities
                .values_mut()
                .next()
                .expect("sample data contains one entity")
                .schema_version = PayloadSchemaVersion::new(2).expect("version two is positive");
        });
        assert_hash_changes("entity bytes", |state| {
            data_mut(state)
                .persistent_entities
                .values_mut()
                .next()
                .expect("sample data contains one entity")
                .bytes
                .push(0xe1);
        });
        assert_hash_changes("continuation count", |state| {
            data_mut(state).continuations.clear();
        });
        assert_hash_changes("continuation ID", |state| {
            let data = data_mut(state);
            let (_, payload) = data
                .continuations
                .pop_first()
                .expect("sample data contains one continuation");
            data.continuations
                .insert(ContinuationId::from_u128(999), payload);
        });
        assert_hash_changes("continuation schema", |state| {
            data_mut(state)
                .continuations
                .values_mut()
                .next()
                .expect("sample data contains one continuation")
                .schema = SchemaId::from_str("latticeaxiom:schema/alternate-continuation@1")
                .expect("the alternate continuation schema ID is valid");
        });
        assert_hash_changes("continuation schema version", |state| {
            data_mut(state)
                .continuations
                .values_mut()
                .next()
                .expect("sample data contains one continuation")
                .schema_version = PayloadSchemaVersion::new(2).expect("version two is positive");
        });
        assert_hash_changes("continuation bytes", |state| {
            data_mut(state)
                .continuations
                .values_mut()
                .next()
                .expect("sample data contains one continuation")
                .bytes
                .push(0xc1);
        });
        assert_hash_changes("provenance count", |state| {
            data_mut(state).provenance.clear();
        });
        assert_hash_changes("provenance ID", |state| {
            let data = data_mut(state);
            let (_, digest) = data
                .provenance
                .pop_first()
                .expect("sample data contains one provenance entry");
            data.provenance.insert(
                StableId::from_str("latticeaxiom:provenance/alternate@1")
                    .expect("the alternate provenance ID is valid"),
                digest,
            );
        });
        assert_hash_changes("provenance digest", |state| {
            *data_mut(state)
                .provenance
                .values_mut()
                .next()
                .expect("sample data contains one provenance entry") =
                CanonicalHash::digest(b"alternate provenance");
        });
    }
}
