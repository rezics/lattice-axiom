//! Single-session local playable slice.
//!
//! This module is deliberately a development fixture: it uses trusted built-in
//! Terrenia identifiers and non-durable in-memory authority so the existing
//! player, physics, edit, and presentation seams can be exercised end to end.
//! It is not a replacement for the frozen package lock or production world
//! writer. The default engine binary boots the production host instead; run
//! `latticeaxiom-playable-fixture` to launch this slice.

mod authority;
mod bootstrap;
mod hud;
mod input;
mod pause;
mod scene;

pub use bootstrap::{PlayableClientError, run_playable_client};
