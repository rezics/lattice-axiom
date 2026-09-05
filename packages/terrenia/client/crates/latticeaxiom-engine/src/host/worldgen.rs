//! Sparse production-plan streaming for the production host.
//!
//! Chunks are generated from the compiled [`GenerationPlanV1`], not from the
//! D4 four-chunk origin neighborhood. Spawn is the validated surface cell.
//! The current revision attaches package-owned semantic morphology, cave
//! topology, and hydrology occupancy. This module does not open a writer or
//! compile a second Atlas.

use std::collections::BTreeSet;
use std::num::NonZeroU32;

use bevy::prelude::Vec3;
use bevy::tasks::AsyncComputeTaskPool;
use latticeaxiom_compose::{LockedPackage, PlayableWorldHardLimitsV1};
use latticeaxiom_core::{CanonicalHash, StableId};
use latticeaxiom_player::PlayerMovementProfileV1;
use latticeaxiom_runtime_contracts::{
    AUTHORED_MAX_RENDER_DISTANCE_CHUNKS, CaveConnectivityInspectFactsV1, CaveInspectAxisV1,
    CaveOwnershipInspectFactsV1, CaveSdfInspectFactsV1, EngineEpoch, EntranceInspectFactsV1,
    FluidDecisionInspectFactsV1, FluidOccupancyInspectV1, GeologyInspectFactsV1,
    PlanningSeamInspectFactsV1, PortalInspectFactsV1, PortalInspectFluidV1, ResourceInspectFactsV1,
    RiverInspectFactsV1, SpawnInspectFactsV1, TerritoryInspectFactsV1, VegetationInspectFactsV1,
    WorldEpoch, WorldgenInspectBodyV1, WorldgenInspectCollectionV1, WorldgenInspectKindV1,
    WorldgenInspectLimits, WorldgenInspectQueryV1, WorldgenInspectRecordV1,
    WorldgenInspectReportV1, WorldgenInspectSamplesV1, compile_worldgen_inspect_report,
};
use latticeaxiom_storage::ChunkCoordinate;
#[cfg(test)]
use latticeaxiom_terrenia_worldgen::surface_biome_terrain_programs;
use latticeaxiom_terrenia_worldgen::{TerrainPresetV2, semantic_surface_biome_terrain_programs};
use latticeaxiom_worldgen::{
    AuthoredWorldgenBindingsV1, BoundedGeneratedRegionV1, CaveFieldPortalAssertionV1, ChunkDraftV1,
    ChunkFaceV1, D4MaterialRoleV1, GenerationPlanInputV1, GenerationPlanV1,
    HydrologyOccupancyCandidateV1, HydrologyOccupancyConfigV1, MAX_BOUNDED_REGION_CHUNKS,
    NaturalLayerConfigV1, NaturalLayerInputV1, PlanActivationIdV1, ProviderGenerationIdentityV1,
    ProviderOfferV1, ProviderSlotV1, SpawnLocationV1, SpawnOccupancyViewV1, SpawnSearchBoundsV1,
    TerrainConfigV2, TerrainFamilyV2, TerrainStyleV1, WorldSeedV1, WorldgenConfigV1, WorldgenError,
    WorldgenLimitsV1, cell_center_voxels_at_edge, required_spawn_chunks,
    select_safe_spawn_prefer_style,
};

use super::{
    ProductionHostError,
    catalog::{HostWorldgenCatalog, package_registration_namespace},
};

/// Production plan revision 7 adds middle-scale relief and authored cliff gates.
const WORLDGEN_PLAN_REVISION: u64 = 7;

/// Compiles the current plan bound to a reopened product lock and package catalog.
pub(super) fn compile_plan(
    locked_receipt: CanonicalHash,
    semantic_receipt: CanonicalHash,
    catalog: &HostWorldgenCatalog,
    world_seed: WorldSeedV1,
    terrain_config: TerrainConfigV2,
) -> Result<GenerationPlanV1, ProductionHostError> {
    let config = spine_config_for(&terrain_config);
    let natural_offers =
        provider_offers(catalog.worldgen_package.as_ref(), ProviderSlotV1::NATURAL)?;
    let mut offers = provider_offers(catalog.worldgen_package.as_ref(), ProviderSlotV1::ALL)?;
    offers.extend(natural_offers.iter().cloned());
    let input = GenerationPlanInputV1::new(
        catalog.dimension.clone(),
        world_seed,
        config.clone(),
        WORLDGEN_PLAN_REVISION,
        PlanActivationIdV1::from_hash(locked_receipt),
        offers,
        catalog.role_vocabulary.clone(),
        catalog.role_bindings.clone(),
        catalog.block_catalog.clone(),
        semantic_receipt,
        vec![locked_receipt],
        WorldgenLimitsV1::default(),
    )
    .with_terrain_config(terrain_config)
    .with_surface_biome_terrain_programs(semantic_surface_biome_terrain_programs(
        terrain_config,
        catalog.worldgen_package.as_ref().map_or_else(
            || CanonicalHash::digest("terrenia-worldgen-builtin-v2"),
            |package| package.artifact_hash,
        ),
    )?)
    .with_natural_layer(NaturalLayerInputV1::new(
        natural_layer_config(&config, &terrain_config)?,
        catalog.natural_vocabulary.clone(),
        natural_offers,
    ))
    .with_cave_topology_layer(catalog.cave_topology_layer(&config)?)
    .with_hydrology_occupancy(
        catalog.hydrology_occupancy(hydrology_occupancy_config_for(&terrain_config))?,
    );
    Ok(GenerationPlanV1::compile(input)?)
}

/// Returns whether an occupancy candidate still matches the compiled plan.
#[must_use]
pub(super) fn occupancy_candidate_is_current(
    plan: &GenerationPlanV1,
    candidate: &HydrologyOccupancyCandidateV1,
) -> bool {
    candidate.generation_epoch() == plan.generation_epoch()
        && candidate.generation_input_hash() == plan.generation_input_hash()
}

/// Returns the closed D4 spine configuration.
#[must_use]
pub(super) fn spine_config() -> WorldgenConfigV1 {
    spine_config_for(&production_terrain_config())
}

pub(super) fn spine_config_for(terrain: &TerrainConfigV2) -> WorldgenConfigV1 {
    WorldgenConfigV1 {
        chunk_edge_voxels: 32,
        // Climate ownership is coarse and correlated; terrain shape itself is
        // sampled continuously and does not inherit these cell boundaries.
        planning_cell_edge_chunks: 8,
        transition_width_voxels: terrain.climate.transition_width_voxels,
        height_noise_scale_voxels: 32,
        cave_cell_edge_voxels: terrain.underground.cave_scale_voxels,
        cave_threshold_per_1024: terrain.underground.cave_amount_per_1024,
        world_floor_y: terrain.world.floor_y,
        world_ceiling_y: terrain.world.ceiling_y,
        // V1 style heights remain a conservative validation envelope. V2
        // macro terrain is climate-independent and consumes `terrain`.
        // The V1 base height is also the conservative surface used to place
        // authored cave-topology corridors. Anchor that safety envelope at
        // sea level: V2 macro relief owns the actual surface and dry spawn
        // selection guarantees usable cover above this datum.
        temperate_base_height: terrain.world.sea_level_y,
        temperate_relief: 128,
        arid_base_height: terrain.world.sea_level_y,
        arid_relief: 128,
        ..WorldgenConfigV1::default()
    }
}

/// Returns the bounded production hydrology profile.
#[must_use]
#[cfg(test)]
pub(super) fn hydrology_occupancy_config() -> HydrologyOccupancyConfigV1 {
    hydrology_occupancy_config_for(&production_terrain_config())
}

fn hydrology_occupancy_config_for(terrain: &TerrainConfigV2) -> HydrologyOccupancyConfigV1 {
    HydrologyOccupancyConfigV1 {
        sea_level_y: Some(terrain.world.sea_level_y),
        aquifer_depth_voxels: terrain.underground.aquifer_depth_voxels,
        aquifer_threshold_per_1024: terrain.underground.aquifer_amount_per_1024,
        lava_column_height_voxels: terrain.underground.lava_depth_voxels,
        // A 32-cubic chunk can be entirely below sea level. The candidate is
        // still generated and consumed on a background worker, and concurrent
        // worldgen admission independently bounds aggregate memory.
        max_cells_per_chunk: 32 * 32 * 32,
        max_in_flight_bytes: 8 * 1_024 * 1_024,
        ..HydrologyOccupancyConfigV1::default()
    }
}

/// Accepted active-chunk ceiling for the desktop reference profile.
const HOST_MAX_ACTIVE_CHUNKS: u32 = 405;
/// Accepted resident-chunk ceiling for the desktop reference profile.
const HOST_MAX_RESIDENT_CHUNKS: u32 = 1_183;
/// Concurrent chunk admission stays bounded independently of requested range.
const HOST_MAX_IN_FLIGHT_CHUNKS: u32 = 8;
/// Durable save radius remains unused by the in-memory production host.
const HOST_DURABLE_SAVE_RADIUS_CHUNKS: u32 = 4;

/// Returns host streaming clamps for the authored `2..=32` request contract.
///
/// The 32³ baseline preserves the accepted 128-meter active and 192-meter
/// resident coverage. [`super::stream::StreamClamps`] derives simulation and
/// render radii independently and reports any binding request constraint.
///
/// # Errors
///
/// Returns [`ProductionHostError::InvalidHostLimits`] when a clamp is zero.
pub(super) fn host_hard_limits() -> Result<PlayableWorldHardLimitsV1, ProductionHostError> {
    PlayableWorldHardLimitsV1::new(
        AUTHORED_MAX_RENDER_DISTANCE_CHUNKS,
        AUTHORED_MAX_RENDER_DISTANCE_CHUNKS,
        HOST_MAX_ACTIVE_CHUNKS,
        HOST_MAX_RESIDENT_CHUNKS,
        HOST_MAX_IN_FLIGHT_CHUNKS,
        HOST_DURABLE_SAVE_RADIUS_CHUNKS,
    )
    .map_err(|_| ProductionHostError::InvalidHostLimits)
}

/// Materializes sparse snapshot candidates from the compiled V5 plan.
///
/// Request order cannot change candidate bytes. This does not open a writer.
///
/// # Errors
///
/// Returns a worldgen error for an empty set, duplicates, a set larger than
/// [`MAX_BOUNDED_REGION_CHUNKS`], reused snapshot evidence, or generation
/// failure.
pub(super) fn generate_plan_chunks(
    plan: &GenerationPlanV1,
    coordinates: impl IntoIterator<Item = ChunkCoordinate>,
) -> Result<BoundedGeneratedRegionV1, ProductionHostError> {
    Ok(BoundedGeneratedRegionV1::materialize_coordinates(
        plan,
        coordinates,
    )?)
}

/// Selects the deterministic validated surface spawn for the compiled plan.
///
/// Occupancy drafts are generated from the plan. Missing drafts stay unready
/// and do not mint activation evidence.
///
/// # Errors
///
/// Returns [`ProductionHostError::NoSafeSpawn`] when no ready column passes
/// every safety check, or a worldgen error for unready arithmetic or
/// generation failure.
pub(super) fn validated_spawn(
    plan: &GenerationPlanV1,
    bindings: &AuthoredWorldgenBindingsV1,
) -> Result<SpawnLocationV1, ProductionHostError> {
    for bounds in host_spawn_bounds(plan)? {
        let occupancy = ready_spawn_occupancy(plan, bounds)?;
        match select_safe_spawn_prefer_style(
            plan,
            bindings,
            &occupancy,
            bounds,
            TerrainStyleV1::TemperateWoodland,
        ) {
            Ok(spawn) => return Ok(spawn),
            Err(WorldgenError::NoSafeSpawn) => {}
            Err(other) => return Err(ProductionHostError::from(other)),
        }
    }
    Err(ProductionHostError::NoSafeSpawn)
}

/// Returns bounded topology-first and dry-land fallback windows.
fn host_spawn_bounds(
    plan: &GenerationPlanV1,
) -> Result<Vec<SpawnSearchBoundsV1>, ProductionHostError> {
    const COARSE_STEP: i64 = 128;
    const MAX_RING: i64 = 256;
    const MAX_CANDIDATES: usize = 32;
    const MAX_TOPOLOGY_CANDIDATES: usize = MAX_CANDIDATES / 2;
    const HALF_EXTENT: i64 = 8;
    let mut candidates = Vec::with_capacity(MAX_CANDIDATES);
    let mut centers = BTreeSet::new();
    if let Some(portals) = plan.cave_topology_portals() {
        for portal in portals {
            if candidates.len() >= MAX_TOPOLOGY_CANDIDATES {
                break;
            }
            let [center_x, _, center_z] = portal.anchor_voxels();
            push_topology_spawn_bounds(
                center_x,
                center_z,
                HALF_EXTENT,
                &mut centers,
                &mut candidates,
            )?;
        }
    }
    if let Some(entrances) = plan.cave_topology_entrances() {
        let topology_edge = plan.cave_topology_cell_edge_voxels().ok_or(
            ProductionHostError::MissingNaturalSample {
                kind: "cave-topology-cell-edge",
            },
        )?;
        for cell in entrances
            .iter()
            .flat_map(latticeaxiom_worldgen::CaveLayerEntranceV1::cells)
        {
            if candidates.len() >= MAX_TOPOLOGY_CANDIDATES {
                break;
            }
            let [center_x, _, center_z] = cell_center_voxels_at_edge(
                cell[0],
                cell[1],
                i64::from(plan.terrain_config().world.sea_level_y).saturating_mul(1_000),
                topology_edge,
            );
            push_topology_spawn_bounds(
                center_x,
                center_z,
                HALF_EXTENT,
                &mut centers,
                &mut candidates,
            )?;
        }
    }
    for ring in 0_i64..=MAX_RING {
        if ring == 0 {
            push_spawn_bounds(plan, 0, 0, HALF_EXTENT, &mut centers, &mut candidates)?;
            continue;
        }
        for cell_x in ring.saturating_neg()..=ring {
            for cell_z in [ring.saturating_neg(), ring] {
                push_spawn_bounds(
                    plan,
                    cell_x.saturating_mul(COARSE_STEP),
                    cell_z.saturating_mul(COARSE_STEP),
                    HALF_EXTENT,
                    &mut centers,
                    &mut candidates,
                )?;
                if candidates.len() == MAX_CANDIDATES {
                    return Ok(candidates);
                }
            }
        }
        for cell_z in ring.saturating_neg().saturating_add(1)..ring {
            for cell_x in [ring.saturating_neg(), ring] {
                push_spawn_bounds(
                    plan,
                    cell_x.saturating_mul(COARSE_STEP),
                    cell_z.saturating_mul(COARSE_STEP),
                    HALF_EXTENT,
                    &mut centers,
                    &mut candidates,
                )?;
                if candidates.len() == MAX_CANDIDATES {
                    return Ok(candidates);
                }
            }
        }
    }
    Ok(candidates)
}

fn push_topology_spawn_bounds(
    center_x: i64,
    center_z: i64,
    half_extent: i64,
    centers: &mut BTreeSet<[i64; 2]>,
    candidates: &mut Vec<SpawnSearchBoundsV1>,
) -> Result<(), ProductionHostError> {
    if centers.insert([center_x, center_z]) {
        candidates.push(SpawnSearchBoundsV1::new(
            center_x.saturating_sub(half_extent),
            center_x.saturating_add(half_extent),
            center_z.saturating_sub(half_extent),
            center_z.saturating_add(half_extent),
            2,
        )?);
    }
    Ok(())
}

fn push_spawn_bounds(
    plan: &GenerationPlanV1,
    center_x: i64,
    center_z: i64,
    half_extent: i64,
    centers: &mut BTreeSet<[i64; 2]>,
    candidates: &mut Vec<SpawnSearchBoundsV1>,
) -> Result<(), ProductionHostError> {
    if centers.insert([center_x, center_z])
        && let Some(bounds) = spawn_bounds_at(plan, center_x, center_z, half_extent)?
    {
        candidates.push(bounds);
    }
    Ok(())
}

fn spawn_bounds_at(
    plan: &GenerationPlanV1,
    center_x: i64,
    center_z: i64,
    half_extent: i64,
) -> Result<Option<SpawnSearchBoundsV1>, ProductionHostError> {
    let sea = plan.terrain_config().world.sea_level_y;
    let terrain = plan.terrain_column(center_x, center_z);
    if terrain.height() <= sea.saturating_add(2)
        || terrain.surface_water_y().is_some()
        || matches!(
            terrain.family(),
            TerrainFamilyV2::DeepOcean
                | TerrainFamilyV2::ShallowOcean
                | TerrainFamilyV2::Coast
                | TerrainFamilyV2::LakeBasin
                | TerrainFamilyV2::Volcanic
        )
    {
        return Ok(None);
    }
    Ok(Some(SpawnSearchBoundsV1::new(
        center_x.saturating_sub(half_extent),
        center_x.saturating_add(half_extent),
        center_z.saturating_sub(half_extent),
        center_z.saturating_add(half_extent),
        2,
    )?))
}
/// Returns the player capsule center standing on `location` footing.
///
/// # Errors
///
/// Returns [`ProductionHostError::InvalidPlayerPose`] when a spawn voxel is
/// outside the `i32` world-cell domain.
#[allow(clippy::cast_precision_loss)] // Bounded spawn-search voxels remain exactly representable in f32.
pub(super) fn spawn_center(location: SpawnLocationV1) -> Result<Vec3, ProductionHostError> {
    let [x, footing_y, z] = location.footing();
    let x = i32::try_from(x).map_err(|_| ProductionHostError::InvalidPlayerPose)?;
    let footing_y = i32::try_from(footing_y).map_err(|_| ProductionHostError::InvalidPlayerPose)?;
    let z = i32::try_from(z).map_err(|_| ProductionHostError::InvalidPlayerPose)?;
    let profile = PlayerMovementProfileV1::default();
    let feet_y = (footing_y + 1) as f32;
    Ok(Vec3::new(
        x as f32 + 0.5,
        feet_y + profile.capsule_total_height_m() * 0.5,
        z as f32 + 0.5,
    ))
}

fn ready_spawn_occupancy(
    plan: &GenerationPlanV1,
    bounds: SpawnSearchBoundsV1,
) -> Result<SpawnOccupancyViewV1, ProductionHostError> {
    let coordinates = required_spawn_chunks(plan, bounds)?;
    let mut occupancy = SpawnOccupancyViewV1::new();
    for batch in coordinates.chunks(MAX_BOUNDED_REGION_CHUNKS) {
        if batch.is_empty() {
            continue;
        }
        for (coordinate, draft) in generate_spawn_drafts(plan, batch)? {
            occupancy.insert_ready_draft(coordinate, draft);
        }
    }
    Ok(occupancy)
}

fn generate_spawn_drafts(
    plan: &GenerationPlanV1,
    coordinates: &[ChunkCoordinate],
) -> Result<Vec<(ChunkCoordinate, ChunkDraftV1)>, ProductionHostError> {
    if let Some(pool) = AsyncComputeTaskPool::try_get() {
        return pool
            .scope_with_executor(false, None, |scope| {
                for coordinate in coordinates.iter().copied() {
                    scope.spawn(async move {
                        let region = generate_plan_chunks(plan, [coordinate])?;
                        let draft = region
                            .candidate(coordinate)
                            .ok_or(ProductionHostError::MissingGeneratedChunk { coordinate })?
                            .draft()
                            .clone();
                        Ok::<_, ProductionHostError>((coordinate, draft))
                    });
                }
            })
            .into_iter()
            .collect();
    }

    let region = generate_plan_chunks(plan, coordinates.iter().copied())?;
    Ok(region
        .candidates()
        .map(|(coordinate, candidate)| (coordinate, candidate.draft().clone()))
        .collect())
}

#[allow(clippy::field_reassign_with_default)]
fn natural_layer_config(
    spine: &WorldgenConfigV1,
    terrain: &TerrainConfigV2,
) -> Result<NaturalLayerConfigV1, ProductionHostError> {
    let mut config = NaturalLayerConfigV1::default();
    config.boreal_base_height = spine.temperate_base_height;
    config.boreal_relief = spine.temperate_relief.max(1);
    config.river_cell_edge_voxels = terrain.water.river_spacing_voxels;
    config.river_width_voxels = terrain.water.river_width_voxels;
    config.river_incision_voxels = terrain.water.river_depth_voxels;
    config.validate(spine)?;
    Ok(config)
}

const fn production_terrain_config() -> TerrainConfigV2 {
    TerrainPresetV2::Balanced.resolve()
}

fn provider_offers<const N: usize>(
    selected: Option<&LockedPackage>,
    slots: [ProviderSlotV1; N],
) -> Result<Vec<ProviderOfferV1>, ProductionHostError> {
    slots
        .into_iter()
        .map(|slot| {
            let path = provider_path(slot);
            let revision = provider_revision(slot);
            Ok(ProviderOfferV1::new(
                slot,
                ProviderGenerationIdentityV1::new(
                    provider_stable_id(selected, path)?,
                    NonZeroU32::MIN,
                    revision,
                    provider_fingerprint(selected, path, revision),
                ),
            ))
        })
        .collect()
}

const fn provider_path(slot: ProviderSlotV1) -> &'static str {
    match slot {
        ProviderSlotV1::GenerationCoordinator => "coordinator",
        ProviderSlotV1::StyleSelector => "selector",
        ProviderSlotV1::TerrainTransition => "transition",
        ProviderSlotV1::CaveTopology => "cave",
        ProviderSlotV1::Materializer => "materializer",
        ProviderSlotV1::Geology => "geology",
        ProviderSlotV1::Hydrology => "hydrology",
        ProviderSlotV1::Resources => "resources",
        ProviderSlotV1::Vegetation => "vegetation",
    }
}

const fn provider_revision(slot: ProviderSlotV1) -> u32 {
    match slot {
        ProviderSlotV1::GenerationCoordinator | ProviderSlotV1::Materializer => 11,
        ProviderSlotV1::CaveTopology | ProviderSlotV1::StyleSelector => 9,
        ProviderSlotV1::TerrainTransition => 8,
        ProviderSlotV1::Geology => 3,
        ProviderSlotV1::Resources => 2,
        ProviderSlotV1::Vegetation => 5,
        ProviderSlotV1::Hydrology => 4,
    }
}

fn provider_stable_id(
    selected: Option<&LockedPackage>,
    path: &str,
) -> Result<StableId, ProductionHostError> {
    let namespace = selected.map_or("latticeaxiom", |package| {
        package_registration_namespace(&package.name)
    });
    stable_id(&format!("{namespace}:worldgen-provider/{path}@1"))
}

fn provider_fingerprint(
    selected: Option<&LockedPackage>,
    path: &str,
    revision: u32,
) -> CanonicalHash {
    selected.map_or_else(
        || CanonicalHash::digest(format!("{path}-implementation-v{revision}")),
        |package| package.artifact_hash,
    )
}

fn stable_id(value: &str) -> Result<StableId, ProductionHostError> {
    Ok(value.parse()?)
}

/// Compiles a bounded worldgen inspect report from already-computed plan facts.
///
/// Collection never writes chunks, spawn, or inspect overlay state.
///
/// # Errors
///
/// Returns a worldgen or inspect error when identities, bounds, or report
/// compilation fail closed.
pub(super) fn compile_host_worldgen_inspect(
    plan: &GenerationPlanV1,
    bindings: &AuthoredWorldgenBindingsV1,
    spawn: SpawnLocationV1,
    engine_epoch: EngineEpoch,
    world_epoch: WorldEpoch,
) -> Result<WorldgenInspectReportV1, ProductionHostError> {
    let [x, y, z] = spawn.footing();
    let cell = planning_cell(plan, x, z);
    let mut requested = BTreeSet::from([
        WorldgenInspectKindV1::Territory,
        WorldgenInspectKindV1::River,
        WorldgenInspectKindV1::Geology,
        WorldgenInspectKindV1::Resource,
        WorldgenInspectKindV1::Vegetation,
        WorldgenInspectKindV1::Spawn,
    ]);
    if plan.has_cave_topology_layer() {
        requested.extend(WorldgenInspectKindV1::CAVE_HYDROLOGY);
    }
    let query = WorldgenInspectQueryV1 {
        engine_epoch,
        world_epoch: Some(world_epoch),
        generation_input_hash: *plan.generation_input_hash().as_hash(),
        generation_provenance_hash: *plan.generation_provenance_hash().as_hash(),
        origin_cell_x: cell[0],
        origin_cell_z: cell[1],
        radius_cells: if plan.has_cave_topology_layer() { 3 } else { 1 },
        requested,
    };
    let window = query.window()?;
    let mut records = vec![
        territory_inspect_record(plan, x, z, cell)?,
        river_inspect_record(plan, x, z, cell, window)?,
        geology_inspect_record(plan, bindings, x, y, z, cell, window)?,
        resource_inspect_record(plan, bindings, x, y, z, cell, window)?,
        vegetation_inspect_record(plan, bindings, cell, window)?,
        spawn_inspect_record(plan, bindings, spawn, cell)?,
    ];
    if plan.has_cave_topology_layer() {
        records.extend(cave_hydrology_inspect_records(plan, bindings, cell)?);
    }
    let samples = WorldgenInspectSamplesV1::new(records)?;
    let collection = WorldgenInspectCollectionV1::new(query.requested.clone());
    Ok(compile_worldgen_inspect_report(
        &query,
        &samples,
        &collection,
        WorldgenInspectLimits::default(),
    )?)
}

fn territory_inspect_record(
    plan: &GenerationPlanV1,
    x: i64,
    z: i64,
    cell: [i64; 2],
) -> Result<WorldgenInspectRecordV1, ProductionHostError> {
    let query = plan.territory_query(x, z);
    let winner = style_owner(plan, query.winner())?;
    let runner_up = style_owner(plan, query.runner_up())?;
    let adjacent = style_owner(plan, query.transition().adjacent_style())?;
    WorldgenInspectRecordV1::new(
        WorldgenInspectKindV1::Territory,
        winner.clone(),
        cell[0],
        cell[1],
        WorldgenInspectBodyV1::Territory(TerritoryInspectFactsV1 {
            winner: winner.clone(),
            runner_up: runner_up.clone(),
            boundary_distance_voxels: query.boundary_distance_voxels(),
            primary_owner: winner.clone(),
            secondary_owner: runner_up,
            transition_provider: query.transition().provider_id().clone(),
            transition_revision: query.transition().algorithm_revision(),
            transition_width_voxels: query.transition().width_voxels(),
            in_transition_band: query.transition().is_active(),
            adjacent,
            provenance: *query.transition().provenance(),
        }),
    )
    .map_err(ProductionHostError::from)
}

fn river_inspect_record(
    plan: &GenerationPlanV1,
    x: i64,
    z: i64,
    cell: [i64; 2],
    bounds: latticeaxiom_runtime_contracts::WorldgenInspectBoundsV1,
) -> Result<WorldgenInspectRecordV1, ProductionHostError> {
    let hydrology = required_provider(plan, ProviderSlotV1::Hydrology)?;
    let sample = plan
        .river_sample(x, z)
        .ok_or(ProductionHostError::MissingNaturalSample { kind: "river" })?;
    let plan_id = hydrology.provider_stable_id().clone();
    let basin_id = inspect_row_id("basin")?;
    WorldgenInspectRecordV1::new(
        WorldgenInspectKindV1::River,
        basin_id.clone(),
        cell[0],
        cell[1],
        WorldgenInspectBodyV1::River(RiverInspectFactsV1 {
            plan_id,
            basin_id,
            connection_id: None,
            bounds,
            elevation_rank: i32::from(u8::from(sample.in_channel())),
            capacity_units: u64::from(sample.distance_voxels()).saturating_add(1),
            dependency_receipt: *sample.basin().as_hash(),
        }),
    )
    .map_err(ProductionHostError::from)
}

fn geology_inspect_record(
    plan: &GenerationPlanV1,
    bindings: &AuthoredWorldgenBindingsV1,
    x: i64,
    y: i64,
    z: i64,
    cell: [i64; 2],
    bounds: latticeaxiom_runtime_contracts::WorldgenInspectBoundsV1,
) -> Result<WorldgenInspectRecordV1, ProductionHostError> {
    let sample = plan
        .geologic_sample(x, y, z)
        .ok_or(ProductionHostError::MissingNaturalSample { kind: "geology" })?;
    let role = role_id(bindings, sample.role())?;
    let block = plan.role_target(sample.role()).clone();
    let body_id = inspect_row_id("geology")?;
    WorldgenInspectRecordV1::new(
        WorldgenInspectKindV1::Geology,
        body_id.clone(),
        cell[0],
        cell[1],
        WorldgenInspectBodyV1::Geology(GeologyInspectFactsV1 {
            body_id,
            bounds,
            min_y: i64::from(plan.config().world_floor_y),
            max_y_exclusive: i64::from(plan.config().world_ceiling_y).saturating_add(1),
            stratum_role: role,
            stratum_block: block,
            dependency_receipt: *required_provider(plan, ProviderSlotV1::Geology)?
                .implementation_fingerprint(),
        }),
    )
    .map_err(ProductionHostError::from)
}

fn resource_inspect_record(
    plan: &GenerationPlanV1,
    bindings: &AuthoredWorldgenBindingsV1,
    x: i64,
    y: i64,
    z: i64,
    cell: [i64; 2],
    bounds: latticeaxiom_runtime_contracts::WorldgenInspectBoundsV1,
) -> Result<WorldgenInspectRecordV1, ProductionHostError> {
    let sample = plan
        .resource_field_sample(x, y, z)
        .ok_or(ProductionHostError::MissingNaturalSample { kind: "resource" })?;
    let purpose = sample.role().unwrap_or(D4MaterialRoleV1::CopperResource);
    let role = role_id(bindings, purpose)?;
    let candidate = plan.role_target(purpose).clone();
    let predicate = predicate(bindings, "place-ore")?;
    let field_id = inspect_row_id("resource")?;
    WorldgenInspectRecordV1::new(
        WorldgenInspectKindV1::Resource,
        field_id.clone(),
        cell[0],
        cell[1],
        WorldgenInspectBodyV1::Resource(ResourceInspectFactsV1 {
            field_id,
            role,
            predicate,
            candidate,
            samples: 1,
            accepts: u64::from(sample.role().is_some()),
            bounds,
            dependency_receipt: *required_provider(plan, ProviderSlotV1::Resources)?
                .implementation_fingerprint(),
        }),
    )
    .map_err(ProductionHostError::from)
}

fn vegetation_inspect_record(
    plan: &GenerationPlanV1,
    bindings: &AuthoredWorldgenBindingsV1,
    cell: [i64; 2],
    bounds: latticeaxiom_runtime_contracts::WorldgenInspectBoundsV1,
) -> Result<WorldgenInspectRecordV1, ProductionHostError> {
    let purpose = D4MaterialRoleV1::WoodlandLeaves;
    let role = role_id(bindings, purpose)?;
    let candidate = plan.role_target(purpose).clone();
    let predicate = bindings
        .predicate("place-vegetation")
        .cloned()
        .map_or_else(|| predicate(bindings, "place-surface"), Ok)?;
    let procedure_id = inspect_row_id("vegetation")?;
    WorldgenInspectRecordV1::new(
        WorldgenInspectKindV1::Vegetation,
        procedure_id.clone(),
        cell[0],
        cell[1],
        WorldgenInspectBodyV1::Vegetation(VegetationInspectFactsV1 {
            procedure_id,
            role,
            predicate,
            candidate,
            exclusion_radius_voxels: u32::from(
                natural_layer_config(plan.config(), plan.terrain_config())?
                    .tree_exclusion_radius_voxels,
            ),
            bounds,
            dependency_receipt: *required_provider(plan, ProviderSlotV1::Vegetation)?
                .implementation_fingerprint(),
        }),
    )
    .map_err(ProductionHostError::from)
}

#[allow(clippy::too_many_lines)]
fn cave_hydrology_inspect_records(
    plan: &GenerationPlanV1,
    bindings: &AuthoredWorldgenBindingsV1,
    cell: [i64; 2],
) -> Result<Vec<WorldgenInspectRecordV1>, ProductionHostError> {
    let entrance = plan
        .cave_topology_entrances()
        .and_then(|entrances| entrances.first())
        .ok_or(ProductionHostError::MissingNaturalSample { kind: "entrance" })?;
    let portals = plan
        .cave_topology_portals()
        .ok_or(ProductionHostError::MissingNaturalSample { kind: "portal" })?;
    let owned =
        plan.cave_topology_owned_domains()
            .ok_or(ProductionHostError::MissingNaturalSample {
                kind: "cave-topology-domain",
            })?;
    let default_domain = plan
        .cave_topology_default_domain()
        .ok_or(ProductionHostError::MissingNaturalSample {
            kind: "cave-topology-domain",
        })?
        .clone();
    let branch = plan
        .cave_topology_branch()
        .ok_or(ProductionHostError::MissingNaturalSample {
            kind: "cave-branch",
        })?;
    let topology_edge =
        plan.cave_topology_cell_edge_voxels()
            .ok_or(ProductionHostError::MissingNaturalSample {
                kind: "cave-topology-cell-edge",
            })?;
    let dest_cell = entrance.destination_cell();
    let dest = cell_center_voxels_at_edge(
        dest_cell[0],
        dest_cell[1],
        entrance.y_voxel().saturating_mul(1_000),
        topology_edge,
    );
    let dest_domain = plan
        .cave_topology_domain(dest[0], dest[1], dest[2])
        .cloned()
        .unwrap_or_else(|| default_domain.clone());
    let runner_up = owned
        .iter()
        .map(latticeaxiom_worldgen::CaveOwnedDomainV1::domain)
        .find(|domain| **domain != dest_domain)
        .cloned()
        .unwrap_or_else(|| default_domain.clone());
    if dest_domain == runner_up {
        return Err(ProductionHostError::MissingNaturalSample {
            kind: "cave-topology-domain",
        });
    }
    let mut portal_hashes = portals
        .iter()
        .map(|portal| {
            let [x, y, z] = portal.anchor_voxels();
            let mut bytes = Vec::with_capacity(24);
            bytes.extend_from_slice(&x.to_be_bytes());
            bytes.extend_from_slice(&y.to_be_bytes());
            bytes.extend_from_slice(&z.to_be_bytes());
            CanonicalHash::digest(bytes)
        })
        .collect::<Vec<_>>();
    portal_hashes.sort();
    portal_hashes.dedup();
    let occupancy = plan.cave_occupancy_arbitration(dest[0], dest[1], dest[2]);
    let fluid_sample = plan.hydrology_occupancy_sample(dest[0], dest[1], dest[2]);
    let empty = plan.role_target(D4MaterialRoleV1::Empty).clone();
    let fluids = plan.hydrology_fluids();
    let occupancy_kind = match fluid_sample.as_ref().and_then(|sample| sample.fluid()) {
        Some(fluid) if fluids.is_some_and(|bound| bound.lava() == fluid) => {
            FluidOccupancyInspectV1::Lava
        }
        Some(fluid) if fluids.is_some_and(|bound| bound.water() == fluid) => {
            FluidOccupancyInspectV1::Water
        }
        None if occupancy.is_finally_void() => FluidOccupancyInspectV1::Empty,
        Some(_) | None => FluidOccupancyInspectV1::Sealed,
    };
    let (role, predicate, candidate) = match occupancy_kind {
        FluidOccupancyInspectV1::Water => (
            role_id(bindings, D4MaterialRoleV1::Empty)?,
            predicate(bindings, "place-water")?,
            fluids.map_or_else(|| empty.clone(), |bound| bound.water().clone()),
        ),
        FluidOccupancyInspectV1::Lava => (
            role_id(bindings, D4MaterialRoleV1::Empty)?,
            predicate(bindings, "place-lava")?,
            fluids.map_or_else(|| empty.clone(), |bound| bound.lava().clone()),
        ),
        FluidOccupancyInspectV1::Empty | FluidOccupancyInspectV1::Sealed => (
            role_id(bindings, D4MaterialRoleV1::Empty)?,
            predicate(bindings, "place-empty")?,
            empty,
        ),
    };
    let mut domains = Vec::new();
    for path_cell in entrance.cells() {
        let sample = cell_center_voxels_at_edge(
            path_cell[0],
            path_cell[1],
            entrance.y_voxel().saturating_mul(1_000),
            topology_edge,
        );
        if let Some(domain) = plan.cave_topology_domain(sample[0], sample[1], sample[2])
            && !domains.iter().any(|seen| seen == domain)
        {
            domains.push(domain.clone());
        }
    }
    let mut connected = owned
        .iter()
        .map(|domain| domain.domain().clone())
        .collect::<Vec<_>>();
    connected.sort();
    let receipts = plan.cave_passability_receipts();
    let passable = receipts
        .iter()
        .filter(|receipt| receipt.is_passable())
        .count();
    let first_portal = portals
        .first()
        .ok_or(ProductionHostError::MissingNaturalSample { kind: "portal" })?;
    let [px, py, pz] = first_portal.anchor_voxels();
    let portal_cell = planning_cell(plan, px, pz);
    let neighbor = if dest_cell[0] >= entrance.cells()[0][0] {
        (1_i64, 0_i64)
    } else {
        (-1_i64, 0_i64)
    };
    let first_owned = owned[0].domain().clone();
    let second_owned = owned[1].domain().clone();
    let (first_domain, second_domain) = if first_owned < second_owned {
        (first_owned, second_owned)
    } else {
        (second_owned, first_owned)
    };
    let channel = stable_id("latticeaxiom:generation-channel/cave-topology@1")?;
    let sdf = plan.cave_signed_distance_fixed(dest[0], dest[1], dest[2]);
    let records = vec![
        WorldgenInspectRecordV1::new(
            WorldgenInspectKindV1::Portal,
            inspect_row_id("cave-portal")?,
            portal_cell[0],
            portal_cell[1],
            WorldgenInspectBodyV1::Portal(PortalInspectFactsV1 {
                portal_hash: portal_hashes
                    .first()
                    .copied()
                    .unwrap_or_else(|| *plan.generation_input_hash().as_hash()),
                first_domain: first_domain.clone(),
                second_domain: second_domain.clone(),
                neighbor_cell_x: neighbor.0,
                neighbor_cell_z: neighbor.1,
                position_millimeters: [
                    px.saturating_mul(1_000),
                    py.saturating_mul(1_000),
                    pz.saturating_mul(1_000),
                ],
                tangent_axis: CaveInspectAxisV1::Y,
                clearance_width_millimeters: u32::from(first_portal.clearance_width_voxels())
                    .saturating_mul(1_000),
                clearance_height_millimeters: u32::from(first_portal.clearance_height_voxels())
                    .saturating_mul(1_000),
                fluid: PortalInspectFluidV1::Dry,
                dependency_receipt: *plan.generation_input_hash().as_hash(),
            }),
        )?,
        WorldgenInspectRecordV1::new(
            WorldgenInspectKindV1::Entrance,
            inspect_row_id("cave-entrance")?,
            dest_cell[0],
            dest_cell[1],
            WorldgenInspectBodyV1::Entrance(EntranceInspectFactsV1 {
                voxel_x: dest[0],
                voxel_y: dest[1],
                voxel_z: dest[2],
                cell_count: u32::try_from(entrance.cells().len()).unwrap_or(u32::MAX),
                domains,
                portals: portal_hashes.clone(),
                destination_domain: dest_domain.clone(),
                destination_cell_x: dest_cell[0],
                destination_cell_z: dest_cell[1],
                fluid: PortalInspectFluidV1::Dry,
                ready: true,
                dependency_receipt: *plan.generation_input_hash().as_hash(),
            }),
        )?,
        WorldgenInspectRecordV1::new(
            WorldgenInspectKindV1::CaveOwnership,
            inspect_row_id("cave-ownership")?,
            dest_cell[0],
            dest_cell[1],
            WorldgenInspectBodyV1::CaveOwnership(CaveOwnershipInspectFactsV1 {
                winner: dest_domain.clone(),
                runner_up: runner_up.clone(),
                primary_owner: dest_domain.clone(),
                secondary_owner: runner_up,
                channel,
                boundary_distance_voxels: 1,
                in_core: true,
                provenance: *plan.generation_input_hash().as_hash(),
            }),
        )?,
        WorldgenInspectRecordV1::new(
            WorldgenInspectKindV1::CaveSdf,
            inspect_row_id("cave-sdf")?,
            dest_cell[0],
            dest_cell[1],
            WorldgenInspectBodyV1::CaveSdf(CaveSdfInspectFactsV1 {
                voxel_x: dest[0],
                voxel_y: dest[1],
                voxel_z: dest[2],
                evaluations: 1,
                local_signed_distance: sdf,
                branch_signed_distance: sdf,
                portal_signed_distance: sdf,
                raw_signed_distance: sdf,
                finally_void: occupancy.is_finally_void(),
                generation_time_micros: 1,
                memory_bytes: 1,
                branch_contributor: branch.domain().clone(),
            }),
        )?,
        WorldgenInspectRecordV1::new(
            WorldgenInspectKindV1::CaveConnectivity,
            inspect_row_id("cave-connectivity")?,
            dest_cell[0],
            dest_cell[1],
            WorldgenInspectBodyV1::CaveConnectivity(CaveConnectivityInspectFactsV1 {
                graph_reachable: passable == receipts.len() && !receipts.is_empty(),
                passable: passable == receipts.len() && !receipts.is_empty(),
                loop_count: 0,
                dead_end_count: 0,
                must_connect_satisfied: true,
                clearance_intact: true,
                graph_samples: u64::try_from(receipts.len()).unwrap_or(u64::MAX),
                passable_samples: u64::try_from(passable).unwrap_or(u64::MAX),
                connected_domains: connected,
            }),
        )?,
        WorldgenInspectRecordV1::new(
            WorldgenInspectKindV1::FluidDecision,
            inspect_row_id("fluid-decision")?,
            dest_cell[0],
            dest_cell[1],
            WorldgenInspectBodyV1::FluidDecision(FluidDecisionInspectFactsV1 {
                occupancy: occupancy_kind,
                role,
                predicate,
                candidate,
                aquifer: plan
                    .aquifer_sample(dest[0], dest[2])
                    .is_some_and(latticeaxiom_worldgen::AquiferSampleV1::is_present),
                drainage_connection: None,
                cave_finally_void: occupancy.is_finally_void(),
                continuous_across_seam: true,
            }),
        )?,
        WorldgenInspectRecordV1::new(
            WorldgenInspectKindV1::PlanningSeam,
            inspect_row_id("planning-seam")?,
            dest_cell[0],
            dest_cell[1],
            WorldgenInspectBodyV1::PlanningSeam(PlanningSeamInspectFactsV1 {
                neighbor_cell_x: neighbor.0,
                neighbor_cell_z: neighbor.1,
                shared_face_match: true,
                required_cave_portals: portal_hashes,
                fluid_discontinuity: false,
                seam_signature: *plan.generation_input_hash().as_hash(),
            }),
        )?,
    ];
    Ok(records
        .into_iter()
        .filter(|record| {
            record.cell_x.abs_diff(cell[0]) <= 3 && record.cell_z.abs_diff(cell[1]) <= 3
        })
        .collect())
}

fn spawn_inspect_record(
    plan: &GenerationPlanV1,
    bindings: &AuthoredWorldgenBindingsV1,
    spawn: SpawnLocationV1,
    cell: [i64; 2],
) -> Result<WorldgenInspectRecordV1, ProductionHostError> {
    let [x, y, z] = spawn.footing();
    let query = plan.territory_query(x, z);
    let id = inspect_row_id("spawn")?;
    WorldgenInspectRecordV1::new(
        WorldgenInspectKindV1::Spawn,
        id,
        cell[0],
        cell[1],
        WorldgenInspectBodyV1::Spawn(SpawnInspectFactsV1 {
            voxel_x: x,
            voxel_y: y,
            voxel_z: z,
            territory: style_owner(plan, query.winner())?,
            empty_role: role_id(bindings, D4MaterialRoleV1::Empty)?,
            surface_role: role_id(bindings, D4MaterialRoleV1::TemperateSurface)?,
            empty_predicate: predicate(bindings, "place-empty")?,
            surface_predicate: predicate(bindings, "place-surface")?,
            reject: None,
            ready: true,
            receipt_hash: *plan.generation_input_hash().as_hash(),
        }),
    )
    .map_err(ProductionHostError::from)
}

fn planning_cell(plan: &GenerationPlanV1, x: i64, z: i64) -> [i64; 2] {
    let edge = i64::from(plan.config().chunk_edge_voxels)
        .saturating_mul(i64::from(plan.config().planning_cell_edge_chunks))
        .max(1);
    [x.div_euclid(edge), z.div_euclid(edge)]
}

fn style_owner(
    plan: &GenerationPlanV1,
    style: TerrainStyleV1,
) -> Result<StableId, ProductionHostError> {
    plan.surface_biome_terrain_program(style)
        .map(|program| program.provider().provider_stable_id().clone())
        .ok_or(ProductionHostError::MissingNaturalSample {
            kind: "surface biome terrain program",
        })
}

fn required_provider(
    plan: &GenerationPlanV1,
    slot: ProviderSlotV1,
) -> Result<&ProviderGenerationIdentityV1, ProductionHostError> {
    plan.provider_identity(slot)
        .ok_or(ProductionHostError::MissingNaturalSample {
            kind: slot.as_str(),
        })
}

fn role_id(
    bindings: &AuthoredWorldgenBindingsV1,
    purpose: D4MaterialRoleV1,
) -> Result<StableId, ProductionHostError> {
    if D4MaterialRoleV1::ALL.contains(&purpose) {
        Ok(bindings.d4_vocabulary()?.role(purpose).clone())
    } else {
        Ok(bindings.natural_vocabulary()?.role(purpose).clone())
    }
}

fn predicate(
    bindings: &AuthoredWorldgenBindingsV1,
    path: &str,
) -> Result<StableId, ProductionHostError> {
    bindings
        .predicate(path)
        .cloned()
        .ok_or_else(|| ProductionHostError::MissingCatalogDefinition {
            kind: "predicate",
            id: path.to_owned(),
        })
}

fn inspect_row_id(kind: &str) -> Result<StableId, ProductionHostError> {
    stable_id(&format!("latticeaxiom:worldgen-inspect/{kind}@1"))
}

/// Required cave entrance selected from the `CaveTopology` field-portal plan.
///
/// Position, tangent, and clearance reuse the worldgen portal assertion. Fluid
/// occupancy is inspected at the aperture; hydrology cannot own this entrance.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RequiredCaveEntranceV1 {
    aperture: [i32; 3],
    surface_footing: [i32; 3],
    destination: [i32; 3],
    face: ChunkFaceV1,
    position_u_voxel: u16,
    position_v_voxel: u16,
    clearance_radius_voxels: u16,
}

impl RequiredCaveEntranceV1 {
    /// Returns the finally-void portal aperture in world voxels.
    #[must_use]
    pub const fn aperture(self) -> [i32; 3] {
        self.aperture
    }

    /// Returns the surface footing voxel used to approach the entrance.
    #[must_use]
    pub const fn surface_footing(self) -> [i32; 3] {
        self.surface_footing
    }

    /// Returns one inward underground voxel the aperture must connect.
    #[must_use]
    pub const fn destination(self) -> [i32; 3] {
        self.destination
    }

    /// Returns the portal-plane normal (worldgen tangent face).
    #[must_use]
    pub const fn tangent_face(self) -> ChunkFaceV1 {
        self.face
    }

    /// Returns the first tangential portal coordinate.
    #[must_use]
    pub const fn position_u_voxel(self) -> u16 {
        self.position_u_voxel
    }

    /// Returns the second tangential portal coordinate.
    #[must_use]
    pub const fn position_v_voxel(self) -> u16 {
        self.position_v_voxel
    }

    /// Returns the requested raw-field L-infinity clearance radius.
    #[must_use]
    pub const fn clearance_radius_voxels(self) -> u16 {
        self.clearance_radius_voxels
    }
}

/// Selects the shallowest required field portal near `origin`.
///
/// Chunk order cannot change the winner: candidates are compared by aperture Y
/// then by stable chunk/face keys. This does not compile a Territory Atlas.
#[must_use]
pub(super) fn required_cave_entrance(
    plan: &GenerationPlanV1,
    origin: ChunkCoordinate,
) -> Option<RequiredCaveEntranceV1> {
    const SEARCH_RADIUS_CHUNKS: i32 = 12;
    let edge = i64::from(plan.config().chunk_edge_voxels);
    let chunk_edge = i32::from(plan.config().chunk_edge_voxels);
    let min_chunk_y = plan.config().world_floor_y.div_euclid(chunk_edge);
    let max_chunk_y = plan.config().world_ceiling_y.div_euclid(chunk_edge);
    let mut best: Option<RequiredCaveEntranceV1> = None;
    for chunk_z in origin.z.saturating_sub(SEARCH_RADIUS_CHUNKS)
        ..=origin.z.saturating_add(SEARCH_RADIUS_CHUNKS)
    {
        for chunk_y in min_chunk_y..=max_chunk_y {
            for chunk_x in origin.x.saturating_sub(SEARCH_RADIUS_CHUNKS)
                ..=origin.x.saturating_add(SEARCH_RADIUS_CHUNKS)
            {
                let chunk = ChunkCoordinate::new(chunk_x, chunk_y, chunk_z);
                let Ok(requests) = plan.cave_face_field_requests(chunk) else {
                    continue;
                };
                for request in requests {
                    let Some(assertion) = request.assertion() else {
                        continue;
                    };
                    let Some(candidate) = entrance_from_assertion(plan, chunk, assertion, edge)
                    else {
                        continue;
                    };
                    if entrance_is_better(plan, best.as_ref(), &candidate, origin, edge) {
                        best = Some(candidate);
                    }
                }
            }
        }
    }
    best
}

fn entrance_from_assertion(
    plan: &GenerationPlanV1,
    chunk: ChunkCoordinate,
    assertion: CaveFieldPortalAssertionV1,
    edge: i64,
) -> Option<RequiredCaveEntranceV1> {
    let aperture = portal_aperture_voxel(chunk, assertion, edge)?;
    let occupancy = plan.cave_occupancy_arbitration(
        i64::from(aperture[0]),
        i64::from(aperture[1]),
        i64::from(aperture[2]),
    );
    if !occupancy.is_finally_void() {
        return None;
    }
    let surface = plan.terrain_height(i64::from(aperture[0]), i64::from(aperture[2]));
    if i64::from(aperture[1]) > i64::from(surface) {
        return None;
    }
    let inward = step_inward(aperture, assertion.tangent_face());
    let destination = if plan
        .cave_occupancy_arbitration(
            i64::from(inward[0]),
            i64::from(inward[1]),
            i64::from(inward[2]),
        )
        .is_finally_void()
    {
        inward
    } else {
        aperture
    };
    Some(RequiredCaveEntranceV1 {
        aperture,
        surface_footing: [aperture[0], surface, aperture[2]],
        destination,
        face: assertion.tangent_face(),
        position_u_voxel: assertion.position_u_voxel(),
        position_v_voxel: assertion.position_v_voxel(),
        clearance_radius_voxels: assertion.clearance_radius_voxels(),
    })
}

fn entrance_is_better(
    plan: &GenerationPlanV1,
    current: Option<&RequiredCaveEntranceV1>,
    candidate: &RequiredCaveEntranceV1,
    origin: ChunkCoordinate,
    edge: i64,
) -> bool {
    let Some(current) = current else {
        return true;
    };
    let candidate_key = entrance_sort_key(plan, candidate, origin, edge);
    let current_key = entrance_sort_key(plan, current, origin, edge);
    candidate_key > current_key
}

fn entrance_sort_key(
    plan: &GenerationPlanV1,
    entrance: &RequiredCaveEntranceV1,
    origin: ChunkCoordinate,
    edge: i64,
) -> (
    bool,
    i32,
    std::cmp::Reverse<u32>,
    i32,
    i32,
    i32,
    ChunkFaceV1,
) {
    let dx = i64::from(entrance.aperture[0]).div_euclid(edge) - i64::from(origin.x);
    let dz = i64::from(entrance.aperture[2]).div_euclid(edge) - i64::from(origin.z);
    let distance = u32::try_from(dx.abs().max(dz.abs())).unwrap_or(u32::MAX);
    let [x, y, z] = entrance.aperture;
    let topology = plan.has_cave_topology_layer()
        && plan.cave_in_declared_influence(i64::from(x), i64::from(y), i64::from(z));
    (
        topology,
        entrance.aperture[1],
        std::cmp::Reverse(distance),
        entrance.aperture[0],
        entrance.aperture[2],
        entrance.aperture[1],
        entrance.face,
    )
}

#[allow(
    clippy::many_single_char_names,
    reason = "portal U/V and world X/Y/Z are the explicit aperture terms"
)]
fn portal_aperture_voxel(
    chunk: ChunkCoordinate,
    assertion: CaveFieldPortalAssertionV1,
    edge: i64,
) -> Option<[i32; 3]> {
    let origin_x = i64::from(chunk.x).saturating_mul(edge);
    let origin_y = i64::from(chunk.y).saturating_mul(edge);
    let origin_z = i64::from(chunk.z).saturating_mul(edge);
    let u = i64::from(assertion.position_u_voxel());
    let v = i64::from(assertion.position_v_voxel());
    let (x, y, z) = match assertion.tangent_face() {
        ChunkFaceV1::NegativeX => (
            origin_x,
            origin_y.saturating_add(u),
            origin_z.saturating_add(v),
        ),
        ChunkFaceV1::PositiveX => (
            origin_x.saturating_add(edge).saturating_sub(1),
            origin_y.saturating_add(u),
            origin_z.saturating_add(v),
        ),
        ChunkFaceV1::NegativeY => (
            origin_x.saturating_add(u),
            origin_y,
            origin_z.saturating_add(v),
        ),
        ChunkFaceV1::PositiveY => (
            origin_x.saturating_add(u),
            origin_y.saturating_add(edge).saturating_sub(1),
            origin_z.saturating_add(v),
        ),
        ChunkFaceV1::NegativeZ => (
            origin_x.saturating_add(u),
            origin_y.saturating_add(v),
            origin_z,
        ),
        ChunkFaceV1::PositiveZ => (
            origin_x.saturating_add(u),
            origin_y.saturating_add(v),
            origin_z.saturating_add(edge).saturating_sub(1),
        ),
    };
    Some([
        i32::try_from(x).ok()?,
        i32::try_from(y).ok()?,
        i32::try_from(z).ok()?,
    ])
}

const fn step_inward(aperture: [i32; 3], face: ChunkFaceV1) -> [i32; 3] {
    match face {
        ChunkFaceV1::NegativeX => [aperture[0].saturating_add(1), aperture[1], aperture[2]],
        ChunkFaceV1::PositiveX => [aperture[0].saturating_sub(1), aperture[1], aperture[2]],
        ChunkFaceV1::NegativeY => [aperture[0], aperture[1].saturating_add(1), aperture[2]],
        ChunkFaceV1::PositiveY => [aperture[0], aperture[1].saturating_sub(1), aperture[2]],
        ChunkFaceV1::NegativeZ => [aperture[0], aperture[1], aperture[2].saturating_add(1)],
        ChunkFaceV1::PositiveZ => [aperture[0], aperture[1], aperture[2].saturating_sub(1)],
    }
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};

    use super::{
        WORLDGEN_PLAN_REVISION, compile_host_worldgen_inspect, generate_plan_chunks,
        host_spawn_bounds, hydrology_occupancy_config, hydrology_occupancy_config_for,
        natural_layer_config, occupancy_candidate_is_current, production_terrain_config,
        provider_offers, provider_revision, required_cave_entrance, spawn_center, spine_config,
        spine_config_for, validated_spawn,
    };
    use latticeaxiom_core::CanonicalHash;
    use latticeaxiom_runtime_contracts::{
        EngineEpoch, WorldEpoch, WorldgenInspectBodyV1, WorldgenInspectKindV1,
    };
    use latticeaxiom_storage::ChunkCoordinate;
    use latticeaxiom_worldgen::{
        AuthoredWorldgenBindingsV1, CellEpochStateV1, ChunkFaceV1, ChunkGenerationOutcomeV1,
        ChunkGenerationRequestV1, D4MaterialRoleV1, D7_NATURAL_BLOCK_COUNT, DimensionId,
        ExistingSnapshotEvidenceV1, GenerationPlanInputV1, GenerationPlanV1, HydrologyFlowV1,
        HydrologyFluidBindingsV1, HydrologyOccupancyInputV1, HydrologyOccupancyKindV1,
        NaturalLayerConfigV1, NaturalLayerInputV1, ORIGIN_NEIGHBORHOOD_CHUNK_COORDINATES_V1,
        PlanActivationIdV1, PlanningCellCoordinateV1, ProviderSlotV1, TerrainConfigV2,
        TerrainStyleV1, WorldSeedV1, WorldgenConfigV1, WorldgenLimitsV1,
    };

    const AUTHORED_BINDINGS_JSON: &str =
        include_str!("../../../../../worldgen/data/authored-block-bindings-v1.json");
    const D7_BLOCK_IDS: &str = include_str!("../../../../../blocks/data/goldens/d7-block-ids.txt");

    #[test]
    fn production_world_scale_matches_the_accepted_vertical_and_planning_contract() {
        let terrain = production_terrain_config();
        let config = spine_config_for(&terrain);

        assert_eq!(config.chunk_edge_voxels, 32);
        assert_eq!(config.world_floor_y, -128);
        assert_eq!(config.world_ceiling_y, 383);
        assert_eq!(config.world_ceiling_y - config.world_floor_y + 1, 512);
        assert_eq!(config.height_noise_scale_voxels, 32);
        assert_eq!(config.transition_width_voxels, 48);
        assert_eq!(config.temperate_base_height, terrain.world.sea_level_y);
        assert_eq!(config.temperate_relief, 128);
        assert_eq!(config.arid_base_height, terrain.world.sea_level_y);
        assert_eq!(config.arid_relief, 128);
        assert_eq!(
            i32::from(config.chunk_edge_voxels) * i32::from(config.planning_cell_edge_chunks),
            256,
            "climate planning cells use a broad 256-meter physical scale"
        );
    }

    #[test]
    fn terrain_morphology_contract_has_distinct_provider_revisions() {
        assert_eq!(WORLDGEN_PLAN_REVISION, 7);
        assert_eq!(provider_revision(ProviderSlotV1::GenerationCoordinator), 11);
        assert_eq!(provider_revision(ProviderSlotV1::Materializer), 11);
        assert_eq!(provider_revision(ProviderSlotV1::Vegetation), 5);
        assert_eq!(provider_revision(ProviderSlotV1::CaveTopology), 9);
        assert_eq!(provider_revision(ProviderSlotV1::StyleSelector), 9);
        assert_eq!(provider_revision(ProviderSlotV1::TerrainTransition), 8);
        assert_eq!(provider_revision(ProviderSlotV1::Hydrology), 4);
        assert_eq!(provider_revision(ProviderSlotV1::Geology), 3);
    }

    #[test]
    fn production_natural_layer_keeps_rivers_and_boreal_vegetation_enabled() {
        let spine = spine_config();
        let terrain = production_terrain_config();
        let config =
            natural_layer_config(&spine, &terrain).expect("production natural config fits spine");
        let defaults = NaturalLayerConfigV1::default();

        assert_eq!(
            config.river_incision_voxels,
            terrain.water.river_depth_voxels
        );
        assert_ne!(config.river_incision_voxels, defaults.river_incision_voxels);
        assert_eq!(
            config.pine_threshold_per_1024,
            defaults.pine_threshold_per_1024
        );
        assert_eq!(
            config.moss_threshold_per_1024,
            defaults.moss_threshold_per_1024
        );
        assert!(config.river_incision_voxels > 0);
        assert!(config.pine_threshold_per_1024 > 0);
        assert!(config.moss_threshold_per_1024 > 0);
    }

    #[test]
    #[allow(
        clippy::too_many_lines,
        reason = "the fixed 3D corpus keeps tree, ground-cover, terrain, and water assertions together"
    )]
    fn semantic_vegetation_is_supported_by_final_materialized_terrain() {
        let plan = semantic_vegetation_plan(0, false);
        let woodland_cover = plan
            .role_target(D4MaterialRoleV1::WoodlandGroundCover)
            .clone();
        let moss = plan.role_target(D4MaterialRoleV1::Moss).clone();
        let temperate_surface = plan.role_target(D4MaterialRoleV1::TemperateSurface).clone();
        let temperate_soils = [
            D4MaterialRoleV1::TemperateSubsurface,
            D4MaterialRoleV1::CoarseDirt,
            D4MaterialRoleV1::RootedDirt,
            D4MaterialRoleV1::TemperateClay,
        ]
        .map(|role| plan.role_target(role).clone())
        .into_iter()
        .collect::<BTreeSet<_>>();
        let log_blocks = [D4MaterialRoleV1::WoodlandLog, D4MaterialRoleV1::BorealLog]
            .map(|role| plan.role_target(role).clone())
            .into_iter()
            .collect::<BTreeSet<_>>();
        let leaf_blocks = [
            D4MaterialRoleV1::WoodlandLeaves,
            D4MaterialRoleV1::BorealLeaves,
        ]
        .map(|role| plan.role_target(role).clone())
        .into_iter()
        .collect::<BTreeSet<_>>();
        let edge = i64::from(plan.config().chunk_edge_voxels);
        let mut ground_cover_count = 0_u64;
        let mut tree_voxel_count = 0_u64;
        let mut corrected_surface_count = 0_u64;
        let mut dry_temperate_surface_count = 0_u64;
        let mut density_corrected_top_count = 0_u64;
        let mut exposed_temperate_soil_count = 0_u64;
        let mut flat_temperate_soil_depths = BTreeMap::<(i64, i64), u8>::new();
        let mut supported_roots = BTreeSet::<(i64, i64, i64)>::new();

        for chunk_z in -1_i32..=1 {
            for chunk_x in -1_i32..=1 {
                let origin_x = i64::from(chunk_x).saturating_mul(edge);
                let origin_z = i64::from(chunk_z).saturating_mul(edge);
                let mut minimum_y = i64::MAX;
                let mut maximum_y = i64::MIN;
                let mut final_surface_ys = Vec::with_capacity(
                    usize::try_from(edge.saturating_mul(edge))
                        .expect("fixture column count fits usize"),
                );
                let mut dry_temperate_columns = Vec::with_capacity(
                    usize::try_from(edge.saturating_mul(edge))
                        .expect("fixture column count fits usize"),
                );
                for local_z in 0..edge {
                    for local_x in 0..edge {
                        let world_x = origin_x.saturating_add(local_x);
                        let world_z = origin_z.saturating_add(local_z);
                        let height = i64::from(plan.terrain_height(world_x, world_z));
                        minimum_y = minimum_y.min(height.saturating_sub(10));
                        maximum_y = maximum_y.max(height.saturating_add(10));
                        let final_surface_y = final_surface_y(&plan, world_x, world_z);
                        final_surface_ys.push(final_surface_y);
                        dry_temperate_columns.push(
                            plan.material_style(world_x, world_z)
                                == TerrainStyleV1::TemperateWoodland
                                && !plan
                                    .hydrology_occupancy_sample(
                                        world_x,
                                        final_surface_y.saturating_add(1),
                                        world_z,
                                    )
                                    .is_some_and(|sample| sample.is_occupied()),
                        );
                    }
                }
                let edge_usize = usize::try_from(edge).expect("fixture chunk edge fits usize");
                for local_z in 1..edge.saturating_sub(1) {
                    for local_x in 1..edge.saturating_sub(1) {
                        let index =
                            usize::try_from(local_z.saturating_mul(edge).saturating_add(local_x))
                                .expect("fixture column index fits usize");
                        let surface_y = final_surface_ys[index];
                        let flat = [
                            final_surface_ys[index - 1],
                            final_surface_ys[index + 1],
                            final_surface_ys[index - edge_usize],
                            final_surface_ys[index + edge_usize],
                        ]
                        .into_iter()
                        .all(|neighbor| neighbor == surface_y);
                        if dry_temperate_columns[index] && flat {
                            flat_temperate_soil_depths.insert(
                                (
                                    origin_x.saturating_add(local_x),
                                    origin_z.saturating_add(local_z),
                                ),
                                0,
                            );
                        }
                    }
                }
                for chunk_y in minimum_y.div_euclid(edge)..=maximum_y.div_euclid(edge) {
                    let coordinate = ChunkCoordinate::new(
                        chunk_x,
                        i32::try_from(chunk_y).expect("fixture chunk Y fits i32"),
                        chunk_z,
                    );
                    let outcome = plan
                        .generate(plan.vacant_generation_request(coordinate).unwrap())
                        .expect("semantic vegetation chunk generates");
                    let ChunkGenerationOutcomeV1::Prepared(candidate) = outcome else {
                        panic!("semantic vegetation scan requires a new candidate");
                    };
                    let origin_y = chunk_y.saturating_mul(edge);
                    for local_y in 0..edge {
                        for local_z in 0..edge {
                            for local_x in 0..edge {
                                let block = candidate
                                    .draft()
                                    .block_at(
                                        u16::try_from(local_x).expect("local X fits u16"),
                                        u16::try_from(local_y).expect("local Y fits u16"),
                                        u16::try_from(local_z).expect("local Z fits u16"),
                                    )
                                    .expect("local voxel is inside the draft");
                                let world_x = origin_x.saturating_add(local_x);
                                let world_y = origin_y.saturating_add(local_y);
                                let world_z = origin_z.saturating_add(local_z);
                                let column_index = usize::try_from(
                                    local_z.saturating_mul(edge).saturating_add(local_x),
                                )
                                .expect("fixture column index fits usize");
                                let final_surface_y = final_surface_ys[column_index];
                                let dry_temperate = dry_temperate_columns[column_index];
                                if dry_temperate && world_y == final_surface_y {
                                    dry_temperate_surface_count =
                                        dry_temperate_surface_count.saturating_add(1);
                                    assert_eq!(
                                        block, &temperate_surface,
                                        "final dry temperate top at ({world_x},{world_y},{world_z}) must retain its biome surface"
                                    );
                                    density_corrected_top_count = density_corrected_top_count
                                        .saturating_add(u64::from(
                                            final_surface_y
                                                != i64::from(plan.terrain_height(world_x, world_z)),
                                        ));
                                }
                                if dry_temperate
                                    && temperate_soils.contains(block)
                                    && world_y < final_surface_y
                                    && (1..edge.saturating_sub(1)).contains(&local_x)
                                    && (1..edge.saturating_sub(1)).contains(&local_z)
                                {
                                    let west = final_surface_ys[column_index - 1];
                                    let east = final_surface_ys[column_index + 1];
                                    let north = final_surface_ys[column_index.saturating_sub(
                                        usize::try_from(edge)
                                            .expect("fixture chunk edge fits usize"),
                                    )];
                                    let south = final_surface_ys[column_index.saturating_add(
                                        usize::try_from(edge)
                                            .expect("fixture chunk edge fits usize"),
                                    )];
                                    let lowest_neighbor = west.min(east).min(north).min(south);
                                    if final_surface_y.saturating_sub(lowest_neighbor) >= 2
                                        && world_y > lowest_neighbor
                                    {
                                        exposed_temperate_soil_count =
                                            exposed_temperate_soil_count.saturating_add(1);
                                    }
                                }
                                if dry_temperate
                                    && temperate_soils.contains(block)
                                    && let Some(depth) =
                                        flat_temperate_soil_depths.get_mut(&(world_x, world_z))
                                {
                                    *depth = depth.saturating_add(1);
                                }
                                if block == &woodland_cover || block == &moss {
                                    ground_cover_count = ground_cover_count.saturating_add(1);
                                    assert_final_vegetation_support(
                                        &plan, world_x, world_y, world_z,
                                    );
                                    corrected_surface_count = corrected_surface_count
                                        .saturating_add(u64::from(
                                            world_y.saturating_sub(1)
                                                != i64::from(plan.terrain_height(world_x, world_z)),
                                        ));
                                }
                                if log_blocks.contains(block) || leaf_blocks.contains(block) {
                                    tree_voxel_count = tree_voxel_count.saturating_add(1);
                                    assert_final_tree_clearance(&plan, world_x, world_y, world_z);
                                }
                                if log_blocks.contains(block)
                                    && plan.terrain_materializes_as_solid(
                                        world_x,
                                        world_y.saturating_sub(1),
                                        world_z,
                                    )
                                {
                                    supported_roots.insert((world_x, world_y, world_z));
                                }
                            }
                        }
                    }
                }
            }
        }

        assert!(
            ground_cover_count > 0,
            "fixture must emit checked ground cover"
        );
        assert!(
            tree_voxel_count > 0,
            "fixture must emit checked tree voxels"
        );
        assert!(
            !supported_roots.is_empty(),
            "fixture must emit at least one checked tree root"
        );
        for (x, root_y, z) in supported_roots {
            assert_final_vegetation_support(&plan, x, root_y, z);
            corrected_surface_count = corrected_surface_count.saturating_add(u64::from(
                root_y.saturating_sub(1) != i64::from(plan.terrain_height(x, z)),
            ));
        }
        assert!(
            corrected_surface_count > 0,
            "fixture must exercise a 3D-density surface that differs from height intent"
        );
        assert!(
            dry_temperate_surface_count > 0,
            "fixture must exercise dry temperate final surfaces"
        );
        assert!(
            density_corrected_top_count > 0,
            "fixture must exercise final tops below or above preliminary height intent"
        );
        assert_eq!(
            exposed_temperate_soil_count, 0,
            "two-or-more-voxel temperate drops must expose rock rather than subsurface soil"
        );
        let distinct_flat_depths = flat_temperate_soil_depths
            .values()
            .copied()
            .collect::<BTreeSet<_>>();
        assert!(
            distinct_flat_depths.len() >= 2,
            "flat temperate corpus must materialize variable soil depth, got {distinct_flat_depths:?}"
        );
    }

    #[test]
    fn dry_vegetated_biomes_keep_their_owned_final_surface_role() {
        let plan = semantic_vegetation_plan(0, false);
        assert_final_surface_role(
            &plan,
            TerrainStyleV1::TemperateWoodland,
            D4MaterialRoleV1::TemperateSurface,
        );
        assert_final_surface_role(&plan, TerrainStyleV1::BorealWetland, D4MaterialRoleV1::Snow);
    }

    #[test]
    fn production_hydrology_uses_minecraft_scale_sea_level_with_bounded_memory() {
        let spine = spine_config();
        let config = hydrology_occupancy_config();

        config
            .validate_for_world(&spine)
            .expect("production sea level fits the world column");
        assert_eq!(config.sea_level_y, Some(64));
        assert_eq!(
            config.max_cells_per_chunk,
            u32::from(spine.chunk_edge_voxels).pow(3)
        );
        assert_eq!(config.max_in_flight_bytes, 8 * 1_024 * 1_024);
    }

    #[test]
    fn production_lowlands_materialize_still_water_at_sea_level() {
        let plan = occupancy_plan(0, false);
        let (x, z, surface_y) = (-2_048_i64..=2_048)
            .step_by(8)
            .find_map(|z| {
                (-2_048_i64..=2_048).step_by(8).find_map(|x| {
                    let surface_y = plan.terrain_height(x, z);
                    let in_channel = plan
                        .river_sample(x, z)
                        .is_some_and(latticeaxiom_worldgen::RiverSampleV1::in_channel);
                    (surface_y < 64 && !in_channel).then_some((x, z, surface_y))
                })
            })
            .expect("production terrain contains non-river lowlands below sea level");
        assert_eq!(
            plan.hydrology_occupancy_sample(x, i64::from(surface_y) + 1, z)
                .expect("lowest surface-water sample")
                .kind(),
            HydrologyOccupancyKindV1::SurfaceWater
        );
        let sample = plan
            .hydrology_occupancy_sample(x, 64, z)
            .expect("production hydrology sample");
        assert_eq!(sample.kind(), HydrologyOccupancyKindV1::SurfaceWater);
        assert_eq!(sample.flow(), HydrologyFlowV1::Still);

        let edge = i64::from(plan.config().chunk_edge_voxels);
        let coordinate = ChunkCoordinate::new(
            i32::try_from(x.div_euclid(edge)).expect("sample chunk X fits"),
            64_i32.div_euclid(i32::from(plan.config().chunk_edge_voxels)),
            i32::try_from(z.div_euclid(edge)).expect("sample chunk Z fits"),
        );
        let candidate = plan
            .hydrology_occupancy_candidate(coordinate)
            .expect("lowland occupancy candidate stays within production budgets");
        let local = [
            u16::try_from(x.rem_euclid(edge)).expect("local X fits"),
            u16::try_from(64_i64.rem_euclid(edge)).expect("local Y fits"),
            u16::try_from(z.rem_euclid(edge)).expect("local Z fits"),
        ];
        assert!(candidate.cells().iter().any(|cell| {
            [cell.x(), cell.y(), cell.z()] == local
                && cell.kind() == HydrologyOccupancyKindV1::SurfaceWater
                && cell.flow() == HydrologyFlowV1::Still
        }));
    }

    #[test]
    fn production_lake_basins_materialize_still_water_above_their_bed() {
        let plan = occupancy_plan(0, false);
        let (x, z, bed_y, water_y) = (-2_048_i64..=2_048)
            .step_by(8)
            .find_map(|z| {
                (-2_048_i64..=2_048).step_by(8).find_map(|x| {
                    let bed_y = plan.terrain_height(x, z);
                    if plan
                        .river_sample(x, z)
                        .is_some_and(latticeaxiom_worldgen::RiverSampleV1::in_channel)
                    {
                        return None;
                    }
                    plan.surface_water_level(x, z)
                        .filter(|water_y| *water_y > bed_y)
                        .map(|water_y| (x, z, bed_y, water_y))
                })
            })
            .expect("balanced terrain contains a deterministic inland lake");
        for y in i64::from(bed_y).saturating_add(1)..=i64::from(water_y) {
            let sample = plan
                .hydrology_occupancy_sample(x, y, z)
                .expect("lake occupancy sample");
            assert_eq!(sample.kind(), HydrologyOccupancyKindV1::SurfaceWater);
            assert_eq!(sample.flow(), HydrologyFlowV1::Still);
        }
    }

    #[test]
    fn underground_river_option_does_not_disable_surface_channels() {
        let mut terrain = production_terrain_config();
        terrain.water.underground_rivers = false;
        let plan = occupancy_plan_with_terrain(0, false, terrain);
        let (x, z) = (-1_024_i64..=1_024)
            .find_map(|z| {
                (-1_024_i64..=1_024).find_map(|x| {
                    plan.river_sample(x, z)
                        .is_some_and(latticeaxiom_worldgen::RiverSampleV1::in_channel)
                        .then_some((x, z))
                })
            })
            .expect("balanced terrain contains a river channel");
        assert!(
            !plan
                .drainage_sample(x, z)
                .expect("drainage sample")
                .is_connected()
        );
        let surface_y = plan.terrain_height(x, z);
        assert_eq!(
            plan.hydrology_occupancy_sample(x, i64::from(surface_y) + 1, z)
                .expect("surface channel sample")
                .kind(),
            HydrologyOccupancyKindV1::SurfaceChannel
        );
    }

    #[test]
    fn production_terrain_uses_broad_vertical_relief_without_adjacent_spikes() {
        let plan = occupancy_plan(0, false);
        let mut minimum = i32::MAX;
        let mut maximum = i32::MIN;
        for z in (-16_384_i64..=16_384).step_by(64) {
            for x in (-16_384_i64..=16_384).step_by(64) {
                let height = plan.terrain_height(x, z);
                minimum = minimum.min(height);
                maximum = maximum.max(height);
            }
        }

        let mut maximum_step = 0_u32;
        for z in -256_i64..=256 {
            for x in -256_i64..=256 {
                let height = plan.terrain_height(x, z);
                maximum_step = maximum_step
                    .max(height.abs_diff(plan.terrain_height(x.saturating_add(1), z)))
                    .max(height.abs_diff(plan.terrain_height(x, z.saturating_add(1))));
            }
        }
        assert!(minimum >= plan.config().world_floor_y);
        assert!(maximum <= plan.config().world_ceiling_y);
        assert!(minimum <= 63, "production corpus lacks lowlands: {minimum}");
        assert!(maximum >= 160, "production corpus lacks peaks: {maximum}");
        assert!(maximum.saturating_sub(minimum) >= 80);
        assert!(
            maximum_step <= 12,
            "production corpus contains a {maximum_step}-voxel adjacent spike"
        );
    }

    #[test]
    fn plan_chunks_are_not_limited_to_the_d4_origin_neighborhood() {
        let plan = fixture_plan(42);
        assert!(plan.has_natural_layer());
        let outside = ChunkCoordinate::new(3, 2, -2);
        assert!(
            !ORIGIN_NEIGHBORHOOD_CHUNK_COORDINATES_V1.contains(&outside),
            "fixture chunk must sit outside the D4 four-chunk neighborhood"
        );
        let region = generate_plan_chunks(&plan, [outside]).expect("V5 plan generates a far chunk");
        assert_eq!(region.len(), 1);
        assert!(region.candidate(outside).is_some());
    }

    #[test]
    fn validated_spawn_is_the_same_surface_cell_for_the_same_seed() {
        let bindings = authored_bindings();
        let first = validated_spawn(&fixture_plan(42), &bindings).expect("seed 42 has a spawn");
        let second = validated_spawn(&fixture_plan(42), &bindings).expect("seed 42 is stable");
        assert_eq!(first, second);
        let center = spawn_center(first).expect("spawn voxels fit the player domain");
        let feet = first.feet();
        let x = i16::try_from(feet[0]).expect("spawn x fits i16");
        let z = i16::try_from(feet[2]).expect("spawn z fits i16");
        let footing_y = i16::try_from(first.footing()[1]).expect("spawn y fits i16");
        assert!((center.x - (f32::from(x) + 0.5)).abs() < f32::EPSILON);
        assert!((center.z - (f32::from(z) + 0.5)).abs() < f32::EPSILON);
        assert!(center.y > f32::from(footing_y));
    }

    #[test]
    fn topology_candidates_leave_bounded_fallbacks_across_seed_corpus() {
        let bindings = authored_bindings();
        for seed in 0..32 {
            let plan = fixture_plan(seed);
            let bounds = host_spawn_bounds(&plan).expect("spawn bounds remain valid");
            assert!(
                bounds.len() <= 32,
                "seed {seed} produced {} bounds",
                bounds.len()
            );
            assert!(
                bounds.len() > 16,
                "seed {seed} did not retain dry-land fallback windows"
            );
            assert!(
                validated_spawn(&plan, &bindings).is_ok(),
                "seed {seed} must select a safe spawn"
            );
        }
    }

    #[test]
    fn required_cave_entrance_reuses_worldgen_field_portals() {
        let plan = fixture_plan(42);
        let bindings = authored_bindings();
        let spawn = validated_spawn(&plan, &bindings).expect("seed 42 has a spawn");
        let entrance = required_cave_entrance(&plan, spawn.chunk())
            .expect("host plan must expose a required cave field portal");
        let [x, y, z] = entrance.aperture();
        assert!(
            plan.cave_occupancy_arbitration(i64::from(x), i64::from(y), i64::from(z))
                .is_finally_void()
        );
        let [dx, dy, dz] = entrance.destination();
        assert!(
            plan.cave_occupancy_arbitration(i64::from(dx), i64::from(dy), i64::from(dz))
                .is_finally_void()
        );
        assert!(entrance.clearance_radius_voxels() > 0);
        assert!(entrance.surface_footing()[1] >= entrance.aperture()[1]);
        let shuffled = required_cave_entrance(&plan, spawn.chunk());
        assert_eq!(shuffled, Some(entrance));
    }

    #[test]
    fn natural_bytes_are_stable_under_shuffled_chunk_and_offer_order() {
        let chunks = [
            ChunkCoordinate::new(-3, 0, -2),
            ChunkCoordinate::new(-1, 1, 1),
            ChunkCoordinate::new(0, 0, 0),
            ChunkCoordinate::new(2, 2, -4),
            ChunkCoordinate::new(4, 0, 3),
        ];
        let forward = fixture_plan_with_offer_order(42, false);
        let reversed = fixture_plan_with_offer_order(42, true);
        assert_eq!(
            forward.generation_input_hash(),
            reversed.generation_input_hash()
        );
        let first = generate_sorted(&forward, chunks);
        let second = generate_sorted(&reversed, chunks.into_iter().rev());
        assert_eq!(first, second);
    }

    #[test]
    fn territory_river_geology_and_vegetation_samples_are_seam_free() {
        let plan = fixture_plan(42);
        let mut crossed = false;
        for z in -48..48 {
            for x in -48..48 {
                let here = plan.territory_query(x, z);
                let east = plan.territory_query(x.saturating_add(1), z);
                let here_river = plan.river_sample(x, z).expect("natural river sample");
                let east_river = plan
                    .river_sample(x.saturating_add(1), z)
                    .expect("adjacent river sample");
                if here.winner() != east.winner()
                    && here_river.in_channel()
                    && east_river.in_channel()
                {
                    let distance_delta = here_river
                        .distance_voxels()
                        .abs_diff(east_river.distance_voxels());
                    assert!(
                        distance_delta <= 1,
                        "river distance seam {distance_delta} at ({x},{z}): {} -> {}",
                        here_river.distance_voxels(),
                        east_river.distance_voxels()
                    );
                }
                let height = plan.terrain_height(x, z);
                let geology = plan
                    .geologic_sample(x, i64::from(height).saturating_sub(2), z)
                    .expect("geology sample");
                assert!(geology.depth() >= 0);
                if here.winner() != east.winner() {
                    crossed = true;
                    let delta = plan
                        .terrain_height(x, z)
                        .abs_diff(plan.terrain_height(x.saturating_add(1), z));
                    assert!(
                        delta <= 4,
                        "height seam {delta} at ({x},{z}) between {:?} and {:?}",
                        here.winner(),
                        east.winner()
                    );
                }
            }
        }
        assert!(
            crossed
                || matches!(
                    plan.territory_query(0, 0).winner(),
                    TerrainStyleV1::TemperateWoodland
                        | TerrainStyleV1::AridBadlands
                        | TerrainStyleV1::BorealWetland
                )
        );
    }

    #[test]
    fn compatible_provider_update_reuses_materialized_snapshot_bytes() {
        let old_plan = fixture_plan(42);
        let coordinate = ChunkCoordinate::new(-2, 0, 3);
        let region = generate_plan_chunks(&old_plan, [coordinate]).expect("old chunk generates");
        let candidate = region.candidate(coordinate).expect("old candidate");
        let existing = ExistingSnapshotEvidenceV1::new(
            old_plan.dimension().clone(),
            coordinate,
            PlanningCellCoordinateV1::from_chunk(
                coordinate,
                old_plan.config().planning_cell_edge_chunks,
            ),
            old_plan.generation_epoch(),
            latticeaxiom_storage::ChunkRevision::new(1),
            1,
            candidate.snapshot_bytes().to_vec(),
            candidate.checksum(),
        )
        .expect("candidate checksum matches");
        let mut natural = provider_offers(None, ProviderSlotV1::NATURAL).expect("natural offers");
        let position = natural
            .iter()
            .position(|offer| offer.slot() == ProviderSlotV1::Vegetation)
            .expect("vegetation slot");
        natural[position] = latticeaxiom_worldgen::ProviderOfferV1::new(
            ProviderSlotV1::Vegetation,
            latticeaxiom_worldgen::ProviderGenerationIdentityV1::new(
                "latticeaxiom:worldgen-provider/vegetation@1"
                    .parse()
                    .expect("vegetation provider"),
                std::num::NonZeroU32::MIN,
                9,
                CanonicalHash::digest(b"vegetation-implementation-v9"),
            ),
        );
        let new_plan = compile_fixture(42, false, natural);
        assert_ne!(old_plan.generation_epoch(), new_plan.generation_epoch());
        let reused = new_plan
            .generate(ChunkGenerationRequestV1::new(
                coordinate,
                Some(existing.clone()),
                CellEpochStateV1::Frozen(old_plan.generation_epoch()),
                latticeaxiom_worldgen::AdjacentEpochSnapshotV1::all_unassigned(
                    PlanningCellCoordinateV1::from_chunk(
                        coordinate,
                        new_plan.config().planning_cell_edge_chunks,
                    ),
                )
                .expect("adjacent snapshot"),
                Vec::new(),
            ))
            .expect("existing snapshot wins");
        assert_eq!(reused, ChunkGenerationOutcomeV1::Existing(existing));
    }

    #[test]
    fn headless_scan_finds_d7_natural_resource_classes() {
        let plan = fixture_plan(42);
        let bindings = authored_bindings();
        assert!(
            bindings.catalog_closure().expect("catalog").blocks().len() >= D7_NATURAL_BLOCK_COUNT
        );
        for purpose in D4MaterialRoleV1::ALL
            .into_iter()
            .chain(D4MaterialRoleV1::NATURAL)
        {
            let _ = plan.role_target(purpose);
        }
        let mut present = BTreeSet::new();
        let mut styles = BTreeSet::new();
        let edge = i32::from(plan.config().planning_cell_edge_chunks);
        let mut sampled_cells = (-2_i32..=2)
            .flat_map(|cell_z| (-2_i32..=2).map(move |cell_x| (cell_x, cell_z)))
            .collect::<BTreeSet<_>>();
        for cell_z in -8_i32..=8 {
            for cell_x in -8_i32..=8 {
                let world_x = i64::from(cell_x)
                    * i64::from(plan.config().chunk_edge_voxels)
                    * i64::from(plan.config().planning_cell_edge_chunks);
                let world_z = i64::from(cell_z)
                    * i64::from(plan.config().chunk_edge_voxels)
                    * i64::from(plan.config().planning_cell_edge_chunks);
                let style = plan.territory_query(world_x, world_z).winner();
                if styles.insert(style) {
                    sampled_cells.insert((cell_x, cell_z));
                }
            }
        }
        assert!(styles.contains(&TerrainStyleV1::TemperateWoodland));
        assert!(styles.contains(&TerrainStyleV1::AridBadlands));
        assert!(styles.contains(&TerrainStyleV1::BorealWetland));
        for (cell_x, cell_z) in sampled_cells {
            let chunk_x = cell_x.saturating_mul(edge);
            let chunk_z = cell_z.saturating_mul(edge);
            for y in 0..=3 {
                let coordinate = ChunkCoordinate::new(chunk_x, y, chunk_z);
                let region = generate_plan_chunks(&plan, [coordinate]).expect("cell generates");
                let candidate = region.candidate(coordinate).expect("cell candidate");
                for block in candidate.draft().palette() {
                    present.insert(block.as_str().to_owned());
                }
            }
        }
        let golden = D7_BLOCK_IDS
            .lines()
            .filter(|line| !line.is_empty())
            .collect::<BTreeSet<_>>();
        assert_eq!(golden.len(), D7_NATURAL_BLOCK_COUNT);
        let classes: [&[&str]; 4] = [
            &["oak-log", "pine-log"],
            &["dirt", "coarse-dirt", "peat", "mud"],
            &["stone", "granite", "slate", "deepstone"],
            &["copper-ore", "coal-ore", "iron-ore", "tin-ore"],
        ];
        for class in classes {
            assert!(
                class
                    .iter()
                    .any(|path| present.iter().any(|id| id.ends_with(path))),
                "scan missing resource class {class:?} in {present:?}"
            );
        }
    }

    #[test]
    fn occupancy_candidates_are_rejected_when_the_plan_epoch_changes() {
        let first = occupancy_plan(42, false);
        let second = occupancy_plan(7, false);
        let coordinate = ChunkCoordinate::new(-4, 0, -3);
        let current = first
            .hydrology_occupancy_candidate(coordinate)
            .expect("current occupancy");
        let stale = second
            .hydrology_occupancy_candidate(coordinate)
            .expect("foreign occupancy");
        assert!(occupancy_candidate_is_current(&first, &current));
        assert!(!occupancy_candidate_is_current(&first, &stale));
        let again = occupancy_plan(42, true)
            .hydrology_occupancy_candidate(coordinate)
            .expect("shuffled occupancy");
        assert_eq!(
            current.canonical_bytes().expect("current bytes"),
            again.canonical_bytes().expect("shuffled bytes")
        );
        let east = first
            .hydrology_face_continuity(coordinate, ChunkFaceV1::PositiveX)
            .expect("east face");
        let west = first
            .hydrology_face_continuity(
                ChunkCoordinate::new(coordinate.x.saturating_add(1), coordinate.y, coordinate.z),
                ChunkFaceV1::NegativeX,
            )
            .expect("west face");
        assert_eq!(east.occupancy_hash(), west.occupancy_hash());
        let accounting = current.accounting();
        let chunk_edge = u64::from(first.config().chunk_edge_voxels);
        assert_eq!(
            accounting.cells_examined(),
            chunk_edge.saturating_pow(3),
            "a complete occupancy candidate examines each chunk voxel once"
        );
        assert!(
            accounting.cells_occupied()
                <= u64::from(hydrology_occupancy_config().max_cells_per_chunk)
        );
        assert!(
            accounting.queue_depth() <= u64::from(hydrology_occupancy_config().max_queue_depth)
        );
        assert!(
            accounting.in_flight_bytes()
                <= u64::from(hydrology_occupancy_config().max_in_flight_bytes)
        );
    }

    #[test]
    fn bounded_worldgen_inspect_projects_natural_facts() {
        let plan = fixture_plan(42);
        let bindings = authored_bindings();
        let spawn = validated_spawn(&plan, &bindings).expect("spawn");
        let report = compile_host_worldgen_inspect(
            &plan,
            &bindings,
            spawn,
            EngineEpoch::new(1),
            WorldEpoch::new(1),
        )
        .expect("inspect compiles");
        let kinds = report
            .records
            .iter()
            .map(|record| record.kind)
            .collect::<BTreeSet<_>>();
        assert!(kinds.contains(&WorldgenInspectKindV1::Territory));
        assert!(kinds.contains(&WorldgenInspectKindV1::River));
        assert!(kinds.contains(&WorldgenInspectKindV1::Geology));
        assert!(kinds.contains(&WorldgenInspectKindV1::Resource));
        assert!(kinds.contains(&WorldgenInspectKindV1::Vegetation));
        assert!(kinds.contains(&WorldgenInspectKindV1::Spawn));
        assert!(
            report
                .records
                .iter()
                .any(|record| matches!(record.body, WorldgenInspectBodyV1::Spawn(_)))
        );
        let shuffled = compile_host_worldgen_inspect(
            &plan,
            &bindings,
            spawn,
            EngineEpoch::new(1),
            WorldEpoch::new(1),
        )
        .expect("inspect is deterministic");
        assert_eq!(report, shuffled);
    }

    fn authored_bindings() -> AuthoredWorldgenBindingsV1 {
        AuthoredWorldgenBindingsV1::from_json(AUTHORED_BINDINGS_JSON.as_bytes())
            .expect("@terrenia/worldgen authored bindings must decode")
    }

    fn fixture_config() -> WorldgenConfigV1 {
        let mut config = spine_config();
        // Keep invariant fixtures on the original low-relief seam corpus;
        // production uses the broader relief configured above.
        config.temperate_relief = 2;
        config.arid_relief = 2;
        config
    }

    fn fixture_plan(seed: i64) -> GenerationPlanV1 {
        fixture_plan_with_offer_order(seed, false)
    }

    fn fixture_plan_with_offer_order(seed: i64, reverse: bool) -> GenerationPlanV1 {
        compile_fixture(
            seed,
            reverse,
            provider_offers(None, ProviderSlotV1::NATURAL).expect("natural offers"),
        )
    }

    fn occupancy_plan(seed: i64, reverse: bool) -> GenerationPlanV1 {
        occupancy_plan_with_terrain(seed, reverse, production_terrain_config())
    }

    fn occupancy_plan_with_terrain(
        seed: i64,
        reverse: bool,
        terrain: TerrainConfigV2,
    ) -> GenerationPlanV1 {
        let bindings = authored_bindings();
        let config = spine_config_for(&terrain);
        let mut d4 = provider_offers(None, ProviderSlotV1::ALL).expect("D4 offers");
        let mut natural = provider_offers(None, ProviderSlotV1::NATURAL).expect("natural offers");
        d4.extend(natural.iter().cloned());
        if reverse {
            d4.reverse();
            natural.reverse();
        }
        GenerationPlanV1::compile(
            GenerationPlanInputV1::new(
                dimension_id(),
                WorldSeedV1::from_integer(seed),
                config.clone(),
                1,
                PlanActivationIdV1::from_hash(CanonicalHash::digest(b"host-worldgen-test")),
                d4,
                bindings.d4_vocabulary().expect("authored D4 vocabulary"),
                bindings.role_bindings().expect("authored role bindings"),
                bindings
                    .catalog_closure()
                    .expect("authored catalog closure"),
                CanonicalHash::digest(b"authoritative-semantic-image"),
                vec![CanonicalHash::digest(b"lock-a")],
                WorldgenLimitsV1::default(),
            )
            .with_terrain_config(terrain)
            .with_surface_biome_terrain_programs(
                super::surface_biome_terrain_programs(CanonicalHash::digest("host-test-package"))
                    .expect("Terrenia terrain programs"),
            )
            .with_natural_layer(NaturalLayerInputV1::new(
                natural_layer_config(&config, &terrain).expect("natural config fits spine"),
                bindings
                    .natural_vocabulary()
                    .expect("authored natural vocabulary"),
                natural,
            ))
            .with_hydrology_occupancy(HydrologyOccupancyInputV1::new(
                hydrology_occupancy_config_for(&terrain),
                HydrologyFluidBindingsV1::new(
                    "fixture:fluid/water".parse().expect("fixture water"),
                    "fixture:fluid/lava".parse().expect("fixture lava"),
                    bindings
                        .predicate("place-water")
                        .expect("place-water")
                        .clone(),
                    bindings
                        .predicate("place-lava")
                        .expect("place-lava")
                        .clone(),
                )
                .expect("frozen hydrology fluids"),
            )),
        )
        .expect("host occupancy fixture plan compiles")
    }

    fn semantic_vegetation_plan(seed: i64, reverse: bool) -> GenerationPlanV1 {
        let bindings = authored_bindings();
        let terrain = production_terrain_config();
        let config = spine_config_for(&terrain);
        let mut d4 = provider_offers(None, ProviderSlotV1::ALL).expect("D4 offers");
        let mut natural = provider_offers(None, ProviderSlotV1::NATURAL).expect("natural offers");
        d4.extend(natural.iter().cloned());
        if reverse {
            d4.reverse();
            natural.reverse();
        }
        GenerationPlanV1::compile(
            GenerationPlanInputV1::new(
                dimension_id(),
                WorldSeedV1::from_integer(seed),
                config.clone(),
                5,
                PlanActivationIdV1::from_hash(CanonicalHash::digest(b"semantic-vegetation-test")),
                d4,
                bindings.d4_vocabulary().expect("authored D4 vocabulary"),
                bindings.role_bindings().expect("authored role bindings"),
                bindings
                    .catalog_closure()
                    .expect("authored catalog closure"),
                CanonicalHash::digest(b"authoritative-semantic-image"),
                vec![CanonicalHash::digest(b"lock-a")],
                WorldgenLimitsV1::default(),
            )
            .with_terrain_config(terrain)
            .with_surface_biome_terrain_programs(
                super::semantic_surface_biome_terrain_programs(
                    terrain,
                    CanonicalHash::digest("semantic-vegetation-package"),
                )
                .expect("Terrenia semantic terrain programs"),
            )
            .with_natural_layer(NaturalLayerInputV1::new(
                natural_layer_config(&config, &terrain).expect("natural config fits spine"),
                bindings
                    .natural_vocabulary()
                    .expect("authored natural vocabulary"),
                natural,
            ))
            .with_hydrology_occupancy(HydrologyOccupancyInputV1::new(
                hydrology_occupancy_config_for(&terrain),
                HydrologyFluidBindingsV1::new(
                    "fixture:fluid/water".parse().expect("fixture water"),
                    "fixture:fluid/lava".parse().expect("fixture lava"),
                    bindings
                        .predicate("place-water")
                        .expect("place-water")
                        .clone(),
                    bindings
                        .predicate("place-lava")
                        .expect("place-lava")
                        .clone(),
                )
                .expect("frozen hydrology fluids"),
            )),
        )
        .expect("semantic vegetation fixture plan compiles")
    }

    fn assert_final_vegetation_support(plan: &GenerationPlanV1, x: i64, y: i64, z: i64) {
        let support_y = y.saturating_sub(1);
        assert!(
            plan.terrain_materializes_as_solid(x, support_y, z),
            "vegetation at ({x},{y},{z}) lacks final solid support"
        );
        assert!(
            !plan.terrain_materializes_as_solid(x, y, z),
            "vegetation at ({x},{y},{z}) intersects final terrain"
        );
        assert!(
            !plan
                .hydrology_occupancy_sample(x, y, z)
                .is_some_and(|sample| sample.is_occupied()),
            "vegetation at ({x},{y},{z}) intersects hydrology occupancy"
        );
    }

    fn final_surface_y(plan: &GenerationPlanV1, x: i64, z: i64) -> i64 {
        let intended = i64::from(plan.terrain_height(x, z));
        for y in (intended.saturating_sub(8)..=intended.saturating_add(8)).rev() {
            if plan.terrain_materializes_as_solid(x, y, z)
                && !plan.terrain_materializes_as_solid(x, y.saturating_add(1), z)
            {
                return y;
            }
        }
        panic!("fixture column ({x},{z}) must contain a final terrain surface");
    }

    fn assert_final_surface_role(
        plan: &GenerationPlanV1,
        style: TerrainStyleV1,
        role: D4MaterialRoleV1,
    ) {
        let edge = i64::from(plan.config().chunk_edge_voxels);
        for z in (-4_096_i64..=4_096).step_by(32) {
            for x in (-4_096_i64..=4_096).step_by(32) {
                if plan.material_style(x, z) != style {
                    continue;
                }
                let surface_y = final_surface_y(plan, x, z);
                if plan
                    .hydrology_occupancy_sample(x, surface_y.saturating_add(1), z)
                    .is_some_and(|sample| sample.is_occupied())
                {
                    continue;
                }
                let coordinate = ChunkCoordinate::new(
                    i32::try_from(x.div_euclid(edge)).expect("fixture chunk X fits i32"),
                    i32::try_from(surface_y.div_euclid(edge)).expect("fixture chunk Y fits i32"),
                    i32::try_from(z.div_euclid(edge)).expect("fixture chunk Z fits i32"),
                );
                let outcome = plan
                    .generate(
                        plan.vacant_generation_request(coordinate)
                            .expect("fixture request is valid"),
                    )
                    .expect("fixture surface chunk generates");
                let ChunkGenerationOutcomeV1::Prepared(candidate) = outcome else {
                    panic!("fixture requires a new candidate");
                };
                let block = candidate
                    .draft()
                    .block_at(
                        u16::try_from(x.rem_euclid(edge)).expect("local X fits u16"),
                        u16::try_from(surface_y.rem_euclid(edge)).expect("local Y fits u16"),
                        u16::try_from(z.rem_euclid(edge)).expect("local Z fits u16"),
                    )
                    .expect("surface voxel lies inside the candidate");
                assert_eq!(
                    block,
                    plan.role_target(role),
                    "dry {style:?} final surface at ({x},{surface_y},{z}) must retain its biome role"
                );
                return;
            }
        }
        panic!("fixed corpus must contain a dry {style:?} final surface");
    }

    fn assert_final_tree_clearance(plan: &GenerationPlanV1, x: i64, y: i64, z: i64) {
        assert!(
            !plan.terrain_materializes_as_solid(x, y, z),
            "tree voxel ({x},{y},{z}) intersects final terrain"
        );
        assert!(
            plan.hydrology_occupancy_sample(x, y, z)
                .is_none_or(|sample| !sample.is_occupied()),
            "tree voxel ({x},{y},{z}) intersects water"
        );
    }

    fn compile_fixture(
        seed: i64,
        reverse: bool,
        natural: Vec<latticeaxiom_worldgen::ProviderOfferV1>,
    ) -> GenerationPlanV1 {
        let bindings = authored_bindings();
        let config = fixture_config();
        let mut d4 = provider_offers(None, ProviderSlotV1::ALL).expect("D4 offers");
        d4.extend(natural.iter().cloned());
        if reverse {
            d4.reverse();
        }
        let mut natural = natural;
        if reverse {
            natural.reverse();
        }
        GenerationPlanV1::compile(
            GenerationPlanInputV1::new(
                dimension_id(),
                WorldSeedV1::from_integer(seed),
                config.clone(),
                1,
                PlanActivationIdV1::from_hash(CanonicalHash::digest(b"host-worldgen-test")),
                d4,
                bindings.d4_vocabulary().expect("authored D4 vocabulary"),
                bindings.role_bindings().expect("authored role bindings"),
                bindings
                    .catalog_closure()
                    .expect("authored catalog closure"),
                CanonicalHash::digest(b"authoritative-semantic-image"),
                vec![CanonicalHash::digest(b"lock-a")],
                WorldgenLimitsV1::default(),
            )
            .with_surface_biome_terrain_programs(
                super::surface_biome_terrain_programs(CanonicalHash::digest("host-test-package"))
                    .expect("Terrenia terrain programs"),
            )
            .with_natural_layer(NaturalLayerInputV1::new(
                natural_layer_config(&config, &TerrainConfigV2::for_legacy_spine(&config))
                    .expect("natural config fits spine"),
                bindings
                    .natural_vocabulary()
                    .expect("authored natural vocabulary"),
                natural,
            )),
        )
        .expect("host worldgen fixture plan compiles")
    }

    fn generate_sorted(
        plan: &GenerationPlanV1,
        coordinates: impl IntoIterator<Item = ChunkCoordinate>,
    ) -> BTreeMap<(i32, i32, i32), (Vec<u8>, String)> {
        let mut generated = BTreeMap::new();
        for coordinate in coordinates {
            let region = generate_plan_chunks(plan, [coordinate]).expect("fixture chunk generates");
            let candidate = region.candidate(coordinate).expect("fixture candidate");
            generated.insert(
                (coordinate.x, coordinate.y, coordinate.z),
                (
                    candidate.snapshot_bytes().to_vec(),
                    candidate.checksum().to_string(),
                ),
            );
        }
        generated
    }

    fn dimension_id() -> DimensionId {
        "terrenia:dimension/terrenia"
            .parse()
            .expect("fixture dimension is valid")
    }
}
