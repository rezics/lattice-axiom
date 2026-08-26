//! Sparse V5/V6 plan streaming for the production host.
//!
//! Chunks are generated from the compiled [`GenerationPlanV1`], not from the
//! D4 four-chunk origin neighborhood. Spawn is the validated surface cell.
//! V6 attaches package-owned cave topology and hydrology occupancy on the
//! existing V4 coordinator. This module does not open a writer or compile a
//! second Atlas.

use std::collections::BTreeSet;
use std::num::NonZeroU32;

use bevy::prelude::Vec3;
use latticeaxiom_compose::{LockedPackage, PlayableWorldHardLimitsV1};
use latticeaxiom_core::{CanonicalHash, StableId};
use latticeaxiom_player::PlayerMovementProfileV1;
use latticeaxiom_runtime_contracts::{
    AUTHORED_MAX_VIEW_DISTANCE_CHUNKS, CaveConnectivityInspectFactsV1, CaveInspectAxisV1,
    CaveOwnershipInspectFactsV1, CaveSdfInspectFactsV1, EngineEpoch, EntranceInspectFactsV1,
    FluidDecisionInspectFactsV1, FluidOccupancyInspectV1, GeologyInspectFactsV1,
    PlanningSeamInspectFactsV1, PortalInspectFactsV1, PortalInspectFluidV1, ResourceInspectFactsV1,
    RiverInspectFactsV1, SpawnInspectFactsV1, TerritoryInspectFactsV1, VegetationInspectFactsV1,
    WorldEpoch, WorldgenInspectBodyV1, WorldgenInspectCollectionV1, WorldgenInspectKindV1,
    WorldgenInspectLimits, WorldgenInspectQueryV1, WorldgenInspectRecordV1,
    WorldgenInspectReportV1, WorldgenInspectSamplesV1, compile_worldgen_inspect_report,
};
use latticeaxiom_storage::ChunkCoordinate;
use latticeaxiom_worldgen::{
    AuthoredWorldgenBindingsV1, BoundedGeneratedRegionV1, CaveFieldPortalAssertionV1, ChunkFaceV1,
    D4MaterialRoleV1, GenerationPlanInputV1, GenerationPlanV1, HydrologyOccupancyCandidateV1,
    MAX_BOUNDED_REGION_CHUNKS, NaturalLayerConfigV1, NaturalLayerInputV1, PlanActivationIdV1,
    ProviderGenerationIdentityV1, ProviderOfferV1, ProviderSlotV1, SpawnLocationV1,
    SpawnOccupancyViewV1, SpawnSearchBoundsV1, TerrainStyleV1, WorldSeedV1, WorldgenConfigV1,
    WorldgenError, WorldgenLimitsV1, required_spawn_chunks, select_safe_spawn_prefer_style,
};

use super::{
    ProductionHostError,
    catalog::{HostWorldgenCatalog, package_registration_namespace},
};

/// Compiles the V5 plan bound to a reopened product lock and package catalog.
pub(super) fn compile_plan(
    locked_receipt: CanonicalHash,
    semantic_receipt: CanonicalHash,
    catalog: &HostWorldgenCatalog,
) -> Result<GenerationPlanV1, ProductionHostError> {
    let config = spine_config();
    let natural_offers =
        provider_offers(catalog.worldgen_package.as_ref(), ProviderSlotV1::NATURAL)?;
    let mut offers = provider_offers(catalog.worldgen_package.as_ref(), ProviderSlotV1::ALL)?;
    offers.extend(natural_offers.iter().cloned());
    let input = GenerationPlanInputV1::new(
        catalog.dimension.clone(),
        WorldSeedV1::from_integer(0),
        config.clone(),
        1,
        PlanActivationIdV1::from_hash(locked_receipt),
        offers,
        catalog.role_vocabulary.clone(),
        catalog.role_bindings.clone(),
        catalog.block_catalog.clone(),
        semantic_receipt,
        vec![locked_receipt],
        WorldgenLimitsV1::default(),
    )
    .with_natural_layer(NaturalLayerInputV1::new(
        natural_layer_config(&config)?,
        catalog.natural_vocabulary.clone(),
        natural_offers,
    ))
    .with_cave_topology_layer(catalog.cave_topology_layer(&config)?)
    .with_hydrology_occupancy(catalog.hydrology_occupancy()?);
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
    WorldgenConfigV1 {
        chunk_edge_voxels: 32,
        // Preserve the authored 64-meter territory and cave planning scale
        // when moving the production chunk edge from 8 to 32 voxels.
        planning_cell_edge_chunks: 2,
        transition_width_voxels: 8,
        world_floor_y: -64,
        world_ceiling_y: 319,
        temperate_base_height: 16,
        // Keep a broad, walkable plain while adding enough relief to avoid a flat test slab.
        temperate_relief: 4,
        arid_base_height: 16,
        arid_relief: 6,
        ..WorldgenConfigV1::default()
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
        AUTHORED_MAX_VIEW_DISTANCE_CHUNKS,
        AUTHORED_MAX_VIEW_DISTANCE_CHUNKS,
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
    let bounds = host_spawn_bounds(plan);
    let occupancy = ready_spawn_occupancy(plan, bounds)?;
    select_safe_spawn_prefer_style(
        plan,
        bindings,
        &occupancy,
        bounds,
        TerrainStyleV1::TemperateWoodland,
    )
    .map_err(|error| match error {
        WorldgenError::NoSafeSpawn => ProductionHostError::NoSafeSpawn,
        other => ProductionHostError::from(other),
    })
}

/// Returns the stable origin-neighborhood spawn window.
///
/// Keeping the initial search around the origin makes the first frame
/// predictable and keeps the player's first working set local. The selected
/// style is still decided by the compiled plan and authored bindings.
fn host_spawn_bounds(_plan: &GenerationPlanV1) -> SpawnSearchBoundsV1 {
    SpawnSearchBoundsV1::origin_neighborhood()
}
/// Returns the player capsule center standing on `location` footing.
///
/// # Errors
///
/// Returns [`ProductionHostError::InvalidPlayerPose`] when a spawn voxel is
/// outside the `i32` world-cell domain.
#[allow(clippy::cast_precision_loss)] // Bounded origin-neighborhood voxels stay in f32.
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
        let region = generate_plan_chunks(plan, batch.iter().copied())?;
        for (coordinate, candidate) in region.candidates() {
            occupancy.insert_ready_draft(coordinate, candidate.draft().clone());
        }
    }
    Ok(occupancy)
}

#[allow(clippy::field_reassign_with_default)]
fn natural_layer_config(
    spine: &WorldgenConfigV1,
) -> Result<NaturalLayerConfigV1, ProductionHostError> {
    let mut config = NaturalLayerConfigV1::default();
    config.boreal_base_height = spine.temperate_base_height;
    config.boreal_relief = spine.temperate_relief.max(1);
    config.river_incision_voxels = 0;
    config.pine_threshold_per_1024 = 0;
    config.moss_threshold_per_1024 = 0;
    config.validate(spine)?;
    Ok(config)
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
        ProviderSlotV1::TemperateTerrain => "temperate",
        ProviderSlotV1::AridTerrain => "arid",
        ProviderSlotV1::TerrainTransition => "transition",
        ProviderSlotV1::CaveTopology => "cave",
        ProviderSlotV1::Materializer => "materializer",
        ProviderSlotV1::Geology => "geology",
        ProviderSlotV1::Hydrology => "hydrology",
        ProviderSlotV1::Resources => "resources",
        ProviderSlotV1::Vegetation => "vegetation",
        ProviderSlotV1::BorealTerrain => "boreal",
    }
}

const fn provider_revision(slot: ProviderSlotV1) -> u32 {
    match slot {
        ProviderSlotV1::CaveTopology => 8,
        ProviderSlotV1::Geology
        | ProviderSlotV1::Hydrology
        | ProviderSlotV1::Resources
        | ProviderSlotV1::Vegetation
        | ProviderSlotV1::BorealTerrain => 1,
        ProviderSlotV1::GenerationCoordinator
        | ProviderSlotV1::StyleSelector
        | ProviderSlotV1::TemperateTerrain
        | ProviderSlotV1::AridTerrain
        | ProviderSlotV1::TerrainTransition
        | ProviderSlotV1::Materializer => 7,
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
                natural_layer_config(plan.config())?.tree_exclusion_radius_voxels,
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
    let dest_cell = entrance.destination_cell();
    let dest = latticeaxiom_worldgen::cell_center_voxels(
        dest_cell[0],
        dest_cell[1],
        entrance.y_voxel().saturating_mul(1_000),
        plan.config(),
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
        let sample = latticeaxiom_worldgen::cell_center_voxels(
            path_cell[0],
            path_cell[1],
            entrance.y_voxel().saturating_mul(1_000),
            plan.config(),
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
    let slot = match style {
        TerrainStyleV1::TemperateWoodland => ProviderSlotV1::TemperateTerrain,
        TerrainStyleV1::AridBadlands => ProviderSlotV1::AridTerrain,
        TerrainStyleV1::BorealWetland => ProviderSlotV1::BorealTerrain,
    };
    Ok(required_provider(plan, slot)?.provider_stable_id().clone())
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
    let edge = i64::from(plan.config().chunk_edge_voxels);
    let chunk_edge = i32::from(plan.config().chunk_edge_voxels);
    let min_chunk_y = plan.config().world_floor_y.div_euclid(chunk_edge);
    let max_chunk_y = plan.config().world_ceiling_y.div_euclid(chunk_edge);
    let mut best: Option<RequiredCaveEntranceV1> = None;
    for chunk_z in origin.z.saturating_sub(6)..=origin.z.saturating_add(6) {
        for chunk_y in min_chunk_y..=max_chunk_y {
            for chunk_x in origin.x.saturating_sub(6)..=origin.x.saturating_add(6) {
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
        compile_host_worldgen_inspect, generate_plan_chunks, natural_layer_config,
        occupancy_candidate_is_current, provider_offers, required_cave_entrance, spawn_center,
        spine_config, validated_spawn,
    };
    use latticeaxiom_core::CanonicalHash;
    use latticeaxiom_runtime_contracts::{
        EngineEpoch, WorldEpoch, WorldgenInspectBodyV1, WorldgenInspectKindV1,
    };
    use latticeaxiom_storage::ChunkCoordinate;
    use latticeaxiom_worldgen::{
        AuthoredWorldgenBindingsV1, CellEpochStateV1, ChunkFaceV1, ChunkGenerationOutcomeV1,
        ChunkGenerationRequestV1, D4MaterialRoleV1, D7_NATURAL_BLOCK_COUNT, DimensionId,
        ExistingSnapshotEvidenceV1, GenerationPlanInputV1, GenerationPlanV1,
        HydrologyFluidBindingsV1, HydrologyOccupancyConfigV1, HydrologyOccupancyInputV1,
        NaturalLayerInputV1, ORIGIN_NEIGHBORHOOD_CHUNK_COORDINATES_V1, PlanActivationIdV1,
        PlanningCellCoordinateV1, ProviderSlotV1, TerrainStyleV1, WorldSeedV1, WorldgenConfigV1,
        WorldgenLimitsV1,
    };

    const AUTHORED_BINDINGS_JSON: &str =
        include_str!("../../../../packages/terrenia/worldgen/data/authored-block-bindings-v1.json");
    const D7_BLOCK_IDS: &str =
        include_str!("../../../../packages/terrenia/blocks/data/goldens/d7-block-ids.txt");

    #[test]
    fn production_world_scale_matches_the_accepted_vertical_and_planning_contract() {
        let config = spine_config();

        assert_eq!(config.chunk_edge_voxels, 32);
        assert_eq!(config.world_floor_y, -64);
        assert_eq!(config.world_ceiling_y, 319);
        assert_eq!(config.world_ceiling_y - config.world_floor_y + 1, 384);
        assert_eq!(
            i32::from(config.chunk_edge_voxels) * i32::from(config.planning_cell_edge_chunks),
            64,
            "planning cells preserve the authored 64-meter physical scale"
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
        for cell_z in -2_i32..=2 {
            for cell_x in -2_i32..=2 {
                let world_x = i64::from(cell_x)
                    * i64::from(plan.config().chunk_edge_voxels)
                    * i64::from(plan.config().planning_cell_edge_chunks);
                let world_z = i64::from(cell_z)
                    * i64::from(plan.config().chunk_edge_voxels)
                    * i64::from(plan.config().planning_cell_edge_chunks);
                styles.insert(plan.territory_query(world_x, world_z).winner());
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
        }
        assert!(styles.contains(&TerrainStyleV1::TemperateWoodland));
        assert!(styles.contains(&TerrainStyleV1::AridBadlands));
        assert!(styles.contains(&TerrainStyleV1::BorealWetland));
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
                <= u64::from(HydrologyOccupancyConfigV1::default().max_cells_per_chunk)
        );
        assert!(
            accounting.queue_depth()
                <= u64::from(HydrologyOccupancyConfigV1::default().max_queue_depth)
        );
        assert!(
            accounting.in_flight_bytes()
                <= u64::from(HydrologyOccupancyConfigV1::default().max_in_flight_bytes)
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
        let bindings = authored_bindings();
        let config = spine_config();
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
            .with_natural_layer(NaturalLayerInputV1::new(
                natural_layer_config(&config).expect("natural config fits spine"),
                bindings
                    .natural_vocabulary()
                    .expect("authored natural vocabulary"),
                natural,
            ))
            .with_hydrology_occupancy(HydrologyOccupancyInputV1::new(
                HydrologyOccupancyConfigV1::default(),
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
            .with_natural_layer(NaturalLayerInputV1::new(
                natural_layer_config(&config).expect("natural config fits spine"),
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
