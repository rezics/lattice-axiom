use bevy::{
    app::{App, Plugin},
    ecs::schedule::IntoScheduleConfigs,
    input::{gamepad::GamepadButton, keyboard::KeyCode, mouse::MouseButton},
    prelude::{Bundle, Query, Reflect, ResMut, With},
};
use leafwing_input_manager::{
    Actionlike,
    action_state::ActionState,
    input_map::InputMap,
    plugin::{InputManagerPlugin, InputManagerSystem},
    prelude::{GamepadStick, MouseMove, VirtualDPad, WithDualAxisProcessingPipelineExt},
};

use crate::{
    ActionAxis2V1, ActionFrameInbox, LocalPlayerInput, PlayerActionButtonsV1, PlayerActionFrameV1,
    PlayerActionV1,
};

const MOUSE_LOOK_RADIANS_PER_PIXEL: f32 = 0.002;
const GAMEPAD_LOOK_RADIANS_PER_SECOND: f32 = 2.1;

/// Runtime-only Leafwing action enum used by the static client adapter.
///
/// This type must not cross save, ABI, network, or portable-package boundaries.
#[derive(Actionlike, Clone, Copy, Debug, Eq, Hash, PartialEq, Reflect)]
pub enum LeafwingPlayerAction {
    /// Runtime movement axis.
    #[actionlike(DualAxis)]
    Move,
    /// Runtime look axis, configured to produce radians per input generation.
    #[actionlike(DualAxis)]
    LookDelta,
    /// Runtime held look rate, integrated once per fixed controller step.
    #[actionlike(DualAxis)]
    LookRate,
    /// Runtime jump button.
    Jump,
    /// Runtime break button.
    BreakBlock,
    /// Runtime place button.
    PlaceBlock,
    /// Runtime inspect button.
    Inspect,
    /// Runtime pause button.
    Pause,
    /// Runtime focused-control activation button.
    SurfaceActivate,
}

/// Returns the provisional keyboard/mouse and standard-gamepad binding map.
///
/// The returned component requires Leafwing's `ActionState` on the same local
/// player entity. Settings can replace this map without changing any stable
/// [`PlayerActionV1`] or [`PlayerActionFrameV1`] contract.
#[must_use]
pub fn default_leafwing_input_map() -> InputMap<LeafwingPlayerAction> {
    let mut input_map = InputMap::default();
    input_map
        .insert_dual_axis(LeafwingPlayerAction::Move, VirtualDPad::wasd())
        .insert_dual_axis(
            LeafwingPlayerAction::Move,
            GamepadStick::LEFT.with_circle_deadzone(0.1),
        )
        .insert_dual_axis(
            LeafwingPlayerAction::LookDelta,
            MouseMove::default().sensitivity(MOUSE_LOOK_RADIANS_PER_PIXEL),
        )
        .insert_dual_axis(
            LeafwingPlayerAction::LookRate,
            GamepadStick::RIGHT.with_circle_deadzone(0.1),
        )
        .insert(LeafwingPlayerAction::Jump, KeyCode::Space)
        .insert(LeafwingPlayerAction::Jump, GamepadButton::South)
        .insert(LeafwingPlayerAction::BreakBlock, MouseButton::Left)
        .insert(LeafwingPlayerAction::BreakBlock, GamepadButton::West)
        .insert(LeafwingPlayerAction::PlaceBlock, MouseButton::Right)
        .insert(LeafwingPlayerAction::PlaceBlock, GamepadButton::East)
        .insert(LeafwingPlayerAction::Inspect, KeyCode::KeyF)
        .insert(LeafwingPlayerAction::Inspect, GamepadButton::North)
        .insert(LeafwingPlayerAction::Pause, KeyCode::Escape)
        .insert(LeafwingPlayerAction::Pause, GamepadButton::Start)
        .insert(LeafwingPlayerAction::SurfaceActivate, KeyCode::Enter)
        .insert(LeafwingPlayerAction::SurfaceActivate, GamepadButton::South);
    input_map
}

#[derive(Debug, Default, bevy::prelude::Resource)]
struct LeafwingInputGeneration(u64);

/// Leafwing action state and default map for the local production client player.
#[derive(Bundle, Debug)]
pub struct LocalPlayerClientInputBundle {
    action_state: ActionState<LeafwingPlayerAction>,
    input_map: InputMap<LeafwingPlayerAction>,
}

impl Default for LocalPlayerClientInputBundle {
    fn default() -> Self {
        Self {
            action_state: ActionState::default(),
            input_map: default_leafwing_input_map(),
        }
    }
}

/// Static client adapter from Leafwing 0.21 state to Lattice action frames.
#[derive(Clone, Copy, Debug, Default)]
pub struct LeafwingInputAdapterPlugin;

impl Plugin for LeafwingInputAdapterPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(InputManagerPlugin::<LeafwingPlayerAction>::default())
            .init_resource::<ActionFrameInbox>()
            .init_resource::<LeafwingInputGeneration>()
            .add_systems(
                bevy::prelude::PreUpdate,
                sample_leafwing_state.after(InputManagerSystem::Update),
            );
    }
}

fn sample_leafwing_state(
    action_states: Query<'_, '_, &ActionState<LeafwingPlayerAction>, With<LocalPlayerInput>>,
    mut inbox: ResMut<'_, ActionFrameInbox>,
    mut generation: ResMut<'_, LeafwingInputGeneration>,
) {
    generation.0 = generation.0.saturating_add(1);
    let Ok(action_state) = action_states.single() else {
        inbox.replace_live_with_neutral(generation.0);
        return;
    };
    let movement = action_state.axis_pair(&LeafwingPlayerAction::Move);
    let look_delta = action_state.axis_pair(&LeafwingPlayerAction::LookDelta);
    let look_rate = action_state.axis_pair(&LeafwingPlayerAction::LookRate);
    let look_rate_radians_per_second = ActionAxis2V1::finite_or_zero(
        look_rate.x * GAMEPAD_LOOK_RADIANS_PER_SECOND,
        -look_rate.y * GAMEPAD_LOOK_RADIANS_PER_SECOND,
    );
    let mut held = PlayerActionButtonsV1::empty();
    let mut started = PlayerActionButtonsV1::empty();
    for (runtime, stable) in [
        (LeafwingPlayerAction::Jump, PlayerActionV1::Jump),
        (LeafwingPlayerAction::BreakBlock, PlayerActionV1::BreakBlock),
        (LeafwingPlayerAction::PlaceBlock, PlayerActionV1::PlaceBlock),
        (LeafwingPlayerAction::Inspect, PlayerActionV1::Inspect),
        (LeafwingPlayerAction::Pause, PlayerActionV1::Pause),
        (
            LeafwingPlayerAction::SurfaceActivate,
            PlayerActionV1::SurfaceActivate,
        ),
    ] {
        if action_state.pressed(&runtime) {
            held.insert(stable);
        }
        if action_state.just_pressed(&runtime) {
            started.insert(stable);
        }
    }

    inbox.publish_live_with_look_rate(
        PlayerActionFrameV1 {
            generation: generation.0,
            movement: ActionAxis2V1::finite_or_zero(movement.x, movement.y),
            look_radians: ActionAxis2V1::finite_or_zero(look_delta.x, look_delta.y),
            held,
            started,
            placement_content: None,
            client_observation: None,
        },
        look_rate_radians_per_second,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runtime_enum_maps_to_stable_action_categories() {
        assert_eq!(
            LeafwingPlayerAction::Move.input_control_kind(),
            leafwing_input_manager::InputControlKind::DualAxis
        );
        assert_eq!(
            LeafwingPlayerAction::BreakBlock.input_control_kind(),
            leafwing_input_manager::InputControlKind::Button
        );
        assert_eq!(
            LeafwingPlayerAction::LookRate.input_control_kind(),
            leafwing_input_manager::InputControlKind::DualAxis
        );
    }

    #[test]
    fn standard_gamepad_can_activate_a_focused_surface() {
        let input_map = default_leafwing_input_map();
        let bindings = input_map
            .get_buttonlike(&LeafwingPlayerAction::SurfaceActivate)
            .expect("default map includes surface activation");
        assert!(bindings.iter().any(|binding| {
            Reflect::as_any(binding.as_ref()).downcast_ref::<GamepadButton>()
                == Some(&GamepadButton::South)
        }));
    }

    #[test]
    fn invalid_local_input_composition_publishes_a_neutral_sample() {
        let mut app = App::new();
        app.init_resource::<ActionFrameInbox>()
            .init_resource::<LeafwingInputGeneration>()
            .add_systems(bevy::prelude::Update, sample_leafwing_state)
            .world_mut()
            .spawn(LocalPlayerInput);
        let mut started = PlayerActionButtonsV1::empty();
        started.insert(PlayerActionV1::Jump);
        started.insert(PlayerActionV1::BreakBlock);
        let mut held = PlayerActionButtonsV1::empty();
        held.insert(PlayerActionV1::PlaceBlock);
        app.world_mut()
            .resource_mut::<ActionFrameInbox>()
            .publish_live_with_look_rate(
                PlayerActionFrameV1 {
                    generation: 0,
                    movement: ActionAxis2V1 { x: 1.0, y: 0.0 },
                    look_radians: ActionAxis2V1 { x: 0.4, y: -0.2 },
                    held,
                    started,
                    ..PlayerActionFrameV1::default()
                },
                ActionAxis2V1 { x: 2.1, y: -2.1 },
            );

        app.update();

        let frame = app
            .world_mut()
            .resource_mut::<ActionFrameInbox>()
            .next_fixed_frame_with_step_seconds(1.0);
        assert_eq!(frame.movement, ActionAxis2V1::default());
        assert_eq!(frame.look_radians, ActionAxis2V1::default());
        assert_eq!(frame.held, PlayerActionButtonsV1::empty());
        assert_eq!(frame.started, PlayerActionButtonsV1::empty());
    }
}
