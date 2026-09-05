//! Materialized-chunk transaction DTOs for the sealed reference kernel.

#[cfg(test)]
use std::cell::Cell;
use std::{collections::BTreeMap, fmt, str::FromStr};

use latticeaxiom_core::{CanonicalHash, SchemaId, StableId, WorldId};
use serde::{Deserialize, Deserializer, Serialize, Serializer, de};

use crate::{MaterializedChunkStateHash, StorageError, StorageResult};

macro_rules! revision_type {
    ($name:ident, $documentation:literal) => {
        #[doc = $documentation]
        #[repr(transparent)]
        #[derive(
            Clone,
            Copy,
            Debug,
            Default,
            Deserialize,
            Eq,
            Hash,
            Ord,
            PartialEq,
            PartialOrd,
            Serialize,
        )]
        #[serde(transparent)]
        pub struct $name(u64);

        impl $name {
            /// Initial revision before the first authoritative change.
            pub const ZERO: Self = Self(0);

            /// Creates a revision from its checked persistent value.
            #[must_use]
            pub const fn new(value: u64) -> Self {
                Self(value)
            }

            /// Returns the persistent integer value.
            #[must_use]
            pub const fn get(self) -> u64 {
                self.0
            }
        }
    };
}

revision_type!(
    WorldRevision,
    "Monotonic sequence shared by every chunk changed by one authoritative transaction."
);
revision_type!(
    ChunkRevision,
    "Total authoritative revision of one materialized chunk."
);
revision_type!(
    VoxelRevision,
    "Voxel-domain revision used to invalidate derived voxel work."
);
revision_type!(
    PersistentEntityRevision,
    "Persistent-entity-domain revision used to invalidate entity work."
);
revision_type!(
    ContinuationRevision,
    "Continuation-domain revision used to invalidate scheduled work."
);

macro_rules! byte_identifier {
    ($name:ident, $documentation:literal) => {
        #[doc = $documentation]
        #[repr(transparent)]
        #[derive(
            Clone,
            Copy,
            Debug,
            Default,
            Deserialize,
            Eq,
            Hash,
            Ord,
            PartialEq,
            PartialOrd,
            Serialize,
        )]
        #[serde(transparent)]
        pub struct $name([u8; 16]);

        impl $name {
            /// Creates an identifier from stable network-order bytes.
            #[must_use]
            pub const fn from_bytes(bytes: [u8; 16]) -> Self {
                Self(bytes)
            }

            /// Creates an identifier from an integer for deterministic fixtures.
            #[must_use]
            pub const fn from_u128(value: u128) -> Self {
                Self(value.to_be_bytes())
            }

            /// Returns the stable network-order bytes.
            #[must_use]
            pub const fn as_bytes(&self) -> &[u8; 16] {
                &self.0
            }
        }
    };
}

byte_identifier!(
    TransactionId,
    "Idempotency identifier scoped to one authoritative world."
);
byte_identifier!(
    PersistentEntityId,
    "World-scoped stable identity of a persistent spatial entity."
);
byte_identifier!(
    ContinuationId,
    "Stable identity of deterministic continuation work within a chunk."
);

/// Canonical right-handed Y-up chunk coordinate in `(x, y, z)` order.
#[derive(
    Clone, Copy, Debug, Default, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize,
)]
pub struct ChunkCoordinate {
    /// Horizontal chunk coordinate increasing to world right.
    pub x: i32,
    /// Vertical chunk coordinate increasing upward.
    pub y: i32,
    /// Horizontal chunk coordinate on the world depth axis.
    pub z: i32,
}

impl ChunkCoordinate {
    /// Creates a coordinate in canonical `(x, y, z)` order.
    #[must_use]
    pub const fn new(x: i32, y: i32, z: i32) -> Self {
        Self { x, y, z }
    }
}

/// Stable dimension registration identity.
#[repr(transparent)]
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct DimensionId(StableId);

impl DimensionId {
    /// Validates a stable ID as a dimension identity.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError::InvalidDimensionId`] when the registration kind
    /// is not `dimension`.
    pub fn new(value: StableId) -> StorageResult<Self> {
        if value.kind() != "dimension" {
            return Err(StorageError::InvalidDimensionId {
                value: value.to_string(),
                reason: "registration kind must be `dimension`".to_owned(),
            });
        }
        Ok(Self(value))
    }

    /// Returns the underlying stable registration ID.
    #[must_use]
    pub const fn as_stable_id(&self) -> &StableId {
        &self.0
    }

    /// Returns the canonical dimension identity text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

impl fmt::Display for DimensionId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl FromStr for DimensionId {
    type Err = StorageError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let stable_id =
            value
                .parse::<StableId>()
                .map_err(|error| StorageError::InvalidDimensionId {
                    value: value.to_owned(),
                    reason: error.to_string(),
                })?;
        Self::new(stable_id)
    }
}

impl Serialize for DimensionId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for DimensionId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        value.parse().map_err(de::Error::custom)
    }
}

/// Complete persistent key of one chunk.
#[derive(Clone, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
pub struct ChunkKey {
    /// World containing the chunk.
    pub world: WorldId,
    /// Dimension containing the chunk.
    pub dimension: DimensionId,
    /// Canonical `(x, y, z)` coordinate.
    pub coordinate: ChunkCoordinate,
}

impl ChunkKey {
    /// Creates a complete authoritative chunk key.
    #[must_use]
    pub const fn new(world: WorldId, dimension: DimensionId, coordinate: ChunkCoordinate) -> Self {
        Self {
            world,
            dimension,
            coordinate,
        }
    }
}

/// Positive owner-controlled schema version of one opaque payload.
#[repr(transparent)]
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct PayloadSchemaVersion(u32);

impl PayloadSchemaVersion {
    /// Creates a positive schema version.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError::InvalidSchemaVersion`] for reserved version
    /// zero.
    pub const fn new(value: u32) -> StorageResult<Self> {
        if value == 0 {
            Err(StorageError::InvalidSchemaVersion)
        } else {
            Ok(Self(value))
        }
    }

    /// Returns the persistent integer value.
    #[must_use]
    pub const fn get(self) -> u32 {
        self.0
    }
}

impl Serialize for PayloadSchemaVersion {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_u32(self.0)
    }
}

impl<'de> Deserialize<'de> for PayloadSchemaVersion {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = u32::deserialize(deserializer)?;
        Self::new(value).map_err(de::Error::custom)
    }
}

/// Canonical bytes owned by an explicit persistent schema.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct VersionedPayload {
    pub(crate) schema: SchemaId,
    pub(crate) schema_version: PayloadSchemaVersion,
    pub(crate) bytes: Vec<u8>,
}

impl VersionedPayload {
    /// Creates an opaque authoritative payload.
    #[must_use]
    pub const fn new(
        schema: SchemaId,
        schema_version: PayloadSchemaVersion,
        bytes: Vec<u8>,
    ) -> Self {
        Self {
            schema,
            schema_version,
            bytes,
        }
    }

    /// Returns the owning schema ID.
    #[must_use]
    pub const fn schema(&self) -> &SchemaId {
        &self.schema
    }

    /// Returns the owner-controlled schema version.
    #[must_use]
    pub const fn schema_version(&self) -> PayloadSchemaVersion {
        self.schema_version
    }

    /// Returns the exact canonical payload bytes.
    #[must_use]
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    fn measure_encoded_len(&self, meter: &mut PayloadByteMeter) -> StorageResult<()> {
        #[cfg(test)]
        record_payload_measure_visit();
        meter.add(8, "payload schema length")?;
        meter.add(
            usize_to_u64(self.schema.as_str().len(), "payload schema ID")?,
            "payload schema ID",
        )?;
        meter.add(4, "payload schema version")?;
        meter.add(8, "versioned payload length")?;
        meter.add(
            usize_to_u64(self.bytes.len(), "versioned payload")?,
            "versioned payload",
        )
    }
}

/// Complete authoritative data of one materialized chunk.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ChunkData {
    pub(crate) voxels: VersionedPayload,
    pub(crate) persistent_entities: BTreeMap<PersistentEntityId, VersionedPayload>,
    pub(crate) continuations: BTreeMap<ContinuationId, VersionedPayload>,
    pub(crate) provenance: BTreeMap<StableId, CanonicalHash>,
}

impl ChunkData {
    /// Creates a complete authoritative chunk replacement.
    #[must_use]
    pub const fn new(
        voxels: VersionedPayload,
        persistent_entities: BTreeMap<PersistentEntityId, VersionedPayload>,
        continuations: BTreeMap<ContinuationId, VersionedPayload>,
        provenance: BTreeMap<StableId, CanonicalHash>,
    ) -> Self {
        Self {
            voxels,
            persistent_entities,
            continuations,
            provenance,
        }
    }

    /// Returns the canonical voxel payload.
    #[must_use]
    pub const fn voxels(&self) -> &VersionedPayload {
        &self.voxels
    }

    /// Returns persistent entity payloads in stable entity-ID order.
    #[must_use]
    pub const fn persistent_entities(&self) -> &BTreeMap<PersistentEntityId, VersionedPayload> {
        &self.persistent_entities
    }

    /// Returns deterministic continuation payloads in stable ID order.
    #[must_use]
    pub const fn continuations(&self) -> &BTreeMap<ContinuationId, VersionedPayload> {
        &self.continuations
    }

    /// Returns generation and semantic provenance in canonical ID order.
    #[must_use]
    pub const fn provenance(&self) -> &BTreeMap<StableId, CanonicalHash> {
        &self.provenance
    }

    #[cfg(test)]
    pub(crate) fn encoded_len(&self) -> StorageResult<u64> {
        let mut meter = PayloadByteMeter::new(u64::MAX);
        self.measure_encoded_len(&mut meter)?;
        Ok(meter.measured())
    }

    pub(crate) fn measure_encoded_len(&self, meter: &mut PayloadByteMeter) -> StorageResult<()> {
        self.voxels.measure_encoded_len(meter)?;

        let _ = usize_to_u64(self.persistent_entities.len(), "persistent entity count")?;
        meter.add(8, "persistent entity count")?;
        for payload in self.persistent_entities.values() {
            meter.add(16, "persistent entity ID")?;
            payload.measure_encoded_len(meter)?;
        }

        let _ = usize_to_u64(self.continuations.len(), "continuation count")?;
        meter.add(8, "continuation count")?;
        for payload in self.continuations.values() {
            meter.add(16, "continuation ID")?;
            payload.measure_encoded_len(meter)?;
        }

        let _ = usize_to_u64(self.provenance.len(), "provenance count")?;
        meter.add(8, "provenance count")?;
        for key in self.provenance.keys() {
            meter.add(8, "provenance key length")?;
            meter.add(
                usize_to_u64(key.as_str().len(), "provenance key")?,
                "provenance key",
            )?;
            meter.add(32, "provenance digest")?;
        }
        Ok(())
    }
}

/// Bit mask naming authoritative domains changed by a chunk replacement.
#[repr(transparent)]
#[derive(Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ChangedDomains(u8);

impl ChangedDomains {
    /// No domain changed.
    pub const NONE: Self = Self(0);
    /// Canonical voxel payload changed.
    pub const VOXELS: Self = Self(1 << 0);
    /// Persistent entity set or payload changed.
    pub const PERSISTENT_ENTITIES: Self = Self(1 << 1);
    /// Deterministic continuation set or payload changed.
    pub const CONTINUATIONS: Self = Self(1 << 2);
    /// Generation or semantic provenance changed.
    pub const PROVENANCE: Self = Self(1 << 3);
    /// Every authoritative domain changed.
    pub const ALL: Self = Self(
        Self::VOXELS.0 | Self::PERSISTENT_ENTITIES.0 | Self::CONTINUATIONS.0 | Self::PROVENANCE.0,
    );

    /// Validates a raw changed-domain mask.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError::InvalidDomainMask`] when unknown bits are set.
    pub const fn from_bits(bits: u8) -> StorageResult<Self> {
        if bits & !Self::ALL.0 == 0 {
            Ok(Self(bits))
        } else {
            Err(StorageError::InvalidDomainMask { bits })
        }
    }

    /// Returns the stable raw mask.
    #[must_use]
    pub const fn bits(self) -> u8 {
        self.0
    }

    /// Returns whether no authoritative domain changed.
    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// Returns whether every bit in `other` is present.
    #[must_use]
    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    /// Returns the union of two valid domain masks.
    #[must_use]
    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }
}

impl Serialize for ChangedDomains {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_u8(self.0)
    }
}

impl<'de> Deserialize<'de> for ChangedDomains {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let bits = u8::deserialize(deserializer)?;
        Self::from_bits(bits).map_err(de::Error::custom)
    }
}

/// Per-domain revisions used only for derived-work invalidation.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct DomainRevisions {
    pub(crate) voxels: VoxelRevision,
    pub(crate) persistent_entities: PersistentEntityRevision,
    pub(crate) continuation: ContinuationRevision,
}

impl DomainRevisions {
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

    /// Returns the continuation-domain revision.
    #[must_use]
    pub const fn continuation(self) -> ContinuationRevision {
        self.continuation
    }
}

/// Fully reconstructed authoritative chunk returned by a read snapshot.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct StoredChunk {
    pub(crate) key: ChunkKey,
    pub(crate) captured_world_revision: WorldRevision,
    pub(crate) revision: ChunkRevision,
    pub(crate) domain_revisions: DomainRevisions,
    pub(crate) data: ChunkData,
}

impl StoredChunk {
    /// Returns the chunk key.
    #[must_use]
    pub const fn key(&self) -> &ChunkKey {
        &self.key
    }

    /// Returns the world revision that atomically published this state.
    #[must_use]
    pub const fn captured_world_revision(&self) -> WorldRevision {
        self.captured_world_revision
    }

    /// Returns the total authoritative chunk revision.
    #[must_use]
    pub const fn revision(&self) -> ChunkRevision {
        self.revision
    }

    /// Returns revisions used to reject stale derived work.
    #[must_use]
    pub const fn domain_revisions(&self) -> DomainRevisions {
        self.domain_revisions
    }

    /// Returns complete authoritative chunk data.
    #[must_use]
    pub const fn data(&self) -> &ChunkData {
        &self.data
    }
}

/// Optimistic revision condition for one chunk replacement.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum ChunkRevisionExpectation {
    /// The chunk must not have been materialized.
    Absent,
    /// The chunk must have exactly this total authoritative revision.
    Exact(ChunkRevision),
}

/// Complete replacement of one chunk inside an atomic world transaction.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ChunkMutation {
    pub(crate) key: ChunkKey,
    pub(crate) expected_revision: ChunkRevisionExpectation,
    pub(crate) changed_domains: ChangedDomains,
    pub(crate) data: ChunkData,
}

impl ChunkMutation {
    /// Creates a complete chunk replacement and its optimistic precondition.
    #[must_use]
    pub const fn new(
        key: ChunkKey,
        expected_revision: ChunkRevisionExpectation,
        changed_domains: ChangedDomains,
        data: ChunkData,
    ) -> Self {
        Self {
            key,
            expected_revision,
            changed_domains,
            data,
        }
    }

    /// Returns the chunk being replaced.
    #[must_use]
    pub const fn key(&self) -> &ChunkKey {
        &self.key
    }

    /// Returns the optimistic chunk precondition.
    #[must_use]
    pub const fn expected_revision(&self) -> ChunkRevisionExpectation {
        self.expected_revision
    }

    /// Returns the caller-declared changed domains.
    #[must_use]
    pub const fn changed_domains(&self) -> ChangedDomains {
        self.changed_domains
    }

    /// Returns the complete replacement data.
    #[must_use]
    pub const fn data(&self) -> &ChunkData {
        &self.data
    }
}

/// Atomic authoritative transaction sharing one new [`WorldRevision`].
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct WorldTransaction {
    pub(crate) id: TransactionId,
    pub(crate) world: WorldId,
    pub(crate) base_world_revision: WorldRevision,
    pub(crate) mutations: Vec<ChunkMutation>,
}

impl WorldTransaction {
    /// Creates an optimistic multi-chunk transaction.
    #[must_use]
    pub const fn new(
        id: TransactionId,
        world: WorldId,
        base_world_revision: WorldRevision,
        mutations: Vec<ChunkMutation>,
    ) -> Self {
        Self {
            id,
            world,
            base_world_revision,
            mutations,
        }
    }

    /// Returns the idempotency identifier.
    #[must_use]
    pub const fn id(&self) -> TransactionId {
        self.id
    }

    /// Returns the authoritative world being changed.
    #[must_use]
    pub const fn world(&self) -> WorldId {
        self.world
    }

    /// Returns the caller's immutable read-snapshot revision.
    #[must_use]
    pub const fn base_world_revision(&self) -> WorldRevision {
        self.base_world_revision
    }

    /// Returns chunk replacements in caller order.
    ///
    /// The kernel enforces limits before canonicalizing this order for validation and
    /// publication.
    #[must_use]
    pub fn mutations(&self) -> &[ChunkMutation] {
        &self.mutations
    }
}

/// Non-durable acknowledgement state of the in-memory reference kernel.
///
/// This single-state type deliberately does not define the production
/// durability ladder owned by the future D3 persistence boundary.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum ReferenceDurability {
    /// Accepted only by the non-durable in-memory reference kernel.
    Queued,
}

/// Revision details for one chunk published by a transaction.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ChunkCommitReceipt {
    pub(crate) key: ChunkKey,
    pub(crate) chunk_revision: ChunkRevision,
    pub(crate) domain_revisions: DomainRevisions,
    pub(crate) changed_domains: ChangedDomains,
}

impl ChunkCommitReceipt {
    /// Returns the committed chunk key.
    #[must_use]
    pub const fn key(&self) -> &ChunkKey {
        &self.key
    }

    /// Returns the new total chunk revision.
    #[must_use]
    pub const fn chunk_revision(&self) -> ChunkRevision {
        self.chunk_revision
    }

    /// Returns the post-commit domain revisions.
    #[must_use]
    pub const fn domain_revisions(&self) -> DomainRevisions {
        self.domain_revisions
    }

    /// Returns domains actually changed by the replacement.
    #[must_use]
    pub const fn changed_domains(&self) -> ChangedDomains {
        self.changed_domains
    }
}

/// Acknowledgement returned only after a complete atomic publication.
///
/// This receipt deliberately omits the full-world reference hash. It is the
/// scale-sensitive acknowledgement used when callers only need publication
/// identity, revisions, changed domains, and the non-durable acknowledgement
/// level. [`CommitReceipt`] remains the stronger reference-evidence form.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct PublicationReceipt {
    pub(crate) transaction_id: TransactionId,
    pub(crate) world: WorldId,
    pub(crate) world_revision: WorldRevision,
    pub(crate) chunks: Vec<ChunkCommitReceipt>,
    pub(crate) durability: ReferenceDurability,
    pub(crate) replayed: bool,
}

impl PublicationReceipt {
    /// Returns the transaction idempotency identifier.
    #[must_use]
    pub const fn transaction_id(&self) -> TransactionId {
        self.transaction_id
    }

    /// Returns the authoritative world.
    #[must_use]
    pub const fn world(&self) -> WorldId {
        self.world
    }

    /// Returns the world revision shared by all changed chunks.
    #[must_use]
    pub const fn world_revision(&self) -> WorldRevision {
        self.world_revision
    }

    /// Returns committed chunks in canonical key order.
    #[must_use]
    pub fn chunks(&self) -> &[ChunkCommitReceipt] {
        &self.chunks
    }

    /// Returns the non-durable acknowledgement state of this publication.
    #[must_use]
    pub const fn durability(&self) -> ReferenceDurability {
        self.durability
    }

    /// Returns whether this receipt came from an exact idempotent replay.
    #[must_use]
    pub const fn replayed(&self) -> bool {
        self.replayed
    }
}

/// Acknowledgement returned only after a complete atomic publication and
/// full-world reference hashing.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CommitReceipt {
    pub(crate) transaction_id: TransactionId,
    pub(crate) world: WorldId,
    pub(crate) world_revision: WorldRevision,
    pub(crate) chunks: Vec<ChunkCommitReceipt>,
    pub(crate) durability: ReferenceDurability,
    pub(crate) materialized_chunk_state_hash: MaterializedChunkStateHash,
    pub(crate) replayed: bool,
}

impl CommitReceipt {
    /// Projects this reference receipt into its scale-sensitive publication
    /// evidence without recomputing or exposing the full-world state hash.
    #[must_use]
    pub fn publication(&self) -> PublicationReceipt {
        PublicationReceipt {
            transaction_id: self.transaction_id,
            world: self.world,
            world_revision: self.world_revision,
            chunks: self.chunks.clone(),
            durability: self.durability,
            replayed: self.replayed,
        }
    }

    /// Returns the transaction idempotency identifier.
    #[must_use]
    pub const fn transaction_id(&self) -> TransactionId {
        self.transaction_id
    }

    /// Returns the authoritative world.
    #[must_use]
    pub const fn world(&self) -> WorldId {
        self.world
    }

    /// Returns the world revision shared by all changed chunks.
    #[must_use]
    pub const fn world_revision(&self) -> WorldRevision {
        self.world_revision
    }

    /// Returns committed chunks in canonical key order.
    #[must_use]
    pub fn chunks(&self) -> &[ChunkCommitReceipt] {
        &self.chunks
    }

    /// Returns the non-durable acknowledgement state of the reference commit.
    #[must_use]
    pub const fn durability(&self) -> ReferenceDurability {
        self.durability
    }

    /// Returns the post-commit materialized-chunk state hash.
    #[must_use]
    pub const fn materialized_chunk_state_hash(&self) -> MaterializedChunkStateHash {
        self.materialized_chunk_state_hash
    }

    /// Returns whether this receipt came from an exact idempotent replay.
    #[must_use]
    pub const fn replayed(&self) -> bool {
        self.replayed
    }
}

/// Deterministic one-shot failure phase exposed by the memory backend.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum FaultPoint {
    /// Fail after all natural validation and optimistic checks.
    AfterValidation,
    /// Fail after applying the first mutation only to private staging state.
    AfterFirstStagedMutation,
    /// Fail after the complete transaction and receipt exist only in staging.
    AfterStagingBeforePublish,
    /// Publish the complete transaction, then lose the acknowledgement.
    AfterPublishBeforeReceipt,
}

/// Reference-kernel ceilings enforced before staging authoritative state.
///
/// These values are implementation safety bounds, not ADR 0027 production
/// storage budgets. D3 evidence must define the future product boundary.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TransactionKernelLimits {
    chunk_count_ceiling: u32,
    payload_byte_ceiling: u64,
    receipt_horizon: u32,
}

impl TransactionKernelLimits {
    /// Non-normative safety values for the in-memory reference kernel.
    pub const REFERENCE_DEFAULT: Self = Self {
        chunk_count_ceiling: 32,
        payload_byte_ceiling: 64 * 1024 * 1024,
        receipt_horizon: 64,
    };

    /// Creates non-zero reference-kernel ceilings.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError::InvalidLimit`] when any ceiling is zero.
    pub const fn new(
        chunk_count_ceiling: u32,
        payload_byte_ceiling: u64,
        receipt_horizon: u32,
    ) -> StorageResult<Self> {
        if chunk_count_ceiling == 0 {
            return Err(StorageError::InvalidLimit {
                name: "max_chunks_per_transaction",
                value: 0,
            });
        }
        if payload_byte_ceiling == 0 {
            return Err(StorageError::InvalidLimit {
                name: "max_payload_bytes_per_transaction",
                value: 0,
            });
        }
        if receipt_horizon == 0 {
            return Err(StorageError::InvalidLimit {
                name: "max_retained_receipts_per_world",
                value: 0,
            });
        }
        Ok(Self {
            chunk_count_ceiling,
            payload_byte_ceiling,
            receipt_horizon,
        })
    }

    /// Returns the maximum chunk replacements in one transaction.
    #[must_use]
    pub const fn max_chunks_per_transaction(self) -> u32 {
        self.chunk_count_ceiling
    }

    /// Returns the maximum canonical uncompressed payload bytes.
    #[must_use]
    pub const fn max_payload_bytes_per_transaction(self) -> u64 {
        self.payload_byte_ceiling
    }

    /// Returns the bounded exact-replay receipt horizon per world.
    #[must_use]
    pub const fn max_retained_receipts_per_world(self) -> u32 {
        self.receipt_horizon
    }
}

impl Default for TransactionKernelLimits {
    fn default() -> Self {
        Self::REFERENCE_DEFAULT
    }
}
/// Immutable full-world view used only as reference-kernel test evidence.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReferenceWorldSnapshot {
    pub(crate) world: WorldId,
    pub(crate) revision: WorldRevision,
    pub(crate) chunks: BTreeMap<ChunkKey, StoredChunk>,
    pub(crate) entity_locations: BTreeMap<PersistentEntityId, ChunkKey>,
    pub(crate) materialized_chunk_state_hash: MaterializedChunkStateHash,
}

impl ReferenceWorldSnapshot {
    /// Returns the captured world identity.
    #[must_use]
    pub const fn world(&self) -> WorldId {
        self.world
    }

    /// Returns the captured contiguous authoritative revision.
    #[must_use]
    pub const fn revision(&self) -> WorldRevision {
        self.revision
    }

    /// Loads a chunk from this immutable read view.
    #[must_use]
    pub fn chunk(&self, key: &ChunkKey) -> Option<&StoredChunk> {
        self.chunks.get(key)
    }

    /// Iterates chunks in canonical world/dimension/`(x, y, z)` order.
    #[must_use]
    pub fn chunks(&self) -> impl ExactSizeIterator<Item = (&ChunkKey, &StoredChunk)> {
        self.chunks.iter()
    }

    /// Locates a world-scoped persistent entity in the derived index.
    #[must_use]
    pub fn entity_chunk(&self, entity: PersistentEntityId) -> Option<&ChunkKey> {
        self.entity_locations.get(&entity)
    }

    /// Returns the number of indexed persistent entities.
    #[must_use]
    pub fn entity_count(&self) -> usize {
        self.entity_locations.len()
    }
    /// Returns the number of materialized chunks.
    #[must_use]
    pub fn len(&self) -> usize {
        self.chunks.len()
    }

    /// Returns whether the world has no materialized chunks.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.chunks.is_empty()
    }

    /// Returns the deterministic materialized-chunk state hash.
    #[must_use]
    pub const fn materialized_chunk_state_hash(&self) -> MaterializedChunkStateHash {
        self.materialized_chunk_state_hash
    }
}

pub(crate) struct PayloadByteMeter {
    measured: u64,
    ceiling: u64,
}

impl PayloadByteMeter {
    pub(crate) const fn new(ceiling: u64) -> Self {
        Self {
            measured: 0,
            ceiling,
        }
    }

    #[cfg(test)]
    pub(crate) const fn measured(&self) -> u64 {
        self.measured
    }

    fn add(&mut self, bytes: u64, what: &'static str) -> StorageResult<()> {
        let minimum_payload_bytes = self
            .measured
            .checked_add(bytes)
            .ok_or(StorageError::PayloadSizeOverflow { what })?;
        if minimum_payload_bytes > self.ceiling {
            return Err(StorageError::TransactionPayloadLimitExceeded {
                minimum_payload_bytes,
                max_payload_bytes: self.ceiling,
            });
        }
        self.measured = minimum_payload_bytes;
        Ok(())
    }
}

fn usize_to_u64(value: usize, what: &'static str) -> StorageResult<u64> {
    u64::try_from(value).map_err(|_| StorageError::PayloadSizeOverflow { what })
}

#[cfg(test)]
std::thread_local! {
    static PAYLOAD_MEASURE_VISITS: Cell<usize> = const { Cell::new(0) };
}

#[cfg(test)]
fn record_payload_measure_visit() {
    PAYLOAD_MEASURE_VISITS.set(PAYLOAD_MEASURE_VISITS.get() + 1);
}

#[cfg(test)]
pub(crate) fn reset_payload_measure_visits() {
    PAYLOAD_MEASURE_VISITS.set(0);
}

#[cfg(test)]
pub(crate) fn payload_measure_visits() -> usize {
    PAYLOAD_MEASURE_VISITS.get()
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dimension_id_rejects_other_registration_kinds() {
        assert!("terrenia:dimension/terrenia".parse::<DimensionId>().is_ok());
        assert!("terrenia:block/stone".parse::<DimensionId>().is_err());
    }

    #[test]
    fn changed_domain_deserialization_rejects_unknown_bits() {
        assert!(serde_json::from_str::<ChangedDomains>("15").is_ok());
        assert!(serde_json::from_str::<ChangedDomains>("16").is_err());
    }

    #[test]
    fn payload_schema_version_rejects_zero_during_deserialization() {
        assert!(serde_json::from_str::<PayloadSchemaVersion>("1").is_ok());
        assert!(serde_json::from_str::<PayloadSchemaVersion>("0").is_err());
    }
}
