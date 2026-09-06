//! Headless semantic contracts for the package-driven client start surface.
//!
//! The crate owns presentation-neutral state, commands, layout evidence, and
//! accessibility semantics. It does not create a Bevy application. A client
//! adapter must render the same tree inside the one current `DefaultPlugins`
//! application. Continue/Play enter the Loading route in this process;
//! [`LaunchHandoff`] remains for a future settings-restart interface and is
//! not the ordinary play path.

mod capability;
mod library;
mod loading;
mod package;
mod semantic;
mod session;
mod settings;
mod shell;
mod typed;
mod world;

pub use capability::*;
pub use library::*;
pub use loading::*;
pub use package::*;
pub use semantic::*;
pub use session::*;
pub use settings::*;
pub use shell::*;
pub use typed::*;
pub use world::*;
