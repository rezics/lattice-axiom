use std::collections::BTreeSet;

use bevy::{
    app::{App, Plugin},
    ecs::schedule::IntoScheduleConfigs,
    input::{gamepad::GamepadButton, keyboard::KeyCode, mouse::MouseButton},
    prelude::{Bundle, Query, Reflect, Res, ResMut, Resource, With},
};
use latticeaxiom_input::{
    AuthoritativePlayerActionV1, ClientSurfaceActionV1, CompiledInputCatalogV1,
    CompiledLeafwingMapV1, GamepadStickV1, LeafwingRecipeV1, MouseButtonV1, MouseWheelDirectionV1,
};
use leafwing_input_manager::{
    Actionlike,
    action_state::ActionState,
    input_map::InputMap,
    plugin::{InputManagerPlugin, InputManagerSystem},
    prelude::{
        GamepadStick, MouseMove, MouseScrollDirection, VirtualDPad,
        WithDualAxisProcessingPipelineExt,
    },
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
    /// Runtime pick-block button.
    PickBlock,
    /// Runtime sprint button.
    Sprint,
    /// Runtime sneak button.
    Sneak,
}

/// Runtime-only Leafwing enum for compiled [`ClientSurfaceActionV1`] rows.
#[derive(Actionlike, Clone, Copy, Debug, Eq, Hash, PartialEq, Reflect)]
pub enum LeafwingSurfaceAction {
    /// Move focus up.
    NavUp,
    /// Move focus down.
    NavDown,
    /// Move focus left.
    NavLeft,
    /// Move focus right.
    NavRight,
    /// Advance focus.
    NavNext,
    /// Move focus to the previous target.
    NavPrevious,
    /// Activate the focused control.
    Activate,
    /// Dismiss or return from the current surface.
    Back,
    /// Open or close pause.
    Pause,
    /// Toggle the inventory overlay.
    ToggleInventory,
    /// Toggle the workbench overlay.
    ToggleWorkbench,
    /// Select hotbar slot 1.
    HotbarSlot1,
    /// Select hotbar slot 2.
    HotbarSlot2,
    /// Select hotbar slot 3.
    HotbarSlot3,
    /// Select hotbar slot 4.
    HotbarSlot4,
    /// Select hotbar slot 5.
    HotbarSlot5,
    /// Select hotbar slot 6.
    HotbarSlot6,
    /// Select hotbar slot 7.
    HotbarSlot7,
    /// Select hotbar slot 8.
    HotbarSlot8,
    /// Select hotbar slot 9.
    HotbarSlot9,
    /// Advance the hotbar selection.
    HotbarNext,
    /// Move the hotbar selection backward.
    HotbarPrevious,
}

impl LeafwingSurfaceAction {
    fn from_surface(action: ClientSurfaceActionV1) -> Self {
        match action {
            ClientSurfaceActionV1::NavUp => Self::NavUp,
            ClientSurfaceActionV1::NavDown => Self::NavDown,
            ClientSurfaceActionV1::NavLeft => Self::NavLeft,
            ClientSurfaceActionV1::NavRight => Self::NavRight,
            ClientSurfaceActionV1::NavNext => Self::NavNext,
            ClientSurfaceActionV1::NavPrevious => Self::NavPrevious,
            ClientSurfaceActionV1::Activate => Self::Activate,
            ClientSurfaceActionV1::Back => Self::Back,
            ClientSurfaceActionV1::Pause => Self::Pause,
            ClientSurfaceActionV1::ToggleInventory => Self::ToggleInventory,
            ClientSurfaceActionV1::ToggleWorkbench => Self::ToggleWorkbench,
            ClientSurfaceActionV1::HotbarSlot1 => Self::HotbarSlot1,
            ClientSurfaceActionV1::HotbarSlot2 => Self::HotbarSlot2,
            ClientSurfaceActionV1::HotbarSlot3 => Self::HotbarSlot3,
            ClientSurfaceActionV1::HotbarSlot4 => Self::HotbarSlot4,
            ClientSurfaceActionV1::HotbarSlot5 => Self::HotbarSlot5,
            ClientSurfaceActionV1::HotbarSlot6 => Self::HotbarSlot6,
            ClientSurfaceActionV1::HotbarSlot7 => Self::HotbarSlot7,
            ClientSurfaceActionV1::HotbarSlot8 => Self::HotbarSlot8,
            ClientSurfaceActionV1::HotbarSlot9 => Self::HotbarSlot9,
            ClientSurfaceActionV1::HotbarNext => Self::HotbarNext,
            ClientSurfaceActionV1::HotbarPrevious => Self::HotbarPrevious,
        }
    }

    fn to_surface(self) -> ClientSurfaceActionV1 {
        match self {
            Self::NavUp => ClientSurfaceActionV1::NavUp,
            Self::NavDown => ClientSurfaceActionV1::NavDown,
            Self::NavLeft => ClientSurfaceActionV1::NavLeft,
            Self::NavRight => ClientSurfaceActionV1::NavRight,
            Self::NavNext => ClientSurfaceActionV1::NavNext,
            Self::NavPrevious => ClientSurfaceActionV1::NavPrevious,
            Self::Activate => ClientSurfaceActionV1::Activate,
            Self::Back => ClientSurfaceActionV1::Back,
            Self::Pause => ClientSurfaceActionV1::Pause,
            Self::ToggleInventory => ClientSurfaceActionV1::ToggleInventory,
            Self::ToggleWorkbench => ClientSurfaceActionV1::ToggleWorkbench,
            Self::HotbarSlot1 => ClientSurfaceActionV1::HotbarSlot1,
            Self::HotbarSlot2 => ClientSurfaceActionV1::HotbarSlot2,
            Self::HotbarSlot3 => ClientSurfaceActionV1::HotbarSlot3,
            Self::HotbarSlot4 => ClientSurfaceActionV1::HotbarSlot4,
            Self::HotbarSlot5 => ClientSurfaceActionV1::HotbarSlot5,
            Self::HotbarSlot6 => ClientSurfaceActionV1::HotbarSlot6,
            Self::HotbarSlot7 => ClientSurfaceActionV1::HotbarSlot7,
            Self::HotbarSlot8 => ClientSurfaceActionV1::HotbarSlot8,
            Self::HotbarSlot9 => ClientSurfaceActionV1::HotbarSlot9,
            Self::HotbarNext => ClientSurfaceActionV1::HotbarNext,
            Self::HotbarPrevious => ClientSurfaceActionV1::HotbarPrevious,
        }
    }
}

/// Compiled Leafwing maps for one lock-selected catalog.
#[derive(Clone, Debug, Resource)]
pub struct CompiledClientInputMaps {
    gameplay: InputMap<LeafwingPlayerAction>,
    surface: InputMap<LeafwingSurfaceAction>,
}

impl CompiledClientInputMaps {
    /// Returns the gameplay Leafwing map.
    #[must_use]
    pub fn gameplay(&self) -> &InputMap<LeafwingPlayerAction> {
        &self.gameplay
    }

    /// Returns the client-surface Leafwing map.
    #[must_use]
    pub fn surface(&self) -> &InputMap<LeafwingSurfaceAction> {
        &self.surface
    }
}

/// Latest client-surface edges sampled from the compiled catalog.
#[derive(Clone, Debug, Default, Resource)]
pub struct SurfaceActionFrame {
    started: BTreeSet<ClientSurfaceActionV1>,
    held: BTreeSet<ClientSurfaceActionV1>,
}

impl SurfaceActionFrame {
    /// Returns rising surface-action edges for this input generation.
    #[must_use]
    pub const fn started(&self) -> &BTreeSet<ClientSurfaceActionV1> {
        &self.started
    }

    /// Returns held surface actions for this input generation.
    #[must_use]
    pub const fn held(&self) -> &BTreeSet<ClientSurfaceActionV1> {
        &self.held
    }

    /// Returns whether `action` started this generation.
    #[must_use]
    pub fn just_started(&self, action: ClientSurfaceActionV1) -> bool {
        self.started.contains(&action)
    }

    /// Clears every recorded edge. Used after a context-stack transition.
    pub fn clear(&mut self) {
        self.started.clear();
        self.held.clear();
    }
}

/// When true, live gameplay axes and edit buttons are suppressed.
#[derive(Clone, Copy, Debug, Default, Resource)]
pub struct GameplaySuppressed(pub bool);

/// Returns the provisional keyboard/mouse and standard-gamepad binding map.
///
/// Production hosts must install [`leafwing_maps_from_catalog`] instead. This
/// default remains for adapter unit tests that do not boot a product lock.
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
        .insert(LeafwingPlayerAction::PickBlock, MouseButton::Middle)
        .insert(LeafwingPlayerAction::Sprint, KeyCode::ControlLeft)
        .insert(LeafwingPlayerAction::Sprint, KeyCode::ControlRight)
        .insert(LeafwingPlayerAction::Sprint, GamepadButton::LeftThumb)
        .insert(LeafwingPlayerAction::Sneak, KeyCode::ShiftLeft)
        .insert(LeafwingPlayerAction::Sneak, KeyCode::ShiftRight)
        .insert(LeafwingPlayerAction::Sneak, GamepadButton::RightThumb)
        .insert(LeafwingPlayerAction::Inspect, KeyCode::KeyF)
        .insert(LeafwingPlayerAction::Inspect, GamepadButton::North)
        .insert(LeafwingPlayerAction::Pause, KeyCode::Escape)
        .insert(LeafwingPlayerAction::Pause, GamepadButton::Start)
        .insert(LeafwingPlayerAction::SurfaceActivate, KeyCode::Enter)
        .insert(LeafwingPlayerAction::SurfaceActivate, GamepadButton::South);
    input_map
}

/// Compiles Leafwing maps from a lock-selected catalog.
#[must_use]
pub fn leafwing_maps_from_catalog(catalog: &CompiledInputCatalogV1) -> CompiledClientInputMaps {
    let mut surface = surface_map_from_compiled(catalog.surface_map());
    append_surface_recipes(&mut surface, catalog.inventory_overlay_map());
    append_surface_recipes(&mut surface, catalog.workbench_overlay_map());
    CompiledClientInputMaps {
        gameplay: gameplay_map_from_compiled(catalog.gameplay_map()),
        surface,
    }
}

fn append_surface_recipes(
    input_map: &mut InputMap<LeafwingSurfaceAction>,
    compiled: &CompiledLeafwingMapV1,
) {
    for entry in &compiled.entries {
        let Some(surface) = ClientSurfaceActionV1::ALL
            .into_iter()
            .find(|action| action.stable_id() == entry.action_id.as_str())
        else {
            continue;
        };
        let action = LeafwingSurfaceAction::from_surface(surface);
        for recipe in &entry.recipes {
            apply_surface_recipe(input_map, action, recipe);
        }
    }
}

fn gameplay_map_from_compiled(compiled: &CompiledLeafwingMapV1) -> InputMap<LeafwingPlayerAction> {
    let mut input_map = InputMap::default();
    for entry in &compiled.entries {
        let Some(player) = AuthoritativePlayerActionV1::ALL
            .into_iter()
            .find(|action| action.stable_id() == entry.action_id.as_str())
        else {
            continue;
        };
        for recipe in &entry.recipes {
            apply_player_recipe(&mut input_map, player, recipe);
        }
    }
    input_map
}

fn surface_map_from_compiled(compiled: &CompiledLeafwingMapV1) -> InputMap<LeafwingSurfaceAction> {
    let mut input_map = InputMap::default();
    for entry in &compiled.entries {
        let Some(surface) = ClientSurfaceActionV1::ALL
            .into_iter()
            .find(|action| action.stable_id() == entry.action_id.as_str())
        else {
            continue;
        };
        let action = LeafwingSurfaceAction::from_surface(surface);
        for recipe in &entry.recipes {
            apply_surface_recipe(&mut input_map, action, recipe);
        }
    }
    input_map
}

fn apply_player_recipe(
    input_map: &mut InputMap<LeafwingPlayerAction>,
    player: AuthoritativePlayerActionV1,
    recipe: &LeafwingRecipeV1,
) {
    match (player, recipe) {
        (
            AuthoritativePlayerActionV1::Move,
            LeafwingRecipeV1::VirtualDPad {
                up,
                down,
                left,
                right,
                ..
            },
        ) => {
            if let (Some(up), Some(down), Some(left), Some(right)) = (
                parse_key_code(up),
                parse_key_code(down),
                parse_key_code(left),
                parse_key_code(right),
            ) {
                input_map.insert_dual_axis(
                    LeafwingPlayerAction::Move,
                    VirtualDPad::new(up, down, left, right),
                );
            }
        }
        (
            AuthoritativePlayerActionV1::Move,
            LeafwingRecipeV1::GamepadStick {
                stick, deadzone, ..
            },
        ) => {
            input_map.insert_dual_axis(
                LeafwingPlayerAction::Move,
                gamepad_stick(*stick).with_circle_deadzone(*deadzone),
            );
        }
        (AuthoritativePlayerActionV1::Look, LeafwingRecipeV1::MouseMove { sensitivity }) => {
            input_map.insert_dual_axis(
                LeafwingPlayerAction::LookDelta,
                MouseMove::default().sensitivity(*sensitivity),
            );
        }
        (
            AuthoritativePlayerActionV1::Look,
            LeafwingRecipeV1::GamepadStick {
                stick, deadzone, ..
            },
        ) => {
            input_map.insert_dual_axis(
                LeafwingPlayerAction::LookRate,
                gamepad_stick(*stick).with_circle_deadzone(*deadzone),
            );
        }
        (_, LeafwingRecipeV1::Keyboard { usage, .. }) => {
            if let Some(key) = parse_key_code(usage)
                && let Some(action) = player_button(player)
            {
                input_map.insert(action, key);
            }
        }
        (_, LeafwingRecipeV1::MouseButton { button }) => {
            if let Some(action) = player_button(player) {
                input_map.insert(action, mouse_button(*button));
            }
        }
        (_, LeafwingRecipeV1::GamepadButton { button }) => {
            if let (Some(action), Some(gamepad)) =
                (player_button(player), parse_gamepad_button(button))
            {
                input_map.insert(action, gamepad);
            }
        }
        _ => {}
    }
}

fn apply_surface_recipe(
    input_map: &mut InputMap<LeafwingSurfaceAction>,
    action: LeafwingSurfaceAction,
    recipe: &LeafwingRecipeV1,
) {
    match recipe {
        LeafwingRecipeV1::Keyboard { usage, .. } => {
            if let Some(key) = parse_key_code(usage) {
                input_map.insert(action, key);
            }
        }
        LeafwingRecipeV1::MouseButton { button } => {
            input_map.insert(action, mouse_button(*button));
        }
        LeafwingRecipeV1::GamepadButton { button } => {
            if let Some(gamepad) = parse_gamepad_button(button) {
                input_map.insert(action, gamepad);
            }
        }
        LeafwingRecipeV1::MouseWheelDirection { direction } => {
            input_map.insert(action, mouse_scroll_direction(*direction));
        }
        _ => {}
    }
}

const fn mouse_scroll_direction(direction: MouseWheelDirectionV1) -> MouseScrollDirection {
    match direction {
        MouseWheelDirectionV1::Up => MouseScrollDirection::UP,
        MouseWheelDirectionV1::Down => MouseScrollDirection::DOWN,
        MouseWheelDirectionV1::Left => MouseScrollDirection::LEFT,
        MouseWheelDirectionV1::Right => MouseScrollDirection::RIGHT,
    }
}

fn player_button(player: AuthoritativePlayerActionV1) -> Option<LeafwingPlayerAction> {
    match player {
        AuthoritativePlayerActionV1::Jump => Some(LeafwingPlayerAction::Jump),
        AuthoritativePlayerActionV1::BreakBlock => Some(LeafwingPlayerAction::BreakBlock),
        AuthoritativePlayerActionV1::PlaceBlock => Some(LeafwingPlayerAction::PlaceBlock),
        AuthoritativePlayerActionV1::Inspect => Some(LeafwingPlayerAction::Inspect),
        AuthoritativePlayerActionV1::Pause => Some(LeafwingPlayerAction::Pause),
        AuthoritativePlayerActionV1::PickBlock => Some(LeafwingPlayerAction::PickBlock),
        AuthoritativePlayerActionV1::Sprint => Some(LeafwingPlayerAction::Sprint),
        AuthoritativePlayerActionV1::Sneak => Some(LeafwingPlayerAction::Sneak),
        AuthoritativePlayerActionV1::Move | AuthoritativePlayerActionV1::Look => None,
    }
}

fn gamepad_stick(stick: GamepadStickV1) -> GamepadStick {
    match stick {
        GamepadStickV1::Left => GamepadStick::LEFT,
        GamepadStickV1::Right => GamepadStick::RIGHT,
    }
}

fn mouse_button(button: MouseButtonV1) -> MouseButton {
    match button {
        MouseButtonV1::Left => MouseButton::Left,
        MouseButtonV1::Right => MouseButton::Right,
        MouseButtonV1::Middle => MouseButton::Middle,
        MouseButtonV1::Back => MouseButton::Back,
        MouseButtonV1::Forward => MouseButton::Forward,
    }
}

fn parse_gamepad_button(button: &str) -> Option<GamepadButton> {
    match button {
        "South" => Some(GamepadButton::South),
        "East" => Some(GamepadButton::East),
        "West" => Some(GamepadButton::West),
        "North" => Some(GamepadButton::North),
        "Start" => Some(GamepadButton::Start),
        "Select" => Some(GamepadButton::Select),
        "DPadUp" => Some(GamepadButton::DPadUp),
        "DPadDown" => Some(GamepadButton::DPadDown),
        "DPadLeft" => Some(GamepadButton::DPadLeft),
        "DPadRight" => Some(GamepadButton::DPadRight),
        "LeftTrigger" => Some(GamepadButton::LeftTrigger),
        "RightTrigger" => Some(GamepadButton::RightTrigger),
        "LeftTrigger2" => Some(GamepadButton::LeftTrigger2),
        "RightTrigger2" => Some(GamepadButton::RightTrigger2),
        "LeftThumb" => Some(GamepadButton::LeftThumb),
        "RightThumb" => Some(GamepadButton::RightThumb),
        _ => None,
    }
}

fn parse_key_code(usage: &str) -> Option<KeyCode> {
    match usage {
        "Escape" => Some(KeyCode::Escape),
        "Enter" | "NumpadEnter" => Some(KeyCode::Enter),
        "Space" => Some(KeyCode::Space),
        "Tab" => Some(KeyCode::Tab),
        "Backspace" => Some(KeyCode::Backspace),
        "ShiftLeft" => Some(KeyCode::ShiftLeft),
        "ShiftRight" => Some(KeyCode::ShiftRight),
        "ControlLeft" => Some(KeyCode::ControlLeft),
        "ControlRight" => Some(KeyCode::ControlRight),
        "AltLeft" => Some(KeyCode::AltLeft),
        "AltRight" => Some(KeyCode::AltRight),
        "ArrowUp" => Some(KeyCode::ArrowUp),
        "ArrowDown" => Some(KeyCode::ArrowDown),
        "ArrowLeft" => Some(KeyCode::ArrowLeft),
        "ArrowRight" => Some(KeyCode::ArrowRight),
        "Digit0" => Some(KeyCode::Digit0),
        "Digit1" => Some(KeyCode::Digit1),
        "Digit2" => Some(KeyCode::Digit2),
        "Digit3" => Some(KeyCode::Digit3),
        "Digit4" => Some(KeyCode::Digit4),
        "Digit5" => Some(KeyCode::Digit5),
        "Digit6" => Some(KeyCode::Digit6),
        "Digit7" => Some(KeyCode::Digit7),
        "Digit8" => Some(KeyCode::Digit8),
        "Digit9" => Some(KeyCode::Digit9),
        "Numpad0" => Some(KeyCode::Numpad0),
        "Numpad1" => Some(KeyCode::Numpad1),
        "Numpad2" => Some(KeyCode::Numpad2),
        "Numpad3" => Some(KeyCode::Numpad3),
        "Numpad4" => Some(KeyCode::Numpad4),
        "Numpad5" => Some(KeyCode::Numpad5),
        "Numpad6" => Some(KeyCode::Numpad6),
        "Numpad7" => Some(KeyCode::Numpad7),
        "Numpad8" => Some(KeyCode::Numpad8),
        "Numpad9" => Some(KeyCode::Numpad9),
        "KeyA" => Some(KeyCode::KeyA),
        "KeyB" => Some(KeyCode::KeyB),
        "KeyC" => Some(KeyCode::KeyC),
        "KeyD" => Some(KeyCode::KeyD),
        "KeyE" => Some(KeyCode::KeyE),
        "KeyF" => Some(KeyCode::KeyF),
        "KeyG" => Some(KeyCode::KeyG),
        "KeyH" => Some(KeyCode::KeyH),
        "KeyI" => Some(KeyCode::KeyI),
        "KeyJ" => Some(KeyCode::KeyJ),
        "KeyK" => Some(KeyCode::KeyK),
        "KeyL" => Some(KeyCode::KeyL),
        "KeyM" => Some(KeyCode::KeyM),
        "KeyN" => Some(KeyCode::KeyN),
        "KeyO" => Some(KeyCode::KeyO),
        "KeyP" => Some(KeyCode::KeyP),
        "KeyQ" => Some(KeyCode::KeyQ),
        "KeyR" => Some(KeyCode::KeyR),
        "KeyS" => Some(KeyCode::KeyS),
        "KeyT" => Some(KeyCode::KeyT),
        "KeyU" => Some(KeyCode::KeyU),
        "KeyV" => Some(KeyCode::KeyV),
        "KeyW" => Some(KeyCode::KeyW),
        "KeyX" => Some(KeyCode::KeyX),
        "KeyY" => Some(KeyCode::KeyY),
        "KeyZ" => Some(KeyCode::KeyZ),
        _ => None,
    }
}

#[derive(Debug, Default, bevy::prelude::Resource)]
struct LeafwingInputGeneration(u64);

/// Leafwing action state and default map for the local production client player.
#[derive(Bundle, Debug)]
pub struct LocalPlayerClientInputBundle {
    action_state: ActionState<LeafwingPlayerAction>,
    input_map: InputMap<LeafwingPlayerAction>,
    surface_state: ActionState<LeafwingSurfaceAction>,
    surface_map: InputMap<LeafwingSurfaceAction>,
}

impl Default for LocalPlayerClientInputBundle {
    fn default() -> Self {
        Self {
            action_state: ActionState::default(),
            input_map: default_leafwing_input_map(),
            surface_state: ActionState::default(),
            surface_map: InputMap::default(),
        }
    }
}

impl LocalPlayerClientInputBundle {
    /// Builds the local player adapter from lock-compiled Leafwing maps.
    #[must_use]
    pub fn from_compiled(maps: &CompiledClientInputMaps) -> Self {
        Self {
            action_state: ActionState::default(),
            input_map: maps.gameplay.clone(),
            surface_state: ActionState::default(),
            surface_map: maps.surface.clone(),
        }
    }
}

/// Static client adapter from Leafwing 0.21 state to Lattice action frames.
#[derive(Clone, Copy, Debug, Default)]
pub struct LeafwingInputAdapterPlugin;

impl Plugin for LeafwingInputAdapterPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(InputManagerPlugin::<LeafwingPlayerAction>::default())
            .add_plugins(InputManagerPlugin::<LeafwingSurfaceAction>::default())
            .init_resource::<ActionFrameInbox>()
            .init_resource::<SurfaceActionFrame>()
            .init_resource::<GameplaySuppressed>()
            .init_resource::<LeafwingInputGeneration>()
            .add_systems(
                bevy::prelude::PreUpdate,
                sample_leafwing_state.after(InputManagerSystem::Update),
            );
    }
}

#[allow(clippy::too_many_lines)] // One Leafwing sample publishes both gameplay and surface frames.
fn sample_leafwing_state(
    action_states: Query<'_, '_, &ActionState<LeafwingPlayerAction>, With<LocalPlayerInput>>,
    surface_states: Query<'_, '_, &ActionState<LeafwingSurfaceAction>, With<LocalPlayerInput>>,
    mut inbox: ResMut<'_, ActionFrameInbox>,
    mut surface: ResMut<'_, SurfaceActionFrame>,
    mut generation: ResMut<'_, LeafwingInputGeneration>,
    suppressed: Option<Res<'_, GameplaySuppressed>>,
) {
    generation.0 = generation.0.saturating_add(1);
    let gameplay_suppressed = suppressed.is_some_and(|flag| flag.0);
    let Ok(action_state) = action_states.single() else {
        inbox.replace_live_with_neutral(generation.0);
        surface.clear();
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
        (LeafwingPlayerAction::PickBlock, PlayerActionV1::PickBlock),
        (LeafwingPlayerAction::Sprint, PlayerActionV1::Sprint),
        (LeafwingPlayerAction::Sneak, PlayerActionV1::Sneak),
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
    if gameplay_suppressed {
        held = PlayerActionButtonsV1::empty();
        started = PlayerActionButtonsV1::empty();
        if action_state.pressed(&LeafwingPlayerAction::Pause) {
            held.insert(PlayerActionV1::Pause);
        }
        if action_state.just_pressed(&LeafwingPlayerAction::Pause) {
            started.insert(PlayerActionV1::Pause);
        }
    }

    inbox.publish_live_with_look_rate(
        PlayerActionFrameV1 {
            generation: generation.0,
            movement: if gameplay_suppressed {
                ActionAxis2V1::default()
            } else {
                ActionAxis2V1::finite_or_zero(movement.x, movement.y)
            },
            look_radians: if gameplay_suppressed {
                ActionAxis2V1::default()
            } else {
                ActionAxis2V1::finite_or_zero(look_delta.x, look_delta.y)
            },
            held,
            started,
            placement_content: None,
            client_observation: None,
        },
        if gameplay_suppressed {
            ActionAxis2V1::default()
        } else {
            look_rate_radians_per_second
        },
    );

    let mut started_surface = BTreeSet::new();
    let mut held_surface = BTreeSet::new();
    if let Ok(surface_state) = surface_states.single() {
        for runtime in [
            LeafwingSurfaceAction::NavUp,
            LeafwingSurfaceAction::NavDown,
            LeafwingSurfaceAction::NavLeft,
            LeafwingSurfaceAction::NavRight,
            LeafwingSurfaceAction::NavNext,
            LeafwingSurfaceAction::NavPrevious,
            LeafwingSurfaceAction::Activate,
            LeafwingSurfaceAction::Back,
            LeafwingSurfaceAction::Pause,
            LeafwingSurfaceAction::ToggleInventory,
            LeafwingSurfaceAction::ToggleWorkbench,
            LeafwingSurfaceAction::HotbarSlot1,
            LeafwingSurfaceAction::HotbarSlot2,
            LeafwingSurfaceAction::HotbarSlot3,
            LeafwingSurfaceAction::HotbarSlot4,
            LeafwingSurfaceAction::HotbarSlot5,
            LeafwingSurfaceAction::HotbarSlot6,
            LeafwingSurfaceAction::HotbarSlot7,
            LeafwingSurfaceAction::HotbarSlot8,
            LeafwingSurfaceAction::HotbarSlot9,
            LeafwingSurfaceAction::HotbarNext,
            LeafwingSurfaceAction::HotbarPrevious,
        ] {
            let action = runtime.to_surface();
            if surface_state.pressed(&runtime) {
                held_surface.insert(action);
            }
            if surface_state.just_pressed(&runtime) {
                started_surface.insert(action);
            }
        }
    }
    *surface = SurfaceActionFrame {
        started: started_surface,
        held: held_surface,
    };
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
            .init_resource::<SurfaceActionFrame>()
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

    #[test]
    fn compiled_catalog_maps_wasd_and_hotbar() {
        let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../packages/latticeaxiom/input/data/action-catalog-v1.json");
        let bytes = std::fs::read(path).expect("shipped catalog exists");
        let catalog = latticeaxiom_input::ActionCatalogDocumentV1::from_bytes(&bytes)
            .expect("shipped catalog parses");
        let provider = latticeaxiom_input::InputActionsProviderV1::new(
            catalog.owner_package.clone(),
            catalog.capability.clone(),
            catalog,
        )
        .expect("provider identity matches");
        let compiled = latticeaxiom_input::compile_input_catalog(
            [provider],
            &latticeaxiom_input::BindingProfileV1::empty(),
        )
        .expect("catalog compiles");
        let maps = leafwing_maps_from_catalog(&compiled);
        assert!(
            maps.gameplay()
                .get_dual_axislike(&LeafwingPlayerAction::Move)
                .is_some()
        );
        assert!(
            maps.gameplay()
                .get_buttonlike(&LeafwingPlayerAction::Sprint)
                .is_some()
        );
        assert!(
            maps.gameplay()
                .get_buttonlike(&LeafwingPlayerAction::Sneak)
                .is_some()
        );
        assert!(
            maps.surface()
                .get_buttonlike(&LeafwingSurfaceAction::ToggleInventory)
                .is_some()
        );
        assert!(
            maps.surface()
                .get_buttonlike(&LeafwingSurfaceAction::HotbarSlot1)
                .is_some()
        );
        assert!(
            maps.surface()
                .get_buttonlike(&LeafwingSurfaceAction::HotbarNext)
                .is_some()
        );
        assert!(
            maps.surface()
                .get_buttonlike(&LeafwingSurfaceAction::HotbarPrevious)
                .is_some()
        );
    }
}
