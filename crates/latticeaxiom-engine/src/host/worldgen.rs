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
    AuthoredWorldgenBindingsV1, BoundedGeneratedRegionV1, GenerationPlanInputV1, GenerationPlanV1,
    MAX_BOUNDED_REGION_CHUNKS, PlanActivationIdV1, ProviderGenerationIdentityV1, ProviderOfferV1,
    ProviderSlotV1, SpawnLocationV1, SpawnOccupancyViewV1, SpawnSearchBoundsV1, WorldSeedV1,
    WorldgenConfigV1, WorldgenError, WorldgenLimitsV1, required_spawn_chunks, select_safe_spawn,
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
    PlayableWorldHardLimitsV1::new(2, 2, 64, 4, 2)
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

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used)]
mod tests {
    use super::{
        generate_plan_chunks, provider_offers, spawn_center, spine_config, validated_spawn,
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
