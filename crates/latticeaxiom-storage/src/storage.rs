//! Semantic storage contract shared by memory and production backends.

use crate::{
    ArtifactReceipt, CheckpointId, CheckpointTarget, ChunkCommit, ChunkKey, CommitReceipt,
    ReceiptQuery, StorageResult, StoredChunk,
};

/// Authoritative storage for materialized world chunks.
///
/// Implementations provide a consistent read view and atomically replace the
/// snapshot, spatial entities, continuation state, and artifact receipts of a
/// chunk. The facade deliberately contains no `RocksDB` types.
pub trait WorldStorage: Send + Sync {
    /// Loads a fully reconstructed chunk from one consistent view.
    ///
    /// # Errors
    ///
    /// Returns an error when the backend fails, a versioned record cannot be
    /// decoded, or logically atomic records do not share one revision.
    fn load_chunk(&self, key: ChunkKey) -> StorageResult<Option<StoredChunk>>;

    /// Atomically commits every authoritative category of one chunk.
    ///
    /// An exact retry with the same [`crate::CommitId`] returns the original
    /// revision with [`CommitReceipt::replayed`] set. Reusing that identifier
    /// for different contents is rejected. No receipt is returned before the
    /// requested [`crate::Durability`] boundary.
    ///
    /// # Errors
    ///
    /// Returns an error for invalid commits, revision conflicts, artifact
    /// ownership collisions, unsupported schemas, or backend failures. A
    /// failed commit must leave all authoritative categories unchanged.
    fn commit_chunk(&self, commit: ChunkCommit) -> StorageResult<CommitReceipt>;

    /// Scans artifact receipts in stable persistent-key order.
    ///
    /// # Errors
    ///
    /// Returns an error when the backend scan fails or encounters malformed
    /// data.
    fn scan_receipts(
        &self,
        query: &ReceiptQuery,
    ) -> StorageResult<Vec<(ChunkKey, crate::ArtifactKey, ArtifactReceipt)>>;

    /// Creates an independently openable checkpoint at an empty target path.
    ///
    /// # Errors
    ///
    /// Returns an error when the target already exists, filesystem work
    /// fails, or the backend cannot create a consistent checkpoint.
    fn create_checkpoint(&self, target: &CheckpointTarget) -> StorageResult<CheckpointId>;
}
