//! Bounded counters and snapshots for committed projections and derived work.

/// Observable counters for one derived queue kind.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct QueueDiagnostics {
    pub(crate) pending: usize,
    pub(crate) in_flight: usize,
    pub(crate) cancel_requested: usize,
    pub(crate) reserved_bytes: u64,
    pub(crate) pending_high_water: usize,
    pub(crate) in_flight_high_water: usize,
    pub(crate) reserved_bytes_high_water: u64,
    pub(crate) enqueued: u64,
    pub(crate) replaced: u64,
    pub(crate) already_queued: u64,
    pub(crate) rejected_capacity: u64,
    pub(crate) rejected_memory_budget: u64,
    pub(crate) started: u64,
    pub(crate) cancel_requests: u64,
    pub(crate) cancel_acknowledged: u64,
    pub(crate) applied: u64,
    pub(crate) apply_failed: u64,
    pub(crate) apply_panicked: u64,
    pub(crate) stale_rejected: u64,
    pub(crate) unknown_completion: u64,
    pub(crate) memory_contract_violations: u64,
}

impl QueueDiagnostics {
    /// Current pending job count.
    #[must_use]
    pub const fn pending(self) -> usize {
        self.pending
    }
    /// Current active and cancel-requested job count.
    #[must_use]
    pub const fn in_flight(self) -> usize {
        self.in_flight
    }
    /// Jobs whose owners have not yet acknowledged cancellation.
    #[must_use]
    pub const fn cancel_requested(self) -> usize {
        self.cancel_requested
    }
    /// Combined input, result, and apply reservations still held.
    #[must_use]
    pub const fn reserved_bytes(self) -> u64 {
        self.reserved_bytes
    }
    /// Maximum observed pending count.
    #[must_use]
    pub const fn pending_high_water(self) -> usize {
        self.pending_high_water
    }
    /// Maximum observed in-flight count.
    #[must_use]
    pub const fn in_flight_high_water(self) -> usize {
        self.in_flight_high_water
    }
    /// Maximum observed combined reservation.
    #[must_use]
    pub const fn reserved_bytes_high_water(self) -> u64 {
        self.reserved_bytes_high_water
    }
    /// Jobs inserted into an empty target slot.
    #[must_use]
    pub const fn enqueued(self) -> u64 {
        self.enqueued
    }
    /// Pending jobs replaced for the same target.
    #[must_use]
    pub const fn replaced(self) -> u64 {
        self.replaced
    }
    /// Requests already represented by equal pending or in-flight work.
    #[must_use]
    pub const fn already_queued(self) -> u64 {
        self.already_queued
    }
    /// Requests rejected by pending capacity.
    #[must_use]
    pub const fn rejected_capacity(self) -> u64 {
        self.rejected_capacity
    }
    /// Requests rejected by their declared combined memory reservation.
    #[must_use]
    pub const fn rejected_memory_budget(self) -> u64 {
        self.rejected_memory_budget
    }
    /// Jobs moved into the in-flight set.
    #[must_use]
    pub const fn started(self) -> u64 {
        self.started
    }
    /// Cancellation requests issued while resources remained reserved.
    #[must_use]
    pub const fn cancel_requests(self) -> u64 {
        self.cancel_requests
    }
    /// Cancellation acknowledgements that released resources.
    #[must_use]
    pub const fn cancel_acknowledged(self) -> u64 {
        self.cancel_acknowledged
    }
    /// Successful applies.
    #[must_use]
    pub const fn applied(self) -> u64 {
        self.applied
    }
    /// Typed apply failures.
    #[must_use]
    pub const fn apply_failed(self) -> u64 {
        self.apply_failed
    }
    /// Contained apply panics.
    #[must_use]
    pub const fn apply_panicked(self) -> u64 {
        self.apply_panicked
    }
    /// Known completions rejected as stale.
    #[must_use]
    pub const fn stale_rejected(self) -> u64 {
        self.stale_rejected
    }
    /// Completions or acknowledgements that did not match an owned ticket.
    #[must_use]
    pub const fn unknown_completion(self) -> u64 {
        self.unknown_completion
    }
    /// Retained-byte declarations that under-reported actual memory.
    #[must_use]
    pub const fn memory_contract_violations(self) -> u64 {
        self.memory_contract_violations
    }
}

/// Working-set diagnostics independent of renderer and physics backends.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RuntimeDiagnostics {
    pub(crate) resident_chunks: usize,
    pub(crate) resident_high_water: usize,
    pub(crate) dirty_chunks: usize,
    pub(crate) active_chunks: usize,
    pub(crate) visible_chunks: usize,
    pub(crate) in_flight_chunks: usize,
    pub(crate) saving_chunks: usize,
    pub(crate) projected_commits: u64,
    pub(crate) projection_replays: u64,
    pub(crate) evictions: u64,
    pub(crate) combined_reserved_bytes: u64,
    pub(crate) combined_reserved_bytes_high_water: u64,
    pub(crate) byte_budget: u64,
    pub(crate) last_commit_to_apply_ticks: Option<u64>,
    pub(crate) max_commit_to_apply_ticks: u64,
    pub(crate) conservative_colliders: usize,
    pub(crate) waiting_to_apply_jobs: usize,
    pub(crate) waiting_to_apply_bytes: u64,
    pub(crate) waiting_to_apply_bytes_high_water: u64,
    pub(crate) mesh: QueueDiagnostics,
    pub(crate) collider: QueueDiagnostics,
}

impl RuntimeDiagnostics {
    /// Current committed projections resident in this cache.
    #[must_use]
    pub const fn resident_chunks(self) -> usize {
        self.resident_chunks
    }
    /// Maximum resident committed projections.
    #[must_use]
    pub const fn resident_high_water(self) -> usize {
        self.resident_high_water
    }
    /// Resident projections pinned because they were edited.
    #[must_use]
    pub const fn dirty_chunks(self) -> usize {
        self.dirty_chunks
    }
    /// Resident projections with both mesh and collider last-applied keys.
    #[must_use]
    pub const fn active_chunks(self) -> usize {
        self.active_chunks
    }
    /// Resident projections with a last-applied mesh key.
    #[must_use]
    pub const fn visible_chunks(self) -> usize {
        self.visible_chunks
    }
    /// Combined mesh and collider jobs currently in flight.
    #[must_use]
    pub const fn in_flight_chunks(self) -> usize {
        self.in_flight_chunks
    }
    /// Chunks currently being written to durable storage.
    ///
    /// Always zero: this runtime is not a persistence authority and does not
    /// open a world writer.
    #[must_use]
    pub const fn saving_chunks(self) -> usize {
        self.saving_chunks
    }
    /// Newly admitted or updated storage commits.
    #[must_use]
    pub const fn projected_commits(self) -> u64 {
        self.projected_commits
    }
    /// Idempotent projection replays.
    #[must_use]
    pub const fn projection_replays(self) -> u64 {
        self.projection_replays
    }
    /// Clean, permitted projection evictions.
    #[must_use]
    pub const fn evictions(self) -> u64 {
        self.evictions
    }
    /// Combined reservations held across mesh and collider queues.
    #[must_use]
    pub const fn combined_reserved_bytes(self) -> u64 {
        self.combined_reserved_bytes
    }
    /// Maximum combined reservation held across both kinds.
    #[must_use]
    pub const fn combined_reserved_bytes_high_water(self) -> u64 {
        self.combined_reserved_bytes_high_water
    }
    /// Cross-kind combined reservation hard limit.
    #[must_use]
    pub const fn byte_budget(self) -> u64 {
        self.byte_budget
    }
    /// Most recent projection-to-successful-apply latency in fixed ticks.
    #[must_use]
    pub const fn last_commit_to_apply_ticks(self) -> Option<u64> {
        self.last_commit_to_apply_ticks
    }
    /// Maximum projection-to-successful-apply latency in fixed ticks.
    #[must_use]
    pub const fn max_commit_to_apply_ticks(self) -> u64 {
        self.max_commit_to_apply_ticks
    }
    /// Resident colliders requiring synchronous projected-occupancy fallback.
    #[must_use]
    pub const fn conservative_colliders(self) -> usize {
        self.conservative_colliders
    }
    /// Receipt-checked results waiting for host presentation apply.
    #[must_use]
    pub const fn waiting_to_apply_jobs(self) -> usize {
        self.waiting_to_apply_jobs
    }
    /// Measured result bytes waiting for host presentation apply.
    #[must_use]
    pub const fn waiting_to_apply_bytes(self) -> u64 {
        self.waiting_to_apply_bytes
    }
    /// Maximum observed waiting-to-apply result bytes.
    #[must_use]
    pub const fn waiting_to_apply_bytes_high_water(self) -> u64 {
        self.waiting_to_apply_bytes_high_water
    }
    /// Mesh queue diagnostics.
    #[must_use]
    pub const fn mesh(self) -> QueueDiagnostics {
        self.mesh
    }
    /// Collider queue diagnostics.
    #[must_use]
    pub const fn collider(self) -> QueueDiagnostics {
        self.collider
    }
}
