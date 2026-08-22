//! Pure-data host policies for runtime settings and observability.
//!
//! This crate validates and operates on the declaration DTOs owned by
//! `latticeaxiom-compose`. It deliberately contains no Bevy facade, UI,
//! renderer, task runtime, or callback execution. Local user/device values use
//! the canonical old-or-complete-new file protocol; world stores remain outside
//! this crate.

mod cave_hydrology_inspect;
mod observability;
mod settings;
mod worldgen_inspect;

pub use cave_hydrology_inspect::*;
pub use latticeaxiom_compose::{
    CostClass, DebugVisualizerSpec, DiagnosticMetricSpec, DisclosureLevel, InfoItemSpec,
    MetricAggregation, ObservabilityCatalog, RuntimeApplyImpact, SettingAuthority,
    SettingPredicate, SettingScope, SettingSensitivity, SettingSpec, SettingsCatalog, UpdatePolicy,
    ValueType, VisualizerBudget,
};
pub use observability::*;
pub use settings::*;
pub use worldgen_inspect::*;
