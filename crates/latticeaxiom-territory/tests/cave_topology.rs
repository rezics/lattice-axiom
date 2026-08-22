//! V6 cave topology graph, branch contributor, and passability receipts.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "test fixtures fail immediately when authored IDs or invariants are invalid"
)]

use std::{collections::BTreeSet, num::NonZeroU32};

use latticeaxiom_core::{CanonicalHash, StableId, canonical_json_bytes};
use latticeaxiom_territory::{
    AtlasConfigV1, AtlasScaleV1, AxisV1, CavePortalV1, CaveTopologyAlgorithmV1,
    CaveTopologyDomainIdV1, CaveTopologyNodeKindV1, CaveTopologyParentV1, ChunkCoordinate,
    ContributionBudgetV1, ContributionChannelV1, ContributionCompositorV1, ContributionTargetV1,
    CoordinatorOfferV1, HydrologyPlanV1, PlanningCellBoundsV1, PlanningCellCoordinateV1,
    PortalHydrologyContractV1, PrimaryProviderOfferV1, ProviderGenerationIdentityV1,
    SpatialContributionV1, SurfaceTerritoryCandidateV1, TerritoryDomainIdV1, TerritoryLimitsV1,
    TerritoryPlanInputV1, TerritoryPlanV1, UndergroundTerritoryV1, VerticalRangeV1, WorldSeedV1,
    WorldgenConfigV1,
};
use proptest::prelude::*;

fn nz(value: u32) -> NonZeroU32 {
    NonZeroU32::new(value).expect("non-zero fixture")
}

fn stable(value: &str) -> StableId {
    value.parse().expect("stable fixture")
}

fn terrain_domain(name: &str) -> TerritoryDomainIdV1 {
    format!("latticeaxiom:territory-domain/{name}")
        .parse()
        .expect("terrain domain")
}

fn cave_domain(name: &str) -> CaveTopologyDomainIdV1 {
    format!("latticeaxiom:cave-topology-domain/{name}")
        .parse()
        .expect("cave domain")
}

fn provider(name: &str, revision: u32) -> ProviderGenerationIdentityV1 {
    ProviderGenerationIdentityV1::new(
        stable(&format!("latticeaxiom:provider/{name}")),
        nz(1),
        revision,
        CanonicalHash::digest(format!("{name}:{revision}")),
    )
}

fn production_plan() -> TerritoryPlanV1 {
    TerritoryPlanV1::compile(production_input()).expect("production plan")
}

#[allow(
    clippy::too_many_lines,
    reason = "production fixture keeps surface, underground, and branch inputs explicit"
)]
fn production_input() -> TerritoryPlanInputV1 {
    let limits = TerritoryLimitsV1::default();
    let default_terrain = terrain_domain("terrenia");
    let default_cave = cave_domain("default");
    TerritoryPlanInputV1 {
        dimension: "latticeaxiom:dimension/terrenia"
            .parse()
            .expect("dimension"),
        world_seed: WorldSeedV1::from_text("Terrenia D7 deterministic fixture"),
        atlas: AtlasConfigV1::new(
            vec![
                AtlasScaleV1::new(0, nz(64)),
                AtlasScaleV1::new(1, nz(8)),
                AtlasScaleV1::new(2, nz(1)),
            ],
            nz(2),
        )
        .expect("atlas"),
        default_terrain_domain: default_terrain.clone(),
        default_cave_domain: default_cave.clone(),
        coordinators: vec![CoordinatorOfferV1::new(provider("coordinator", 1))],
        primary_offers: vec![
            PrimaryProviderOfferV1::terrain(
                default_terrain.clone(),
                provider("terrain-default", 1),
            ),
            PrimaryProviderOfferV1::terrain(
                terrain_domain("woodland"),
                provider("terrain-woodland", 1),
            ),
            PrimaryProviderOfferV1::terrain(terrain_domain("dunes"), provider("terrain-dunes", 1)),
            PrimaryProviderOfferV1::cave(default_cave.clone(), provider("cave-default", 2)),
            PrimaryProviderOfferV1::cave(cave_domain("limestone"), provider("cave-limestone", 1)),
            PrimaryProviderOfferV1::cave(cave_domain("crystal"), provider("cave-crystal", 1)),
        ],
        surface_candidates: vec![
            SurfaceTerritoryCandidateV1::new(
                stable("latticeaxiom:territory-candidate/woodland"),
                terrain_domain("woodland"),
                default_terrain.clone(),
                0,
                PlanningCellCoordinateV1::new(0, 0),
                nz(3),
            )
            .expect("woodland"),
            SurfaceTerritoryCandidateV1::new(
                stable("latticeaxiom:territory-candidate/dunes"),
                terrain_domain("dunes"),
                default_terrain,
                0,
                PlanningCellCoordinateV1::new(8, 0),
                nz(2),
            )
            .expect("dunes"),
            SurfaceTerritoryCandidateV1::new(
                stable("latticeaxiom:territory-candidate/badlands"),
                terrain_domain("badlands"),
                terrain_domain("woodland"),
                1,
                PlanningCellCoordinateV1::new(0, 0),
                nz(1),
            )
            .expect("badlands"),
            SurfaceTerritoryCandidateV1::new(
                stable("latticeaxiom:territory-candidate/meadow"),
                terrain_domain("meadow"),
                terrain_domain("badlands"),
                2,
                PlanningCellCoordinateV1::new(0, 0),
                nz(1),
            )
            .expect("meadow"),
        ],
        underground_territories: vec![
            UndergroundTerritoryV1::new(
                cave_domain("limestone"),
                CaveTopologyParentV1::DimensionDefault,
                PlanningCellBoundsV1::new(0, 0, 4, 4).expect("limestone bounds"),
                VerticalRangeV1::new(-64, -16).expect("limestone range"),
            ),
            UndergroundTerritoryV1::new(
                cave_domain("crystal"),
                CaveTopologyParentV1::DimensionDefault,
                PlanningCellBoundsV1::new(4, 0, 8, 4).expect("crystal bounds"),
                VerticalRangeV1::new(-64, -16).expect("crystal range"),
            ),
        ],
        contributions: vec![
            SpatialContributionV1::new(
                stable("latticeaxiom:contribution/limestone-branch"),
                provider("cave-branch", 1),
                ContributionChannelV1::CaveBranch,
                ContributionCompositorV1::Union,
                ContributionTargetV1::Cave(cave_domain("limestone")),
                PlanningCellBoundsV1::new(1, 1, 3, 3).expect("branch bounds"),
                Some(VerticalRangeV1::new(-48, -24).expect("branch range")),
                1,
                4,
                ContributionBudgetV1::new(1_000, 4_096).expect("branch budget"),
                CanonicalHash::digest(b"limestone-branch-v1"),
            )
            .expect("branch"),
        ],
        cave_portals: vec![
            CavePortalV1::new(
                cave_domain("limestone"),
                PlanningCellCoordinateV1::new(0, 0),
                default_cave.clone(),
                PlanningCellCoordinateV1::new(-1, 0),
                [0, -32_000, 0],
                AxisV1::Y,
                2_000,
                3_000,
                PortalHydrologyContractV1::Dry,
            )
            .expect("limestone portal"),
            CavePortalV1::new(
                cave_domain("crystal"),
                PlanningCellCoordinateV1::new(7, 1),
                default_cave,
                PlanningCellCoordinateV1::new(8, 1),
                [128_000, -32_000, 16_000],
                AxisV1::Y,
                2_000,
                3_000,
                PortalHydrologyContractV1::Sealed,
            )
            .expect("crystal portal"),
        ],
        hydrology: HydrologyPlanV1::compile(Vec::new(), Vec::new(), limits).expect("hydrology"),
        limits,
    }
}

#[test]
fn topology_plan_has_default_two_distinct_algorithms_branch_and_destination() {
    let plan = production_plan();
    let cave = plan
        .cave_topology_plan(&WorldgenConfigV1::default())
        .expect("topology plan");
    assert_eq!(cave.default_cave_domain(), plan.default_cave_domain());
    assert_eq!(cave.underground_domains().len(), 2);
    let default_algorithm = cave
        .algorithms()
        .iter()
        .find(|entry| entry.domain() == cave.default_cave_domain())
        .expect("default algorithm")
        .algorithm();
    let limestone = cave
        .algorithms()
        .iter()
        .find(|entry| entry.domain() == &cave_domain("limestone"))
        .expect("limestone algorithm")
        .algorithm();
    let crystal = cave
        .algorithms()
        .iter()
        .find(|entry| entry.domain() == &cave_domain("crystal"))
        .expect("crystal algorithm")
        .algorithm();
    assert_eq!(default_algorithm, CaveTopologyAlgorithmV1::CoarseCell);
    assert_ne!(limestone, crystal);
    assert!(
        matches!(limestone, CaveTopologyAlgorithmV1::ConstrainedGraph)
            || matches!(limestone, CaveTopologyAlgorithmV1::FieldGrowth)
    );
    assert!(
        matches!(crystal, CaveTopologyAlgorithmV1::ConstrainedGraph)
            || matches!(crystal, CaveTopologyAlgorithmV1::FieldGrowth)
    );
    assert_eq!(cave.branch().channel(), ContributionChannelV1::CaveBranch);
    assert!(!cave.must_connect().is_empty());
    assert!(!cave.nodes().is_empty());
    assert!(!cave.edges().is_empty());
    assert!(
        cave.nodes()
            .iter()
            .any(|node| node.kind() == CaveTopologyNodeKindV1::SurfaceEntrance)
    );
    assert!(
        cave.nodes()
            .iter()
            .any(|node| node.kind() == CaveTopologyNodeKindV1::Destination)
    );
}

#[test]
fn passability_receipts_connect_surface_entrances_to_destinations() {
    let cave = production_plan()
        .cave_topology_plan(&WorldgenConfigV1::default())
        .expect("topology plan");
    assert_eq!(cave.passability().len(), cave.surface_entrances().len());
    for receipt in cave.passability() {
        assert!(receipt.graph_connected());
        assert!(receipt.hop_count() >= 1);
        assert_eq!(
            receipt.receipt_hash().to_string().len(),
            CanonicalHash::digest([]).to_string().len()
        );
    }
}

#[test]
fn topology_hash_matches_golden_and_ignores_chunk_order() {
    let plan = production_plan();
    let config = WorldgenConfigV1::default();
    let cave = plan.cave_topology_plan(&config).expect("topology plan");
    assert_eq!(
        cave.topology_hash().to_string(),
        include_str!("goldens/production-cave-topology-hash.txt").trim()
    );
    let mut chunks = vec![
        ChunkCoordinate::new(0, -2, 0),
        ChunkCoordinate::new(8, -2, 8),
        ChunkCoordinate::new(-8, -2, 0),
        ChunkCoordinate::new(56, -2, 8),
    ];
    let forward = plan
        .cave_topology_plan_from_chunks(&config, chunks.clone())
        .expect("forward");
    chunks.reverse();
    let reversed = plan
        .cave_topology_plan_from_chunks(&config, chunks)
        .expect("reversed");
    assert_eq!(
        canonical_json_bytes(&forward).expect("forward bytes"),
        canonical_json_bytes(&reversed).expect("reversed bytes")
    );
}

#[test]
fn realization_layer_survives_registration_reorder() {
    let plan = production_plan();
    let mut reordered = production_input();
    reordered.underground_territories.reverse();
    reordered.cave_portals.reverse();
    reordered.contributions.reverse();
    let second = TerritoryPlanV1::compile(reordered).expect("reordered");
    let config = WorldgenConfigV1::default();
    let first_layer = plan
        .cave_topology_plan(&config)
        .expect("first")
        .realization_layer(&config)
        .expect("first layer");
    let second_layer = second
        .cave_topology_plan(&config)
        .expect("second")
        .realization_layer(&config)
        .expect("second layer");
    assert_eq!(first_layer, second_layer);
    assert_eq!(
        first_layer.canonical_hash().expect("hash"),
        second_layer.canonical_hash().expect("hash")
    );
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(16))]

    #[test]
    fn negative_cells_keep_stable_topology_queries(
        x in -64_i64..64,
        z in -64_i64..64,
    ) {
        let plan = production_plan();
        let cell = PlanningCellCoordinateV1::new(x, z);
        let first = plan
            .underground_territory_query(cell, -32)
            .expect("first query");
        let second = plan
            .underground_territory_query(cell, -32)
            .expect("second query");
        prop_assert_eq!(&first, &second);
        let domains = BTreeSet::from([
            plan.default_cave_domain().clone(),
            cave_domain("limestone"),
            cave_domain("crystal"),
        ]);
        prop_assert!(domains.contains(first.primary()));
    }
}
