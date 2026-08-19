//! Backend-independent persistent world data transfer objects.

use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use latticeaxiom_core::ChunkPos;
use serde::{Deserialize, Serialize};

use crate::IdentifierError;

const MAX_IDENTIFIER_BYTES: usize = 1_024;

macro_rules! textual_identifier {
    ($name:ident, $description:literal) => {
        #[doc = $description]
        #[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
        pub struct $name(String);

        impl $name {
            /// Creates a validated persistent identifier.
            ///
            /// # Errors
            ///
            /// Returns [`IdentifierError`] when `value` is empty or too long
            /// for a persistent key.
            pub fn new(value: impl Into<String>) -> Result<Self, IdentifierError> {
                let value = value.into();
                validate_identifier(&value)?;
                Ok(Self(value))
            }

            /// Returns the identifier text.
            #[must_use]
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }
    };
}

textual_identifier!(OwnerId, "Stable owner of a persistent payload schema.");
textual_identifier!(
    ImplementationId,
    "Exact package/capability implementation recorded in generation provenance."
);
textual_identifier!(ReceiptDomain, "Stable domain of an artifact receipt.");
textual_identifier!(WorkKind, "Stable kind of deterministic continuation work.");
textual_identifier!(
    BoundaryContractId,
    "Boundary contract used when a chunk was generated."
);

fn validate_identifier(value: &str) -> Result<(), IdentifierError> {
    if value.is_empty() {
        return Err(IdentifierError::Empty);
    }
    if value.len() > MAX_IDENTIFIER_BYTES {
        return Err(IdentifierError::TooLong {
            length: value.len(),
            maximum: MAX_IDENTIFIER_BYTES,
        });
    }
    Ok(())
}

/// Stable identifier of a world.
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
pub struct WorldId(u128);

impl WorldId {
    /// Creates a world identifier from its persistent integer representation.
    #[must_use]
    pub const fn from_u128(value: u128) -> Self {
        Self(value)
    }

    /// Returns the persistent integer representation.
    #[must_use]
    pub const fn to_u128(self) -> u128 {
        self.0
    }
}

/// Stable identifier of a dimension inside a world.
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
pub struct DimensionId(u32);

impl DimensionId {
    /// Creates a dimension identifier from its persistent integer representation.
    #[must_use]
    pub const fn from_u32(value: u32) -> Self {
        Self(value)
    }

    /// Returns the persistent integer representation.
    #[must_use]
    pub const fn to_u32(self) -> u32 {
        self.0
    }
}

/// Idempotency identifier supplied with one immutable chunk commit.
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
pub struct CommitId(u128);

impl CommitId {
    /// Creates a commit identifier.
    #[must_use]
    pub const fn from_u128(value: u128) -> Self {
        Self(value)
    }

    /// Returns the persistent integer representation.
    #[must_use]
    pub const fn to_u128(self) -> u128 {
        self.0
    }
}

/// Stable identifier of a spatial entity.
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
pub struct EntityId(u128);

impl EntityId {
    /// Creates a spatial entity identifier.
    #[must_use]
    pub const fn from_u128(value: u128) -> Self {
        Self(value)
    }

    /// Returns the persistent integer representation.
    #[must_use]
    pub const fn to_u128(self) -> u128 {
        self.0
    }
}

/// Stable identifier of a generated artifact within its domain.
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
pub struct ArtifactId(u128);

impl ArtifactId {
    /// Creates an artifact identifier.
    #[must_use]
    pub const fn from_u128(value: u128) -> Self {
        Self(value)
    }

    /// Returns the persistent integer representation.
    #[must_use]
    pub const fn to_u128(self) -> u128 {
        self.0
    }
}

/// Stable identifier returned for a completed checkpoint.
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
pub struct CheckpointId(u128);

impl CheckpointId {
    /// Creates a checkpoint identifier.
    #[must_use]
    pub const fn from_u128(value: u128) -> Self {
        Self(value)
    }

    /// Returns the persistent integer representation.
    #[must_use]
    pub const fn to_u128(self) -> u128 {
        self.0
    }
}

/// Monotonic generation epoch recorded for a materialized chunk.
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
pub struct GenerationEpoch(u64);

impl GenerationEpoch {
    /// Creates a generation epoch.
    #[must_use]
    pub const fn from_u64(value: u64) -> Self {
        Self(value)
    }

    /// Returns the persistent integer representation.
    #[must_use]
    pub const fn to_u64(self) -> u64 {
        self.0
    }
}

/// Fixed-width content digest used by provenance and receipts.
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
pub struct ContentHash([u8; 32]);

impl ContentHash {
    /// Digest containing only zero bytes, useful for explicitly unknown input.
    pub const ZERO: Self = Self([0; 32]);

    /// Creates a content digest from its bytes.
    #[must_use]
    pub const fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// Returns the digest bytes.
    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

/// Version of an engine-owned or package-owned payload schema.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct SchemaVersion {
    /// Breaking schema generation.
    pub major: u16,
    /// Backward-compatible schema revision.
    pub minor: u16,
}

impl SchemaVersion {
    /// Creates a schema version.
    #[must_use]
    pub const fn new(major: u16, minor: u16) -> Self {
        Self { major, minor }
    }
}

/// Full persistent key of one chunk.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ChunkKey {
    /// World containing the chunk.
    pub world: WorldId,
    /// Dimension containing the chunk.
    pub dimension: DimensionId,
    /// Chunk position in canonical `(x, y, z)` order.
    #[serde(with = "chunk_pos_serde")]
    pub position: ChunkPos,
}

impl ChunkKey {
    /// Creates a chunk key.
    #[must_use]
    pub const fn new(world: WorldId, dimension: DimensionId, position: ChunkPos) -> Self {
        Self {
            world,
            dimension,
            position,
        }
    }
}

impl PartialOrd for ChunkKey {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for ChunkKey {
    fn cmp(&self, other: &Self) -> Ordering {
        (
            self.world,
            self.dimension,
            self.position.x,
            self.position.y,
            self.position.z,
        )
            .cmp(&(
                other.world,
                other.dimension,
                other.position.x,
                other.position.y,
                other.position.z,
            ))
    }
}

/// Generation inputs needed to explain or explicitly regenerate a snapshot.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GenerationProvenance {
    /// Creation timestamp in Unix milliseconds.
    pub created_at_unix_millis: u64,
    /// Generation-lock epoch used for this chunk.
    pub epoch: GenerationEpoch,
    /// Digest of the canonical generation configuration.
    pub configuration_hash: ContentHash,
    /// Exact package/capability implementations and their content digests.
    pub implementations: BTreeMap<ImplementationId, ContentHash>,
    /// Digest of the upstream generation plan.
    pub upstream_plan_hash: ContentHash,
    /// Boundary contract used for neighboring inputs and outputs.
    pub boundary_contract: BoundaryContractId,
}

/// Complete materialized baseline and current block state of one chunk.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChunkSnapshot {
    /// Schema of the opaque snapshot payload.
    pub schema: SchemaVersion,
    /// Generation provenance retained independently of generator availability.
    pub provenance: GenerationProvenance,
    /// Engine-defined snapshot bytes; derived meshes and caches never belong here.
    pub payload: Vec<u8>,
}

/// Package-owned persistent payload with an explicit owner schema.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OwnedPayload {
    /// Package or engine subsystem that owns the schema.
    pub owner: OwnerId,
    /// Owner-controlled schema version.
    pub schema: SchemaVersion,
    /// Opaque bytes preserved even when the owner is unavailable.
    pub payload: Vec<u8>,
}

/// Compound key of one provenance receipt.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct ArtifactKey {
    /// Receipt domain.
    pub domain: ReceiptDomain,
    /// Artifact identifier within the domain.
    pub artifact: ArtifactId,
}

impl ArtifactKey {
    /// Creates an artifact key.
    #[must_use]
    pub const fn new(domain: ReceiptDomain, artifact: ArtifactId) -> Self {
        Self { domain, artifact }
    }
}

/// Provenance receipt for an artifact produced while materializing a chunk.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArtifactReceipt {
    /// Exact implementation that produced the artifact.
    pub producer: ImplementationId,
    /// Digest of canonical producer configuration.
    pub configuration_hash: ContentHash,
    /// Digest of the complete producer inputs.
    pub input_hash: ContentHash,
    /// Digest of the resulting artifact bytes or semantic value.
    pub content_hash: ContentHash,
}

/// Fully reconstructed authoritative chunk returned by storage.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredChunk {
    /// Persistent key of the chunk.
    pub key: ChunkKey,
    /// Monotonic storage revision, beginning at one.
    pub revision: u64,
    /// Materialized baseline and current block state.
    pub snapshot: ChunkSnapshot,
    /// Persistent spatial entities keyed by stable numeric identifier.
    pub spatial_entities: BTreeMap<EntityId, OwnedPayload>,
    /// Deterministic work required to resume simulation after load.
    pub continuation: BTreeMap<WorkKind, OwnedPayload>,
    /// Artifacts produced for this chunk and recorded atomically with it.
    pub artifact_receipts: BTreeMap<ArtifactKey, ArtifactReceipt>,
}

/// Optimistic condition evaluated immediately before an atomic commit.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum CommitCondition {
    /// Accept any current revision, including an absent chunk.
    Any,
    /// Accept only a chunk that has never been materialized.
    IfAbsent,
    /// Accept only the exact current revision.
    IfRevision(u64),
}

/// Durability acknowledgement requested for a commit.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Durability {
    /// Acknowledge after the write is present in the write-ahead log.
    Wal,
    /// Acknowledge only after requesting a synchronous durable write.
    Sync,
}

/// Immutable replacement for every persistent category of one chunk.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChunkCommit {
    /// Idempotency identifier scoped to [`ChunkKey::world`].
    pub id: CommitId,
    /// Persistent key being created or replaced.
    pub key: ChunkKey,
    /// Revision precondition checked before any write is visible.
    pub condition: CommitCondition,
    /// Requested acknowledgement point.
    pub durability: Durability,
    /// Complete materialized baseline and current block state.
    pub snapshot: ChunkSnapshot,
    /// Complete replacement set of persistent spatial entities.
    pub spatial_entities: BTreeMap<EntityId, OwnedPayload>,
    /// Complete replacement set of deterministic continuation work.
    pub continuation: BTreeMap<WorkKind, OwnedPayload>,
    /// Complete replacement set of artifact receipts.
    pub artifact_receipts: BTreeMap<ArtifactKey, ArtifactReceipt>,
}

/// Acknowledgement returned only after the requested durability boundary.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommitReceipt {
    /// Idempotency identifier of the acknowledged commit.
    pub commit_id: CommitId,
    /// Chunk changed by the commit.
    pub key: ChunkKey,
    /// New authoritative revision.
    pub revision: u64,
    /// Durability mode used for the original write.
    pub durability: Durability,
    /// Whether this response came from an idempotent retry marker.
    pub replayed: bool,
}

/// Filter for deterministic artifact-receipt scans.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReceiptQuery {
    /// World whose receipts should be returned.
    pub world: WorldId,
    /// Optional exact receipt domain.
    pub domain: Option<ReceiptDomain>,
}

/// Caller-selected checkpoint identity and empty destination directory.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CheckpointTarget {
    id: CheckpointId,
    directory: PathBuf,
}

impl CheckpointTarget {
    /// Creates a checkpoint target.
    #[must_use]
    pub fn new(id: CheckpointId, directory: impl Into<PathBuf>) -> Self {
        Self {
            id,
            directory: directory.into(),
        }
    }

    /// Returns the checkpoint identifier.
    #[must_use]
    pub const fn id(&self) -> CheckpointId {
        self.id
    }

    /// Returns the destination directory.
    #[must_use]
    pub fn directory(&self) -> &Path {
        &self.directory
    }
}

mod chunk_pos_serde {
    use latticeaxiom_core::ChunkPos;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub(super) fn serialize<S>(position: &ChunkPos, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        (position.x, position.y, position.z).serialize(serializer)
    }

    pub(super) fn deserialize<'de, D>(deserializer: D) -> Result<ChunkPos, D::Error>
    where
        D: Deserializer<'de>,
    {
        let (x, y, z) = <(i32, i32, i32)>::deserialize(deserializer)?;
        Ok(ChunkPos::new(x, y, z))
    }
}
