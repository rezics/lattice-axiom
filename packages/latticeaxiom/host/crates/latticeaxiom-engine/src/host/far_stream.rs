//! Bounded hierarchical scheduling for presentation-only far-terrain tiles.

use std::{
    cmp::Reverse,
    collections::{BTreeMap, BTreeSet, VecDeque},
    mem,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

use bevy::tasks::{AsyncComputeTaskPool, Task, block_on};
use latticeaxiom_runtime_contracts::FarTerrainQualityV1;
use latticeaxiom_storage::ChunkCoordinate;
use latticeaxiom_worldgen::{
    FarTerrainLodLevelV1, FarTerrainProceduralProvenanceV1, FarTerrainSourceProvenanceV1,
    FarTerrainSurfaceSampleV1, FarTerrainSurfaceSourceV1, FarTerrainTileAddressV1,
    FarTerrainTileCoordinateV1, FarTerrainTileV1, GenerationPlanV1, MAX_FAR_TERRAIN_LOD_LEVEL_V1,
    WorldgenError, WorldgenResult, build_far_terrain_tile_v1, far_terrain_edit_invalidation_v1,
};

/// Maximum queued tile inputs retained before Bevy task admission.
pub const FAR_TERRAIN_PENDING_CAP: usize = 128;
/// Maximum CPU-heavy far tile jobs allowed at once.
pub const FAR_TERRAIN_IN_FLIGHT_CAP: usize = 2;
/// Maximum ready far tiles retained by one production host.
pub const FAR_TERRAIN_READY_CAP: usize = 1_024;

/// Bounded occupancy and cancellation evidence for the far-terrain lane.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct FarTerrainQueueSnapshotV1 {
    /// Tiles in the current hierarchical interest set.
    pub desired: usize,
    /// Desired tiles suppressed because authoritative edits make procedural data stale.
    pub blocked_by_edits: usize,
    /// Inputs waiting for Bevy worker admission.
    pub pending: usize,
    /// Bevy tasks currently owned by the far-terrain lane.
    pub in_flight: usize,
    /// Completed results waiting for stable sequence publication.
    pub waiting_to_apply: usize,
    /// Current ready presentation tiles.
    pub ready: usize,
    /// Conservative retained vector bytes across ready tiles.
    pub ready_vector_bytes: u64,
    /// Interest generation used to reject stale results.
    pub interest_generation: u64,
    /// Pending or in-flight jobs cancelled after interest changed.
    pub cancel_requests: u64,
    /// Completed or waiting results discarded after their interest became stale.
    pub stale_results: u64,
    /// Pump opportunities deliberately yielded to near worldgen or derived work.
    pub near_work_reservations: u64,
    /// Largest contiguous near-plus-far radius ready for presentation.
    pub presented_distance_chunks: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct FarTerrainInterestKey {
    origin_x: i32,
    origin_z: i32,
    full_detail_distance: u32,
    target_render_distance: u32,
    quality: FarTerrainQualityV1,
    edited_revision: u64,
}

#[derive(Clone)]
struct FarTerrainInput {
    sequence: u64,
    interest_generation: u64,
    address: FarTerrainTileAddressV1,
    base_tile_edge_voxels: u16,
    plan: Arc<GenerationPlanV1>,
    provenance: FarTerrainSourceProvenanceV1,
    cancellation_generation: Arc<AtomicU64>,
}

struct FarTerrainComputed {
    sequence: u64,
    interest_generation: u64,
    address: FarTerrainTileAddressV1,
    result: WorldgenResult<FarTerrainTileV1>,
}

struct InFlightFarTerrain {
    address: FarTerrainTileAddressV1,
    task: Task<FarTerrainComputed>,
}

/// Host-owned hierarchical far-terrain state. Tiles never enter collision,
/// simulation, mining, or persistence paths.
pub(super) struct FarTerrainStream {
    plan: Arc<GenerationPlanV1>,
    provenance: FarTerrainSourceProvenanceV1,
    cancellation_generation: Arc<AtomicU64>,
    base_tile_edge_voxels: u16,
    quality: FarTerrainQualityV1,
    interest_key: Option<FarTerrainInterestKey>,
    origin: ChunkCoordinate,
    full_detail_distance: u32,
    target_render_distance: u32,
    desired: BTreeSet<FarTerrainTileAddressV1>,
    desired_order: Arc<[FarTerrainTileAddressV1]>,
    blocked_by_edits: BTreeSet<FarTerrainTileAddressV1>,
    pending: VecDeque<FarTerrainInput>,
    pending_addresses: BTreeSet<FarTerrainTileAddressV1>,
    in_flight: Vec<InFlightFarTerrain>,
    waiting: BTreeMap<u64, FarTerrainComputed>,
    ready: BTreeMap<FarTerrainTileAddressV1, Arc<FarTerrainTileV1>>,
    next_sequence: u64,
    next_apply_sequence: u64,
    interest_generation: u64,
    cancel_requests: u64,
    stale_results: u64,
    near_work_reservations: u64,
    presented_distance_chunks: u32,
}

impl FarTerrainStream {
    pub(super) fn new(
        plan: Arc<GenerationPlanV1>,
        base_tile_edge_voxels: u16,
        quality: FarTerrainQualityV1,
    ) -> Self {
        let provenance = FarTerrainSourceProvenanceV1::Procedural(
            FarTerrainProceduralProvenanceV1::from_plan(&plan),
        );
        Self {
            plan,
            provenance,
            cancellation_generation: Arc::new(AtomicU64::new(0)),
            base_tile_edge_voxels,
            quality,
            interest_key: None,
            origin: ChunkCoordinate::new(0, 0, 0),
            full_detail_distance: 1,
            target_render_distance: 1,
            desired: BTreeSet::new(),
            desired_order: Arc::from([]),
            blocked_by_edits: BTreeSet::new(),
            pending: VecDeque::new(),
            pending_addresses: BTreeSet::new(),
            in_flight: Vec::new(),
            waiting: BTreeMap::new(),
            ready: BTreeMap::new(),
            next_sequence: 0,
            next_apply_sequence: 0,
            interest_generation: 0,
            cancel_requests: 0,
            stale_results: 0,
            near_work_reservations: 0,
            presented_distance_chunks: 1,
        }
    }

    pub(super) fn quality(&self) -> FarTerrainQualityV1 {
        self.quality
    }

    pub(super) fn set_quality(&mut self, quality: FarTerrainQualityV1) {
        self.quality = quality;
    }

    pub(super) fn reconcile(
        &mut self,
        origin: ChunkCoordinate,
        full_detail_distance: u32,
        target_render_distance: u32,
        edited_revision: u64,
        edited_chunks: &BTreeSet<ChunkCoordinate>,
        look_ahead: [i32; 2],
    ) -> WorldgenResult<()> {
        let key = FarTerrainInterestKey {
            origin_x: origin.x,
            origin_z: origin.z,
            full_detail_distance,
            target_render_distance,
            quality: self.quality,
            edited_revision,
        };
        if self.interest_key == Some(key) {
            return Ok(());
        }

        let desired = select_far_terrain_tiles(
            origin,
            full_detail_distance,
            target_render_distance,
            self.quality,
        )?;
        if desired.len() > FAR_TERRAIN_READY_CAP {
            return Err(WorldgenError::InvalidFarTerrainTile {
                field: "desired_tile_count",
                reason: format!(
                    "hierarchical interest contains {} tiles, exceeding ready cap {FAR_TERRAIN_READY_CAP}",
                    desired.len()
                ),
            });
        }
        let blocked_by_edits = blocked_tiles_for_edits(
            &desired,
            edited_chunks,
            origin,
            target_render_distance,
            self.base_tile_edge_voxels,
        )?;
        let desired_order = prioritize_far_tiles(&desired, origin, look_ahead);

        self.cancel_requests = self.cancel_requests.saturating_add(
            u64::try_from(self.pending.len().saturating_add(self.in_flight.len()))
                .unwrap_or(u64::MAX),
        );
        self.stale_results = self
            .stale_results
            .saturating_add(u64::try_from(self.waiting.len()).unwrap_or(u64::MAX));
        self.pending.clear();
        self.pending_addresses.clear();
        self.in_flight.clear();
        self.waiting.clear();
        self.interest_generation = self.interest_generation.saturating_add(1);
        self.cancellation_generation
            .store(self.interest_generation, Ordering::Release);
        self.next_sequence = 0;
        self.next_apply_sequence = 0;
        self.origin = origin;
        self.full_detail_distance = full_detail_distance;
        self.target_render_distance = target_render_distance;
        self.desired = desired;
        self.desired_order = Arc::from(desired_order);
        self.blocked_by_edits = blocked_by_edits;
        self.ready.retain(|address, _| {
            self.desired.contains(address) && !self.blocked_by_edits.contains(address)
        });
        self.interest_key = Some(key);
        self.refill_pending();
        self.refresh_presented_distance();
        Ok(())
    }

    pub(super) fn poll_completed(&mut self) -> WorldgenResult<()> {
        let tasks = mem::take(&mut self.in_flight);
        let mut remaining = Vec::new();
        for task in tasks {
            if task.task.is_finished() {
                let completed = block_on(task.task);
                self.waiting.insert(completed.sequence, completed);
            } else {
                remaining.push(task);
            }
        }
        self.in_flight = remaining;
        self.publish_waiting()?;
        self.refill_pending();
        self.refresh_presented_distance();
        Ok(())
    }

    pub(super) fn spawn(&mut self, pool: &'static AsyncComputeTaskPool, maximum_new: usize) {
        let available = FAR_TERRAIN_IN_FLIGHT_CAP.saturating_sub(self.in_flight.len());
        let count = available.min(maximum_new).min(self.pending.len());
        for _ in 0..count {
            let Some(input) = self.pending.pop_front() else {
                break;
            };
            self.pending_addresses.remove(&input.address);
            let address = input.address;
            self.in_flight.push(InFlightFarTerrain {
                address,
                task: pool.spawn(async move { compute_far_terrain(input) }),
            });
        }
    }

    pub(super) fn record_near_work_reservation(&mut self) {
        if !self.pending.is_empty() {
            self.near_work_reservations = self.near_work_reservations.saturating_add(1);
        }
    }

    pub(super) fn in_flight_count(&self) -> usize {
        self.in_flight.len()
    }

    pub(super) fn presented_distance_chunks(&self) -> u32 {
        self.presented_distance_chunks
    }

    pub(super) fn ready_tiles(&self) -> Vec<Arc<FarTerrainTileV1>> {
        self.ready.values().cloned().collect()
    }

    pub(super) fn snapshot(&self) -> FarTerrainQueueSnapshotV1 {
        FarTerrainQueueSnapshotV1 {
            desired: self.desired.len(),
            blocked_by_edits: self.blocked_by_edits.len(),
            pending: self.pending.len(),
            in_flight: self.in_flight.len(),
            waiting_to_apply: self.waiting.len(),
            ready: self.ready.len(),
            ready_vector_bytes: self.ready.values().fold(0_u64, |total, tile| {
                total
                    .saturating_add(u64::try_from(tile.retained_vector_bytes()).unwrap_or(u64::MAX))
            }),
            interest_generation: self.interest_generation,
            cancel_requests: self.cancel_requests,
            stale_results: self.stale_results,
            near_work_reservations: self.near_work_reservations,
            presented_distance_chunks: self.presented_distance_chunks,
        }
    }

    fn publish_waiting(&mut self) -> WorldgenResult<()> {
        while let Some(completed) = self.waiting.remove(&self.next_apply_sequence) {
            self.next_apply_sequence = self.next_apply_sequence.saturating_add(1);
            if completed.interest_generation != self.interest_generation
                || !self.desired.contains(&completed.address)
                || self.blocked_by_edits.contains(&completed.address)
            {
                self.stale_results = self.stale_results.saturating_add(1);
                continue;
            }
            let tile = completed.result?;
            self.ready.insert(completed.address, Arc::new(tile));
        }
        Ok(())
    }

    fn refill_pending(&mut self) {
        if self.pending.len() >= FAR_TERRAIN_PENDING_CAP {
            return;
        }
        let in_flight_addresses = self
            .in_flight
            .iter()
            .map(|task| task.address)
            .collect::<BTreeSet<_>>();
        let waiting_addresses = self
            .waiting
            .values()
            .map(|completed| completed.address)
            .collect::<BTreeSet<_>>();
        for &address in self.desired_order.iter() {
            if self.pending.len() >= FAR_TERRAIN_PENDING_CAP {
                break;
            }
            if self.ready.contains_key(&address)
                || self.blocked_by_edits.contains(&address)
                || self.pending_addresses.contains(&address)
                || in_flight_addresses.contains(&address)
                || waiting_addresses.contains(&address)
            {
                continue;
            }
            let input = FarTerrainInput {
                sequence: self.next_sequence,
                interest_generation: self.interest_generation,
                address,
                base_tile_edge_voxels: self.base_tile_edge_voxels,
                plan: Arc::clone(&self.plan),
                provenance: self.provenance.clone(),
                cancellation_generation: Arc::clone(&self.cancellation_generation),
            };
            self.next_sequence = self.next_sequence.saturating_add(1);
            self.pending.push_back(input);
            self.pending_addresses.insert(address);
        }
    }

    fn refresh_presented_distance(&mut self) {
        let mut presented = self.full_detail_distance.min(self.target_render_distance);
        for radius in presented.saturating_add(1)..=self.target_render_distance {
            let missing = self.desired.iter().any(|address| {
                tile_intersects_radius(*address, self.origin, radius)
                    && !self.ready.contains_key(address)
            });
            if missing {
                break;
            }
            presented = radius;
        }
        self.presented_distance_chunks = presented;
    }
}

fn compute_far_terrain(input: FarTerrainInput) -> FarTerrainComputed {
    let source = CancellableFarTerrainSource {
        plan: Arc::clone(&input.plan),
        cancellation_generation: input.cancellation_generation,
        expected_generation: input.interest_generation,
    };
    let result = build_far_terrain_tile_v1(
        &source,
        input.provenance,
        input.address,
        input.base_tile_edge_voxels,
    );
    FarTerrainComputed {
        sequence: input.sequence,
        interest_generation: input.interest_generation,
        address: input.address,
        result,
    }
}

struct CancellableFarTerrainSource {
    plan: Arc<GenerationPlanV1>,
    cancellation_generation: Arc<AtomicU64>,
    expected_generation: u64,
}

impl FarTerrainSurfaceSourceV1 for CancellableFarTerrainSource {
    fn sample_far_terrain_surface(
        &self,
        world_x: i64,
        world_z: i64,
    ) -> WorldgenResult<FarTerrainSurfaceSampleV1> {
        if self.cancellation_generation.load(Ordering::Acquire) != self.expected_generation {
            return Err(WorldgenError::InvalidFarTerrainTile {
                field: "interest_generation",
                reason: "far-terrain build was cancelled after interest changed".to_owned(),
            });
        }
        self.plan.far_terrain_surface_sample(world_x, world_z)
    }
}

fn select_far_terrain_tiles(
    origin: ChunkCoordinate,
    full_detail_distance: u32,
    target_render_distance: u32,
    quality: FarTerrainQualityV1,
) -> WorldgenResult<BTreeSet<FarTerrainTileAddressV1>> {
    let mut selected = BTreeSet::new();
    if target_render_distance <= full_detail_distance {
        return Ok(selected);
    }
    let root_lod = FarTerrainLodLevelV1::new(MAX_FAR_TERRAIN_LOD_LEVEL_V1).ok_or(
        WorldgenError::InvalidFarTerrainTile {
            field: "maximum_lod",
            reason: "published maximum LOD is not constructible".to_owned(),
        },
    )?;
    let root_span = i64::from(root_lod.sample_step_voxels());
    let target = i64::from(target_render_distance);
    let origin_x = i64::from(origin.x);
    let origin_z = i64::from(origin.z);
    let minimum_x = origin_x.saturating_sub(target).div_euclid(root_span);
    let maximum_x = origin_x.saturating_add(target).div_euclid(root_span);
    let minimum_z = origin_z.saturating_sub(target).div_euclid(root_span);
    let maximum_z = origin_z.saturating_add(target).div_euclid(root_span);
    for tile_z in minimum_z..=maximum_z {
        for tile_x in minimum_x..=maximum_x {
            let coordinate = FarTerrainTileCoordinateV1::new(
                i32::try_from(tile_x).map_err(|_| WorldgenError::ArithmeticOverflow {
                    operation: "far-terrain root tile X",
                })?,
                i32::try_from(tile_z).map_err(|_| WorldgenError::ArithmeticOverflow {
                    operation: "far-terrain root tile Z",
                })?,
            );
            select_tile_recursive(
                FarTerrainTileAddressV1::new(coordinate, root_lod),
                origin,
                full_detail_distance,
                target_render_distance,
                quality,
                &mut selected,
            )?;
        }
    }
    Ok(selected)
}

fn select_tile_recursive(
    address: FarTerrainTileAddressV1,
    origin: ChunkCoordinate,
    full_detail_distance: u32,
    target_render_distance: u32,
    quality: FarTerrainQualityV1,
    selected: &mut BTreeSet<FarTerrainTileAddressV1>,
) -> WorldgenResult<()> {
    if !tile_intersects_radius(address, origin, target_render_distance)
        || tile_fully_inside_near_square(address, origin, full_detail_distance)
    {
        return Ok(());
    }
    let lod = address.lod().get();
    if lod == 0 {
        selected.insert(address);
        return Ok(());
    }
    let minimum_distance = integer_sqrt(tile_minimum_distance_squared(address, origin));
    let desired_lod = desired_lod_for_distance(minimum_distance, full_detail_distance, quality);
    let must_split_near_boundary =
        tile_intersects_near_square(address, origin, full_detail_distance);
    if lod <= desired_lod && !must_split_near_boundary {
        selected.insert(address);
        return Ok(());
    }
    let child_lod = FarTerrainLodLevelV1::new(lod.saturating_sub(1)).ok_or(
        WorldgenError::InvalidFarTerrainTile {
            field: "child_lod",
            reason: "hierarchical child LOD is invalid".to_owned(),
        },
    )?;
    let parent = address.coordinate();
    for offset_z in 0..=1_i64 {
        for offset_x in 0..=1_i64 {
            let child_x = i64::from(parent.x())
                .checked_mul(2)
                .and_then(|value| value.checked_add(offset_x))
                .ok_or(WorldgenError::ArithmeticOverflow {
                    operation: "far-terrain child tile X",
                })?;
            let child_z = i64::from(parent.z())
                .checked_mul(2)
                .and_then(|value| value.checked_add(offset_z))
                .ok_or(WorldgenError::ArithmeticOverflow {
                    operation: "far-terrain child tile Z",
                })?;
            select_tile_recursive(
                FarTerrainTileAddressV1::new(
                    FarTerrainTileCoordinateV1::new(
                        i32::try_from(child_x).map_err(|_| WorldgenError::ArithmeticOverflow {
                            operation: "far-terrain child tile X conversion",
                        })?,
                        i32::try_from(child_z).map_err(|_| WorldgenError::ArithmeticOverflow {
                            operation: "far-terrain child tile Z conversion",
                        })?,
                    ),
                    child_lod,
                ),
                origin,
                full_detail_distance,
                target_render_distance,
                quality,
                selected,
            )?;
        }
    }
    Ok(())
}

fn desired_lod_for_distance(
    minimum_distance: u32,
    full_detail_distance: u32,
    quality: FarTerrainQualityV1,
) -> u8 {
    let (lod0_end, lod1_end, lod2_end) = match quality {
        FarTerrainQualityV1::Performance => (full_detail_distance.saturating_add(2), 12, 24),
        FarTerrainQualityV1::Balanced => (full_detail_distance.saturating_add(4), 18, 28),
        FarTerrainQualityV1::Quality => (full_detail_distance.saturating_add(6), 22, 32),
    };
    if minimum_distance <= lod0_end {
        0
    } else if minimum_distance <= lod1_end {
        1
    } else if minimum_distance <= lod2_end {
        2
    } else {
        3
    }
}

fn prioritize_far_tiles(
    desired: &BTreeSet<FarTerrainTileAddressV1>,
    origin: ChunkCoordinate,
    look_ahead: [i32; 2],
) -> Vec<FarTerrainTileAddressV1> {
    let mut ordered = desired.iter().copied().collect::<Vec<_>>();
    ordered.sort_by_key(|address| {
        let tile_midpoint = tile_center_twice(*address);
        let direction = tile_midpoint[0]
            .saturating_sub(i64::from(origin.x).saturating_mul(2))
            .saturating_mul(i64::from(look_ahead[0]))
            .saturating_add(
                tile_midpoint[1]
                    .saturating_sub(i64::from(origin.z).saturating_mul(2))
                    .saturating_mul(i64::from(look_ahead[1])),
            );
        (
            tile_minimum_distance_squared(*address, origin),
            Reverse(direction),
            address.lod().get(),
            address.coordinate().x(),
            address.coordinate().z(),
        )
    });
    ordered
}

fn blocked_tiles_for_edits(
    desired: &BTreeSet<FarTerrainTileAddressV1>,
    edited_chunks: &BTreeSet<ChunkCoordinate>,
    origin: ChunkCoordinate,
    target_render_distance: u32,
    base_tile_edge_voxels: u16,
) -> WorldgenResult<BTreeSet<FarTerrainTileAddressV1>> {
    let mut blocked = BTreeSet::new();
    let maximum_lod = FarTerrainLodLevelV1::new(MAX_FAR_TERRAIN_LOD_LEVEL_V1).ok_or(
        WorldgenError::InvalidFarTerrainTile {
            field: "maximum_lod",
            reason: "published maximum LOD is not constructible".to_owned(),
        },
    )?;
    let margin = target_render_distance.saturating_add(1);
    for chunk in edited_chunks {
        if chunk.x.abs_diff(origin.x) > margin || chunk.z.abs_diff(origin.z) > margin {
            continue;
        }
        let minimum_x = i64::from(chunk.x).saturating_mul(i64::from(base_tile_edge_voxels));
        let minimum_z = i64::from(chunk.z).saturating_mul(i64::from(base_tile_edge_voxels));
        let maximum_x = minimum_x.saturating_add(i64::from(base_tile_edge_voxels));
        let maximum_z = minimum_z.saturating_add(i64::from(base_tile_edge_voxels));
        for x in [minimum_x, maximum_x] {
            for z in [minimum_z, maximum_z] {
                blocked.extend(
                    far_terrain_edit_invalidation_v1(x, z, base_tile_edge_voxels, maximum_lod)?
                        .into_iter()
                        .filter(|address| desired.contains(address)),
                );
            }
        }
    }
    Ok(blocked)
}

fn tile_intersects_radius(
    address: FarTerrainTileAddressV1,
    origin: ChunkCoordinate,
    radius: u32,
) -> bool {
    tile_minimum_distance_squared(address, origin)
        <= u64::from(radius).saturating_mul(u64::from(radius))
}

fn tile_minimum_distance_squared(address: FarTerrainTileAddressV1, origin: ChunkCoordinate) -> u64 {
    let [minimum_x, maximum_x, minimum_z, maximum_z] = tile_chunk_bounds(address);
    let origin_x = i64::from(origin.x);
    let origin_z = i64::from(origin.z);
    let nearest_x = origin_x.clamp(minimum_x, maximum_x);
    let nearest_z = origin_z.clamp(minimum_z, maximum_z);
    let dx = origin_x.abs_diff(nearest_x);
    let dz = origin_z.abs_diff(nearest_z);
    dx.saturating_mul(dx).saturating_add(dz.saturating_mul(dz))
}

fn tile_fully_inside_near_square(
    address: FarTerrainTileAddressV1,
    origin: ChunkCoordinate,
    radius: u32,
) -> bool {
    let [minimum_x, maximum_x, minimum_z, maximum_z] = tile_chunk_bounds(address);
    let radius = i64::from(radius);
    let near_minimum_x = i64::from(origin.x).saturating_sub(radius);
    let near_maximum_x = i64::from(origin.x).saturating_add(radius);
    let near_minimum_z = i64::from(origin.z).saturating_sub(radius);
    let near_maximum_z = i64::from(origin.z).saturating_add(radius);
    minimum_x >= near_minimum_x
        && maximum_x <= near_maximum_x
        && minimum_z >= near_minimum_z
        && maximum_z <= near_maximum_z
}

fn tile_intersects_near_square(
    address: FarTerrainTileAddressV1,
    origin: ChunkCoordinate,
    radius: u32,
) -> bool {
    let [minimum_x, maximum_x, minimum_z, maximum_z] = tile_chunk_bounds(address);
    let radius = i64::from(radius);
    let near_minimum_x = i64::from(origin.x).saturating_sub(radius);
    let near_maximum_x = i64::from(origin.x).saturating_add(radius);
    let near_minimum_z = i64::from(origin.z).saturating_sub(radius);
    let near_maximum_z = i64::from(origin.z).saturating_add(radius);
    maximum_x >= near_minimum_x
        && minimum_x <= near_maximum_x
        && maximum_z >= near_minimum_z
        && minimum_z <= near_maximum_z
}

fn tile_chunk_bounds(address: FarTerrainTileAddressV1) -> [i64; 4] {
    let span = i64::from(address.lod().sample_step_voxels());
    let minimum_x = i64::from(address.coordinate().x()).saturating_mul(span);
    let minimum_z = i64::from(address.coordinate().z()).saturating_mul(span);
    [
        minimum_x,
        minimum_x.saturating_add(span.saturating_sub(1)),
        minimum_z,
        minimum_z.saturating_add(span.saturating_sub(1)),
    ]
}

fn tile_center_twice(address: FarTerrainTileAddressV1) -> [i64; 2] {
    let [minimum_x, maximum_x, minimum_z, maximum_z] = tile_chunk_bounds(address);
    [
        minimum_x.saturating_add(maximum_x),
        minimum_z.saturating_add(maximum_z),
    ]
}

fn integer_sqrt(value: u64) -> u32 {
    let mut remainder = value;
    let mut result = 0_u64;
    let mut bit = 1_u64 << 62;
    while bit > remainder {
        bit >>= 2;
    }
    while bit != 0 {
        if remainder >= result.saturating_add(bit) {
            remainder = remainder.saturating_sub(result.saturating_add(bit));
            result = result.wrapping_shr(1).saturating_add(bit);
        } else {
            result >>= 1;
        }
        bit >>= 2;
    }
    u32::try_from(result).unwrap_or(u32::MAX)
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn hierarchical_selection_covers_circle_once_and_excludes_near_square() {
        for origin in [
            ChunkCoordinate::new(0, 0, 0),
            ChunkCoordinate::new(-17, 3, -9),
        ] {
            let desired = select_far_terrain_tiles(origin, 6, 32, FarTerrainQualityV1::Balanced)
                .expect("bounded hierarchy builds");
            let mut covered = BTreeSet::new();
            for address in desired {
                let [minimum_x, maximum_x, minimum_z, maximum_z] = tile_chunk_bounds(address);
                for z in minimum_z..=maximum_z {
                    for x in minimum_x..=maximum_x {
                        assert!(covered.insert((x, z)), "hierarchical tiles cannot overlap");
                        assert!(
                            x.abs_diff(i64::from(origin.x)) > 6
                                || z.abs_diff(i64::from(origin.z)) > 6,
                            "far hierarchy cannot cover the full-detail square"
                        );
                    }
                }
            }
            for dz in -32_i64..=32 {
                for dx in -32_i64..=32 {
                    if dx.saturating_mul(dx).saturating_add(dz.saturating_mul(dz)) > 32 * 32
                        || (dx.abs() <= 6 && dz.abs() <= 6)
                    {
                        continue;
                    }
                    assert!(
                        covered.contains(&(
                            i64::from(origin.x).saturating_add(dx),
                            i64::from(origin.z).saturating_add(dz)
                        )),
                        "every chunk center inside the requested circle needs one far owner"
                    );
                }
            }
        }
    }

    #[test]
    fn every_quality_stays_inside_the_ready_cap_at_maximum_distance() {
        for quality in [
            FarTerrainQualityV1::Performance,
            FarTerrainQualityV1::Balanced,
            FarTerrainQualityV1::Quality,
        ] {
            let selected =
                select_far_terrain_tiles(ChunkCoordinate::new(-1, 0, -1), 6, 32, quality)
                    .expect("maximum authored interest remains bounded");
            assert!(
                selected.len() <= FAR_TERRAIN_READY_CAP,
                "{quality:?} selected {} tiles",
                selected.len()
            );
        }
    }

    #[test]
    fn priority_is_stable_and_look_ahead_only_breaks_equal_distance() {
        let origin = ChunkCoordinate::new(0, 0, 0);
        let desired = [
            FarTerrainTileAddressV1::new(
                FarTerrainTileCoordinateV1::new(-8, 0),
                FarTerrainLodLevelV1::new(0).expect("LOD zero"),
            ),
            FarTerrainTileAddressV1::new(
                FarTerrainTileCoordinateV1::new(8, 0),
                FarTerrainLodLevelV1::new(0).expect("LOD zero"),
            ),
            FarTerrainTileAddressV1::new(
                FarTerrainTileCoordinateV1::new(0, 9),
                FarTerrainLodLevelV1::new(0).expect("LOD zero"),
            ),
        ]
        .into_iter()
        .collect();
        let east = prioritize_far_tiles(&desired, origin, [1, 0]);
        assert_eq!(east[0].coordinate().x(), 8);
        assert_eq!(east[2].coordinate().z(), 9);
        assert_eq!(east, prioritize_far_tiles(&desired, origin, [1, 0]));
    }

    #[test]
    fn edited_chunk_blocks_every_selected_ancestor_and_shared_border() {
        let origin = ChunkCoordinate::new(0, 0, 0);
        let desired = select_far_terrain_tiles(origin, 2, 16, FarTerrainQualityV1::Balanced)
            .expect("interest builds");
        let edited = [ChunkCoordinate::new(8, 0, 0)].into_iter().collect();
        let blocked = blocked_tiles_for_edits(&desired, &edited, origin, 16, 32)
            .expect("edit invalidation is bounded");
        assert!(!blocked.is_empty());
        assert!(blocked.iter().all(|address| desired.contains(address)));
    }
}
