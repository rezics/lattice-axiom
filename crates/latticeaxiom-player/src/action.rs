use std::collections::VecDeque;

use bevy::prelude::Resource;
use latticeaxiom_gameplay::BlockId;
use thiserror::Error;

use crate::ClientTargetObservationV1;

/// Maximum number of fixed-tick frames accepted by the headless queue.
pub const MAX_HEADLESS_ACTION_FRAMES: usize = 4_096;

/// Stable version-one logical player action.
///
/// Numeric values are explicit so command fixtures do not inherit enum layout
/// from Leafwing or Bevy.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[repr(u8)]
pub enum PlayerActionV1 {
    /// Two-axis walk input: `+x` right and `+y` forward.
    Move = 1,
    /// Two-axis yaw and pitch delta, in radians.
    Look = 2,
    /// Jump button.
    Jump = 3,
    /// Break the authoritative voxel target.
    BreakBlock = 4,
    /// Place content beside the authoritative voxel target.
    PlaceBlock = 5,
    /// Inspect the current target.
    Inspect = 6,
    /// Open or close the pause surface.
    Pause = 7,
    /// Activate the focused surface control.
    SurfaceActivate = 8,
}

/// Portable finite two-axis action value.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ActionAxis2V1 {
    /// Horizontal component.
    pub x: f32,
    /// Vertical component.
    pub y: f32,
}

impl ActionAxis2V1 {
    /// Creates an axis value, replacing non-finite components with zero.
    #[must_use]
    pub fn finite_or_zero(x: f32, y: f32) -> Self {
        Self {
            x: if x.is_finite() { x } else { 0.0 },
            y: if y.is_finite() { y } else { 0.0 },
        }
    }

    /// Returns a value with magnitude no greater than one.
    #[must_use]
    pub fn clamp_unit(self) -> Self {
        let maximum_component = self.x.abs().max(self.y.abs());
        if maximum_component > 1.0 {
            let scaled_x = self.x / maximum_component;
            let scaled_y = self.y / maximum_component;
            let reciprocal_length = scaled_x.hypot(scaled_y).recip();
            return Self {
                x: scaled_x * reciprocal_length,
                y: scaled_y * reciprocal_length,
            };
        }

        let length_squared = self.x.mul_add(self.x, self.y * self.y);
        if length_squared > 1.0 {
            let reciprocal_length = length_squared.sqrt().recip();
            Self {
                x: self.x * reciprocal_length,
                y: self.y * reciprocal_length,
            }
        } else {
            self
        }
    }
}

/// Compact stable bit set for button-like [`PlayerActionV1`] values.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PlayerActionButtonsV1(u16);

impl PlayerActionButtonsV1 {
    const JUMP: u16 = 1 << 0;
    const BREAK_BLOCK: u16 = 1 << 1;
    const PLACE_BLOCK: u16 = 1 << 2;
    const INSPECT: u16 = 1 << 3;
    const PAUSE: u16 = 1 << 4;
    const SURFACE_ACTIVATE: u16 = 1 << 5;

    /// Returns an empty button set.
    #[must_use]
    pub const fn empty() -> Self {
        Self(0)
    }

    /// Returns whether the supplied button-like action is present.
    #[must_use]
    pub const fn contains(self, action: PlayerActionV1) -> bool {
        let bit = match action {
            PlayerActionV1::Jump => Self::JUMP,
            PlayerActionV1::BreakBlock => Self::BREAK_BLOCK,
            PlayerActionV1::PlaceBlock => Self::PLACE_BLOCK,
            PlayerActionV1::Inspect => Self::INSPECT,
            PlayerActionV1::Pause => Self::PAUSE,
            PlayerActionV1::SurfaceActivate => Self::SURFACE_ACTIVATE,
            PlayerActionV1::Move | PlayerActionV1::Look => return false,
        };
        self.0 & bit != 0
    }

    /// Inserts a button-like action.
    pub fn insert(&mut self, action: PlayerActionV1) {
        let bit = match action {
            PlayerActionV1::Jump => Self::JUMP,
            PlayerActionV1::BreakBlock => Self::BREAK_BLOCK,
            PlayerActionV1::PlaceBlock => Self::PLACE_BLOCK,
            PlayerActionV1::Inspect => Self::INSPECT,
            PlayerActionV1::Pause => Self::PAUSE,
            PlayerActionV1::SurfaceActivate => Self::SURFACE_ACTIVATE,
            PlayerActionV1::Move | PlayerActionV1::Look => return,
        };
        self.0 |= bit;
    }

    /// Returns the union of two button sets.
    #[must_use]
    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }
}

/// Stable version-one action frame consumed once per authoritative fixed tick.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PlayerActionFrameV1 {
    /// Monotonic source generation used to suppress repeated button edges.
    pub generation: u64,
    /// Normalized walk input; `+y` maps to conventional world forward `-Z`.
    pub movement: ActionAxis2V1,
    /// Yaw and pitch delta in radians for this sampled input generation.
    pub look_radians: ActionAxis2V1,
    /// Buttons held in this input generation.
    pub held: PlayerActionButtonsV1,
    /// Rising button edges in this input generation.
    pub started: PlayerActionButtonsV1,
    /// Optional concrete content selected for placement.
    pub placement_content: Option<BlockId>,
    /// Optional presentation observation; authority never trusts it as a hit.
    pub client_observation: Option<ClientTargetObservationV1>,
}

impl PlayerActionFrameV1 {
    /// Returns a sanitized copy suitable for deterministic fixed-tick use.
    #[must_use]
    pub fn sanitized(mut self) -> Self {
        self.movement =
            ActionAxis2V1::finite_or_zero(self.movement.x, self.movement.y).clamp_unit();
        self.look_radians = ActionAxis2V1::finite_or_zero(self.look_radians.x, self.look_radians.y);
        self
    }

    fn without_edges(mut self) -> Self {
        self.started = PlayerActionButtonsV1::empty();
        self.look_radians = ActionAxis2V1::default();
        self
    }
}

/// Bounded action-frame source shared by live input and headless command tests.
///
/// Headless frames take precedence and are consumed one per fixed tick. Live
/// input keeps only the latest axes while preserving unconsumed button edges.
#[derive(Debug, Default, Resource)]
pub struct ActionFrameInbox {
    headless: VecDeque<PlayerActionFrameV1>,
    live: PlayerActionFrameV1,
    live_available: bool,
    live_look_rate_radians_per_second: ActionAxis2V1,
    last_headless_generation: Option<u64>,
    last_live_generation: Option<u64>,
}

impl ActionFrameInbox {
    /// Enqueues one exact fixed-tick frame for headless or replay-style input.
    ///
    /// # Errors
    ///
    /// Returns a typed error if the queue is full or the generation is not
    /// strictly newer than the last queued generation.
    pub fn push_headless(
        &mut self,
        frame: PlayerActionFrameV1,
    ) -> Result<(), ActionFrameInboxError> {
        if self.headless.len() >= MAX_HEADLESS_ACTION_FRAMES {
            return Err(ActionFrameInboxError::QueueFull {
                limit: MAX_HEADLESS_ACTION_FRAMES,
            });
        }
        let previous = self
            .headless
            .back()
            .map(|queued| queued.generation)
            .or(self.last_headless_generation);
        if let Some(previous) = previous
            && frame.generation <= previous
        {
            return Err(ActionFrameInboxError::NonMonotonicGeneration {
                previous,
                actual: frame.generation,
            });
        }
        self.headless.push_back(frame.sanitized());
        Ok(())
    }

    /// Publishes a strictly newer live frame while preserving unconsumed
    /// edges and look deltas from earlier generations.
    pub fn publish_live(&mut self, frame: PlayerActionFrameV1) {
        self.publish_live_with_look_rate(frame, ActionAxis2V1::default());
    }

    pub(crate) fn publish_live_with_look_rate(
        &mut self,
        mut frame: PlayerActionFrameV1,
        look_rate_radians_per_second: ActionAxis2V1,
    ) {
        let generation_floor = self
            .last_live_generation
            .into_iter()
            .chain(self.live_available.then_some(self.live.generation))
            .max();
        if generation_floor.is_some_and(|floor| frame.generation <= floor) {
            return;
        }
        if self.live_available && self.last_live_generation != Some(self.live.generation) {
            frame.started = frame.started.union(self.live.started);
            frame.look_radians.x += self.live.look_radians.x;
            frame.look_radians.y += self.live.look_radians.y;
        }
        self.live = frame.sanitized();
        self.live_look_rate_radians_per_second = ActionAxis2V1::finite_or_zero(
            look_rate_radians_per_second.x,
            look_rate_radians_per_second.y,
        );
        self.live_available = true;
    }

    #[cfg(feature = "client-input")]
    pub(crate) fn replace_live_with_neutral(&mut self, generation: u64) {
        let generation_floor = self
            .last_live_generation
            .into_iter()
            .chain(self.live_available.then_some(self.live.generation))
            .max();
        if generation_floor.is_some_and(|floor| generation < floor) {
            return;
        }
        self.live = PlayerActionFrameV1 {
            generation,
            ..PlayerActionFrameV1::default()
        };
        self.live_look_rate_radians_per_second = ActionAxis2V1::default();
        self.live_available = true;
    }

    /// Returns the next frame for a fixed tick.
    #[must_use]
    pub fn next_fixed_frame(&mut self) -> PlayerActionFrameV1 {
        self.next_fixed_frame_with_step_seconds(0.0)
    }

    pub(crate) fn next_fixed_frame_with_step_seconds(
        &mut self,
        fixed_step_seconds: f32,
    ) -> PlayerActionFrameV1 {
        if let Some(frame) = self.headless.pop_front() {
            self.last_headless_generation = Some(frame.generation);
            return frame;
        }
        if !self.live_available {
            return PlayerActionFrameV1::default();
        }

        let mut frame = self.live.clone();
        let is_repeated = self.last_live_generation == Some(frame.generation);
        self.last_live_generation = Some(frame.generation);
        if is_repeated {
            frame = frame.without_edges();
        }
        let fixed_step_seconds = if fixed_step_seconds.is_finite() && fixed_step_seconds >= 0.0 {
            fixed_step_seconds
        } else {
            0.0
        };
        frame.look_radians.x += self.live_look_rate_radians_per_second.x * fixed_step_seconds;
        frame.look_radians.y += self.live_look_rate_radians_per_second.y * fixed_step_seconds;
        frame.sanitized()
    }

    /// Returns the number of queued exact headless frames.
    #[must_use]
    pub fn queued_headless_frames(&self) -> usize {
        self.headless.len()
    }
}

/// Rejection returned by [`ActionFrameInbox::push_headless`].
#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum ActionFrameInboxError {
    /// The bounded queue has reached its implementation ceiling.
    #[error("headless action queue reached its {limit}-frame limit")]
    QueueFull {
        /// Inclusive queue capacity.
        limit: usize,
    },
    /// A fixture supplied a duplicate or older generation.
    #[error("action generation {actual} is not newer than {previous}")]
    NonMonotonicGeneration {
        /// Most recently queued generation.
        previous: u64,
        /// Rejected generation.
        actual: u64,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn action_discriminants_are_the_frozen_v1_mapping() {
        assert_eq!(PlayerActionV1::Move as u8, 1);
        assert_eq!(PlayerActionV1::Look as u8, 2);
        assert_eq!(PlayerActionV1::Jump as u8, 3);
        assert_eq!(PlayerActionV1::BreakBlock as u8, 4);
        assert_eq!(PlayerActionV1::PlaceBlock as u8, 5);
        assert_eq!(PlayerActionV1::Inspect as u8, 6);
        assert_eq!(PlayerActionV1::Pause as u8, 7);
        assert_eq!(PlayerActionV1::SurfaceActivate as u8, 8);
    }

    #[test]
    fn repeated_live_generation_keeps_axes_but_not_edges() {
        let mut inbox = ActionFrameInbox::default();
        let mut started = PlayerActionButtonsV1::empty();
        started.insert(PlayerActionV1::Jump);
        inbox.publish_live(PlayerActionFrameV1 {
            generation: 7,
            movement: ActionAxis2V1 { x: 1.0, y: 0.0 },
            started,
            ..PlayerActionFrameV1::default()
        });

        let first = inbox.next_fixed_frame();
        let second = inbox.next_fixed_frame();

        assert!(first.started.contains(PlayerActionV1::Jump));
        assert!(!second.started.contains(PlayerActionV1::Jump));
        assert_eq!(second.movement, ActionAxis2V1 { x: 1.0, y: 0.0 });
    }

    #[test]
    fn non_finite_axes_are_sanitized_at_the_boundary() {
        let frame = PlayerActionFrameV1 {
            movement: ActionAxis2V1 {
                x: f32::NAN,
                y: f32::INFINITY,
            },
            look_radians: ActionAxis2V1 {
                x: f32::NEG_INFINITY,
                y: 0.25,
            },
            ..PlayerActionFrameV1::default()
        }
        .sanitized();

        assert_eq!(frame.movement, ActionAxis2V1::default());
        assert_eq!(frame.look_radians, ActionAxis2V1 { x: 0.0, y: 0.25 });
    }

    #[test]
    fn maximum_finite_movement_is_normalized_without_nan() {
        let axis = ActionAxis2V1 {
            x: f32::MAX,
            y: f32::MAX,
        }
        .clamp_unit();
        assert!(axis.x.is_finite() && axis.y.is_finite());
        assert!((axis.x.hypot(axis.y) - 1.0).abs() < f32::EPSILON * 2.0);
    }

    #[test]
    fn idle_and_headless_frames_do_not_replay_a_live_edge() {
        let mut inbox = ActionFrameInbox::default();
        let _ = inbox.next_fixed_frame();
        let mut started = PlayerActionButtonsV1::empty();
        started.insert(PlayerActionV1::Jump);
        inbox.publish_live(PlayerActionFrameV1 {
            generation: 0,
            started,
            ..PlayerActionFrameV1::default()
        });
        assert!(
            inbox
                .next_fixed_frame()
                .started
                .contains(PlayerActionV1::Jump)
        );
        inbox
            .push_headless(PlayerActionFrameV1 {
                generation: 0,
                ..PlayerActionFrameV1::default()
            })
            .expect("headless and live generations use separate watermarks");
        let _ = inbox.next_fixed_frame();
        assert!(
            !inbox
                .next_fixed_frame()
                .started
                .contains(PlayerActionV1::Jump)
        );
    }

    #[test]
    fn unconsumed_live_look_deltas_and_edges_are_accumulated() {
        let mut inbox = ActionFrameInbox::default();
        let mut started = PlayerActionButtonsV1::empty();
        started.insert(PlayerActionV1::Jump);
        inbox.publish_live(PlayerActionFrameV1 {
            generation: 1,
            look_radians: ActionAxis2V1 { x: 0.1, y: -0.2 },
            started,
            ..PlayerActionFrameV1::default()
        });
        inbox.publish_live(PlayerActionFrameV1 {
            generation: 2,
            look_radians: ActionAxis2V1 { x: 0.2, y: 0.1 },
            ..PlayerActionFrameV1::default()
        });

        let frame = inbox.next_fixed_frame();
        assert!((frame.look_radians.x - 0.3).abs() < f32::EPSILON * 2.0);
        assert!((frame.look_radians.y + 0.1).abs() < f32::EPSILON * 2.0);
        assert!(frame.started.contains(PlayerActionV1::Jump));
    }

    #[test]
    fn duplicate_unconsumed_live_generation_is_idempotent() {
        let mut inbox = ActionFrameInbox::default();
        let mut started = PlayerActionButtonsV1::empty();
        started.insert(PlayerActionV1::Jump);
        let frame = PlayerActionFrameV1 {
            generation: 11,
            look_radians: ActionAxis2V1 { x: 0.2, y: -0.1 },
            started,
            ..PlayerActionFrameV1::default()
        };
        inbox.publish_live(frame.clone());
        inbox.publish_live(frame);

        let received = inbox.next_fixed_frame();
        assert_eq!(received.look_radians, ActionAxis2V1 { x: 0.2, y: -0.1 });
        assert!(received.started.contains(PlayerActionV1::Jump));
        assert!(
            !inbox
                .next_fixed_frame()
                .started
                .contains(PlayerActionV1::Jump)
        );
    }

    #[test]
    fn held_live_look_rate_is_integrated_once_per_fixed_tick() {
        let mut inbox = ActionFrameInbox::default();
        inbox.publish_live_with_look_rate(
            PlayerActionFrameV1 {
                generation: 1,
                ..PlayerActionFrameV1::default()
            },
            ActionAxis2V1 { x: 2.1, y: -2.1 },
        );

        for _ in 0..4 {
            let frame = inbox.next_fixed_frame_with_step_seconds(1.0 / 60.0);
            assert!((frame.look_radians.x - 0.035).abs() < f32::EPSILON * 4.0);
            assert!((frame.look_radians.y + 0.035).abs() < f32::EPSILON * 4.0);
        }
    }

    #[test]
    fn stale_live_generation_is_dropped() {
        let mut inbox = ActionFrameInbox::default();
        inbox.publish_live(PlayerActionFrameV1 {
            generation: 9,
            ..PlayerActionFrameV1::default()
        });
        let _ = inbox.next_fixed_frame();
        inbox.publish_live(PlayerActionFrameV1 {
            generation: 8,
            movement: ActionAxis2V1 { x: 1.0, y: 0.0 },
            ..PlayerActionFrameV1::default()
        });
        assert_eq!(inbox.next_fixed_frame().movement, ActionAxis2V1::default());
    }

    #[test]
    fn headless_generation_remains_monotonic_after_queue_is_drained() {
        let mut inbox = ActionFrameInbox::default();
        inbox
            .push_headless(PlayerActionFrameV1 {
                generation: 8,
                ..PlayerActionFrameV1::default()
            })
            .expect("first headless generation is valid");
        let _ = inbox.next_fixed_frame();

        assert_eq!(
            inbox.push_headless(PlayerActionFrameV1 {
                generation: 8,
                ..PlayerActionFrameV1::default()
            }),
            Err(ActionFrameInboxError::NonMonotonicGeneration {
                previous: 8,
                actual: 8,
            })
        );
    }
}
