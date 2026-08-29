use avian3d::prelude::PhysicsSystems;
use bevy::{
    app::{App, Plugin},
    ecs::schedule::{IntoScheduleConfigs, SystemSet},
    prelude::{
        FixedFirst, FixedLast, FixedPostUpdate, FixedUpdate, MessageReader, MessageWriter, Query,
        Res, ResMut, Resource, Time, Transform, With,
    },
    time::Fixed,
};

use crate::{
    ActionFrameInbox, AuthoritativeBlockEditRequestV1, AuthoritativeMiningCancelRequestV1,
    BlockEditActionV1, BlockEditAuthorityResource, BlockEditInputStateV1, BlockEditIntentV1,
    BlockEditReceiptV1, BlockEditRejectV1, CurrentPlayerActionFrame, D2Player, DetachedSpectator,
    LocalPlayerInput, MiningCancelReceiptV1, PlayerMovementProfileV1, PlayerViewV1,
    SimulationClock, SimulationTickRateChanged, SimulationTickRateRequest, TargetEyePoseV1,
    movement::{
        install_fixed_action_frame, move_players, prepare_velocity, update_grounded,
        update_player_view, update_spectator,
    },
};

/// Named controller stages mapped onto Bevy's fixed main cycle.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, SystemSet)]
pub enum PlayerSystemSet {
    /// Install one stable action frame at the start of a fixed tick.
    SampleInput,
    /// Apply authoritative view deltas or local spectator motion.
    UpdateView,
    /// Query walkable ground against the previous completed physics state.
    ProbeGround,
    /// Resolve sprint, flight, buffered jump, coyote time, gravity, and target velocity.
    PrepareMovement,
    /// Move the kinematic capsule through Avian queries.
    MoveCapsule,
    /// Evaluate edits after Avian has completed `FixedPostUpdate` writeback.
    EvaluateEdit,
    /// Advance the stable fixed-tick counter in `FixedLast`.
    AdvanceTick,
}

/// Monotonic authoritative fixed tick owned by the player adapter.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Resource)]
pub struct PlayerFixedTick(u64);

impl PlayerFixedTick {
    /// Returns the current tick used for movement and edit receipts.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

/// Installs the D2 player controller without owning the Bevy app or Avian group.
///
/// The host must add `PhysicsPlugins::default()`. This plugin deliberately
/// schedules the controller in `FixedUpdate`, authoritative edit evaluation
/// after `PhysicsSystems::Last` in Avian's default `FixedPostUpdate`, and the
/// tick commit in `FixedLast`.
#[derive(Clone, Copy, Debug, Default)]
pub struct PlayerPlugin;

impl Plugin for PlayerPlugin {
    fn build(&self, app: &mut App) {
        let clock = SimulationClock::default();
        app.insert_resource(Time::<Fixed>::from_duration(clock.active_rate().timestep()))
            .insert_resource(clock)
            .init_resource::<ActionFrameInbox>()
            .init_resource::<PlayerFixedTick>()
            .add_message::<BlockEditReceiptV1>()
            .add_message::<MiningCancelReceiptV1>()
            .add_message::<SimulationTickRateRequest>()
            .add_message::<SimulationTickRateChanged>()
            .configure_sets(FixedFirst, PlayerSystemSet::SampleInput)
            .configure_sets(
                FixedUpdate,
                (
                    PlayerSystemSet::UpdateView,
                    PlayerSystemSet::ProbeGround,
                    PlayerSystemSet::PrepareMovement,
                    PlayerSystemSet::MoveCapsule,
                )
                    .chain(),
            )
            .configure_sets(
                FixedPostUpdate,
                PlayerSystemSet::EvaluateEdit.after(PhysicsSystems::Last),
            )
            .configure_sets(FixedLast, PlayerSystemSet::AdvanceTick)
            .add_systems(
                FixedFirst,
                install_fixed_action_frame.in_set(PlayerSystemSet::SampleInput),
            )
            .add_systems(
                FixedUpdate,
                (update_player_view, update_spectator).in_set(PlayerSystemSet::UpdateView),
            )
            .add_systems(
                FixedUpdate,
                update_grounded.in_set(PlayerSystemSet::ProbeGround),
            )
            .add_systems(
                FixedUpdate,
                prepare_velocity.in_set(PlayerSystemSet::PrepareMovement),
            )
            .add_systems(
                FixedUpdate,
                move_players.in_set(PlayerSystemSet::MoveCapsule),
            )
            .add_systems(
                FixedPostUpdate,
                evaluate_block_edits.in_set(PlayerSystemSet::EvaluateEdit),
            )
            .add_systems(
                FixedLast,
                (advance_fixed_tick, apply_tick_rate_requests)
                    .chain()
                    .in_set(PlayerSystemSet::AdvanceTick),
            );
    }
}

#[allow(clippy::needless_pass_by_value)] // Bevy systems receive SystemParams by value.
#[allow(clippy::type_complexity)] // The tuple is the single bounded local-player command origin.
fn evaluate_block_edits(
    tick: Res<'_, PlayerFixedTick>,
    clock: Res<'_, SimulationClock>,
    mut authority: Option<ResMut<'_, BlockEditAuthorityResource>>,
    mut receipts: MessageWriter<'_, BlockEditReceiptV1>,
    mut cancel_receipts: MessageWriter<'_, MiningCancelReceiptV1>,
    mut players: Query<
        '_,
        '_,
        (
            &D2Player,
            &CurrentPlayerActionFrame,
            &PlayerMovementProfileV1,
            &PlayerViewV1,
            &Transform,
            &mut BlockEditInputStateV1,
            Option<&DetachedSpectator>,
        ),
        With<LocalPlayerInput>,
    >,
) {
    // D2 has exactly one local command origin. Treat zero or multiple local
    // players as an invalid host composition and emit no order-dependent work.
    let mut local_players = players.iter_mut();
    let Some((player, frame, profile, view, transform, mut edit_input, spectator)) =
        local_players.next()
    else {
        return;
    };
    if local_players.next().is_some() {
        return;
    }

    let break_active = frame.0.held.contains(crate::PlayerActionV1::BreakBlock)
        || frame.0.started.contains(crate::PlayerActionV1::BreakBlock);
    let break_sample = edit_input.sample_break(break_active, clock.active_rate().hertz());
    if break_sample.released {
        let result = if spectator.is_some() {
            Err(BlockEditRejectV1::PermissionDenied)
        } else if let Some(authority) = authority.as_mut() {
            authority.cancel_mining(AuthoritativeMiningCancelRequestV1 {
                player: player.player_id,
                fixed_tick: tick.get(),
                input_generation: frame.0.generation,
            })
        } else {
            Err(BlockEditRejectV1::ContentUnavailable)
        };
        cancel_receipts.write(MiningCancelReceiptV1 {
            player: player.player_id,
            fixed_tick: tick.get(),
            input_generation: frame.0.generation,
            result,
        });
    }

    let place_steps = frame
        .0
        .started
        .contains(crate::PlayerActionV1::PlaceBlock)
        .then_some(latticeaxiom_gameplay::MiningStepCountV1::ONE);
    for (action, mining_steps) in [
        (BlockEditActionV1::Break, break_sample.steps),
        (BlockEditActionV1::Place, place_steps),
    ] {
        let Some(mining_steps) = mining_steps else {
            continue;
        };

        let result = if spectator.is_some() {
            Err(BlockEditRejectV1::PermissionDenied)
        } else if let Some(authority) = authority.as_mut() {
            let origin = transform.translation
                + bevy::prelude::Vec3::Y
                    * (profile.eye_height_m() - profile.capsule_total_height_m() * 0.5);
            let forward = view.forward();
            authority.apply(AuthoritativeBlockEditRequestV1 {
                player: player.player_id,
                fixed_tick: tick.get(),
                eye_pose: TargetEyePoseV1 {
                    origin_m: origin.to_array(),
                    forward: forward.to_array(),
                },
                intent: BlockEditIntentV1 {
                    action,
                    input_generation: frame.0.generation,
                    placement_content: frame.0.placement_content.clone(),
                    client_observation: frame.0.client_observation.clone(),
                    mining_steps,
                },
            })
        } else {
            Err(BlockEditRejectV1::ContentUnavailable)
        };

        receipts.write(BlockEditReceiptV1 {
            player: player.player_id,
            fixed_tick: tick.get(),
            action,
            input_generation: frame.0.generation,
            result,
        });
    }
}

#[allow(clippy::needless_pass_by_value)] // Bevy systems receive SystemParams by value.
fn advance_fixed_tick(mut tick: ResMut<'_, PlayerFixedTick>) {
    tick.0 = tick.0.saturating_add(1);
}

#[allow(clippy::needless_pass_by_value)] // Bevy systems receive SystemParams by value.
fn apply_tick_rate_requests(
    mut requests: MessageReader<'_, '_, SimulationTickRateRequest>,
    mut receipts: MessageWriter<'_, SimulationTickRateChanged>,
    mut clock: ResMut<'_, SimulationClock>,
    mut fixed_time: ResMut<'_, Time<Fixed>>,
) {
    let Some(request) = requests.read().copied().last() else {
        return;
    };
    let changed = clock.activate(request.rate);
    if changed {
        fixed_time.set_timestep(request.rate.timestep());
    }
    receipts.write(SimulationTickRateChanged {
        request_id: request.request_id,
        rate: request.rate,
        revision: clock.revision(),
        changed,
    });
}
