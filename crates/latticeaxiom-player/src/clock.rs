//! Validated simulation-clock control for Bevy's fixed schedule.

use std::{num::NonZeroU16, time::Duration};

use bevy::prelude::{Message, Resource};
use thiserror::Error;

/// Default authoritative simulation frequency in hertz.
pub const DEFAULT_SIMULATION_TICK_RATE_HZ: u16 = 60;
/// Lowest supported real-time simulation frequency in hertz.
pub const MIN_SIMULATION_TICK_RATE_HZ: u16 = 1;
/// Highest supported developer simulation frequency in hertz.
pub const MAX_SIMULATION_TICK_RATE_HZ: u16 = 10_000;

/// A simulation frequency proved to be inside the supported domain.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct SimulationTickRate(NonZeroU16);

impl SimulationTickRate {
    /// Validates a requested simulation frequency.
    ///
    /// # Errors
    ///
    /// Returns [`SimulationTickRateError`] when `hertz` is outside
    /// `1..=10_000`.
    pub const fn new(hertz: u16) -> Result<Self, SimulationTickRateError> {
        if hertz < MIN_SIMULATION_TICK_RATE_HZ || hertz > MAX_SIMULATION_TICK_RATE_HZ {
            return Err(SimulationTickRateError::OutsideSupportedRange {
                requested: hertz,
                minimum: MIN_SIMULATION_TICK_RATE_HZ,
                maximum: MAX_SIMULATION_TICK_RATE_HZ,
            });
        }
        match NonZeroU16::new(hertz) {
            Some(nonzero) => Ok(Self(nonzero)),
            None => Err(SimulationTickRateError::OutsideSupportedRange {
                requested: hertz,
                minimum: MIN_SIMULATION_TICK_RATE_HZ,
                maximum: MAX_SIMULATION_TICK_RATE_HZ,
            }),
        }
    }

    /// Returns the validated frequency in hertz.
    #[must_use]
    pub const fn hertz(self) -> u16 {
        self.0.get()
    }

    /// Returns the exact nanosecond-resolution Bevy fixed timestep.
    #[must_use]
    pub fn timestep(self) -> Duration {
        Duration::from_secs_f64(1.0 / f64::from(self.hertz()))
    }
}

impl Default for SimulationTickRate {
    fn default() -> Self {
        // The constant is inside the checked public construction domain.
        match Self::new(DEFAULT_SIMULATION_TICK_RATE_HZ) {
            Ok(rate) => rate,
            Err(_) => unreachable!("the default simulation tick rate is compile-time valid"),
        }
    }
}

/// Rejection returned before an unproved tick rate reaches Bevy time.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum SimulationTickRateError {
    /// The requested frequency is outside the supported clock domain.
    #[error("simulation tick rate {requested} Hz is outside {minimum}..={maximum} Hz")]
    OutsideSupportedRange {
        /// Untrusted requested frequency.
        requested: u16,
        /// Inclusive supported minimum.
        minimum: u16,
        /// Inclusive supported maximum.
        maximum: u16,
    },
}

/// Active authoritative fixed-clock configuration.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Resource)]
pub struct SimulationClock {
    active_rate: SimulationTickRate,
    revision: u64,
}

impl SimulationClock {
    /// Creates a clock at a validated frequency.
    #[must_use]
    pub const fn new(active_rate: SimulationTickRate) -> Self {
        Self {
            active_rate,
            revision: 0,
        }
    }

    /// Returns the currently active simulation frequency.
    #[must_use]
    pub const fn active_rate(self) -> SimulationTickRate {
        self.active_rate
    }

    /// Returns the monotonic clock-configuration revision.
    #[must_use]
    pub const fn revision(self) -> u64 {
        self.revision
    }

    pub(crate) fn activate(&mut self, rate: SimulationTickRate) -> bool {
        if self.active_rate == rate {
            return false;
        }
        self.active_rate = rate;
        self.revision = self.revision.saturating_add(1);
        true
    }
}

impl Default for SimulationClock {
    fn default() -> Self {
        Self::new(SimulationTickRate::default())
    }
}

/// Request to change the fixed timestep at the next fixed-schedule boundary.
#[derive(Clone, Copy, Debug, Eq, Message, PartialEq)]
pub struct SimulationTickRateRequest {
    /// Caller-owned correlation identity.
    pub request_id: u64,
    /// Already validated target frequency.
    pub rate: SimulationTickRate,
}

/// Receipt proving which clock revision became active.
#[derive(Clone, Copy, Debug, Eq, Message, PartialEq)]
pub struct SimulationTickRateChanged {
    /// Correlation identity from the accepted request.
    pub request_id: u64,
    /// Active validated frequency.
    pub rate: SimulationTickRate,
    /// Monotonic clock-configuration revision.
    pub revision: u64,
    /// Whether this request changed the active frequency.
    pub changed: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tick_rate_rejects_values_outside_the_supported_domain() {
        assert!(SimulationTickRate::new(0).is_err());
        assert!(SimulationTickRate::new(10_001).is_err());
        assert_eq!(
            SimulationTickRate::new(60).map(SimulationTickRate::hertz),
            Ok(60)
        );
    }

    #[test]
    fn tick_rate_produces_the_expected_fixed_timestep() {
        let rate = SimulationTickRate::new(240).unwrap_or_default();
        assert_eq!(rate.timestep(), Duration::from_nanos(4_166_667));
    }
}
