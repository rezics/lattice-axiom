//! Typed, deterministic SDK registration contracts.
//!
//! This crate is the public authoring surface for code-bound package rows. Its
//! proc macros emit a sealed intermediate representation; build tooling then
//! turns that IR into canonical manifest, provenance, callback-map, static
//! adapter, and portable batch-shim plans. It does not discover linked crates,
//! keep a runtime global registry, load native libraries, or mirror Bevy's ECS.
//!
//! A dual-realization row kernel uses only the finite SDK parameter vocabulary:
//!
//! ```
//! use latticeaxiom_sdk::{CommandSink, FixedTick, Read, Write, component, system};
//!
//! #[repr(C)]
//! #[component(
//!     id = "example:component/position",
//!     schema = "example:schema/position@1",
//!     mode = "generated-shared-schema"
//! )]
//! struct Position {
//!     x: i32,
//! }
//!
//! #[repr(C)]
//! #[component(
//!     id = "example:component/velocity",
//!     schema = "example:schema/velocity@1",
//!     mode = "generated-shared-schema"
//! )]
//! struct Velocity {
//!     x: i32,
//! }
//!
//! #[system(
//!     id = "example:system/integrate",
//!     callback = "example:callback/integrate@1",
//!     stage = "latticeaxiom:system-stage/gameplay/fixed@1",
//!     policy = "dual"
//! )]
//! fn integrate(
//!     _position: Write<'_, Position>,
//!     _velocity: Read<'_, Velocity>,
//!     _tick: FixedTick,
//!     _commands: CommandSink<'_>,
//! ) {
//! }
//! ```
//!
//! Unsupported dynamic parameters are rejected for explicit dual systems:
//!
//! ```compile_fail
//! use latticeaxiom_sdk::system;
//!
//! struct World;
//!
//! #[system(
//!     id = "example:system/exclusive",
//!     callback = "example:callback/exclusive@1",
//!     stage = "latticeaxiom:system-stage/gameplay/fixed@1",
//!     policy = "dual"
//! )]
//! fn exclusive(_world: World) {}
//! ```

#![forbid(unsafe_code)]

mod artifact;
mod component;
mod error;
mod ir;
mod system;

pub use artifact::{
    CallbackMapArtifact, CallbackMapEntry, DynamicBatchColumn, DynamicBatchShimPlan,
    GENERATED_ARTIFACT_SCHEMA_VERSION, GeneratedRegistrationArtifacts, ProducerInput,
    ProducerReceipt, ProvenanceArtifact, StaticAdapterPlan,
};
pub use component::{ComponentContract, ComponentMode};
pub use error::RegistrationIrError;
pub use ir::{ComponentIr, ProvenanceCatalog, RegistrationIr, SystemIr};
pub use latticeaxiom_sdk_macros::{component, registration_ir, system};
pub use system::{
    CommandSink, ComponentAccess, ComponentAccessKind, ComponentRequirement, FixedTick,
    NativeStaticOnlyReason, OptionalRead, OptionalWrite, QueryFilter, Read, RowEntity,
    RowEntityKey, SystemParameter, SystemPortability, SystemSignature, With, Without, Write,
};

/// Implementation details consumed only by SDK macro expansion.
///
/// This module is not a second authoring API. Its traits are sealed by the
/// generated implementations, and its seed values cannot become a validated
/// [`RegistrationIr`] without the crate-owned compiler.
#[doc(hidden)]
pub mod __private {
    pub use crate::component::{AbiPodField, ComponentSeed, SealedComponent};
    pub use crate::ir::build_registration_ir;
    pub use crate::system::{
        ParameterSeed, SystemPolicySeed, SystemSeed, command_sink_parameter, component_parameter,
        filter_parameter, fixed_tick_parameter, row_entity_parameter, static_only_parameter,
    };
}
