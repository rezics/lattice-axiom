//! Player-interest chunk streaming for the production host working set.
//!
//! Desired chunks are derived from the local player plus a clamped view and
//! generation radius around the validated V5 spawn, then around the moving
//! player. Generation uses the compiled V5 plan rather than the D4 four-chunk
//! origin neighborhood. Near-to-far order and movement look-ahead only affect
//! admission priority; they do not change authoritative voxel bytes.

use std::collections::BTreeSet;

use latticeaxiom_compose::PlayableWorldHardLimitsV1;
use latticeaxiom_runtime_contracts::{
    FullDetailDistanceChunksV1, PresentedRenderDistanceChunksV1,
    RequestedFullDetailDistanceChunksV1, RequestedRenderDistanceChunksV1,
    RequestedSimulationDistanceChunksV1, SimulationDistanceChunksV1, TargetRenderDistanceChunksV1,
    TerrainDistanceRequestsV1, admit_render_distance_chunks,
};
use latticeaxiom_storage::ChunkCoordinate;
use latticeaxiom_worldgen::{MAX_BOUNDED_REGION_CHUNKS, WorldgenConfigV1};

use super::ProductionHostError;

/// Ticks a look-ahead column stays after the last non-zero movement delta.
pub(super) const LOOK_AHEAD_EXPIRY_TICKS: u64 = 24;
/// Ticks a former core chunk stays resident after leaving the core ring.
pub(super) const RETAIN_GRACE_TICKS: u64 = 32;
/// Ticks a chunk must stay resident after admission before distance eviction.
pub(super) const MIN_RESIDENCY_TICKS: u64 = 8;

/// Typed vertical radius around the player's current chunk.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct VerticalStreamRadiusChunks(u32);

/// ADR 0026 active vertical radius for authoritative simulation.
const ACTIVE_VERTICAL_STREAM_RADIUS: VerticalStreamRadiusChunks = VerticalStreamRadiusChunks(2);
/// ADR 0026 resident vertical radius for full-resolution presentation data.
const RESIDENT_VERTICAL_STREAM_RADIUS: VerticalStreamRadiusChunks = VerticalStreamRadiusChunks(3);

/// Constraint that reduced the full-detail radius below the render target.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FullDetailClampReasonV1 {
    /// World generation is not configured to produce the requested radius.
    GenerationRadius,
    /// The bounded resident working set cannot hold the requested radius.
    ResidentBudget,
    /// Generation and resident limits meet at the same lower radius.
    GenerationRadiusAndResidentBudget,
}

macro_rules! chunk_distance_type {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
        pub struct $name(u32);

        impl $name {
            /// Creates a non-zero chunk distance.
            #[must_use]
            pub const fn new(chunks: u32) -> Option<Self> {
                if chunks == 0 { None } else { Some(Self(chunks)) }
            }

            /// Returns the horizontal Chebyshev radius in chunks.
            #[must_use]
            pub const fn chunks(self) -> u32 {
                self.0
            }
        }
    };
}

/// Contiguous presented terrain radius in world meters.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct PresentedRenderDistanceMetersV1(u32);

impl PresentedRenderDistanceMetersV1 {
    const fn from_chunks(chunks: u32, chunk_edge_meters: u16) -> Self {
        let meters = chunks.saturating_mul(chunk_edge_meters as u32);
        if meters == 0 { Self(1) } else { Self(meters) }
    }

    /// Returns the horizontal radius in world meters.
    #[must_use]
    pub const fn meters(self) -> u32 {
        self.0
    }
}
chunk_distance_type!(
    /// Base radius retained as committed full-resolution chunks.
    ResidentDistanceChunksV1
);
chunk_distance_type!(
    /// Furthest directional look-ahead distance admitted for prefetch.
    PrefetchDistanceChunksV1
);

/// Typed target, full-detail, presented, simulation, and prefetch distances.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TerrainDistanceStatusV1 {
    requested_render: RequestedRenderDistanceChunksV1,
    target_render: TargetRenderDistanceChunksV1,
    requested_full_detail: RequestedFullDetailDistanceChunksV1,
    full_detail: FullDetailDistanceChunksV1,
    presented_render: PresentedRenderDistanceChunksV1,
    presented_render_meters: PresentedRenderDistanceMetersV1,
    requested_simulation: RequestedSimulationDistanceChunksV1,
    simulation: SimulationDistanceChunksV1,
    resident: ResidentDistanceChunksV1,
    prefetch: PrefetchDistanceChunksV1,
    requested_cap: u32,
    generation_cap: u32,
    active_budget_cap: u32,
    resident_budget_cap: u32,
    full_detail_clamp_reason: Option<FullDetailClampReasonV1>,
}

impl TerrainDistanceStatusV1 {
    /// Returns the authored render-distance request before host admission.
    #[must_use]
    pub const fn requested_render_distance(self) -> RequestedRenderDistanceChunksV1 {
        self.requested_render
    }

    /// Returns the render-distance request admitted after the host cap.
    #[must_use]
    pub const fn target_render_distance(self) -> TargetRenderDistanceChunksV1 {
        self.target_render
    }

    /// Returns the validated full-detail request before host admission.
    #[must_use]
    pub const fn requested_full_detail_distance(self) -> RequestedFullDetailDistanceChunksV1 {
        self.requested_full_detail
    }

    /// Returns the radius retaining complete authoritative voxel chunks.
    #[must_use]
    pub const fn full_detail_distance(self) -> FullDetailDistanceChunksV1 {
        self.full_detail
    }

    /// Returns the largest contiguous near-plus-far radius ready this frame.
    #[must_use]
    pub const fn presented_render_distance(self) -> PresentedRenderDistanceChunksV1 {
        self.presented_render
    }

    /// Returns the contiguous presented radius in world meters.
    #[must_use]
    pub const fn presented_render_distance_meters(self) -> PresentedRenderDistanceMetersV1 {
        self.presented_render_meters
    }

    /// Returns the configured authoritative simulation radius.
    #[must_use]
    pub const fn simulation_distance(self) -> SimulationDistanceChunksV1 {
        self.simulation
    }

    /// Returns the validated simulation request before host admission.
    #[must_use]
    pub const fn requested_simulation_distance(self) -> RequestedSimulationDistanceChunksV1 {
        self.requested_simulation
    }

    /// Returns the base full-resolution resident radius.
    #[must_use]
    pub const fn resident_distance(self) -> ResidentDistanceChunksV1 {
        self.resident
    }

    /// Returns the furthest directional prefetch distance.
    #[must_use]
    pub const fn prefetch_distance(self) -> PrefetchDistanceChunksV1 {
        self.prefetch
    }

    /// Returns the inclusive host cap for player requests.
    #[must_use]
    pub const fn requested_cap(self) -> u32 {
        self.requested_cap
    }

    /// Returns the maximum radius supported by generation.
    #[must_use]
    pub const fn generation_cap(self) -> u32 {
        self.generation_cap
    }

    /// Returns the largest simulation radius that fits the active budget.
    #[must_use]
    pub const fn active_budget_cap(self) -> u32 {
        self.active_budget_cap
    }

    /// Returns the largest radius that fits the resident budget.
    #[must_use]
    pub const fn resident_budget_cap(self) -> u32 {
        self.resident_budget_cap
    }

    /// Returns the binding constraint when the effective radius is lower.
    #[must_use]
    pub const fn full_detail_clamp_reason(self) -> Option<FullDetailClampReasonV1> {
        self.full_detail_clamp_reason
    }

    /// Returns whether any host constraint lowers the authored request.
    #[must_use]
    pub const fn full_detail_is_below_target(self) -> bool {
        self.full_detail.chunks() < self.target_render.chunks()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct StreamDistances {
    requested_render: RequestedRenderDistanceChunksV1,
    target_render: TargetRenderDistanceChunksV1,
    requested_full_detail: RequestedFullDetailDistanceChunksV1,
    full_detail: FullDetailDistanceChunksV1,
    presented_render: PresentedRenderDistanceChunksV1,
    requested_simulation: RequestedSimulationDistanceChunksV1,
    simulation: SimulationDistanceChunksV1,
    resident: ResidentDistanceChunksV1,
    prefetch: PrefetchDistanceChunksV1,
}

/// Lifecycle of one streamed chunk in the production working set.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ChunkLifecycle {
    /// The chunk is outside the working set.
    Absent,
    /// The chunk is being generated from the compiled D4 plan.
    Generate,
    /// The chunk is being reloaded from the session memory kernel.
    Load,
    /// Storage-committed voxels are resident in the voxel working set.
    Resident,
    /// Mesh and collider derivation is in progress or partially applied.
    MeshCollider,
    /// Derived presentation is ready for the Bevy collider entity.
    Active,
}

/// Host clamps plus the derived interest radius used for one session.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct StreamClamps {
    pub(super) hard_limits: PlayableWorldHardLimitsV1,
    distances: StreamDistances,
    pub(super) vertical_min_chunk: i32,
    pub(super) vertical_max_chunk: i32,
    active_vertical_layers: u32,
    resident_vertical_layers: u32,
    chunk_edge_meters: u16,
}

impl StreamClamps {
    /// Derives streaming clamps from playable hard limits and the V5 plan config.
    ///
    /// # Errors
    ///
    /// Returns [`ProductionHostError::InvalidHostLimits`] when the host clamps
    /// are zero or the derived interest radius cannot be represented.
    pub(super) fn new(
        hard_limits: PlayableWorldHardLimitsV1,
        config: &WorldgenConfigV1,
    ) -> Result<Self, ProductionHostError> {
        hard_limits
            .validate()
            .map_err(|_| ProductionHostError::InvalidHostLimits)?;
        let (vertical_min_chunk, vertical_max_chunk) = vertical_chunk_bounds(config);
        let world_vertical_layers = vertical_layer_count(vertical_min_chunk, vertical_max_chunk)?;
        let active_vertical_layers =
            world_vertical_layers.min(vertical_layer_span(ACTIVE_VERTICAL_STREAM_RADIUS));
        let resident_vertical_layers =
            world_vertical_layers.min(vertical_layer_span(RESIDENT_VERTICAL_STREAM_RADIUS));
        let distances = stream_distances(
            hard_limits,
            TerrainDistanceRequestsV1::default(),
            active_vertical_layers,
            resident_vertical_layers,
        )
        .ok_or(ProductionHostError::InvalidHostLimits)?;
        Ok(Self {
            hard_limits,
            distances,
            vertical_min_chunk,
            vertical_max_chunk,
            active_vertical_layers,
            resident_vertical_layers,
            chunk_edge_meters: config.chunk_edge_voxels,
        })
    }

    /// Atomically applies the three independent terrain-distance requests.
    pub(super) fn set_terrain_distances(
        &mut self,
        requests: TerrainDistanceRequestsV1,
    ) -> Result<(), ProductionHostError> {
        self.distances = stream_distances(
            self.hard_limits,
            requests,
            self.active_vertical_layers,
            self.resident_vertical_layers,
        )
        .ok_or(ProductionHostError::InvalidHostLimits)?;
        Ok(())
    }

    /// Returns the independent target, near, presented, and simulation radii.
    pub(super) fn terrain_distance_status(self) -> TerrainDistanceStatusV1 {
        let requested_cap = self.hard_limits.view_distance_chunks.max(1);
        let generation_cap = self
            .hard_limits
            .generation_radius_chunks
            .max(1)
            .min(requested_cap);
        let active_budget_cap = budget_radius(
            requested_cap,
            self.active_vertical_layers,
            self.hard_limits.max_active_chunks,
        );
        let resident_budget_cap = resident_budget_radius(
            requested_cap,
            self.resident_vertical_layers,
            self.hard_limits.max_resident_chunks,
        );
        let generation_binds = generation_cap < self.distances.requested_full_detail.chunks()
            && generation_cap == self.distances.full_detail.chunks();
        let resident_binds = resident_budget_cap < self.distances.requested_full_detail.chunks()
            && resident_budget_cap == self.distances.full_detail.chunks();
        let full_detail_clamp_reason = match (generation_binds, resident_binds) {
            (true, true) => Some(FullDetailClampReasonV1::GenerationRadiusAndResidentBudget),
            (true, false) => Some(FullDetailClampReasonV1::GenerationRadius),
            (false, true) => Some(FullDetailClampReasonV1::ResidentBudget),
            (false, false) => None,
        };
        let presented_render_meters = PresentedRenderDistanceMetersV1::from_chunks(
            self.distances.presented_render.chunks(),
            self.chunk_edge_meters,
        );
        TerrainDistanceStatusV1 {
            requested_render: self.distances.requested_render,
            target_render: self.distances.target_render,
            requested_full_detail: self.distances.requested_full_detail,
            full_detail: self.distances.full_detail,
            presented_render: self.distances.presented_render,
            presented_render_meters,
            requested_simulation: self.distances.requested_simulation,
            simulation: self.distances.simulation,
            resident: self.distances.resident,
            prefetch: self.distances.prefetch,
            requested_cap,
            generation_cap,
            active_budget_cap,
            resident_budget_cap,
            full_detail_clamp_reason,
        }
    }

    pub(super) const fn target_render_distance(self) -> u32 {
        self.distances.target_render.chunks()
    }

    pub(super) fn set_presented_render_distance(&mut self, chunks: u32) {
        let admitted = chunks.clamp(
            self.distances.full_detail.chunks(),
            self.distances.target_render.chunks(),
        );
        if let Some(distance) = PresentedRenderDistanceChunksV1::new(admitted) {
            self.distances.presented_render = distance;
        }
    }

    pub(super) const fn full_detail_distance(self) -> u32 {
        self.distances.full_detail.chunks()
    }

    pub(super) const fn simulation_distance(self) -> u32 {
        self.distances.simulation.chunks()
    }

    pub(super) const fn resident_distance(self) -> u32 {
        self.distances.resident.chunks()
    }

    pub(super) const fn prefetch_distance(self) -> u32 {
        self.distances.prefetch.chunks()
    }

    pub(super) fn max_resident(self) -> usize {
        usize::try_from(self.hard_limits.max_resident_chunks).unwrap_or(usize::MAX)
    }

    pub(super) fn max_in_flight(self) -> usize {
        usize::try_from(self.hard_limits.max_in_flight_chunks)
            .unwrap_or(1)
            .clamp(1, MAX_BOUNDED_REGION_CHUNKS)
    }

    /// Soft high-water at which prefetch admission stops.
    pub(super) fn prefetch_high_water(self) -> usize {
        prefetch_high_water(self.max_resident())
    }

    fn vertical_bounds(self, origin_y: i32, radius: VerticalStreamRadiusChunks) -> (i32, i32) {
        let world_min = i64::from(self.vertical_min_chunk);
        let world_max = i64::from(self.vertical_max_chunk);
        let radius = i64::from(radius.0);
        let world_span = world_max.saturating_sub(world_min);
        let window_span = radius.saturating_mul(2).min(world_span);
        let last_start = world_max.saturating_sub(window_span);
        let minimum = i64::from(origin_y)
            .saturating_sub(radius)
            .clamp(world_min, last_start);
        let maximum = minimum.saturating_add(window_span);
        (
            i32::try_from(minimum).unwrap_or(self.vertical_min_chunk),
            i32::try_from(maximum).unwrap_or(self.vertical_max_chunk),
        )
    }

    fn active_vertical_bounds(self, origin_y: i32) -> (i32, i32) {
        self.vertical_bounds(origin_y, ACTIVE_VERTICAL_STREAM_RADIUS)
    }

    fn resident_vertical_bounds(self, origin_y: i32) -> (i32, i32) {
        self.vertical_bounds(origin_y, RESIDENT_VERTICAL_STREAM_RADIUS)
    }

    fn contains_active_y(self, chunk_y: i32, origin_y: i32) -> bool {
        let (minimum, maximum) = self.active_vertical_bounds(origin_y);
        (minimum..=maximum).contains(&chunk_y)
    }

    fn contains_resident_y(self, chunk_y: i32, origin_y: i32) -> bool {
        let (minimum, maximum) = self.resident_vertical_bounds(origin_y);
        (minimum..=maximum).contains(&chunk_y)
    }
}

/// Interest class used to admit, retain, and evict streamed chunks.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(super) enum InterestClass {
    /// Authoritative dirty, edit, or player pin. Never distance-evicted.
    Pin,
    /// Player core ring. Never distance-evicted while it remains core.
    Core,
    /// Former core protected by minimum residency or grace ticks.
    Retain,
    /// Movement look-ahead. Lowest admission priority.
    Prefetch,
}

/// Inclusive generated chunk Y range covering the D4 floor and ceiling.
#[must_use]
pub(super) fn vertical_chunk_bounds(config: &WorldgenConfigV1) -> (i32, i32) {
    let edge = i32::from(config.chunk_edge_voxels);
    (
        config.world_floor_y.div_euclid(edge),
        config.world_ceiling_y.div_euclid(edge),
    )
}

/// Chebyshev distance on the horizontal `(x, z)` plane.
#[must_use]
pub(super) fn chebyshev_xz(left: ChunkCoordinate, right: ChunkCoordinate) -> u32 {
    left.x.abs_diff(right.x).max(left.z.abs_diff(right.z))
}

/// Quantizes horizontal translation delta into a unit look-ahead axis.
#[must_use]
pub(super) fn look_ahead_axis(delta_xz: [f32; 2]) -> [i32; 2] {
    const MIN_DELTA_M: f32 = 0.05;
    [
        quantized_axis(delta_xz[0], MIN_DELTA_M),
        quantized_axis(delta_xz[1], MIN_DELTA_M),
    ]
}

/// Keeps the last valid movement axis until an explicit expiry.
///
/// A single zero delta does not revoke look-ahead. Expiry is counted from the
/// last non-zero quantized axis.
#[must_use]
pub(super) fn sticky_look_ahead(
    delta_xz: [f32; 2],
    last_axis: [i32; 2],
    last_valid_tick: u64,
    now: u64,
    expiry_ticks: u64,
) -> ([i32; 2], u64) {
    let axis = look_ahead_axis(delta_xz);
    if axis != [0, 0] {
        (axis, now)
    } else if last_axis != [0, 0] && now.saturating_sub(last_valid_tick) < expiry_ticks {
        (last_axis, last_valid_tick)
    } else {
        ([0, 0], last_valid_tick)
    }
}

/// Returns whether a resident chunk is still inside the retain/grace window.
#[must_use]
pub(super) fn retain_protected(now: u64, admitted_tick: u64, last_core_tick: Option<u64>) -> bool {
    now.saturating_sub(admitted_tick) < MIN_RESIDENCY_TICKS
        || last_core_tick.is_some_and(|tick| now.saturating_sub(tick) < RETAIN_GRACE_TICKS)
}

/// Classifies one chunk relative to the current player origin.
#[must_use]
pub(super) fn interest_class(
    chunk: ChunkCoordinate,
    origin: ChunkCoordinate,
    clamps: StreamClamps,
    look_ahead: [i32; 2],
    pins: &BTreeSet<ChunkCoordinate>,
) -> InterestClass {
    if pins.contains(&chunk) {
        return InterestClass::Pin;
    }
    if is_render_chunk(chunk, origin, clamps) {
        return InterestClass::Core;
    }
    if is_prefetch_chunk(chunk, origin, clamps, look_ahead) {
        return InterestClass::Prefetch;
    }
    InterestClass::Retain
}

/// Desired working-set coordinates for one player chunk.
///
/// Core is the clamped horizontal ring within a bounded vertical window around
/// the player. Prefetch may add one look-ahead column over the same window.
/// Pins stay in the set even when they leave either radius. Retain is eviction
/// hysteresis only and is not admitted here.
#[must_use]
pub(super) fn desired_chunks(
    origin: ChunkCoordinate,
    clamps: StreamClamps,
    look_ahead: [i32; 2],
    pins: &BTreeSet<ChunkCoordinate>,
) -> BTreeSet<ChunkCoordinate> {
    let mut desired = BTreeSet::new();
    let (vertical_min, vertical_max) = clamps.resident_vertical_bounds(origin.y);
    insert_column(&mut desired, origin.x, origin.z, vertical_min, vertical_max);
    let radius = i32::try_from(clamps.resident_distance()).unwrap_or(i32::MAX);
    let min_x = origin.x.saturating_sub(radius);
    let max_x = origin.x.saturating_add(radius);
    let min_z = origin.z.saturating_sub(radius);
    let max_z = origin.z.saturating_add(radius);
    for z in min_z..=max_z {
        for x in min_x..=max_x {
            insert_column(&mut desired, x, z, vertical_min, vertical_max);
        }
    }
    if let Some((ahead_x, ahead_z)) =
        look_ahead_column(origin, clamps.prefetch_distance(), look_ahead)
    {
        insert_column(&mut desired, ahead_x, ahead_z, vertical_min, vertical_max);
    }
    desired.extend(pins.iter().copied());
    fit_desired(desired, origin, look_ahead, pins, clamps)
}

/// Near-to-far admission order. Look-ahead may raise same-ring priority.
#[must_use]
pub(super) fn prioritize_chunks(
    desired: &BTreeSet<ChunkCoordinate>,
    origin: ChunkCoordinate,
    look_ahead: [i32; 2],
) -> Vec<ChunkCoordinate> {
    let mut ordered = desired.iter().copied().collect::<Vec<_>>();
    ordered.sort_by_key(|chunk| {
        let distance = chebyshev_xz(*chunk, origin);
        let vertical_distance = chunk.y.abs_diff(origin.y);
        let look = i64::from(chunk.x.saturating_sub(origin.x))
            .saturating_mul(i64::from(look_ahead[0]))
            .saturating_add(
                i64::from(chunk.z.saturating_sub(origin.z))
                    .saturating_mul(i64::from(look_ahead[1])),
            );
        (
            distance,
            vertical_distance,
            std::cmp::Reverse(look),
            chunk.x,
            chunk.y,
            chunk.z,
        )
    });
    ordered
}

fn clamped_full_detail_radius_for(
    limits: PlayableWorldHardLimitsV1,
    requested_full_detail: u32,
    resident_vertical_layers: u32,
) -> u32 {
    let requested = requested_full_detail.min(limits.generation_radius_chunks);
    resident_budget_radius(
        requested,
        resident_vertical_layers,
        limits.max_resident_chunks,
    )
}

fn stream_distances(
    limits: PlayableWorldHardLimitsV1,
    requests: TerrainDistanceRequestsV1,
    active_vertical_layers: u32,
    resident_vertical_layers: u32,
) -> Option<StreamDistances> {
    let requested_render = requests.render();
    let target_render = admit_render_distance_chunks(requested_render, limits.view_distance_chunks);
    let requested_full_detail = requests.full_detail();
    let full_detail = clamped_full_detail_radius_for(
        limits,
        requested_full_detail.chunks().min(target_render.chunks()),
        resident_vertical_layers,
    );
    let requested_simulation = requests.simulation();
    let simulation = budget_radius(
        requested_simulation.chunks().min(full_detail),
        active_vertical_layers,
        limits.max_active_chunks,
    );
    let generation_cap = limits
        .generation_radius_chunks
        .max(1)
        .min(target_render.chunks());
    let prefetch = full_detail
        .saturating_add(1)
        .min(generation_cap)
        .max(full_detail);
    Some(StreamDistances {
        requested_render,
        target_render,
        requested_full_detail,
        full_detail: FullDetailDistanceChunksV1::new(full_detail)?,
        // The far path remains feature-disabled in Slice 1, so only the near
        // full-detail frontier can be presented contiguously.
        presented_render: PresentedRenderDistanceChunksV1::new(full_detail)?,
        requested_simulation,
        simulation: SimulationDistanceChunksV1::new(simulation)?,
        resident: ResidentDistanceChunksV1::new(full_detail)?,
        prefetch: PrefetchDistanceChunksV1::new(prefetch)?,
    })
}

fn resident_budget_radius(
    maximum_radius: u32,
    vertical_layers: u32,
    max_resident_chunks: u32,
) -> u32 {
    budget_radius(maximum_radius, vertical_layers, max_resident_chunks)
}

fn budget_radius(maximum_radius: u32, vertical_layers: u32, maximum_chunks: u32) -> u32 {
    let mut radius = maximum_radius;
    while radius > 0 {
        if interest_volume(radius, vertical_layers, false) <= maximum_chunks {
            return radius;
        }
        radius -= 1;
    }
    radius
}

const fn vertical_layer_span(radius: VerticalStreamRadiusChunks) -> u32 {
    radius.0.saturating_mul(2).saturating_add(1)
}

fn interest_volume(radius: u32, vertical_layers: u32, include_look_ahead: bool) -> u32 {
    let width = radius.saturating_mul(2).saturating_add(1);
    let mut columns = width.saturating_mul(width);
    if include_look_ahead {
        columns = columns.saturating_add(1);
    }
    columns.saturating_mul(vertical_layers.max(1))
}

fn vertical_layer_count(min_y: i32, max_y: i32) -> Result<u32, ProductionHostError> {
    if max_y < min_y {
        return Err(ProductionHostError::InvalidHostLimits);
    }
    u32::try_from(
        i64::from(max_y)
            .saturating_sub(i64::from(min_y))
            .saturating_add(1),
    )
    .map_err(|_| ProductionHostError::InvalidHostLimits)
}

fn insert_column(desired: &mut BTreeSet<ChunkCoordinate>, x: i32, z: i32, min_y: i32, max_y: i32) {
    for y in min_y..=max_y {
        desired.insert(ChunkCoordinate::new(x, y, z));
    }
}

fn look_ahead_column(
    origin: ChunkCoordinate,
    distance: u32,
    look_ahead: [i32; 2],
) -> Option<(i32, i32)> {
    if look_ahead == [0, 0] {
        return None;
    }
    let extra = i32::try_from(distance).ok()?;
    let x = origin.x.checked_add(look_ahead[0].saturating_mul(extra))?;
    let z = origin.z.checked_add(look_ahead[1].saturating_mul(extra))?;
    Some((x, z))
}

pub(super) fn is_render_chunk(
    chunk: ChunkCoordinate,
    origin: ChunkCoordinate,
    clamps: StreamClamps,
) -> bool {
    clamps.contains_resident_y(chunk.y, origin.y)
        && chebyshev_xz(chunk, origin) <= clamps.full_detail_distance()
}

/// Returns whether a chunk is inside the bounded authoritative simulation set.
#[must_use]
pub(super) fn is_simulation_chunk(
    chunk: ChunkCoordinate,
    origin: ChunkCoordinate,
    clamps: StreamClamps,
) -> bool {
    clamps.contains_active_y(chunk.y, origin.y)
        && chebyshev_xz(chunk, origin) <= clamps.simulation_distance()
}

fn is_prefetch_chunk(
    chunk: ChunkCoordinate,
    origin: ChunkCoordinate,
    clamps: StreamClamps,
    look_ahead: [i32; 2],
) -> bool {
    look_ahead_column(origin, clamps.prefetch_distance(), look_ahead) == Some((chunk.x, chunk.z))
        && clamps.contains_resident_y(chunk.y, origin.y)
}

fn prefetch_high_water(max_resident: usize) -> usize {
    max_resident.saturating_mul(3) / 4
}

fn fit_desired(
    mut desired: BTreeSet<ChunkCoordinate>,
    origin: ChunkCoordinate,
    look_ahead: [i32; 2],
    pins: &BTreeSet<ChunkCoordinate>,
    clamps: StreamClamps,
) -> BTreeSet<ChunkCoordinate> {
    let max_resident = clamps.max_resident();
    while desired.len() > max_resident {
        let victim = desired
            .iter()
            .copied()
            .filter(|chunk| {
                !matches!(
                    interest_class(*chunk, origin, clamps, look_ahead, pins),
                    InterestClass::Pin | InterestClass::Core
                )
            })
            .max_by_key(|chunk| {
                let prefetch = u8::from(
                    interest_class(*chunk, origin, clamps, look_ahead, pins)
                        == InterestClass::Prefetch,
                );
                (
                    prefetch,
                    chebyshev_xz(*chunk, origin),
                    chunk.x,
                    chunk.y,
                    chunk.z,
                )
            });
        let Some(victim) = victim else {
            break;
        };
        desired.remove(&victim);
    }
    desired
}

fn quantized_axis(delta: f32, minimum: f32) -> i32 {
    if !delta.is_finite() || delta.abs() < minimum {
        0
    } else if delta > 0.0 {
        1
    } else {
        -1
    }
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used)]
mod tests {
    use super::{
        ChunkLifecycle, FullDetailClampReasonV1, InterestClass, LOOK_AHEAD_EXPIRY_TICKS,
        MIN_RESIDENCY_TICKS, RETAIN_GRACE_TICKS, StreamClamps, chebyshev_xz, desired_chunks,
        interest_class, is_simulation_chunk, look_ahead_axis, prioritize_chunks, retain_protected,
        sticky_look_ahead,
    };
    use latticeaxiom_compose::PlayableWorldHardLimitsV1;
    use latticeaxiom_runtime_contracts::{
        TerrainDistanceRequestsV1, clamp_full_detail_distance_chunks,
        clamp_requested_render_distance_chunks, clamp_simulation_distance_chunks,
    };
    use latticeaxiom_storage::ChunkCoordinate;
    use latticeaxiom_worldgen::{
        ORIGIN_NEIGHBORHOOD_CHUNK_COORDINATES_V1, PlanningCellCoordinateV1, WorldgenConfigV1,
    };
    use std::collections::BTreeSet;

    fn requests(render: i64, simulation: i64, full_detail: i64) -> TerrainDistanceRequestsV1 {
        TerrainDistanceRequestsV1::new(
            clamp_requested_render_distance_chunks(render),
            clamp_simulation_distance_chunks(simulation),
            clamp_full_detail_distance_chunks(full_detail),
        )
    }

    fn clamps() -> StreamClamps {
        let limits = PlayableWorldHardLimitsV1::new(2, 2, 64, 64, 4, 2).expect("nonzero clamps");
        StreamClamps::new(
            limits,
            &WorldgenConfigV1 {
                chunk_edge_voxels: 8,
                world_floor_y: 0,
                world_ceiling_y: 31,
                ..WorldgenConfigV1::default()
            },
        )
        .expect("clamps are valid")
    }

    #[test]
    fn interest_radius_fits_resident_budget() {
        let clamps = clamps();
        assert_eq!(clamps.full_detail_distance(), 1);
        assert_eq!(clamps.simulation_distance(), 1);
        assert_eq!(clamps.resident_distance(), 1);
        assert_eq!(clamps.prefetch_distance(), 2);
        assert_eq!(clamps.target_render_distance(), 2);
        assert_eq!(clamps.vertical_min_chunk, 0);
        assert_eq!(clamps.vertical_max_chunk, 3);
        assert!(clamps.max_in_flight() <= 16);
        assert_eq!(
            PlanningCellCoordinateV1::from_chunk(ChunkCoordinate::new(17, 2, -9), 8),
            PlanningCellCoordinateV1::new(2, -2)
        );
    }

    #[test]
    fn nonzero_distance_types_reject_an_unrepresentable_resident_budget() {
        let limits = PlayableWorldHardLimitsV1::new(2, 2, 1, 1, 1, 1).expect("nonzero clamps");
        let result = StreamClamps::new(
            limits,
            &WorldgenConfigV1 {
                chunk_edge_voxels: 8,
                world_floor_y: 0,
                world_ceiling_y: 31,
                ..WorldgenConfigV1::default()
            },
        );
        assert!(result.is_err());
    }

    #[test]
    fn requested_render_full_detail_and_simulation_are_admitted_independently() {
        let limits = PlayableWorldHardLimitsV1::new(4, 4, 128, 128, 8, 4).expect("nonzero clamps");
        let mut clamps = StreamClamps::new(
            limits,
            &WorldgenConfigV1 {
                chunk_edge_voxels: 8,
                world_floor_y: 0,
                world_ceiling_y: 31,
                ..WorldgenConfigV1::default()
            },
        )
        .expect("clamps are valid");
        clamps
            .set_terrain_distances(requests(1, 4, 6))
            .expect("distance contract remains valid");
        assert_eq!(clamps.target_render_distance(), 2);
        assert_eq!(clamps.full_detail_distance(), 2);
        clamps
            .set_terrain_distances(requests(4, 4, 6))
            .expect("distance contract remains valid");
        assert_eq!(clamps.target_render_distance(), 4);
        assert_eq!(clamps.full_detail_distance(), 2);
        assert_eq!(
            clamps.terrain_distance_status().full_detail_clamp_reason(),
            Some(FullDetailClampReasonV1::ResidentBudget)
        );
        clamps
            .set_terrain_distances(requests(0, 4, 6))
            .expect("distance contract remains valid");
        assert_eq!(clamps.target_render_distance(), 2);
        clamps
            .set_terrain_distances(requests(99, 4, 6))
            .expect("distance contract remains valid");
        assert_eq!(clamps.target_render_distance(), 4);
    }

    #[test]
    fn desktop_target_stays_independent_from_certified_full_detail_radius() {
        let limits = PlayableWorldHardLimitsV1::new(32, 32, 405, 1_183, 8, 4)
            .expect("desktop request clamps are nonzero");
        let mut clamps = StreamClamps::new(
            limits,
            &WorldgenConfigV1 {
                chunk_edge_voxels: 8,
                world_floor_y: 0,
                world_ceiling_y: 31,
                ..WorldgenConfigV1::default()
            },
        )
        .expect("desktop request clamps are valid");
        clamps
            .set_terrain_distances(requests(32, 4, 6))
            .expect("distance contract remains valid");

        let status = clamps.terrain_distance_status();
        assert_eq!(status.requested_render_distance().chunks(), 32);
        assert_eq!(status.target_render_distance().chunks(), 32);
        assert_eq!(status.requested_cap(), 32);
        assert_eq!(status.generation_cap(), 32);
        assert_eq!(status.active_budget_cap(), 4);
        assert_eq!(status.resident_budget_cap(), 8);
        assert_eq!(status.full_detail_distance().chunks(), 6);
        assert_eq!(status.presented_render_distance().chunks(), 6);
        assert_eq!(status.presented_render_distance_meters().meters(), 48);
        assert_eq!(status.simulation_distance().chunks(), 4);
        assert_eq!(status.resident_distance().chunks(), 6);
        assert_eq!(status.prefetch_distance().chunks(), 7);
        assert_eq!(status.full_detail_clamp_reason(), None);
        assert!(status.full_detail_is_below_target());

        clamps.set_presented_render_distance(21);
        assert_eq!(
            clamps
                .terrain_distance_status()
                .presented_render_distance()
                .chunks(),
            21
        );
        clamps.set_presented_render_distance(u32::MAX);
        assert_eq!(
            clamps
                .terrain_distance_status()
                .presented_render_distance()
                .chunks(),
            32
        );
        clamps.set_presented_render_distance(1);
        assert_eq!(
            clamps
                .terrain_distance_status()
                .presented_render_distance()
                .chunks(),
            6
        );

        let desired = desired_chunks(
            ChunkCoordinate::new(0, 0, 0),
            clamps,
            [1, 0],
            &BTreeSet::new(),
        );
        assert_eq!(desired.len(), 680);
        assert!(desired.len() <= clamps.max_resident());
    }

    #[test]
    fn host_cap_changes_target_without_mutating_requested_draft() {
        let limits = PlayableWorldHardLimitsV1::new(4, 4, 128, 128, 8, 4).expect("nonzero clamps");
        let mut clamps = StreamClamps::new(
            limits,
            &WorldgenConfigV1 {
                chunk_edge_voxels: 8,
                world_floor_y: 0,
                world_ceiling_y: 31,
                ..WorldgenConfigV1::default()
            },
        )
        .expect("clamps are valid");
        let requested_draft = 99;

        clamps
            .set_terrain_distances(requests(i64::from(requested_draft), 4, 6))
            .expect("distance contract remains valid");
        let status = clamps.terrain_distance_status();

        assert_eq!(requested_draft, 99);
        assert_eq!(status.requested_render_distance().chunks(), 32);
        assert_eq!(status.target_render_distance().chunks(), 4);
        assert_eq!(status.full_detail_distance().chunks(), 2);
        assert_eq!(status.requested_cap(), 4);
        assert!(
            status.target_render_distance().chunks() < status.requested_render_distance().chunks()
        );
        assert!(status.full_detail_is_below_target());
    }

    #[test]
    fn desired_chunks_are_not_the_d4_origin_neighborhood() {
        let origin = ChunkCoordinate::new(4, 2, -3);
        let desired = desired_chunks(origin, clamps(), [0, 0], &BTreeSet::new());
        for chunk in ORIGIN_NEIGHBORHOOD_CHUNK_COORDINATES_V1 {
            assert!(
                !desired.contains(&chunk),
                "interest around a far origin must not collapse to {chunk:?}"
            );
        }
        assert!(desired.contains(&ChunkCoordinate::new(4, 0, -3)));
        assert!(desired.contains(&ChunkCoordinate::new(5, 3, -2)));
    }

    #[test]
    fn desired_chunks_cover_full_vertical_column() {
        let origin = ChunkCoordinate::new(-1, 2, -1);
        let desired = desired_chunks(origin, clamps(), [0, 0], &BTreeSet::new());
        for y in 0..=3 {
            assert!(desired.contains(&ChunkCoordinate::new(-1, y, -1)));
            assert!(desired.contains(&ChunkCoordinate::new(0, y, -1)));
            assert!(desired.contains(&ChunkCoordinate::new(-2, y, 0)));
        }
        assert!(!desired.contains(&ChunkCoordinate::new(2, 0, 0)));
    }

    #[test]
    fn tall_world_streams_a_bounded_vertical_window() {
        let limits = PlayableWorldHardLimitsV1::new(32, 32, 405, 1_183, 8, 4)
            .expect("desktop request clamps are nonzero");
        let mut clamps = StreamClamps::new(
            limits,
            &WorldgenConfigV1 {
                chunk_edge_voxels: 32,
                world_floor_y: -64,
                world_ceiling_y: 319,
                ..WorldgenConfigV1::default()
            },
        )
        .expect("tall-world clamps are valid");
        let origin = ChunkCoordinate::new(0, 2, 0);
        let desired = desired_chunks(origin, clamps, [0, 0], &BTreeSet::new());

        assert_eq!(clamps.vertical_min_chunk, -2);
        assert_eq!(clamps.vertical_max_chunk, 9);
        assert_eq!(clamps.full_detail_distance(), 6);
        assert_eq!(
            clamps
                .terrain_distance_status()
                .presented_render_distance_meters()
                .meters(),
            192
        );
        assert_eq!(clamps.simulation_distance(), 4);
        assert_eq!(clamps.resident_distance(), 6);
        assert_eq!(desired.len(), 1_183);
        for y in -1..=5 {
            assert!(desired.contains(&ChunkCoordinate::new(0, y, 0)));
        }
        assert!(!desired.contains(&ChunkCoordinate::new(0, -2, 0)));
        assert!(!desired.contains(&ChunkCoordinate::new(0, 6, 0)));
        assert!(is_simulation_chunk(
            ChunkCoordinate::new(4, 4, -4),
            origin,
            clamps
        ));
        assert!(!is_simulation_chunk(
            ChunkCoordinate::new(5, 2, 0),
            origin,
            clamps
        ));
        assert!(!is_simulation_chunk(
            ChunkCoordinate::new(0, 5, 0),
            origin,
            clamps
        ));

        clamps
            .set_terrain_distances(requests(4, 4, 4))
            .expect("four-chunk request remains inside the desktop profile");
        let reduced = desired_chunks(origin, clamps, [0, 0], &BTreeSet::new());
        assert_eq!(clamps.full_detail_distance(), 4);
        assert_eq!(
            clamps
                .terrain_distance_status()
                .presented_render_distance_meters()
                .meters(),
            128
        );
        assert_eq!(reduced.len(), 567);
        assert!(reduced.len() < desired.len());
    }

    #[test]
    fn look_ahead_and_edited_chunks_are_pinned() {
        let origin = ChunkCoordinate::new(0, 0, 0);
        let edited = BTreeSet::from([ChunkCoordinate::new(6, 1, 0)]);
        let desired = desired_chunks(origin, clamps(), [1, 0], &edited);
        assert!(desired.contains(&ChunkCoordinate::new(2, 0, 0)));
        assert!(desired.contains(&ChunkCoordinate::new(6, 1, 0)));
        assert_eq!(look_ahead_axis([0.2, -0.01]), [1, 0]);
    }

    #[test]
    fn priority_is_near_to_far_then_look_ahead() {
        let origin = ChunkCoordinate::new(0, 0, 0);
        let desired = BTreeSet::from([
            ChunkCoordinate::new(1, 0, 0),
            ChunkCoordinate::new(-1, 0, 0),
            ChunkCoordinate::new(0, 0, 0),
        ]);
        let ordered = prioritize_chunks(&desired, origin, [1, 0]);
        assert_eq!(ordered[0], ChunkCoordinate::new(0, 0, 0));
        assert_eq!(ordered[1], ChunkCoordinate::new(1, 0, 0));
        assert_eq!(ordered[2], ChunkCoordinate::new(-1, 0, 0));
        let vertical = BTreeSet::from([
            ChunkCoordinate::new(0, 0, 0),
            ChunkCoordinate::new(0, 2, 0),
            ChunkCoordinate::new(0, 3, 0),
        ]);
        assert_eq!(
            prioritize_chunks(&vertical, ChunkCoordinate::new(0, 3, 0), [0, 0]),
            [
                ChunkCoordinate::new(0, 3, 0),
                ChunkCoordinate::new(0, 2, 0),
                ChunkCoordinate::new(0, 0, 0),
            ]
        );
        assert_eq!(chebyshev_xz(ChunkCoordinate::new(-3, 2, 1), origin), 3);
        assert_ne!(ChunkLifecycle::Absent, ChunkLifecycle::Active);
    }

    #[test]
    fn look_ahead_is_not_revoked_on_a_zero_delta() {
        let (axis, tick) = sticky_look_ahead([0.2, 0.0], [0, 0], 0, 10, LOOK_AHEAD_EXPIRY_TICKS);
        assert_eq!(axis, [1, 0]);
        let (held, held_tick) = sticky_look_ahead(
            [0.0, 0.0],
            axis,
            tick,
            tick.saturating_add(1),
            LOOK_AHEAD_EXPIRY_TICKS,
        );
        assert_eq!(held, [1, 0]);
        assert_eq!(held_tick, tick);
        let expired_at = tick.saturating_add(LOOK_AHEAD_EXPIRY_TICKS);
        let (expired, _) =
            sticky_look_ahead([0.0, 0.0], axis, tick, expired_at, LOOK_AHEAD_EXPIRY_TICKS);
        assert_eq!(expired, [0, 0]);
    }

    #[test]
    fn core_is_never_a_distance_victim_and_prefetch_sorts_last() {
        let origin = ChunkCoordinate::new(0, 0, 0);
        let pins = BTreeSet::from([ChunkCoordinate::new(6, 1, 0)]);
        let core = ChunkCoordinate::new(1, 0, 0);
        let prefetch = ChunkCoordinate::new(2, 0, 0);
        assert_eq!(
            interest_class(core, origin, clamps(), [1, 0], &pins),
            InterestClass::Core
        );
        assert_eq!(
            interest_class(prefetch, origin, clamps(), [1, 0], &pins),
            InterestClass::Prefetch
        );
        assert_eq!(
            interest_class(
                ChunkCoordinate::new(6, 1, 0),
                origin,
                clamps(),
                [1, 0],
                &pins
            ),
            InterestClass::Pin
        );
        assert!(desired_chunks(origin, clamps(), [1, 0], &pins).contains(&core));
        assert!(desired_chunks(origin, clamps(), [1, 0], &pins).contains(&prefetch));
    }

    #[test]
    fn retain_grace_and_minimum_residency_protect_former_core() {
        assert!(retain_protected(7, 0, None));
        assert!(!retain_protected(MIN_RESIDENCY_TICKS, 0, None));
        assert!(retain_protected(MIN_RESIDENCY_TICKS, 0, Some(0)));
        assert!(!retain_protected(RETAIN_GRACE_TICKS, 0, Some(0)));
        assert!(retain_protected(
            RETAIN_GRACE_TICKS.saturating_sub(1),
            0,
            Some(0)
        ));
    }
}
