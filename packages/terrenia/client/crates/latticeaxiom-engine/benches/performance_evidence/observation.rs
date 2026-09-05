//! Machine-readable observation artifact emitted by the harness.

use serde::Serialize;

use crate::histogram::HistogramV1;

/// Schema identity written into every observation file.
pub const OBSERVATION_SCHEMA: &str = "latticeaxiom:schema/performance-observation-v1";
/// Observation schema major.
pub const OBSERVATION_SCHEMA_VERSION: u32 = 1;
/// Measurement tool identity.
pub const MEASUREMENT_TOOL: &str = "latticeaxiom-performance-evidence/v1";
/// Histogram algorithm identity frozen by ADR 0026 section 10.
pub const HISTOGRAM_VERSION: &str = "nearest-rank-v1";

/// Bounded harness mode. Traversal is the 2 + 10 minute protocol; smoke is shorter.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum HarnessMode {
    /// Local verification: abbreviated ticks, not a D10 gate.
    Smoke,
    /// Bounded 7,200 warmup + 36,000 measure ticks at 60 Hz.
    Traversal,
}

impl HarnessMode {
    /// Parses a CLI mode token.
    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "smoke" => Some(Self::Smoke),
            "traversal" => Some(Self::Traversal),
            _ => None,
        }
    }

    /// Stable kebab-case name.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Smoke => "smoke",
            Self::Traversal => "traversal",
        }
    }
}

/// Headless observation payload. This is not a normative pass report.
#[derive(Clone, Debug, Serialize)]
pub struct PerformanceObservationV1 {
    /// Schema identifier.
    pub schema: &'static str,
    /// Schema major.
    pub schema_version: u32,
    /// Smoke or bounded 10-minute traversal.
    pub mode: HarnessMode,
    /// Measurement scenario. Headless cannot declare a render-budget pass.
    pub scenario: &'static str,
    /// Tool identity.
    pub measurement_tool: &'static str,
    /// Histogram algorithm.
    pub histogram_version: &'static str,
    /// Tick protocol actually executed.
    pub protocol: ProtocolObservationV1,
    /// Lock and seed fingerprints captured from the reopened host.
    pub fingerprints: ObservationFingerprintsV1,
    /// Live streaming-profile receipt copied from the production host.
    pub streaming_profile: serde_json::Value,
    /// Magnitude histograms for elapsed samples.
    pub histograms: ObservationHistogramsV1,
    /// Working-set and queue high-water marks.
    pub high_waters: ObservationHighWatersV1,
    /// Cancellation, stale, and backpressure counters.
    pub queues: ObservationQueuesV1,
    /// Bounded path coverage.
    pub path: PathObservationV1,
    /// Harness faults.
    pub faults: ObservationFaultsV1,
}

/// Tick and wall-clock bounds for one run.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct ProtocolObservationV1 {
    /// Planned warmup ticks.
    pub warmup_ticks: u32,
    /// Planned measure ticks.
    pub measure_ticks: u32,
    /// Completed warmup ticks.
    pub completed_warmup_ticks: u32,
    /// Completed measure ticks.
    pub completed_measure_ticks: u32,
    /// Authoritative fixed rate.
    pub fixed_hz: u32,
    /// Wall-clock safety cap.
    pub max_wall_seconds: u32,
    /// Observed wall-clock milliseconds of the whole run.
    pub wall_millis: u64,
    /// True when the wall-clock cap stopped the run early.
    pub truncated_by_wall_cap: bool,
}

/// Identities rebound from the reopened lock.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ObservationFingerprintsV1 {
    /// Sealed product-lock hash.
    pub product_lock_hash: String,
    /// Registration semantic hash from the lock-verified images.
    pub registration_semantic_hash: String,
    /// Registration image hash from the lock.
    pub registration_image_hash: String,
    /// Exact `EngineBuildId`, when the lock records one.
    pub engine_build_id: Option<String>,
    /// SHA-256 of the lock file bytes.
    pub lock_file_sha256: String,
    /// SHA-256 of `run/shell/latticeaxiom.lock` when present.
    pub shell_lock_file_sha256: Option<String>,
    /// Selected realization target triple.
    pub realization_target: String,
    /// Exact hexadecimal 32-byte seed used by the production spine.
    pub world_seed: String,
}

/// Histogram set required by ADR 0026 section 10.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct ObservationHistogramsV1 {
    /// Wall duration of one headless `advance_fixed_ticks(1)` call.
    pub fixed_update_cpu_ns: HistogramV1,
    /// Resident chunk occupancy per measured tick.
    pub resident_chunks: HistogramV1,
    /// Active chunk occupancy per measured tick.
    pub active_chunks: HistogramV1,
    /// Visible chunk occupancy per measured tick.
    pub visible_chunks: HistogramV1,
    /// Combined mesh+collider in-flight jobs per measured tick.
    pub in_flight_chunks: HistogramV1,
    /// Combined reserved derived bytes per measured tick.
    pub reserved_bytes: HistogramV1,
}

/// Peak occupancy copied from live diagnostics.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct ObservationHighWatersV1 {
    /// Peak resident chunks.
    pub resident_chunks: u32,
    /// Peak active chunks.
    pub active_chunks: u32,
    /// Peak visible chunks.
    pub visible_chunks: u32,
    /// Peak requested/prefetch set.
    pub requested_chunks: u32,
    /// Peak in-flight derived chunks.
    pub in_flight_chunks: u32,
    /// Peak pending+in-flight mesh jobs.
    pub mesh_jobs: u64,
    /// Peak pending+in-flight collider jobs.
    pub collider_jobs: u64,
    /// Peak combined derived jobs.
    pub combined_jobs: u64,
    /// Peak combined reserved bytes.
    pub combined_reserved_bytes: u64,
    /// Process RSS when the harness can observe it; omitted otherwise.
    pub ram_bytes: Option<u64>,
}

/// Queue cancellation and backpressure counters.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize)]
pub struct ObservationQueuesV1 {
    /// Combined mesh+collider cancellation requests.
    pub cancel_requests: u64,
    /// Apply slices stopped by the job cap.
    pub apply_stopped_jobs: u64,
    /// Apply slices stopped by the byte cap.
    pub apply_stopped_bytes: u64,
    /// Apply slices stopped by the wall-clock cap.
    pub apply_stopped_wall_clock: u64,
}

/// Fixed-path coverage for the bounded traversal.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct PathObservationV1 {
    /// Spawn translation in millimetres.
    pub start_translation_mm: [i64; 3],
    /// Final translation in millimetres.
    pub end_translation_mm: [i64; 3],
    /// Peak Chebyshev XZ displacement from spawn, in millimetres.
    pub max_horizontal_displacement_mm: i64,
    /// Distinct player chunks visited.
    pub unique_player_chunks: u64,
    /// Whether a π-radian look was enqueued.
    pub turn_180_executed: bool,
    /// Break-action frames enqueued.
    pub edit_attempts: u64,
    /// Whether a reverse-walk phase ran after the outbound walk.
    pub returned_toward_start: bool,
}

/// Harness-level faults. A truncated run is not a silent pass.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize)]
pub struct ObservationFaultsV1 {
    /// True when the wall-clock cap fired.
    pub truncated: bool,
    /// True when planned measure ticks were not completed.
    pub incomplete_measure: bool,
}
