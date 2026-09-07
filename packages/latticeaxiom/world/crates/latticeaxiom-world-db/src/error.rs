//! Typed failures for the D3 world-storage product boundary.

use latticeaxiom_core::WorldId;
use latticeaxiom_storage::{
    ChangedDomains, ChunkKey, ChunkRevision, ChunkRevisionExpectation, PersistentEntityId,
    TransactionId, WorldRevision,
};
use latticeaxiom_world_catalog::StoreId;
use latticeaxiom_world_wire::WorldWireError;
use thiserror::Error;

use crate::model::{CheckpointId, DigestV1, MetadataEpoch};

/// Result type shared by world-storage operations.
pub type WorldDbResult<T> = Result<T, WorldDbError>;

/// Typed rejection from a world-storage contract or backend operation.
///
/// Database publication and sidecar publication are intentionally separate.
/// Reference-backend failures reject the operation before publication.
/// [`Self::PhysicalStorage`] instead requires reopening and reconciling an
/// uncertain physical outcome; it must never be interpreted as rollback.
/// A database-ahead/header-
/// behind state is instead represented by the successful operation's header
/// status.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum WorldDbError {
    /// Physical I/O failed. The session must stop writes until explicit reopen
    /// and reconciliation; an ambiguous commit is never treated as rollback.
    #[error("physical storage requires reconciliation: {reason}")]
    PhysicalStorage {
        /// Database diagnostic retained for the recovery UI.
        reason: String,
    },
    /// A `SemVer` value used by authoritative metadata was malformed.
    #[error("invalid SemVer in {field} `{value}`: {reason}")]
    InvalidSemver {
        /// Stable metadata field name.
        field: &'static str,
        /// Rejected textual value.
        value: String,
        /// Parser diagnostic.
        reason: String,
    },
    /// An engine-coupled realization omitted the exact engine build identity.
    #[error("engine-coupled package `{package}` is missing its engine build ID")]
    MissingEngineBuildId {
        /// Package whose receipt was incomplete.
        package: String,
    },
    /// A frozen-lock package-map key disagreed with its embedded receipt.
    #[error("frozen-lock package key `{key}` disagrees with receipt package `{receipt}`")]
    PackageReceiptKeyMismatch {
        /// Canonical package-map key.
        key: String,
        /// Package identity carried by the receipt.
        receipt: String,
    },
    /// A package-requirement map key disagreed with its embedded requirement.
    #[error("package requirement key `{key}` disagrees with requirement package `{receipt}`")]
    PackageRequirementKeyMismatch {
        /// Canonical package-map key.
        key: String,
        /// Package identity carried by the requirement.
        receipt: String,
    },
    /// A schema-requirement map key disagreed with its embedded requirement.
    #[error("schema requirement key `{key}` disagrees with requirement schema `{receipt}`")]
    SchemaRequirementKeyMismatch {
        /// Canonical schema-map key.
        key: String,
        /// Schema identity carried by the requirement.
        receipt: String,
    },
    /// A persisted schema read range used version zero or was inverted.
    #[error("invalid inclusive schema version range {minimum}..={maximum}")]
    InvalidSchemaVersionRange {
        /// Requested minimum readable schema version.
        minimum: u32,
        /// Requested maximum readable schema version.
        maximum: u32,
    },
    /// A semantic contract used reserved major version zero.
    #[error("semantic contract `{contract}` uses reserved major version zero")]
    InvalidSemanticContractMajor {
        /// Semantic contract whose major was invalid.
        contract: String,
    },
    /// A monotonic persistent counter could not advance without wrapping.
    #[error("{counter} overflowed its persistent u64 range")]
    RevisionOverflow {
        /// Stable counter name.
        counter: &'static str,
    },
    /// A host length could not be represented by the bounded storage contract.
    #[error("{what} length cannot be represented by the world-storage contract")]
    LengthOverflow {
        /// Value whose length overflowed.
        what: &'static str,
    },
    /// Authoritative metadata exceeded a configured hard ceiling.
    #[error("{what} has {actual} entries or bytes, exceeding the hard ceiling {maximum}")]
    MetadataLimitExceeded {
        /// Metadata quantity that exceeded its ceiling.
        what: &'static str,
        /// Rejected entry or byte count.
        actual: u64,
        /// Active hard ceiling.
        maximum: u64,
    },
    /// Canonical JSON serialization of an internal schema failed.
    #[error("failed to encode {artifact} as canonical JSON: {reason}")]
    JsonEncode {
        /// Versioned artifact being encoded.
        artifact: &'static str,
        /// Serializer diagnostic.
        reason: String,
    },
    /// Canonical JSON deserialization of an internal schema failed.
    #[error("failed to decode {artifact} from canonical JSON: {reason}")]
    JsonDecode {
        /// Versioned artifact being decoded.
        artifact: &'static str,
        /// Deserializer diagnostic.
        reason: String,
    },
    /// Postcard serialization of a versioned internal DTO failed.
    #[error("failed to encode {artifact} as postcard: {reason}")]
    PostcardEncode {
        /// Versioned artifact being encoded.
        artifact: &'static str,
        /// Serializer diagnostic.
        reason: String,
    },
    /// Postcard deserialization of a versioned internal DTO failed.
    #[error("failed to decode {artifact} from postcard: {reason}")]
    PostcardDecode {
        /// Versioned artifact being decoded.
        artifact: &'static str,
        /// Deserializer diagnostic.
        reason: String,
    },
    /// Portable key or snapshot validation failed.
    #[error("portable world-wire validation failed: {source}")]
    WorldWire {
        /// Underlying portable-wire rejection.
        #[from]
        source: WorldWireError,
    },
    /// A world store already exists for the supplied identity.
    #[error("world {world} already has a provisioned store")]
    WorldAlreadyExists {
        /// Duplicate world identity.
        world: WorldId,
    },
    /// No authoritative database metadata exists for a world.
    #[error("world {world} is not provisioned in this store")]
    WorldNotFound {
        /// Missing world identity.
        world: WorldId,
    },
    /// A store generation did not match authoritative database metadata.
    #[error("world {world} expected store generation {expected:?}, but found {actual:?}")]
    StoreIdentityMismatch {
        /// World whose store identity was checked.
        world: WorldId,
        /// Store identity retained by authoritative metadata.
        expected: StoreId,
        /// Store identity supplied by another artifact or permit.
        actual: StoreId,
    },
    /// A mandatory-empty physical column family contained unexpected data.
    #[error("column family `{family}` must be empty, but contains {entries} entries")]
    NonEmptyReservedColumnFamily {
        /// Stable physical column-family name.
        family: &'static str,
        /// Number of unexpected entries observed by bounded inspection.
        entries: u64,
    },
    /// Authoritative metadata bytes were missing or internally inconsistent.
    #[error("authoritative metadata for world {world} is corrupt in {section}: {reason}")]
    CorruptMetadata {
        /// World whose metadata could not be trusted.
        world: WorldId,
        /// Stable metadata section name.
        section: &'static str,
        /// Validation diagnostic.
        reason: String,
    },
    /// A chunk record disagreed with its key, envelope, or authoritative index.
    #[error("chunk record {key:?} is corrupt: {reason}")]
    CorruptChunkRecord {
        /// Key under which the record was found.
        key: Box<ChunkKey>,
        /// Validation diagnostic.
        reason: String,
    },
    /// A writer activation permit was not backed by ready preflight evidence.
    #[error("writer activation permit for world {world} is not ready: {reason}")]
    ActivationPermitInvalid {
        /// World named by the rejected permit.
        world: WorldId,
        /// Stable validation diagnostic.
        reason: &'static str,
    },
    /// Authoritative metadata changed after preflight produced a permit.
    #[error(
        "writer activation permit for world {world} was issued at metadata epoch {permit_epoch:?}, but the authoritative epoch is {actual_epoch:?}"
    )]
    ActivationPermitStale {
        /// World whose metadata advanced.
        world: WorldId,
        /// Epoch captured by pure preflight.
        permit_epoch: MetadataEpoch,
        /// Epoch observed immediately before activation.
        actual_epoch: MetadataEpoch,
    },
    /// An activation permit hash disagreed with authoritative metadata.
    #[error("writer activation permit hash mismatch for world {world}: {field}")]
    ActivationPermitHashMismatch {
        /// World whose permit was revalidated.
        world: WorldId,
        /// Stable name of the mismatching digest.
        field: &'static str,
        /// Digest captured by the permit.
        permit: DigestV1,
        /// Digest recomputed from authoritative state.
        actual: DigestV1,
    },
    /// Catalog acceptance lacks the typed evidence needed for safe activation.
    #[error(
        "writer activation for world {world} is blocked until catalog supplies bound plan evidence"
    )]
    ActivationEvidenceUnavailable {
        /// World whose activation failed closed.
        world: WorldId,
    },
    /// A header-repair permit was not backed by repair-required preflight evidence.
    #[error("header-repair permit for world {world} is not valid: {reason}")]
    HeaderRepairPermitInvalid {
        /// World named by the rejected permit.
        world: WorldId,
        /// Stable validation diagnostic.
        reason: &'static str,
    },
    /// Authoritative metadata changed after preflight produced a repair permit.
    #[error(
        "header-repair permit for world {world} was issued at metadata epoch {permit_epoch:?}, but the authoritative epoch is {actual_epoch:?}"
    )]
    HeaderRepairPermitStale {
        /// World whose metadata advanced.
        world: WorldId,
        /// Epoch captured by pure preflight.
        permit_epoch: MetadataEpoch,
        /// Epoch observed immediately before sidecar repair.
        actual_epoch: MetadataEpoch,
    },
    /// A header-repair permit hash disagreed with authoritative metadata.
    #[error("header-repair permit hash mismatch for world {world}: {field}")]
    HeaderRepairPermitHashMismatch {
        /// World whose repair permit was revalidated.
        world: WorldId,
        /// Stable name of the mismatching digest.
        field: &'static str,
        /// Digest captured by the repair permit.
        permit: DigestV1,
        /// Digest recomputed from authoritative state.
        actual: DigestV1,
    },
    /// Another exclusive writer lease is already active for a world.
    #[error("world {world} already has an active authoritative writer")]
    WriterAlreadyActive {
        /// World whose writer is already active.
        world: WorldId,
    },
    /// An operation used a closed, replaced, or otherwise invalid writer lease.
    #[error("authoritative writer lease for world {world} is no longer valid")]
    WriterLeaseInvalid {
        /// World formerly owned by the lease.
        world: WorldId,
    },
    /// A transaction named a different world from its writer lease.
    #[error("transaction world {transaction_world} does not match writer world {writer_world}")]
    TransactionWorldMismatch {
        /// World owned by the active writer.
        writer_world: WorldId,
        /// World carried by the transaction.
        transaction_world: WorldId,
    },
    /// A transaction contained no authoritative chunk mutation.
    #[error("an authoritative world transaction must contain at least one chunk mutation")]
    EmptyTransaction,
    /// A chunk key did not belong to the transaction's world.
    #[error("chunk {key:?} does not belong to transaction world {transaction_world}")]
    ChunkWorldMismatch {
        /// Transaction's authoritative world.
        transaction_world: WorldId,
        /// Rejected chunk key.
        key: Box<ChunkKey>,
    },
    /// One transaction named the same chunk more than once.
    #[error("transaction contains duplicate chunk mutation {key:?}")]
    DuplicateChunk {
        /// Duplicated chunk key.
        key: Box<ChunkKey>,
    },
    /// A transaction exceeded its chunk-count ceiling.
    #[error("transaction contains {actual} chunks, exceeding the hard ceiling {maximum}")]
    TransactionChunkLimitExceeded {
        /// Number of mutations supplied.
        actual: u64,
        /// Maximum number accepted atomically.
        maximum: u32,
    },
    /// A transaction exceeded its canonical uncompressed byte ceiling.
    #[error("transaction payload is at least {actual} bytes, exceeding the hard ceiling {maximum}")]
    TransactionPayloadLimitExceeded {
        /// Minimum canonical bytes measured when traversal stopped.
        actual: u64,
        /// Maximum canonical uncompressed bytes accepted atomically.
        maximum: u64,
    },
    /// Optimistic world revision validation rejected a transaction.
    #[error(
        "world {world} expected authoritative revision {expected:?}, but current revision is {actual:?}"
    )]
    WorldRevisionConflict {
        /// World being mutated.
        world: WorldId,
        /// Revision captured by the caller.
        expected: WorldRevision,
        /// Current authoritative revision.
        actual: WorldRevision,
    },
    /// Optimistic chunk revision validation rejected a mutation.
    #[error("chunk {key:?} expected {expected:?}, but current revision is {actual:?}")]
    ChunkRevisionConflict {
        /// Chunk whose precondition failed.
        key: Box<ChunkKey>,
        /// Revision condition captured by the caller.
        expected: ChunkRevisionExpectation,
        /// Current revision, or `None` when no record exists.
        actual: Option<ChunkRevision>,
    },
    /// An ordinary commit attempted to replace the exact frozen lock.
    #[error("world {world} frozen lock changed without an accepted checkpointed migration")]
    FrozenLockChangeRequiresMigration {
        /// World whose exact lock was changed.
        world: WorldId,
    },
    /// An ordinary commit attempted to shrink or rewrite the used-data closure.
    #[error("world {world} requirement closure is not a monotonic expansion")]
    RequirementClosureRegression {
        /// World whose closure would regress.
        world: WorldId,
    },
    /// A reference backend was asked to claim a physical persistence boundary.
    #[error(
        "the reference backend cannot claim {operation}; a physical backend capability is required"
    )]
    PhysicalDurabilityUnsupported {
        /// Physical operation that was requested.
        operation: &'static str,
    },
    /// New authoritative mutation is paused by storage-pressure admission.
    #[error("world {world} rejected authoritative mutation under storage pressure `{pressure}`")]
    LowDiskMutationPaused {
        /// World whose mutation was refused before publication.
        world: WorldId,
        /// Stable storage-pressure state name.
        pressure: &'static str,
    },
    /// Writer activation is refused because only read-only recovery is safe.
    #[error("world {world} is recoverable read-only: {reason}")]
    RecoverableReadOnly {
        /// World that must not open a writer.
        world: WorldId,
        /// Stable recovery diagnostic.
        reason: &'static str,
    },
    /// A canonical durable-store image failed bounded decode or integrity checks.
    #[error("canonical durable store image is corrupt: {reason}")]
    CorruptDurableImage {
        /// Validation diagnostic.
        reason: String,
    },
    /// A catalog identity conflict is non-repairable and blocks opening.
    #[error("world {world} header identity disagrees with authoritative metadata: {reason}")]
    ReconciliationBlocked {
        /// Authoritative world identity.
        world: WorldId,
        /// Stable catalog reconciliation diagnostic.
        reason: String,
    },
    /// A mutation's declared domains disagreed with its actual replacement.
    #[error(
        "chunk {key:?} declared changed domains {declared:?}, but replacement changes {actual:?}"
    )]
    ChangedDomainsMismatch {
        /// Chunk whose declaration was inaccurate.
        key: Box<ChunkKey>,
        /// Caller-declared changed domains.
        declared: ChangedDomains,
        /// Domains computed from old and replacement data.
        actual: ChangedDomains,
    },
    /// A replacement was identical to the current authoritative chunk.
    #[error("chunk {key:?} replacement is an authoritative no-op")]
    NoopMutation {
        /// Chunk that would not change.
        key: Box<ChunkKey>,
    },
    /// An idempotency identifier was reused for different content.
    #[error(
        "transaction id {transaction_id:?} was reused with different contents in world {world}"
    )]
    TransactionIdReuse {
        /// World in which transaction identifiers are scoped.
        world: WorldId,
        /// Reused transaction identity.
        transaction_id: TransactionId,
    },
    /// An exact retry fell outside the bounded retained-receipt horizon.
    #[error(
        "transaction {transaction_id:?} in world {world} at base {transaction_base:?} is older than replayable base {oldest_replayable_base:?}"
    )]
    RetryWindowExpired {
        /// World whose retry horizon advanced.
        world: WorldId,
        /// Expired transaction identity.
        transaction_id: TransactionId,
        /// Base revision captured by the expired request.
        transaction_base: WorldRevision,
        /// Oldest base revision still covered by retained receipts.
        oldest_replayable_base: WorldRevision,
    },
    /// A persistent entity was claimed by two chunks in one world.
    #[error(
        "persistent entity {entity:?} in world {world} belongs to {existing:?}, not {attempted:?}"
    )]
    PersistentEntityCollision {
        /// World containing the entity.
        world: WorldId,
        /// Colliding world-scoped entity identity.
        entity: PersistentEntityId,
        /// Existing authoritative owner chunk.
        existing: Box<ChunkKey>,
        /// Chunk attempting to claim the entity.
        attempted: Box<ChunkKey>,
    },
    /// Bounded receipt ordering disagreed with the retained receipt map.
    #[error("retained receipt history invariant failed in world {world}")]
    ReceiptHistoryInvariant {
        /// World containing the inconsistent history.
        world: WorldId,
    },
    /// The persistent-entity index disagreed with authoritative chunk data.
    #[error("persistent-entity index invariant failed for {entity:?} in world {world}")]
    EntityIndexInvariant {
        /// World containing the inconsistent index.
        world: WorldId,
        /// Entity whose derived location was inconsistent.
        entity: PersistentEntityId,
    },
    /// Persistence frontiers violated their required monotonic ordering.
    #[error("persistence frontier invariant failed in world {world}: {reason}")]
    FrontierInvariant {
        /// World containing invalid frontier metadata.
        world: WorldId,
        /// Stable validation diagnostic.
        reason: &'static str,
    },
    /// A checkpoint was requested before all current writes became durable.
    #[error(
        "checkpoint for world {world} requires a durable frontier, but current is {current:?} and durable is {durable:?}"
    )]
    CheckpointRequiresDurableFrontier {
        /// World being checkpointed.
        world: WorldId,
        /// Latest authoritative revision.
        current: WorldRevision,
        /// Latest contiguous durable revision.
        durable: WorldRevision,
    },
    /// A checkpoint identity was already retained for the world.
    #[error("checkpoint {checkpoint:?} already exists for world {world}")]
    CheckpointAlreadyExists {
        /// World owning the checkpoint namespace.
        world: WorldId,
        /// Duplicate checkpoint identity.
        checkpoint: CheckpointId,
    },
    /// A requested checkpoint duplicated equivalent retained evidence.
    #[error(
        "checkpoint {requested:?} for world {world} is equivalent to retained checkpoint {existing:?}"
    )]
    EquivalentCheckpoint {
        /// World whose checkpoint image would be duplicated.
        world: WorldId,
        /// New checkpoint identity.
        requested: CheckpointId,
        /// Existing equivalent checkpoint identity.
        existing: CheckpointId,
    },
    /// A world reached its bounded checkpoint count.
    #[error("world {world} has {actual} checkpoints, reaching the hard ceiling {maximum}")]
    CheckpointLimitExceeded {
        /// World whose retention set is full.
        world: WorldId,
        /// Retained count including the proposed checkpoint.
        actual: u64,
        /// Maximum retained count.
        maximum: u32,
    },
    /// A requested checkpoint does not exist.
    #[error("checkpoint {checkpoint:?} does not exist for world {world}")]
    CheckpointNotFound {
        /// World expected to own the checkpoint.
        world: WorldId,
        /// Missing checkpoint identity.
        checkpoint: CheckpointId,
    },
    /// An independent checkpoint image failed content or restore validation.
    #[error("checkpoint {checkpoint:?} for world {world} is corrupt: {reason}")]
    CorruptCheckpoint {
        /// World owning the checkpoint.
        world: WorldId,
        /// Checkpoint that failed verification.
        checkpoint: CheckpointId,
        /// Verification diagnostic.
        reason: String,
    },
    /// An automatic backend operation exhausted its bounded retry allowance.
    #[error("{operation} exhausted {attempts} automatic attempts: {last_error}")]
    RetryLimitExhausted {
        /// Stable retried operation name.
        operation: &'static str,
        /// Total attempts performed.
        attempts: u8,
        /// Final backend diagnostic.
        last_error: String,
    },
    /// A deterministic database failpoint interrupted pre-publication work.
    #[error("injected deterministic database fault at {point}")]
    InjectedDatabaseFault {
        /// Stable failpoint name.
        point: &'static str,
    },
    /// A second failpoint was installed before the pending one ran.
    #[error("a deterministic database failpoint is already pending")]
    FaultAlreadyPending,
    /// An in-process backend lock was poisoned by a panic.
    #[error("world-storage lock was poisoned during {operation}")]
    LockPoisoned {
        /// Operation that attempted to acquire the lock.
        operation: &'static str,
    },
}

impl WorldDbError {
    pub(crate) fn metadata_encode(error: &serde_json::Error) -> Self {
        Self::JsonEncode {
            artifact: "authoritative metadata v1",
            reason: error.to_string(),
        }
    }

    pub(crate) fn postcard_encode(artifact: &'static str, error: &postcard::Error) -> Self {
        Self::PostcardEncode {
            artifact,
            reason: error.to_string(),
        }
    }
}
