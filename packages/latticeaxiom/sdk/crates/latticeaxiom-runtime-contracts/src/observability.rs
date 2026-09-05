//! Bounded observability catalogs, subscriptions, samples, and reports.

use std::collections::{BTreeMap, BTreeSet};

use latticeaxiom_compose::{ObservabilityCatalog, UpdatePolicy};
use latticeaxiom_core::{
    CanonicalHash, CanonicalJsonError, SchemaId, StableId, canonical_json_bytes,
    canonical_json_hash,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;
use unicode_normalization::is_nfc;

/// D2 hard maximum source and visualizer update frequency.
pub const MAX_OBSERVABILITY_UPDATE_HZ: u32 = 10;

/// Minimum interval corresponding to [`MAX_OBSERVABILITY_UPDATE_HZ`].
pub const MIN_OBSERVABILITY_INTERVAL_MS: u32 = 100;

/// D2 hard primitive count for one visualizer update.
pub const MAX_VISUALIZER_PRIMITIVES: u32 = 8_192;

/// D2 hard verified upload bytes for one visualizer update.
pub const MAX_VISUALIZER_UPLOAD_BYTES: u64 = 512 * 1_024;

/// D2 hard chunk-radius limit for one visualizer.
pub const MAX_CHUNK_VISUALIZER_RADIUS: u32 = 8;

/// D2 hard meter-radius limit for one physics visualizer.
pub const MAX_PHYSICS_VISUALIZER_RADIUS_METERS: u32 = 32;

/// Default defensive cap on all observability registrations.
pub const DEFAULT_MAX_OBSERVABILITY_ROWS: usize = 8_192;

/// Default defensive cap on history samples for one metric or visualizer.
pub const DEFAULT_MAX_HISTORY_SAMPLES: u32 = 4_096;

/// Default defensive cap on history bytes for one metric or visualizer.
pub const DEFAULT_MAX_HISTORY_BYTES: u64 = 16 * 1_024 * 1_024;

/// Host policy required to fill fields absent from the current metric DTO.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct MetricHistoryPolicy {
    /// Maximum encoded bytes retained by the host for this metric.
    pub max_bytes: u64,
}

/// Unit controlling the accepted visualizer radius hard cap.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum VisualizerRadiusUnit {
    /// Radius is measured in chunks and capped at eight.
    Chunks,
    /// Radius is measured in meters and capped at thirty-two.
    Meters,
}

/// Host policy required to fill fields absent from the current visualizer DTO.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct VisualizerHostPolicy {
    /// Unit determining the applicable radius cap.
    pub radius_unit: VisualizerRadiusUnit,
    /// Requested source update interval.
    pub update_interval_ms: u32,
    /// Maximum encoded history bytes retained by the host.
    pub max_history_bytes: u64,
}

/// Per-registration policies that cannot yet be expressed by compose DTOs.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ObservabilityRuntimePolicies {
    /// Metric history byte caps keyed by metric ID.
    pub metrics: BTreeMap<StableId, MetricHistoryPolicy>,
    /// Visualizer unit, rate, and byte caps keyed by visualizer ID.
    pub visualizers: BTreeMap<StableId, VisualizerHostPolicy>,
}

/// Closure-wide defensive limits for observability catalog compilation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ObservabilityCatalogPolicy {
    row_limit: usize,
    max_history_samples: u32,
    max_history_bytes: u64,
}

impl ObservabilityCatalogPolicy {
    /// Creates explicit catalog limits.
    ///
    /// # Errors
    ///
    /// Returns [`ObservabilityPolicyError`] if any limit is zero.
    pub const fn new(
        row_limit: usize,
        max_history_samples: u32,
        max_history_bytes: u64,
    ) -> Result<Self, ObservabilityPolicyError> {
        if row_limit == 0 {
            Err(ObservabilityPolicyError::ZeroRows)
        } else if max_history_samples == 0 {
            Err(ObservabilityPolicyError::ZeroHistorySamples)
        } else if max_history_bytes == 0 {
            Err(ObservabilityPolicyError::ZeroHistoryBytes)
        } else {
            Ok(Self {
                row_limit,
                max_history_samples,
                max_history_bytes,
            })
        }
    }
}

impl Default for ObservabilityCatalogPolicy {
    fn default() -> Self {
        Self {
            row_limit: DEFAULT_MAX_OBSERVABILITY_ROWS,
            max_history_samples: DEFAULT_MAX_HISTORY_SAMPLES,
            max_history_bytes: DEFAULT_MAX_HISTORY_BYTES,
        }
    }
}

/// Invalid observability catalog limits.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum ObservabilityPolicyError {
    /// The total row cap was zero.
    #[error("observability row limit must be positive")]
    ZeroRows,
    /// The per-source history count cap was zero.
    #[error("observability history sample limit must be positive")]
    ZeroHistorySamples,
    /// The per-source history byte cap was zero.
    #[error("observability history byte limit must be positive")]
    ZeroHistoryBytes,
}

/// An observability catalog checked against host hard policies expressible now.
///
/// Missing compose fields such as metric callbacks, privacy, visualizer
/// primitives, permissions, headless fallback, and coordinate space remain a
/// registration-compiler responsibility. This wrapper intentionally does not
/// claim complete ADR 0025 schema conformance.
#[derive(Clone, Debug, Serialize)]
#[serde(transparent)]
pub struct ValidatedObservabilityCatalog {
    catalog: ObservabilityCatalog,
}

impl ValidatedObservabilityCatalog {
    /// Validates deterministic map keys, rates, histories, and visualizer caps.
    ///
    /// # Errors
    ///
    /// Returns [`ObservabilityCatalogError`] for a count, key, rate, history,
    /// missing/extra host policy, radius, primitive, or upload violation.
    #[allow(
        clippy::too_many_lines,
        reason = "linear fail-closed catalog validation keeps the policy order auditable"
    )]
    pub fn compile(
        catalog: ObservabilityCatalog,
        runtime: &ObservabilityRuntimePolicies,
        policy: ObservabilityCatalogPolicy,
    ) -> Result<Self, ObservabilityCatalogError> {
        let observed = catalog
            .info_items
            .len()
            .checked_add(catalog.metrics.len())
            .and_then(|count| count.checked_add(catalog.inspect.len()))
            .and_then(|count| count.checked_add(catalog.visualizers.len()))
            .ok_or(ObservabilityCatalogError::RowCountOverflow)?;
        if observed > policy.row_limit {
            return Err(ObservabilityCatalogError::CatalogLimitExceeded {
                observed,
                maximum: policy.row_limit,
            });
        }

        for (key, item) in &catalog.info_items {
            if key != &item.id {
                return Err(ObservabilityCatalogError::CatalogKeyMismatch {
                    key: key.clone(),
                    declared: Box::new(item.id.clone()),
                });
            }
            if let UpdatePolicy::FixedInterval(interval) = item.update
                && interval < MIN_OBSERVABILITY_INTERVAL_MS
            {
                return Err(ObservabilityCatalogError::UpdateRateExceeded {
                    source_id: item.id.clone(),
                    interval_ms: interval,
                    minimum_ms: MIN_OBSERVABILITY_INTERVAL_MS,
                });
            }
        }
        for (key, metric) in &catalog.metrics {
            if key != &metric.id {
                return Err(ObservabilityCatalogError::CatalogKeyMismatch {
                    key: key.clone(),
                    declared: Box::new(metric.id.clone()),
                });
            }
            if metric.sampling_interval_ms < MIN_OBSERVABILITY_INTERVAL_MS {
                return Err(ObservabilityCatalogError::UpdateRateExceeded {
                    source_id: metric.id.clone(),
                    interval_ms: metric.sampling_interval_ms,
                    minimum_ms: MIN_OBSERVABILITY_INTERVAL_MS,
                });
            }
            validate_history(
                &metric.id,
                metric.history_limit,
                runtime.metrics.get(&metric.id).map(|entry| entry.max_bytes),
                policy,
            )?;
        }
        for (key, provider) in &catalog.inspect {
            if key != &provider.id {
                return Err(ObservabilityCatalogError::CatalogKeyMismatch {
                    key: key.clone(),
                    declared: Box::new(provider.id.clone()),
                });
            }
        }
        for (key, visualizer) in &catalog.visualizers {
            if key != &visualizer.id {
                return Err(ObservabilityCatalogError::CatalogKeyMismatch {
                    key: key.clone(),
                    declared: Box::new(visualizer.id.clone()),
                });
            }
            let host = runtime.visualizers.get(&visualizer.id).ok_or_else(|| {
                ObservabilityCatalogError::MissingVisualizerPolicy {
                    visualizer: visualizer.id.clone(),
                }
            })?;
            if host.update_interval_ms < MIN_OBSERVABILITY_INTERVAL_MS {
                return Err(ObservabilityCatalogError::UpdateRateExceeded {
                    source_id: visualizer.id.clone(),
                    interval_ms: host.update_interval_ms,
                    minimum_ms: MIN_OBSERVABILITY_INTERVAL_MS,
                });
            }
            let radius_max = match host.radius_unit {
                VisualizerRadiusUnit::Chunks => MAX_CHUNK_VISUALIZER_RADIUS,
                VisualizerRadiusUnit::Meters => MAX_PHYSICS_VISUALIZER_RADIUS_METERS,
            };
            if visualizer.budget.radius > radius_max {
                return Err(ObservabilityCatalogError::VisualizerRadiusExceeded {
                    visualizer: visualizer.id.clone(),
                    observed: visualizer.budget.radius,
                    maximum: radius_max,
                });
            }
            if visualizer.budget.primitives > MAX_VISUALIZER_PRIMITIVES {
                return Err(
                    ObservabilityCatalogError::VisualizerPrimitiveLimitExceeded {
                        visualizer: visualizer.id.clone(),
                        observed: visualizer.budget.primitives,
                        maximum: MAX_VISUALIZER_PRIMITIVES,
                    },
                );
            }
            if visualizer.budget.upload_bytes > MAX_VISUALIZER_UPLOAD_BYTES {
                return Err(ObservabilityCatalogError::VisualizerUploadLimitExceeded {
                    visualizer: visualizer.id.clone(),
                    observed: visualizer.budget.upload_bytes,
                    maximum: MAX_VISUALIZER_UPLOAD_BYTES,
                });
            }
            validate_history(
                &visualizer.id,
                visualizer.budget.history,
                Some(host.max_history_bytes),
                policy,
            )?;
        }

        if let Some(extra) = runtime
            .metrics
            .keys()
            .find(|id| !catalog.metrics.contains_key(*id))
        {
            return Err(ObservabilityCatalogError::UnknownMetricPolicy {
                metric: extra.clone(),
            });
        }
        if let Some(extra) = runtime
            .visualizers
            .keys()
            .find(|id| !catalog.visualizers.contains_key(*id))
        {
            return Err(ObservabilityCatalogError::UnknownVisualizerPolicy {
                visualizer: extra.clone(),
            });
        }
        Ok(Self { catalog })
    }

    /// Returns the compose-owned observability DTO.
    #[must_use]
    pub const fn as_catalog(&self) -> &ObservabilityCatalog {
        &self.catalog
    }

    /// Encodes the catalog as recursively key-sorted compact JSON.
    ///
    /// # Errors
    ///
    /// Returns [`CanonicalJsonError`] if serialization fails.
    pub fn canonical_bytes(&self) -> Result<Vec<u8>, CanonicalJsonError> {
        canonical_json_bytes(&self.catalog)
    }
}

fn validate_history(
    source: &StableId,
    samples: u32,
    bytes: Option<u64>,
    policy: ObservabilityCatalogPolicy,
) -> Result<(), ObservabilityCatalogError> {
    if samples == 0 || samples > policy.max_history_samples {
        return Err(ObservabilityCatalogError::HistorySampleLimitInvalid {
            source_id: source.clone(),
            observed: samples,
            maximum: policy.max_history_samples,
        });
    }
    let bytes = bytes.ok_or_else(|| ObservabilityCatalogError::MissingHistoryBytePolicy {
        source_id: source.clone(),
    })?;
    if bytes == 0 || bytes > policy.max_history_bytes {
        return Err(ObservabilityCatalogError::HistoryByteLimitInvalid {
            source_id: source.clone(),
            observed: bytes,
            maximum: policy.max_history_bytes,
        });
    }
    Ok(())
}

/// An observability catalog violated a deterministic host policy.
#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum ObservabilityCatalogError {
    /// Total count arithmetic overflowed.
    #[error("observability row count overflowed")]
    RowCountOverflow,
    /// The catalog exceeded the configured total row limit.
    #[error("observability catalog has {observed} rows; maximum is {maximum}")]
    CatalogLimitExceeded {
        /// Observed total.
        observed: usize,
        /// Configured maximum.
        maximum: usize,
    },
    /// A B-tree key did not equal its declaration ID.
    #[error("observability catalog key `{key}` does not match declared ID `{declared}`")]
    CatalogKeyMismatch {
        /// Map key.
        key: StableId,
        /// Embedded declaration ID.
        declared: Box<StableId>,
    },
    /// A source requested more than ten updates per second.
    #[error(
        "observability source `{source_id}` interval {interval_ms} ms is below {minimum_ms} ms"
    )]
    UpdateRateExceeded {
        /// Source ID.
        source_id: StableId,
        /// Requested interval.
        interval_ms: u32,
        /// Accepted minimum.
        minimum_ms: u32,
    },
    /// A metric had no byte cap in the supplemental policy.
    #[error("observability source `{source_id}` has no history byte policy")]
    MissingHistoryBytePolicy {
        /// Source ID.
        source_id: StableId,
    },
    /// History sample count was zero or above the host maximum.
    #[error("source `{source_id}` history count {observed} is outside 1..={maximum}")]
    HistorySampleLimitInvalid {
        /// Source ID.
        source_id: StableId,
        /// Declared count.
        observed: u32,
        /// Host maximum.
        maximum: u32,
    },
    /// History bytes were zero or above the host maximum.
    #[error("source `{source_id}` history bytes {observed} is outside 1..={maximum}")]
    HistoryByteLimitInvalid {
        /// Source ID.
        source_id: StableId,
        /// Declared bytes.
        observed: u64,
        /// Host maximum.
        maximum: u64,
    },
    /// A visualizer did not receive required supplemental host policy.
    #[error("visualizer `{visualizer}` has no unit/rate/history host policy")]
    MissingVisualizerPolicy {
        /// Visualizer ID.
        visualizer: StableId,
    },
    /// A supplemental metric policy did not match a metric.
    #[error("history policy refers to unknown metric `{metric}`")]
    UnknownMetricPolicy {
        /// Unknown metric ID.
        metric: StableId,
    },
    /// A supplemental visualizer policy did not match a visualizer.
    #[error("host policy refers to unknown visualizer `{visualizer}`")]
    UnknownVisualizerPolicy {
        /// Unknown visualizer ID.
        visualizer: StableId,
    },
    /// A visualizer radius exceeded its unit-specific hard cap.
    #[error("visualizer `{visualizer}` radius {observed} exceeds {maximum}")]
    VisualizerRadiusExceeded {
        /// Visualizer ID.
        visualizer: StableId,
        /// Requested radius.
        observed: u32,
        /// Unit-specific hard cap.
        maximum: u32,
    },
    /// A visualizer exceeded 8,192 primitives per update.
    #[error("visualizer `{visualizer}` primitives {observed} exceeds {maximum}")]
    VisualizerPrimitiveLimitExceeded {
        /// Visualizer ID.
        visualizer: StableId,
        /// Requested primitives.
        observed: u32,
        /// Hard cap.
        maximum: u32,
    },
    /// A visualizer exceeded 512 KiB verified upload per update.
    #[error("visualizer `{visualizer}` upload {observed} bytes exceeds {maximum}")]
    VisualizerUploadLimitExceeded {
        /// Visualizer ID.
        visualizer: StableId,
        /// Requested bytes.
        observed: u64,
        /// Hard cap.
        maximum: u64,
    },
}

/// Dense sample key valid only within one registration/engine epoch.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct SampleKey(u32);

impl SampleKey {
    /// Creates a process-local numeric sample key.
    #[must_use]
    pub const fn new(value: u32) -> Self {
        Self(value)
    }

    /// Returns the numeric value.
    #[must_use]
    pub const fn get(self) -> u32 {
        self.0
    }
}

/// Stable identity of a subscription consumer within one host instance.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct SubscriptionConsumerId(u64);

impl SubscriptionConsumerId {
    /// Creates a consumer ID.
    #[must_use]
    pub const fn new(value: u64) -> Self {
        Self(value)
    }
}

/// Generation invalidating asynchronous samples after subscription changes.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct SubscriptionGeneration(u64);

impl SubscriptionGeneration {
    /// First generation used by a newly known target.
    pub const INITIAL: Self = Self(1);

    /// Creates an explicit generation for a wire request or test fixture.
    #[must_use]
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    /// Returns the numeric generation.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

/// Identity of one asynchronous owner-level sample request.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct SampleRequestId(u64);

impl SampleRequestId {
    /// Creates a request ID.
    #[must_use]
    pub const fn new(value: u64) -> Self {
        Self(value)
    }
}

/// Permitted reason for holding a source subscription.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum SubscriptionOrigin {
    /// The item is in a currently visible layout.
    VisibleLayout,
    /// The user explicitly pinned the item.
    PinnedItem,
    /// A bounded diagnostic report explicitly requested it.
    ExplicitReport,
}

/// One source/key pair managed by reference-counted subscription policy.
#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SubscriptionTarget {
    /// Stable callback/source ID.
    pub source: StableId,
    /// Dense key within the current registration epoch.
    pub key: SampleKey,
}

impl SubscriptionTarget {
    /// Creates a source/key target.
    #[must_use]
    pub const fn new(source: StableId, key: SampleKey) -> Self {
        Self { source, key }
    }
}

/// Observable phase of one target's subscription state machine.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SubscriptionPhase {
    /// No consumer and no cancellation acknowledgment remains.
    Inactive,
    /// At least one old request is being cancelled and no consumer remains.
    Cancelling,
    /// At least one allowed consumer keeps dedicated work enabled.
    Active,
}

/// Cancellation command emitted when a generation is retired.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct SampleCancellation {
    /// Target whose request must be cancelled.
    pub target: SubscriptionTarget,
    /// Retired generation.
    pub generation: SubscriptionGeneration,
    /// Request to cancel.
    pub request: SampleRequestId,
}

/// Resulting state after a subscription transition.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SubscriptionTransition {
    /// Current generation after the operation.
    pub generation: SubscriptionGeneration,
    /// Number of distinct consumers.
    pub reference_count: usize,
    /// Whether dedicated callback/query work may run.
    pub dedicated_work_enabled: bool,
    /// Optional asynchronous request cancellation to issue.
    pub cancellation: Option<SampleCancellation>,
}

#[derive(Clone, Debug)]
struct SubscriptionEntry {
    generation: SubscriptionGeneration,
    consumers: BTreeMap<SubscriptionConsumerId, SubscriptionOrigin>,
    in_flight: Option<SampleRequestId>,
    pending_cancellations: BTreeSet<(SubscriptionGeneration, SampleRequestId)>,
}

impl Default for SubscriptionEntry {
    fn default() -> Self {
        Self {
            generation: SubscriptionGeneration::INITIAL,
            consumers: BTreeMap::new(),
            in_flight: None,
            pending_cancellations: BTreeSet::new(),
        }
    }
}

/// Default maximum distinct subscription targets retained by one planner.
pub const DEFAULT_MAX_SUBSCRIPTION_TARGETS: usize = DEFAULT_MAX_OBSERVABILITY_ROWS;

/// Default maximum distinct consumers referencing one target.
pub const DEFAULT_MAX_CONSUMERS_PER_TARGET: usize = 1_024;

/// Default maximum cancellation acknowledgments retained per target.
pub const DEFAULT_MAX_RETAINED_CANCELLATIONS: usize = 8;

/// Defensive memory bounds for one subscription planner.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SubscriptionPlannerLimits {
    targets: usize,
    consumers_per_target: usize,
    retained_cancellations: usize,
}

impl SubscriptionPlannerLimits {
    /// Creates positive planner bounds.
    ///
    /// # Errors
    ///
    /// Returns [`SubscriptionLimitError`] when any bound is zero.
    pub const fn new(
        target_limit: usize,
        consumer_limit: usize,
        cancellation_limit: usize,
    ) -> Result<Self, SubscriptionLimitError> {
        if target_limit == 0 {
            Err(SubscriptionLimitError::ZeroTargets)
        } else if consumer_limit == 0 {
            Err(SubscriptionLimitError::ZeroConsumers)
        } else if cancellation_limit == 0 {
            Err(SubscriptionLimitError::ZeroCancellations)
        } else {
            Ok(Self {
                targets: target_limit,
                consumers_per_target: consumer_limit,
                retained_cancellations: cancellation_limit,
            })
        }
    }
}

impl Default for SubscriptionPlannerLimits {
    fn default() -> Self {
        Self {
            targets: DEFAULT_MAX_SUBSCRIPTION_TARGETS,
            consumers_per_target: DEFAULT_MAX_CONSUMERS_PER_TARGET,
            retained_cancellations: DEFAULT_MAX_RETAINED_CANCELLATIONS,
        }
    }
}

/// Invalid subscription planner memory bounds.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum SubscriptionLimitError {
    /// Target cap was zero.
    #[error("subscription target limit must be positive")]
    ZeroTargets,
    /// Consumer cap was zero.
    #[error("subscription consumer limit must be positive")]
    ZeroConsumers,
    /// Retained cancellation cap was zero.
    #[error("subscription cancellation limit must be positive")]
    ZeroCancellations,
}

/// Pure-data subscription reference counter and cancellation barrier.
#[derive(Clone, Debug, Default)]
pub struct SubscriptionPlanner {
    entries: BTreeMap<SubscriptionTarget, SubscriptionEntry>,
    limits: SubscriptionPlannerLimits,
}

impl SubscriptionPlanner {
    /// Creates an empty planner with explicit validated memory bounds.
    #[must_use]
    pub const fn new(limits: SubscriptionPlannerLimits) -> Self {
        Self {
            entries: BTreeMap::new(),
            limits,
        }
    }

    /// Adds an allowed consumer idempotently and enables dedicated work on 0→1.
    ///
    /// # Errors
    ///
    /// Returns [`SubscriptionError`] before allocation when a new target or
    /// consumer would exceed the planner's hard bound.
    pub fn subscribe(
        &mut self,
        target: SubscriptionTarget,
        consumer: SubscriptionConsumerId,
        origin: SubscriptionOrigin,
    ) -> Result<SubscriptionTransition, SubscriptionError> {
        if !self.entries.contains_key(&target) && self.entries.len() == self.limits.targets {
            return Err(SubscriptionError::TargetLimitExceeded {
                maximum: self.limits.targets,
            });
        }
        let entry = self.entries.entry(target.clone()).or_default();
        if !entry.consumers.contains_key(&consumer)
            && entry.consumers.len() == self.limits.consumers_per_target
        {
            return Err(SubscriptionError::ConsumerLimitExceeded {
                target,
                maximum: self.limits.consumers_per_target,
            });
        }
        entry.consumers.entry(consumer).or_insert(origin);
        Ok(transition(entry, None))
    }

    /// Removes one consumer and retires/cancels work after the last reference.
    ///
    /// # Errors
    ///
    /// Returns [`SubscriptionError`] for an unknown consumer or generation
    /// overflow. Overflow leaves the live subscription unchanged.
    pub fn unsubscribe(
        &mut self,
        target: &SubscriptionTarget,
        consumer: SubscriptionConsumerId,
    ) -> Result<SubscriptionTransition, SubscriptionError> {
        let entry =
            self.entries
                .get_mut(target)
                .ok_or_else(|| SubscriptionError::UnknownTarget {
                    target: target.clone(),
                })?;
        if !entry.consumers.contains_key(&consumer) {
            return Err(SubscriptionError::UnknownConsumer {
                target: target.clone(),
                consumer,
            });
        }
        let is_last = entry.consumers.len() == 1;
        let next = is_last
            .then(|| checked_next_generation(entry.generation))
            .transpose()?;
        entry.consumers.remove(&consumer);
        let cancellation = if let Some(next) = next {
            let retired = entry.generation;
            entry.generation = next;
            entry.in_flight.take().map(|request| {
                retain_cancellation(entry, retired, request, self.limits.retained_cancellations);
                SampleCancellation {
                    target: target.clone(),
                    generation: retired,
                    request,
                }
            })
        } else {
            None
        };
        Ok(transition(entry, cancellation))
    }

    /// Invalidates the current generation while retaining active consumers.
    ///
    /// World/target/permission/context changes use this barrier. Cancellation
    /// is an optimization; generation comparison is the correctness boundary.
    ///
    /// # Errors
    ///
    /// Returns [`SubscriptionError`] when the target is unknown or its
    /// generation cannot increment without wrapping.
    pub fn invalidate(
        &mut self,
        target: &SubscriptionTarget,
    ) -> Result<SubscriptionTransition, SubscriptionError> {
        let entry =
            self.entries
                .get_mut(target)
                .ok_or_else(|| SubscriptionError::UnknownTarget {
                    target: target.clone(),
                })?;
        let next = checked_next_generation(entry.generation)?;
        let retired = entry.generation;
        entry.generation = next;
        let cancellation = entry.in_flight.take().map(|request| {
            retain_cancellation(entry, retired, request, self.limits.retained_cancellations);
            SampleCancellation {
                target: target.clone(),
                generation: retired,
                request,
            }
        });
        Ok(transition(entry, cancellation))
    }

    /// Starts at most one request for an active target and returns its generation.
    ///
    /// # Errors
    ///
    /// Returns [`SubscriptionError`] when the target is unknown/inactive or
    /// already has a current-generation request.
    pub fn begin_request(
        &mut self,
        target: &SubscriptionTarget,
        request: SampleRequestId,
    ) -> Result<SubscriptionGeneration, SubscriptionError> {
        let entry =
            self.entries
                .get_mut(target)
                .ok_or_else(|| SubscriptionError::UnknownTarget {
                    target: target.clone(),
                })?;
        if entry.consumers.is_empty() {
            return Err(SubscriptionError::InactiveTarget {
                target: target.clone(),
            });
        }
        if let Some(existing) = entry.in_flight {
            return Err(SubscriptionError::RequestAlreadyInFlight {
                target: target.clone(),
                request: existing,
            });
        }
        entry.in_flight = Some(request);
        Ok(entry.generation)
    }

    /// Classifies completion against the live or a retired generation.
    ///
    /// # Errors
    ///
    /// Returns [`SubscriptionError`] when no matching live or retired request
    /// exists in the current state-machine horizon.
    pub fn complete_request(
        &mut self,
        target: &SubscriptionTarget,
        generation: SubscriptionGeneration,
        request: SampleRequestId,
    ) -> Result<RequestCompletion, SubscriptionError> {
        let entry =
            self.entries
                .get_mut(target)
                .ok_or_else(|| SubscriptionError::UnknownTarget {
                    target: target.clone(),
                })?;
        if generation == entry.generation && entry.in_flight == Some(request) {
            entry.in_flight = None;
            return Ok(RequestCompletion::Accepted);
        }
        if entry.pending_cancellations.remove(&(generation, request))
            || generation < entry.generation
        {
            return Ok(RequestCompletion::DiscardedRetiredGeneration);
        }
        Err(SubscriptionError::UnknownRequest {
            target: target.clone(),
            generation,
            request,
        })
    }

    /// Acknowledges cancellation of a retired request idempotently.
    #[must_use]
    pub fn acknowledge_cancellation(&mut self, cancellation: &SampleCancellation) -> bool {
        self.entries
            .get_mut(&cancellation.target)
            .is_some_and(|entry| {
                entry
                    .pending_cancellations
                    .remove(&(cancellation.generation, cancellation.request))
            })
    }

    /// Returns the phase of a known target.
    #[must_use]
    pub fn phase(&self, target: &SubscriptionTarget) -> Option<SubscriptionPhase> {
        self.entries.get(target).map(|entry| {
            if !entry.consumers.is_empty() {
                SubscriptionPhase::Active
            } else if !entry.pending_cancellations.is_empty() {
                SubscriptionPhase::Cancelling
            } else {
                SubscriptionPhase::Inactive
            }
        })
    }

    /// Returns the live generation of a known target.
    #[must_use]
    pub fn generation(&self, target: &SubscriptionTarget) -> Option<SubscriptionGeneration> {
        self.entries.get(target).map(|entry| entry.generation)
    }

    /// Returns active targets in canonical source/key order.
    #[must_use]
    pub fn active_targets(&self) -> Vec<&SubscriptionTarget> {
        self.entries
            .iter()
            .filter_map(|(target, entry)| (!entry.consumers.is_empty()).then_some(target))
            .collect()
    }
}

fn retain_cancellation(
    entry: &mut SubscriptionEntry,
    generation: SubscriptionGeneration,
    request: SampleRequestId,
    limit: usize,
) {
    entry.pending_cancellations.insert((generation, request));
    while entry.pending_cancellations.len() > limit {
        entry.pending_cancellations.pop_first();
    }
}

fn checked_next_generation(
    generation: SubscriptionGeneration,
) -> Result<SubscriptionGeneration, SubscriptionError> {
    generation
        .0
        .checked_add(1)
        .map(SubscriptionGeneration)
        .ok_or(SubscriptionError::GenerationOverflow { generation })
}

fn transition(
    entry: &SubscriptionEntry,
    cancellation: Option<SampleCancellation>,
) -> SubscriptionTransition {
    SubscriptionTransition {
        generation: entry.generation,
        reference_count: entry.consumers.len(),
        dedicated_work_enabled: !entry.consumers.is_empty(),
        cancellation,
    }
}

/// Completion classification after generation validation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RequestCompletion {
    /// Completion belongs to the current active request.
    Accepted,
    /// Completion belongs to a retired generation and must not be displayed.
    DiscardedRetiredGeneration,
}

/// Invalid subscription state transition.
#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum SubscriptionError {
    /// A new target would exceed the planner's hard bound.
    #[error("subscription target limit {maximum} exceeded")]
    TargetLimitExceeded {
        /// Configured maximum targets.
        maximum: usize,
    },
    /// A new consumer would exceed one target's hard bound.
    #[error("subscription target `{target:?}` consumer limit {maximum} exceeded")]
    ConsumerLimitExceeded {
        /// Target at capacity.
        target: SubscriptionTarget,
        /// Configured maximum consumers.
        maximum: usize,
    },
    /// Target was never registered with this planner.
    #[error("subscription target `{target:?}` is unknown")]
    UnknownTarget {
        /// Unknown target.
        target: SubscriptionTarget,
    },
    /// Consumer was not holding a reference.
    #[error("consumer `{consumer:?}` is not subscribed to `{target:?}`")]
    UnknownConsumer {
        /// Target involved.
        target: SubscriptionTarget,
        /// Unknown consumer.
        consumer: SubscriptionConsumerId,
    },
    /// No consumer permits dedicated work.
    #[error("subscription target `{target:?}` is inactive")]
    InactiveTarget {
        /// Inactive target.
        target: SubscriptionTarget,
    },
    /// Only one current request may be in flight per target.
    #[error("subscription target `{target:?}` already has request `{request:?}`")]
    RequestAlreadyInFlight {
        /// Target involved.
        target: SubscriptionTarget,
        /// Existing request.
        request: SampleRequestId,
    },
    /// Completion did not match live or retained retired state.
    #[error("request `{request:?}` generation `{generation:?}` is unknown for `{target:?}`")]
    UnknownRequest {
        /// Target involved.
        target: SubscriptionTarget,
        /// Supplied generation.
        generation: SubscriptionGeneration,
        /// Supplied request ID.
        request: SampleRequestId,
    },
    /// Generation increment would wrap and admit an old sample.
    #[error("subscription generation `{generation:?}` cannot increment")]
    GenerationOverflow {
        /// Exhausted generation.
        generation: SubscriptionGeneration,
    },
}

/// Epoch of one `EngineInstance`.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct EngineEpoch(u64);

impl EngineEpoch {
    /// Creates an engine epoch.
    #[must_use]
    pub const fn new(value: u64) -> Self {
        Self(value)
    }
}

/// Epoch of an activated world.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct WorldEpoch(u64);

impl WorldEpoch {
    /// Creates a world epoch.
    #[must_use]
    pub const fn new(value: u64) -> Self {
        Self(value)
    }
}

/// Epoch of an authoritative inspected target.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct TargetEpoch(u64);

impl TargetEpoch {
    /// Creates a target epoch.
    #[must_use]
    pub const fn new(value: u64) -> Self {
        Self(value)
    }
}

/// Authoritative fixed-tick observation coordinate.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct FixedTick(u64);

impl FixedTick {
    /// Creates a fixed-tick coordinate.
    #[must_use]
    pub const fn new(value: u64) -> Self {
        Self(value)
    }
}

/// Host monotonic time in nanoseconds from an instance-local origin.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct MonotonicNanos(u64);

impl MonotonicNanos {
    /// Creates an instance-local monotonic coordinate.
    #[must_use]
    pub const fn new(value: u64) -> Self {
        Self(value)
    }
}

/// Host-owned rules for deriving fresh versus stale.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FreshnessPolicy {
    /// Maximum accepted fixed-tick age.
    pub max_tick_age: Option<u64>,
    /// Maximum accepted monotonic age in nanoseconds.
    pub max_monotonic_age_ns: Option<u64>,
    /// Minimum source revision accepted as fresh.
    pub minimum_source_revision: Option<u64>,
}

/// One requested numeric key plus host-only validation expectations.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SampleRequestEntryV1 {
    /// Dense registration key.
    pub key: SampleKey,
    /// Schema expected for a Value response.
    pub expected_schema: SchemaId,
    /// Host-owned freshness policy.
    pub freshness: FreshnessPolicy,
}

/// Limits for one owner-level sample batch.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SampleBatchLimits {
    key_limit: usize,
    max_response_bytes: u32,
    max_error_arguments: usize,
    max_error_text_bytes: usize,
}

impl SampleBatchLimits {
    /// Creates positive hard limits.
    ///
    /// # Errors
    ///
    /// Returns [`SampleBatchLimitError`] if any limit is zero.
    pub const fn new(
        key_limit: usize,
        max_response_bytes: u32,
        max_error_arguments: usize,
        max_error_text_bytes: usize,
    ) -> Result<Self, SampleBatchLimitError> {
        if key_limit == 0 {
            Err(SampleBatchLimitError::ZeroKeys)
        } else if max_response_bytes == 0 {
            Err(SampleBatchLimitError::ZeroBytes)
        } else if max_error_arguments == 0 {
            Err(SampleBatchLimitError::ZeroErrorArguments)
        } else if max_error_text_bytes == 0 {
            Err(SampleBatchLimitError::ZeroErrorTextBytes)
        } else {
            Ok(Self {
                key_limit,
                max_response_bytes,
                max_error_arguments,
                max_error_text_bytes,
            })
        }
    }
}

/// Invalid owner-level sample batch limits.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum SampleBatchLimitError {
    /// Key count was zero.
    #[error("sample key limit must be positive")]
    ZeroKeys,
    /// Byte cap was zero.
    #[error("sample byte limit must be positive")]
    ZeroBytes,
    /// Error argument count was zero.
    #[error("sample error argument limit must be positive")]
    ZeroErrorArguments,
    /// Error text byte cap was zero.
    #[error("sample error text limit must be positive")]
    ZeroErrorTextBytes,
}

/// Epoch, generation, and deadline shared by one owner-level request.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SampleRequestContextV1 {
    /// Engine instance epoch.
    pub engine_epoch: EngineEpoch,
    /// Optional active world epoch.
    pub world_epoch: Option<WorldEpoch>,
    /// Optional authoritative target epoch.
    pub target_epoch: Option<TargetEpoch>,
    /// Current subscription generation.
    pub generation: SubscriptionGeneration,
    /// Instance-local monotonic deadline.
    pub deadline: MonotonicNanos,
}

/// Validated owner-level request batch.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SampleRequestBatchV1 {
    /// Engine instance epoch.
    pub engine_epoch: EngineEpoch,
    /// World epoch when the source depends on a world.
    pub world_epoch: Option<WorldEpoch>,
    /// Target epoch when the source depends on an inspected target.
    pub target_epoch: Option<TargetEpoch>,
    /// Current subscription generation.
    pub generation: SubscriptionGeneration,
    /// Stable-sorted numeric keys with validation metadata.
    pub entries: Vec<SampleRequestEntryV1>,
    /// Instance-local monotonic deadline.
    pub deadline: MonotonicNanos,
    /// Maximum encoded response bytes.
    pub byte_budget: u32,
}

impl SampleRequestBatchV1 {
    /// Creates a request, sorting keys and rejecting duplicates or hard-cap overflow.
    ///
    /// # Errors
    ///
    /// Returns [`SampleProtocolError`] when no key is requested, limits are
    /// exceeded, a key is duplicated, or the byte budget is zero/too large.
    pub fn new<I>(
        context: SampleRequestContextV1,
        entries: I,
        byte_budget: u32,
        limits: SampleBatchLimits,
    ) -> Result<Self, SampleProtocolError>
    where
        I: IntoIterator<Item = SampleRequestEntryV1>,
    {
        let mut collected: Vec<SampleRequestEntryV1> = Vec::new();
        for entry in entries {
            collected.push(entry);
            if collected.len() > limits.key_limit {
                return Err(SampleProtocolError::KeyLimitExceeded {
                    observed: collected.len(),
                    maximum: limits.key_limit,
                });
            }
        }
        if collected.is_empty() {
            return Err(SampleProtocolError::EmptyRequest);
        }
        if byte_budget == 0 || byte_budget > limits.max_response_bytes {
            return Err(SampleProtocolError::InvalidByteBudget {
                observed: byte_budget,
                maximum: limits.max_response_bytes,
            });
        }
        collected.sort_by_key(|entry| entry.key);
        if let Some(pair) = collected.windows(2).find(|pair| pair[0].key == pair[1].key) {
            return Err(SampleProtocolError::DuplicateKey { key: pair[0].key });
        }
        Ok(Self {
            engine_epoch: context.engine_epoch,
            world_epoch: context.world_epoch,
            target_epoch: context.target_epoch,
            generation: context.generation,
            entries: collected,
            deadline: context.deadline,
            byte_budget,
        })
    }
}

/// Typed sample payload tagged by the expected schema ID.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TypedSampleValueV1 {
    /// Stable value schema.
    pub schema: SchemaId,
    /// Schema-owned JSON projection at this portable boundary.
    pub value: Value,
}

/// Typed reason why a requested source has no value.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum UnavailableReasonV1 {
    /// No world is active.
    NoWorld,
    /// No authoritative target is active.
    NoTarget,
    /// This source has no headless representation.
    Headless,
    /// The realization cannot provide this source.
    NotSupported,
    /// The owning source is absent from the active closure.
    SourceNotInstalled,
}

/// Whether a stable source error may succeed on retry.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum RetryabilityV1 {
    /// Repeating without a state/configuration change will not help.
    Permanent,
    /// A later bounded retry may succeed.
    Retryable,
}

/// Bounded typed diagnostic argument.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(
    deny_unknown_fields,
    rename_all = "kebab-case",
    tag = "type",
    content = "value"
)]
pub enum DiagnosticArgumentV1 {
    /// Boolean argument.
    Bool(bool),
    /// Signed integer argument.
    Integer(i64),
    /// Unsigned integer argument.
    Unsigned(u64),
    /// Bounded NFC text argument.
    Text(String),
    /// Stable identifier argument.
    StableId(StableId),
}

/// Raw package-produced result for one requested key.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case", tag = "state")]
pub enum RawSampleStateV1 {
    /// A typed observation with host-comparable metadata.
    Value {
        /// Typed payload.
        value: TypedSampleValueV1,
        /// Fixed tick when observed.
        observed_fixed_tick: FixedTick,
        /// Monotonic instant when observed.
        observed_monotonic: MonotonicNanos,
        /// Source-owned monotonic revision.
        source_revision: u64,
    },
    /// No value exists in the current technical context.
    Unavailable {
        /// Typed absence reason.
        reason: UnavailableReasonV1,
    },
    /// Sampling failed with a stable code and bounded arguments.
    Error {
        /// Stable diagnostic code.
        code: StableId,
        /// Canonically keyed typed arguments.
        arguments: BTreeMap<String, DiagnosticArgumentV1>,
        /// Retry policy.
        retryability: RetryabilityV1,
    },
}

/// One raw sample response row.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RawSampleRowV1 {
    /// Requested dense key.
    pub key: SampleKey,
    /// Explicit Value, Unavailable, or Error state.
    pub state: RawSampleStateV1,
}

/// Raw owner-level response batch with a preflight byte declaration.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SampleBatchV1 {
    /// Engine instance epoch copied from the request.
    pub engine_epoch: EngineEpoch,
    /// World epoch copied from the request.
    pub world_epoch: Option<WorldEpoch>,
    /// Target epoch copied from the request.
    pub target_epoch: Option<TargetEpoch>,
    /// Subscription generation copied from the request.
    pub generation: SubscriptionGeneration,
    /// Stable-sorted rows.
    pub rows: Vec<RawSampleRowV1>,
    /// Exact canonical encoded bytes of this envelope.
    pub declared_bytes: u32,
}

impl SampleBatchV1 {
    /// Computes a self-consistent canonical envelope byte declaration.
    ///
    /// # Errors
    ///
    /// Returns [`SampleBatchEncodingError`] if canonical serialization fails,
    /// its length exceeds `u32`, or the declaration does not converge.
    pub fn with_measured_bytes(mut self) -> Result<Self, SampleBatchEncodingError> {
        for _ in 0..4 {
            let bytes = canonical_json_bytes(&self)?;
            let measured = u32::try_from(bytes.len())
                .map_err(|_| SampleBatchEncodingError::LengthExceedsU32 { bytes: bytes.len() })?;
            if self.declared_bytes == measured {
                return Ok(self);
            }
            self.declared_bytes = measured;
        }
        Err(SampleBatchEncodingError::DeclarationDidNotConverge)
    }
}

/// Failure to produce a preflightable sample batch envelope.
#[derive(Debug, Error)]
pub enum SampleBatchEncodingError {
    /// Canonical serialization failed.
    #[error(transparent)]
    Canonical(#[from] CanonicalJsonError),
    /// Canonical bytes cannot fit the wire declaration.
    #[error("sample batch length {bytes} exceeds u32")]
    LengthExceedsU32 {
        /// Observed encoded length.
        bytes: usize,
    },
    /// Updating the decimal declaration did not reach a fixed point.
    #[error("sample batch byte declaration did not converge")]
    DeclarationDidNotConverge,
}

/// Host context used to reject epochs and derive freshness.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HostSampleContext {
    /// Current engine epoch.
    pub engine_epoch: EngineEpoch,
    /// Current world epoch.
    pub world_epoch: Option<WorldEpoch>,
    /// Current target epoch.
    pub target_epoch: Option<TargetEpoch>,
    /// Current subscription generation.
    pub generation: SubscriptionGeneration,
    /// Current authoritative fixed tick.
    pub fixed_tick: FixedTick,
    /// Current monotonic instant.
    pub monotonic: MonotonicNanos,
}

/// Host-derived freshness of a Value sample.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum SampleFreshness {
    /// Observation lies within all host TTL/revision bounds.
    Fresh,
    /// Observation is valid but older than at least one host bound.
    Stale,
}

/// Host-accepted state for one requested key.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case", tag = "state")]
pub enum HostSampleStateV1 {
    /// Typed value plus host-derived freshness.
    Value {
        /// Typed payload.
        value: TypedSampleValueV1,
        /// Host-derived status.
        freshness: SampleFreshness,
        /// Fixed tick reported by the validated source response.
        observed_fixed_tick: FixedTick,
        /// Monotonic instant reported by the validated source response.
        observed_monotonic: MonotonicNanos,
        /// Source revision.
        source_revision: u64,
    },
    /// Explicit technical absence.
    Unavailable {
        /// Typed absence reason.
        reason: UnavailableReasonV1,
    },
    /// Stable bounded source error.
    Error {
        /// Stable diagnostic code.
        code: StableId,
        /// Typed bounded arguments.
        arguments: BTreeMap<String, DiagnosticArgumentV1>,
        /// Retry policy.
        retryability: RetryabilityV1,
    },
}

/// One validated host sample row.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HostSampleRowV1 {
    /// Dense requested key.
    pub key: SampleKey,
    /// Validated host state.
    pub state: HostSampleStateV1,
}

/// Validated host-owned batch in requested key order.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AcceptedSampleBatchV1 {
    /// Stable-sorted validated rows.
    pub rows: Vec<HostSampleRowV1>,
}

/// Epoch/generation reason for discarding a complete response.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SampleDiscardReason {
    /// Engine instance changed.
    EngineEpoch,
    /// Active world changed or disappeared.
    WorldEpoch,
    /// Authoritative target changed or disappeared.
    TargetEpoch,
    /// Subscription generation was retired.
    SubscriptionGeneration,
}

/// Result of response validation before presentation or history insertion.
#[derive(Clone, Debug, PartialEq)]
pub enum SampleBatchDisposition {
    /// Complete validated batch.
    Accepted(AcceptedSampleBatchV1),
    /// Complete batch discarded at an epoch/generation barrier.
    Discarded(SampleDiscardReason),
}

/// Validates a raw batch and derives freshness without trusting the source.
///
/// Epoch or generation mismatch discards the entire batch before it can be
/// displayed as stale. Protocol errors also reject the entire batch: rows are
/// never partially applied or truncated.
///
/// # Errors
///
/// Returns [`SampleProtocolError`] for key-set/order, schema, byte, argument,
/// future-observation, or encoding violations.
#[allow(
    clippy::too_many_lines,
    reason = "the explicit whole-batch protocol sequence is kept together for auditability"
)]
pub fn validate_sample_batch(
    request: &SampleRequestBatchV1,
    response: SampleBatchV1,
    host: HostSampleContext,
    limits: SampleBatchLimits,
) -> Result<SampleBatchDisposition, SampleProtocolError> {
    if response.engine_epoch != request.engine_epoch || response.engine_epoch != host.engine_epoch {
        return Ok(SampleBatchDisposition::Discarded(
            SampleDiscardReason::EngineEpoch,
        ));
    }
    if response.world_epoch != request.world_epoch || response.world_epoch != host.world_epoch {
        return Ok(SampleBatchDisposition::Discarded(
            SampleDiscardReason::WorldEpoch,
        ));
    }
    if response.target_epoch != request.target_epoch || response.target_epoch != host.target_epoch {
        return Ok(SampleBatchDisposition::Discarded(
            SampleDiscardReason::TargetEpoch,
        ));
    }
    if response.generation != request.generation || response.generation != host.generation {
        return Ok(SampleBatchDisposition::Discarded(
            SampleDiscardReason::SubscriptionGeneration,
        ));
    }
    if response.rows.len() > limits.key_limit {
        return Err(SampleProtocolError::KeyLimitExceeded {
            observed: response.rows.len(),
            maximum: limits.key_limit,
        });
    }
    if response.rows.len() != request.entries.len() {
        return Err(SampleProtocolError::KeySetSizeMismatch {
            requested: request.entries.len(),
            returned: response.rows.len(),
        });
    }
    if response.declared_bytes == 0
        || response.declared_bytes > request.byte_budget
        || response.declared_bytes > limits.max_response_bytes
    {
        return Err(SampleProtocolError::ResponseByteLimitExceeded {
            observed: response.declared_bytes,
            maximum: request.byte_budget.min(limits.max_response_bytes),
        });
    }
    let encoded = canonical_json_bytes(&response)
        .map_err(|source| SampleProtocolError::CanonicalEncoding { source })?;
    let actual =
        u32::try_from(encoded.len()).map_err(|_| SampleProtocolError::EncodedLengthExceedsU32 {
            observed: encoded.len(),
        })?;
    if actual != response.declared_bytes {
        return Err(SampleProtocolError::DeclaredByteMismatch {
            declared: response.declared_bytes,
            actual,
        });
    }

    let mut accepted = Vec::with_capacity(response.rows.len());
    for (expected, row) in request.entries.iter().zip(response.rows) {
        if row.key != expected.key {
            return Err(SampleProtocolError::KeyOrderOrSetMismatch {
                expected: expected.key,
                returned: row.key,
            });
        }
        let state = match row.state {
            RawSampleStateV1::Value {
                value,
                observed_fixed_tick,
                observed_monotonic,
                source_revision,
            } => {
                if value.schema != expected.expected_schema {
                    return Err(SampleProtocolError::WrongValueSchema {
                        key: row.key,
                        expected: Box::new(expected.expected_schema.clone()),
                        returned: Box::new(value.schema),
                    });
                }
                if observed_fixed_tick > host.fixed_tick || observed_monotonic > host.monotonic {
                    return Err(SampleProtocolError::ObservationFromFuture { key: row.key });
                }
                let stale_tick = expected.freshness.max_tick_age.is_some_and(|maximum| {
                    host.fixed_tick.0.saturating_sub(observed_fixed_tick.0) > maximum
                });
                let stale_time = expected
                    .freshness
                    .max_monotonic_age_ns
                    .is_some_and(|maximum| {
                        host.monotonic.0.saturating_sub(observed_monotonic.0) > maximum
                    });
                let stale_revision = expected
                    .freshness
                    .minimum_source_revision
                    .is_some_and(|minimum| source_revision < minimum);
                let freshness = if stale_tick || stale_time || stale_revision {
                    SampleFreshness::Stale
                } else {
                    SampleFreshness::Fresh
                };
                HostSampleStateV1::Value {
                    value,
                    freshness,
                    observed_fixed_tick,
                    observed_monotonic,
                    source_revision,
                }
            }
            RawSampleStateV1::Unavailable { reason } => HostSampleStateV1::Unavailable { reason },
            RawSampleStateV1::Error {
                code,
                arguments,
                retryability,
            } => {
                validate_error_arguments(row.key, &arguments, limits)?;
                HostSampleStateV1::Error {
                    code,
                    arguments,
                    retryability,
                }
            }
        };
        accepted.push(HostSampleRowV1 {
            key: row.key,
            state,
        });
    }
    Ok(SampleBatchDisposition::Accepted(AcceptedSampleBatchV1 {
        rows: accepted,
    }))
}

fn validate_error_arguments(
    key: SampleKey,
    arguments: &BTreeMap<String, DiagnosticArgumentV1>,
    limits: SampleBatchLimits,
) -> Result<(), SampleProtocolError> {
    if arguments.len() > limits.max_error_arguments {
        return Err(SampleProtocolError::ErrorArgumentLimitExceeded {
            key,
            observed: arguments.len(),
            maximum: limits.max_error_arguments,
        });
    }
    let mut text_bytes = 0_usize;
    for (name, argument) in arguments {
        if name.is_empty() || !is_nfc(name) {
            return Err(SampleProtocolError::InvalidErrorArgumentText { key });
        }
        text_bytes = text_bytes
            .checked_add(name.len())
            .ok_or(SampleProtocolError::ErrorArgumentByteOverflow { key })?;
        if let DiagnosticArgumentV1::Text(value) = argument {
            if !is_nfc(value) {
                return Err(SampleProtocolError::InvalidErrorArgumentText { key });
            }
            text_bytes = text_bytes
                .checked_add(value.len())
                .ok_or(SampleProtocolError::ErrorArgumentByteOverflow { key })?;
        }
        if text_bytes > limits.max_error_text_bytes {
            return Err(SampleProtocolError::ErrorArgumentTextLimitExceeded {
                key,
                observed: text_bytes,
                maximum: limits.max_error_text_bytes,
            });
        }
    }
    Ok(())
}

/// Invalid sample request or response protocol state.
#[derive(Debug, Error)]
pub enum SampleProtocolError {
    /// A request contained no keys.
    #[error("sample request contains no keys")]
    EmptyRequest,
    /// Key count exceeded the configured batch cap.
    #[error("sample batch has {observed} keys; maximum is {maximum}")]
    KeyLimitExceeded {
        /// Observed keys.
        observed: usize,
        /// Hard maximum.
        maximum: usize,
    },
    /// A request repeated a dense key.
    #[error("sample request repeats key `{key:?}`")]
    DuplicateKey {
        /// Repeated key.
        key: SampleKey,
    },
    /// Request byte budget was zero or above host policy.
    #[error("sample byte budget {observed} is outside 1..={maximum}")]
    InvalidByteBudget {
        /// Requested bytes.
        observed: u32,
        /// Host maximum.
        maximum: u32,
    },
    /// Response row count differed from the request.
    #[error("sample response has {returned} rows for {requested} requested keys")]
    KeySetSizeMismatch {
        /// Requested rows.
        requested: usize,
        /// Returned rows.
        returned: usize,
    },
    /// Response order or exact key set differed.
    #[error("sample response returned key `{returned:?}` where `{expected:?}` was required")]
    KeyOrderOrSetMismatch {
        /// Requested key.
        expected: SampleKey,
        /// Returned key.
        returned: SampleKey,
    },
    /// Declared response bytes exceeded request or host policy.
    #[error("sample response declares {observed} bytes; maximum is {maximum}")]
    ResponseByteLimitExceeded {
        /// Declared bytes.
        observed: u32,
        /// Effective maximum.
        maximum: u32,
    },
    /// Canonical envelope bytes differed from the preflight declaration.
    #[error("sample response declared {declared} bytes but encoded to {actual}")]
    DeclaredByteMismatch {
        /// Source declaration.
        declared: u32,
        /// Measured canonical bytes.
        actual: u32,
    },
    /// Encoded response length did not fit the wire field.
    #[error("sample response encoded length {observed} exceeds u32")]
    EncodedLengthExceedsU32 {
        /// Measured length.
        observed: usize,
    },
    /// Canonical response serialization failed.
    #[error("failed to encode sample response: {source}")]
    CanonicalEncoding {
        /// Serialization error.
        #[source]
        source: CanonicalJsonError,
    },
    /// Value schema did not match registration expectations.
    #[error("sample key `{key:?}` returned schema `{returned}` instead of `{expected}`")]
    WrongValueSchema {
        /// Dense key.
        key: SampleKey,
        /// Expected schema.
        expected: Box<SchemaId>,
        /// Returned schema.
        returned: Box<SchemaId>,
    },
    /// Source claimed an observation later than host time.
    #[error("sample key `{key:?}` has an observation from the future")]
    ObservationFromFuture {
        /// Dense key.
        key: SampleKey,
    },
    /// Stable error arguments exceeded count policy.
    #[error("sample key `{key:?}` has {observed} error arguments; maximum is {maximum}")]
    ErrorArgumentLimitExceeded {
        /// Dense key.
        key: SampleKey,
        /// Observed arguments.
        observed: usize,
        /// Maximum arguments.
        maximum: usize,
    },
    /// Error argument bytes overflowed host arithmetic.
    #[error("sample key `{key:?}` error argument byte count overflowed")]
    ErrorArgumentByteOverflow {
        /// Dense key.
        key: SampleKey,
    },
    /// Error argument name or text was empty/non-NFC.
    #[error("sample key `{key:?}` contains invalid error argument text")]
    InvalidErrorArgumentText {
        /// Dense key.
        key: SampleKey,
    },
    /// Error argument text exceeded the byte cap.
    #[error("sample key `{key:?}` error text has {observed} bytes; maximum is {maximum}")]
    ErrorArgumentTextLimitExceeded {
        /// Dense key.
        key: SampleKey,
        /// Observed bytes.
        observed: usize,
        /// Maximum bytes.
        maximum: usize,
    },
}

/// Stable section used in canonical diagnostic report ordering.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ReportSectionV1 {
    /// Application and platform facts.
    Application,
    /// Lock, registration, and engine fingerprints.
    Fingerprint,
    /// Explicitly requested metric snapshots or history summaries.
    Metric,
    /// Relevant non-secret setting differences.
    Setting,
    /// World facts permitted by consent.
    World,
    /// Target/chunk facts permitted by consent.
    Target,
    /// Unavailable/truncation/budget facts.
    Budget,
}

/// Canonical report record key.
#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ReportRecordKeyV1 {
    /// Stable report section.
    pub section: ReportSectionV1,
    /// Stable semantic item ID.
    pub id: StableId,
}

/// Privacy classification applied before report serialization.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ReportSensitivityV1 {
    /// Safe non-secret diagnostic data.
    Public,
    /// World identity, redacted unless explicitly included.
    WorldIdentity,
    /// Selected coordinate, included only after explicit opt-in.
    Coordinate,
    /// Local path, redacted unless explicitly included.
    PrivatePath,
    /// Server address, redacted unless explicitly included.
    ServerAddress,
    /// Player identity, redacted unless explicitly included.
    PlayerIdentity,
    /// Secret data, rejected before ordinary report serialization.
    Secret,
}

/// Explicit consent controlling diagnostic report disclosure.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ReportConsentV1 {
    included: BTreeSet<ReportSensitivityV1>,
}

impl ReportConsentV1 {
    /// Adds one non-secret sensitivity class to the explicit consent set.
    ///
    /// `Public` is always included. `Secret` is rejected by the report builder,
    /// so adding either has no effect.
    #[must_use]
    pub fn include(mut self, sensitivity: ReportSensitivityV1) -> Self {
        if !matches!(
            sensitivity,
            ReportSensitivityV1::Public | ReportSensitivityV1::Secret
        ) {
            self.included.insert(sensitivity);
        }
        self
    }

    /// Returns whether a sensitivity class may be emitted.
    #[must_use]
    pub fn allows(&self, sensitivity: ReportSensitivityV1) -> bool {
        match sensitivity {
            ReportSensitivityV1::Public => true,
            ReportSensitivityV1::Secret => false,
            other => self.included.contains(&other),
        }
    }
}

/// Unredacted candidate record supplied to the bounded report builder.
#[derive(Clone, Debug, PartialEq)]
pub struct DiagnosticReportInputV1 {
    /// Canonical record key.
    pub key: ReportRecordKeyV1,
    /// Structured value.
    pub value: Value,
    /// Privacy classification.
    pub sensitivity: ReportSensitivityV1,
}

/// Redacted-or-present report value.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(
    deny_unknown_fields,
    rename_all = "kebab-case",
    tag = "state",
    content = "value"
)]
pub enum ReportValueV1 {
    /// Included structured value.
    Present(Value),
    /// Value omitted by privacy policy before serialization.
    Redacted,
}

/// One canonical diagnostic report row.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DiagnosticReportRecordV1 {
    /// Stable section/ID key.
    pub key: ReportRecordKeyV1,
    /// Included or redacted value.
    pub value: ReportValueV1,
}

/// Applied report bounds retained as truncation evidence.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AppliedReportLimitsV1 {
    /// Maximum output records.
    pub max_records: usize,
    /// Maximum canonical report bytes.
    pub max_bytes: usize,
}

/// Versioned, redacted, bounded, canonically ordered diagnostic report.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DiagnosticReportV1 {
    /// Report schema version, currently one.
    pub schema_version: u32,
    /// Canonically ordered output records.
    pub records: Vec<DiagnosticReportRecordV1>,
    /// Candidate count omitted by stable suffix truncation.
    pub truncated_records: usize,
    /// Limits that caused any truncation.
    pub limits: AppliedReportLimitsV1,
}

impl DiagnosticReportV1 {
    /// Returns canonical compact JSON suitable for preview/copy/write.
    ///
    /// # Errors
    ///
    /// Returns [`CanonicalJsonError`] if serialization fails.
    pub fn canonical_bytes(&self) -> Result<Vec<u8>, CanonicalJsonError> {
        canonical_json_bytes(self)
    }

    /// Returns the hash of canonical report bytes.
    ///
    /// # Errors
    ///
    /// Returns [`CanonicalJsonError`] if serialization fails.
    pub fn canonical_hash(&self) -> Result<CanonicalHash, CanonicalJsonError> {
        canonical_json_hash(self)
    }
}

/// Defensive input and output bounds for report construction.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DiagnosticReportLimits {
    candidate_limit: usize,
    max_records: usize,
    max_bytes: usize,
    max_value_bytes: usize,
}

impl DiagnosticReportLimits {
    /// Creates positive report bounds.
    ///
    /// # Errors
    ///
    /// Returns [`DiagnosticReportLimitError`] if any limit is zero or output
    /// record count exceeds the bounded candidate count.
    pub const fn new(
        candidate_limit: usize,
        max_records: usize,
        max_bytes: usize,
        max_value_bytes: usize,
    ) -> Result<Self, DiagnosticReportLimitError> {
        if candidate_limit == 0 {
            Err(DiagnosticReportLimitError::ZeroCandidates)
        } else if max_records == 0 {
            Err(DiagnosticReportLimitError::ZeroRecords)
        } else if max_records > candidate_limit {
            Err(DiagnosticReportLimitError::RecordsExceedCandidates)
        } else if max_bytes == 0 {
            Err(DiagnosticReportLimitError::ZeroBytes)
        } else if max_value_bytes == 0 {
            Err(DiagnosticReportLimitError::ZeroValueBytes)
        } else {
            Ok(Self {
                candidate_limit,
                max_records,
                max_bytes,
                max_value_bytes,
            })
        }
    }
}

/// Invalid diagnostic report limits.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum DiagnosticReportLimitError {
    /// Candidate cap was zero.
    #[error("report candidate limit must be positive")]
    ZeroCandidates,
    /// Output record cap was zero.
    #[error("report record limit must be positive")]
    ZeroRecords,
    /// Output cap exceeded the preflight candidate cap.
    #[error("report record limit cannot exceed candidate limit")]
    RecordsExceedCandidates,
    /// Total report byte cap was zero.
    #[error("report byte limit must be positive")]
    ZeroBytes,
    /// Per-value byte cap was zero.
    #[error("report value byte limit must be positive")]
    ZeroValueBytes,
}

/// Builds a redacted report and truncates only a canonical-order suffix.
///
/// Redaction occurs before serializing candidate values. Secret values are
/// never encoded by this function. Callers still preview the returned bytes
/// before copy/write and this function never uploads anything.
///
/// # Errors
///
/// Returns [`DiagnosticReportError`] for candidate/input caps, duplicate keys,
/// oversized public values, an envelope too small for even zero records, or
/// canonical serialization failure.
pub fn build_diagnostic_report(
    inputs: impl IntoIterator<Item = DiagnosticReportInputV1>,
    consent: &ReportConsentV1,
    limits: DiagnosticReportLimits,
) -> Result<DiagnosticReportV1, DiagnosticReportError> {
    let mut records = Vec::new();
    for input in inputs {
        if input.sensitivity == ReportSensitivityV1::Secret {
            return Err(DiagnosticReportError::SecretInputRejected { key: input.key });
        }
        if records.len() == limits.candidate_limit {
            return Err(DiagnosticReportError::CandidateLimitExceeded {
                maximum: limits.candidate_limit,
            });
        }
        let include = consent.allows(input.sensitivity);
        let value = if include {
            let encoded = canonical_json_bytes(&input.value)
                .map_err(|source| DiagnosticReportError::CanonicalEncoding { source })?;
            if encoded.len() > limits.max_value_bytes {
                return Err(DiagnosticReportError::ValueLimitExceeded {
                    key: input.key,
                    observed: encoded.len(),
                    maximum: limits.max_value_bytes,
                });
            }
            ReportValueV1::Present(input.value)
        } else {
            ReportValueV1::Redacted
        };
        records.push(DiagnosticReportRecordV1 {
            key: input.key,
            value,
        });
    }
    records.sort_by(|left, right| left.key.cmp(&right.key));
    if let Some(pair) = records.windows(2).find(|pair| pair[0].key == pair[1].key) {
        return Err(DiagnosticReportError::DuplicateKey {
            key: pair[0].key.clone(),
        });
    }

    let total = records.len();
    let count_limited = total.min(limits.max_records);
    let applied = AppliedReportLimitsV1 {
        max_records: limits.max_records,
        max_bytes: limits.max_bytes,
    };
    let mut low = 0_usize;
    let mut high = count_limited;
    while low < high {
        let middle = low + (high - low).div_ceil(2);
        let candidate = DiagnosticReportV1 {
            schema_version: 1,
            records: records[..middle].to_vec(),
            truncated_records: total - middle,
            limits: applied,
        };
        let bytes = canonical_json_bytes(&candidate)
            .map_err(|source| DiagnosticReportError::CanonicalEncoding { source })?;
        if bytes.len() <= limits.max_bytes {
            low = middle;
        } else {
            high = middle - 1;
        }
    }
    let report = DiagnosticReportV1 {
        schema_version: 1,
        records: records[..low].to_vec(),
        truncated_records: total - low,
        limits: applied,
    };
    let bytes = canonical_json_bytes(&report)
        .map_err(|source| DiagnosticReportError::CanonicalEncoding { source })?;
    if bytes.len() > limits.max_bytes {
        return Err(DiagnosticReportError::EnvelopeExceedsLimit {
            observed: bytes.len(),
            maximum: limits.max_bytes,
        });
    }
    Ok(report)
}

/// Invalid diagnostic report input or bound.
#[derive(Debug, Error)]
pub enum DiagnosticReportError {
    /// Secret material is not part of the ordinary diagnostic report model.
    #[error("secret report input `{key:?}` was rejected before serialization")]
    SecretInputRejected {
        /// Rejected record key.
        key: ReportRecordKeyV1,
    },
    /// Input exceeded the preflight candidate cap.
    #[error("diagnostic report exceeds {maximum} candidate records")]
    CandidateLimitExceeded {
        /// Configured maximum.
        maximum: usize,
    },
    /// Two candidate rows shared one canonical key.
    #[error("diagnostic report repeats key `{key:?}`")]
    DuplicateKey {
        /// Repeated key.
        key: ReportRecordKeyV1,
    },
    /// One included public value exceeded its independent byte cap.
    #[error("report value `{key:?}` has {observed} bytes; maximum is {maximum}")]
    ValueLimitExceeded {
        /// Oversized record key.
        key: ReportRecordKeyV1,
        /// Observed canonical value bytes.
        observed: usize,
        /// Per-value maximum.
        maximum: usize,
    },
    /// Even an empty report envelope exceeded the total byte cap.
    #[error("empty report envelope has {observed} bytes; maximum is {maximum}")]
    EnvelopeExceedsLimit {
        /// Observed canonical bytes.
        observed: usize,
        /// Total maximum.
        maximum: usize,
    },
    /// Canonical serialization failed.
    #[error("failed to encode diagnostic report: {source}")]
    CanonicalEncoding {
        /// Serialization error.
        #[source]
        source: CanonicalJsonError,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use latticeaxiom_compose::{
        CostClass, DebugVisualizerSpec, DiagnosticMetricSpec, InfoItemSpec, MetricAggregation,
        VisualizerBudget, VisualizerDepthMode,
    };
    use latticeaxiom_core::PackageName;
    use serde_json::json;

    fn id(value: &str) -> StableId {
        match value.parse() {
            Ok(value) => value,
            Err(error) => panic!("invalid stable-ID fixture `{value}`: {error}"),
        }
    }

    fn schema(value: &str) -> SchemaId {
        match value.parse() {
            Ok(value) => value,
            Err(error) => panic!("invalid schema fixture `{value}`: {error}"),
        }
    }

    fn package(value: &str) -> PackageName {
        match value.parse() {
            Ok(value) => value,
            Err(error) => panic!("invalid package fixture `{value}`: {error}"),
        }
    }

    fn limits() -> SampleBatchLimits {
        match SampleBatchLimits::new(4, 4_096, 4, 128) {
            Ok(value) => value,
            Err(error) => panic!("invalid batch limits fixture: {error}"),
        }
    }

    fn request(entries: Vec<SampleRequestEntryV1>) -> SampleRequestBatchV1 {
        match SampleRequestBatchV1::new(
            SampleRequestContextV1 {
                engine_epoch: EngineEpoch::new(1),
                world_epoch: Some(WorldEpoch::new(2)),
                target_epoch: Some(TargetEpoch::new(3)),
                generation: SubscriptionGeneration::new(4),
                deadline: MonotonicNanos::new(1_000),
            },
            entries,
            4_096,
            limits(),
        ) {
            Ok(value) => value,
            Err(error) => panic!("invalid request fixture: {error}"),
        }
    }

    fn host() -> HostSampleContext {
        HostSampleContext {
            engine_epoch: EngineEpoch::new(1),
            world_epoch: Some(WorldEpoch::new(2)),
            target_epoch: Some(TargetEpoch::new(3)),
            generation: SubscriptionGeneration::new(4),
            fixed_tick: FixedTick::new(100),
            monotonic: MonotonicNanos::new(1_000),
        }
    }

    fn response(rows: Vec<RawSampleRowV1>) -> SampleBatchV1 {
        let value = SampleBatchV1 {
            engine_epoch: EngineEpoch::new(1),
            world_epoch: Some(WorldEpoch::new(2)),
            target_epoch: Some(TargetEpoch::new(3)),
            generation: SubscriptionGeneration::new(4),
            rows,
            declared_bytes: 0,
        };
        match value.with_measured_bytes() {
            Ok(value) => value,
            Err(error) => panic!("failed to measure response fixture: {error}"),
        }
    }

    #[test]
    fn catalog_enforces_rate_history_and_visualizer_boundaries() {
        let metric_id = id("example:metric/frame-time");
        let visualizer_id = id("example:visualizer/chunks");
        let catalog = ObservabilityCatalog {
            info_items: BTreeMap::from([(
                id("example:info/fps"),
                InfoItemSpec {
                    id: id("example:info/fps"),
                    declared_by: package("@example/diagnostics"),
                    label_key: "fps".to_owned(),
                    unit: Some("hz".to_owned()),
                    value_schema: schema("example:schema/fps@1"),
                    disclosure: latticeaxiom_compose::DisclosureLevel::Basic,
                    update: UpdatePolicy::FixedInterval(MIN_OBSERVABILITY_INTERVAL_MS),
                    cost: CostClass::Trivial,
                    callback: id("example:callback/sample"),
                },
            )]),
            metrics: BTreeMap::from([(
                metric_id.clone(),
                DiagnosticMetricSpec {
                    id: metric_id.clone(),
                    declared_by: package("@example/diagnostics"),
                    unit: "ms".to_owned(),
                    aggregation: MetricAggregation::P95,
                    sampling_interval_ms: MIN_OBSERVABILITY_INTERVAL_MS,
                    history_limit: 1,
                    cost: CostClass::Trivial,
                },
            )]),
            inspect: BTreeMap::new(),
            visualizers: BTreeMap::from([(
                visualizer_id.clone(),
                DebugVisualizerSpec {
                    id: visualizer_id.clone(),
                    declared_by: package("@example/diagnostics"),
                    data_source: id("example:data-source/chunks"),
                    legend: BTreeSet::from(["active".to_owned()]),
                    depth_mode: VisualizerDepthMode::DepthTested,
                    pick_support: true,
                    budget: VisualizerBudget {
                        radius: MAX_CHUNK_VISUALIZER_RADIUS,
                        primitives: MAX_VISUALIZER_PRIMITIVES,
                        upload_bytes: MAX_VISUALIZER_UPLOAD_BYTES,
                        history: 1,
                    },
                },
            )]),
        };
        let runtime = ObservabilityRuntimePolicies {
            metrics: BTreeMap::from([(metric_id, MetricHistoryPolicy { max_bytes: 1 })]),
            visualizers: BTreeMap::from([(
                visualizer_id.clone(),
                VisualizerHostPolicy {
                    radius_unit: VisualizerRadiusUnit::Chunks,
                    update_interval_ms: MIN_OBSERVABILITY_INTERVAL_MS,
                    max_history_bytes: 1,
                },
            )]),
        };
        assert!(
            ValidatedObservabilityCatalog::compile(
                catalog.clone(),
                &runtime,
                ObservabilityCatalogPolicy::default()
            )
            .is_ok()
        );

        let mut too_many = catalog;
        if let Some(visualizer) = too_many.visualizers.get_mut(&visualizer_id) {
            visualizer.budget.primitives = MAX_VISUALIZER_PRIMITIVES + 1;
        }
        assert!(matches!(
            ValidatedObservabilityCatalog::compile(
                too_many,
                &runtime,
                ObservabilityCatalogPolicy::default()
            ),
            Err(ObservabilityCatalogError::VisualizerPrimitiveLimitExceeded { .. })
        ));
    }

    #[test]
    fn last_unsubscribe_cancels_and_retires_generation_with_zero_work() {
        let target = SubscriptionTarget::new(id("example:callback/sample"), SampleKey::new(7));
        let mut planner = SubscriptionPlanner::default();
        let first = match planner.subscribe(
            target.clone(),
            SubscriptionConsumerId::new(1),
            SubscriptionOrigin::VisibleLayout,
        ) {
            Ok(value) => value,
            Err(error) => panic!("first subscription failed: {error}"),
        };
        let duplicate = match planner.subscribe(
            target.clone(),
            SubscriptionConsumerId::new(1),
            SubscriptionOrigin::VisibleLayout,
        ) {
            Ok(value) => value,
            Err(error) => panic!("duplicate subscription failed: {error}"),
        };
        assert_eq!(first.reference_count, 1);
        assert_eq!(duplicate.reference_count, 1);
        let generation = planner.begin_request(&target, SampleRequestId::new(9)).ok();
        assert_eq!(generation, Some(SubscriptionGeneration::INITIAL));

        let stopped = planner.unsubscribe(&target, SubscriptionConsumerId::new(1));
        let stopped = match stopped {
            Ok(value) => value,
            Err(error) => panic!("unsubscribe fixture failed: {error}"),
        };
        assert_eq!(stopped.reference_count, 0);
        assert!(!stopped.dedicated_work_enabled);
        assert!(stopped.cancellation.is_some());
        assert_eq!(planner.phase(&target), Some(SubscriptionPhase::Cancelling));
        assert_eq!(
            planner.complete_request(
                &target,
                SubscriptionGeneration::INITIAL,
                SampleRequestId::new(9)
            ),
            Ok(RequestCompletion::DiscardedRetiredGeneration)
        );
    }

    #[test]
    fn subscription_bounds_and_generation_overflow_fail_closed() {
        let limits = match SubscriptionPlannerLimits::new(1, 1, 1) {
            Ok(value) => value,
            Err(error) => panic!("invalid planner limits: {error}"),
        };
        let target = SubscriptionTarget::new(id("example:callback/one"), SampleKey::new(1));
        let other = SubscriptionTarget::new(id("example:callback/two"), SampleKey::new(2));
        let mut planner = SubscriptionPlanner::new(limits);
        assert!(
            planner
                .subscribe(
                    target.clone(),
                    SubscriptionConsumerId::new(1),
                    SubscriptionOrigin::PinnedItem,
                )
                .is_ok()
        );
        assert!(
            planner
                .subscribe(
                    target.clone(),
                    SubscriptionConsumerId::new(1),
                    SubscriptionOrigin::PinnedItem,
                )
                .is_ok()
        );
        assert!(matches!(
            planner.subscribe(
                target.clone(),
                SubscriptionConsumerId::new(2),
                SubscriptionOrigin::ExplicitReport,
            ),
            Err(SubscriptionError::ConsumerLimitExceeded { .. })
        ));
        assert!(matches!(
            planner.subscribe(
                other,
                SubscriptionConsumerId::new(1),
                SubscriptionOrigin::PinnedItem,
            ),
            Err(SubscriptionError::TargetLimitExceeded { .. })
        ));

        assert!(
            planner
                .begin_request(&target, SampleRequestId::new(10))
                .is_ok()
        );
        assert!(planner.invalidate(&target).is_ok());
        assert!(
            planner
                .begin_request(&target, SampleRequestId::new(11))
                .is_ok()
        );
        assert!(planner.invalidate(&target).is_ok());
        assert_eq!(
            planner
                .entries
                .get(&target)
                .map(|entry| entry.pending_cancellations.len()),
            Some(1)
        );

        if let Some(entry) = planner.entries.get_mut(&target) {
            entry.generation = SubscriptionGeneration::new(u64::MAX);
        }
        assert_eq!(
            planner.invalidate(&target),
            Err(SubscriptionError::GenerationOverflow {
                generation: SubscriptionGeneration::new(u64::MAX),
            })
        );
        assert_eq!(
            planner.generation(&target),
            Some(SubscriptionGeneration::new(u64::MAX))
        );
    }

    #[test]
    fn old_generation_batch_is_discarded_not_stale() {
        let entry = SampleRequestEntryV1 {
            key: SampleKey::new(1),
            expected_schema: schema("example:schema/value@1"),
            freshness: FreshnessPolicy::default(),
        };
        let request = request(vec![entry]);
        let mut response = response(vec![RawSampleRowV1 {
            key: SampleKey::new(1),
            state: RawSampleStateV1::Unavailable {
                reason: UnavailableReasonV1::NoTarget,
            },
        }]);
        response.generation = SubscriptionGeneration::new(3);
        let result = validate_sample_batch(&request, response, host(), limits());
        assert_eq!(
            result.ok(),
            Some(SampleBatchDisposition::Discarded(
                SampleDiscardReason::SubscriptionGeneration
            ))
        );
    }

    #[test]
    fn value_unavailable_error_and_host_freshness_are_distinct() {
        let value_schema = schema("example:schema/value@1");
        let entries = vec![
            SampleRequestEntryV1 {
                key: SampleKey::new(1),
                expected_schema: value_schema.clone(),
                freshness: FreshnessPolicy {
                    max_tick_age: Some(2),
                    max_monotonic_age_ns: None,
                    minimum_source_revision: None,
                },
            },
            SampleRequestEntryV1 {
                key: SampleKey::new(2),
                expected_schema: value_schema.clone(),
                freshness: FreshnessPolicy::default(),
            },
            SampleRequestEntryV1 {
                key: SampleKey::new(3),
                expected_schema: value_schema.clone(),
                freshness: FreshnessPolicy::default(),
            },
        ];
        let request = request(entries);
        let response = response(vec![
            RawSampleRowV1 {
                key: SampleKey::new(1),
                state: RawSampleStateV1::Value {
                    value: TypedSampleValueV1 {
                        schema: value_schema,
                        value: json!(42),
                    },
                    observed_fixed_tick: FixedTick::new(97),
                    observed_monotonic: MonotonicNanos::new(999),
                    source_revision: 1,
                },
            },
            RawSampleRowV1 {
                key: SampleKey::new(2),
                state: RawSampleStateV1::Unavailable {
                    reason: UnavailableReasonV1::NoWorld,
                },
            },
            RawSampleRowV1 {
                key: SampleKey::new(3),
                state: RawSampleStateV1::Error {
                    code: id("example:diagnostic/source-failed"),
                    arguments: BTreeMap::from([(
                        "attempt".to_owned(),
                        DiagnosticArgumentV1::Unsigned(2),
                    )]),
                    retryability: RetryabilityV1::Retryable,
                },
            },
        ]);
        let result = validate_sample_batch(&request, response, host(), limits());
        let rows = match result {
            Ok(SampleBatchDisposition::Accepted(batch)) => batch.rows,
            Ok(other) => panic!("expected accepted batch, got {other:?}"),
            Err(error) => panic!("sample validation failed: {error}"),
        };
        assert!(matches!(
            rows[0].state,
            HostSampleStateV1::Value {
                freshness: SampleFreshness::Stale,
                ..
            }
        ));
        assert!(matches!(
            rows[1].state,
            HostSampleStateV1::Unavailable { .. }
        ));
        assert!(matches!(rows[2].state, HostSampleStateV1::Error { .. }));
    }

    #[test]
    fn response_rejects_omission_order_schema_and_byte_boundary_plus_one() {
        let expected_schema = schema("example:schema/value@1");
        let request = request(vec![SampleRequestEntryV1 {
            key: SampleKey::new(1),
            expected_schema: expected_schema.clone(),
            freshness: FreshnessPolicy::default(),
        }]);
        let omitted = response(Vec::new());
        assert!(matches!(
            validate_sample_batch(&request, omitted, host(), limits()),
            Err(SampleProtocolError::KeySetSizeMismatch { .. })
        ));

        let wrong_schema = response(vec![RawSampleRowV1 {
            key: SampleKey::new(1),
            state: RawSampleStateV1::Value {
                value: TypedSampleValueV1 {
                    schema: schema("example:schema/other@1"),
                    value: json!(1),
                },
                observed_fixed_tick: FixedTick::new(100),
                observed_monotonic: MonotonicNanos::new(1_000),
                source_revision: 1,
            },
        }]);
        assert!(matches!(
            validate_sample_batch(&request, wrong_schema, host(), limits()),
            Err(SampleProtocolError::WrongValueSchema { .. })
        ));

        let valid = response(vec![RawSampleRowV1 {
            key: SampleKey::new(1),
            state: RawSampleStateV1::Value {
                value: TypedSampleValueV1 {
                    schema: expected_schema,
                    value: json!(1),
                },
                observed_fixed_tick: FixedTick::new(100),
                observed_monotonic: MonotonicNanos::new(1_000),
                source_revision: 1,
            },
        }]);
        let exact = match SampleBatchLimits::new(4, valid.declared_bytes, 4, 128) {
            Ok(value) => value,
            Err(error) => panic!("invalid exact limits: {error}"),
        };
        let exact_request = match SampleRequestBatchV1::new(
            SampleRequestContextV1 {
                engine_epoch: EngineEpoch::new(1),
                world_epoch: Some(WorldEpoch::new(2)),
                target_epoch: Some(TargetEpoch::new(3)),
                generation: SubscriptionGeneration::new(4),
                deadline: MonotonicNanos::new(1_000),
            },
            [SampleRequestEntryV1 {
                key: SampleKey::new(1),
                expected_schema: schema("example:schema/value@1"),
                freshness: FreshnessPolicy::default(),
            }],
            valid.declared_bytes,
            exact,
        ) {
            Ok(value) => value,
            Err(error) => panic!("exact request failed: {error}"),
        };
        assert!(validate_sample_batch(&exact_request, valid.clone(), host(), exact).is_ok());
        let below = match SampleBatchLimits::new(4, valid.declared_bytes - 1, 4, 128) {
            Ok(value) => value,
            Err(error) => panic!("invalid below limits: {error}"),
        };
        let mut below_request = exact_request;
        below_request.byte_budget = valid.declared_bytes - 1;
        assert!(matches!(
            validate_sample_batch(&below_request, valid, host(), below),
            Err(SampleProtocolError::ResponseByteLimitExceeded { .. })
        ));
    }

    #[test]
    fn request_key_cap_accepts_boundary_and_rejects_plus_one() {
        let bounded = match SampleBatchLimits::new(2, 4_096, 4, 128) {
            Ok(value) => value,
            Err(error) => panic!("invalid bounded limits: {error}"),
        };
        let context = SampleRequestContextV1 {
            engine_epoch: EngineEpoch::new(1),
            world_epoch: Some(WorldEpoch::new(2)),
            target_epoch: Some(TargetEpoch::new(3)),
            generation: SubscriptionGeneration::new(4),
            deadline: MonotonicNanos::new(1_000),
        };
        let entry = |key| SampleRequestEntryV1 {
            key: SampleKey::new(key),
            expected_schema: schema("example:schema/value@1"),
            freshness: FreshnessPolicy::default(),
        };
        assert!(SampleRequestBatchV1::new(context, [entry(1), entry(2)], 4_096, bounded,).is_ok());
        assert!(matches!(
            SampleRequestBatchV1::new(context, [entry(1), entry(2), entry(3)], 4_096, bounded,),
            Err(SampleProtocolError::KeyLimitExceeded {
                observed: 3,
                maximum: 2,
            })
        ));
    }

    fn report_input(
        path: &str,
        value: Value,
        sensitivity: ReportSensitivityV1,
    ) -> DiagnosticReportInputV1 {
        DiagnosticReportInputV1 {
            key: ReportRecordKeyV1 {
                section: ReportSectionV1::Metric,
                id: id(&format!("example:report/{path}")),
            },
            value,
            sensitivity,
        }
    }

    #[test]
    fn report_is_permutation_stable_redacted_and_deterministically_truncated() {
        let public = report_input("a", json!(1), ReportSensitivityV1::Public);
        let path = report_input(
            "b",
            json!("D:/private/world"),
            ReportSensitivityV1::PrivatePath,
        );
        let world_identity =
            report_input("c", json!("world-id"), ReportSensitivityV1::WorldIdentity);
        let limits = match DiagnosticReportLimits::new(8, 2, 4_096, 128) {
            Ok(value) => value,
            Err(error) => panic!("invalid report limits: {error}"),
        };
        let forward = build_diagnostic_report(
            [public.clone(), path.clone(), world_identity.clone()],
            &ReportConsentV1::default(),
            limits,
        );
        let reverse = build_diagnostic_report(
            [world_identity, path, public],
            &ReportConsentV1::default(),
            limits,
        );
        let forward = match forward {
            Ok(value) => value,
            Err(error) => panic!("report fixture failed: {error}"),
        };
        let reverse = match reverse {
            Ok(value) => value,
            Err(error) => panic!("report fixture failed: {error}"),
        };
        assert_eq!(
            forward.canonical_bytes().ok(),
            reverse.canonical_bytes().ok()
        );
        assert_eq!(forward.records.len(), 2);
        assert_eq!(forward.truncated_records, 1);
        assert!(matches!(forward.records[1].value, ReportValueV1::Redacted));
    }

    #[test]
    fn report_canonical_bytes_match_golden_contract() {
        let limits = match DiagnosticReportLimits::new(1, 1, 4_096, 128) {
            Ok(value) => value,
            Err(error) => panic!("invalid report limits: {error}"),
        };
        let report = match build_diagnostic_report(
            [report_input("a", json!(1), ReportSensitivityV1::Public)],
            &ReportConsentV1::default(),
            limits,
        ) {
            Ok(value) => value,
            Err(error) => panic!("golden report failed: {error}"),
        };
        let bytes = match report.canonical_bytes() {
            Ok(value) => value,
            Err(error) => panic!("golden encoding failed: {error}"),
        };
        assert_eq!(
            bytes,
            br#"{"limits":{"max_bytes":4096,"max_records":1},"records":[{"key":{"id":"example:report/a","section":"metric"},"value":{"state":"present","value":1}}],"schema_version":1,"truncated_records":0}"#
        );
    }

    #[test]
    fn secret_report_input_is_rejected_before_ordinary_value_handling() {
        let limits = match DiagnosticReportLimits::new(1, 1, 1_024, 128) {
            Ok(value) => value,
            Err(error) => panic!("invalid report limits: {error}"),
        };
        let result = build_diagnostic_report(
            [report_input(
                "secret",
                json!("token"),
                ReportSensitivityV1::Secret,
            )],
            &ReportConsentV1::default(),
            limits,
        );
        assert!(matches!(
            result,
            Err(DiagnosticReportError::SecretInputRejected { .. })
        ));
    }

    #[test]
    fn report_byte_cap_keeps_a_stable_prefix_and_records_actual_limit() {
        let inputs = [
            report_input("a", json!("small"), ReportSensitivityV1::Public),
            report_input("b", json!("larger-value"), ReportSensitivityV1::Public),
        ];
        let roomy = match DiagnosticReportLimits::new(2, 2, 4_096, 128) {
            Ok(value) => value,
            Err(error) => panic!("invalid report limits: {error}"),
        };
        let full = match build_diagnostic_report(inputs.clone(), &ReportConsentV1::default(), roomy)
        {
            Ok(value) => value,
            Err(error) => panic!("full report failed: {error}"),
        };
        let one = DiagnosticReportV1 {
            schema_version: 1,
            records: full.records[..1].to_vec(),
            truncated_records: 1,
            limits: AppliedReportLimitsV1 {
                max_records: 2,
                max_bytes: 4_096,
            },
        };
        let one_bytes = one.canonical_bytes().map_or(0, |bytes| bytes.len());
        let tight = match DiagnosticReportLimits::new(2, 2, one_bytes, 128) {
            Ok(value) => value,
            Err(error) => panic!("invalid tight limits: {error}"),
        };
        let truncated = match build_diagnostic_report(inputs, &ReportConsentV1::default(), tight) {
            Ok(value) => value,
            Err(error) => panic!("tight report failed: {error}"),
        };
        assert!(truncated.records.len() <= 1);
        assert!(truncated.truncated_records >= 1);
        assert!(
            truncated
                .canonical_bytes()
                .is_ok_and(|bytes| bytes.len() <= one_bytes)
        );
    }

    #[test]
    fn report_and_sample_dtos_deny_unknown_fields() {
        let report = r#"{
            "schema_version":1,
            "records":[],
            "truncated_records":0,
            "limits":{"max_records":1,"max_bytes":128},
            "surprise":true
        }"#;
        assert!(serde_json::from_str::<DiagnosticReportV1>(report).is_err());
        let batch = r#"{
            "engine_epoch":1,
            "world_epoch":null,
            "target_epoch":null,
            "generation":1,
            "rows":[],
            "declared_bytes":2,
            "surprise":true
        }"#;
        assert!(serde_json::from_str::<SampleBatchV1>(batch).is_err());
    }
}
