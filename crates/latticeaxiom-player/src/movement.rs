use std::f32::consts::{FRAC_PI_2, FRAC_PI_4};

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

use crate::{ActionFrameInbox, PlayerActionFrameV1, PlayerActionV1, SuccessfulEditCooldownV1};

pub(crate) const FIXED_HZ: f32 = 60.0;
const GRAVITY_MPS2: f32 = 19.62;
// Derived for a 1.25 m semi-implicit 60 Hz apex with 21 rising steps.
const JUMP_SPEED_MPS: f32 = 6.841_429;
const GROUND_SNAP_M: f32 = 0.10;
const CONTROLLER_SKIN_M: f32 = 0.01;
const MAX_SUPPORT_QUERY_HITS: u32 = 64;
const STEP_EPSILON_M: f32 = 0.001;
const SPECTATOR_SPEED_MPS: f32 = 12.0;
const SPRINT_MULTIPLIER: f32 = 1.3;
const FLY_SPEED_MPS: f32 = 10.80;
const FLY_SPRINT_MULTIPLIER: f32 = 2.0;
const FLY_TOGGLE_WINDOW_TICKS: u8 = 21;

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
    jump_buffer_ticks: u8,
    coyote_ticks: u8,
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

    /// Returns the jump input-buffer duration in fixed ticks.
    #[must_use]
    pub const fn jump_buffer_ticks(self) -> u8 {
        self.jump_buffer_ticks
    }

    /// Returns the coyote-time duration in fixed ticks.
    #[must_use]
    pub const fn coyote_ticks(self) -> u8 {
        self.coyote_ticks
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
        if self.jump_buffer_ticks == 0 || self.coyote_ticks == 0 {
            return Err(PlayerMovementProfileError::ZeroTickWindow);
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
            jump_buffer_ticks: 6,
            coyote_ticks: 6,
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
    /// Jump buffering or coyote time was disabled.
    #[error("jump buffer and coyote windows must contain at least one fixed tick")]
    ZeroTickWindow,
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
    ticks_since_grounded: u8,
    jump_buffer_remaining: u8,
    flying: bool,
    ticks_since_jump_started: u8,
}

impl PlayerControllerState {
    /// Returns whether the most recent controller probe found walkable ground.
    #[must_use]
    pub const fn grounded(self) -> bool {
        self.grounded
    }

    /// Returns fixed ticks elapsed since walkable ground was observed.
    #[must_use]
    pub const fn ticks_since_grounded(self) -> u8 {
        self.ticks_since_grounded
    }

    /// Returns buffered jump ticks remaining.
    #[must_use]
    pub const fn jump_buffer_remaining(self) -> u8 {
        self.jump_buffer_remaining
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
            ticks_since_grounded: u8::MAX,
            jump_buffer_remaining: 0,
            flying: false,
            ticks_since_jump_started: u8::MAX,
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
    edit_cooldown: SuccessfulEditCooldownV1,
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
            edit_cooldown: SuccessfulEditCooldownV1::default(),
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
    for (entity, collider, mut transform, profile, mut state) in &mut players {
        if state.flying {
            state.grounded = false;
            state.ticks_since_grounded = state.ticks_since_grounded.saturating_add(1);
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
            state.ticks_since_grounded = 0;
        } else {
            state.grounded = false;
            state.ticks_since_grounded = state.ticks_since_grounded.saturating_add(1);
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
    let delta_seconds = time.delta_secs();
    for (profile, frame, view, mut state, mut velocity) in &mut players {
        let input = frame.0.movement.clamp_unit();
        let jump_started = frame.0.started.contains(PlayerActionV1::Jump);
        let jump_held = frame.0.held.contains(PlayerActionV1::Jump);
        let sprint_held = frame.0.held.contains(PlayerActionV1::Sprint);
        let sneak_held = frame.0.held.contains(PlayerActionV1::Sneak);
        apply_jump_and_fly_toggle(*profile, jump_started, &mut state);
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
            delta_seconds,
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
) {
    if jump_started {
        let within_toggle_window = state.ticks_since_jump_started <= FLY_TOGGLE_WINDOW_TICKS;
        if state.flying && within_toggle_window {
            state.flying = false;
            state.jump_buffer_remaining = 0;
        } else if !state.flying && !state.grounded && within_toggle_window {
            state.flying = true;
            state.jump_buffer_remaining = 0;
        } else if !state.flying {
            state.jump_buffer_remaining = profile.jump_buffer_ticks;
        }
        state.ticks_since_jump_started = 0;
    }
    state.ticks_since_jump_started = state.ticks_since_jump_started.saturating_add(1);
}

fn prepared_vertical_velocity(
    profile: PlayerMovementProfileV1,
    jump_held: bool,
    sprint_held: bool,
    sneak_held: bool,
    state: &mut PlayerControllerState,
    current_velocity_y: f32,
    delta_seconds: f32,
) -> f32 {
    if state.flying {
        state.jump_buffer_remaining = 0;
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
    let within_coyote = state.grounded || state.ticks_since_grounded <= profile.coyote_ticks;
    if state.jump_buffer_remaining > 0 && within_coyote {
        state.jump_buffer_remaining = 0;
        state.grounded = false;
        state.ticks_since_grounded = profile.coyote_ticks.saturating_add(1);
        JUMP_SPEED_MPS
    } else {
        state.jump_buffer_remaining = state.jump_buffer_remaining.saturating_sub(1);
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
            state.ticks_since_grounded = 0;
        } else {
            transform.translation = direct.position.f32();
            velocity.0 = direct.projected_velocity;
            if hit_walkable_ground && velocity.y <= 0.0 {
                velocity.y = 0.0;
                state.flying = false;
                state.grounded = true;
                state.ticks_since_grounded = 0;
            }
        }
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
        assert_eq!(profile.jump_buffer_ticks(), 6);
        assert_eq!(profile.coyote_ticks(), 6);
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
    fn fixed_profile_rate_is_sixty_hz() {
        assert_eq!(FIXED_HZ, 60.0);
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
        let delta_seconds = FIXED_HZ.recip();
        let mut accepted = PlayerControllerState {
            grounded: false,
            ticks_since_grounded: 6,
            jump_buffer_remaining: 0,
            flying: false,
            ticks_since_jump_started: u8::MAX,
        };
        let accepted_velocity = tick_vertical(profile, true, &mut accepted, -1.0, delta_seconds);
        assert_eq!(accepted_velocity, JUMP_SPEED_MPS);

        let mut rejected = PlayerControllerState {
            grounded: false,
            ticks_since_grounded: 7,
            jump_buffer_remaining: 0,
            flying: false,
            ticks_since_jump_started: u8::MAX,
        };
        let rejected_velocity = tick_vertical(profile, true, &mut rejected, -1.0, delta_seconds);
        assert!(rejected_velocity < 0.0);
    }

    #[test]
    fn jump_buffer_accepts_landing_on_sixth_tick_and_rejects_seventh() {
        let profile = PlayerMovementProfileV1::default();
        let delta_seconds = FIXED_HZ.recip();
        let airborne = PlayerControllerState {
            grounded: false,
            ticks_since_grounded: u8::MAX,
            jump_buffer_remaining: 0,
            flying: false,
            ticks_since_jump_started: u8::MAX,
        };

        let mut sixth_tick = airborne;
        let mut velocity = tick_vertical(profile, true, &mut sixth_tick, -1.0, delta_seconds);
        for _ in 0..4 {
            velocity = tick_vertical(profile, false, &mut sixth_tick, velocity, delta_seconds);
        }
        sixth_tick.grounded = true;
        velocity = tick_vertical(profile, false, &mut sixth_tick, velocity, delta_seconds);
        assert_eq!(velocity, JUMP_SPEED_MPS);

        let mut seventh_tick = airborne;
        let mut velocity = tick_vertical(profile, true, &mut seventh_tick, -1.0, delta_seconds);
        for _ in 0..5 {
            velocity = tick_vertical(profile, false, &mut seventh_tick, velocity, delta_seconds);
        }
        seventh_tick.grounded = true;
        velocity = tick_vertical(profile, false, &mut seventh_tick, velocity, delta_seconds);
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
    fn airborne_double_jump_toggles_flight_and_cancels_gravity() {
        let profile = PlayerMovementProfileV1::default();
        let delta_seconds = FIXED_HZ.recip();
        let mut state = PlayerControllerState {
            grounded: false,
            ticks_since_grounded: 1,
            jump_buffer_remaining: 0,
            flying: false,
            ticks_since_jump_started: 1,
        };
        let velocity = tick_vertical(profile, true, &mut state, JUMP_SPEED_MPS, delta_seconds);
        assert!(state.flying);
        assert_eq!(velocity, FLY_SPEED_MPS);

        let hover = tick_vertical(profile, false, &mut state, 0.0, delta_seconds);
        assert_eq!(hover, 0.0);

        apply_jump_and_fly_toggle(profile, false, &mut state);
        let climb =
            prepared_vertical_velocity(profile, true, false, false, &mut state, 0.0, delta_seconds);
        assert_eq!(climb, FLY_SPEED_MPS);
        apply_jump_and_fly_toggle(profile, false, &mut state);
        let descend =
            prepared_vertical_velocity(profile, false, false, true, &mut state, 0.0, delta_seconds);
        assert_eq!(descend, -FLY_SPEED_MPS);
    }

    fn tick_vertical(
        profile: PlayerMovementProfileV1,
        jump_started: bool,
        state: &mut PlayerControllerState,
        current_velocity_y: f32,
        delta_seconds: f32,
    ) -> f32 {
        apply_jump_and_fly_toggle(profile, jump_started, state);
        prepared_vertical_velocity(
            profile,
            jump_started,
            false,
            false,
            state,
            current_velocity_y,
            delta_seconds,
        )
    }
}
