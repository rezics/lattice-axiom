//! Package-composable information, metrics, inspection, and visualizers.

use std::collections::{BTreeMap, BTreeSet};

use latticeaxiom_core::{PackageName, SchemaId, StableId};
use serde::{Deserialize, Serialize};

/// Information disclosure level.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum DisclosureLevel {
    /// Player-facing essential information.
    Basic,
    /// Expanded contextual information.
    Detail,
    /// Author-facing technical information.
    Technical,
}

/// Update policy for an information source.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(
    deny_unknown_fields,
    rename_all = "kebab-case",
    tag = "mode",
    content = "interval-ms"
)]
pub enum UpdatePolicy {
    /// Recompute only after a source change event.
    OnChange,
    /// Recompute when the inspected target changes.
    TargetChange,
    /// Recompute at a bounded interval.
    FixedInterval(u32),
    /// Recompute only after an explicit request.
    Manual,
}

/// Estimated source cost used by subscription planning.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum CostClass {
    /// Negligible shared counter or cached value.
    Trivial,
    /// Bounded local query.
    Moderate,
    /// Expensive scan, I/O, or remote request.
    Expensive,
}

/// Structured information item registration.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct InfoItemSpec {
    /// Stable item ID.
    pub id: StableId,
    /// Package owning the item.
    pub declared_by: PackageName,
    /// Localization key.
    pub label_key: String,
    /// Optional physical unit.
    pub unit: Option<String>,
    /// Typed sample schema.
    pub value_schema: SchemaId,
    /// Information disclosure level.
    pub disclosure: DisclosureLevel,
    /// Source update policy.
    pub update: UpdatePolicy,
    /// Source cost estimate.
    pub cost: CostClass,
    /// Stable callback key, never a function pointer.
    pub callback: StableId,
}

/// Metric aggregation exposed by the workbench.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum MetricAggregation {
    /// Most recent sample.
    Latest,
    /// Arithmetic mean.
    Mean,
    /// Median sample.
    P50,
    /// Ninety-fifth percentile sample.
    P95,
    /// Maximum sample.
    Max,
    /// Per-second rate.
    Rate,
}

/// Structured diagnostic metric registration.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DiagnosticMetricSpec {
    /// Stable metric ID.
    pub id: StableId,
    /// Package owning the metric.
    pub declared_by: PackageName,
    /// Physical or logical unit.
    pub unit: String,
    /// Supported aggregation.
    pub aggregation: MetricAggregation,
    /// Minimum sampling interval.
    pub sampling_interval_ms: u32,
    /// Maximum retained samples.
    pub history_limit: u32,
    /// Source cost estimate.
    pub cost: CostClass,
}

/// Target kinds understood by the inspect surface.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum InspectTargetKind {
    /// Block or fluid voxel.
    Voxel,
    /// Entity target.
    Entity,
    /// Inventory stack.
    Item,
}

/// Target-inspection fragment provider registration.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct InspectFragmentProviderSpec {
    /// Stable provider ID.
    pub id: StableId,
    /// Package owning the provider.
    pub declared_by: PackageName,
    /// Target kinds supported by the provider.
    pub target_kinds: BTreeSet<InspectTargetKind>,
    /// Fragment keys for which this provider is the primary owner.
    pub primary_keys: BTreeSet<String>,
    /// Explicit extension slots used by additive fragments.
    pub extension_slots: BTreeSet<String>,
    /// Providers that must run before this provider.
    pub after: BTreeSet<StableId>,
    /// Stable batched callback key.
    pub callback: StableId,
}

/// Depth behavior for a world-space debug visualizer.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum VisualizerDepthMode {
    /// Primitives are occluded normally.
    DepthTested,
    /// Primitives remain visible through geometry.
    XRay,
    /// User may switch between the two modes.
    Configurable,
}

/// Hard budgets for one debug visualizer subscription.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct VisualizerBudget {
    /// Maximum horizontal radius in chunks or meters, defined by the provider.
    pub radius: u32,
    /// Maximum emitted primitives per sample.
    pub primitives: u32,
    /// Maximum upload bytes per sample.
    pub upload_bytes: u64,
    /// Maximum retained samples.
    pub history: u32,
}

/// World-space debug visualizer registration.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DebugVisualizerSpec {
    /// Stable visualizer ID.
    pub id: StableId,
    /// Package owning the visualizer.
    pub declared_by: PackageName,
    /// Stable typed data-source ID.
    pub data_source: StableId,
    /// Human-readable legend keys.
    pub legend: BTreeSet<String>,
    /// Depth behavior.
    pub depth_mode: VisualizerDepthMode,
    /// Whether primitives support selection.
    pub pick_support: bool,
    /// Hard subscription budgets.
    pub budget: VisualizerBudget,
}

/// Deterministic observability catalog.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ObservabilityCatalog {
    /// Information items keyed by stable ID.
    pub info_items: BTreeMap<StableId, InfoItemSpec>,
    /// Metrics keyed by stable ID.
    pub metrics: BTreeMap<StableId, DiagnosticMetricSpec>,
    /// Inspect providers keyed by stable ID.
    pub inspect: BTreeMap<StableId, InspectFragmentProviderSpec>,
    /// Visualizers keyed by stable ID.
    pub visualizers: BTreeMap<StableId, DebugVisualizerSpec>,
}
