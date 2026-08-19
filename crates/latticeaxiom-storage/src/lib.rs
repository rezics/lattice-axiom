//! Backend-agnostic authoritative world storage (ADR 0009).
//!
//! [`WorldStorage`] commits the materialized snapshot, persistent spatial
//! entities, deterministic simulation continuation, and provenance receipts
//! of one chunk as a single atomic operation. Values use a versioned envelope,
//! while signed chunk coordinates use sign-bit-flipped big-endian key bytes so
//! persistent iteration follows numeric `(x, y, z)` order.
//!
//! [`MemoryWorldStorage`] is the reference implementation used by unit and
//! property tests. Production `RocksDB` access lives exclusively in the
//! `latticeaxiom-storage-rocksdb` crate and consumes the hidden backend support
//! module without leaking backend types through this facade.

#[doc(hidden)]
pub mod backend;
#[allow(
    clippy::expect_used,
    reason = "the conformance suite is test support and reports violated invariants"
)]
pub mod conformance;
mod error;
mod memory;
mod model;
mod storage;

pub use error::{IdentifierError, StorageError, StorageResult};
pub use latticeaxiom_core::ChunkPos;
pub use memory::MemoryWorldStorage;
pub use model::{
    ArtifactId, ArtifactKey, ArtifactReceipt, BoundaryContractId, CheckpointId, CheckpointTarget,
    ChunkCommit, ChunkKey, ChunkSnapshot, CommitCondition, CommitId, CommitReceipt, ContentHash,
    DimensionId, Durability, EntityId, GenerationEpoch, GenerationProvenance, ImplementationId,
    OwnedPayload, OwnerId, ReceiptDomain, ReceiptQuery, SchemaVersion, StoredChunk, WorkKind,
    WorldId,
};
pub use storage::WorldStorage;
