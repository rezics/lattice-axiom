//! Typed launcher boundary errors.

use std::io;

use latticeaxiom_core::CanonicalHash;
use thiserror::Error;

use crate::{LaunchGeneration, LaunchTargetV1};

/// Invalid construction of a launcher DTO or typed counter.
#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum LaunchModelError {
    /// A counter that must be non-zero was zero.
    #[error("{field} must be non-zero")]
    Zero {
        /// Stable field name.
        field: &'static str,
    },
    /// A monotonic counter could not be incremented.
    #[error("{field} exhausted its u64 range")]
    CounterExhausted {
        /// Stable field name.
        field: &'static str,
    },
    /// An intent expiry preceded its issue instant.
    #[error("launch intent expires at {expires_at_ms} before issue time {issued_at_ms}")]
    InvalidLifetime {
        /// Intent issue time.
        issued_at_ms: u64,
        /// Intent expiry time.
        expires_at_ms: u64,
    },
    /// An intent lifetime exceeded the hard policy.
    #[error("launch intent lifetime {actual_ms} ms exceeds hard limit {maximum_ms} ms")]
    LifetimeExceeded {
        /// Declared lifetime.
        actual_ms: u64,
        /// Hard maximum lifetime.
        maximum_ms: u64,
    },
    /// V1 permits exactly one immutable target boot attempt.
    #[error("launch attempt {actual} is invalid; expected exactly one")]
    InvalidAttempt {
        /// Rejected attempt count.
        actual: u8,
    },
    /// Target-specific hashes were absent or unexpectedly present.
    #[error("target {target:?} has an invalid world lock/open-plan hash shape")]
    InvalidTargetHashes {
        /// Target whose hashes were invalid.
        target: LaunchTargetV1,
    },
    /// A recovery request named a source generation newer than its durable generation.
    #[error(
        "recovery source generation {source_generation:?} exceeds claim generation {recovery_generation}"
    )]
    InvalidRecoveryGeneration {
        /// Failed or current generation, when known.
        source_generation: Option<LaunchGeneration>,
        /// Durable generation carried by the recovery claim.
        recovery_generation: LaunchGeneration,
    },
    /// Canonical encoding failed while sealing a DTO.
    #[error("failed to canonicalize launch data: {reason}")]
    CanonicalEncoding {
        /// Bounded upstream diagnostic.
        reason: String,
    },
}

/// Rejection while decoding or validating an untrusted launch intent.
#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum LaunchIntentError {
    /// The envelope exceeded the fixed input ceiling.
    #[error("launch intent has {actual_bytes} bytes; maximum is {maximum_bytes}")]
    InputTooLarge {
        /// Actual byte length.
        actual_bytes: usize,
        /// Maximum accepted byte length.
        maximum_bytes: usize,
    },
    /// The bytes were not a strict V1 JSON envelope.
    #[error("launch intent JSON is invalid: {reason}")]
    InvalidJson {
        /// Bounded parser diagnostic.
        reason: String,
    },
    /// The envelope was valid JSON but not canonical JSON bytes.
    #[error("launch intent bytes are not the canonical JSON encoding")]
    NonCanonicalBytes,
    /// The schema version is not supported.
    #[error("unsupported launch intent schema version {actual}; expected {expected}")]
    UnsupportedSchema {
        /// Encountered schema version.
        actual: u32,
        /// Supported schema version.
        expected: u32,
    },
    /// The checksum did not authenticate the normalized body.
    #[error("launch intent checksum mismatch: expected {expected}, recomputed {actual}")]
    ChecksumMismatch {
        /// Claimed checksum.
        expected: CanonicalHash,
        /// Recomputed checksum.
        actual: CanonicalHash,
    },
    /// Target-specific lock fields were malformed.
    #[error("launch intent target/hash shape is invalid")]
    InvalidTargetHashes,
    /// The immutable attempt count violated the one-attempt policy.
    #[error("launch intent attempt count {actual} is invalid; expected exactly one")]
    InvalidAttempt {
        /// Encountered attempt count.
        actual: u8,
    },
    /// The issued time was implausibly far in the future.
    #[error(
        "launch intent issue time {issued_at_ms} exceeds now {now_ms} plus allowed skew {allowed_skew_ms}"
    )]
    NotYetValid {
        /// Declared issue time.
        issued_at_ms: u64,
        /// Validation time.
        now_ms: u64,
        /// Allowed future skew.
        allowed_skew_ms: u64,
    },
    /// The envelope expired before validation.
    #[error("launch intent expired at {expires_at_ms}; validation time is {now_ms}")]
    Expired {
        /// Declared expiry.
        expires_at_ms: u64,
        /// Validation time.
        now_ms: u64,
    },
    /// The envelope declared an invalid or excessive lifetime.
    #[error("launch intent lifetime is invalid")]
    InvalidLifetime,
    /// A generation was stale or replayed.
    #[error("launch generation {actual} is not newer than handled generation {last_handled}")]
    StaleGeneration {
        /// Rejected generation.
        actual: LaunchGeneration,
        /// Highest already handled generation.
        last_handled: LaunchGeneration,
    },
    /// A generation skipped the exact next monotonic value.
    #[error("launch generation {actual} skipped required generation {expected}")]
    GenerationGap {
        /// Rejected generation.
        actual: LaunchGeneration,
        /// Exact required generation.
        expected: LaunchGeneration,
    },
    /// The shell lock differs from the launcher-selected lock.
    #[error("launch intent shell lock does not match the selected shell lock")]
    ShellLockMismatch,
    /// The settings journal revision differs from the shutdown-barrier receipt.
    #[error("launch intent confirmed settings revision does not match the boot journal")]
    ConfirmedSettingsRevisionMismatch,
    /// The world target differs from the preflight-selected target.
    #[error("launch intent target does not match the selected target")]
    TargetMismatch,
    /// The world lock differs from the frozen lock selected by preflight.
    #[error("launch intent world lock does not match the selected frozen lock")]
    WorldLockMismatch,
    /// The world-open plan differs from the read-only preflight result.
    #[error("launch intent open-plan hash does not match the selected preflight plan")]
    WorldOpenPlanMismatch,
}

/// Stable operation category for atomic intent storage.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum StoreOperation {
    /// Inspect the bounded slot.
    Read,
    /// Create or write a temporary file.
    WriteTemporary,
    /// Synchronize temporary file contents.
    SyncTemporary,
    /// Atomically publish or move a slot file.
    Replace,
    /// Synchronize directory metadata.
    SyncDirectory,
    /// Remove a prior terminal slot.
    Retire,
}

/// Failure at the atomic launch-intent storage boundary.
#[derive(Debug, Error)]
pub enum IntentStoreError {
    /// An operating-system file operation failed.
    #[error("launch intent store operation {operation:?} failed: {source}")]
    Io {
        /// Stable operation category.
        operation: StoreOperation,
        /// Operating-system error.
        #[source]
        source: io::Error,
    },
    /// The configured root was not a literal, non-symlink directory.
    #[error("launch intent store root is not a literal non-symlink directory")]
    UnsafeRoot,
    /// A slot path was a symlink, reparse escape, or non-regular file.
    #[error("launch intent store slot `{slot}` is not a confined regular file")]
    UnsafeSlot {
        /// Stable slot label, not an arbitrary filesystem path.
        slot: &'static str,
    },
    /// More than one durable slot state existed simultaneously.
    #[error("launch intent store contains conflicting slot states")]
    ConflictingSlots,
    /// A slot payload exceeded the bounded read ceiling.
    #[error("launch intent slot has {actual_bytes} bytes; maximum is {maximum_bytes}")]
    SlotTooLarge {
        /// Reported file size.
        actual_bytes: u64,
        /// Fixed maximum.
        maximum_bytes: u64,
    },
    /// An operation required a different slot state.
    #[error("launch intent store expected {expected}, found {actual}")]
    UnexpectedState {
        /// Required state.
        expected: &'static str,
        /// Actual state.
        actual: &'static str,
    },
    /// A compare-before-move digest did not match.
    #[error("launch intent slot changed before atomic transition")]
    BlobMismatch,
    /// A different intent already occupies the bounded slot.
    #[error("launch intent slot is occupied by different canonical bytes")]
    Occupied,
}

impl IntentStoreError {
    pub(crate) fn io(operation: StoreOperation, source: io::Error) -> Self {
        Self::Io { operation, source }
    }
}
