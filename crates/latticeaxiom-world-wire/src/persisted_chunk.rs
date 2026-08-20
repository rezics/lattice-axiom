use std::{
    collections::BTreeMap,
    num::{NonZeroU16, NonZeroU32, NonZeroU64},
    str::FromStr,
};

use latticeaxiom_core::{CanonicalHash, PackageName, SchemaId, StableId, WorldId};
use latticeaxiom_storage::{
    ChunkData, ChunkKey, ChunkRevision, ContinuationId, ContinuationRevision, DimensionId,
    PayloadSchemaVersion, PersistentEntityId, PersistentEntityRevision, VersionedPayload,
    VoxelRevision, WorldRevision,
};
use serde::{Deserialize, Serialize};

use crate::snapshot::{codec_seal, encode_typed_snapshot_exact};
use crate::{
    SnapshotCodecLimits, SnapshotCodecResource, SnapshotContract, SnapshotSchemaCodec,
    ValidatedSnapshotPayload, WireResult, WireSegment, WorldWireError, WorldWireLimits,
    decode_typed_snapshot,
};

/// Fixed first-party package owner of persisted chunk snapshot schema version one.
pub const PERSISTED_CHUNK_SNAPSHOT_OWNER_V1: &str = "latticeaxiom";

/// Fixed schema identity of persisted chunk snapshot schema version one.
pub const PERSISTED_CHUNK_SNAPSHOT_SCHEMA_ID_V1: &str = "latticeaxiom:schema/persisted-chunk@1";

/// Exact owner-controlled persisted chunk payload version.
pub const PERSISTED_CHUNK_SNAPSHOT_SCHEMA_VERSION_V1: u32 = 1;

const MAX_PAYLOAD_BYTES: u64 = 64 * 1024 * 1024;
const MAX_COLLECTION_ENTRIES: u32 = 65_536;
const MAX_NESTING_DEPTH: u16 = 4;
const MAX_IDENTIFIER_BYTES: u64 = 1_024;
const MAX_NESTED_PAYLOAD_BYTES: u64 = 64 * 1024 * 1024;
const MAX_TOTAL_NESTED_PAYLOAD_BYTES: u64 = 64 * 1024 * 1024;

/// Persisted per-domain revisions belonging to one complete chunk snapshot.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize)]
pub struct PersistedChunkDomainRevisionsV1 {
    voxels: VoxelRevision,
    persistent_entities: PersistentEntityRevision,
    continuation: ContinuationRevision,
}

#[derive(Deserialize)]
struct PersistedChunkDomainRevisionsDecodeV1 {
    voxels: VoxelRevision,
    persistent_entities: PersistentEntityRevision,
    continuation: ContinuationRevision,
}

impl PersistedChunkDomainRevisionsDecodeV1 {
    const fn into_value(self) -> PersistedChunkDomainRevisionsV1 {
        PersistedChunkDomainRevisionsV1::new(
            self.voxels,
            self.persistent_entities,
            self.continuation,
        )
    }
}

impl PersistedChunkDomainRevisionsV1 {
    /// Creates the exact derived-work revision tuple captured by a chunk snapshot.
    #[must_use]
    pub const fn new(
        voxels: VoxelRevision,
        persistent_entities: PersistentEntityRevision,
        continuation: ContinuationRevision,
    ) -> Self {
        Self {
            voxels,
            persistent_entities,
            continuation,
        }
    }

    /// Returns the voxel-domain revision.
    #[must_use]
    pub const fn voxels(self) -> VoxelRevision {
        self.voxels
    }

    /// Returns the persistent-entity-domain revision.
    #[must_use]
    pub const fn persistent_entities(self) -> PersistentEntityRevision {
        self.persistent_entities
    }

    /// Returns the simulation-continuation-domain revision.
    #[must_use]
    pub const fn continuation(self) -> ContinuationRevision {
        self.continuation
    }
}

/// First-party wire DTO for one complete authoritative persisted chunk.
///
/// This schema deliberately reuses canonical core/storage identities and
/// payloads. It contains no process-local handles, implicit enums, hash-ordered
/// collections, platform-sized integers, or Bevy values.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct PersistedChunkSnapshotV1 {
    key: ChunkKey,
    captured_world_revision: WorldRevision,
    chunk_revision: ChunkRevision,
    domain_revisions: PersistedChunkDomainRevisionsV1,
    data: ChunkData,
    requirement_closure_hash: [u8; 32],
}

#[derive(Deserialize)]
struct PersistedChunkSnapshotDecodeV1 {
    key: ChunkKey,
    captured_world_revision: WorldRevision,
    chunk_revision: ChunkRevision,
    domain_revisions: PersistedChunkDomainRevisionsDecodeV1,
    data: ChunkData,
    requirement_closure_hash: [u8; 32],
}

impl PersistedChunkSnapshotDecodeV1 {
    fn into_value(self) -> PersistedChunkSnapshotV1 {
        PersistedChunkSnapshotV1::new(
            self.key,
            self.captured_world_revision,
            self.chunk_revision,
            self.domain_revisions.into_value(),
            self.data,
            self.requirement_closure_hash,
        )
    }
}

impl PersistedChunkSnapshotV1 {
    /// Creates one complete persisted chunk snapshot value.
    #[must_use]
    pub const fn new(
        key: ChunkKey,
        captured_world_revision: WorldRevision,
        chunk_revision: ChunkRevision,
        domain_revisions: PersistedChunkDomainRevisionsV1,
        data: ChunkData,
        requirement_closure_hash: [u8; 32],
    ) -> Self {
        Self {
            key,
            captured_world_revision,
            chunk_revision,
            domain_revisions,
            data,
            requirement_closure_hash,
        }
    }

    /// Returns the complete logical chunk key duplicated for corruption checks.
    #[must_use]
    pub const fn key(&self) -> &ChunkKey {
        &self.key
    }

    /// Returns the world revision captured by this complete snapshot.
    #[must_use]
    pub const fn captured_world_revision(&self) -> WorldRevision {
        self.captured_world_revision
    }

    /// Returns the complete authoritative chunk revision.
    #[must_use]
    pub const fn chunk_revision(&self) -> ChunkRevision {
        self.chunk_revision
    }

    /// Returns the derived-work revision tuple.
    #[must_use]
    pub const fn domain_revisions(&self) -> PersistedChunkDomainRevisionsV1 {
        self.domain_revisions
    }

    /// Returns the complete authoritative chunk data.
    #[must_use]
    pub const fn data(&self) -> &ChunkData {
        &self.data
    }

    /// Returns the exact world requirement-closure digest captured by this record.
    #[must_use]
    pub const fn requirement_closure_hash(&self) -> &[u8; 32] {
        &self.requirement_closure_hash
    }
}

/// Result of schema-owned persisted chunk encoding.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EncodedPersistedChunkSnapshotV1 {
    envelope: Vec<u8>,
    postcard_payload_bytes: u64,
}

impl EncodedPersistedChunkSnapshotV1 {
    /// Returns the exact `LAXWSNP\0` envelope bytes.
    #[must_use]
    pub fn envelope(&self) -> &[u8] {
        &self.envelope
    }

    /// Consumes the receipt and returns the exact envelope bytes.
    #[must_use]
    pub fn into_envelope(self) -> Vec<u8> {
        self.envelope
    }

    /// Returns uncompressed postcard bytes for commit-byte admission accounting.
    #[must_use]
    pub const fn postcard_payload_bytes(&self) -> u64 {
        self.postcard_payload_bytes
    }
}

/// Sealed first-party codec for [`PersistedChunkSnapshotV1`].
///
/// Construction takes no schema, owner, version, or budget arguments, so a
/// storage adapter cannot publish authoritative bytes under a drifting contract.
#[derive(Clone, Debug)]
pub struct PersistedChunkSnapshotCodecV1 {
    schema: SchemaId,
    owner: PackageName,
    version: PayloadSchemaVersion,
    limits: SnapshotCodecLimits,
    #[cfg(test)]
    postcard_decode_calls: std::cell::Cell<u32>,
}

impl PersistedChunkSnapshotCodecV1 {
    /// Creates the fixed first-party schema codec.
    ///
    /// # Errors
    ///
    /// Returns a typed identity/version error if a compile-time contract
    /// constant ceases to satisfy the canonical shared identity grammar.
    pub fn new() -> WireResult<Self> {
        let schema =
            SchemaId::from_str(PERSISTED_CHUNK_SNAPSHOT_SCHEMA_ID_V1).map_err(|error| {
                WorldWireError::InvalidSchemaId {
                    reason: error.to_string(),
                }
            })?;
        let owner = PackageName::from_str(PERSISTED_CHUNK_SNAPSHOT_OWNER_V1).map_err(|error| {
            WorldWireError::InvalidSnapshotOwner {
                reason: error.to_string(),
            }
        })?;
        let version = PayloadSchemaVersion::new(PERSISTED_CHUNK_SNAPSHOT_SCHEMA_VERSION_V1)
            .map_err(|_| WorldWireError::InvalidSchemaVersion)?;
        let max_payload_bytes =
            NonZeroU64::new(MAX_PAYLOAD_BYTES).ok_or(WorldWireError::InvalidLimit {
                name: "persisted_chunk_max_payload_bytes",
                value: MAX_PAYLOAD_BYTES,
            })?;
        let max_collection_entries =
            NonZeroU32::new(MAX_COLLECTION_ENTRIES).ok_or(WorldWireError::InvalidLimit {
                name: "persisted_chunk_max_collection_entries",
                value: u64::from(MAX_COLLECTION_ENTRIES),
            })?;
        let max_nesting_depth =
            NonZeroU16::new(MAX_NESTING_DEPTH).ok_or(WorldWireError::InvalidLimit {
                name: "persisted_chunk_max_nesting_depth",
                value: u64::from(MAX_NESTING_DEPTH),
            })?;
        let limits =
            SnapshotCodecLimits::new(max_payload_bytes, max_collection_entries, max_nesting_depth);
        Ok(Self {
            schema,
            owner,
            version,
            limits,
            #[cfg(test)]
            postcard_decode_calls: std::cell::Cell::new(0),
        })
    }

    /// Returns the fixed schema identity.
    #[must_use]
    pub const fn schema(&self) -> &SchemaId {
        &self.schema
    }

    /// Returns the fixed first-party owner.
    #[must_use]
    pub const fn owner(&self) -> &PackageName {
        &self.owner
    }

    /// Returns the exact positive payload schema version.
    #[must_use]
    pub const fn version(&self) -> PayloadSchemaVersion {
        self.version
    }

    /// Returns the immutable schema-owned decode budgets.
    #[must_use]
    pub const fn codec_limits(&self) -> SnapshotCodecLimits {
        self.limits
    }

    #[cfg(test)]
    fn postcard_decode_calls(&self) -> u32 {
        self.postcard_decode_calls.get()
    }
}

impl codec_seal::Sealed for PersistedChunkSnapshotCodecV1 {}

impl SnapshotSchemaCodec for PersistedChunkSnapshotCodecV1 {
    type Value = PersistedChunkSnapshotV1;

    fn contract(&self) -> SnapshotContract<'_> {
        SnapshotContract::new(&self.schema, &self.owner, self.version)
    }

    fn limits(&self) -> SnapshotCodecLimits {
        self.limits
    }

    fn validate_for_encode(
        &self,
        value: &Self::Value,
        limits: SnapshotCodecLimits,
    ) -> WireResult<()> {
        validate_value(value, limits)
    }

    fn decode_postcard_bounded(
        &self,
        payload: ValidatedSnapshotPayload<'_>,
    ) -> WireResult<Self::Value> {
        preflight_payload(payload.bytes(), payload.limits())?;
        #[cfg(test)]
        self.postcard_decode_calls
            .set(self.postcard_decode_calls.get().saturating_add(1));
        let (decoded, remaining): (PersistedChunkSnapshotDecodeV1, &[u8]) =
            postcard::take_from_bytes(payload.bytes()).map_err(|error| {
                WorldWireError::MalformedPayload {
                    reason: error.to_string(),
                }
            })?;
        let decoded = decoded.into_value();
        if !remaining.is_empty() {
            return Err(WorldWireError::TrailingPayloadBytes {
                remaining: remaining.len(),
            });
        }
        validate_value(&decoded, payload.limits())?;

        Ok(decoded)
    }
}

/// Encodes a persisted chunk exclusively through its fixed audited schema codec.
///
/// The function computes and checks the exact postcard length before allocating
/// serialization scratch. It returns the uncompressed payload length separately
/// for transaction admission accounting.
///
/// # Errors
///
/// Returns a typed contract, canonicality, resource-budget, postcard, or outer
/// envelope error.
pub fn encode_persisted_chunk_snapshot_v1(
    value: &PersistedChunkSnapshotV1,
    limits: WorldWireLimits,
) -> WireResult<EncodedPersistedChunkSnapshotV1> {
    let codec = PersistedChunkSnapshotCodecV1::new()?;
    codec.validate_for_encode(value, codec.limits())?;
    let payload_length = encoded_payload_length(value)?;
    codec.limits().check_payload_bytes(payload_length)?;
    limits.check(WireSegment::SnapshotPayload, payload_length)?;
    let envelope = encode_typed_snapshot_exact(&codec, value, payload_length, limits)?;
    let postcard_payload_bytes =
        u64::try_from(payload_length).map_err(|_| WorldWireError::LengthOverflow {
            segment: WireSegment::SnapshotPayload,
        })?;
    Ok(EncodedPersistedChunkSnapshotV1 {
        envelope,
        postcard_payload_bytes,
    })
}

/// Decodes a complete persisted chunk envelope through the fixed audited codec.
///
/// # Errors
///
/// Returns a typed envelope, exact-contract, resource-budget, malformed,
/// trailing, or canonicality error before exposing a persisted DTO.
pub fn decode_persisted_chunk_snapshot_v1(
    encoded: &[u8],
    limits: WorldWireLimits,
) -> WireResult<PersistedChunkSnapshotV1> {
    let codec = PersistedChunkSnapshotCodecV1::new()?;
    decode_typed_snapshot(encoded, &codec, limits)
}

fn validate_value(value: &PersistedChunkSnapshotV1, limits: SnapshotCodecLimits) -> WireResult<()> {
    limits.check_nesting_depth(u32::from(MAX_NESTING_DEPTH))?;
    check_identifier_bytes(value.key.dimension.as_str().len())?;

    let chunk_revision = value.chunk_revision.get();
    if chunk_revision > value.captured_world_revision.get() {
        return Err(WorldWireError::NonCanonicalPayload {
            reason: "chunk revision exceeds its captured world revision".to_owned(),
        });
    }
    for (field, revision) in [
        ("voxel", value.domain_revisions.voxels.get()),
        (
            "persistent-entity",
            value.domain_revisions.persistent_entities.get(),
        ),
        ("continuation", value.domain_revisions.continuation.get()),
    ] {
        if revision > chunk_revision {
            return Err(WorldWireError::NonCanonicalPayload {
                reason: format!("{field} domain revision exceeds the complete chunk revision"),
            });
        }
    }

    let data = &value.data;
    let collection_entries = data
        .persistent_entities()
        .len()
        .checked_add(data.continuations().len())
        .and_then(|count| count.checked_add(data.provenance().len()))
        .ok_or(WorldWireError::CodecBudgetExceeded {
            resource: SnapshotCodecResource::CollectionElements,
            actual: u64::MAX,
            maximum: u64::from(limits.max_collection_elements()),
        })?;
    limits.check_collection_elements(u64::try_from(collection_entries).unwrap_or(u64::MAX))?;

    let mut nested_payload_bytes = 0_u64;
    validate_versioned_payload(data.voxels(), &mut nested_payload_bytes)?;
    for payload in data.persistent_entities().values() {
        validate_versioned_payload(payload, &mut nested_payload_bytes)?;
    }
    for payload in data.continuations().values() {
        validate_versioned_payload(payload, &mut nested_payload_bytes)?;
    }
    for stable_id in data.provenance().keys() {
        check_identifier_bytes(stable_id.as_str().len())?;
    }
    Ok(())
}

fn validate_versioned_payload(payload: &VersionedPayload, total: &mut u64) -> WireResult<()> {
    check_identifier_bytes(payload.schema().as_str().len())?;
    let bytes =
        u64::try_from(payload.bytes().len()).map_err(|_| WorldWireError::LengthOverflow {
            segment: WireSegment::SnapshotPayload,
        })?;
    check_codec_budget(
        SnapshotCodecResource::NestedPayloadBytes,
        bytes,
        MAX_NESTED_PAYLOAD_BYTES,
    )?;
    *total = total
        .checked_add(bytes)
        .ok_or(WorldWireError::CodecBudgetExceeded {
            resource: SnapshotCodecResource::NestedPayloadTotalBytes,
            actual: u64::MAX,
            maximum: MAX_TOTAL_NESTED_PAYLOAD_BYTES,
        })?;
    check_codec_budget(
        SnapshotCodecResource::NestedPayloadTotalBytes,
        *total,
        MAX_TOTAL_NESTED_PAYLOAD_BYTES,
    )
}

fn check_identifier_bytes(actual: usize) -> WireResult<()> {
    let actual = u64::try_from(actual).map_err(|_| WorldWireError::LengthOverflow {
        segment: WireSegment::SchemaId,
    })?;
    check_codec_budget(
        SnapshotCodecResource::IdentifierBytes,
        actual,
        MAX_IDENTIFIER_BYTES,
    )
}

fn check_codec_budget(
    resource: SnapshotCodecResource,
    actual: u64,
    maximum: u64,
) -> WireResult<()> {
    if actual > maximum {
        Err(WorldWireError::CodecBudgetExceeded {
            resource,
            actual,
            maximum,
        })
    } else {
        Ok(())
    }
}

fn preflight_payload(payload: &[u8], limits: SnapshotCodecLimits) -> WireResult<()> {
    limits.check_payload_bytes(payload.len())?;
    limits.check_nesting_depth(u32::from(MAX_NESTING_DEPTH))?;
    let mut cursor = PayloadCursor::new(payload);
    let mut budget = DecodeBudget::new(limits);

    scan_world_id(&mut cursor)?;
    scan_dimension_id(&mut cursor)?;
    cursor.read_i32("chunk x")?;
    cursor.read_i32("chunk y")?;
    cursor.read_i32("chunk z")?;
    cursor.read_u64("captured world revision")?;
    cursor.read_u64("chunk revision")?;
    cursor.read_u64("voxel revision")?;
    cursor.read_u64("persistent-entity revision")?;
    cursor.read_u64("continuation revision")?;

    scan_versioned_payload(&mut cursor, &mut budget)?;
    scan_payload_map::<PersistentEntityId>(&mut cursor, &mut budget, "persistent entities")?;
    scan_payload_map::<ContinuationId>(&mut cursor, &mut budget, "continuations")?;
    scan_provenance_map(&mut cursor, &mut budget)?;
    cursor.read_exact(32, "requirement closure hash")?;
    if cursor.remaining() != 0 {
        return Err(WorldWireError::TrailingPayloadBytes {
            remaining: cursor.remaining(),
        });
    }
    Ok(())
}

fn scan_world_id(cursor: &mut PayloadCursor<'_>) -> WireResult<()> {
    let text = cursor.read_identifier_text("chunk world ID")?;
    WorldId::from_str(text).map_err(|error| WorldWireError::InvalidWorldId {
        reason: error.to_string(),
    })?;
    Ok(())
}

fn scan_dimension_id(cursor: &mut PayloadCursor<'_>) -> WireResult<()> {
    let text = cursor.read_identifier_text("chunk dimension ID")?;
    DimensionId::from_str(text).map_err(|error| WorldWireError::InvalidDimensionId {
        reason: error.to_string(),
    })?;
    Ok(())
}

fn scan_schema_id(cursor: &mut PayloadCursor<'_>) -> WireResult<()> {
    let text = cursor.read_identifier_text("nested payload schema ID")?;
    SchemaId::from_str(text).map_err(|error| WorldWireError::InvalidSchemaId {
        reason: error.to_string(),
    })?;
    Ok(())
}

fn scan_versioned_payload(
    cursor: &mut PayloadCursor<'_>,
    budget: &mut DecodeBudget,
) -> WireResult<()> {
    scan_schema_id(cursor)?;
    if cursor.read_u32("nested payload schema version")? == 0 {
        return Err(WorldWireError::InvalidSchemaVersion);
    }
    let length = cursor.read_length("nested payload length")?;
    let length_u64 = u64::try_from(length).map_err(|_| WorldWireError::LengthOverflow {
        segment: WireSegment::SnapshotPayload,
    })?;
    check_codec_budget(
        SnapshotCodecResource::NestedPayloadBytes,
        length_u64,
        MAX_NESTED_PAYLOAD_BYTES,
    )?;
    budget.add_nested_payload_bytes(length_u64)?;
    cursor.read_exact(length, "nested payload bytes")?;
    Ok(())
}

trait FixedMapKey {
    const SECTION: &'static str;
}

impl FixedMapKey for PersistentEntityId {
    const SECTION: &'static str = "persistent entity ID";
}

impl FixedMapKey for ContinuationId {
    const SECTION: &'static str = "continuation ID";
}

fn scan_payload_map<K: FixedMapKey>(
    cursor: &mut PayloadCursor<'_>,
    budget: &mut DecodeBudget,
    section: &'static str,
) -> WireResult<()> {
    let count = cursor.read_length(section)?;
    budget.add_collection_entries(count)?;
    let mut previous: Option<&[u8]> = None;
    for _ in 0..count {
        let key = cursor.read_exact(16, K::SECTION)?;
        if previous.is_some_and(|previous| previous >= key) {
            return Err(WorldWireError::NonCanonicalPayload {
                reason: format!("{section} map keys are not strictly ordered"),
            });
        }
        previous = Some(key);
        scan_versioned_payload(cursor, budget)?;
    }
    Ok(())
}

fn scan_provenance_map(
    cursor: &mut PayloadCursor<'_>,
    budget: &mut DecodeBudget,
) -> WireResult<()> {
    let count = cursor.read_length("provenance entries")?;
    budget.add_collection_entries(count)?;
    let mut previous: Option<&str> = None;
    for _ in 0..count {
        let key = cursor.read_identifier_text("provenance stable ID")?;
        StableId::from_str(key).map_err(|error| WorldWireError::MalformedPayload {
            reason: format!("invalid provenance stable ID: {error}"),
        })?;
        if previous.is_some_and(|previous| previous >= key) {
            return Err(WorldWireError::NonCanonicalPayload {
                reason: "provenance map keys are not strictly ordered".to_owned(),
            });
        }
        previous = Some(key);
        let digest = cursor.read_text("provenance digest")?;
        if digest.len() != CanonicalHash::BYTE_LENGTH * 2
            || !digest
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err(WorldWireError::NonCanonicalPayload {
                reason: "provenance digest is not canonical lowercase SHA-256 hex".to_owned(),
            });
        }
    }
    Ok(())
}

struct DecodeBudget {
    limits: SnapshotCodecLimits,
    collection_entries: u64,
    nested_payload_bytes: u64,
}

impl DecodeBudget {
    const fn new(limits: SnapshotCodecLimits) -> Self {
        Self {
            limits,
            collection_entries: 0,
            nested_payload_bytes: 0,
        }
    }

    fn add_collection_entries(&mut self, additional: usize) -> WireResult<()> {
        let additional =
            u64::try_from(additional).map_err(|_| WorldWireError::CodecBudgetExceeded {
                resource: SnapshotCodecResource::CollectionElements,
                actual: u64::MAX,
                maximum: u64::from(self.limits.max_collection_elements()),
            })?;
        self.collection_entries = self.collection_entries.checked_add(additional).ok_or(
            WorldWireError::CodecBudgetExceeded {
                resource: SnapshotCodecResource::CollectionElements,
                actual: u64::MAX,
                maximum: u64::from(self.limits.max_collection_elements()),
            },
        )?;
        self.limits
            .check_collection_elements(self.collection_entries)
    }

    fn add_nested_payload_bytes(&mut self, additional: u64) -> WireResult<()> {
        self.nested_payload_bytes = self.nested_payload_bytes.checked_add(additional).ok_or(
            WorldWireError::CodecBudgetExceeded {
                resource: SnapshotCodecResource::NestedPayloadTotalBytes,
                actual: u64::MAX,
                maximum: MAX_TOTAL_NESTED_PAYLOAD_BYTES,
            },
        )?;
        check_codec_budget(
            SnapshotCodecResource::NestedPayloadTotalBytes,
            self.nested_payload_bytes,
            MAX_TOTAL_NESTED_PAYLOAD_BYTES,
        )
    }
}

struct PayloadCursor<'a> {
    bytes: &'a [u8],
    position: usize,
}

impl<'a> PayloadCursor<'a> {
    const fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, position: 0 }
    }

    fn read_exact(&mut self, length: usize, section: &'static str) -> WireResult<&'a [u8]> {
        let end =
            self.position
                .checked_add(length)
                .ok_or_else(|| WorldWireError::MalformedPayload {
                    reason: format!("{section} length overflow"),
                })?;
        let Some(value) = self.bytes.get(self.position..end) else {
            return Err(WorldWireError::MalformedPayload {
                reason: format!("truncated {section}"),
            });
        };
        self.position = end;
        Ok(value)
    }

    fn read_varint(&mut self, section: &'static str, maximum: u64) -> WireResult<u64> {
        let mut value = 0_u64;
        for index in 0..10_u32 {
            let byte = self.read_exact(1, section)?[0];
            let low = u64::from(byte & 0x7f);
            if index == 9 && low > 1 {
                return Err(WorldWireError::MalformedPayload {
                    reason: format!("{section} varint overflows u64"),
                });
            }
            value |= low << (index * 7);
            if byte & 0x80 == 0 {
                if index > 0 && low == 0 {
                    return Err(WorldWireError::NonCanonicalPayload {
                        reason: format!("{section} uses a redundant varint byte"),
                    });
                }
                if value > maximum {
                    return Err(WorldWireError::MalformedPayload {
                        reason: format!("{section} exceeds its fixed integer width"),
                    });
                }
                return Ok(value);
            }
        }
        Err(WorldWireError::MalformedPayload {
            reason: format!("unterminated {section} varint"),
        })
    }

    fn read_u32(&mut self, section: &'static str) -> WireResult<u32> {
        let value = self.read_varint(section, u64::from(u32::MAX))?;
        u32::try_from(value).map_err(|_| WorldWireError::MalformedPayload {
            reason: format!("{section} exceeds u32"),
        })
    }

    fn read_u64(&mut self, section: &'static str) -> WireResult<u64> {
        self.read_varint(section, u64::MAX)
    }

    fn read_i32(&mut self, section: &'static str) -> WireResult<()> {
        self.read_varint(section, u64::from(u32::MAX))?;
        Ok(())
    }

    fn read_length(&mut self, section: &'static str) -> WireResult<usize> {
        let length = self.read_u64(section)?;
        usize::try_from(length).map_err(|_| WorldWireError::LengthOverflow {
            segment: WireSegment::SnapshotPayload,
        })
    }

    fn read_text(&mut self, section: &'static str) -> WireResult<&'a str> {
        let length = self.read_length(section)?;
        let bytes = self.read_exact(length, section)?;
        std::str::from_utf8(bytes).map_err(|_| WorldWireError::MalformedPayload {
            reason: format!("{section} is not UTF-8"),
        })
    }

    fn read_identifier_text(&mut self, section: &'static str) -> WireResult<&'a str> {
        let length = self.read_u64(section)?;
        check_codec_budget(
            SnapshotCodecResource::IdentifierBytes,
            length,
            MAX_IDENTIFIER_BYTES,
        )?;
        let length = usize::try_from(length).map_err(|_| WorldWireError::LengthOverflow {
            segment: WireSegment::SchemaId,
        })?;
        let bytes = self.read_exact(length, section)?;
        std::str::from_utf8(bytes).map_err(|_| WorldWireError::MalformedPayload {
            reason: format!("{section} is not UTF-8"),
        })
    }

    const fn remaining(&self) -> usize {
        self.bytes.len() - self.position
    }
}

fn encoded_payload_length(value: &PersistedChunkSnapshotV1) -> WireResult<usize> {
    let mut meter = LengthMeter::default();
    meter.text(&value.key.world.to_string())?;
    meter.text(value.key.dimension.as_str())?;
    meter.signed_i32(value.key.coordinate.x)?;
    meter.signed_i32(value.key.coordinate.y)?;
    meter.signed_i32(value.key.coordinate.z)?;
    meter.unsigned(value.captured_world_revision.get())?;
    meter.unsigned(value.chunk_revision.get())?;
    meter.unsigned(value.domain_revisions.voxels.get())?;
    meter.unsigned(value.domain_revisions.persistent_entities.get())?;
    meter.unsigned(value.domain_revisions.continuation.get())?;
    meter.versioned_payload(value.data.voxels())?;
    meter.payload_map(value.data.persistent_entities())?;
    meter.payload_map(value.data.continuations())?;
    meter.provenance_map(value.data.provenance())?;
    meter.add(32)?;
    Ok(meter.length)
}

#[derive(Default)]
struct LengthMeter {
    length: usize,
}

impl LengthMeter {
    fn add(&mut self, additional: usize) -> WireResult<()> {
        self.length =
            self.length
                .checked_add(additional)
                .ok_or(WorldWireError::LengthOverflow {
                    segment: WireSegment::SnapshotPayload,
                })?;
        Ok(())
    }

    fn unsigned(&mut self, value: u64) -> WireResult<()> {
        self.add(varint_length(value))
    }

    fn signed_i32(&mut self, value: i32) -> WireResult<()> {
        let sign_mask = u32::from(value < 0).wrapping_neg();
        let zigzag = value.cast_unsigned().wrapping_shl(1) ^ sign_mask;
        self.unsigned(u64::from(zigzag))
    }

    fn text(&mut self, value: &str) -> WireResult<()> {
        self.unsigned(
            u64::try_from(value.len()).map_err(|_| WorldWireError::LengthOverflow {
                segment: WireSegment::SchemaId,
            })?,
        )?;
        self.add(value.len())
    }

    fn bytes(&mut self, value: &[u8]) -> WireResult<()> {
        self.unsigned(
            u64::try_from(value.len()).map_err(|_| WorldWireError::LengthOverflow {
                segment: WireSegment::SnapshotPayload,
            })?,
        )?;
        self.add(value.len())
    }

    fn versioned_payload(&mut self, payload: &VersionedPayload) -> WireResult<()> {
        self.text(payload.schema().as_str())?;
        self.unsigned(u64::from(payload.schema_version().get()))?;
        self.bytes(payload.bytes())
    }

    fn payload_map<K: Ord>(&mut self, map: &BTreeMap<K, VersionedPayload>) -> WireResult<()> {
        self.unsigned(
            u64::try_from(map.len()).map_err(|_| WorldWireError::LengthOverflow {
                segment: WireSegment::SnapshotPayload,
            })?,
        )?;
        for payload in map.values() {
            self.add(16)?;
            self.versioned_payload(payload)?;
        }
        Ok(())
    }

    fn provenance_map(&mut self, map: &BTreeMap<StableId, CanonicalHash>) -> WireResult<()> {
        self.unsigned(
            u64::try_from(map.len()).map_err(|_| WorldWireError::LengthOverflow {
                segment: WireSegment::SnapshotPayload,
            })?,
        )?;
        for (key, digest) in map {
            self.text(key.as_str())?;
            self.text(&digest.to_string())?;
        }
        Ok(())
    }
}

const fn varint_length(mut value: u64) -> usize {
    let mut length = 1;
    while value >= 0x80 {
        value >>= 7;
        length += 1;
    }
    length
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeMap, error::Error, str::FromStr};

    use latticeaxiom_core::{CanonicalHash, PackageName, SchemaId, StableId, WorldId};
    use latticeaxiom_storage::{
        ChunkCoordinate, ChunkData, ChunkKey, ChunkRevision, ContinuationId, ContinuationRevision,
        DimensionId, PayloadSchemaVersion, PersistentEntityId, PersistentEntityRevision,
        VersionedPayload, VoxelRevision, WorldRevision,
    };

    use super::*;
    use crate::{decode_typed_snapshot, encode_snapshot, preflight_snapshot};

    fn payload(seed: u8) -> Result<VersionedPayload, Box<dyn Error>> {
        Ok(VersionedPayload::new(
            SchemaId::from_str("latticeaxiom:schema/test-payload@1")?,
            PayloadSchemaVersion::new(1)?,
            vec![seed, seed ^ 0x5a],
        ))
    }

    fn fixture() -> Result<PersistedChunkSnapshotV1, Box<dyn Error>> {
        let mut entities = BTreeMap::new();
        entities.insert(PersistentEntityId::from_u128(2), payload(7)?);
        let mut continuations = BTreeMap::new();
        continuations.insert(ContinuationId::from_u128(3), payload(11)?);
        let mut provenance = BTreeMap::new();
        provenance.insert(
            StableId::from_str("terrenia:generator/chunk@1")?,
            CanonicalHash::digest(b"fixture provenance"),
        );
        Ok(PersistedChunkSnapshotV1::new(
            ChunkKey::new(
                WorldId::from_str("018f1e2d-3c4b-4a59-8c6d-7e8f9012abcd")?,
                DimensionId::from_str("terrenia:dimension/terrenia")?,
                ChunkCoordinate::new(-2, 0, 3),
            ),
            WorldRevision::new(9),
            ChunkRevision::new(4),
            PersistedChunkDomainRevisionsV1::new(
                VoxelRevision::new(4),
                PersistentEntityRevision::new(2),
                ContinuationRevision::new(3),
            ),
            ChunkData::new(payload(1)?, entities, continuations, provenance),
            [0xa5; 32],
        ))
    }

    fn raw_envelope(codec: &PersistedChunkSnapshotCodecV1, bytes: Vec<u8>) -> WireResult<Vec<u8>> {
        encode_snapshot(
            codec.owner(),
            &VersionedPayload::new(codec.schema().clone(), codec.version(), bytes),
            WorldWireLimits::default(),
        )
    }

    fn fixture_payload() -> Result<Vec<u8>, Box<dyn Error>> {
        let value = fixture()?;
        let length = encoded_payload_length(&value)?;
        let mut bytes = vec![0_u8; length];
        Ok(postcard::to_slice(&value, &mut bytes)?.to_vec())
    }

    fn voxel_length_and_entity_count_offsets(payload: &[u8]) -> WireResult<(usize, usize)> {
        let mut cursor = PayloadCursor::new(payload);
        scan_world_id(&mut cursor)?;
        scan_dimension_id(&mut cursor)?;
        cursor.read_i32("chunk x")?;
        cursor.read_i32("chunk y")?;
        cursor.read_i32("chunk z")?;
        for field in [
            "captured world revision",
            "chunk revision",
            "voxel revision",
            "persistent-entity revision",
            "continuation revision",
        ] {
            cursor.read_u64(field)?;
        }
        scan_schema_id(&mut cursor)?;
        cursor.read_u32("nested payload schema version")?;
        let voxel_length_offset = cursor.position;
        let voxel_length = cursor.read_length("nested payload length")?;
        cursor.read_exact(voxel_length, "nested payload bytes")?;
        Ok((voxel_length_offset, cursor.position))
    }

    fn encoded_varint(mut value: u64) -> Vec<u8> {
        let mut encoded = Vec::new();
        loop {
            let low = u8::try_from(value & 0x7f).unwrap_or_default();
            value >>= 7;
            if value == 0 {
                encoded.push(low);
                return encoded;
            }
            encoded.push(low | 0x80);
        }
    }

    #[test]
    fn fixed_codec_round_trips_and_meter_matches_pinned_postcard() -> Result<(), Box<dyn Error>> {
        let value = fixture()?;
        let encoded = encode_persisted_chunk_snapshot_v1(&value, WorldWireLimits::default())?;
        assert_eq!(
            usize::try_from(encoded.postcard_payload_bytes())?,
            encoded_payload_length(&value)?
        );
        assert_eq!(
            decode_persisted_chunk_snapshot_v1(encoded.envelope(), WorldWireLimits::default())?,
            value
        );
        assert_eq!(
            hex::encode(
                preflight_snapshot(encoded.envelope(), WorldWireLimits::default())?.payload_bytes()
            ),
            "2430313866316532642d336334622d346135392d386336642d3765386639303132616263641b74657272656e69613a64696d656e73696f6e2f74657272656e69610300060904040203226c6174746963656178696f6d3a736368656d612f746573742d7061796c6f616440310102015b0100000000000000000000000000000002226c6174746963656178696f6d3a736368656d612f746573742d7061796c6f616440310102075d0100000000000000000000000000000003226c6174746963656178696f6d3a736368656d612f746573742d7061796c6f6164403101020b51011a74657272656e69613a67656e657261746f722f6368756e6b40314038353533623062396137653339343962373938366561666264353462373334653739393737623936326439326631663961303562626462393364323763623564a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5"
        );
        Ok(())
    }

    #[test]
    fn malformed_and_unbounded_payloads_never_reach_postcard_decode() -> Result<(), Box<dyn Error>>
    {
        let codec = PersistedChunkSnapshotCodecV1::new()?;
        let limits = WorldWireLimits::default();

        let mut redundant_world_length = fixture_payload()?;
        assert_eq!(redundant_world_length[0], 36);
        redundant_world_length.splice(0..1, [0xa4, 0]);
        let encoded = raw_envelope(&codec, redundant_world_length)?;
        assert!(matches!(
            decode_typed_snapshot(&encoded, &codec, limits),
            Err(WorldWireError::NonCanonicalPayload { .. })
        ));
        assert_eq!(codec.postcard_decode_calls(), 0);

        let mut trailing = fixture_payload()?;
        trailing.push(0);
        let encoded = raw_envelope(&codec, trailing)?;
        assert!(matches!(
            decode_typed_snapshot(&encoded, &codec, limits),
            Err(WorldWireError::TrailingPayloadBytes { remaining: 1 })
        ));
        assert_eq!(codec.postcard_decode_calls(), 0);

        let mut nested_oversize = fixture_payload()?;
        let (voxel_length_offset, _) = voxel_length_and_entity_count_offsets(&nested_oversize)?;
        nested_oversize.splice(
            voxel_length_offset..=voxel_length_offset,
            encoded_varint(MAX_NESTED_PAYLOAD_BYTES + 1),
        );
        let encoded = raw_envelope(&codec, nested_oversize)?;
        assert!(matches!(
            decode_typed_snapshot(&encoded, &codec, limits),
            Err(WorldWireError::CodecBudgetExceeded {
                resource: SnapshotCodecResource::NestedPayloadBytes,
                actual,
                maximum: MAX_NESTED_PAYLOAD_BYTES,
            }) if actual == MAX_NESTED_PAYLOAD_BYTES + 1
        ));
        assert_eq!(codec.postcard_decode_calls(), 0);

        let mut collection_oversize = fixture_payload()?;
        let (_, entity_count_offset) = voxel_length_and_entity_count_offsets(&collection_oversize)?;
        collection_oversize.splice(
            entity_count_offset..=entity_count_offset,
            encoded_varint(u64::from(MAX_COLLECTION_ENTRIES) + 1),
        );
        let encoded = raw_envelope(&codec, collection_oversize)?;
        assert!(matches!(
            decode_typed_snapshot(&encoded, &codec, limits),
            Err(WorldWireError::CodecBudgetExceeded {
                resource: SnapshotCodecResource::CollectionElements,
                actual,
                maximum,
            }) if actual == u64::from(MAX_COLLECTION_ENTRIES) + 1
                && maximum == u64::from(MAX_COLLECTION_ENTRIES)
        ));
        assert_eq!(codec.postcard_decode_calls(), 0);

        let payload = fixture_payload()?;
        let shallow = SnapshotCodecLimits::new(
            NonZeroU64::new(MAX_PAYLOAD_BYTES).ok_or("payload ceiling")?,
            NonZeroU32::new(MAX_COLLECTION_ENTRIES).ok_or("collection ceiling")?,
            NonZeroU16::new(MAX_NESTING_DEPTH - 1).ok_or("depth ceiling")?,
        );
        assert!(matches!(
            preflight_payload(&payload, shallow),
            Err(WorldWireError::CodecBudgetExceeded {
                resource: SnapshotCodecResource::NestingDepth,
                actual: 4,
                maximum: 3
            })
        ));
        assert_eq!(codec.postcard_decode_calls(), 0);
        Ok(())
    }

    #[test]
    fn truncation_and_mutation_corpora_never_panic() -> Result<(), Box<dyn Error>> {
        let codec = PersistedChunkSnapshotCodecV1::new()?;
        let payload = fixture_payload()?;
        for length in 0..=payload.len() {
            let envelope = raw_envelope(&codec, payload[..length].to_vec())?;
            assert!(
                std::panic::catch_unwind(|| {
                    let _ =
                        decode_persisted_chunk_snapshot_v1(&envelope, WorldWireLimits::default());
                })
                .is_ok()
            );
        }
        for index in 0..payload.len() {
            let mut mutation = payload.clone();
            mutation[index] ^= 0x5a;
            let envelope = raw_envelope(&codec, mutation)?;
            assert!(
                std::panic::catch_unwind(|| {
                    let _ =
                        decode_persisted_chunk_snapshot_v1(&envelope, WorldWireLimits::default());
                })
                .is_ok()
            );
        }

        let encoded = encode_persisted_chunk_snapshot_v1(&fixture()?, WorldWireLimits::default())?
            .into_envelope();
        for length in 0..=encoded.len() {
            assert!(
                std::panic::catch_unwind(|| {
                    let _ = decode_persisted_chunk_snapshot_v1(
                        &encoded[..length],
                        WorldWireLimits::default(),
                    );
                })
                .is_ok()
            );
        }
        for index in 0..encoded.len() {
            let mut mutation = encoded.clone();
            mutation[index] ^= 0xa5;
            assert!(
                std::panic::catch_unwind(|| {
                    let _ =
                        decode_persisted_chunk_snapshot_v1(&mutation, WorldWireLimits::default());
                })
                .is_ok()
            );
        }

        let malicious = raw_envelope(&codec, vec![u8::MAX; 512])?;
        assert!(
            std::panic::catch_unwind(|| {
                let _ = decode_persisted_chunk_snapshot_v1(&malicious, WorldWireLimits::default());
            })
            .is_ok()
        );
        Ok(())
    }

    #[test]
    fn unknown_contract_and_revision_invariants_fail_closed() -> Result<(), Box<dyn Error>> {
        let codec = PersistedChunkSnapshotCodecV1::new()?;
        let alternate_schema = SchemaId::from_str("other:schema/persisted-chunk@1")?;
        let encoded = encode_snapshot(
            codec.owner(),
            &VersionedPayload::new(alternate_schema, codec.version(), fixture_payload()?),
            WorldWireLimits::default(),
        )?;
        assert!(matches!(
            decode_typed_snapshot(&encoded, &codec, WorldWireLimits::default()),
            Err(WorldWireError::UnknownSchema { .. })
        ));
        assert_eq!(codec.postcard_decode_calls(), 0);

        let alternate_owner = PackageName::from_str("other")?;
        let encoded = encode_snapshot(
            &alternate_owner,
            &VersionedPayload::new(codec.schema().clone(), codec.version(), fixture_payload()?),
            WorldWireLimits::default(),
        )?;
        assert!(matches!(
            decode_typed_snapshot(&encoded, &codec, WorldWireLimits::default()),
            Err(WorldWireError::UnknownOwner { .. })
        ));
        assert_eq!(codec.postcard_decode_calls(), 0);

        let version_two = PayloadSchemaVersion::new(2)?;
        let encoded = encode_snapshot(
            codec.owner(),
            &VersionedPayload::new(codec.schema().clone(), version_two, fixture_payload()?),
            WorldWireLimits::default(),
        )?;
        assert!(matches!(
            decode_typed_snapshot(&encoded, &codec, WorldWireLimits::default()),
            Err(WorldWireError::UnsupportedSchemaVersion {
                found: 2,
                expected: 1
            })
        ));
        assert_eq!(codec.postcard_decode_calls(), 0);

        let mut invalid = fixture()?;
        invalid.chunk_revision = ChunkRevision::new(10);
        assert!(matches!(
            encode_persisted_chunk_snapshot_v1(&invalid, WorldWireLimits::default()),
            Err(WorldWireError::NonCanonicalPayload { .. })
        ));
        Ok(())
    }
}
