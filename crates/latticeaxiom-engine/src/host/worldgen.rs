//! D4 snapshot-candidate materialization for the V2 playable spine.

use std::num::NonZeroU32;

use latticeaxiom_compose::PlayableWorldHardLimitsV1;
use latticeaxiom_core::{CanonicalHash, StableId};
use latticeaxiom_worldgen::{
    D4BlockCatalogClosureV1, D4MaterialRoleV1, D4RoleVocabularyV1, DimensionId,
    FrozenRoleBindingsV1, GenerationPlanInputV1, GenerationPlanV1, PlanActivationIdV1,
    ProviderGenerationIdentityV1, ProviderOfferV1, ProviderSlotV1, WorldSeedV1, WorldgenConfigV1,
    WorldgenLimitsV1,
};

use super::ProductionHostError;

const CATALOG_PATHS: [&str; 18] = [
    "air",
    "grass",
    "dirt",
    "stone",
    "obsidian",
    "copper-block",
    "clay",
    "sand",
    "red-sand",
    "gravel",
    "limestone",
    "basalt",
    "sandstone",
    "red-sandstone",
    "copper-ore",
    "oak-log",
    "oak-leaves",
    "tall-grass",
];

const ROLE_TARGETS: [(D4MaterialRoleV1, &str); 16] = [
    (D4MaterialRoleV1::Empty, "air"),
    (D4MaterialRoleV1::TemperateSurface, "grass"),
    (D4MaterialRoleV1::TemperateSubsurface, "dirt"),
    (D4MaterialRoleV1::TemperateBaseRock, "stone"),
    (D4MaterialRoleV1::TemperateSecondaryRock, "limestone"),
    (D4MaterialRoleV1::TemperateClay, "clay"),
    (D4MaterialRoleV1::TemperateGravel, "gravel"),
    (D4MaterialRoleV1::WoodlandLog, "oak-log"),
    (D4MaterialRoleV1::WoodlandLeaves, "oak-leaves"),
    (D4MaterialRoleV1::WoodlandGroundCover, "tall-grass"),
    (D4MaterialRoleV1::AridSand, "sand"),
    (D4MaterialRoleV1::AridRedSand, "red-sand"),
    (D4MaterialRoleV1::AridSandstone, "sandstone"),
    (D4MaterialRoleV1::AridRedSandstone, "red-sandstone"),
    (D4MaterialRoleV1::AridBaseRock, "basalt"),
    (D4MaterialRoleV1::CopperResource, "copper-ore"),
];

/// Compiles the D4 plan bound to a reopened product lock.
pub(super) fn compile_plan(
    locked_receipt: CanonicalHash,
    semantic_receipt: CanonicalHash,
) -> Result<GenerationPlanV1, ProductionHostError> {
    let input = GenerationPlanInputV1::new(
        dimension_id()?,
        WorldSeedV1::from_integer(42),
        spine_config(),
        1,
        PlanActivationIdV1::from_hash(locked_receipt),
        provider_offers()?,
        role_vocabulary()?,
        role_bindings()?,
        block_catalog()?,
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

pub(super) fn dimension_id() -> Result<DimensionId, ProductionHostError> {
    Ok("terrenia:dimension/terrenia".parse()?)
}

fn block_catalog() -> Result<D4BlockCatalogClosureV1, ProductionHostError> {
    let blocks = CATALOG_PATHS
        .iter()
        .map(|path| stable_id(&format!("terrenia:block/{path}")))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(D4BlockCatalogClosureV1::new(blocks)?)
}

fn role_vocabulary() -> Result<D4RoleVocabularyV1, ProductionHostError> {
    let entries = ROLE_TARGETS
        .iter()
        .map(|(purpose, _)| {
            Ok((
                *purpose,
                stable_id(&format!("terrenia:block-role/d4/{}@1", purpose.as_str()))?,
            ))
        })
        .collect::<Result<Vec<_>, ProductionHostError>>()?;
    Ok(D4RoleVocabularyV1::new(entries)?)
}

fn role_bindings() -> Result<FrozenRoleBindingsV1, ProductionHostError> {
    let entries = ROLE_TARGETS
        .iter()
        .map(|(purpose, target)| {
            Ok((
                stable_id(&format!("terrenia:block-role/d4/{}@1", purpose.as_str()))?,
                stable_id(&format!("terrenia:block/{target}"))?,
            ))
        })
        .collect::<Result<Vec<_>, ProductionHostError>>()?;
    Ok(FrozenRoleBindingsV1::new(entries)?)
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
