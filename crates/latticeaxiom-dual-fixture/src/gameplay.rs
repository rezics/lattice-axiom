//! Gameplay-shaped ABI-POD rows and the single-source business kernel.

use latticeaxiom_sdk::{
    CommandSink, FixedTick, Read, RowEntity, Write, component, registration_ir, system,
};
use serde::{Deserialize, Serialize};

/// Authoritative fixed-point agent state written by the gameplay system.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[component(
    id = "example:component/agent-state",
    schema = "example:schema/agent-state@1",
    mode = "generated-shared-schema"
)]
pub struct AgentState {
    /// Fixed-point world X coordinate.
    pub x_millimeters: i64,
    /// Fixed-point world Y height.
    pub y_millimeters: i64,
    /// Fixed-point world Z coordinate; conventional forward is negative Z.
    pub z_millimeters: i64,
    /// Bounded work stamina available to the agent.
    pub stamina: u32,
    /// Fixed ticks remaining before another break command may be emitted.
    pub break_cooldown: u32,
}

/// Read-only movement and work intent for one fixed tick.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[component(
    id = "example:component/motion-intent",
    schema = "example:schema/motion-intent@1",
    mode = "generated-shared-schema"
)]
pub struct MotionIntent {
    /// Signed X velocity in millimeters per second.
    pub velocity_x: i32,
    /// Signed Y velocity in millimeters per second.
    pub velocity_y: i32,
    /// Signed Z velocity in millimeters per second.
    pub velocity_z: i32,
    /// Work units requested against the current mining target.
    pub effort: u32,
}

/// Read-only voxel target consumed by the fixed gameplay system.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[component(
    id = "example:component/mining-target",
    schema = "example:schema/mining-target@1",
    mode = "generated-shared-schema"
)]
pub struct MiningTarget {
    /// Target voxel X coordinate.
    pub x: i32,
    /// Target voxel Y height.
    pub y: i32,
    /// Target voxel Z coordinate.
    pub z: i32,
    /// Required effort; zero means that there is no active target.
    pub hardness: u32,
}

/// One canonical authoritative command emitted by the row kernel.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CanonicalCommand {
    /// Authoritative fixed tick.
    pub tick: u64,
    /// Versioned semantic stage.
    pub stage: String,
    /// Stable system ID.
    pub system: String,
    /// Stable row identity used to canonicalize independently of batch splits.
    pub row_entity: u64,
    /// Callback-local canonical sequence.
    pub sequence: u64,
    /// Target voxel X coordinate.
    pub x: i32,
    /// Target voxel Y height.
    pub y: i32,
    /// Target voxel Z coordinate.
    pub z: i32,
}

/// Minimal command emission surface consumed by the single-source row kernel.
pub trait CommandEmitter {
    /// Stages one canonical command for later host validation.
    fn emit(&mut self, command: CanonicalCommand);
}

/// Deterministic in-memory command staging used by both generated adapters.
#[derive(Debug, Default)]
pub struct VecCommandSink {
    commands: Vec<CanonicalCommand>,
}

impl VecCommandSink {
    /// Borrows staged commands in emission order.
    #[must_use]
    pub fn commands(&self) -> &[CanonicalCommand] {
        &self.commands
    }

    /// Consumes the sink and returns staged commands.
    #[must_use]
    pub fn into_commands(self) -> Vec<CanonicalCommand> {
        self.commands
    }
}

impl CommandEmitter for VecCommandSink {
    fn emit(&mut self, command: CanonicalCommand) {
        self.commands.push(command);
    }
}

#[derive(Debug, Default)]
struct DiscardCommandSink;

impl CommandEmitter for DiscardCommandSink {
    fn emit(&mut self, _command: CanonicalCommand) {}
}

#[allow(dead_code)]
#[system(
    id = "example:system/integrate-and-mine",
    callback = "example:callback/integrate-and-mine@1",
    stage = "latticeaxiom:system-stage/gameplay/fixed@1",
    policy = "dual"
)]
fn integrate_and_mine(
    mut state: Write<'_, AgentState>,
    intent: Read<'_, MotionIntent>,
    target: Read<'_, MiningTarget>,
    entity: RowEntity,
    tick: FixedTick,
    _commands: CommandSink<'_>,
) {
    gameplay_business_row(
        &mut state,
        &intent,
        &target,
        entity.0.index(),
        tick,
        &mut DiscardCommandSink,
    );
}

pub(crate) fn gameplay_business_row(
    state: &mut AgentState,
    intent: &MotionIntent,
    target: &MiningTarget,
    row_entity: u64,
    tick: FixedTick,
    commands: &mut impl CommandEmitter,
) {
    state.x_millimeters = integrate_axis(state.x_millimeters, intent.velocity_x, tick.delta_nanos);
    state.y_millimeters = integrate_axis(state.y_millimeters, intent.velocity_y, tick.delta_nanos);
    state.z_millimeters = integrate_axis(state.z_millimeters, intent.velocity_z, tick.delta_nanos);
    state.stamina = state
        .stamina
        .saturating_sub(intent.effort.min(state.stamina));
    state.break_cooldown = state.break_cooldown.saturating_sub(1);
    if target.hardness != 0 && intent.effort >= target.hardness && state.break_cooldown == 0 {
        commands.emit(CanonicalCommand {
            tick: tick.tick,
            stage: "latticeaxiom:system-stage/gameplay/fixed@1".to_owned(),
            system: "example:system/integrate-and-mine".to_owned(),
            row_entity,
            sequence: row_entity.saturating_add(1),
            x: target.x,
            y: target.y,
            z: target.z,
        });
        state.break_cooldown = 3;
    }
}

fn integrate_axis(position: i64, velocity: i32, delta_nanos: u64) -> i64 {
    const NANOS_PER_SECOND: i128 = 1_000_000_000;
    let delta = i128::from(velocity).saturating_mul(i128::from(delta_nanos)) / NANOS_PER_SECOND;
    let next = i128::from(position).saturating_add(delta);
    match i64::try_from(next) {
        Ok(value) => value,
        Err(_) if next.is_negative() => i64::MIN,
        Err(_) => i64::MAX,
    }
}

pub(crate) fn registration_ir()
-> Result<latticeaxiom_sdk::RegistrationIr, latticeaxiom_sdk::RegistrationIrError> {
    registration_ir! {
        package = "@example/dual-gameplay",
        version = "0.1.0",
        components = [AgentState, MotionIntent, MiningTarget],
        systems = [integrate_and_mine],
    }
}

const _: () = {
    assert!(core::mem::size_of::<AgentState>() == 32);
    assert!(core::mem::size_of::<MotionIntent>() == 16);
    assert!(core::mem::size_of::<MiningTarget>() == 16);
    assert!(
        core::mem::size_of::<AgentState>()
            + core::mem::size_of::<MotionIntent>()
            + core::mem::size_of::<MiningTarget>()
            == 64
    );
};
