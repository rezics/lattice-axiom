//! V6 cave topology realization, occupancy arbitration, and passability.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "test fixtures fail immediately when authored IDs or invariants are invalid"
)]

use std::num::NonZeroU32;

use latticeaxiom_core::{CanonicalHash, StableId};
use latticeaxiom_worldgen::{
    CaveBranchContributorV1, CaveLayerCorridorV1, CaveLayerEntranceV1, CaveLayerPortalV1,
    CaveOwnedDomainV1, CaveTopologyAlgorithmV1, CaveTopologyLayerInputV1, ChunkCoordinate,
    ChunkGenerationOutcomeV1, D4BlockCatalogClosureV1, D4MaterialRoleV1, D4RoleVocabularyV1,
    DimensionId, FrozenRoleBindingsV1, GenerationPlanInputV1, GenerationPlanV1, PlanActivationIdV1,
    ProviderGenerationIdentityV1, ProviderOfferV1, ProviderSlotV1, WorldSeedV1, WorldgenConfigV1,
    WorldgenLimitsV1, cell_center_voxels,
};
use proptest::prelude::*;

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

fn topology_plan(reverse: bool) -> GenerationPlanV1 {
    GenerationPlanV1::compile(
        GenerationPlanInputV1::new(
            dimension_id(),
            WorldSeedV1::from_integer(42),
            config(),
            7,
            PlanActivationIdV1::from_hash(CanonicalHash::digest(b"cave-topology-activation")),
            provider_offers(reverse),
            role_vocabulary(),
            role_bindings(),
            block_catalog(),
            CanonicalHash::digest(b"authoritative-semantic-image"),
            vec![CanonicalHash::digest(b"lock-a")],
            WorldgenLimitsV1::default(),
        )
        .with_cave_topology_layer(topology_layer()),
    )
    .expect("topology plan compiles")
}

fn topology_layer() -> CaveTopologyLayerInputV1 {
    let config = config();
    let limestone_path = [
        cell_center_voxels(-1, 0, -32_000, &config),
        cell_center_voxels(0, 0, -32_000, &config),
        cell_center_voxels(1, 0, -32_000, &config),
        cell_center_voxels(2, 0, -32_000, &config),
    ];
    let crystal_path = [
        cell_center_voxels(8, 1, -32_000, &config),
        cell_center_voxels(7, 1, -32_000, &config),
        cell_center_voxels(6, 1, -32_000, &config),
        cell_center_voxels(5, 1, -32_000, &config),
    ];
    CaveTopologyLayerInputV1::new(
        stable_id("latticeaxiom:cave-topology-domain/default"),
        CaveTopologyAlgorithmV1::CoarseCell,
        vec![
            CaveOwnedDomainV1::new(
                stable_id("latticeaxiom:cave-topology-domain/crystal"),
                CaveTopologyAlgorithmV1::ConstrainedGraph,
                [4, 0],
                [8, 4],
                -64,
                -16,
            )
            .expect("crystal domain"),
            CaveOwnedDomainV1::new(
                stable_id("latticeaxiom:cave-topology-domain/limestone"),
                CaveTopologyAlgorithmV1::FieldGrowth,
                [0, 0],
                [4, 4],
                -64,
                -16,
            )
            .expect("limestone domain"),
        ],
        vec![
            CaveLayerCorridorV1::new(limestone_path[0], limestone_path[1]),
            CaveLayerCorridorV1::new(limestone_path[1], limestone_path[2]),
            CaveLayerCorridorV1::new(limestone_path[2], limestone_path[3]),
            CaveLayerCorridorV1::new(crystal_path[0], crystal_path[1]),
            CaveLayerCorridorV1::new(crystal_path[1], crystal_path[2]),
            CaveLayerCorridorV1::new(crystal_path[2], crystal_path[3]),
        ],
        vec![
            CaveLayerPortalV1::new(limestone_path[1], 2, 3).expect("limestone portal"),
            CaveLayerPortalV1::new(crystal_path[1], 2, 3).expect("crystal portal"),
        ],
        vec![
            CaveLayerEntranceV1::new(vec![[-1, 0], [0, 0], [1, 0], [2, 0]], -32, [2, 0])
                .expect("limestone entrance"),
            CaveLayerEntranceV1::new(vec![[8, 1], [7, 1], [6, 1], [5, 1]], -32, [5, 1])
                .expect("crystal entrance"),
        ],
        CaveBranchContributorV1::new(
            stable_id("latticeaxiom:cave-topology-domain/limestone"),
            [1, 1],
            [3, 3],
            -48,
            -24,
        )
        .expect("branch"),
    )
    .expect("topology layer")
}

#[test]
fn topology_layer_keeps_d4_output_unchanged_when_omitted() {
    let d4 = GenerationPlanV1::compile(GenerationPlanInputV1::new(
        dimension_id(),
        WorldSeedV1::from_integer(42),
        config(),
        7,
        PlanActivationIdV1::from_hash(CanonicalHash::digest(b"cave-topology-activation")),
        provider_offers(false),
        role_vocabulary(),
        role_bindings(),
        block_catalog(),
        CanonicalHash::digest(b"authoritative-semantic-image"),
        vec![CanonicalHash::digest(b"lock-a")],
        WorldgenLimitsV1::default(),
    ))
    .expect("d4 plan");
    assert!(!d4.has_cave_topology_layer());
    let topology = topology_plan(false);
    assert!(topology.has_cave_topology_layer());
    assert_ne!(d4.generation_input_hash(), topology.generation_input_hash());
}

#[test]
fn two_underground_algorithms_are_distinct_and_preserve_outside_influence() {
    let plan = topology_plan(false);
    let config = config();
    let limestone = cell_center_voxels(1, 0, -32_000, &config);
    let crystal = cell_center_voxels(6, 1, -32_000, &config);
    let outside = cell_center_voxels(20, 20, -32_000, &config);
    assert_eq!(
        plan.cave_topology_algorithm(limestone[0], limestone[1], limestone[2]),
        Some(CaveTopologyAlgorithmV1::FieldGrowth)
    );
    assert_eq!(
        plan.cave_topology_algorithm(crystal[0], crystal[1], crystal[2]),
        Some(CaveTopologyAlgorithmV1::ConstrainedGraph)
    );
    assert!(plan.cave_in_declared_influence(limestone[0], limestone[1], limestone[2]));
    assert!(
        plan.cave_occupancy_arbitration(limestone[0], limestone[1], limestone[2])
            .is_raw_void()
    );
    assert!(
        plan.cave_occupancy_arbitration(crystal[0], crystal[1], crystal[2])
            .is_raw_void()
    );
    assert!(!plan.cave_in_declared_influence(outside[0], outside[1], outside[2]));
    let outside_occupancy = plan.cave_occupancy_arbitration(outside[0], outside[1], outside[2]);
    assert!(!outside_occupancy.is_raw_void());
    assert!(outside_occupancy.allows_solid_placement());

    let graph_side = plan.cave_occupancy_arbitration(crystal[0], crystal[1], crystal[2] + 2);
    let growth_side = plan.cave_occupancy_arbitration(limestone[0], limestone[1], limestone[2] + 2);
    assert!(graph_side.is_raw_void());
    assert!(!growth_side.is_raw_void());
}

#[test]
fn passability_and_shared_faces_are_stable_under_chunk_reorder() {
    let forward = topology_plan(false);
    let reversed = topology_plan(true);
    assert_eq!(
        forward.generation_input_hash(),
        reversed.generation_input_hash()
    );
    let receipts = forward.cave_passability_receipts();
    assert!(!receipts.is_empty());
    assert!(
        receipts
            .iter()
            .all(latticeaxiom_worldgen::CaveVoxelPassabilityReceiptV1::is_passable)
    );
    let mut chunks = vec![
        ChunkCoordinate::new(-1, -3, 0),
        ChunkCoordinate::new(0, -3, 0),
        ChunkCoordinate::new(5, -3, 1),
        ChunkCoordinate::new(6, -3, 1),
    ];
    let first = forward
        .cave_field_portal_plan(chunks.clone())
        .expect("forward portal plan");
    chunks.reverse();
    let second = reversed
        .cave_field_portal_plan(chunks)
        .expect("reversed portal plan");
    assert_eq!(first, second);
}

#[test]
fn final_occupancy_matches_draft_inside_corridors() {
    let plan = topology_plan(false);
    let config = config();
    let center = cell_center_voxels(0, 0, -32_000, &config);
    let edge = i64::from(config.chunk_edge_voxels);
    let chunk = ChunkCoordinate::new(
        i32::try_from(center[0].div_euclid(edge)).expect("chunk x"),
        i32::try_from(center[1].div_euclid(edge)).expect("chunk y"),
        i32::try_from(center[2].div_euclid(edge)).expect("chunk z"),
    );
    let outcome = plan
        .generate(plan.vacant_generation_request(chunk).expect("vacant"))
        .expect("generate");
    let ChunkGenerationOutcomeV1::Prepared(candidate) = outcome else {
        panic!("expected prepared candidate");
    };
    let empty = plan.role_target(D4MaterialRoleV1::Empty);
    let local_x = u16::try_from(center[0].rem_euclid(edge)).expect("local x");
    let local_y = u16::try_from(center[1].rem_euclid(edge)).expect("local y");
    let local_z = u16::try_from(center[2].rem_euclid(edge)).expect("local z");
    assert_eq!(
        candidate.draft().block_at(local_x, local_y, local_z),
        Some(empty)
    );
    for validation in candidate.receipt().cave_occupancy_validations() {
        if validation.request().portal_requested() {
            assert!(validation.portal_clearance_intact());
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(12))]

    #[test]
    fn negative_coordinates_stay_solid_outside_influence(
        x in -400_i64..-200,
        z in -400_i64..-200,
    ) {
        let plan = topology_plan(false);
        prop_assert!(!plan.cave_in_declared_influence(x, -32, z));
        let occupancy = plan.cave_occupancy_arbitration(x, -32, z);
        prop_assert!(!occupancy.is_raw_void());
        prop_assert!(occupancy.allows_solid_placement());
    }
}

fn config() -> WorldgenConfigV1 {
    WorldgenConfigV1::default()
}

fn dimension_id() -> DimensionId {
    "fixture:dimension/cave-topology"
        .parse()
        .expect("dimension")
}

fn stable_id(value: &str) -> StableId {
    value.parse().expect("stable id")
}

fn provider_offers(reverse: bool) -> Vec<ProviderOfferV1> {
    let paths = [
        (ProviderSlotV1::GenerationCoordinator, "coordinator"),
        (ProviderSlotV1::StyleSelector, "selector"),
        (ProviderSlotV1::TemperateTerrain, "temperate"),
        (ProviderSlotV1::AridTerrain, "arid"),
        (ProviderSlotV1::TerrainTransition, "transition"),
        (ProviderSlotV1::CaveTopology, "cave"),
        (ProviderSlotV1::Materializer, "materializer"),
    ];
    let mut offers = paths
        .into_iter()
        .map(|(slot, path)| {
            let revision = if slot == ProviderSlotV1::CaveTopology {
                8
            } else {
                7
            };
            ProviderOfferV1::new(
                slot,
                ProviderGenerationIdentityV1::new(
                    stable_id(&format!("fixture:worldgen-provider/{path}@1")),
                    NonZeroU32::MIN,
                    revision,
                    CanonicalHash::digest(format!("{path}-implementation-v{revision}")),
                ),
            )
        })
        .collect::<Vec<_>>();
    if reverse {
        offers.reverse();
    }
    offers
}

fn role_vocabulary() -> D4RoleVocabularyV1 {
    D4RoleVocabularyV1::new(
        ROLE_TARGETS
            .iter()
            .map(|(purpose, _)| {
                (
                    *purpose,
                    stable_id(&format!("terrenia:block-role/d4/{}@1", purpose.as_str())),
                )
            })
            .collect::<Vec<_>>(),
    )
    .expect("vocabulary")
}

fn role_bindings() -> FrozenRoleBindingsV1 {
    FrozenRoleBindingsV1::new(
        ROLE_TARGETS
            .iter()
            .map(|(purpose, path)| {
                (
                    stable_id(&format!("terrenia:block-role/d4/{}@1", purpose.as_str())),
                    stable_id(&format!("terrenia:block/{path}@1")),
                )
            })
            .collect::<Vec<_>>(),
    )
    .expect("bindings")
}

fn block_catalog() -> D4BlockCatalogClosureV1 {
    D4BlockCatalogClosureV1::new(
        CATALOG_PATHS
            .iter()
            .map(|path| stable_id(&format!("terrenia:block/{path}@1")))
            .collect::<Vec<_>>(),
    )
    .expect("catalog")
}
