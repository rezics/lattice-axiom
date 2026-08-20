//! D4 snapshot-candidate materialization for the V2 playable spine.

use std::num::NonZeroU32;

use latticeaxiom_compose::PlayableWorldHardLimitsV1;
use latticeaxiom_core::{CanonicalHash, StableId};
use latticeaxiom_worldgen::{
    GenerationPlanInputV1, GenerationPlanV1, PlanActivationIdV1, ProviderGenerationIdentityV1,
    ProviderOfferV1, ProviderSlotV1, WorldSeedV1, WorldgenConfigV1, WorldgenLimitsV1,
};

use super::{ProductionHostError, catalog::HostWorldgenCatalog};

/// Compiles the D4 plan bound to a reopened product lock and package catalog.
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
