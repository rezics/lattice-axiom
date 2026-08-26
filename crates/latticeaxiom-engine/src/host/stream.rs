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
    AUTHORED_MAX_VIEW_DISTANCE_CHUNKS, DEFAULT_VIEW_DISTANCE_CHUNKS, MIN_VIEW_DISTANCE_CHUNKS,
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

/// Constraint that reduced a requested view distance for the active host.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ViewDistanceClampReasonV1 {
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

chunk_distance_type!(
    /// Authored render-distance request before host admission.
    RequestedRenderDistanceChunksV1
);
chunk_distance_type!(
    /// Render-distance request admitted by the active host capability.
    AdmittedRenderDistanceChunksV1
);
chunk_distance_type!(
    /// Render distance actually supported by generation and resident budgets.
    EffectiveRenderDistanceChunksV1
);
chunk_distance_type!(
    /// Radius in which authoritative gameplay simulation may tick.
    SimulationDistanceChunksV1
);
chunk_distance_type!(
    /// Base radius retained as committed full-resolution chunks.
    ResidentDistanceChunksV1
);
chunk_distance_type!(
    /// Furthest directional look-ahead distance admitted for prefetch.
    PrefetchDistanceChunksV1
);

/// Typed render, simulation, residency, and prefetch distances for one host.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ViewDistanceStatusV1 {
    requested_render: RequestedRenderDistanceChunksV1,
    admitted_render: AdmittedRenderDistanceChunksV1,
    effective_render: EffectiveRenderDistanceChunksV1,
    simulation: SimulationDistanceChunksV1,
    resident: ResidentDistanceChunksV1,
    prefetch: PrefetchDistanceChunksV1,
    requested_cap: u32,
    generation_cap: u32,
    resident_budget_cap: u32,
    clamp_reason: Option<ViewDistanceClampReasonV1>,
}

impl ViewDistanceStatusV1 {
    /// Returns the authored render-distance request before host admission.
    #[must_use]
    pub const fn requested_render_distance(self) -> RequestedRenderDistanceChunksV1 {
        self.requested_render
    }

    /// Returns the render-distance request admitted after the host cap.
    #[must_use]
    pub const fn admitted_render_distance(self) -> AdmittedRenderDistanceChunksV1 {
        self.admitted_render
    }

    /// Returns the render radius actually supported by bounded streaming.
    #[must_use]
    pub const fn effective_render_distance(self) -> EffectiveRenderDistanceChunksV1 {
        self.effective_render
    }

    /// Returns the configured authoritative simulation radius.
    #[must_use]
    pub const fn simulation_distance(self) -> SimulationDistanceChunksV1 {
        self.simulation
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

    /// Returns the largest radius that fits the resident budget.
    #[must_use]
    pub const fn resident_budget_cap(self) -> u32 {
        self.resident_budget_cap
    }

    /// Returns the binding constraint when the effective radius is lower.
    #[must_use]
    pub const fn clamp_reason(self) -> Option<ViewDistanceClampReasonV1> {
        self.clamp_reason
    }

    /// Returns whether any host constraint lowers the authored request.
    #[must_use]
    pub const fn is_clamped(self) -> bool {
        self.effective_render.chunks() < self.requested_render.chunks()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct StreamDistances {
    requested_render: RequestedRenderDistanceChunksV1,
    admitted_render: AdmittedRenderDistanceChunksV1,
    effective_render: EffectiveRenderDistanceChunksV1,
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
    vertical_layers: u32,
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
        let vertical_layers = vertical_layer_count(vertical_min_chunk, vertical_max_chunk)?;
        let distances =
            stream_distances(hard_limits, DEFAULT_VIEW_DISTANCE_CHUNKS, vertical_layers)
                .ok_or(ProductionHostError::InvalidHostLimits)?;
        Ok(Self {
            hard_limits,
            distances,
            vertical_min_chunk,
            vertical_max_chunk,
            vertical_layers,
        })
    }

    /// Sets the player-requested view radius and recomputes interest against the resident budget.
    pub(super) fn set_requested_view_distance(
        &mut self,
        requested: u32,
    ) -> Result<(), ProductionHostError> {
        self.distances = stream_distances(self.hard_limits, requested, self.vertical_layers)
            .ok_or(ProductionHostError::InvalidHostLimits)?;
        Ok(())
    }

    /// Returns the accepted request, effective radius, and binding clamp.
    pub(super) fn view_distance_status(self) -> ViewDistanceStatusV1 {
        let requested_cap = self.hard_limits.view_distance_chunks.max(1);
        let generation_cap = self
            .hard_limits
            .generation_radius_chunks
            .max(1)
            .min(requested_cap);
        let resident_budget_cap = resident_budget_radius(
            requested_cap,
            self.vertical_layers,
            self.hard_limits.max_resident_chunks,
        );
        let generation_binds = generation_cap < self.distances.admitted_render.chunks()
            && generation_cap == self.distances.effective_render.chunks();
        let resident_binds = resident_budget_cap < self.distances.admitted_render.chunks()
            && resident_budget_cap == self.distances.effective_render.chunks();
        let clamp_reason = match (generation_binds, resident_binds) {
            (true, true) => Some(ViewDistanceClampReasonV1::GenerationRadiusAndResidentBudget),
            (true, false) => Some(ViewDistanceClampReasonV1::GenerationRadius),
            (false, true) => Some(ViewDistanceClampReasonV1::ResidentBudget),
            (false, false) => None,
        };
        ViewDistanceStatusV1 {
            requested_render: self.distances.requested_render,
            admitted_render: self.distances.admitted_render,
            effective_render: self.distances.effective_render,
            simulation: self.distances.simulation,
            resident: self.distances.resident,
            prefetch: self.distances.prefetch,
            requested_cap,
            generation_cap,
            resident_budget_cap,
            clamp_reason,
        }
    }

    pub(super) const fn admitted_render_distance(self) -> u32 {
        self.distances.admitted_render.chunks()
    }

    pub(super) const fn effective_render_distance(self) -> u32 {
        self.distances.effective_render.chunks()
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
    if is_core_chunk(chunk, origin, clamps) {
        return InterestClass::Core;
    }
    if is_prefetch_chunk(chunk, origin, clamps, look_ahead) {
        return InterestClass::Prefetch;
    }
    InterestClass::Retain
}

/// Desired working-set coordinates for one player chunk.
///
/// Core is the player column plus the clamped horizontal ring. Prefetch may add
/// one look-ahead column. Pins stay in the set even when they leave the view
/// radius. Retain is eviction hysteresis only and is not admitted here.
#[must_use]
pub(super) fn desired_chunks(
    origin: ChunkCoordinate,
    clamps: StreamClamps,
    look_ahead: [i32; 2],
    pins: &BTreeSet<ChunkCoordinate>,
) -> BTreeSet<ChunkCoordinate> {
    let mut desired = BTreeSet::new();
    insert_column(
        &mut desired,
        origin.x,
        origin.z,
        clamps.vertical_min_chunk,
        clamps.vertical_max_chunk,
    );
    let radius = i32::try_from(clamps.resident_distance()).unwrap_or(i32::MAX);
    let min_x = origin.x.saturating_sub(radius);
    let max_x = origin.x.saturating_add(radius);
    let min_z = origin.z.saturating_sub(radius);
    let max_z = origin.z.saturating_add(radius);
    for z in min_z..=max_z {
        for x in min_x..=max_x {
            insert_column(
                &mut desired,
                x,
                z,
                clamps.vertical_min_chunk,
                clamps.vertical_max_chunk,
            );
        }
    }
    if let Some((ahead_x, ahead_z)) =
        look_ahead_column(origin, clamps.prefetch_distance(), look_ahead)
    {
        insert_column(
            &mut desired,
            ahead_x,
            ahead_z,
            clamps.vertical_min_chunk,
            clamps.vertical_max_chunk,
        );
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
        let look = i64::from(chunk.x.saturating_sub(origin.x))
            .saturating_mul(i64::from(look_ahead[0]))
            .saturating_add(
                i64::from(chunk.z.saturating_sub(origin.z))
                    .saturating_mul(i64::from(look_ahead[1])),
            );
        (distance, std::cmp::Reverse(look), chunk.x, chunk.y, chunk.z)
    });
    ordered
}

fn clamped_interest_radius_for(
    limits: PlayableWorldHardLimitsV1,
    requested_view: u32,
    vertical_layers: u32,
) -> u32 {
    let requested = requested_view
        .min(limits.view_distance_chunks)
        .min(limits.generation_radius_chunks);
    resident_budget_radius(requested, vertical_layers, limits.max_resident_chunks)
}

fn stream_distances(
    limits: PlayableWorldHardLimitsV1,
    requested_render: u32,
    vertical_layers: u32,
) -> Option<StreamDistances> {
    let requested_render =
        requested_render.clamp(MIN_VIEW_DISTANCE_CHUNKS, AUTHORED_MAX_VIEW_DISTANCE_CHUNKS);
    let requested_cap = limits.view_distance_chunks.max(1);
    let admitted_render = requested_render.min(requested_cap);
    let effective_render = clamped_interest_radius_for(limits, admitted_render, vertical_layers);
    let generation_cap = limits.generation_radius_chunks.max(1).min(requested_cap);
    let prefetch = effective_render
        .saturating_add(1)
        .min(generation_cap)
        .max(effective_render);
    Some(StreamDistances {
        requested_render: RequestedRenderDistanceChunksV1::new(requested_render)?,
        admitted_render: AdmittedRenderDistanceChunksV1::new(admitted_render)?,
        effective_render: EffectiveRenderDistanceChunksV1::new(effective_render)?,
        simulation: SimulationDistanceChunksV1::new(effective_render)?,
        resident: ResidentDistanceChunksV1::new(effective_render)?,
        prefetch: PrefetchDistanceChunksV1::new(prefetch)?,
    })
}

fn resident_budget_radius(
    maximum_radius: u32,
    vertical_layers: u32,
    max_resident_chunks: u32,
) -> u32 {
    let mut radius = maximum_radius;
    while radius > 0 {
        if interest_volume(radius, vertical_layers, true) <= max_resident_chunks {
            return radius;
        }
        radius -= 1;
    }
    radius
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

fn is_core_chunk(chunk: ChunkCoordinate, origin: ChunkCoordinate, clamps: StreamClamps) -> bool {
    chunk.y >= clamps.vertical_min_chunk
        && chunk.y <= clamps.vertical_max_chunk
        && chebyshev_xz(chunk, origin) <= clamps.effective_render_distance()
}

fn is_prefetch_chunk(
    chunk: ChunkCoordinate,
    origin: ChunkCoordinate,
    clamps: StreamClamps,
    look_ahead: [i32; 2],
) -> bool {
    look_ahead_column(origin, clamps.prefetch_distance(), look_ahead) == Some((chunk.x, chunk.z))
        && chunk.y >= clamps.vertical_min_chunk
        && chunk.y <= clamps.vertical_max_chunk
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
        ChunkLifecycle, InterestClass, LOOK_AHEAD_EXPIRY_TICKS, MIN_RESIDENCY_TICKS,
        RETAIN_GRACE_TICKS, StreamClamps, ViewDistanceClampReasonV1, chebyshev_xz, desired_chunks,
        interest_class, look_ahead_axis, prioritize_chunks, retain_protected, sticky_look_ahead,
    };
    use latticeaxiom_compose::PlayableWorldHardLimitsV1;
    use latticeaxiom_storage::ChunkCoordinate;
    use latticeaxiom_worldgen::{
        ORIGIN_NEIGHBORHOOD_CHUNK_COORDINATES_V1, PlanningCellCoordinateV1, WorldgenConfigV1,
    };
    use std::collections::BTreeSet;

    fn clamps() -> StreamClamps {
        let limits = PlayableWorldHardLimitsV1::new(2, 2, 64, 4, 2).expect("nonzero clamps");
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
        assert_eq!(clamps.effective_render_distance(), 1);
        assert_eq!(clamps.simulation_distance(), 1);
        assert_eq!(clamps.resident_distance(), 1);
        assert_eq!(clamps.prefetch_distance(), 2);
        assert_eq!(clamps.admitted_render_distance(), 2);
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
        let limits = PlayableWorldHardLimitsV1::new(2, 2, 1, 1, 1).expect("nonzero clamps");
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
    fn requested_view_distance_is_admitted_and_budgeted() {
        let limits = PlayableWorldHardLimitsV1::new(4, 4, 128, 8, 4).expect("nonzero clamps");
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
            .set_requested_view_distance(1)
            .expect("distance contract remains valid");
        assert_eq!(clamps.admitted_render_distance(), 2);
        assert_eq!(clamps.effective_render_distance(), 2);
        clamps
            .set_requested_view_distance(4)
            .expect("distance contract remains valid");
        assert_eq!(clamps.admitted_render_distance(), 4);
        assert_eq!(clamps.effective_render_distance(), 2);
        clamps
            .set_requested_view_distance(0)
            .expect("distance contract remains valid");
        assert_eq!(clamps.admitted_render_distance(), 2);
        clamps
            .set_requested_view_distance(99)
            .expect("distance contract remains valid");
        assert_eq!(clamps.admitted_render_distance(), 4);
    }

    #[test]
    fn desktop_request_cap_reports_resident_budget_clamp() {
        let limits = PlayableWorldHardLimitsV1::new(32, 32, 405, 8, 4)
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
            .set_requested_view_distance(32)
            .expect("distance contract remains valid");

        let status = clamps.view_distance_status();
        assert_eq!(status.requested_render_distance().chunks(), 32);
        assert_eq!(status.admitted_render_distance().chunks(), 32);
        assert_eq!(status.requested_cap(), 32);
        assert_eq!(status.generation_cap(), 32);
        assert_eq!(status.resident_budget_cap(), 4);
        assert_eq!(status.effective_render_distance().chunks(), 4);
        assert_eq!(status.simulation_distance().chunks(), 4);
        assert_eq!(status.resident_distance().chunks(), 4);
        assert_eq!(status.prefetch_distance().chunks(), 5);
        assert_eq!(
            status.clamp_reason(),
            Some(ViewDistanceClampReasonV1::ResidentBudget)
        );
        assert!(status.is_clamped());

        let desired = desired_chunks(
            ChunkCoordinate::new(0, 0, 0),
            clamps,
            [1, 0],
            &BTreeSet::new(),
        );
        assert_eq!(desired.len(), 328);
        assert!(desired.len() <= clamps.max_resident());
    }

    #[test]
    fn host_cap_changes_admitted_and_effective_without_mutating_requested_draft() {
        let limits = PlayableWorldHardLimitsV1::new(4, 4, 128, 8, 4).expect("nonzero clamps");
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
            .set_requested_view_distance(requested_draft)
            .expect("distance contract remains valid");
        let status = clamps.view_distance_status();

        assert_eq!(requested_draft, 99);
        assert_eq!(status.requested_render_distance().chunks(), 32);
        assert_eq!(status.admitted_render_distance().chunks(), 4);
        assert_eq!(status.effective_render_distance().chunks(), 2);
        assert_eq!(status.requested_cap(), 4);
        assert!(status.is_clamped());
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
