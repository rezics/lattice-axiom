//! Sealed materialized-chunk transaction-kernel boundary.

use latticeaxiom_core::WorldId;

use crate::{
    ChunkKey, CommitReceipt, ReferenceWorldSnapshot, StorageResult, StoredChunk,
    TransactionKernelLimits, WorldRevision, WorldTransaction,
};

pub(crate) mod sealed {
    pub trait Sealed {}
}

/// Atomic transaction kernel for already-materialized world chunks.
///
/// This is deliberately not the complete D3 `WorldStorage` product boundary.
/// It does not model world metadata, requirement closure, checkpoints,
/// durable frontiers, migration, or physical database failures. Implementors
/// remain sealed inside this crate until those contracts are represented
/// without opaque placeholders.
///
/// The full-world reference snapshots are test evidence, not a production
/// scale read API. A production boundary must instead expose bounded chunk
/// reads, a contiguous world frontier, and checkpoint receipts.
pub trait AuthoritativeTransactionKernel: sealed::Sealed + Send + Sync {
    /// Returns hard transaction and retained-receipt ceilings for this instance.
    #[must_use]
    fn limits(&self) -> TransactionKernelLimits;

    /// Reads the current contiguous revision of one world without materializing
    /// a full-world reference snapshot.
    ///
    /// # Errors
    ///
    /// Returns a typed kernel error if a consistent frontier cannot be read.
    fn world_frontier(&self, world: WorldId) -> StorageResult<WorldRevision>;

    /// Reads one complete chunk without materializing unrelated world chunks.
    ///
    /// The returned value is owned and snapshot-isolated from later commits.
    ///
    /// # Errors
    ///
    /// Returns a typed kernel error if a consistent bounded read cannot be
    /// captured.
    fn read_chunk(&self, key: &ChunkKey) -> StorageResult<Option<StoredChunk>>;

    /// Captures an owned, snapshot-isolated reference view of one world.
    ///
    /// Later commits cannot change the returned value. Chunk iteration and the
    /// materialized-chunk state hash are deterministic.
    ///
    /// # Errors
    ///
    /// Returns a typed kernel error if a consistent view cannot be captured.
    fn reference_snapshot(&self, world: WorldId) -> StorageResult<ReferenceWorldSnapshot>;

    /// Atomically publishes an optimistic multi-chunk transaction.
    ///
    /// An exact retry inside the configured bounded receipt horizon returns its
    /// original receipt with `replayed = true`. Reusing a retained ID for
    /// different canonical contents is rejected. An older retry returns a
    /// stable expiration or revision-conflict error and is never re-applied.
    /// Every validation, conflict, limit, or pre-publish failure leaves
    /// authoritative state unchanged.
    ///
    /// # Errors
    ///
    /// Returns an error for invalid or over-limit transactions, optimistic
    /// conflicts, revision overflow, retry expiration, transaction-ID reuse,
    /// or reference-kernel failure.
    fn commit(&self, transaction: WorldTransaction) -> StorageResult<CommitReceipt>;
}
