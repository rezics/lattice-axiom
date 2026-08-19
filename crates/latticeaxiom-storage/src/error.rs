//! Domain errors returned by world storage implementations.

use std::io;
use std::path::PathBuf;

use thiserror::Error;

use crate::{ArtifactKey, ChunkKey, CommitCondition, CommitId, WorldId};

/// Errors raised while constructing a stable textual identifier.
#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum IdentifierError {
    /// The identifier was empty.
    #[error("identifier must not be empty")]
    Empty,
    /// The identifier exceeded the persistent-key limit.
    #[error("identifier is {length} bytes, exceeding the {maximum}-byte limit")]
    TooLong {
        /// Actual UTF-8 byte length.
        length: usize,
        /// Maximum accepted UTF-8 byte length.
        maximum: usize,
    },
}

/// Errors produced by [`crate::WorldStorage`] implementations.
#[derive(Debug, Error)]
pub enum StorageError {
    /// A public identifier failed validation.
    #[error("invalid identifier: {0}")]
    InvalidIdentifier(#[from] IdentifierError),
    /// A chunk commit violated a storage invariant.
    #[error("invalid chunk commit: {reason}")]
    InvalidCommit {
        /// Human-readable invariant failure.
        reason: String,
    },
    /// Optimistic revision validation rejected a commit.
    #[error("chunk {key:?} does not satisfy {expected:?}; current revision is {actual:?}")]
    RevisionConflict {
        /// Chunk whose revision did not match.
        key: ChunkKey,
        /// Revision condition requested by the caller.
        expected: CommitCondition,
        /// Revision found in storage, or `None` when the chunk is absent.
        actual: Option<u64>,
    },
    /// A commit identifier was reused for different contents.
    #[error("commit id {commit_id:?} was reused with different contents in world {world:?}")]
    CommitIdReuse {
        /// World in which commit identifiers are scoped.
        world: WorldId,
        /// Reused commit identifier.
        commit_id: CommitId,
    },
    /// An artifact key was already owned by another chunk.
    #[error("artifact {artifact:?} in world {world:?} belongs to {existing:?}, not {attempted:?}")]
    ArtifactCollision {
        /// World containing the artifact.
        world: WorldId,
        /// Colliding artifact key.
        artifact: Box<ArtifactKey>,
        /// Existing authoritative chunk.
        existing: ChunkKey,
        /// Chunk attempting to claim the artifact.
        attempted: ChunkKey,
    },
    /// Persistent data was incomplete, malformed, or internally inconsistent.
    #[error("corrupt world storage record: {reason}")]
    CorruptRecord {
        /// Human-readable corruption diagnostic.
        reason: String,
    },
    /// A value envelope used a newer outer format.
    #[error("unsupported value envelope format {found}; this build supports {supported}")]
    UnsupportedEnvelope {
        /// Version found on disk.
        found: u16,
        /// Latest supported version.
        supported: u16,
    },
    /// A record used an unsupported schema version.
    #[error("unsupported {record} schema version {found}; this build supports {supported}")]
    UnsupportedSchema {
        /// Logical record kind.
        record: &'static str,
        /// Version found on disk.
        found: u32,
        /// Latest supported version.
        supported: u32,
    },
    /// Encoding or decoding a versioned record failed.
    #[error("could not {operation} {record}: {message}")]
    Codec {
        /// Operation being performed.
        operation: &'static str,
        /// Logical record kind.
        record: &'static str,
        /// Backend codec diagnostic.
        message: String,
    },
    /// A backend failed without exposing its concrete error type.
    #[error("world storage backend error during {operation}: {message}")]
    Backend {
        /// Backend operation being performed.
        operation: &'static str,
        /// Human-readable backend diagnostic.
        message: String,
    },
    /// An in-process storage lock was poisoned by a panic.
    #[error("world storage lock was poisoned during {operation}")]
    LockPoisoned {
        /// Operation that attempted to acquire the lock.
        operation: &'static str,
    },
    /// Creating or restoring a checkpoint failed at the filesystem boundary.
    #[error("checkpoint I/O failed at {path:?}: {source}")]
    CheckpointIo {
        /// Path involved in the failed operation.
        path: PathBuf,
        /// Underlying filesystem error.
        #[source]
        source: io::Error,
    },
}

/// Result type shared by storage facade operations.
pub type StorageResult<T> = Result<T, StorageError>;
