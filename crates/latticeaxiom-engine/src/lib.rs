//! Bevy host scaffolding for receipt-verified Lattice Axiom images.
//!
//! Ordinary launch reopens and fully verifies `latticeaxiom.lock` before this
//! crate constructs [`latticeaxiom_compose::RuntimeImage`] or a Bevy
//! [`bevy::app::App`]. Client [`bevy::prelude::DefaultPlugins`] and GPU-free
//! headless hosts share that reopened lock and must not re-resolve. Compiled
//! registration evidence is transported alongside the lock and rebound to it.
//! Native modules are never mapped on this path. `RuntimeImage` still carries
//! callback keys rather than signature or loaded-code attestations, so actual
//! adapter installation remains a later fail-closed boundary. Each instance
//! owns one Bevy [`bevy::app::App`].

mod instance;
#[cfg(feature = "client")]
mod playable;
mod prepared;
#[cfg(feature = "client")]
mod presentation_fixture;

pub use instance::{
    EngineInstance, EngineInstanceError, EngineProfile, FixedTickCount, MAX_TICKS_PER_ADVANCE,
    VerifiedProductLockHash,
};
#[cfg(feature = "client")]
pub use playable::{PlayableClientError, run_playable_client};
pub use prepared::{
    CallbackContext, CatalogKind, LockVerifiedComposeImages, PreparationError,
    StructurallyValidatedComposeImages,
};
#[cfg(feature = "client")]
pub use presentation_fixture::run_temporary_client_presentation_fixture;
