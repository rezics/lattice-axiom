//! Pure-data host policies for runtime settings and observability.
//!
//! This crate validates and operates on the declaration DTOs owned by
//! `latticeaxiom-compose`. It deliberately contains no Bevy facade, UI,
//! renderer, task runtime, persistence backend, or callback execution.

mod observability;
mod settings;

pub use latticeaxiom_compose::{
    CostClass, DebugVisualizerSpec, DiagnosticMetricSpec, DisclosureLevel, InfoItemSpec,
    MetricAggregation, ObservabilityCatalog, RuntimeApplyImpact, SettingAuthority,
    SettingPredicate, SettingScope, SettingSensitivity, SettingSpec, SettingsCatalog, UpdatePolicy,
    ValueType, VisualizerBudget,
};
pub use observability::*;
pub use settings::*;
