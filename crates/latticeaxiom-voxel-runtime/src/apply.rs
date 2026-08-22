//! Bounded main-world apply admission and executor abort contracts.
//!
//! Dispatch still uses [`crate::VoxelRuntime::dispatch_next`]. This module owns
//! the P3/V4 completion-apply slice: job count, waiting-to-apply bytes, and a
//! host-supplied wall-clock so Bevy can stop after ADR 0026's 2 ms budget
//! without the runtime sampling a clock.

use crate::{
    CompletionOutcome, DerivedInput, DerivedJobId, DerivedTicket, RuntimeError, RuntimeLimits,
    RuntimeResult,
};

/// Host-supplied monotonic elapsed time for one main-world apply slice.
///
/// The runtime never reads a clock. Hosts pass Bevy frame time or a test
/// sequence so admission stays deterministic.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct WallClockNanos(u64);

impl WallClockNanos {
    /// Creates an elapsed-time value.
    #[must_use]
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    /// Numeric nanosecond value.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }

    /// ADR 0026 main-world apply wall-clock cap.
    #[must_use]
    pub const fn main_world_apply_cap() -> Self {
        Self(RuntimeLimits::MAIN_WORLD_APPLY_WALL_CLOCK_NANOS)
    }
}

/// Hard caps for one main-world derived completion-apply slice.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DerivedApplyBudget {
    job_cap: usize,
    byte_cap: u64,
    wall_clock_nanos: u64,
}

impl DerivedApplyBudget {
    /// Validates apply-slice caps.
    ///
    /// # Errors
    ///
    /// Returns an error when any cap is zero.
    pub const fn new(job_cap: usize, byte_cap: u64, wall_clock_nanos: u64) -> RuntimeResult<Self> {
        if job_cap == 0 {
            return Err(RuntimeError::InvalidLimit { name: "job_cap" });
        }
        if byte_cap == 0 {
            return Err(RuntimeError::InvalidLimit { name: "byte_cap" });
        }
        if wall_clock_nanos == 0 {
            return Err(RuntimeError::InvalidLimit {
                name: "wall_clock_nanos",
            });
        }
        Ok(Self {
            job_cap,
            byte_cap,
            wall_clock_nanos,
        })
    }

    /// ADR 0026 `desktop-reference-v1` main-world apply caps.
    ///
    /// The accepted values are 16 jobs, 16 MiB, and 2 ms per render frame.
    #[must_use]
    pub const fn desktop_reference_v1() -> Self {
        Self {
            job_cap: RuntimeLimits::MAIN_WORLD_APPLY_JOB_CAP,
            byte_cap: RuntimeLimits::MAIN_WORLD_APPLY_BYTE_CAP,
            wall_clock_nanos: RuntimeLimits::MAIN_WORLD_APPLY_WALL_CLOCK_NANOS,
        }
    }

    /// Maximum jobs applied in one slice.
    #[must_use]
    pub const fn max_jobs(self) -> usize {
        self.job_cap
    }

    /// Maximum waiting-to-apply bytes consumed in one slice.
    #[must_use]
    pub const fn max_bytes(self) -> u64 {
        self.byte_cap
    }

    /// Maximum host-supplied elapsed nanoseconds in one slice.
    #[must_use]
    pub const fn max_wall_clock_nanos(self) -> u64 {
        self.wall_clock_nanos
    }
}

/// Progress of one main-world completion-apply slice.
///
/// The first waiting job is always admitted so an oversized payload cannot
/// stall the queue. Later jobs stop on job count, remaining bytes, or elapsed
/// wall-clock, in that order.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DerivedApplySlice {
    applied_jobs: usize,
    applied_bytes: u64,
    elapsed_nanos: u64,
}

/// Why the next waiting job must wait for a later slice.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ApplyAdmission {
    /// The job may be applied now.
    Admit,
    /// [`DerivedApplyBudget::max_jobs`] was reached.
    StopJobs,
    /// Another job would exceed [`DerivedApplyBudget::max_bytes`].
    StopBytes,
    /// Host-supplied elapsed time reached the wall-clock cap.
    StopWallClock,
}

impl ApplyAdmission {
    /// Returns whether the job should be applied in this slice.
    #[must_use]
    pub const fn is_admit(self) -> bool {
        matches!(self, Self::Admit)
    }
}

impl DerivedApplySlice {
    /// Empty slice at the start of a render frame apply pass.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            applied_jobs: 0,
            applied_bytes: 0,
            elapsed_nanos: 0,
        }
    }

    /// Jobs already applied in this slice.
    #[must_use]
    pub const fn applied_jobs(self) -> usize {
        self.applied_jobs
    }

    /// Result bytes already applied in this slice.
    #[must_use]
    pub const fn applied_bytes(self) -> u64 {
        self.applied_bytes
    }

    /// Last observed host-supplied elapsed time.
    #[must_use]
    pub const fn elapsed(self) -> WallClockNanos {
        WallClockNanos::new(self.elapsed_nanos)
    }

    /// Records elapsed time before asking whether the next job fits.
    pub const fn set_elapsed(&mut self, elapsed: WallClockNanos) {
        self.elapsed_nanos = elapsed.get();
    }

    /// Decides whether `next_bytes` may be applied under `budget`.
    #[must_use]
    pub const fn admission(self, next_bytes: u64, budget: DerivedApplyBudget) -> ApplyAdmission {
        if self.applied_jobs == 0 {
            return ApplyAdmission::Admit;
        }
        if self.applied_jobs >= budget.max_jobs() {
            return ApplyAdmission::StopJobs;
        }
        if self.applied_bytes.saturating_add(next_bytes) > budget.max_bytes() {
            return ApplyAdmission::StopBytes;
        }
        if self.elapsed_nanos >= budget.max_wall_clock_nanos() {
            return ApplyAdmission::StopWallClock;
        }
        ApplyAdmission::Admit
    }

    /// Records a successful apply and the host's elapsed time after it.
    pub const fn commit_applied(&mut self, bytes: u64, elapsed: WallClockNanos) {
        self.applied_jobs = self.applied_jobs.saturating_add(1);
        self.applied_bytes = self.applied_bytes.saturating_add(bytes);
        self.elapsed_nanos = elapsed.get();
    }
}

/// Executor-visible terminal result of one derived job before host apply.
///
/// Lost inputs are recovered with [`crate::VoxelRuntime::recover_lost_ticket`]
/// using a ticket cloned before the executor took ownership.
#[derive(Debug)]
pub enum ExecutorOutcome<V, R> {
    /// Worker finished and still owns the input and result.
    Ready {
        /// Owned halo input.
        input: DerivedInput<V>,
        /// Worker result.
        result: R,
    },
    /// Worker observed cancellation and still owns buffers.
    Cancelled {
        /// Owned halo input.
        input: DerivedInput<V>,
        /// Optional partial result.
        result: Option<R>,
    },
    /// Worker panicked but the owned input survived unwinding.
    Panicked {
        /// Owned halo input.
        input: DerivedInput<V>,
    },
}

/// Snapshot of pending, in-flight, and waiting-to-apply ledgers.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DerivedAdmissionSnapshot {
    pub(crate) pending_jobs: usize,
    pub(crate) pending_bytes: u64,
    pub(crate) in_flight_jobs: usize,
    pub(crate) in_flight_bytes: u64,
    pub(crate) waiting_to_apply_jobs: usize,
    pub(crate) waiting_to_apply_bytes: u64,
    pub(crate) cpu_heavy_concurrency: usize,
    pub(crate) combined_in_flight_cap: usize,
    pub(crate) combined_byte_cap: u64,
    pub(crate) at_soft_high_water: bool,
}

impl DerivedAdmissionSnapshot {
    /// Combined pending mesh and collider jobs.
    #[must_use]
    pub const fn pending_jobs(self) -> usize {
        self.pending_jobs
    }

    /// Combined declared reservations of pending jobs.
    #[must_use]
    pub const fn pending_bytes(self) -> u64 {
        self.pending_bytes
    }

    /// Combined in-flight jobs, including cancel-requested work.
    #[must_use]
    pub const fn in_flight_jobs(self) -> usize {
        self.in_flight_jobs
    }

    /// Combined input, result, and apply reservations still held in flight.
    #[must_use]
    pub const fn in_flight_bytes(self) -> u64 {
        self.in_flight_bytes
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

    /// Maximum simultaneous CPU-heavy derived jobs.
    #[must_use]
    pub const fn cpu_heavy_concurrency(self) -> usize {
        self.cpu_heavy_concurrency
    }

    /// Cross-kind in-flight job hard cap.
    #[must_use]
    pub const fn combined_in_flight_cap(self) -> usize {
        self.combined_in_flight_cap
    }

    /// Cross-kind in-flight byte hard cap.
    #[must_use]
    pub const fn combined_byte_cap(self) -> u64 {
        self.combined_byte_cap
    }

    /// Whether any queue or combined ledger reached the 75% soft high-water.
    #[must_use]
    pub const fn at_soft_high_water(self) -> bool {
        self.at_soft_high_water
    }

    /// Remaining CPU-heavy dispatch slots.
    #[must_use]
    pub const fn cpu_heavy_slots_remaining(self) -> usize {
        self.cpu_heavy_concurrency
            .saturating_sub(self.in_flight_jobs)
    }
}

/// Resource release evidence after an executor panic or lost ticket.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WorkerAbortReceipt {
    job: DerivedJobId,
    released_reserved_bytes: u64,
}

impl WorkerAbortReceipt {
    pub(crate) const fn new(job: DerivedJobId, released_reserved_bytes: u64) -> Self {
        Self {
            job,
            released_reserved_bytes,
        }
    }

    /// Aborted job.
    #[must_use]
    pub const fn job(self) -> DerivedJobId {
        self.job
    }

    /// Released combined reservation.
    #[must_use]
    pub const fn released_reserved_bytes(self) -> u64 {
        self.released_reserved_bytes
    }
}

/// Host-facing result of [`crate::VoxelRuntime::complete_executor`].
#[derive(Debug)]
#[allow(
    clippy::large_enum_variant,
    reason = "owned halo input is returned without a hot-path allocation"
)]
pub enum ExecutorFinish<A, E, V, R> {
    /// Apply, stale, cancel, or memory-contract completion.
    Completed(CompletionOutcome<A, E, V, R>),
    /// Worker panic, lost ticket, or unowned cancel-without-result input.
    Aborted(WorkerAbortOutcome<V>),
}

/// Fail-closed abort of in-flight derived work that must not apply.
#[derive(Debug)]
#[allow(
    clippy::large_enum_variant,
    reason = "unknown resources must be returned without allocation"
)]
pub enum WorkerAbortOutcome<V> {
    /// Worker panicked; reservation released; previous derived state unchanged.
    Panicked(WorkerAbortReceipt),
    /// Ticket recovered after the executor dropped its input.
    Lost(WorkerAbortReceipt),
    /// Input was not owned by this runtime.
    UnknownInput {
        /// Returned input.
        input: DerivedInput<V>,
    },
    /// Ticket was not in flight on this runtime.
    UnknownTicket {
        /// Missing job.
        job: DerivedJobId,
        /// Ticket the caller attempted to recover.
        ticket: DerivedTicket,
    },
}

#[cfg(test)]
mod tests {
    use super::{ApplyAdmission, DerivedApplyBudget, DerivedApplySlice, WallClockNanos};
    use crate::RuntimeLimits;

    #[test]
    fn desktop_reference_apply_budget_matches_adr_0026() {
        let budget = DerivedApplyBudget::desktop_reference_v1();
        assert_eq!(budget.max_jobs(), 16);
        assert_eq!(budget.max_bytes(), 16 * 1024 * 1024);
        assert_eq!(budget.max_wall_clock_nanos(), 2_000_000);
        assert_eq!(RuntimeLimits::MAIN_WORLD_APPLY_JOB_CAP, 16);
        assert_eq!(
            RuntimeLimits::MAIN_WORLD_APPLY_WALL_CLOCK_NANOS,
            WallClockNanos::main_world_apply_cap().get()
        );
    }

    #[test]
    fn first_job_is_admitted_even_when_over_budget() {
        let budget = DerivedApplyBudget::desktop_reference_v1();
        let mut slice = DerivedApplySlice::new();
        slice.set_elapsed(WallClockNanos::new(budget.max_wall_clock_nanos()));
        assert_eq!(
            slice.admission(budget.max_bytes().saturating_add(1), budget),
            ApplyAdmission::Admit
        );
    }

    #[test]
    fn later_jobs_stop_on_jobs_bytes_then_wall_clock() {
        let budget = DerivedApplyBudget::new(2, 8, 100).expect("fixture apply budget is nonzero");
        let mut slice = DerivedApplySlice::new();
        slice.commit_applied(4, WallClockNanos::new(10));
        assert_eq!(slice.admission(5, budget), ApplyAdmission::StopBytes);
        slice.commit_applied(3, WallClockNanos::new(100));
        assert_eq!(slice.admission(1, budget), ApplyAdmission::StopJobs);

        let mut wall = DerivedApplySlice::new();
        wall.commit_applied(1, WallClockNanos::new(100));
        assert_eq!(wall.admission(1, budget), ApplyAdmission::StopWallClock);
        assert!(!wall.admission(1, budget).is_admit());
    }

    #[test]
    fn apply_admission_is_deterministic_for_the_same_sequence() {
        let budget = DerivedApplyBudget::desktop_reference_v1();
        let samples = [1_u64, 4 * 1024 * 1024, 16 * 1024 * 1024 + 1];
        let clocks = [
            WallClockNanos::new(0),
            WallClockNanos::new(1_000_000),
            WallClockNanos::new(2_000_000),
            WallClockNanos::new(2_000_001),
        ];
        let mut first = Vec::new();
        let mut slice = DerivedApplySlice::new();
        for bytes in samples {
            for elapsed in clocks {
                slice.set_elapsed(elapsed);
                let decision = slice.admission(bytes, budget);
                first.push(decision);
                if decision.is_admit() {
                    slice.commit_applied(bytes, elapsed);
                }
            }
        }
        let mut second = Vec::new();
        let mut slice = DerivedApplySlice::new();
        for bytes in samples {
            for elapsed in clocks {
                slice.set_elapsed(elapsed);
                let decision = slice.admission(bytes, budget);
                second.push(decision);
                if decision.is_admit() {
                    slice.commit_applied(bytes, elapsed);
                }
            }
        }
        assert_eq!(first, second);
        assert!(slice.applied_jobs() > 0);
        assert!(slice.applied_jobs() <= budget.max_jobs());
        assert!(first.iter().any(|decision| decision.is_admit()));
        assert!(first.iter().any(|decision| !decision.is_admit()));
    }
}
