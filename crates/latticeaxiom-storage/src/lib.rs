//! Sealed materialized-chunk transaction kernel and deterministic reference.
//!
//! A [`WorldTransaction`] atomically publishes bounded complete chunk
//! replacements under one [`WorldRevision`]. [`MemoryTransactionKernel`] is a
//! non-durable, reference-scale implementation with snapshot isolation,
//! optimistic conflicts, checked revisions, bounded exact replay, a derived
//! world-scoped entity index, and deterministic fault injection.
//!
//! This is not the complete D3 `WorldStorage` boundary. The trait is sealed and
//! intentionally omits typed world metadata and requirement closure, bounded
//! production reads, frontiers, checkpoints, a frozen wire envelope, `RocksDB`,
//! Bevy, and runtime facades.
mod canonical_hash;
#[cfg(any(test, feature = "conformance"))]
#[allow(
    clippy::expect_used,
    reason = "conformance assertions name the invariant that a backend violated"
)]
pub mod conformance;
mod error;
mod memory;
mod model;
mod storage;

pub use canonical_hash::MaterializedChunkStateHash;
pub use error::{RevisionCounter, StorageError, StorageResult};
pub use memory::MemoryTransactionKernel;
pub use model::{
    ChangedDomains, ChunkCommitReceipt, ChunkCoordinate, ChunkData, ChunkKey, ChunkMutation,
    ChunkRevision, ChunkRevisionExpectation, CommitReceipt, ContinuationId, ContinuationRevision,
    DimensionId, DomainRevisions, FaultPoint, PayloadSchemaVersion, PersistentEntityId,
    PersistentEntityRevision, ReferenceDurability, ReferenceWorldSnapshot, StoredChunk,
    TransactionId, TransactionKernelLimits, VersionedPayload, VoxelRevision, WorldRevision,
    WorldTransaction,
};
pub use storage::AuthoritativeTransactionKernel;

#[cfg(test)]
mod property_tests;
