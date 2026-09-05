//! D7 territory, cave, and epoch conformance tests.

use std::{collections::BTreeSet, num::NonZeroU32};

use latticeaxiom_core::{CanonicalHash, StableId, canonical_json_bytes, canonical_json_hash};
use latticeaxiom_territory::{
    AtlasConfigV1, AtlasScaleV1, AxisV1, CardinalDirectionV1, CavePortalV1, CaveTopologyDomainIdV1,
    CaveTopologyParentV1, ChunkCoordinate, ContributionBudgetV1, ContributionChannelV1,
    ContributionCompositorV1, ContributionTargetV1, CoordinatorOfferV1, GenerationEpochIdV1,
    HydrologyBasinV1, HydrologyConnectionV1, HydrologyPlanV1, LockedClosureFingerprintV1,
    PlanningCellBoundsV1, PlanningCellCoordinateV1, PlanningCellEpochLedgerV1,
    PlanningCellTransitionAdapterV1, PlanningCellTransitionReceiptV1, PortalHydrologyContractV1,
    PrimaryChannelV1, PrimaryOwnershipDomainV1, PrimaryProviderOfferV1,
    ProviderGenerationIdentityV1, SpatialContributionV1, SurfaceTerritoryCandidateV1,
    TerritoryConflictDiagnosticV1, TerritoryDomainIdV1, TerritoryLimitsV1, TerritoryPlanInputV1,
    TerritoryPlanV1, TerritoryQueryCoverageV1, UndergroundTerritoryV1, VerticalRangeV1,
    WorldSeedV1, WorldgenConfigV1,
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

fn cave_branch_contribution() -> SpatialContributionV1 {
    SpatialContributionV1::new(
        stable("latticeaxiom:contribution/limestone-branch"),
        provider("cave-branch", 1),
        ContributionChannelV1::CaveBranch,
        ContributionCompositorV1::Union,
        ContributionTargetV1::Cave(cave_domain("limestone")),
        PlanningCellBoundsV1::new(1, 1, 3, 3)
            .unwrap_or_else(|error| panic!("valid branch bounds were rejected: {error}")),
        Some(
            VerticalRangeV1::new(-48, -24)
                .unwrap_or_else(|error| panic!("valid branch range was rejected: {error}")),
        ),
        1,
        4,
        ContributionBudgetV1::new(1_000, 4_096)
            .unwrap_or_else(|error| panic!("valid branch budget was rejected: {error}")),
        CanonicalHash::digest(b"limestone-branch-v1"),
    )
    .unwrap_or_else(|error| panic!("valid cave branch contribution was rejected: {error}"))
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
    let branch = cave_branch_contribution();
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
        contributions: vec![contribution, branch],
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
    let coordinator_error = TerritoryPlanV1::compile(coordinator_conflict)
        .err()
        .unwrap_or_else(|| panic!("conflicting coordinators were accepted"));
    let coordinator_diagnostic = coordinator_error
        .conflict_diagnostic()
        .unwrap_or_else(|| panic!("coordinator conflict lacked a diagnostic"));
    assert!(coordinator_diagnostic.is_exclusive_owner_conflict());
    assert_eq!(
        coordinator_diagnostic,
        TerritoryConflictDiagnosticV1::ConflictingCoordinators {
            providers: vec![
                "latticeaxiom:provider/coordinator".to_owned(),
                "latticeaxiom:provider/other-coordinator".to_owned(),
            ],
        }
    );

    let mut owner_conflict = input(3);
    owner_conflict
        .primary_offers
        .push(PrimaryProviderOfferV1::terrain(
            terrain_domain("terrenia"),
            provider("other-terrain", 1),
        ));
    let owner_error = TerritoryPlanV1::compile(owner_conflict)
        .err()
        .unwrap_or_else(|| panic!("conflicting primary owners were accepted"));
    assert_eq!(
        owner_error.conflict_diagnostic(),
        Some(TerritoryConflictDiagnosticV1::ConflictingPrimaryOwners {
            channel: "terrain.base".to_owned(),
            domain: terrain_domain("terrenia").to_string(),
            providers: vec![
                "latticeaxiom:provider/other-terrain".to_owned(),
                "latticeaxiom:provider/terrain-default".to_owned(),
            ],
        })
    );
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

fn production_surface_candidates() -> Vec<SurfaceTerritoryCandidateV1> {
    let default = terrain_domain("terrenia");
    let woodland = terrain_domain("woodland");
    let dunes = terrain_domain("dunes");
    let badlands = terrain_domain("badlands");
    [
        (
            "woodland",
            woodland.clone(),
            default.clone(),
            0_u8,
            0_i64,
            3_u32,
        ),
        ("dunes", dunes, default.clone(), 0, 64, 2),
        ("badlands", badlands, default, 0, -64, 2),
        (
            "woodland-hills",
            terrain_domain("woodland-hills"),
            woodland.clone(),
            1,
            0,
            1,
        ),
        (
            "woodland-grove",
            terrain_domain("woodland-grove"),
            terrain_domain("woodland-hills"),
            2,
            0,
            1,
        ),
    ]
    .into_iter()
    .map(|(name, domain, parent, level, anchor, weight)| {
        SurfaceTerritoryCandidateV1::new(
            stable(&format!("latticeaxiom:territory-candidate/{name}")),
            domain,
            parent,
            level,
            PlanningCellCoordinateV1::new(anchor, -anchor),
            nz(weight),
        )
        .unwrap_or_else(|error| panic!("valid production candidate {name} was rejected: {error}"))
    })
    .collect()
}

fn production_input() -> TerritoryPlanInputV1 {
    let mut fixture = input(3);
    fixture.surface_candidates = production_surface_candidates();
    fixture.contributions = vec![cave_branch_contribution()];
    fixture.primary_offers = vec![
        PrimaryProviderOfferV1::terrain(terrain_domain("terrenia"), provider("terrain-default", 1)),
        PrimaryProviderOfferV1::terrain(
            terrain_domain("woodland"),
            provider("terrain-woodland", 1),
        ),
        PrimaryProviderOfferV1::terrain(terrain_domain("dunes"), provider("terrain-dunes", 1)),
        PrimaryProviderOfferV1::terrain(
            terrain_domain("badlands"),
            provider("terrain-badlands", 1),
        ),
        PrimaryProviderOfferV1::cave(cave_domain("default"), provider("cave-default", 2)),
        PrimaryProviderOfferV1::cave(cave_domain("limestone"), provider("cave-limestone", 1)),
        PrimaryProviderOfferV1::cave(cave_domain("crystal"), provider("cave-crystal", 1)),
    ];
    fixture
}

fn production_plan() -> TerritoryPlanV1 {
    TerritoryPlanV1::compile(production_input())
        .unwrap_or_else(|error| panic!("valid production territory plan was rejected: {error}"))
}

fn production_lock() -> LockedClosureFingerprintV1 {
    LockedClosureFingerprintV1::from_hash(CanonicalHash::digest(b"lock-a"))
}

fn production_chunks() -> Vec<ChunkCoordinate> {
    let mut chunks = vec![
        ChunkCoordinate::new(8, -2, 8),
        ChunkCoordinate::new(40, 3, 8),
        ChunkCoordinate::new(0, 0, 0),
        ChunkCoordinate::new(160, 4, 160),
        ChunkCoordinate::new(8, 7, 8),
    ];
    for index in -6_i32..=6 {
        chunks.push(ChunkCoordinate::new(
            index.saturating_mul(512),
            index.rem_euclid(5).saturating_sub(2),
            index.saturating_mul(-384),
        ));
    }
    chunks
}

#[test]
fn production_queries_expose_three_surface_and_two_underground_territories() {
    let plan = production_plan();
    let mut surface = BTreeSet::new();
    let mut surface_owners = BTreeSet::new();
    for tile_z in -8_i64..=8 {
        for tile_x in -8_i64..=8 {
            let cell = PlanningCellCoordinateV1::new(
                tile_x.saturating_mul(64).saturating_add(32),
                tile_z.saturating_mul(64).saturating_add(32),
            );
            let query = plan
                .surface_territory_query(cell)
                .unwrap_or_else(|error| panic!("surface ownership query failed: {error}"));
            assert!(!query.primary().as_str().is_empty());
            assert!(!query.secondary().as_str().is_empty());
            assert_eq!(
                query.primary_owner().domain().as_str(),
                query.primary().as_str()
            );
            assert_eq!(
                query.secondary_owner().domain().as_str(),
                query.secondary().as_str()
            );
            surface.insert(query.primary().clone());
            surface_owners.insert(query.primary_owner().provider().clone());
        }
    }
    assert!(
        surface.len() >= 3,
        "production Atlas must expose at least three surface territories, found {surface:?}"
    );
    assert!(
        surface_owners.len() >= 3,
        "production surface territories must keep stable distinct primary owners"
    );

    let limestone = plan
        .underground_territory_query(PlanningCellCoordinateV1::new(1, 1), -32)
        .unwrap_or_else(|error| panic!("limestone ownership query failed: {error}"));
    let crystal = plan
        .underground_territory_query(PlanningCellCoordinateV1::new(5, 1), -32)
        .unwrap_or_else(|error| panic!("crystal ownership query failed: {error}"));
    let default_cave = plan
        .underground_territory_query(PlanningCellCoordinateV1::new(20, 20), -32)
        .unwrap_or_else(|error| panic!("default cave ownership query failed: {error}"));
    assert_eq!(limestone.primary(), &cave_domain("limestone"));
    assert_eq!(crystal.primary(), &cave_domain("crystal"));
    assert_eq!(default_cave.primary(), plan.default_cave_domain());
    assert_ne!(limestone.primary(), crystal.primary());
    assert_ne!(
        limestone.primary_owner().provider(),
        crystal.primary_owner().provider()
    );
    assert_eq!(limestone.secondary(), plan.default_cave_domain());
    assert_eq!(crystal.secondary(), plan.default_cave_domain());
    assert_ne!(default_cave.secondary(), default_cave.primary());
    assert_eq!(limestone.boundary_distance_cells(), 1);
    assert_eq!(crystal.boundary_distance_cells(), 1);

    let limestone_order = limestone.ordered_candidates();
    assert_eq!(limestone_order[0].rank(), 1);
    assert_eq!(limestone_order[0].domain(), limestone.primary());
    assert_eq!(limestone_order[1].rank(), 2);
    assert_eq!(limestone_order[1].domain(), limestone.secondary());
    assert_eq!(
        limestone_order[0].owner().channel(),
        PrimaryChannelV1::CaveTopology
    );
    assert_eq!(
        limestone_order[0].owner().domain().channel(),
        PrimaryChannelV1::CaveTopology
    );
}

#[test]
fn production_plan_hash_matches_golden() {
    let plan = production_plan();
    assert_eq!(
        plan.plan_hash().to_string(),
        include_str!("goldens/production-plan-hash.txt").trim()
    );
}

#[test]
fn production_coverage_bytes_are_identical_under_shuffled_chunk_order() {
    let plan = production_plan();
    let config = WorldgenConfigV1::default();
    let lock = production_lock();
    let chunks = production_chunks();
    let forward = TerritoryQueryCoverageV1::from_chunks(&plan, lock, &config, chunks.clone(), -32)
        .unwrap_or_else(|error| panic!("forward production coverage failed: {error}"));
    let reverse =
        TerritoryQueryCoverageV1::from_chunks(&plan, lock, &config, chunks.into_iter().rev(), -32)
            .unwrap_or_else(|error| panic!("reversed production coverage failed: {error}"));
    let forward_bytes = canonical_json_bytes(&forward)
        .unwrap_or_else(|error| panic!("forward coverage encoding failed: {error}"));
    let reverse_bytes = canonical_json_bytes(&reverse)
        .unwrap_or_else(|error| panic!("reversed coverage encoding failed: {error}"));
    assert_eq!(forward_bytes, reverse_bytes);
    assert_eq!(
        canonical_json_hash(&forward)
            .unwrap_or_else(|error| panic!("coverage hash failed: {error}"))
            .to_string(),
        include_str!("goldens/production-coverage-hash.txt").trim()
    );

    let surface = forward
        .surface()
        .iter()
        .map(latticeaxiom_territory::SurfaceTerritoryQueryV1::primary)
        .cloned()
        .collect::<BTreeSet<_>>();
    let underground = forward
        .underground()
        .iter()
        .map(latticeaxiom_territory::UndergroundTerritoryQueryV1::primary)
        .cloned()
        .collect::<BTreeSet<_>>();
    assert!(
        surface.len() >= 3,
        "shuffled chunk coverage must keep at least three surface territories, found {surface:?}"
    );
    assert!(
        underground.len() >= 2,
        "shuffled chunk coverage must keep at least two underground territories, found {underground:?}"
    );

    let mutated_lock = LockedClosureFingerprintV1::from_hash(CanonicalHash::digest(b"lock-b"));
    let mutated = TerritoryQueryCoverageV1::from_chunks(
        &plan,
        mutated_lock,
        &config,
        production_chunks(),
        -32,
    )
    .unwrap_or_else(|error| panic!("lock-divergent coverage failed: {error}"));
    assert_ne!(
        canonical_json_bytes(&mutated)
            .unwrap_or_else(|error| panic!("divergent coverage encoding failed: {error}")),
        forward_bytes
    );
}

#[test]
fn parallel_queries_match_sequential_queries() {
    let plan = production_plan();
    let cells = (-32_i64..32)
        .flat_map(|z| (-32_i64..32).map(move |x| PlanningCellCoordinateV1::new(x, z)))
        .collect::<Vec<_>>();
    let sequential = cells
        .iter()
        .map(|cell| {
            (
                plan.query(*cell),
                plan.surface_territory_query(*cell)
                    .unwrap_or_else(|error| panic!("sequential surface query failed: {error}")),
                plan.underground_territory_query(*cell, -32)
                    .unwrap_or_else(|error| panic!("sequential underground query failed: {error}")),
            )
        })
        .collect::<Vec<_>>();
    let parallel = std::thread::scope(|scope| {
        let workers = 4_usize;
        let chunk_len = cells.len().div_ceil(workers);
        let handles = cells
            .chunks(chunk_len)
            .map(|chunk| {
                scope.spawn(|| {
                    chunk
                        .iter()
                        .map(|cell| {
                            (
                                plan.query(*cell),
                                plan.surface_territory_query(*cell).unwrap_or_else(|error| {
                                    panic!("parallel surface query failed: {error}")
                                }),
                                plan.underground_territory_query(*cell, -32).unwrap_or_else(
                                    |error| panic!("parallel underground query failed: {error}"),
                                ),
                            )
                        })
                        .collect::<Vec<_>>()
                })
            })
            .collect::<Vec<_>>();
        handles
            .into_iter()
            .flat_map(|handle| {
                handle
                    .join()
                    .unwrap_or_else(|_| panic!("query worker panicked"))
            })
            .collect::<Vec<_>>()
    });
    assert_eq!(sequential, parallel);
}

#[test]
fn production_plan_bytes_ignore_registration_and_chunk_order() {
    let first = production_plan();
    let mut reordered = production_input();
    reordered.surface_candidates.reverse();
    reordered.primary_offers.reverse();
    reordered.underground_territories.reverse();
    reordered.cave_portals.reverse();
    let second = TerritoryPlanV1::compile(reordered)
        .unwrap_or_else(|error| panic!("reordered production plan was rejected: {error}"));
    assert_eq!(first.plan_hash(), second.plan_hash());
    assert_eq!(
        canonical_json_bytes(&first).ok(),
        canonical_json_bytes(&second).ok()
    );
}

fn cave_plan_chunks() -> Vec<ChunkCoordinate> {
    let mut chunks = production_chunks();
    for cell_x in -2_i32..=10 {
        chunks.push(ChunkCoordinate::new(cell_x.saturating_mul(8), -2, 0));
        chunks.push(ChunkCoordinate::new(cell_x.saturating_mul(8), -2, 8));
    }
    chunks
}

#[test]
fn v6_cave_plan_has_default_domain_two_subdomains_entrance_portal_and_destination() {
    let plan = production_plan();
    let config = WorldgenConfigV1::default();
    let cave_plan = plan
        .cave_topology_plan(&config)
        .unwrap_or_else(|error| panic!("V6 cave topology plan failed: {error}"));
    assert_eq!(cave_plan.default_cave_domain(), plan.default_cave_domain());
    assert_eq!(cave_plan.underground_domains().len(), 2);
    assert!(
        cave_plan
            .underground_domains()
            .contains(&cave_domain("limestone"))
    );
    assert!(
        cave_plan
            .underground_domains()
            .contains(&cave_domain("crystal"))
    );
    assert!(!cave_plan.surface_entrances().is_empty());
    assert!(!cave_plan.portals().is_empty());
    assert!(!cave_plan.must_connect().is_empty());
    assert!(
        cave_plan.portals().iter().any(|portal| {
            let (first, second) = portal.domains();
            first != second
        }),
        "V6 cave plan must include a cross-domain portal"
    );

    let owners = plan
        .primary_owners()
        .iter()
        .filter(|owner| owner.domain().channel() == PrimaryChannelV1::CaveTopology)
        .map(|owner| owner.domain().clone())
        .collect::<BTreeSet<_>>();
    assert!(owners.contains(&PrimaryOwnershipDomainV1::Cave(
        plan.default_cave_domain().clone()
    )));
    assert!(
        !plan
            .primary_owners()
            .iter()
            .any(
                |owner| owner.domain().channel() != PrimaryChannelV1::CaveTopology
                    && matches!(owner.domain(), PrimaryOwnershipDomainV1::Cave(_))
            ),
        "hydrology cannot own cave topology domains"
    );
}

#[test]
fn v6_surface_entrance_crosses_four_cells_and_two_topology_domains() {
    let plan = production_plan();
    let cave_plan = plan
        .cave_topology_plan(&WorldgenConfigV1::default())
        .unwrap_or_else(|error| panic!("V6 cave topology plan failed: {error}"));
    let entrance = cave_plan
        .surface_entrances()
        .iter()
        .find(|entrance| entrance.cells().len() >= 4 && entrance.domains().len() >= 2)
        .unwrap_or_else(|| panic!("V6 cave plan lacks a four-cell two-domain entrance"));
    let cells = entrance.cells();
    for window in cells.windows(2) {
        assert!(
            (window[0].x - window[1].x).abs() + (window[0].z - window[1].z).abs() == 1,
            "entrance cells must be cardinal neighbors"
        );
    }
    assert_eq!(entrance.surface_cell(), cells[0]);
    assert_eq!(entrance.destination().cell(), cells[cells.len() - 1]);
    assert_eq!(
        entrance.destination().domain(),
        entrance.domains().last().unwrap_or_else(|| panic!(
            "entrance domain list is non-empty after the four-cell check"
        ))
    );
    assert_ne!(
        entrance.domains()[0],
        *entrance.destination().domain(),
        "surface opening and must-connect destination must be distinct domains"
    );
    assert!(!entrance.portals().is_empty());
}

#[test]
fn v6_portal_assertions_expose_position_tangent_clearance_and_fluid() {
    let plan = production_plan();
    let cave_plan = plan
        .cave_topology_plan(&WorldgenConfigV1::default())
        .unwrap_or_else(|error| panic!("V6 cave topology plan failed: {error}"));
    assert!(!cave_plan.assertions().is_empty());
    for (portal, assertion) in cave_plan.portals().iter().zip(cave_plan.assertions()) {
        assert_eq!(assertion.portal_id(), portal.portal_id());
        assert_eq!(
            assertion.position_millimeters(),
            portal.anchor_millimeters()
        );
        assert_eq!(assertion.tangent_axis(), portal.tangent_axis());
        assert_eq!(assertion.tangent_axis(), AxisV1::Y);
        assert!(assertion.clearance_width_millimeters() > 0);
        assert!(assertion.clearance_height_millimeters() > 0);
        assert_eq!(assertion.fluid(), portal.hydrology());
        assert!(matches!(
            assertion.fluid(),
            PortalHydrologyContractV1::Dry | PortalHydrologyContractV1::Sealed
        ));
        let encoded = serde_json::to_value(assertion)
            .unwrap_or_else(|error| panic!("portal assertion encoding failed: {error}"));
        assert!(encoded.get("position_millimeters").is_some());
        assert!(encoded.get("tangent_axis").is_some());
        assert!(encoded.get("clearance_width_millimeters").is_some());
        assert!(encoded.get("clearance_height_millimeters").is_some());
        assert!(encoded.get("fluid").is_some());
    }
}

#[test]
fn v6_cave_portal_plan_is_identical_under_shuffled_chunk_order() {
    let plan = production_plan();
    let config = WorldgenConfigV1::default();
    let chunks = cave_plan_chunks();
    let forward = plan
        .cave_topology_plan_from_chunks(&config, chunks.clone())
        .unwrap_or_else(|error| panic!("forward V6 cave plan failed: {error}"));
    let reversed = plan
        .cave_topology_plan_from_chunks(&config, chunks.iter().copied().rev())
        .unwrap_or_else(|error| panic!("reversed V6 cave plan failed: {error}"));
    let rotated = {
        let mut rotated = chunks;
        let rotation = rotated.len() / 3;
        rotated.rotate_left(rotation);
        plan.cave_topology_plan_from_chunks(&config, rotated)
            .unwrap_or_else(|error| panic!("rotated V6 cave plan failed: {error}"))
    };
    let forward_bytes = canonical_json_bytes(&forward)
        .unwrap_or_else(|error| panic!("forward cave plan encoding failed: {error}"));
    assert_eq!(
        forward_bytes,
        canonical_json_bytes(&reversed)
            .unwrap_or_else(|error| panic!("reversed cave plan encoding failed: {error}"))
    );
    assert_eq!(
        forward_bytes,
        canonical_json_bytes(&rotated)
            .unwrap_or_else(|error| panic!("rotated cave plan encoding failed: {error}"))
    );
    assert_eq!(forward.world_seed(), plan.world_seed());
    assert!(
        forward
            .surface_entrances()
            .iter()
            .any(|entrance| entrance.cells().len() >= 4 && entrance.domains().len() >= 2)
    );
}

#[test]
fn recursive_surface_delegation_and_cave_channel_stay_independent() {
    let plan = production_plan();
    let mut grove_query = None;
    for tile_z in -8_i64..=8 {
        for tile_x in -8_i64..=8 {
            let cell = PlanningCellCoordinateV1::new(
                tile_x.saturating_mul(64).saturating_add(32),
                tile_z.saturating_mul(64).saturating_add(32),
            );
            let query = plan
                .surface_territory_query(cell)
                .unwrap_or_else(|error| panic!("surface ownership query failed: {error}"));
            if query.primary() == &terrain_domain("woodland-grove") {
                grove_query = Some((cell, query));
                break;
            }
        }
        if grove_query.is_some() {
            break;
        }
    }
    let (cell, query) = grove_query.unwrap_or_else(|| {
        panic!("production Atlas never selected the nested woodland-grove child")
    });
    let atlas = plan.query(cell);
    assert_eq!(atlas.levels().len(), 3);
    assert_eq!(atlas.levels()[0].domain(), &terrain_domain("woodland"));
    assert_eq!(
        atlas.levels()[1].domain(),
        &terrain_domain("woodland-hills")
    );
    assert_eq!(
        atlas.levels()[2].domain(),
        &terrain_domain("woodland-grove")
    );
    assert_eq!(
        atlas.levels()[0].local_offset_cells(cell),
        (cell.x.rem_euclid(64), cell.z.rem_euclid(64))
    );
    assert_eq!(atlas.levels()[2].local_offset_cells(cell), (0, 0));
    let ordered = query.ordered_candidates();
    assert_eq!(ordered[0].rank(), 1);
    assert_eq!(ordered[0].domain(), query.primary());
    assert_eq!(ordered[0].candidate_id(), query.primary_candidate());
    assert_eq!(ordered[1].rank(), 2);
    assert_eq!(ordered[1].domain(), query.secondary());
    assert_eq!(ordered[1].candidate_id(), query.secondary_candidate());
    assert_ne!(ordered[0].domain(), ordered[1].domain());
    assert_eq!(
        query.primary_owner().channel(),
        PrimaryChannelV1::TerrainBase
    );

    let cave = plan
        .underground_territory_query(cell, -32)
        .unwrap_or_else(|error| panic!("cave ownership query failed: {error}"));
    assert_ne!(
        cave.primary_owner().channel(),
        PrimaryChannelV1::TerrainBase,
        "surface child takeover must not claim cave.topology"
    );
    assert_eq!(
        cave.primary_owner().channel(),
        PrimaryChannelV1::CaveTopology
    );
}

#[test]
fn sibling_underground_overlap_emits_stable_conflict_diagnostic() {
    let mut fixture = input(3);
    fixture
        .underground_territories
        .push(UndergroundTerritoryV1::new(
            cave_domain("overlap"),
            CaveTopologyParentV1::DimensionDefault,
            PlanningCellBoundsV1::new(0, 0, 2, 2)
                .unwrap_or_else(|error| panic!("valid overlap bounds were rejected: {error}")),
            VerticalRangeV1::new(-64, -16)
                .unwrap_or_else(|error| panic!("valid overlap range was rejected: {error}")),
        ));
    let error = TerritoryPlanV1::compile(fixture)
        .err()
        .unwrap_or_else(|| panic!("overlapping underground siblings were accepted"));
    let diagnostic = error
        .conflict_diagnostic()
        .unwrap_or_else(|| panic!("overlap failure lacked a conflict diagnostic"));
    match &diagnostic {
        TerritoryConflictDiagnosticV1::InvalidUndergroundTerritory { territory, reason } => {
            assert_eq!(territory, &cave_domain("overlap").to_string());
            assert!(
                reason.contains("overlaps sibling"),
                "expected sibling-overlap diagnostic, got {reason}"
            );
        }
        other => panic!("unexpected conflict diagnostic: {other:?}"),
    }
    assert_eq!(
        diagnostic
            .canonical_hash()
            .ok()
            .map(|hash| hash.to_string()),
        Some(
            include_str!("goldens/sibling-overlap-conflict-hash.txt")
                .trim()
                .to_owned()
        )
    );
}

#[test]
fn production_plan_receipt_is_finite_and_matches_golden() {
    let plan = production_plan();
    let receipt = plan
        .receipt()
        .unwrap_or_else(|error| panic!("production plan receipt failed: {error}"));
    receipt
        .validate()
        .unwrap_or_else(|error| panic!("production plan receipt failed validation: {error}"));
    assert_eq!(receipt.plan_hash(), plan.plan_hash());
    assert_eq!(receipt.world_seed(), plan.world_seed());
    assert_eq!(receipt.dimension(), plan.dimension());
    assert_eq!(receipt.coordinator(), plan.coordinator());
    assert_eq!(receipt.atlas(), plan.atlas());
    assert_eq!(
        receipt.surface_candidate_count(),
        u32::try_from(plan.surface_candidates().len())
            .unwrap_or_else(|error| panic!("surface candidate count overflowed u32: {error}"))
    );
    assert_eq!(receipt.underground_territory_count(), 2);
    assert_eq!(receipt.hydrology_plan_hash(), plan.hydrology().plan_hash());
    assert_eq!(
        receipt.receipt_hash().to_string(),
        include_str!("goldens/production-plan-receipt-hash.txt").trim()
    );
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
        prop_assert_eq!(
            baseline.receipt().ok().map(|receipt| receipt.receipt_hash()),
            compiled.receipt().ok().map(|receipt| receipt.receipt_hash())
        );
    }

    #[test]
    fn queries_are_stable_for_negative_and_positive_cells(
        x in -4096_i64..4096,
        z in -4096_i64..4096,
    ) {
        let plan = plan(24);
        let cell = PlanningCellCoordinateV1::new(x, z);
        let first = plan.query(cell);
        let second = plan.query(cell);
        prop_assert_eq!(first, second);
        let surface = plan
            .surface_territory_query(cell)
            .unwrap_or_else(|error| panic!("surface query failed: {error}"));
        let again = plan
            .surface_territory_query(cell)
            .unwrap_or_else(|error| panic!("repeated surface query failed: {error}"));
        prop_assert_eq!(&surface, &again);
        let ordered = surface.ordered_candidates();
        prop_assert_eq!(ordered[0].domain(), surface.primary());
        prop_assert_eq!(ordered[1].domain(), surface.secondary());
        let underground = plan
            .underground_territory_query(cell, -32)
            .unwrap_or_else(|error| panic!("underground query failed: {error}"));
        let underground_ordered = underground.ordered_candidates();
        prop_assert_eq!(underground_ordered[0].domain(), underground.primary());
    }
}
