//! Keyboard and mouse input for the local playable slice.

use bevy::{
    app::{App, Plugin, PreUpdate},
    ecs::schedule::IntoScheduleConfigs,
    input::{
        ButtonInput, InputSystems,
        keyboard::KeyCode,
        mouse::{AccumulatedMouseMotion, MouseButton},
    },
    prelude::{Res, ResMut, Resource},
};
use latticeaxiom_gameplay::BlockId;
use latticeaxiom_player::{
    ActionAxis2V1, ActionFrameInbox, PlayerActionButtonsV1, PlayerActionFrameV1, PlayerActionV1,
};

const MOUSE_LOOK_RADIANS_PER_PIXEL: f32 = 0.002;

/// Installs the direct keyboard-and-mouse adapter used by the playable slice.
#[derive(Clone, Debug)]
pub(crate) struct PlayableInputPlugin {
    placement_content: BlockId,
}

impl PlayableInputPlugin {
    /// Creates an input adapter whose place action selects `placement_content`.
    pub(crate) const fn new(placement_content: BlockId) -> Self {
        Self { placement_content }
    }
}

impl Plugin for PlayableInputPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ActionFrameInbox>()
            .insert_resource(PlayableInputState {
                generation: 0,
                placement_content: self.placement_content.clone(),
            })
            .add_systems(PreUpdate, sample_keyboard_and_mouse.after(InputSystems));
    }
}

#[derive(Debug, Resource)]
struct PlayableInputState {
    generation: u64,
    placement_content: BlockId,
}

#[allow(clippy::needless_pass_by_value)] // Bevy systems receive SystemParams by value.
fn sample_keyboard_and_mouse(
    keyboard: Res<'_, ButtonInput<KeyCode>>,
    mouse: Res<'_, ButtonInput<MouseButton>>,
    mouse_motion: Res<'_, AccumulatedMouseMotion>,
    mut state: ResMut<'_, PlayableInputState>,
    mut inbox: ResMut<'_, ActionFrameInbox>,
) {
    state.generation = state.generation.saturating_add(1);

    let movement = ActionAxis2V1::finite_or_zero(
        axis(
            keyboard.pressed(KeyCode::KeyD),
            keyboard.pressed(KeyCode::KeyA),
        ),
        axis(
            keyboard.pressed(KeyCode::KeyW),
            keyboard.pressed(KeyCode::KeyS),
        ),
    );
    let look_radians = ActionAxis2V1::finite_or_zero(
        mouse_motion.delta.x * MOUSE_LOOK_RADIANS_PER_PIXEL,
        mouse_motion.delta.y * MOUSE_LOOK_RADIANS_PER_PIXEL,
    );

    let mut held = PlayerActionButtonsV1::empty();
    let mut started = PlayerActionButtonsV1::empty();
    if keyboard.pressed(KeyCode::Space) {
        held.insert(PlayerActionV1::Jump);
    }
    if keyboard.just_pressed(KeyCode::Space) {
        started.insert(PlayerActionV1::Jump);
    }
    if mouse.just_pressed(MouseButton::Left) {
        started.insert(PlayerActionV1::BreakBlock);
    }
    if mouse.just_pressed(MouseButton::Right) {
        started.insert(PlayerActionV1::PlaceBlock);
    }

    inbox.publish_live(PlayerActionFrameV1 {
        generation: state.generation,
        movement,
        look_radians,
        held,
        started,
        placement_content: Some(state.placement_content.clone()),
        client_observation: None,
    });
}

const fn axis(positive: bool, negative: bool) -> f32 {
    match (positive, negative) {
        (true, false) => 1.0,
        (false, true) => -1.0,
        (true, true) | (false, false) => 0.0,
    }
}
