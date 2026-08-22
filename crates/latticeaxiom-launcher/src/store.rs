//! Atomic single-slot launch-intent storage boundary.

use latticeaxiom_core::CanonicalHash;

use crate::{IntentStoreError, MAX_LAUNCH_INTENT_BYTES};

/// Durable disposition of the single bounded intent slot.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum SlotDisposition {
    /// Canonical bytes are published but have not consumed their one attempt.
    Pending,
    /// A launcher atomically claimed the only target attempt.
    Claimed,
    /// A matching safe-bootstrap acknowledgement was committed.
    Consumed,
    /// Invalid or failed bytes were isolated from automatic target retry.
    Quarantined,
    /// The one allowed recovery-shell attempt was durably claimed.
    RecoveryClaimed,
}

impl SlotDisposition {
    /// Returns the stable diagnostic label.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Claimed => "claimed",
            Self::Consumed => "consumed",
            Self::Quarantined => "quarantined",
            Self::RecoveryClaimed => "recovery-claimed",
        }
    }

    /// Reports whether this state is a terminal predecessor state.
    #[must_use]
    pub const fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Consumed | Self::Quarantined | Self::RecoveryClaimed
        )
    }
}

/// Exact authenticated terminal state retained beside a newer active state.
///
/// A predecessor closes the crash window between publishing a newer pending
/// intent and validating/claiming it. Callers must validate these exact bytes;
/// an untrusted numeric generation alone is not a replay watermark.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TerminalPredecessor {
    disposition: SlotDisposition,
    bytes: Vec<u8>,
    blob_hash: CanonicalHash,
}

impl TerminalPredecessor {
    /// Creates a bounded terminal predecessor observation.
    ///
    /// # Errors
    ///
    /// Returns [`IntentStoreError::UnexpectedState`] for a non-terminal
    /// disposition, or [`IntentStoreError::SlotTooLarge`] above the bound.
    pub fn occupied(
        disposition: SlotDisposition,
        bytes: Vec<u8>,
    ) -> Result<Self, IntentStoreError> {
        if !disposition.is_terminal() {
            return Err(IntentStoreError::UnexpectedState {
                expected: "consumed, quarantined, or recovery-claimed",
                actual: disposition.as_str(),
            });
        }
        check_bound(bytes.len())?;
        let blob_hash = CanonicalHash::digest(&bytes);
        Ok(Self {
            disposition,
            bytes,
            blob_hash,
        })
    }

    /// Returns the durable terminal disposition.
    #[must_use]
    pub const fn disposition(&self) -> SlotDisposition {
        self.disposition
    }

    /// Returns the exact retained bytes.
    #[must_use]
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// Returns the digest of the exact retained bytes.
    #[must_use]
    pub const fn blob_hash(&self) -> CanonicalHash {
        self.blob_hash
    }
}

/// One bounded durable slot observation.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub enum IntentSlot {
    /// No intent or terminal receipt exists.
    #[default]
    Empty,
    /// Exact bytes and their durable disposition.
    Occupied {
        /// Durable state of the bytes.
        disposition: SlotDisposition,
        /// Exact canonical or quarantined bytes, bounded by the read policy.
        bytes: Vec<u8>,
        /// SHA-256 of the exact bytes used for compare-before-move.
        blob_hash: CanonicalHash,
        /// Exact terminal state retained during a no-gap replacement.
        predecessor: Option<TerminalPredecessor>,
    },
}

impl IntentSlot {
    /// Creates a bounded occupied observation and computes its exact-byte hash.
    ///
    /// # Errors
    ///
    /// Returns [`IntentStoreError::SlotTooLarge`] when the byte ceiling is
    /// exceeded.
    pub fn occupied(
        disposition: SlotDisposition,
        bytes: Vec<u8>,
    ) -> Result<Self, IntentStoreError> {
        Self::occupied_with_predecessor(disposition, bytes, None)
    }

    /// Creates a bounded occupied observation with an exact predecessor.
    ///
    /// Pending, claimed, or quarantined state may retain an authenticated
    /// replay watermark. A recovery claim may retain its terminal source until
    /// directory durability is confirmed.
    ///
    /// # Errors
    ///
    /// Returns [`IntentStoreError`] when bytes exceed the bound or the state
    /// and predecessor shape is invalid.
    pub fn occupied_with_predecessor(
        disposition: SlotDisposition,
        bytes: Vec<u8>,
        predecessor: Option<TerminalPredecessor>,
    ) -> Result<Self, IntentStoreError> {
        check_bound(bytes.len())?;
        if predecessor.is_some() {
            let valid = matches!(
                disposition,
                SlotDisposition::Pending
                    | SlotDisposition::Claimed
                    | SlotDisposition::Quarantined
                    | SlotDisposition::RecoveryClaimed
            );
            if !valid {
                return Err(IntentStoreError::UnexpectedState {
                    expected: "pending, claimed, quarantined, or recovery-claimed",
                    actual: disposition.as_str(),
                });
            }
        }
        let blob_hash = CanonicalHash::digest(&bytes);
        Ok(Self::Occupied {
            disposition,
            bytes,
            blob_hash,
            predecessor,
        })
    }

    /// Returns the slot disposition, if occupied.
    #[must_use]
    pub const fn disposition(&self) -> Option<SlotDisposition> {
        match self {
            Self::Empty => None,
            Self::Occupied { disposition, .. } => Some(*disposition),
        }
    }

    /// Returns the exact bytes, if occupied.
    #[must_use]
    pub fn bytes(&self) -> Option<&[u8]> {
        match self {
            Self::Empty => None,
            Self::Occupied { bytes, .. } => Some(bytes),
        }
    }

    /// Returns the exact-byte digest, if occupied.
    #[must_use]
    pub const fn blob_hash(&self) -> Option<CanonicalHash> {
        match self {
            Self::Empty => None,
            Self::Occupied { blob_hash, .. } => Some(*blob_hash),
        }
    }

    /// Returns the exact retained predecessor, if a replacement is in flight.
    #[must_use]
    pub const fn predecessor(&self) -> Option<&TerminalPredecessor> {
        match self {
            Self::Empty => None,
            Self::Occupied { predecessor, .. } => predecessor.as_ref(),
        }
    }
}

/// Durability result of a namespace mutation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MutationDurability {
    /// The destination namespace was synchronized successfully.
    Durable,
    /// The exact committed state was re-read after directory sync failed.
    ///
    /// The caller must treat the mutation as committed and must not retry or
    /// launch a fallback process. Durability across sudden power loss is not
    /// established, so a higher-level supervisor should halt until restart or
    /// platform-specific reconciliation.
    Indeterminate,
}

/// Result of idempotently publishing canonical intent bytes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PublishDisposition {
    /// Bytes were newly published and directory durability was confirmed.
    Published,
    /// Bytes were published and re-read, but directory sync failed.
    PublishedIndeterminate,
    /// The identical pending bytes were already durable.
    AlreadyPublished,
    /// Identical bytes were already claimed, consumed, or quarantined.
    AlreadyHandled(SlotDisposition),
}

/// Result of durably claiming one recovery-shell request.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RecoveryClaimOutcome {
    blob_hash: CanonicalHash,
    durability: MutationDurability,
}

impl RecoveryClaimOutcome {
    /// Creates a recovery claim outcome for exact persisted request bytes.
    #[must_use]
    pub const fn new(blob_hash: CanonicalHash, durability: MutationDurability) -> Self {
        Self {
            blob_hash,
            durability,
        }
    }

    /// Returns the digest of the exact persisted recovery request bytes.
    #[must_use]
    pub const fn blob_hash(self) -> CanonicalHash {
        self.blob_hash
    }

    /// Returns the namespace durability result.
    #[must_use]
    pub const fn durability(self) -> MutationDurability {
        self.durability
    }
}

/// Durable disposition of the single bounded child-exit slot.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum ChildExitDisposition {
    /// Canonical bytes are published but have not been consumed.
    Pending,
    /// The supervisor atomically consumed the one-shot report.
    Consumed,
    /// Invalid or mismatched bytes were isolated from automatic replay.
    Quarantined,
}

impl ChildExitDisposition {
    /// Returns the stable diagnostic label.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Consumed => "consumed",
            Self::Quarantined => "quarantined",
        }
    }
}

/// One bounded durable child-exit observation.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub enum ChildExitSlot {
    /// No child-exit report exists.
    #[default]
    Empty,
    /// Exact bytes and their durable disposition.
    Occupied {
        /// Durable state of the bytes.
        disposition: ChildExitDisposition,
        /// Exact canonical or quarantined bytes, bounded by the read policy.
        bytes: Vec<u8>,
        /// SHA-256 of the exact bytes used for compare-before-move.
        blob_hash: CanonicalHash,
    },
}

impl ChildExitSlot {
    /// Creates a bounded occupied observation and computes its exact-byte hash.
    ///
    /// # Errors
    ///
    /// Returns [`IntentStoreError::SlotTooLarge`] when the byte ceiling is
    /// exceeded.
    pub fn occupied(
        disposition: ChildExitDisposition,
        bytes: Vec<u8>,
    ) -> Result<Self, IntentStoreError> {
        check_bound(bytes.len())?;
        let blob_hash = CanonicalHash::digest(&bytes);
        Ok(Self::Occupied {
            disposition,
            bytes,
            blob_hash,
        })
    }

    /// Returns the slot disposition, if occupied.
    #[must_use]
    pub const fn disposition(&self) -> Option<ChildExitDisposition> {
        match self {
            Self::Empty => None,
            Self::Occupied { disposition, .. } => Some(*disposition),
        }
    }

    /// Returns the exact bytes, if occupied.
    #[must_use]
    pub fn bytes(&self) -> Option<&[u8]> {
        match self {
            Self::Empty => None,
            Self::Occupied { bytes, .. } => Some(bytes),
        }
    }

    /// Returns the exact-byte digest, if occupied.
    #[must_use]
    pub const fn blob_hash(&self) -> Option<CanonicalHash> {
        match self {
            Self::Empty => None,
            Self::Occupied { blob_hash, .. } => Some(*blob_hash),
        }
    }
}

/// Durable single-slot operations for one-shot [`crate::ChildExitReportV1`] bytes.
///
/// `Pending -> Consumed` is the durable one-shot gate that prevents a crash
/// from replaying the same child result. Publishing over `consumed` or
/// `quarantined` is the next child's report, never a second report for the
/// same generation.
pub trait AtomicChildExitStore {
    /// Reads at most [`MAX_LAUNCH_INTENT_BYTES`] from the durable slot.
    ///
    /// # Errors
    ///
    /// Returns [`IntentStoreError`] for I/O, confinement, layout, or bound
    /// failures.
    fn read(&mut self) -> Result<ChildExitSlot, IntentStoreError>;

    /// Atomically publishes exact canonical child-exit bytes.
    ///
    /// # Errors
    ///
    /// Returns [`IntentStoreError`] if another pending blob occupies the slot
    /// or an atomic storage step fails before the destination is observable.
    fn publish(&mut self, canonical_bytes: &[u8]) -> Result<PublishDisposition, IntentStoreError>;

    /// Atomically records successful supervisor consumption.
    ///
    /// # Errors
    ///
    /// Returns [`IntentStoreError`] if the slot is not pending, changed, or
    /// fails before the consumed destination is observable.
    fn consume(
        &mut self,
        expected_blob_hash: CanonicalHash,
    ) -> Result<MutationDurability, IntentStoreError>;

    /// Atomically isolates pending child-exit bytes from automatic replay.
    ///
    /// # Errors
    ///
    /// Returns [`IntentStoreError`] if the slot is not pending, changed, or
    /// fails before the quarantine destination is observable.
    fn quarantine(
        &mut self,
        expected_blob_hash: CanonicalHash,
    ) -> Result<MutationDurability, IntentStoreError>;
}

/// Durable single-slot operations required by the launcher state machine.
///
/// Implementations must make each state change atomic. In particular,
/// `Pending -> Claimed` is the durable one-attempt gate that prevents a crash
/// from spawning the same world process again. `Quarantined ->
/// RecoveryClaimed` (or `Empty -> RecoveryClaimed` for a missing intent) is the
/// independent durable gate that permits at most one recovery-shell spawn.
pub trait AtomicLaunchIntentStore {
    /// Reads at most [`MAX_LAUNCH_INTENT_BYTES`] from the durable slot.
    ///
    /// When a no-gap replacement has not finished cleanup, the newer state is
    /// authoritative and its exact terminal predecessor is returned alongside
    /// it for replay-watermark validation.
    ///
    /// # Errors
    ///
    /// Returns [`IntentStoreError`] for I/O, confinement, layout, or bound
    /// failures.
    fn read(&mut self) -> Result<IntentSlot, IntentStoreError>;

    /// Atomically publishes exact canonical bytes through a temporary write,
    /// file sync, and same-directory move.
    ///
    /// # Errors
    ///
    /// Returns [`IntentStoreError`] if another blob occupies the slot or an
    /// atomic storage step fails before the exact destination is observable.
    fn publish(&mut self, canonical_bytes: &[u8]) -> Result<PublishDisposition, IntentStoreError>;

    /// Publishes a newer pending intent without deleting its exact terminal predecessor.
    ///
    /// The predecessor remains available through [`IntentSlot::predecessor`]
    /// until the pending intent is validated and atomically claimed. An empty
    /// crash window is forbidden.
    ///
    /// # Errors
    ///
    /// Returns [`IntentStoreError`] if the exact terminal payload is absent,
    /// changed, or the fail-safe replacement cannot be completed.
    fn publish_replacing_terminal(
        &mut self,
        expected_terminal_blob_hash: CanonicalHash,
        canonical_bytes: &[u8],
    ) -> Result<PublishDisposition, IntentStoreError>;

    /// Atomically claims the pending blob's only boot attempt.
    ///
    /// A retained terminal predecessor is removed only after the claimed state
    /// is observable. [`MutationDurability::Indeterminate`] is committed for
    /// retry-suppression purposes and must never trigger another spawn.
    ///
    /// # Errors
    ///
    /// Returns [`IntentStoreError`] if the slot is not pending, changed after
    /// validation, or fails before the claimed destination is observable.
    fn claim(
        &mut self,
        expected_blob_hash: CanonicalHash,
    ) -> Result<MutationDurability, IntentStoreError>;

    /// Atomically records successful acknowledgement as consumed.
    ///
    /// # Errors
    ///
    /// Returns [`IntentStoreError`] if the slot is not claimed, changed, or
    /// fails before the consumed destination is observable.
    fn consume(
        &mut self,
        expected_blob_hash: CanonicalHash,
    ) -> Result<MutationDurability, IntentStoreError>;

    /// Atomically isolates pending, claimed, or supervisor-proven exited consumed bytes.
    ///
    /// # Errors
    ///
    /// Returns [`IntentStoreError`] if the slot is not pending/claimed/consumed,
    /// changed, or fails before the quarantine destination is observable.
    fn quarantine(
        &mut self,
        expected_blob_hash: CanonicalHash,
    ) -> Result<MutationDurability, IntentStoreError>;

    /// Atomically claims the one allowed recovery-shell attempt.
    ///
    /// `recovery_request_bytes` must be the caller's bounded canonical,
    /// versioned, self-authenticating recovery request. `Some(hash)` requires
    /// that exact terminal source (normally quarantined or consumed); `None`
    /// requires an empty slot. The return hash always authenticates the
    /// persisted request bytes, never its terminal source.
    ///
    /// A recovered [`MutationDurability::Indeterminate`] still suppresses all
    /// additional recovery spawns.
    ///
    /// # Errors
    ///
    /// Returns [`IntentStoreError`] if the source state or blob differs, the
    /// request exceeds the bound, or publication fails before the exact
    /// recovery request is observable.
    fn claim_recovery(
        &mut self,
        expected_terminal_blob_hash: Option<CanonicalHash>,
        recovery_request_bytes: &[u8],
    ) -> Result<RecoveryClaimOutcome, IntentStoreError>;
}

fn check_bound(actual: usize) -> Result<(), IntentStoreError> {
    if actual > MAX_LAUNCH_INTENT_BYTES {
        return Err(IntentStoreError::SlotTooLarge {
            actual_bytes: u64::try_from(actual).unwrap_or(u64::MAX),
            maximum_bytes: MAX_LAUNCH_INTENT_BYTES as u64,
        });
    }
    Ok(())
}
