//! Deterministic bounded queues for discardable derived work.
#![allow(
    clippy::expect_used,
    reason = "private queue indexes are mutated together and each expectation states the invariant"
)]

use std::collections::{BTreeMap, BTreeSet};

use latticeaxiom_storage::ChunkCoordinate;

use crate::{
    BackpressureReason, CancellationRequest, CancellationToken, DerivedJobId, DerivedJobKey,
    DerivedOwner, DerivedPriority, DerivedQueueLimits, DerivedTicket, EnqueueDecision, FixedTick,
    QueueDiagnostics, RuntimeGeneration, WorldEpoch,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct PendingJob {
    pub(crate) key: DerivedJobKey,
    pub(crate) priority: DerivedPriority,
    pub(crate) owner: DerivedOwner,
    pub(crate) input_bytes: u64,
    pub(crate) result_bytes: u64,
    pub(crate) apply_bytes: u64,
    pub(crate) reserved_bytes: u64,
    pub(crate) projected_tick: FixedTick,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct QueueOrderKey {
    priority: DerivedPriority,
    coordinate: ChunkCoordinate,
}

impl QueueOrderKey {
    const fn new(priority: DerivedPriority, coordinate: ChunkCoordinate) -> Self {
        Self {
            priority,
            coordinate,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum JobState {
    Active,
    CancelRequested,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct InFlightJob {
    ticket: DerivedTicket,
    state: JobState,
}

#[derive(Debug)]
pub(super) struct DerivedQueue {
    limits: DerivedQueueLimits,
    pending: BTreeMap<ChunkCoordinate, PendingJob>,
    order: BTreeSet<QueueOrderKey>,
    in_flight: BTreeMap<DerivedJobId, InFlightJob>,
    in_flight_targets: BTreeSet<ChunkCoordinate>,
    diagnostics: QueueDiagnostics,
}

impl DerivedQueue {
    pub(crate) fn new(limits: DerivedQueueLimits) -> Self {
        Self {
            limits,
            pending: BTreeMap::new(),
            order: BTreeSet::new(),
            in_flight: BTreeMap::new(),
            in_flight_targets: BTreeSet::new(),
            diagnostics: QueueDiagnostics::default(),
        }
    }

    pub(crate) fn enqueue(&mut self, pending: PendingJob) -> EnqueueDecision {
        if pending.reserved_bytes > self.limits.max_reserved_bytes() {
            self.mark_rejected_memory_budget();
            return EnqueueDecision::RejectedMemoryBudget;
        }

        let coordinate = pending.key.coordinate();
        if self.pending.get(&coordinate) == Some(&pending)
            || self.in_flight.values().any(|job| {
                job.ticket.key == pending.key
                    && job.ticket.owner == pending.owner
                    && job.ticket.input_bytes == pending.input_bytes
                    && job.ticket.result_byte_budget == pending.result_bytes
                    && job.ticket.apply_byte_budget == pending.apply_bytes
            })
        {
            self.diagnostics.already_queued = self.diagnostics.already_queued.saturating_add(1);
            return EnqueueDecision::AlreadyQueued;
        }

        if let Some(previous) = self.pending.get(&coordinate) {
            let removed = self.order.remove(&QueueOrderKey::new(
                previous.priority,
                previous.key.coordinate(),
            ));
            debug_assert!(removed, "pending queue and stable order must agree");
            self.pending.insert(coordinate, pending.clone());
            let inserted = self
                .order
                .insert(QueueOrderKey::new(pending.priority, coordinate));
            debug_assert!(inserted, "replacement order key must be unique");
            self.diagnostics.replaced = self.diagnostics.replaced.saturating_add(1);
            self.refresh_current();
            return EnqueueDecision::Replaced;
        }

        if self.pending.len() >= self.limits.max_pending() {
            self.diagnostics.rejected_capacity =
                self.diagnostics.rejected_capacity.saturating_add(1);
            return EnqueueDecision::RejectedCapacity;
        }

        let inserted = self
            .order
            .insert(QueueOrderKey::new(pending.priority, coordinate));
        debug_assert!(inserted, "new target must have a unique stable order key");
        self.pending.insert(coordinate, pending);
        self.diagnostics.enqueued = self.diagnostics.enqueued.saturating_add(1);
        self.refresh_current();
        EnqueueDecision::Enqueued
    }

    pub(crate) fn reject_pending_memory_contract(&mut self, key: &DerivedJobKey) -> bool {
        let coordinate = key.coordinate();
        let Some(pending) = self.pending.get(&coordinate) else {
            return false;
        };
        if pending.key != *key {
            return false;
        }
        let pending = self
            .pending
            .remove(&coordinate)
            .expect("validated pending job must remain present");
        let removed = self
            .order
            .remove(&QueueOrderKey::new(pending.priority, coordinate));
        debug_assert!(removed, "pending target index must agree with stable order");
        self.mark_memory_contract_violation();
        self.refresh_current();
        true
    }

    pub(crate) fn next_candidate(
        &self,
        global_available_bytes: u64,
    ) -> Result<PendingJob, Option<BackpressureReason>> {
        if self.pending.is_empty() {
            return Err(None);
        }
        if self.in_flight.len() >= self.limits.max_in_flight() {
            return Err(Some(BackpressureReason::InFlightJobs));
        }

        let kind_available = self
            .limits
            .max_reserved_bytes()
            .saturating_sub(self.diagnostics.reserved_bytes);
        let mut saw_target = false;
        let mut saw_kind_bytes = false;
        let mut saw_combined_bytes = false;
        for order_key in &self.order {
            if self.in_flight_targets.contains(&order_key.coordinate) {
                saw_target = true;
                continue;
            }
            let pending = self
                .pending
                .get(&order_key.coordinate)
                .expect("stable order entries must reference pending jobs");
            if pending.reserved_bytes > kind_available {
                saw_kind_bytes = true;
                continue;
            }
            if pending.reserved_bytes > global_available_bytes {
                saw_combined_bytes = true;
                continue;
            }
            return Ok(pending.clone());
        }

        if saw_kind_bytes {
            Err(Some(BackpressureReason::KindReservedBytes))
        } else if saw_combined_bytes {
            Err(Some(BackpressureReason::CombinedReservedBytes))
        } else {
            debug_assert!(saw_target, "nonempty queue must report a limiting resource");
            Err(Some(BackpressureReason::TargetInFlight))
        }
    }

    pub(crate) fn start(
        &mut self,
        pending: &PendingJob,
        id: DerivedJobId,
        epoch: WorldEpoch,
        runtime_generation: RuntimeGeneration,
    ) -> DerivedTicket {
        let coordinate = pending.key.coordinate();
        let removed = self
            .pending
            .remove(&coordinate)
            .expect("selected pending candidate must remain queued until start");
        debug_assert_eq!(
            &removed, pending,
            "selected pending candidate must not change"
        );
        let removed_order = self
            .order
            .remove(&QueueOrderKey::new(pending.priority, coordinate));
        debug_assert!(removed_order, "selected order key must remain queued");

        let ticket = DerivedTicket {
            id,
            owner: pending.owner,
            cancel_token: CancellationToken::new(epoch, runtime_generation, id),
            key: pending.key.clone(),
            input_bytes: pending.input_bytes,
            result_byte_budget: pending.result_bytes,
            apply_byte_budget: pending.apply_bytes,
            reserved_bytes: pending.reserved_bytes,
            projected_tick: pending.projected_tick,
        };
        let previous = self.in_flight.insert(
            id,
            InFlightJob {
                ticket: ticket.clone(),
                state: JobState::Active,
            },
        );
        debug_assert!(previous.is_none(), "checked derived job IDs must be unique");
        let inserted_target = self.in_flight_targets.insert(coordinate);
        debug_assert!(
            inserted_target,
            "one target may have only one job in flight per kind"
        );
        self.diagnostics.reserved_bytes = self
            .diagnostics
            .reserved_bytes
            .checked_add(pending.reserved_bytes)
            .expect("candidate admission proved the per-kind reservation fits");
        self.diagnostics.started = self.diagnostics.started.saturating_add(1);
        self.refresh_current();
        ticket
    }

    pub(crate) fn state(&mut self, ticket: &DerivedTicket) -> Option<JobState> {
        let Some(job) = self.in_flight.get(&ticket.id) else {
            self.mark_unknown_completion();
            return None;
        };
        if job.ticket != *ticket {
            self.mark_unknown_completion();
            return None;
        }
        Some(job.state)
    }

    pub(crate) fn request_cancel_target(
        &mut self,
        coordinate: ChunkCoordinate,
    ) -> Vec<CancellationRequest> {
        if let Some(pending) = self.pending.remove(&coordinate) {
            let removed = self
                .order
                .remove(&QueueOrderKey::new(pending.priority, coordinate));
            debug_assert!(removed, "pending target index must agree with stable order");
        }

        let mut requests = Vec::new();
        for job in self.in_flight.values_mut() {
            if job.ticket.key.coordinate() == coordinate && job.state == JobState::Active {
                job.state = JobState::CancelRequested;
                requests.push(CancellationRequest::new(&job.ticket));
            }
        }
        self.diagnostics.cancel_requests = self
            .diagnostics
            .cancel_requests
            .saturating_add(u64::try_from(requests.len()).unwrap_or(u64::MAX));
        self.refresh_current();
        requests
    }

    pub(crate) fn release(&mut self, ticket: &DerivedTicket) -> Option<u64> {
        if self.in_flight.get(&ticket.id).map(|job| &job.ticket) != Some(ticket) {
            self.mark_unknown_completion();
            return None;
        }
        let removed = self
            .in_flight
            .remove(&ticket.id)
            .expect("validated in-flight ticket must remain present");
        let removed_target = self
            .in_flight_targets
            .remove(&removed.ticket.key.coordinate());
        debug_assert!(
            removed_target,
            "in-flight target index must agree with tickets"
        );
        self.diagnostics.reserved_bytes = self
            .diagnostics
            .reserved_bytes
            .checked_sub(removed.ticket.reserved_bytes)
            .expect("released ticket reservation must remain accounted");
        self.refresh_current();
        Some(removed.ticket.reserved_bytes)
    }

    pub(crate) const fn mark_rejected_memory_budget(&mut self) {
        self.diagnostics.rejected_memory_budget =
            self.diagnostics.rejected_memory_budget.saturating_add(1);
    }

    pub(crate) const fn mark_cancel_acknowledged(&mut self) {
        self.diagnostics.cancel_acknowledged =
            self.diagnostics.cancel_acknowledged.saturating_add(1);
    }

    pub(crate) const fn mark_applied(&mut self) {
        self.diagnostics.applied = self.diagnostics.applied.saturating_add(1);
    }

    pub(crate) const fn mark_apply_failed(&mut self) {
        self.diagnostics.apply_failed = self.diagnostics.apply_failed.saturating_add(1);
    }

    pub(crate) const fn mark_apply_panicked(&mut self) {
        self.diagnostics.apply_panicked = self.diagnostics.apply_panicked.saturating_add(1);
    }

    pub(crate) const fn mark_stale_rejected(&mut self) {
        self.diagnostics.stale_rejected = self.diagnostics.stale_rejected.saturating_add(1);
    }

    pub(crate) const fn mark_unknown_completion(&mut self) {
        self.diagnostics.unknown_completion = self.diagnostics.unknown_completion.saturating_add(1);
    }

    pub(crate) const fn mark_memory_contract_violation(&mut self) {
        self.diagnostics.memory_contract_violations = self
            .diagnostics
            .memory_contract_violations
            .saturating_add(1);
    }

    pub(crate) const fn diagnostics(&self) -> QueueDiagnostics {
        self.diagnostics
    }

    fn refresh_current(&mut self) {
        self.diagnostics.pending = self.pending.len();
        self.diagnostics.in_flight = self.in_flight.len();
        self.diagnostics.cancel_requested = self
            .in_flight
            .values()
            .filter(|job| job.state == JobState::CancelRequested)
            .count();
        self.diagnostics.pending_high_water = self
            .diagnostics
            .pending_high_water
            .max(self.diagnostics.pending);
        self.diagnostics.in_flight_high_water = self
            .diagnostics
            .in_flight_high_water
            .max(self.diagnostics.in_flight);
        self.diagnostics.reserved_bytes_high_water = self
            .diagnostics
            .reserved_bytes_high_water
            .max(self.diagnostics.reserved_bytes);
    }
}
