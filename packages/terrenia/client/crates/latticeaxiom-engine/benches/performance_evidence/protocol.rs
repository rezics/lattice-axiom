//! Bounded tick protocol for smoke and the 10-minute traversal.

use crate::observation::HarnessMode;

/// Authoritative fixed rate from ADR 0026.
pub const FIXED_HZ: u32 = 60;
/// ADR 0026 warmup: 2 minutes at 60 Hz.
pub const TRAVERSAL_WARMUP_TICKS: u32 = 7_200;
/// ADR 0026 measure: 10 minutes at 60 Hz.
pub const TRAVERSAL_MEASURE_TICKS: u32 = 36_000;
/// Short local verification warmup.
pub const SMOKE_WARMUP_TICKS: u32 = 16;
/// Short local verification measure window.
pub const SMOKE_MEASURE_TICKS: u32 = 128;
/// Smoke wall-clock safety cap.
pub const SMOKE_MAX_WALL_SECONDS: u32 = 90;
/// Traversal wall-clock safety cap. This bounds the process; it is not a budget.
pub const TRAVERSAL_MAX_WALL_SECONDS: u32 = 3_600;
/// Headless action frames kept below [`latticeaxiom_player::MAX_HEADLESS_ACTION_FRAMES`].
pub const ACTION_BATCH: u32 = 256;
/// Fixed-tick advances issued together during warmup only.
pub const WARMUP_ADVANCE_BATCH: u32 = 32;

/// Resolved, still-bounded tick plan.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TickPlan {
    /// Warmup ticks discarded from histograms.
    pub warmup_ticks: u32,
    /// Measure ticks retained in histograms.
    pub measure_ticks: u32,
    /// Process wall-clock cap.
    pub max_wall_seconds: u32,
}

impl TickPlan {
    /// Default plan for a harness mode.
    #[must_use]
    pub const fn for_mode(mode: HarnessMode) -> Self {
        match mode {
            HarnessMode::Smoke => Self {
                warmup_ticks: SMOKE_WARMUP_TICKS,
                measure_ticks: SMOKE_MEASURE_TICKS,
                max_wall_seconds: SMOKE_MAX_WALL_SECONDS,
            },
            HarnessMode::Traversal => Self {
                warmup_ticks: TRAVERSAL_WARMUP_TICKS,
                measure_ticks: TRAVERSAL_MEASURE_TICKS,
                max_wall_seconds: TRAVERSAL_MAX_WALL_SECONDS,
            },
        }
    }

    /// Rejects a plan that would exceed the frozen 2 + 10 minute protocol.
    #[must_use]
    pub const fn is_within_hard_bound(self) -> bool {
        self.warmup_ticks <= TRAVERSAL_WARMUP_TICKS && self.measure_ticks <= TRAVERSAL_MEASURE_TICKS
    }
}

/// One path phase executed with a bounded action batch.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PathPhase {
    /// Forward walk with occasional jumps.
    WalkForward,
    /// π-radian look then continue forward.
    Turn180,
    /// Break-block attempts.
    EditBurst,
    /// Reverse walk toward the spawn hemisphere.
    Return,
}

/// Splits the measure window across the ADR 0026 path events.
#[must_use]
pub fn measure_phases(measure_ticks: u32) -> [(PathPhase, u32); 4] {
    if measure_ticks == 0 {
        return [
            (PathPhase::WalkForward, 0),
            (PathPhase::Turn180, 0),
            (PathPhase::EditBurst, 0),
            (PathPhase::Return, 0),
        ];
    }
    let turn = measure_ticks.min(8);
    let after_turn = measure_ticks.saturating_sub(turn);
    let edit = after_turn.min(16);
    let after_edit = after_turn.saturating_sub(edit);
    let return_ticks = after_edit / 2;
    let forward = after_edit.saturating_sub(return_ticks);
    [
        (PathPhase::WalkForward, forward),
        (PathPhase::Turn180, turn),
        (PathPhase::EditBurst, edit),
        (PathPhase::Return, return_ticks),
    ]
}
