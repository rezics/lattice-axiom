//! Typed failures returned by the materialized-chunk transaction kernel.

use latticeaxiom_core::WorldId;
use thiserror::Error;

use crate::{
    ChangedDomains, ChunkKey, ChunkRevision, ChunkRevisionExpectation, FaultPoint, TransactionId,
    WorldRevision,
};

/// Revision counter that could not be incremented without wrapping.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RevisionCounter {
    /// The world's global authoritative command sequence.
    World,
    /// A chunk's total authoritative revision.
    Chunk,
    /// A chunk's voxel-domain revision.
    Voxels,
    /// A chunk's persistent-entity-domain revision.
    PersistentEntities,
    /// A chunk's continuation-domain revision.
    Continuation,
}

/// Errors produced by [`crate::AuthoritativeTransactionKernel`] implementations.
#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum StorageError {
    /// A configured hard limit was zero.
    #[error("transaction-kernel limit {name} must be greater than zero, got {value}")]
    InvalidLimit {
        /// Stable limit name.
        name: &'static str,
        /// Rejected value.
        value: u64,
    },
    /// A dimension identifier used a non-dimension registration kind.
    #[error("invalid dimension identifier {value}: {reason}")]
    InvalidDimensionId {
        /// Rejected identifier text.
        value: String,
        /// Stable validation explanation.
        reason: String,
    },
    /// A payload schema version used reserved version zero.
    #[error("payload schema version must be greater than zero")]
    InvalidSchemaVersion,
    /// A changed-domain mask contained unknown bits.
    #[error("changed-domain mask contains unknown bits: {bits:#010b}")]
    InvalidDomainMask {
        /// Rejected raw bit mask.
        bits: u8,
    },
    /// A transaction did not contain any authoritative mutation.
    #[error("an authoritative transaction must contain at least one chunk mutation")]
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
        /// Duplicated key.
        key: Box<ChunkKey>,
    },
    /// A transaction exceeded its chunk-count ceiling.
    #[error("transaction requires {actual_chunks} chunks; limit is {max_chunks}")]
    TransactionChunkLimitExceeded {
        /// Number of mutations supplied.
        actual_chunks: u64,
        /// Maximum mutations accepted atomically.
        max_chunks: u32,
    },
    /// A transaction exceeded its canonical uncompressed payload ceiling.
    #[error(
        "transaction payload is at least {minimum_payload_bytes} bytes; limit is {max_payload_bytes}"
    )]
    TransactionPayloadLimitExceeded {
        /// Minimum canonical bytes measured when bounded traversal stopped.
        minimum_payload_bytes: u64,
        /// Maximum canonical uncompressed payload bytes accepted.
        max_payload_bytes: u64,
    },
    /// Computing a bounded canonical payload length overflowed `u64`.
    #[error("canonical payload size overflow while measuring {what}")]
    PayloadSizeOverflow {
        /// Value whose size could not be represented.
        what: &'static str,
    },
    /// Optimistic world revision validation rejected the transaction.
    #[error(
        "world {world} expected authoritative revision {expected:?}, but current revision is {actual:?}"
    )]
    WorldRevisionConflict {
        /// World being mutated.
        world: WorldId,
        /// Revision captured by the caller.
        expected: WorldRevision,
        /// Current storage revision.
        actual: WorldRevision,
    },
    /// Optimistic chunk revision validation rejected one mutation.
    #[error("chunk {key:?} expected {expected:?}, but current revision is {actual:?}")]
    ChunkRevisionConflict {
        /// Chunk whose precondition failed.
        key: Box<ChunkKey>,
        /// Revision condition captured by the caller.
        expected: ChunkRevisionExpectation,
        /// Current revision, or `None` when absent.
        actual: Option<ChunkRevision>,
    },
    /// A mutation's declared domains did not match its actual replacement.
    #[error(
        "chunk {key:?} declared changed domains {declared:?}, but replacement changes {actual:?}"
    )]
    ChangedDomainsMismatch {
        /// Chunk whose declaration was inaccurate.
        key: Box<ChunkKey>,
        /// Caller-declared changed domains.
        declared: ChangedDomains,
        /// Domains computed from authoritative old and new state.
        actual: ChangedDomains,
    },
    /// A replacement was byte-for-byte identical to existing authoritative state.
    #[error("chunk {key:?} replacement is an authoritative no-op")]
    NoopMutation {
        /// Chunk that would not change.
        key: Box<ChunkKey>,
    },
    /// A checked monotonic revision exhausted its `u64` range.
    #[error("{counter:?} revision overflow while committing {key:?}")]
    RevisionOverflow {
        /// Counter that could not advance.
        counter: RevisionCounter,
        /// Affected chunk, or `None` for the world counter.
        key: Option<Box<ChunkKey>>,
    },
    /// An idempotency identifier was reused for a different transaction.
    #[error(
        "transaction id {transaction_id:?} was reused with different contents in world {world}"
    )]
    TransactionIdReuse {
        /// World in which transaction identifiers are scoped.
        world: WorldId,
        /// Reused identifier.
        transaction_id: TransactionId,
    },
    /// Reference evidence was requested for a transaction originally accepted
    /// through the scale-sensitive publication path.
    #[error(
        "transaction {transaction_id:?} in world {world} has no retained full-world reference hash"
    )]
    ReferenceReceiptUnavailable {
        /// World in which the transaction was published.
        world: WorldId,
        /// Transaction whose reference-only evidence was not computed.
        transaction_id: TransactionId,
    },
    /// An exact retry fell outside the bounded retained-receipt horizon.
    #[error(
        "transaction {transaction_id:?} in world {world} is older than replayable base revision {oldest_replayable_base:?}"
    )]
    RetryWindowExpired {
        /// World whose retry horizon advanced.
        world: WorldId,
        /// Expired transaction identifier.
        transaction_id: TransactionId,
        /// Base revision captured by the expired transaction.
        transaction_base: WorldRevision,
        /// Oldest base revision still covered by a retained receipt.
        oldest_replayable_base: WorldRevision,
    },
    /// A world-scoped persistent entity was claimed by two chunks.
    #[error(
        "persistent entity {entity:?} in world {world} belongs to {existing:?}, not {attempted:?}"
    )]
    PersistentEntityCollision {
        /// World containing the entity.
        world: WorldId,
        /// Colliding world-scoped entity identifier.
        entity: crate::PersistentEntityId,
        /// Existing authoritative chunk.
        existing: Box<ChunkKey>,
        /// Chunk attempting to claim the entity.
        attempted: Box<ChunkKey>,
    },
    /// The bounded receipt order and receipt map disagreed internally.
    #[error("retained receipt history invariant failed in world {world}")]
    ReceiptHistoryInvariant {
        /// World containing the inconsistent reference history.
        world: WorldId,
    },
    /// The reference kernel's derived entity index disagreed with chunk data.
    #[error("derived entity index invariant failed for {entity:?} in world {world}")]
    EntityIndexInvariant {
        /// World containing the inconsistent index.
        world: WorldId,
        /// Entity whose derived location was inconsistent.
        entity: crate::PersistentEntityId,
    },
    /// A deterministic memory-reference failpoint interrupted the commit.
    #[error("injected memory storage fault at {point:?}")]
    InjectedFault {
        /// Phase that deliberately failed.
        point: FaultPoint,
    },
    /// A second failpoint was installed before the pending one ran.
    #[error("a memory storage failpoint is already pending")]
    FaultAlreadyPending,
    /// An in-process storage lock was poisoned by a panic.
    #[error("memory transaction-kernel lock was poisoned during {operation}")]
    LockPoisoned {
        /// Operation that attempted to acquire the lock.
        operation: &'static str,
    },
}

/// Result type shared by materialized-chunk transaction-kernel operations.
pub type StorageResult<T> = Result<T, StorageError>;
