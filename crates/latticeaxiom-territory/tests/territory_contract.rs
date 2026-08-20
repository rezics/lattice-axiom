//! D7 territory, cave, and epoch conformance tests.

use std::num::NonZeroU32;

use latticeaxiom_core::{CanonicalHash, StableId, canonical_json_bytes};
use latticeaxiom_territory::{
    AtlasConfigV1, AtlasScaleV1, AxisV1, CardinalDirectionV1, CavePortalV1, CaveTopologyDomainIdV1,
    CaveTopologyParentV1, ContributionBudgetV1, ContributionChannelV1, ContributionCompositorV1,
    ContributionTargetV1, CoordinatorOfferV1, GenerationEpochIdV1, HydrologyBasinV1,
    HydrologyConnectionV1, HydrologyPlanV1, PlanningCellBoundsV1, PlanningCellCoordinateV1,
    PlanningCellEpochLedgerV1, PlanningCellTransitionAdapterV1, PlanningCellTransitionReceiptV1,
    PortalHydrologyContractV1, PrimaryProviderOfferV1, ProviderGenerationIdentityV1,
    SpatialContributionV1, SurfaceTerritoryCandidateV1, TerritoryDomainIdV1, TerritoryLimitsV1,
    TerritoryPlanInputV1, TerritoryPlanV1, UndergroundTerritoryV1, VerticalRangeV1, WorldSeedV1,
};
use proptest::prelude::*;

fn nz(value: u32) -> NonZeroU32 {
    NonZeroU32::new(value)
        .unwrap_or_else(|| panic!("test fixture expected a non-zero value, got {value}"))
}

fn stable(value: &str) -> StableId {
    value
        .parse()
        .unwrap_or_else(|error| panic!("invalid stable-ID fixture {value}: {error}"))
}

fn terrain_domain(name: &str) -> TerritoryDomainIdV1 {
    format!("latticeaxiom:territory-domain/{name}")
        .parse()
        .unwrap_or_else(|error| panic!("invalid terrain-domain fixture {name}: {error}"))
}

fn cave_domain(name: &str) -> CaveTopologyDomainIdV1 {
    format!("latticeaxiom:cave-topology-domain/{name}")
        .parse()
        .unwrap_or_else(|error| panic!("invalid cave-domain fixture {name}: {error}"))
}

fn provider(name: &str, revision: u32) -> ProviderGenerationIdentityV1 {
    ProviderGenerationIdentityV1::new(
        stable(&format!("latticeaxiom:provider/{name}")),
        nz(1),
        revision,
        CanonicalHash::digest(format!("{name}:{revision}")),
    )
}

fn atlas() -> AtlasConfigV1 {
    AtlasConfigV1::new(
        vec![
            AtlasScaleV1::new(0, nz(64)),
            AtlasScaleV1::new(1, nz(8)),
            AtlasScaleV1::new(2, nz(1)),
        ],
        nz(2),
    )
    .unwrap_or_else(|error| panic!("valid Atlas fixture was rejected: {error}"))
}

fn candidates(count: usize) -> Vec<SurfaceTerritoryCandidateV1> {
    let default = terrain_domain("terrenia");
    let woodland = terrain_domain("woodland");
    let dunes = terrain_domain("dunes");
    let badlands = terrain_domain("badlands");
    let scrub = terrain_domain("scrub");
    let meadow = terrain_domain("meadow");
    let canyon = terrain_domain("canyon");
    (0..count.max(3))
        .map(|index| {
            let level = u8::try_from(index % 3)
                .unwrap_or_else(|error| panic!("fixture level conversion failed: {error}"));
            let alternate = (index / 3) % 2 == 1;
            let (domain, parent) = match (level, alternate) {
                (0, false) => (woodland.clone(), default.clone()),
                (0, true) => (dunes.clone(), default.clone()),
                (1, false) => (badlands.clone(), woodland.clone()),
                (1, true) => (scrub.clone(), dunes.clone()),
                (2, false) => (meadow.clone(), badlands.clone()),
                (2, true) => (canyon.clone(), scrub.clone()),
                _ => panic!("fixture produced an impossible Atlas level"),
            };
            let index_i64 = i64::try_from(index)
                .unwrap_or_else(|error| panic!("fixture index conversion failed: {error}"));
            SurfaceTerritoryCandidateV1::new(
                stable(&format!("latticeaxiom:territory-candidate/c{index}")),
                domain,
                parent,
                level,
                PlanningCellCoordinateV1::new(index_i64, -index_i64),
                nz(1 + u32::try_from(index % 7).unwrap_or_default()),
            )
            .unwrap_or_else(|error| panic!("valid surface candidate was rejected: {error}"))
        })
        .collect()
}

fn portal_pair() -> Vec<CavePortalV1> {
    let default = cave_domain("default");
    let limestone = cave_domain("limestone");
    let crystal = cave_domain("crystal");
    vec![
        CavePortalV1::new(
            limestone,
            PlanningCellCoordinateV1::new(0, 0),
            default.clone(),
            PlanningCellCoordinateV1::new(-1, 0),
            [0, -32_000, 0],
            AxisV1::Y,
            2_000,
            3_000,
            PortalHydrologyContractV1::Dry,
        )
        .unwrap_or_else(|error| panic!("valid limestone portal was rejected: {error}")),
        CavePortalV1::new(
            crystal,
            PlanningCellCoordinateV1::new(7, 1),
            default,
            PlanningCellCoordinateV1::new(8, 1),
            [128_000, -32_000, 16_000],
            AxisV1::Y,
            2_000,
            3_000,
            PortalHydrologyContractV1::Sealed,
        )
        .unwrap_or_else(|error| panic!("valid crystal portal was rejected: {error}")),
    ]
}

fn input(candidate_count: usize) -> TerritoryPlanInputV1 {
    let limits = TerritoryLimitsV1::default();
    let default_terrain = terrain_domain("terrenia");
    let default_cave = cave_domain("default");
    let limestone = cave_domain("limestone");
    let crystal = cave_domain("crystal");
    let underground = vec![
        UndergroundTerritoryV1::new(
            limestone,
            CaveTopologyParentV1::DimensionDefault,
            PlanningCellBoundsV1::new(0, 0, 4, 4)
                .unwrap_or_else(|error| panic!("valid bounds were rejected: {error}")),
            VerticalRangeV1::new(-64, -16)
                .unwrap_or_else(|error| panic!("valid vertical range was rejected: {error}")),
        ),
        UndergroundTerritoryV1::new(
            crystal,
            CaveTopologyParentV1::DimensionDefault,
            PlanningCellBoundsV1::new(4, 0, 8, 4)
                .unwrap_or_else(|error| panic!("valid bounds were rejected: {error}")),
            VerticalRangeV1::new(-64, -16)
                .unwrap_or_else(|error| panic!("valid vertical range was rejected: {error}")),
        ),
    ];
    let terrain_provider = provider("terrain-default", 1);
    let contribution = SpatialContributionV1::new(
        stable("latticeaxiom:contribution/meadow-detail"),
        terrain_provider.clone(),
        ContributionChannelV1::TerrainDetail,
        ContributionCompositorV1::PriorityStack,
        ContributionTargetV1::Terrain(terrain_domain("meadow")),
        PlanningCellBoundsV1::new(-128, -128, 129, 129)
            .unwrap_or_else(|error| panic!("valid contribution bounds were rejected: {error}")),
        None,
        2,
        10,
        ContributionBudgetV1::new(10_000, 65_536)
            .unwrap_or_else(|error| panic!("valid contribution budget was rejected: {error}")),
        CanonicalHash::digest(b"meadow-detail-v1"),
    )
    .unwrap_or_else(|error| panic!("valid contribution was rejected: {error}"));
    TerritoryPlanInputV1 {
        dimension: "latticeaxiom:dimension/terrenia"
            .parse()
            .unwrap_or_else(|error| panic!("invalid dimension fixture: {error}")),
        world_seed: WorldSeedV1::from_text("Terrenia D7 deterministic fixture"),
        atlas: atlas(),
        default_terrain_domain: default_terrain.clone(),
        default_cave_domain: default_cave.clone(),
        coordinators: vec![CoordinatorOfferV1::new(provider("coordinator", 1))],
        primary_offers: vec![
            PrimaryProviderOfferV1::terrain(default_terrain, terrain_provider),
            PrimaryProviderOfferV1::cave(default_cave, provider("cave-default", 2)),
        ],
        surface_candidates: candidates(candidate_count),
        underground_territories: underground,
        contributions: vec![contribution],
        cave_portals: portal_pair(),
        hydrology: HydrologyPlanV1::compile(Vec::new(), Vec::new(), limits)
            .unwrap_or_else(|error| panic!("empty abstract hydrology plan was rejected: {error}")),
        limits,
    }
}

fn plan(candidate_count: usize) -> TerritoryPlanV1 {
    TerritoryPlanV1::compile(input(candidate_count))
        .unwrap_or_else(|error| panic!("valid territory plan was rejected: {error}"))
}

#[test]
fn thousand_candidate_atlas_is_bounded_queryable_and_statistical() {
    let plan = plan(1_000);
    let query = plan.query(PlanningCellCoordinateV1::new(17, -9));
    assert_eq!(query.levels().len(), 3);
    let stats = plan
        .statistics(
            PlanningCellBoundsV1::new(-16, -16, 16, 16)
                .unwrap_or_else(|error| panic!("valid stats bounds were rejected: {error}")),
        )
        .unwrap_or_else(|error| panic!("bounded statistics query failed: {error}"));
    assert_eq!(
        stats
            .areas()
            .iter()
            .map(latticeaxiom_territory::TerritoryAreaStatisticsV1::cell_count)
            .sum::<u64>(),
        1_024
    );
    assert!(
        stats
            .areas()
            .iter()
            .all(|area| area.connected_components() > 0)
    );
}

#[test]
fn registration_order_does_not_change_plan_bytes_hash_or_queries() {
    let first = plan(1_000);
    let mut reordered = input(1_000);
    reordered.surface_candidates.reverse();
    reordered.primary_offers.reverse();
    reordered.underground_territories.reverse();
    reordered.cave_portals.reverse();
    let second = TerritoryPlanV1::compile(reordered)
        .unwrap_or_else(|error| panic!("reordered plan was rejected: {error}"));
    assert_eq!(first.plan_hash(), second.plan_hash());
    assert_eq!(
        canonical_json_bytes(&first).ok(),
        canonical_json_bytes(&second).ok()
    );
    for cell in [
        PlanningCellCoordinateV1::new(0, 0),
        PlanningCellCoordinateV1::new(-91, 27),
        PlanningCellCoordinateV1::new(8_192, -4_096),
    ] {
        assert_eq!(first.query(cell), second.query(cell));
    }
}

#[test]
fn exactly_one_coordinator_and_primary_owner_are_enforced() {
    let mut coordinator_conflict = input(3);
    coordinator_conflict
        .coordinators
        .push(CoordinatorOfferV1::new(provider("other-coordinator", 1)));
    assert!(TerritoryPlanV1::compile(coordinator_conflict).is_err());

    let mut owner_conflict = input(3);
    owner_conflict
        .primary_offers
        .push(PrimaryProviderOfferV1::terrain(
            terrain_domain("terrenia"),
            provider("other-terrain", 1),
        ));
    assert!(TerritoryPlanV1::compile(owner_conflict).is_err());
}

#[test]
fn portal_identity_is_direction_independent() {
    let forward = CavePortalV1::new(
        cave_domain("limestone"),
        PlanningCellCoordinateV1::new(0, 0),
        cave_domain("default"),
        PlanningCellCoordinateV1::new(-1, 0),
        [0, -32_000, 0],
        AxisV1::Y,
        2_000,
        3_000,
        PortalHydrologyContractV1::Dry,
    )
    .unwrap_or_else(|error| panic!("forward portal was rejected: {error}"));
    let reverse = CavePortalV1::new(
        cave_domain("default"),
        PlanningCellCoordinateV1::new(-1, 0),
        cave_domain("limestone"),
        PlanningCellCoordinateV1::new(0, 0),
        [0, -32_000, 0],
        AxisV1::Y,
        2_000,
        3_000,
        PortalHydrologyContractV1::Dry,
    )
    .unwrap_or_else(|error| panic!("reverse portal was rejected: {error}"));
    assert_eq!(forward, reverse);
}

#[test]
fn epoch_freeze_is_idempotent_and_transition_receipts_ignore_direction() {
    let dimension = "latticeaxiom:dimension/terrenia"
        .parse()
        .unwrap_or_else(|error| panic!("invalid dimension fixture: {error}"));
    let mut ledger = PlanningCellEpochLedgerV1::empty(dimension)
        .unwrap_or_else(|error| panic!("empty ledger failed: {error}"));
    let first_cell = PlanningCellCoordinateV1::new(-1, 0);
    let second_cell = PlanningCellCoordinateV1::new(0, 0);
    let first_epoch = GenerationEpochIdV1::from_hash(CanonicalHash::digest(b"epoch-a"));
    let second_epoch = GenerationEpochIdV1::from_hash(CanonicalHash::digest(b"epoch-b"));
    assert!(
        ledger
            .freeze_for_materialization(first_cell, first_epoch)
            .unwrap_or_else(|error| panic!("first freeze failed: {error}"))
            .created()
    );
    assert!(
        !ledger
            .freeze_for_materialization(first_cell, first_epoch)
            .unwrap_or_else(|error| panic!("idempotent freeze failed: {error}"))
            .created()
    );
    ledger
        .freeze_for_materialization(second_cell, second_epoch)
        .unwrap_or_else(|error| panic!("second freeze failed: {error}"));
    assert!(
        ledger
            .freeze_for_materialization(first_cell, second_epoch)
            .is_err()
    );

    let plan = plan(3);
    let adjacency = plan
        .cave_adjacency(first_cell, second_cell)
        .unwrap_or_else(|| panic!("fixture cave adjacency was not compiled"));
    let adapter = PlanningCellTransitionAdapterV1::new(
        stable("latticeaxiom:transition-adapter/terrain-cave"),
        nz(1),
        CanonicalHash::digest(b"adapter-v1"),
        first_epoch,
        second_epoch,
        nz(2),
        CanonicalHash::digest(b"terrain-boundary-v1"),
        adjacency.portal_ids().to_vec(),
        50_000,
    )
    .unwrap_or_else(|error| panic!("valid transition adapter was rejected: {error}"));
    let forward = PlanningCellTransitionReceiptV1::issue(
        &ledger,
        first_cell,
        second_cell,
        &adapter,
        adjacency,
    )
    .unwrap_or_else(|error| panic!("forward transition receipt failed: {error}"));
    let reverse = PlanningCellTransitionReceiptV1::issue(
        &ledger,
        second_cell,
        first_cell,
        &adapter,
        adjacency,
    )
    .unwrap_or_else(|error| panic!("reverse transition receipt failed: {error}"));
    assert_eq!(forward.receipt_hash(), reverse.receipt_hash());
    assert_eq!(forward.cells(), reverse.cells());
}

#[test]
fn plan_hash_matches_golden_and_serialized_plan_revalidates() {
    let plan = plan(3);
    assert_eq!(
        plan.plan_hash().to_string(),
        include_str!("goldens/minimal-plan-hash.txt").trim()
    );
    let encoded = serde_json::to_vec_pretty(&plan)
        .unwrap_or_else(|error| panic!("plan serialization failed: {error}"));
    let decoded: TerritoryPlanV1 = serde_json::from_slice(&encoded)
        .unwrap_or_else(|error| panic!("plan deserialization failed: {error}"));
    decoded
        .validate_hash()
        .unwrap_or_else(|error| panic!("serialized plan hash failed validation: {error}"));
}

#[test]
fn selector_limit_and_serialized_index_are_fail_closed() {
    let mut at_limit = input(3);
    at_limit.limits.max_candidates_per_selector = 1;
    TerritoryPlanV1::compile(at_limit)
        .unwrap_or_else(|error| panic!("selector at its hard limit was rejected: {error}"));

    let mut above_limit = input(6);
    above_limit.limits.max_candidates_per_selector = 1;
    assert!(TerritoryPlanV1::compile(above_limit).is_err());

    let mut encoded = serde_json::to_value(plan(3))
        .unwrap_or_else(|error| panic!("plan value serialization failed: {error}"));
    encoded["candidate_selectors"][0]["candidate_indices"][0] = serde_json::Value::from(1_u64);
    let stale: TerritoryPlanV1 = serde_json::from_value(encoded)
        .unwrap_or_else(|error| panic!("tampered plan deserialization failed: {error}"));
    assert!(stale.validate_hash().is_err());
}
#[test]
fn provider_fingerprints_are_consistent_across_ownership_domains() {
    let mut fixture = input(3);
    let conflicting_identity = ProviderGenerationIdentityV1::new(
        stable("latticeaxiom:provider/terrain-default"),
        nz(1),
        1,
        CanonicalHash::digest(b"different-implementation"),
    );
    fixture.primary_offers.push(PrimaryProviderOfferV1::terrain(
        terrain_domain("woodland"),
        conflicting_identity,
    ));
    assert!(TerritoryPlanV1::compile(fixture).is_err());
}

#[test]
fn one_ownership_domain_cannot_span_multiple_atlas_scales() {
    let mut fixture = input(3);
    fixture.surface_candidates.push(
        SurfaceTerritoryCandidateV1::new(
            stable("latticeaxiom:territory-candidate/cross-scale"),
            terrain_domain("woodland"),
            terrain_domain("terrenia"),
            1,
            PlanningCellCoordinateV1::new(10, 10),
            nz(1),
        )
        .unwrap_or_else(|error| panic!("cross-scale candidate setup failed: {error}")),
    );
    assert!(TerritoryPlanV1::compile(fixture).is_err());
}

#[test]
fn drainage_portal_must_match_abstract_hydrology_domains() {
    let mut fixture = input(3);
    let limestone_basin = HydrologyBasinV1::new(
        stable("latticeaxiom:hydrology-basin/limestone"),
        cave_domain("limestone"),
        PlanningCellBoundsV1::new(0, 0, 4, 4)
            .unwrap_or_else(|error| panic!("valid basin bounds were rejected: {error}")),
        VerticalRangeV1::new(-64, -16)
            .unwrap_or_else(|error| panic!("valid basin range was rejected: {error}")),
        10,
        1_000,
    )
    .unwrap_or_else(|error| panic!("valid limestone basin was rejected: {error}"));
    let crystal_basin = HydrologyBasinV1::new(
        stable("latticeaxiom:hydrology-basin/crystal"),
        cave_domain("crystal"),
        PlanningCellBoundsV1::new(4, 0, 8, 4)
            .unwrap_or_else(|error| panic!("valid basin bounds were rejected: {error}")),
        VerticalRangeV1::new(-64, -16)
            .unwrap_or_else(|error| panic!("valid basin range was rejected: {error}")),
        0,
        1_000,
    )
    .unwrap_or_else(|error| panic!("valid crystal basin was rejected: {error}"));
    let connection_id = stable("latticeaxiom:hydrology-link/limestone-crystal");
    let connection = HydrologyConnectionV1::new(
        connection_id.clone(),
        limestone_basin.basin_id().clone(),
        crystal_basin.basin_id().clone(),
        CardinalDirectionV1::East,
        100,
    )
    .unwrap_or_else(|error| panic!("valid hydrology link was rejected: {error}"));
    fixture.hydrology = HydrologyPlanV1::compile(
        vec![limestone_basin, crystal_basin],
        vec![connection],
        fixture.limits,
    )
    .unwrap_or_else(|error| panic!("valid abstract hydrology was rejected: {error}"));
    fixture.cave_portals[0] = CavePortalV1::new(
        cave_domain("limestone"),
        PlanningCellCoordinateV1::new(0, 0),
        cave_domain("default"),
        PlanningCellCoordinateV1::new(-1, 0),
        [0, -32_000, 0],
        AxisV1::Y,
        2_000,
        3_000,
        PortalHydrologyContractV1::Drainage { connection_id },
    )
    .unwrap_or_else(|error| panic!("drainage portal setup failed: {error}"));
    assert!(TerritoryPlanV1::compile(fixture).is_err());
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(24))]

    #[test]
    fn candidate_rotation_preserves_plan_identity(rotation in 0_usize..512) {
        let baseline = plan(96);
        let mut rotated = input(96);
        let len = rotated.surface_candidates.len();
        rotated.surface_candidates.rotate_left(rotation % len);
        let compiled = TerritoryPlanV1::compile(rotated)
            .unwrap_or_else(|error| panic!("rotated plan was rejected: {error}"));
        prop_assert_eq!(baseline.plan_hash(), compiled.plan_hash());
    }
}
