//! Fixed-tick player control and authoritative block-edit integration.
//!
//! This crate keeps the portable [`PlayerActionFrameV1`] and block-edit DTOs
//! independent from both Avian and Leafwing. Avian components remain runtime
//! implementation details, while the optional `client-input` feature adapts a
//! Leafwing `ActionState` into the same action frames used by headless tests.
//!
//! [`PlayerPlugin`] intentionally does not install Avian's [`avian3d::PhysicsPlugins`].
//! The Bevy host owns ecosystem plugin composition and must install
//! `PhysicsPlugins::default()`, whose physics schedule is `FixedPostUpdate`.

mod action;
mod clock;
mod edit;
mod inspect;
#[cfg(feature = "client-input")]
mod leafwing_adapter;
mod movement;
mod plugin;

pub use action::{
    ActionAxis2V1, ActionFrameInbox, ActionFrameInboxError, PlayerActionButtonsV1,
    PlayerActionFrameV1, PlayerActionV1,
};
pub use clock::{
    DEFAULT_SIMULATION_TICK_RATE_HZ, MAX_SIMULATION_TICK_RATE_HZ, MIN_SIMULATION_TICK_RATE_HZ,
    SimulationClock, SimulationTickRate, SimulationTickRateChanged, SimulationTickRateError,
    SimulationTickRateRequest,
};
pub use edit::{
    AuthoritativeBlockEditRequestV1, AuthoritativeMiningCancelRequestV1, BlockEditActionV1,
    BlockEditAuthority, BlockEditAuthorityResource, BlockEditInputStateV1, BlockEditIntentV1,
    BlockEditReceiptV1, BlockEditRejectV1, BlockEditSuccessV1, BlockFaceV1,
    CANONICAL_MINING_STEPS_PER_SECOND, ClientTargetObservationV1, MAX_BLOCK_EDIT_REACH_M,
    MiningCancelReceiptV1, MiningCancelSuccessV1, TargetEyePoseV1,
};
pub use inspect::{
    AuthoritativeTargetInspectRequestV1, HeadlessTargetInspectV1, TargetInspectReceiptV1,
    TargetInspectRejectV1, chunk_line, occupancy_line,
};
pub use latticeaxiom_gameplay::{InventoryInspectV1, MiningInspectV1, RecipeInspectV1};
pub use latticeaxiom_input::{
    AuthoritativePlayerActionV1, ClientSurfaceActionV1, CompiledInputCatalogV1,
    CompiledLeafwingMapV1,
};
#[cfg(feature = "client-input")]
pub use leafwing_adapter::{
    ClientInputOwnership, ClientInputSystemSet, CompiledClientInputMaps, GameplaySuppressed,
    LeafwingInputAdapterPlugin, LeafwingPlayerAction, LeafwingSurfaceAction,
    LocalPlayerClientInputBundle, SurfaceActionFrame, default_leafwing_input_map,
    leafwing_maps_from_catalog,
};
/// Leafwing action-state type used by the static client adapter.
#[cfg(feature = "client-input")]
pub use leafwing_input_manager::action_state::ActionState;
pub use movement::{
    CurrentPlayerActionFrame, D2Player, D2PlayerBundle, DetachedSpectator, LocalPlayerInput,
    PlayerControllerState, PlayerMovementProfileError, PlayerMovementProfileV1, PlayerViewV1,
};
pub use plugin::{PlayerFixedTick, PlayerPlugin, PlayerSystemSet};
