//! Headless semantic contracts for the package-driven client start surface.
//!
//! The crate owns presentation-neutral state, commands, layout evidence, and
//! accessibility semantics. It does not create a Bevy application. A client
//! adapter must render the same tree inside the one current `DefaultPlugins`
//! application and use [`LaunchHandoff`] to enter a world in a replacement
//! process.

mod capability;
mod loading;
mod package;
mod semantic;
mod session;
mod settings;
mod shell;
mod world;

pub use capability::*;
pub use loading::*;
pub use package::*;
pub use semantic::*;
pub use session::*;
pub use settings::*;
pub use shell::*;
pub use world::*;
