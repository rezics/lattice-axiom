//! Headless ADR 0026 performance observation harness.
//!
//! The binary records machine-readable histograms from the production
//! headless host started on a reopened frozen lock. It does not declare a
//! D10 freeze, a render-budget pass, or a three-run normative gate.
//! Invoke through `scripts/performance/performance_evidence.py`.

#![allow(
    missing_docs,
    reason = "the bench crate documents the public protocol in the crate rustdoc"
)]

mod boot;
mod histogram;
mod observation;
mod protocol;

use std::{
    collections::BTreeSet,
    env, fs,
    io::{self, Write},
    path::PathBuf,
    time::{Duration, Instant},
};

use bevy::math::Vec3Swizzles;
use boot::{BootedLock, boot_from_workspace};
use histogram::SampleSeries;
use latticeaxiom_compose::ProductLockError;
use latticeaxiom_engine::{
    ActionAxis2V1, ChunkCoordinate, DerivedQueueSnapshotV1, EngineInstance, PlayerActionButtonsV1,
    PlayerActionFrameV1, PlayerActionV1, ProductionSpine, STREAMING_PROFILE_EVIDENCE_SCHEMA_V1,
};
use latticeaxiom_launcher::ProductLockBootError;
use latticeaxiom_packages::CasError;
use observation::{
    HISTOGRAM_VERSION, HarnessMode, MEASUREMENT_TOOL, OBSERVATION_SCHEMA,
    OBSERVATION_SCHEMA_VERSION, ObservationFaultsV1, ObservationFingerprintsV1,
    ObservationHighWatersV1, ObservationHistogramsV1, ObservationQueuesV1, PathObservationV1,
    PerformanceObservationV1, ProtocolObservationV1,
};
use protocol::{ACTION_BATCH, FIXED_HZ, PathPhase, TickPlan, WARMUP_ADVANCE_BATCH, measure_phases};
use thiserror::Error;

fn main() {
    if let Err(error) = run() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), HarnessError> {
    let args = Args::parse(env::args().skip(1))?;
    let workspace = args
        .workspace
        .clone()
        .map_or_else(|| env::current_dir().map_err(HarnessError::CurrentDir), Ok)?;
    let booted = boot_from_workspace(&workspace, args.lock.as_deref())?;
    let observation = capture(&args, booted)?;
    let encoded = serde_json::to_vec_pretty(&observation)?;
    if let Some(parent) = args.output.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent).map_err(|source| HarnessError::Io {
            path: parent.to_path_buf(),
            source,
        })?;
    }
    fs::write(&args.output, &encoded).map_err(|source| HarnessError::Io {
        path: args.output.clone(),
        source,
    })?;
    writeln!(
        io::stderr(),
        "wrote {} ({} bytes, mode {}, measure ticks {}/{})",
        args.output.display(),
        encoded.len(),
        observation.mode.as_str(),
        observation.protocol.completed_measure_ticks,
        observation.protocol.measure_ticks
    )
    .map_err(HarnessError::Stderr)?;
    Ok(())
}

#[allow(
    clippy::too_many_lines,
    reason = "one capture path keeps warmup, measure, and evidence fields aligned"
)]
fn capture(args: &Args, booted: BootedLock) -> Result<PerformanceObservationV1, HarnessError> {
    let plan = args.plan;
    let BootedLock {
        images,
        lock_file_sha256,
        shell_lock_file_sha256,
        target,
        engine_build_id,
        registration_image_hash,
    } = booted;
    let product_lock_hash = images.product_lock_hash();
    let registration_semantic_hash = images.images().registration_semantic_hash();
    let timestep = Duration::from_nanos(1_000_000_000 / u64::from(FIXED_HZ));
    let mut instance = EngineInstance::new_headless_host_from_lock(images, timestep)?;
    let spine = instance
        .app()
        .world()
        .get_resource::<ProductionSpine>()
        .cloned()
        .ok_or(HarnessError::MissingSpine)?;
    let start_pose = spine.player_pose().translation;
    let start = millimetres(start_pose.x, start_pose.y, start_pose.z);
    let started_at = Instant::now();
    let max_wall = Duration::from_secs(u64::from(plan.max_wall_seconds));

    let mut generation = 1_u64;
    let mut completed_warmup = 0_u32;
    let mut truncated = false;
    generation = enqueue_walk(
        &mut instance,
        generation,
        0.0,
        plan.warmup_ticks.min(ACTION_BATCH),
    )?;
    while completed_warmup < plan.warmup_ticks {
        if started_at.elapsed() >= max_wall {
            truncated = true;
            break;
        }
        let remaining = plan.warmup_ticks.saturating_sub(completed_warmup);
        let step = remaining.min(WARMUP_ADVANCE_BATCH);
        instance.advance_fixed_ticks(step)?;
        completed_warmup = completed_warmup.saturating_add(step);
        if completed_warmup.is_multiple_of(ACTION_BATCH) && completed_warmup < plan.warmup_ticks {
            generation = enqueue_walk(
                &mut instance,
                generation,
                0.0,
                plan.warmup_ticks
                    .saturating_sub(completed_warmup)
                    .min(ACTION_BATCH),
            )?;
        }
    }

    let mut cpu = SampleSeries::default();
    let mut resident = SampleSeries::default();
    let mut active = SampleSeries::default();
    let mut visible = SampleSeries::default();
    let mut in_flight = SampleSeries::default();
    let mut reserved = SampleSeries::default();
    let mut high = HighWater::default();
    let mut unique_chunks = BTreeSet::new();
    let mut completed_measure = 0_u32;
    let mut turn_180 = false;
    let mut edit_attempts = 0_u64;
    let mut returned = false;
    let mut return_start_distance = None;
    let mut max_displacement_mm = 0_i64;

    if !truncated {
        for (phase, ticks) in measure_phases(plan.measure_ticks) {
            if ticks == 0 {
                continue;
            }
            match phase {
                PathPhase::WalkForward => {
                    generation =
                        enqueue_walk(&mut instance, generation, 1.0, ticks.min(ACTION_BATCH))?;
                }
                PathPhase::Turn180 => {
                    generation = enqueue_look(&mut instance, generation, std::f32::consts::PI)?;
                    generation = enqueue_walk(
                        &mut instance,
                        generation,
                        1.0,
                        ticks.saturating_sub(1).min(ACTION_BATCH),
                    )?;
                    turn_180 = true;
                }
                PathPhase::EditBurst => {
                    generation = enqueue_edits(&mut instance, generation, ticks.min(ACTION_BATCH))?;
                    edit_attempts =
                        edit_attempts.saturating_add(u64::from(ticks.min(ACTION_BATCH)));
                }
                PathPhase::Return => {
                    generation =
                        enqueue_walk(&mut instance, generation, 1.0, ticks.min(ACTION_BATCH))?;
                    return_start_distance = Some(
                        spine
                            .player_pose()
                            .translation
                            .xz()
                            .distance(start_pose.xz()),
                    );
                }
            }
            let mut remaining = ticks;
            let mut enqueued = ticks.min(ACTION_BATCH);
            while remaining > 0 {
                if started_at.elapsed() >= max_wall {
                    truncated = true;
                    break;
                }
                let tick_started = Instant::now();
                instance.advance_fixed_ticks(1)?;
                if phase == PathPhase::Return
                    && return_start_distance.is_some_and(|distance| {
                        spine
                            .player_pose()
                            .translation
                            .xz()
                            .distance(start_pose.xz())
                            + 0.25
                            < distance
                    })
                {
                    returned = true;
                }
                let elapsed_ns =
                    u64::try_from(tick_started.elapsed().as_nanos()).unwrap_or(u64::MAX);
                cpu.push(elapsed_ns);
                sample_live(
                    &spine,
                    &mut resident,
                    &mut active,
                    &mut visible,
                    &mut in_flight,
                    &mut reserved,
                    &mut high,
                    &mut unique_chunks,
                    &mut max_displacement_mm,
                    start,
                );
                completed_measure = completed_measure.saturating_add(1);
                remaining = remaining.saturating_sub(1);
                enqueued = enqueued.saturating_sub(1);
                if enqueued == 0 && remaining > 0 {
                    let batch = remaining.min(ACTION_BATCH);
                    generation = match phase {
                        PathPhase::EditBurst => enqueue_edits(&mut instance, generation, batch)?,
                        PathPhase::Return => enqueue_walk(&mut instance, generation, 1.0, batch)?,
                        PathPhase::WalkForward | PathPhase::Turn180 => {
                            enqueue_walk(&mut instance, generation, 1.0, batch)?
                        }
                    };
                    if phase == PathPhase::EditBurst {
                        edit_attempts = edit_attempts.saturating_add(u64::from(batch));
                    }
                    enqueued = batch;
                }
            }
            if truncated {
                break;
            }
        }
    }

    let end_pose = spine.player_pose().translation;
    let end = millimetres(end_pose.x, end_pose.y, end_pose.z);
    let queues = spine.derived_queue_snapshot();
    high.observe_queues(&queues);
    let streaming = spine
        .streaming_profile_evidence()
        .map_or(serde_json::Value::Null, |evidence| {
            serde_json::to_value(evidence).unwrap_or(serde_json::Value::Null)
        });
    if streaming != serde_json::Value::Null
        && streaming.get("schema").and_then(serde_json::Value::as_str)
            != Some(STREAMING_PROFILE_EVIDENCE_SCHEMA_V1)
    {
        return Err(HarnessError::StreamingSchema);
    }

    let wall_millis = u64::try_from(started_at.elapsed().as_millis()).unwrap_or(u64::MAX);
    let incomplete = completed_measure < plan.measure_ticks;
    Ok(PerformanceObservationV1 {
        schema: OBSERVATION_SCHEMA,
        schema_version: OBSERVATION_SCHEMA_VERSION,
        mode: args.mode,
        scenario: "headless-manual-time",
        measurement_tool: MEASUREMENT_TOOL,
        histogram_version: HISTOGRAM_VERSION,
        protocol: ProtocolObservationV1 {
            warmup_ticks: plan.warmup_ticks,
            measure_ticks: plan.measure_ticks,
            completed_warmup_ticks: completed_warmup,
            completed_measure_ticks: completed_measure,
            fixed_hz: FIXED_HZ,
            max_wall_seconds: plan.max_wall_seconds,
            wall_millis,
            truncated_by_wall_cap: truncated,
        },
        fingerprints: ObservationFingerprintsV1 {
            product_lock_hash: product_lock_hash.to_string(),
            registration_semantic_hash: registration_semantic_hash.to_string(),
            registration_image_hash: registration_image_hash.to_string(),
            engine_build_id: engine_build_id.map(|value| value.to_string()),
            lock_file_sha256: lock_file_sha256.to_string(),
            shell_lock_file_sha256: shell_lock_file_sha256.map(|value| value.to_string()),
            realization_target: target.to_string(),
            world_seed: spine.world_seed()?.to_string(),
        },
        streaming_profile: streaming,
        histograms: ObservationHistogramsV1 {
            fixed_update_cpu_ns: cpu.summarize(),
            resident_chunks: resident.summarize(),
            active_chunks: active.summarize(),
            visible_chunks: visible.summarize(),
            in_flight_chunks: in_flight.summarize(),
            reserved_bytes: reserved.summarize(),
        },
        high_waters: ObservationHighWatersV1 {
            resident_chunks: high.resident,
            active_chunks: high.active,
            visible_chunks: high.visible,
            requested_chunks: high.requested,
            in_flight_chunks: high.in_flight,
            mesh_jobs: high.mesh_jobs,
            collider_jobs: high.collider_jobs,
            combined_jobs: high.combined_jobs,
            combined_reserved_bytes: high.reserved_bytes,
            ram_bytes: None,
        },
        queues: ObservationQueuesV1 {
            cancel_requests: queues.cancel_requests,
            apply_stopped_jobs: queues.apply_stopped_jobs,
            apply_stopped_bytes: queues.apply_stopped_bytes,
            apply_stopped_wall_clock: queues.apply_stopped_wall_clock,
        },
        path: PathObservationV1 {
            start_translation_mm: start,
            end_translation_mm: end,
            max_horizontal_displacement_mm: max_displacement_mm,
            unique_player_chunks: u64::try_from(unique_chunks.len()).unwrap_or(u64::MAX),
            turn_180_executed: turn_180,
            edit_attempts,
            returned_toward_start: returned,
        },
        faults: ObservationFaultsV1 {
            truncated,
            incomplete_measure: incomplete,
        },
    })
}

#[allow(
    clippy::too_many_arguments,
    reason = "one sample records occupancy, path, and high-water together"
)]
fn sample_live(
    spine: &ProductionSpine,
    resident: &mut SampleSeries,
    active: &mut SampleSeries,
    visible: &mut SampleSeries,
    in_flight: &mut SampleSeries,
    reserved: &mut SampleSeries,
    high: &mut HighWater,
    unique_chunks: &mut BTreeSet<ChunkCoordinate>,
    max_displacement_mm: &mut i64,
    start: [i64; 3],
) {
    let diagnostics = spine.working_set_diagnostics();
    let queues = spine.derived_queue_snapshot();
    resident.push(u64::from(diagnostics.resident()));
    active.push(u64::from(diagnostics.active()));
    visible.push(u64::from(diagnostics.visible()));
    in_flight.push(u64::from(diagnostics.in_flight()));
    reserved.push(diagnostics.reserved_bytes());
    high.observe_diagnostics(
        diagnostics.resident(),
        diagnostics.active(),
        diagnostics.visible(),
        diagnostics.in_flight(),
    );
    high.observe_queues(&queues);
    if let Some(evidence) = spine.streaming_profile_evidence() {
        high.requested = high.requested.max(evidence.counts.requested);
    }
    let translation = spine.player_pose().translation;
    let pose = millimetres(translation.x, translation.y, translation.z);
    let dx = (pose[0] - start[0]).unsigned_abs();
    let dz = (pose[2] - start[2]).unsigned_abs();
    let chebyshev = i64::try_from(dx.max(dz)).unwrap_or(i64::MAX);
    *max_displacement_mm = (*max_displacement_mm).max(chebyshev);
    unique_chunks.insert(chunk_from_mm(pose, spine.chunk_edge()));
}

fn enqueue_walk(
    instance: &mut EngineInstance,
    start_generation: u64,
    forward: f32,
    ticks: u32,
) -> Result<u64, HarnessError> {
    if ticks == 0 {
        return Ok(start_generation);
    }
    let frames = (0..ticks).map(|offset| {
        let mut started = PlayerActionButtonsV1::empty();
        if forward != 0.0 && offset.is_multiple_of(18) {
            started.insert(PlayerActionV1::Jump);
        }
        PlayerActionFrameV1 {
            generation: start_generation + u64::from(offset),
            movement: ActionAxis2V1 { x: 0.0, y: forward },
            started,
            ..PlayerActionFrameV1::default()
        }
    });
    instance.enqueue_headless_actions(frames)?;
    Ok(start_generation.saturating_add(u64::from(ticks)))
}

fn enqueue_look(
    instance: &mut EngineInstance,
    start_generation: u64,
    yaw: f32,
) -> Result<u64, HarnessError> {
    instance.enqueue_headless_actions([PlayerActionFrameV1 {
        generation: start_generation,
        look_radians: ActionAxis2V1 { x: yaw, y: 0.0 },
        ..PlayerActionFrameV1::default()
    }])?;
    Ok(start_generation.saturating_add(1))
}

fn enqueue_edits(
    instance: &mut EngineInstance,
    start_generation: u64,
    ticks: u32,
) -> Result<u64, HarnessError> {
    if ticks == 0 {
        return Ok(start_generation);
    }
    let frames = (0..ticks).map(|offset| {
        let mut started = PlayerActionButtonsV1::empty();
        started.insert(PlayerActionV1::BreakBlock);
        PlayerActionFrameV1 {
            generation: start_generation + u64::from(offset),
            started,
            ..PlayerActionFrameV1::default()
        }
    });
    instance.enqueue_headless_actions(frames)?;
    Ok(start_generation.saturating_add(u64::from(ticks)))
}

fn millimetres(x: f32, y: f32, z: f32) -> [i64; 3] {
    [f32_to_mm(x), f32_to_mm(y), f32_to_mm(z)]
}

#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    reason = "millimetres are a display-only path receipt, not an authority value"
)]
fn f32_to_mm(value: f32) -> i64 {
    let scaled = value * 1000.0;
    if scaled.is_finite() {
        scaled.round() as i64
    } else {
        0
    }
}

fn chunk_from_mm(translation_mm: [i64; 3], edge: u16) -> ChunkCoordinate {
    let edge = i64::from(edge).max(1);
    let cell = |mm: i64| -> i32 {
        let voxels = mm.div_euclid(1000);
        i32::try_from(voxels.div_euclid(edge)).unwrap_or(i32::MAX)
    };
    ChunkCoordinate::new(
        cell(translation_mm[0]),
        cell(translation_mm[1]),
        cell(translation_mm[2]),
    )
}

#[derive(Clone, Copy, Debug, Default)]
struct HighWater {
    resident: u32,
    active: u32,
    visible: u32,
    requested: u32,
    in_flight: u32,
    mesh_jobs: u64,
    collider_jobs: u64,
    combined_jobs: u64,
    reserved_bytes: u64,
}

impl HighWater {
    fn observe_diagnostics(&mut self, resident: u32, active: u32, visible: u32, in_flight: u32) {
        self.resident = self.resident.max(resident);
        self.active = self.active.max(active);
        self.visible = self.visible.max(visible);
        self.in_flight = self.in_flight.max(in_flight);
    }

    fn observe_queues(&mut self, queues: &DerivedQueueSnapshotV1) {
        // Per-lane job caps govern queued work; execution has separate slot/byte caps.
        let mesh = u64::try_from(queues.mesh_pending).unwrap_or(u64::MAX);
        let collider = u64::try_from(queues.collider_pending).unwrap_or(u64::MAX);
        self.mesh_jobs = self.mesh_jobs.max(mesh);
        self.collider_jobs = self.collider_jobs.max(collider);
        let executing = u64::try_from(
            queues
                .mesh_in_flight
                .saturating_add(queues.collider_in_flight),
        )
        .unwrap_or(u64::MAX);
        self.combined_jobs = self
            .combined_jobs
            .max(mesh.saturating_add(collider).saturating_add(executing));
        self.reserved_bytes = self.reserved_bytes.max(queues.reserved_bytes);
    }
}

struct Args {
    mode: HarnessMode,
    plan: TickPlan,
    workspace: Option<PathBuf>,
    lock: Option<PathBuf>,
    output: PathBuf,
}

impl Args {
    fn parse(args: impl IntoIterator<Item = String>) -> Result<Self, HarnessError> {
        let mut mode = HarnessMode::Smoke;
        let mut warmup = None;
        let mut measure = None;
        let mut max_wall = None;
        let mut workspace = None;
        let mut lock = None;
        let mut output = None;
        let mut tokens = args.into_iter();
        while let Some(token) = tokens.next() {
            match token.as_str() {
                "--mode" => {
                    let value = tokens.next().ok_or(HarnessError::MissingValue("--mode"))?;
                    mode = HarnessMode::parse(&value).ok_or(HarnessError::UnknownMode)?;
                }
                "--warmup-ticks" => {
                    warmup = Some(parse_u32(&next_value(&mut tokens, "--warmup-ticks")?)?);
                }
                "--measure-ticks" => {
                    measure = Some(parse_u32(&next_value(&mut tokens, "--measure-ticks")?)?);
                }
                "--max-wall-seconds" => {
                    max_wall = Some(parse_u32(&next_value(&mut tokens, "--max-wall-seconds")?)?);
                }
                "--workspace" => {
                    workspace = Some(PathBuf::from(next_value(&mut tokens, "--workspace")?));
                }
                "--lock" => lock = Some(PathBuf::from(next_value(&mut tokens, "--lock")?)),
                "--output" => output = Some(PathBuf::from(next_value(&mut tokens, "--output")?)),
                "--bench" | "--nocapture" => {}
                other if other.starts_with("--") => {
                    return Err(HarnessError::UnknownFlag(other.to_owned()));
                }
                _ => {}
            }
        }
        let mut plan = TickPlan::for_mode(mode);
        if let Some(warmup) = warmup {
            plan.warmup_ticks = warmup;
        }
        if let Some(measure) = measure {
            plan.measure_ticks = measure;
        }
        if let Some(max_wall) = max_wall {
            plan.max_wall_seconds = max_wall;
        }
        if !plan.is_within_hard_bound() {
            return Err(HarnessError::UnboundedPlan);
        }
        let output = output.ok_or(HarnessError::MissingOutput)?;
        Ok(Self {
            mode,
            plan,
            workspace,
            lock,
            output,
        })
    }
}

fn next_value(
    tokens: &mut impl Iterator<Item = String>,
    flag: &'static str,
) -> Result<String, HarnessError> {
    tokens.next().ok_or(HarnessError::MissingValue(flag))
}

fn parse_u32(value: &str) -> Result<u32, HarnessError> {
    value.parse().map_err(|_| HarnessError::InvalidInteger)
}

/// Failures that keep the harness fail-closed.
#[derive(Debug, Error)]
enum HarnessError {
    /// Current directory could not be resolved.
    #[error("workspace directory is unavailable: {0}")]
    CurrentDir(io::Error),
    /// Product lock is missing.
    #[error("product lock is missing at {}; ordinary launch does not create it", path.display())]
    MissingLock {
        /// Expected lock path.
        path: PathBuf,
    },
    /// Catalog CAS is missing.
    #[error("catalog CAS is missing at {}; frozen reopen never creates the store", path.display())]
    MissingCas {
        /// Expected CAS directory.
        path: PathBuf,
    },
    /// Filesystem read or write failed.
    #[error("io failed at {}: {source}", path.display())]
    Io {
        /// Path that failed.
        path: PathBuf,
        /// Underlying error.
        #[source]
        source: io::Error,
    },
    /// Product lock could not be decoded.
    #[error(transparent)]
    ProductLock(Box<ProductLockError>),
    /// Frozen reopen failed.
    #[error(transparent)]
    Boot(Box<ProductLockBootError>),
    /// CAS open failed.
    #[error(transparent)]
    Cas(Box<CasError>),
    /// Image binding failed.
    #[error(transparent)]
    Preparation(Box<latticeaxiom_engine::PreparationError>),
    /// Host construction failed.
    #[error(transparent)]
    Host(Box<latticeaxiom_engine::ProductionHostError>),
    /// Manual ticks failed.
    #[error(transparent)]
    Instance(#[from] latticeaxiom_engine::EngineInstanceError),
    /// Action enqueue failed.
    #[error(transparent)]
    Actions(#[from] latticeaxiom_engine::ActionFrameInboxError),
    /// JSON encode failed.
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    /// Stderr write failed.
    #[error("stderr write failed: {0}")]
    Stderr(io::Error),
    /// The lock has no usable realization.
    #[error("product lock has no realization for this host")]
    MissingHostRealization,
    /// Production spine resource is absent.
    #[error("production spine is not installed")]
    MissingSpine,
    /// Streaming evidence schema drifted.
    #[error("streaming profile evidence schema is not {STREAMING_PROFILE_EVIDENCE_SCHEMA_V1}")]
    StreamingSchema,
    /// CLI flag required a value.
    #[error("missing value for {0}")]
    MissingValue(&'static str),
    /// Unknown `--mode`.
    #[error("unknown mode; expected smoke or traversal")]
    UnknownMode,
    /// Unknown flag.
    #[error("unknown flag {0}")]
    UnknownFlag(String),
    /// `--output` is required so the harness does not print JSON to stdout.
    #[error("--output is required")]
    MissingOutput,
    /// Tick plan exceeded the frozen 2 + 10 minute bound.
    #[error("tick plan exceeds the frozen 7200+36000 bound")]
    UnboundedPlan,
    /// Integer CLI parse failed.
    #[error("integer argument is invalid")]
    InvalidInteger,
}

impl From<ProductLockError> for HarnessError {
    fn from(error: ProductLockError) -> Self {
        Self::ProductLock(Box::new(error))
    }
}

impl From<ProductLockBootError> for HarnessError {
    fn from(error: ProductLockBootError) -> Self {
        Self::Boot(Box::new(error))
    }
}

impl From<CasError> for HarnessError {
    fn from(error: CasError) -> Self {
        Self::Cas(Box::new(error))
    }
}

impl From<latticeaxiom_engine::PreparationError> for HarnessError {
    fn from(error: latticeaxiom_engine::PreparationError) -> Self {
        Self::Preparation(Box::new(error))
    }
}

impl From<latticeaxiom_engine::ProductionHostError> for HarnessError {
    fn from(error: latticeaxiom_engine::ProductionHostError) -> Self {
        Self::Host(Box::new(error))
    }
}
