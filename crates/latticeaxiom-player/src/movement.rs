use std::{
    f32::consts::{FRAC_PI_2, FRAC_PI_4},
    time::Duration,
};

use avian3d::{
    math::{AdjustPrecision as _, AsF32 as _},
    prelude::{
        Collider, CustomPositionIntegration, LinearVelocity, MoveAndSlide, MoveAndSlideConfig,
        MoveAndSlideHitResponse, MoveAndSlideOutput, RigidBody, Sensor, ShapeCastConfig,
        SpatialQuery, SpatialQueryFilter, SpeculativeMargin,
    },
};
use bevy::{
    ecs::bundle::Bundle,
    prelude::{
        Component, Dir3, Entity, GlobalTransform, Quat, Query, Res, Time, Transform, Vec2, Vec3,
        With, Without,
    },
    time::Fixed,
};
use latticeaxiom_gameplay::PlayerId;
use thiserror::Error;

use crate::{ActionFrameInbox, BlockEditInputStateV1, PlayerActionFrameV1, PlayerActionV1};

#[cfg(test)]
const REFERENCE_FIXED_HZ: f32 = 60.0;
const GRAVITY_MPS2: f32 = 19.62;
const GROUND_SNAP_M: f32 = 0.10;
const CONTROLLER_SKIN_M: f32 = 0.01;
const MAX_SUPPORT_QUERY_HITS: u32 = 64;
const STEP_EPSILON_M: f32 = 0.001;
const SPECTATOR_SPEED_MPS: f32 = 12.0;
const SPRINT_MULTIPLIER: f32 = 1.3;
const FLY_SPEED_MPS: f32 = 10.80;
const FLY_SPRINT_MULTIPLIER: f32 = 2.0;
const JUMP_BUFFER_DURATION: Duration = Duration::from_millis(100);
const COYOTE_DURATION: Duration = Duration::from_millis(100);
const FLY_TOGGLE_DURATION: Duration = Duration::from_millis(350);
// `Duration::from_secs_f64(1 / hz)` rounds each fixed step to nanoseconds.
// This covers the maximum aggregate rounding across a 350 ms window at the
// supported 10 kHz ceiling without extending the window by one full tick.
const TIMER_ROUNDING_TOLERANCE: Duration = Duration::from_micros(2);

/// Frozen D2 movement profile from ADR 0025.
#[derive(Clone, Copy, Component, Debug, PartialEq)]
pub struct PlayerMovementProfileV1 {
    capsule_total_height_m: f32,
    capsule_radius_m: f32,
    eye_height_m: f32,
    maximum_walk_speed_mps: f32,
    maximum_walkable_slope_radians: f32,
    maximum_step_height_m: f32,
    jump_apex_m: f32,
    jump_buffer_duration: Duration,
    coyote_duration: Duration,
}

impl PlayerMovementProfileV1 {
    /// Returns the capsule's total height, including both hemispheres.
    #[must_use]
    pub const fn capsule_total_height_m(self) -> f32 {
        self.capsule_total_height_m
    }

    /// Returns the capsule radius.
    #[must_use]
    pub const fn capsule_radius_m(self) -> f32 {
        self.capsule_radius_m
    }

    /// Returns eye height above the capsule feet.
    #[must_use]
    pub const fn eye_height_m(self) -> f32 {
        self.eye_height_m
    }

    /// Returns maximum horizontal walk speed.
    #[must_use]
    pub const fn maximum_walk_speed_mps(self) -> f32 {
        self.maximum_walk_speed_mps
    }

    /// Returns maximum walkable slope angle in radians.
    #[must_use]
    pub const fn maximum_walkable_slope_radians(self) -> f32 {
        self.maximum_walkable_slope_radians
    }

    /// Returns maximum step height.
    #[must_use]
    pub const fn maximum_step_height_m(self) -> f32 {
        self.maximum_step_height_m
    }

    /// Returns the required approximate jump apex above takeoff.
    #[must_use]
    pub const fn jump_apex_m(self) -> f32 {
        self.jump_apex_m
    }

    /// Returns the jump input-buffer duration in simulation seconds.
    #[must_use]
    pub fn jump_buffer_seconds(self) -> f32 {
        self.jump_buffer_duration.as_secs_f32()
    }

    /// Returns the coyote-time duration in simulation seconds.
    #[must_use]
    pub fn coyote_seconds(self) -> f32 {
        self.coyote_duration.as_secs_f32()
    }

    /// Returns whether a slope angle satisfies the inclusive V1 boundary.
    #[must_use]
    pub fn accepts_slope_radians(self, angle: f32) -> bool {
        angle.is_finite() && (0.0..=self.maximum_walkable_slope_radians).contains(&angle)
    }

    /// Returns whether a non-negative step height satisfies the inclusive V1 boundary.
    #[must_use]
    pub fn accepts_step_height_m(self, height: f32) -> bool {
        height.is_finite() && height >= 0.0 && height <= self.maximum_step_height_m
    }

    fn capsule_segment_length_m(self) -> f32 {
        self.capsule_total_height_m - self.capsule_radius_m * 2.0
    }

    fn eye_offset_from_center_m(self) -> f32 {
        self.eye_height_m - self.capsule_total_height_m * 0.5
    }

    fn validate(self) -> Result<Self, PlayerMovementProfileError> {
        let finite_positive = [
            self.capsule_total_height_m,
            self.capsule_radius_m,
            self.eye_height_m,
            self.maximum_walk_speed_mps,
            self.maximum_step_height_m,
            self.jump_apex_m,
        ]
        .into_iter()
        .all(|value| value.is_finite() && value > 0.0);
        if !finite_positive || !self.maximum_walkable_slope_radians.is_finite() {
            return Err(PlayerMovementProfileError::NonFiniteOrNonPositive);
        }
        if self.capsule_total_height_m <= self.capsule_radius_m * 2.0 {
            return Err(PlayerMovementProfileError::InvalidCapsule);
        }
        if self.eye_height_m > self.capsule_total_height_m {
            return Err(PlayerMovementProfileError::EyeOutsideCapsule);
        }
        if !(0.0..FRAC_PI_2).contains(&self.maximum_walkable_slope_radians) {
            return Err(PlayerMovementProfileError::InvalidSlope);
        }
        if self.jump_buffer_duration.is_zero() || self.coyote_duration.is_zero() {
            return Err(PlayerMovementProfileError::InvalidTimeWindow);
        }
        Ok(self)
    }
}

impl Default for PlayerMovementProfileV1 {
    fn default() -> Self {
        Self {
            capsule_total_height_m: 1.80,
            capsule_radius_m: 0.35,
            eye_height_m: 1.62,
            maximum_walk_speed_mps: 4.50,
            maximum_walkable_slope_radians: FRAC_PI_4,
            maximum_step_height_m: 0.60,
            jump_apex_m: 1.25,
            jump_buffer_duration: JUMP_BUFFER_DURATION,
            coyote_duration: COYOTE_DURATION,
        }
    }
}

/// Invalid movement profile construction.
#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum PlayerMovementProfileError {
    /// A distance or speed was non-finite, zero, or negative.
    #[error("movement profile distances and speeds must be finite and positive")]
    NonFiniteOrNonPositive,
    /// Capsule hemispheres do not leave a positive segment.
    #[error("capsule total height must exceed its diameter")]
    InvalidCapsule,
    /// Eye height is above the capsule.
    #[error("eye height must lie within the capsule height")]
    EyeOutsideCapsule,
    /// Walkable slope is outside `[0, pi/2)`.
    #[error("walkable slope must lie in [0, pi/2)")]
    InvalidSlope,
    /// Jump buffering or coyote time was non-finite or non-positive.
    #[error("jump buffer and coyote windows must be finite positive durations")]
    InvalidTimeWindow,
}

/// Stable player marker plus persistent player identity.
#[derive(Clone, Copy, Component, Debug, Eq, PartialEq)]
pub struct D2Player {
    /// Stable player key used by authoritative commands.
    pub player_id: PlayerId,
}

/// Marks the single local player that consumes [`ActionFrameInbox`].
#[derive(Clone, Copy, Component, Debug, Default, Eq, PartialEq)]
pub struct LocalPlayerInput;

/// Current stable action frame installed at the start of a fixed tick.
#[derive(Clone, Component, Debug, Default, PartialEq)]
pub struct CurrentPlayerActionFrame(pub PlayerActionFrameV1);

/// Authoritative player view orientation in radians.
#[derive(Clone, Copy, Component, Debug, Default, PartialEq)]
pub struct PlayerViewV1 {
    yaw_radians: f32,
    pitch_radians: f32,
}

impl PlayerViewV1 {
    /// Returns yaw around world `+Y`.
    #[must_use]
    pub const fn yaw_radians(self) -> f32 {
        self.yaw_radians
    }

    /// Returns pitch around local `+X`.
    #[must_use]
    pub const fn pitch_radians(self) -> f32 {
        self.pitch_radians
    }

    /// Returns the normalized conventional-forward direction.
    #[must_use]
    pub fn forward(self) -> Vec3 {
        (Quat::from_rotation_y(self.yaw_radians)
            * Quat::from_rotation_x(self.pitch_radians)
            * Vec3::NEG_Z)
            .normalize_or_zero()
    }

    fn apply_look(&mut self, look: crate::ActionAxis2V1) {
        self.yaw_radians = wrap_radians(self.yaw_radians - look.x);
        self.pitch_radians =
            (self.pitch_radians - look.y).clamp(-FRAC_PI_2 + 0.001, FRAC_PI_2 - 0.001);
    }
}

/// Fixed-tick movement state that is not a portable solver representation.
#[derive(Clone, Copy, Component, Debug, PartialEq)]
pub struct PlayerControllerState {
    grounded: bool,
    time_since_grounded: Duration,
    jump_buffer_remaining: Duration,
    flying: bool,
    time_since_jump_started: Duration,
}

impl PlayerControllerState {
    /// Returns whether the most recent controller probe found walkable ground.
    #[must_use]
    pub const fn grounded(self) -> bool {
        self.grounded
    }

    /// Returns simulation seconds elapsed since walkable ground was observed.
    #[must_use]
    pub fn seconds_since_grounded(self) -> f32 {
        self.time_since_grounded.as_secs_f32()
    }

    /// Returns buffered jump duration remaining in simulation seconds.
    #[must_use]
    pub fn jump_buffer_remaining_seconds(self) -> f32 {
        self.jump_buffer_remaining.as_secs_f32()
    }

    /// Returns whether creative flight is currently active.
    #[must_use]
    pub const fn flying(self) -> bool {
        self.flying
    }
}

impl Default for PlayerControllerState {
    fn default() -> Self {
        Self {
            grounded: false,
            time_since_grounded: Duration::MAX,
            jump_buffer_remaining: Duration::ZERO,
            flying: false,
            time_since_jump_started: Duration::MAX,
        }
    }
}

/// Detached developer spectator presentation state.
///
/// Inserting this component freezes the authoritative player controller and
/// edits. Only this local pose changes, so save bytes and authoritative state
/// remain untouched.
#[derive(Clone, Copy, Component, Debug, PartialEq)]
pub struct DetachedSpectator {
    /// Local-only spectator position.
    pub position: Vec3,
    /// Local-only spectator yaw.
    pub yaw_radians: f32,
    /// Local-only spectator pitch.
    pub pitch_radians: f32,
}

impl DetachedSpectator {
    /// Creates a detached camera pose from the current authoritative eye anchor.
    #[must_use]
    pub fn from_player(
        transform: &Transform,
        view: PlayerViewV1,
        profile: PlayerMovementProfileV1,
    ) -> Self {
        Self {
            position: transform.translation + Vec3::Y * profile.eye_offset_from_center_m(),
            yaw_radians: view.yaw_radians,
            pitch_radians: view.pitch_radians,
        }
    }
}

/// Spawn bundle for the root entity of a D2 kinematic capsule player.
#[derive(Bundle, Debug)]
pub struct D2PlayerBundle {
    player: D2Player,
    local_input: LocalPlayerInput,
    profile: PlayerMovementProfileV1,
    controller: PlayerControllerState,
    view: PlayerViewV1,
    action_frame: CurrentPlayerActionFrame,
    edit_input: BlockEditInputStateV1,
    rigid_body: RigidBody,
    custom_position_integration: CustomPositionIntegration,
    speculative_margin: SpeculativeMargin,
    collider: Collider,
    linear_velocity: LinearVelocity,
    transform: Transform,
    global_transform: GlobalTransform,
}

impl D2PlayerBundle {
    /// Creates the frozen D2 capsule at a center-position transform.
    #[must_use]
    pub fn new(player_id: PlayerId, transform: Transform) -> Self {
        // Default is an accepted constant profile and is covered by validation tests.
        Self::from_validated_profile(player_id, transform, PlayerMovementProfileV1::default())
    }

    /// Creates a capsule after validating a supplied profile.
    ///
    /// # Errors
    ///
    /// Returns a typed error when dimensions, slope, or tick windows are invalid.
    pub fn with_profile(
        player_id: PlayerId,
        transform: Transform,
        profile: PlayerMovementProfileV1,
    ) -> Result<Self, PlayerMovementProfileError> {
        Ok(Self::from_validated_profile(
            player_id,
            transform,
            profile.validate()?,
        ))
    }

    fn from_validated_profile(
        player_id: PlayerId,
        transform: Transform,
        profile: PlayerMovementProfileV1,
    ) -> Self {
        Self {
            player: D2Player { player_id },
            local_input: LocalPlayerInput,
            profile,
            controller: PlayerControllerState::default(),
            view: PlayerViewV1::default(),
            action_frame: CurrentPlayerActionFrame::default(),
            edit_input: BlockEditInputStateV1::default(),
            rigid_body: RigidBody::Kinematic,
            custom_position_integration: CustomPositionIntegration,
            speculative_margin: SpeculativeMargin(0.0),
            collider: Collider::capsule(
                profile.capsule_radius_m(),
                profile.capsule_segment_length_m(),
            ),
            linear_velocity: LinearVelocity::ZERO,
            transform,
            global_transform: GlobalTransform::default(),
        }
    }
}

#[allow(clippy::needless_pass_by_value)] // Bevy systems receive SystemParams by value.
pub(crate) fn install_fixed_action_frame(
    time: Res<'_, Time<Fixed>>,
    mut inbox: bevy::prelude::ResMut<'_, ActionFrameInbox>,
    mut players: Query<'_, '_, &mut CurrentPlayerActionFrame, With<LocalPlayerInput>>,
) {
    let mut local_players = players.iter_mut();
    let Some(mut current) = local_players.next() else {
        return;
    };
    if let Some(mut duplicate) = local_players.next() {
        current.0 = PlayerActionFrameV1::default();
        duplicate.0 = PlayerActionFrameV1::default();
        for mut additional in local_players {
            additional.0 = PlayerActionFrameV1::default();
        }
        return;
    }
    let frame = inbox.next_fixed_frame_with_step_seconds(time.delta_secs());
    current.0 = frame;
}

#[allow(clippy::needless_pass_by_value)] // Bevy systems receive SystemParams by value.
#[allow(clippy::type_complexity)] // Component filters encode disjoint Bevy access.
pub(crate) fn update_player_view(
    mut players: Query<
        '_,
        '_,
        (&CurrentPlayerActionFrame, &mut PlayerViewV1),
        Without<DetachedSpectator>,
    >,
) {
    for (frame, mut view) in &mut players {
        view.apply_look(frame.0.look_radians);
    }
}

#[allow(clippy::needless_pass_by_value)] // Bevy systems receive SystemParams by value.
#[allow(clippy::type_complexity)] // Component filters encode disjoint Bevy access.
pub(crate) fn update_spectator(
    time: Res<'_, Time<Fixed>>,
    mut spectators: Query<'_, '_, (&CurrentPlayerActionFrame, &mut DetachedSpectator)>,
) {
    let delta_seconds = time.delta_secs();
    for (frame, mut spectator) in &mut spectators {
        spectator.yaw_radians = wrap_radians(spectator.yaw_radians - frame.0.look_radians.x);
        spectator.pitch_radians = (spectator.pitch_radians - frame.0.look_radians.y)
            .clamp(-FRAC_PI_2 + 0.001, FRAC_PI_2 - 0.001);

        let input = frame.0.movement.clamp_unit();
        let local = Vec3::new(input.x, 0.0, -input.y);
        let horizontal = Quat::from_rotation_y(spectator.yaw_radians) * local;
        let jump_held = frame.0.held.contains(PlayerActionV1::Jump);
        let sneak_held = frame.0.held.contains(PlayerActionV1::Sneak);
        let vertical = match (jump_held, sneak_held) {
            (true, false) => Vec3::Y,
            (false, true) => Vec3::NEG_Y,
            (true, true) | (false, false) => Vec3::ZERO,
        };
        let speed = if frame.0.held.contains(PlayerActionV1::Sprint) {
            SPECTATOR_SPEED_MPS * FLY_SPRINT_MULTIPLIER
        } else {
            SPECTATOR_SPEED_MPS
        };
        spectator.position += (horizontal + vertical) * speed * delta_seconds;
    }
}

#[allow(clippy::needless_pass_by_value)] // Bevy systems receive SystemParams by value.
#[allow(clippy::type_complexity)] // The tuple is one bounded controller query.
pub(crate) fn update_grounded(
    time: Res<'_, Time<Fixed>>,
    spatial_query: SpatialQuery<'_, '_>,
    solid_colliders: Query<'_, '_, (), (With<Collider>, Without<Sensor>)>,
    mut players: Query<
        '_,
        '_,
        (
            Entity,
            &Collider,
            &mut Transform,
            &PlayerMovementProfileV1,
            &mut PlayerControllerState,
        ),
        (With<D2Player>, Without<DetachedSpectator>),
    >,
) {
    let delta = time.delta();
    for (entity, collider, mut transform, profile, mut state) in &mut players {
        if state.flying {
            state.grounded = false;
            state.time_since_grounded = state.time_since_grounded.saturating_add(delta);
            continue;
        }
        let filter = SpatialQueryFilter::from_excluded_entities([entity]);
        let nearest_walkable_distance = nearest_walkable_support(
            &spatial_query,
            collider,
            transform.translation,
            transform.rotation,
            GROUND_SNAP_M + CONTROLLER_SKIN_M,
            *profile,
            &filter,
            |candidate| solid_colliders.contains(candidate),
        );
        if let Some(support) = nearest_walkable_distance {
            transform.translation.y -= (support.distance - CONTROLLER_SKIN_M).max(0.0);
            state.grounded = true;
            state.time_since_grounded = Duration::ZERO;
        } else {
            state.grounded = false;
            state.time_since_grounded = state.time_since_grounded.saturating_add(delta);
        }
    }
}

#[allow(clippy::needless_pass_by_value)] // Bevy systems receive SystemParams by value.
#[allow(clippy::type_complexity)] // The tuple is one bounded controller query.
pub(crate) fn prepare_velocity(
    time: Res<'_, Time<Fixed>>,
    mut players: Query<
        '_,
        '_,
        (
            &PlayerMovementProfileV1,
            &CurrentPlayerActionFrame,
            &PlayerViewV1,
            &mut PlayerControllerState,
            &mut LinearVelocity,
        ),
        (With<D2Player>, Without<DetachedSpectator>),
    >,
) {
    let delta = time.delta();
    for (profile, frame, view, mut state, mut velocity) in &mut players {
        let input = frame.0.movement.clamp_unit();
        let jump_started = frame.0.started.contains(PlayerActionV1::Jump);
        let jump_held = frame.0.held.contains(PlayerActionV1::Jump);
        let sprint_held = frame.0.held.contains(PlayerActionV1::Sprint);
        let sneak_held = frame.0.held.contains(PlayerActionV1::Sneak);
        apply_jump_and_fly_toggle(*profile, jump_started, &mut state, delta);
        let speed = horizontal_speed_mps(
            profile.maximum_walk_speed_mps,
            state.flying,
            sprint_held,
            sneak_held,
            input.y,
        );
        let local_direction = Vec3::new(input.x, 0.0, -input.y);
        let world_direction = Quat::from_rotation_y(view.yaw_radians) * local_direction;
        velocity.x = world_direction.x * speed;
        velocity.z = world_direction.z * speed;
        velocity.y = prepared_vertical_velocity(
            *profile,
            jump_held,
            sprint_held,
            sneak_held,
            &mut state,
            velocity.y,
            delta,
        );
    }
}

fn horizontal_speed_mps(
    walk_speed_mps: f32,
    flying: bool,
    sprint_held: bool,
    sneak_held: bool,
    forward: f32,
) -> f32 {
    if flying {
        if sprint_held {
            FLY_SPEED_MPS * FLY_SPRINT_MULTIPLIER
        } else {
            FLY_SPEED_MPS
        }
    } else if sprint_held && !sneak_held && forward > 0.0 {
        walk_speed_mps * SPRINT_MULTIPLIER
    } else {
        walk_speed_mps
    }
}

fn apply_jump_and_fly_toggle(
    profile: PlayerMovementProfileV1,
    jump_started: bool,
    state: &mut PlayerControllerState,
    delta: Duration,
) {
    if jump_started {
        let within_toggle_window =
            duration_within_window(state.time_since_jump_started, FLY_TOGGLE_DURATION);
        if state.flying && within_toggle_window {
            state.flying = false;
            state.jump_buffer_remaining = Duration::ZERO;
        } else if !state.flying && !state.grounded && within_toggle_window {
            state.flying = true;
            state.jump_buffer_remaining = Duration::ZERO;
        } else if !state.flying {
            state.jump_buffer_remaining = profile.jump_buffer_duration;
        }
        state.time_since_jump_started = Duration::ZERO;
    }
    state.time_since_jump_started = state.time_since_jump_started.saturating_add(delta);
}

fn prepared_vertical_velocity(
    profile: PlayerMovementProfileV1,
    jump_held: bool,
    sprint_held: bool,
    sneak_held: bool,
    state: &mut PlayerControllerState,
    current_velocity_y: f32,
    delta: Duration,
) -> f32 {
    let delta_seconds = delta.as_secs_f32();
    if state.flying {
        state.jump_buffer_remaining = Duration::ZERO;
        let speed = if sprint_held {
            FLY_SPEED_MPS * FLY_SPRINT_MULTIPLIER
        } else {
            FLY_SPEED_MPS
        };
        return match (jump_held, sneak_held) {
            (true, false) => speed,
            (false, true) => -speed,
            (true, true) | (false, false) => 0.0,
        };
    }
    let within_coyote = state.grounded
        || duration_within_window(state.time_since_grounded, profile.coyote_duration);
    if !state.jump_buffer_remaining.is_zero() && within_coyote {
        state.jump_buffer_remaining = Duration::ZERO;
        state.grounded = false;
        state.time_since_grounded = profile.coyote_duration.saturating_add(delta);
        jump_launch_speed_mps(profile.jump_apex_m, delta_seconds)
    } else {
        state.jump_buffer_remaining = subtract_timer(state.jump_buffer_remaining, delta);
        if state.grounded && current_velocity_y <= 0.0 {
            0.0
        } else {
            (current_velocity_y - GRAVITY_MPS2 * delta_seconds).max(-50.0)
        }
    }
}

#[allow(clippy::needless_pass_by_value)] // Bevy systems receive SystemParams by value.
#[allow(clippy::type_complexity)] // The tuple is one bounded controller query.
pub(crate) fn move_players(
    time: Res<'_, Time<Fixed>>,
    move_and_slide: MoveAndSlide<'_, '_>,
    mut players: Query<
        '_,
        '_,
        (
            Entity,
            &Collider,
            &PlayerMovementProfileV1,
            &mut PlayerControllerState,
            &mut Transform,
            &mut LinearVelocity,
        ),
        (With<D2Player>, Without<DetachedSpectator>),
    >,
) {
    for (entity, collider, profile, mut state, mut transform, mut velocity) in &mut players {
        let start = transform.translation;
        let filter = SpatialQueryFilter::from_excluded_entities([entity]);
        let flying = state.flying;
        let mut hit_walkable_ground = false;
        let direct = move_and_slide.move_and_slide(
            collider,
            start.adjust_precision(),
            transform.rotation.adjust_precision(),
            velocity.0,
            time.delta(),
            &controller_move_config(),
            &filter,
            |hit| {
                let normal = hit.normal.adjust_precision();
                let walkable = normal.dot(Vec3::Y) > 0.0
                    && profile.accepts_slope_radians(normal.angle_between(Vec3::Y));
                if walkable {
                    hit_walkable_ground = true;
                } else if normal.y > 0.0 {
                    reject_horizontal_steep_climb(normal, hit.velocity);
                }
                MoveAndSlideHitResponse::Accept
            },
        );

        let stepped = if state.grounded && !flying {
            try_step(
                &move_and_slide,
                collider,
                start,
                transform.rotation,
                velocity.0,
                time.delta(),
                *profile,
                &filter,
                direct.position.f32(),
            )
        } else {
            None
        };

        if let Some(step_position) = stepped {
            transform.translation = step_position;
            velocity.y = 0.0;
            state.grounded = true;
            state.time_since_grounded = Duration::ZERO;
        } else {
            transform.translation = direct.position.f32();
            velocity.0 = direct.projected_velocity;
            if hit_walkable_ground && velocity.y <= 0.0 {
                velocity.y = 0.0;
                state.flying = false;
                state.grounded = true;
                state.time_since_grounded = Duration::ZERO;
            }
        }
    }
}

fn jump_launch_speed_mps(apex_m: f32, delta_seconds: f32) -> f32 {
    // Solve the discrete semi-implicit trajectory rather than approximating
    // its integration error. The selected integral step count is the unique
    // one whose positive-velocity samples bracket the authored apex.
    let normalized_height = apex_m / (GRAVITY_MPS2 * delta_seconds * delta_seconds);
    let rising_steps = (((1.0 + 8.0 * normalized_height).sqrt() - 1.0) * 0.5)
        .ceil()
        .max(1.0);
    apex_m / (rising_steps * delta_seconds)
        + GRAVITY_MPS2 * delta_seconds * (rising_steps - 1.0) * 0.5
}

fn duration_within_window(elapsed: Duration, window: Duration) -> bool {
    elapsed <= window.saturating_add(TIMER_ROUNDING_TOLERANCE)
}

fn subtract_timer(current: Duration, delta: Duration) -> Duration {
    let remaining = current.saturating_sub(delta);
    if remaining <= TIMER_ROUNDING_TOLERANCE {
        Duration::ZERO
    } else {
        remaining
    }
}

fn reject_horizontal_steep_climb(normal: Vec3, velocity: &mut Vec3) {
    let horizontal_normal = Vec3::new(normal.x, 0.0, normal.z);
    let length_squared = horizontal_normal.length_squared();
    if length_squared <= f32::EPSILON {
        return;
    }
    let horizontal_direction = horizontal_normal / length_squared.sqrt();
    let velocity_into_surface = velocity.dot(horizontal_direction);
    if velocity_into_surface < 0.0 {
        *velocity -= horizontal_direction * velocity_into_surface;
    }
}

#[allow(clippy::too_many_arguments)]
fn try_step(
    move_and_slide: &MoveAndSlide<'_, '_>,
    collider: &Collider,
    start: Vec3,
    rotation: Quat,
    velocity: Vec3,
    delta: std::time::Duration,
    profile: PlayerMovementProfileV1,
    filter: &SpatialQueryFilter,
    direct_position: Vec3,
) -> Option<Vec3> {
    let horizontal_velocity = Vec3::new(velocity.x, 0.0, velocity.z);
    let desired_horizontal_distance = horizontal_velocity.length() * delta.as_secs_f32();
    if desired_horizontal_distance <= STEP_EPSILON_M {
        return None;
    }
    let direct_horizontal_distance = horizontal_distance(start, direct_position);
    if direct_horizontal_distance + STEP_EPSILON_M >= desired_horizontal_distance {
        return None;
    }

    let raise = profile.maximum_step_height_m + CONTROLLER_SKIN_M;
    if upward_clearance_is_blocked(
        &move_and_slide.spatial_query,
        collider,
        start,
        rotation,
        raise,
        filter,
        |candidate| move_and_slide.colliders.contains(candidate),
    ) {
        return None;
    }
    let base_support = nearest_walkable_support(
        &move_and_slide.spatial_query,
        collider,
        start,
        rotation,
        GROUND_SNAP_M + CONTROLLER_SKIN_M,
        profile,
        filter,
        |candidate| move_and_slide.colliders.contains(candidate),
    )?;

    // A capsule exactly touching a riser needs enough forward clearance for
    // the downward cast to see a walkable top normal instead of the rounded
    // top edge. Bound that assist from the frozen radius, skin, and slope.
    let minimum_edge_clearance = (profile.capsule_radius_m + CONTROLLER_SKIN_M
        - profile.capsule_radius_m * profile.maximum_walkable_slope_radians.sin())
    .max(0.0);
    let attempted_horizontal_distance = desired_horizontal_distance.max(minimum_edge_clearance);
    let raised_velocity = horizontal_velocity.normalize_or_zero()
        * (attempted_horizontal_distance / delta.as_secs_f32());
    let raised_start = start + Vec3::Y * raise;
    let MoveAndSlideOutput {
        position: raised_output,
        ..
    } = move_and_slide.move_and_slide(
        collider,
        raised_start.adjust_precision(),
        rotation.adjust_precision(),
        raised_velocity,
        delta,
        &controller_move_config(),
        filter,
        |_| MoveAndSlideHitResponse::Accept,
    );
    let raised_output = raised_output.f32();
    if horizontal_distance(raised_start, raised_output)
        <= direct_horizontal_distance + STEP_EPSILON_M
    {
        return None;
    }

    let down_distance = raise + GROUND_SNAP_M + CONTROLLER_SKIN_M;
    let landing_support = nearest_walkable_support(
        &move_and_slide.spatial_query,
        collider,
        raised_output,
        rotation,
        down_distance,
        profile,
        filter,
        |candidate| move_and_slide.colliders.contains(candidate),
    )?;
    let step_height = landing_support.point_y - base_support.point_y;
    if step_height < -GROUND_SNAP_M || step_height > profile.maximum_step_height_m + STEP_EPSILON_M
    {
        return None;
    }
    let landed = raised_output - Vec3::Y * (landing_support.distance - CONTROLLER_SKIN_M).max(0.0);
    let actual_rise = landed.y - start.y;
    if actual_rise < -GROUND_SNAP_M || actual_rise > profile.maximum_step_height_m + STEP_EPSILON_M
    {
        return None;
    }
    Some(landed)
}

fn controller_move_config() -> MoveAndSlideConfig {
    MoveAndSlideConfig {
        skin_width: CONTROLLER_SKIN_M,
        ..MoveAndSlideConfig::default()
    }
}

#[derive(Clone, Copy)]
struct WalkableSupport {
    distance: f32,
    point_y: f32,
}

#[allow(clippy::too_many_arguments)] // Bounded geometric query inputs are clearer as named parameters.
fn nearest_walkable_support(
    spatial_query: &SpatialQuery<'_, '_>,
    collider: &Collider,
    origin: Vec3,
    rotation: Quat,
    max_distance: f32,
    profile: PlayerMovementProfileV1,
    filter: &SpatialQueryFilter,
    mut is_solid: impl FnMut(Entity) -> bool,
) -> Option<WalkableSupport> {
    let mut query_hit_count = 0_u32;
    let mut overflowed = false;
    let mut nearest_walkable_support: Option<WalkableSupport> = None;
    spatial_query.shape_hits_callback(
        collider,
        origin.adjust_precision(),
        rotation.adjust_precision(),
        Dir3::NEG_Y,
        &ShapeCastConfig::from_max_distance(max_distance),
        filter,
        |hit| {
            // Count every geometric hit, including sensors, so adversarial
            // overlap cannot turn this fixed-tick path into unbounded work.
            // Overflow deliberately fails closed as unsupported ground.
            query_hit_count = query_hit_count.saturating_add(1);
            if query_hit_count > MAX_SUPPORT_QUERY_HITS {
                overflowed = true;
                return false;
            }
            if is_solid(hit.entity)
                && hit.normal1.dot(Vec3::Y) > 0.0
                && profile.accepts_slope_radians(hit.normal1.angle_between(Vec3::Y))
                && nearest_walkable_support.is_none_or(|support| hit.distance < support.distance)
            {
                nearest_walkable_support = Some(WalkableSupport {
                    distance: hit.distance,
                    point_y: hit.point1.y,
                });
            }
            true
        },
    );
    if overflowed {
        None
    } else {
        nearest_walkable_support
    }
}

#[allow(clippy::too_many_arguments)]
fn upward_clearance_is_blocked(
    spatial_query: &SpatialQuery<'_, '_>,
    collider: &Collider,
    origin: Vec3,
    rotation: Quat,
    max_distance: f32,
    filter: &SpatialQueryFilter,
    mut is_solid: impl FnMut(Entity) -> bool,
) -> bool {
    let mut query_hit_count = 0_u32;
    let mut overflowed = false;
    let mut blocked = false;
    let config = ShapeCastConfig {
        ignore_origin_penetration: true,
        ..ShapeCastConfig::from_max_distance(max_distance)
    };
    spatial_query.shape_hits_callback(
        collider,
        origin.adjust_precision(),
        rotation.adjust_precision(),
        Dir3::Y,
        &config,
        filter,
        |hit| {
            query_hit_count = query_hit_count.saturating_add(1);
            if query_hit_count > MAX_SUPPORT_QUERY_HITS {
                overflowed = true;
                return false;
            }
            if is_solid(hit.entity)
                && hit.normal1.dot(Vec3::Y) < 0.0
                && hit.distance + STEP_EPSILON_M < max_distance
            {
                blocked = true;
                return false;
            }
            true
        },
    );
    overflowed || blocked
}

fn horizontal_distance(a: Vec3, b: Vec3) -> f32 {
    Vec2::new(b.x - a.x, b.z - a.z).length()
}

fn wrap_radians(value: f32) -> f32 {
    let full_turn = std::f32::consts::TAU;
    (value + std::f32::consts::PI).rem_euclid(full_turn) - std::f32::consts::PI
}

#[cfg(test)]
#[allow(clippy::float_cmp)] // Frozen ADR constants and exact helper outputs are intentional.
mod tests {
    use super::*;

    #[test]
    fn frozen_profile_matches_adr_0025() {
        let profile = PlayerMovementProfileV1::default();
        assert_eq!(profile.capsule_total_height_m(), 1.80);
        assert_eq!(profile.capsule_radius_m(), 0.35);
        assert_eq!(profile.eye_height_m(), 1.62);
        assert_eq!(profile.maximum_walk_speed_mps(), 4.50);
        assert_eq!(profile.maximum_step_height_m(), 0.60);
        assert_eq!(profile.jump_buffer_seconds(), 0.1);
        assert_eq!(profile.coyote_seconds(), 0.1);
        assert!(profile.validate().is_ok());
    }

    #[test]
    fn slope_and_step_boundaries_are_inclusive_only_at_the_contract_limit() {
        let profile = PlayerMovementProfileV1::default();
        assert!(profile.accepts_slope_radians(45.0_f32.to_radians()));
        assert!(!profile.accepts_slope_radians(46.0_f32.to_radians()));
        assert!(!profile.accepts_slope_radians(-f32::EPSILON));
        assert!(profile.accepts_step_height_m(0.60));
        assert!(!profile.accepts_step_height_m(0.61));
    }

    #[test]
    fn conventional_forward_is_negative_z() {
        assert_eq!(PlayerViewV1::default().forward(), Vec3::NEG_Z);
    }

    #[test]
    fn capsule_constructor_uses_height_excluding_hemispheres() {
        let profile = PlayerMovementProfileV1::default();
        assert!((profile.capsule_segment_length_m() - 1.10).abs() <= f32::EPSILON * 2.0);
    }

    #[test]
    fn authored_time_windows_use_sixty_hz_reference_durations() {
        assert_eq!(REFERENCE_FIXED_HZ, 60.0);
    }

    #[test]
    fn discrete_jump_apex_is_stable_across_fixed_rates() {
        for hertz in [60.0_f32, 240.0, 1_000.0] {
            let delta_seconds = hertz.recip();
            let mut velocity = jump_launch_speed_mps(1.25, delta_seconds);
            let mut height = 0.0_f32;
            while velocity > 0.0 {
                height += velocity * delta_seconds;
                velocity -= GRAVITY_MPS2 * delta_seconds;
            }
            assert!(
                (height - 1.25).abs() < 0.000_1,
                "{hertz} Hz discrete apex was {height}"
            );
        }
        assert!(
            (jump_launch_speed_mps(1.25, REFERENCE_FIXED_HZ.recip()) - 6.841_429).abs() < 0.000_01
        );
    }

    #[test]
    fn steep_climb_rejection_preserves_vertical_collision_response() {
        let normal = Vec3::new(0.0, 0.5, 0.866_025_4);

        let mut horizontal = Vec3::NEG_Z;
        reject_horizontal_steep_climb(normal, &mut horizontal);
        assert_eq!(horizontal, Vec3::ZERO);

        let mut falling = Vec3::NEG_Y;
        reject_horizontal_steep_climb(normal, &mut falling);
        assert_eq!(falling, Vec3::NEG_Y);
    }

    #[test]
    fn slanted_ceiling_keeps_its_actual_collision_plane() {
        let ceiling_normal = Vec3::new(0.0, -0.94, 0.342);
        let mut rising = Vec3::Y;
        if ceiling_normal.y > 0.0 {
            reject_horizontal_steep_climb(ceiling_normal, &mut rising);
        }
        assert_eq!(rising, Vec3::Y);
    }

    #[test]
    fn coyote_window_accepts_six_ticks_and_rejects_seven() {
        let profile = PlayerMovementProfileV1::default();
        let delta = Duration::from_nanos(1_000_000_000 / 60);
        let mut accepted = PlayerControllerState {
            grounded: false,
            time_since_grounded: delta.saturating_mul(6),
            jump_buffer_remaining: Duration::ZERO,
            flying: false,
            time_since_jump_started: Duration::MAX,
        };
        let accepted_velocity = tick_vertical(profile, true, &mut accepted, -1.0, delta);
        assert_eq!(
            accepted_velocity,
            jump_launch_speed_mps(profile.jump_apex_m(), delta.as_secs_f32())
        );

        let mut rejected = PlayerControllerState {
            grounded: false,
            time_since_grounded: delta.saturating_mul(7),
            jump_buffer_remaining: Duration::ZERO,
            flying: false,
            time_since_jump_started: Duration::MAX,
        };
        let rejected_velocity = tick_vertical(profile, true, &mut rejected, -1.0, delta);
        assert!(rejected_velocity < 0.0);
    }

    #[test]
    fn jump_buffer_accepts_landing_on_sixth_tick_and_rejects_seventh() {
        let profile = PlayerMovementProfileV1::default();
        let delta = Duration::from_nanos(1_000_000_000 / 60);
        let airborne = PlayerControllerState {
            grounded: false,
            time_since_grounded: Duration::MAX,
            jump_buffer_remaining: Duration::ZERO,
            flying: false,
            time_since_jump_started: Duration::MAX,
        };

        let mut sixth_tick = airborne;
        let mut velocity = tick_vertical(profile, true, &mut sixth_tick, -1.0, delta);
        for _ in 0..4 {
            velocity = tick_vertical(profile, false, &mut sixth_tick, velocity, delta);
        }
        sixth_tick.grounded = true;
        velocity = tick_vertical(profile, false, &mut sixth_tick, velocity, delta);
        assert_eq!(
            velocity,
            jump_launch_speed_mps(profile.jump_apex_m(), delta.as_secs_f32())
        );

        let mut seventh_tick = airborne;
        let mut velocity = tick_vertical(profile, true, &mut seventh_tick, -1.0, delta);
        for _ in 0..5 {
            velocity = tick_vertical(profile, false, &mut seventh_tick, velocity, delta);
        }
        seventh_tick.grounded = true;
        velocity = tick_vertical(profile, false, &mut seventh_tick, velocity, delta);
        assert_eq!(velocity, 0.0);
    }

    #[test]
    fn sprint_requires_forward_and_is_blocked_by_sneak() {
        let walk = PlayerMovementProfileV1::default().maximum_walk_speed_mps();
        assert!(
            (horizontal_speed_mps(walk, false, true, false, 1.0) - walk * SPRINT_MULTIPLIER).abs()
                < f32::EPSILON
        );
        assert_eq!(horizontal_speed_mps(walk, false, true, false, 0.0), walk);
        assert_eq!(horizontal_speed_mps(walk, false, true, true, 1.0), walk);
        assert_eq!(
            horizontal_speed_mps(walk, true, false, false, 0.0),
            FLY_SPEED_MPS
        );
        assert_eq!(
            horizontal_speed_mps(walk, true, true, false, 0.0),
            FLY_SPEED_MPS * FLY_SPRINT_MULTIPLIER
        );
    }

    #[test]
    fn flight_toggle_window_has_stable_inclusive_tick_boundaries() {
        let profile = PlayerMovementProfileV1::default();
        for (hertz, boundary_ticks) in [(60_u32, 21_u32), (240, 84), (10_000, 3_500)] {
            let delta = Duration::from_secs_f64(1.0 / f64::from(hertz));
            let base = PlayerControllerState {
                grounded: false,
                time_since_grounded: delta,
                jump_buffer_remaining: Duration::ZERO,
                flying: false,
                time_since_jump_started: delta.saturating_mul(boundary_ticks),
            };

            let mut inclusive = base;
            apply_jump_and_fly_toggle(profile, true, &mut inclusive, delta);
            assert!(inclusive.flying, "{hertz} Hz boundary must be inclusive");

            let mut outside = PlayerControllerState {
                time_since_jump_started: delta.saturating_mul(boundary_ticks + 1),
                ..base
            };
            apply_jump_and_fly_toggle(profile, true, &mut outside, delta);
            assert!(!outside.flying, "{hertz} Hz next tick must be outside");
        }
    }

    #[test]
    fn airborne_double_jump_toggles_flight_and_cancels_gravity() {
        let profile = PlayerMovementProfileV1::default();
        let delta = Duration::from_nanos(1_000_000_000 / 60);
        let mut state = PlayerControllerState {
            grounded: false,
            time_since_grounded: delta,
            jump_buffer_remaining: Duration::ZERO,
            flying: false,
            time_since_jump_started: delta,
        };
        let velocity = tick_vertical(
            profile,
            true,
            &mut state,
            jump_launch_speed_mps(profile.jump_apex_m(), delta.as_secs_f32()),
            delta,
        );
        assert!(state.flying);
        assert_eq!(velocity, FLY_SPEED_MPS);

        let hover = tick_vertical(profile, false, &mut state, 0.0, delta);
        assert_eq!(hover, 0.0);

        apply_jump_and_fly_toggle(profile, false, &mut state, delta);
        let climb = prepared_vertical_velocity(profile, true, false, false, &mut state, 0.0, delta);
        assert_eq!(climb, FLY_SPEED_MPS);
        apply_jump_and_fly_toggle(profile, false, &mut state, delta);
        let descend =
            prepared_vertical_velocity(profile, false, false, true, &mut state, 0.0, delta);
        assert_eq!(descend, -FLY_SPEED_MPS);
    }

    fn tick_vertical(
        profile: PlayerMovementProfileV1,
        jump_started: bool,
        state: &mut PlayerControllerState,
        current_velocity_y: f32,
        delta: Duration,
    ) -> f32 {
        apply_jump_and_fly_toggle(profile, jump_started, state, delta);
        prepared_vertical_velocity(
            profile,
            jump_started,
            false,
            false,
            state,
            current_velocity_y,
            delta,
        )
    }
}
