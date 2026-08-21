//! Sparse V5 plan streaming for the production host.
//!
//! Chunks are generated from the compiled [`GenerationPlanV1`], not from the
//! D4 four-chunk origin neighborhood. Spawn is the validated surface cell.
//! This module does not open a writer, compile a second Atlas, or extend V6
//! cave topology.

use std::num::NonZeroU32;

use bevy::prelude::Vec3;
use latticeaxiom_compose::PlayableWorldHardLimitsV1;
use latticeaxiom_core::{CanonicalHash, StableId};
use latticeaxiom_player::PlayerMovementProfileV1;
use latticeaxiom_storage::ChunkCoordinate;
use latticeaxiom_worldgen::{
    AuthoredWorldgenBindingsV1, BoundedGeneratedRegionV1, CaveFieldPortalAssertionV1, ChunkFaceV1,
    GenerationPlanInputV1, GenerationPlanV1, MAX_BOUNDED_REGION_CHUNKS, PlanActivationIdV1,
    ProviderGenerationIdentityV1, ProviderOfferV1, ProviderSlotV1, SpawnLocationV1,
    SpawnOccupancyViewV1, SpawnSearchBoundsV1, WorldSeedV1, WorldgenConfigV1, WorldgenError,
    WorldgenLimitsV1, required_spawn_chunks, select_safe_spawn,
};

use super::{ProductionHostError, catalog::HostWorldgenCatalog};

/// Compiles the V5 plan bound to a reopened product lock and package catalog.
pub(super) fn compile_plan(
    locked_receipt: CanonicalHash,
    semantic_receipt: CanonicalHash,
    catalog: &HostWorldgenCatalog,
) -> Result<GenerationPlanV1, ProductionHostError> {
    let input = GenerationPlanInputV1::new(
        catalog.dimension.clone(),
        WorldSeedV1::from_integer(42),
        spine_config(),
        1,
        PlanActivationIdV1::from_hash(locked_receipt),
        provider_offers()?,
        catalog.role_vocabulary.clone(),
        catalog.role_bindings.clone(),
        catalog.block_catalog.clone(),
        semantic_receipt,
        vec![locked_receipt],
        WorldgenLimitsV1::default(),
    );
    Ok(GenerationPlanV1::compile(input)?)
}

/// Returns the closed D4 spine configuration.
#[must_use]
pub(super) fn spine_config() -> WorldgenConfigV1 {
    WorldgenConfigV1 {
        chunk_edge_voxels: 8,
        planning_cell_edge_chunks: 8,
        transition_width_voxels: 8,
        world_floor_y: 0,
        world_ceiling_y: 31,
        temperate_base_height: 16,
        temperate_relief: 2,
        arid_base_height: 16,
        arid_relief: 2,
        ..WorldgenConfigV1::default()
    }
}

/// Returns host streaming clamps. Durable save radius is unused.
///
/// # Errors
///
/// Returns [`ProductionHostError::InvalidHostLimits`] when a clamp is zero.
pub(super) fn host_hard_limits() -> Result<PlayableWorldHardLimitsV1, ProductionHostError> {
    PlayableWorldHardLimitsV1::new(4, 4, 128, 8, 4)
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
    let bounds = SpawnSearchBoundsV1::origin_neighborhood();
    let occupancy = ready_spawn_occupancy(plan, bounds)?;
    select_safe_spawn(plan, bindings, &occupancy, bounds).map_err(|error| match error {
        WorldgenError::NoSafeSpawn => ProductionHostError::NoSafeSpawn,
        other => ProductionHostError::from(other),
    })
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

fn provider_offers() -> Result<Vec<ProviderOfferV1>, ProductionHostError> {
    let paths = [
        (ProviderSlotV1::GenerationCoordinator, "coordinator"),
        (ProviderSlotV1::StyleSelector, "selector"),
        (ProviderSlotV1::TemperateTerrain, "temperate"),
        (ProviderSlotV1::AridTerrain, "arid"),
        (ProviderSlotV1::TerrainTransition, "transition"),
        (ProviderSlotV1::CaveTopology, "cave"),
        (ProviderSlotV1::Materializer, "materializer"),
    ];
    paths
        .into_iter()
        .map(|(slot, path)| {
            let revision = if slot == ProviderSlotV1::CaveTopology {
                8
            } else {
                7
            };
            Ok(ProviderOfferV1::new(
                slot,
                ProviderGenerationIdentityV1::new(
                    stable_id(&format!("latticeaxiom:worldgen-provider/{path}@1"))?,
                    NonZeroU32::MIN,
                    revision,
                    CanonicalHash::digest(format!("{path}-implementation-v{revision}")),
                ),
            ))
        })
        .collect()
}

fn stable_id(value: &str) -> Result<StableId, ProductionHostError> {
    Ok(value.parse()?)
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
                    if entrance_is_better(best.as_ref(), &candidate, origin, edge) {
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
    current: Option<&RequiredCaveEntranceV1>,
    candidate: &RequiredCaveEntranceV1,
    origin: ChunkCoordinate,
    edge: i64,
) -> bool {
    let Some(current) = current else {
        return true;
    };
    let candidate_key = entrance_sort_key(candidate, origin, edge);
    let current_key = entrance_sort_key(current, origin, edge);
    candidate_key > current_key
}

fn entrance_sort_key(
    entrance: &RequiredCaveEntranceV1,
    origin: ChunkCoordinate,
    edge: i64,
) -> (i32, std::cmp::Reverse<u32>, i32, i32, i32, ChunkFaceV1) {
    let dx = i64::from(entrance.aperture[0]).div_euclid(edge) - i64::from(origin.x);
    let dz = i64::from(entrance.aperture[2]).div_euclid(edge) - i64::from(origin.z);
    let distance = u32::try_from(dx.abs().max(dz.abs())).unwrap_or(u32::MAX);
    (
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
    use super::{
        generate_plan_chunks, provider_offers, required_cave_entrance, spawn_center, spine_config,
        validated_spawn,
    };
    use latticeaxiom_core::CanonicalHash;
    use latticeaxiom_storage::ChunkCoordinate;
    use latticeaxiom_worldgen::{
        AuthoredWorldgenBindingsV1, DimensionId, GenerationPlanInputV1, GenerationPlanV1,
        ORIGIN_NEIGHBORHOOD_CHUNK_COORDINATES_V1, PlanActivationIdV1, WorldSeedV1,
        WorldgenLimitsV1,
    };

    const AUTHORED_BINDINGS_JSON: &str =
        include_str!("../../../../packages/terrenia/worldgen/data/authored-block-bindings-v1.json");

    #[test]
    fn plan_chunks_are_not_limited_to_the_d4_origin_neighborhood() {
        let plan = fixture_plan(42);
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

    fn authored_bindings() -> AuthoredWorldgenBindingsV1 {
        AuthoredWorldgenBindingsV1::from_json(AUTHORED_BINDINGS_JSON.as_bytes())
            .expect("@terrenia/worldgen authored bindings must decode")
    }

    fn fixture_plan(seed: i64) -> GenerationPlanV1 {
        let bindings = authored_bindings();
        GenerationPlanV1::compile(GenerationPlanInputV1::new(
            dimension_id(),
            WorldSeedV1::from_integer(seed),
            spine_config(),
            1,
            PlanActivationIdV1::from_hash(CanonicalHash::digest(b"host-worldgen-test")),
            provider_offers().expect("fixture providers are complete"),
            bindings.d4_vocabulary().expect("authored D4 vocabulary"),
            bindings.role_bindings().expect("authored role bindings"),
            bindings
                .catalog_closure()
                .expect("authored catalog closure"),
            CanonicalHash::digest(b"authoritative-semantic-image"),
            vec![CanonicalHash::digest(b"lock-a")],
            WorldgenLimitsV1::default(),
        ))
        .expect("host worldgen fixture plan compiles")
    }

    fn dimension_id() -> DimensionId {
        "terrenia:dimension/terrenia"
            .parse()
            .expect("fixture dimension is valid")
    }
}
