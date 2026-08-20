//! Bevy host scaffolding for receipt-verified Lattice Axiom images.
//!
//! Composition and package resolution happen before this crate is entered.
//! The prepared boundary verifies a complete compiled registration and binds
//! its callback receipt to the selected runtime image before an
//! [`EngineInstance`] is constructed. The lock format still does not freeze the
//! registration semantic or callback-map hashes; callers must therefore retain
//! and transport the complete compiled output alongside the exact lock and
//! runtime image. `RuntimeImage` still carries callback keys rather than
//! signature or loaded-code attestations, so actual adapter installation remains
//! a later fail-closed boundary. Each instance owns one Bevy [`bevy::app::App`].

mod instance;
#[cfg(feature = "client")]
mod playable;
mod prepared;
#[cfg(feature = "client")]
mod presentation_fixture;

pub use instance::{
    EngineInstance, EngineInstanceError, EngineProfile, FixedTickCount, MAX_TICKS_PER_ADVANCE,
};
#[cfg(feature = "client")]
pub use playable::{PlayableClientError, run_playable_client};
pub use prepared::{
    CallbackContext, CatalogKind, PreparationError, StructurallyValidatedComposeImages,
};
#[cfg(feature = "client")]
pub use presentation_fixture::run_temporary_client_presentation_fixture;
