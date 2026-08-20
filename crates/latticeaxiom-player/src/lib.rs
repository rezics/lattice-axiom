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
pub use edit::{
    AuthoritativeBlockEditRequestV1, BlockEditActionV1, BlockEditAuthority,
    BlockEditAuthorityResource, BlockEditIntentV1, BlockEditReceiptV1, BlockEditRejectV1,
    BlockEditSuccessV1, BlockFaceV1, ClientTargetObservationV1, MAX_BLOCK_EDIT_REACH_M,
    PlayerEditLimiter, SUCCESSFUL_EDIT_COOLDOWN_TICKS, SuccessfulEditCooldownV1, TargetEyePoseV1,
};
pub use inspect::{
    AuthoritativeTargetInspectRequestV1, HeadlessTargetInspectV1, TargetInspectReceiptV1,
    TargetInspectRejectV1, chunk_line, occupancy_line,
};
#[cfg(feature = "client-input")]
pub use leafwing_adapter::{
    LeafwingInputAdapterPlugin, LeafwingPlayerAction, LocalPlayerClientInputBundle,
    default_leafwing_input_map,
};
pub use movement::{
    CurrentPlayerActionFrame, D2Player, D2PlayerBundle, DetachedSpectator, LocalPlayerInput,
    PlayerControllerState, PlayerMovementProfileError, PlayerMovementProfileV1, PlayerViewV1,
};
pub use plugin::{PlayerFixedTick, PlayerPlugin, PlayerSystemSet};
