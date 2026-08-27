use avian3d::prelude::PhysicsSystems;
use bevy::{
    app::{App, Plugin},
    ecs::schedule::{IntoScheduleConfigs, SystemSet},
    prelude::{
        FixedFirst, FixedLast, FixedPostUpdate, FixedUpdate, MessageWriter, Query, Res, ResMut,
        Resource, Time, Transform, With,
    },
    time::Fixed,
};

use crate::{
    ActionFrameInbox, AuthoritativeBlockEditRequestV1, BlockEditActionV1,
    BlockEditAuthorityResource, BlockEditIntentV1, BlockEditReceiptV1, BlockEditRejectV1,
    CurrentPlayerActionFrame, D2Player, DetachedSpectator, LocalPlayerInput,
    PlayerMovementProfileV1, PlayerViewV1, SuccessfulEditCooldownV1, TargetEyePoseV1,
    movement::{
        FIXED_HZ, install_fixed_action_frame, move_players, prepare_velocity, update_grounded,
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

/// Monotonic authoritative 60 Hz tick owned by the player adapter.
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
        app.insert_resource(Time::<Fixed>::from_hz(f64::from(FIXED_HZ)))
            .init_resource::<ActionFrameInbox>()
            .init_resource::<PlayerFixedTick>()
            .add_message::<BlockEditReceiptV1>()
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
                advance_fixed_tick.in_set(PlayerSystemSet::AdvanceTick),
            );
    }
}

#[allow(clippy::needless_pass_by_value)] // Bevy systems receive SystemParams by value.
#[allow(clippy::type_complexity)] // The tuple is the single bounded local-player command origin.
fn evaluate_block_edits(
    tick: Res<'_, PlayerFixedTick>,
    mut authority: Option<ResMut<'_, BlockEditAuthorityResource>>,
    mut receipts: MessageWriter<'_, BlockEditReceiptV1>,
    mut players: Query<
        '_,
        '_,
        (
            &D2Player,
            &CurrentPlayerActionFrame,
            &PlayerMovementProfileV1,
            &PlayerViewV1,
            &Transform,
            &mut SuccessfulEditCooldownV1,
            Option<&DetachedSpectator>,
        ),
        With<LocalPlayerInput>,
    >,
) {
    // D2 has exactly one local command origin. Treat zero or multiple local
    // players as an invalid host composition and emit no order-dependent work.
    let mut local_players = players.iter_mut();
    let Some((player, frame, profile, view, transform, mut cooldown, spectator)) =
        local_players.next()
    else {
        return;
    };
    if local_players.next().is_some() {
        return;
    }

    for (action, source_action) in [
        (BlockEditActionV1::Break, crate::PlayerActionV1::BreakBlock),
        (BlockEditActionV1::Place, crate::PlayerActionV1::PlaceBlock),
    ] {
        if !frame.0.started.contains(source_action) {
            continue;
        }

        let result = if spectator.is_some() {
            Err(BlockEditRejectV1::PermissionDenied)
        } else {
            let remaining_ticks = cooldown.remaining_ticks(tick.get());
            if remaining_ticks > 0 {
                Err(BlockEditRejectV1::Cooldown { remaining_ticks })
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
                    },
                })
            } else {
                Err(BlockEditRejectV1::ContentUnavailable)
            }
        };

        if result.is_ok() {
            cooldown.record_success(tick.get());
        }
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
