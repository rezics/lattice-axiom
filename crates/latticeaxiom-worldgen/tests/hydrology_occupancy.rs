//! V6 hydrology occupancy: drainage, aquifers, continuity, and accounting.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "test fixtures fail immediately when authored IDs or invariants are invalid"
)]

use std::num::NonZeroU32;

use latticeaxiom_core::{CanonicalHash, StableId};
use latticeaxiom_worldgen::{
    AuthoredWorldgenBindingsV1, ChunkCoordinate, ChunkFaceV1, ChunkGenerationOutcomeV1,
    DimensionId, GenerationPlanInputV1, GenerationPlanV1, HydrologyFluidBindingsV1,
    HydrologyOccupancyConfigV1, HydrologyOccupancyInputV1, HydrologyOccupancyKindV1,
    NaturalLayerConfigV1, NaturalLayerInputV1, PlanActivationIdV1, ProviderGenerationIdentityV1,
    ProviderOfferV1, ProviderSlotV1, WorldSeedV1, WorldgenConfigV1, WorldgenError,
    WorldgenLimitsV1,
};

const AUTHORED_BINDINGS_JSON: &str =
    include_str!("../../../packages/terrenia/worldgen/data/authored-block-bindings-v1.json");

#[test]
fn hydrology_occupancy_requires_the_natural_layer() {
    let bindings = authored_bindings();
    let error = GenerationPlanV1::compile(
        GenerationPlanInputV1::new(
            dimension_id(),
            WorldSeedV1::from_integer(1),
            WorldgenConfigV1::default(),
            1,
            PlanActivationIdV1::from_hash(CanonicalHash::digest(b"hydro-activation")),
            d4_provider_offers(false),
            bindings.d4_vocabulary().expect("D4 vocabulary"),
            bindings.role_bindings().expect("role bindings"),
            bindings.catalog_closure().expect("catalog"),
            CanonicalHash::digest(b"authoritative-semantic-image"),
            vec![CanonicalHash::digest(b"lock-a")],
            WorldgenLimitsV1::default(),
        )
        .with_hydrology_occupancy(occupancy_input(HydrologyOccupancyConfigV1::default())),
    )
    .expect_err("occupancy without rivers must fail closed");
    assert!(matches!(
        error,
        WorldgenError::MissingNaturalLayerForHydrology
    ));
}

#[test]
fn occupancy_candidates_are_deterministic_and_do_not_change_snapshots() {
    let chunks = [
        ChunkCoordinate::new(-3, 0, -2),
        ChunkCoordinate::new(0, 0, 0),
        ChunkCoordinate::new(2, -1, 4),
    ];
    let first = occupancy_plan(11, false);
    let second = occupancy_plan(11, true);
    assert_eq!(
        first.hydrology_occupancy_hash(),
        second.hydrology_occupancy_hash()
    );
    let origin = ChunkCoordinate::new(0, 0, 0);
    assert_ne!(
        first
            .hydrology_occupancy_candidate(origin)
            .expect("seed 11 occupancy")
            .canonical_bytes()
            .unwrap(),
        occupancy_plan(12, false)
            .hydrology_occupancy_candidate(origin)
            .expect("seed 12 occupancy")
            .canonical_bytes()
            .unwrap()
    );

    let baseline = natural_plan(11, false);
    for coordinate in chunks {
        let left = first
            .hydrology_occupancy_candidate(coordinate)
            .expect("occupancy candidate");
        let right = second
            .hydrology_occupancy_candidate(coordinate)
            .expect("reversed occupancy candidate");
        assert_eq!(
            left.canonical_bytes().unwrap(),
            right.canonical_bytes().unwrap()
        );
        assert_eq!(
            snapshot_bytes(&first, coordinate),
            snapshot_bytes(&baseline, coordinate),
            "hydrology occupancy must not rewrite durable snapshot bytes"
        );
    }
}

#[test]
fn occupancy_candidate_matches_independent_coordinate_queries() {
    let plan = occupancy_plan(37, false);
    let coordinate = ChunkCoordinate::new(-1, 0, 2);
    let edge = i64::from(WorldgenConfigV1::default().chunk_edge_voxels);
    let origin = [
        i64::from(coordinate.x).saturating_mul(edge),
        i64::from(coordinate.y).saturating_mul(edge),
        i64::from(coordinate.z).saturating_mul(edge),
    ];
    let mut expected = Vec::new();
    for local_y in 0..edge {
        for local_z in 0..edge {
            for local_x in 0..edge {
                let sample = plan
                    .hydrology_occupancy_sample(
                        origin[0].saturating_add(local_x),
                        origin[1].saturating_add(local_y),
                        origin[2].saturating_add(local_z),
                    )
                    .expect("occupancy query remains available");
                if sample.is_occupied() {
                    expected.push((local_x, local_y, local_z, sample));
                }
            }
        }
    }

    let candidate = plan
        .hydrology_occupancy_candidate(coordinate)
        .expect("column-cached occupancy candidate");
    assert_eq!(candidate.cells().len(), expected.len());
    for (cell, (local_x, local_y, local_z, sample)) in candidate.cells().iter().zip(expected) {
        assert_eq!(i64::from(cell.x()), local_x);
        assert_eq!(i64::from(cell.y()), local_y);
        assert_eq!(i64::from(cell.z()), local_z);
        assert_eq!(cell.kind(), sample.kind());
        assert_eq!(cell.fluid(), sample.fluid().expect("occupied sample fluid"));
        assert_eq!(cell.level(), sample.level());
        assert_eq!(cell.flow(), sample.flow());
    }
}

#[test]
fn snapshot_cave_reuse_matches_independent_occupancy_generation() {
    let plan = occupancy_plan(37, false);
    for coordinate in [
        ChunkCoordinate::new(-1, -1, 2),
        ChunkCoordinate::new(0, 0, 0),
        ChunkCoordinate::new(2, 1, -3),
    ] {
        let outcome = plan
            .generate(plan.vacant_generation_request(coordinate).unwrap())
            .expect("snapshot candidate generates");
        let ChunkGenerationOutcomeV1::Prepared(snapshot) = outcome else {
            panic!("expected a prepared snapshot candidate");
        };
        let reused = plan
            .hydrology_occupancy_candidate_for_snapshot(&snapshot)
            .expect("snapshot cave decisions are reusable")
            .expect("the plan includes hydrology occupancy");
        let independent = plan
            .hydrology_occupancy_candidate(coordinate)
            .expect("independent occupancy candidate");
        assert_eq!(
            reused.canonical_bytes().unwrap(),
            independent.canonical_bytes().unwrap(),
            "snapshot reuse must preserve exact occupancy bytes for {coordinate:?}"
        );
    }
}

#[test]
fn snapshot_cave_reuse_rejects_a_foreign_generation_plan() {
    let producer = occupancy_plan(37, false);
    let coordinate = ChunkCoordinate::new(-1, 0, 2);
    let outcome = producer
        .generate(producer.vacant_generation_request(coordinate).unwrap())
        .expect("snapshot candidate generates");
    let ChunkGenerationOutcomeV1::Prepared(snapshot) = outcome else {
        panic!("expected a prepared snapshot candidate");
    };
    let error = occupancy_plan(38, false)
        .hydrology_occupancy_candidate_for_snapshot(&snapshot)
        .expect_err("generation identity mismatch must fail closed");
    assert!(matches!(
        error,
        WorldgenError::InvalidHydrologyOccupancy {
            field: "snapshot",
            ..
        }
    ));
}

#[test]
fn negative_coordinates_and_shared_faces_stay_continuous() {
    let plan = occupancy_plan(42, false);
    let coordinate = ChunkCoordinate::new(-4, 0, -3);
    let first = plan
        .hydrology_occupancy_candidate(coordinate)
        .expect("negative occupancy candidate");
    let again = occupancy_plan(42, true)
        .hydrology_occupancy_candidate(coordinate)
        .expect("regenerated occupancy candidate");
    assert_eq!(
        first.canonical_bytes().unwrap(),
        again.canonical_bytes().unwrap()
    );
    assert!(coordinate.x < 0 && coordinate.z < 0);

    let east = plan
        .hydrology_face_continuity(coordinate, ChunkFaceV1::PositiveX)
        .expect("east face");
    let west_neighbor =
        ChunkCoordinate::new(coordinate.x.saturating_add(1), coordinate.y, coordinate.z);
    let west = plan
        .hydrology_face_continuity(west_neighbor, ChunkFaceV1::NegativeX)
        .expect("west face of eastern neighbor");
    assert_eq!(east.occupancy_hash(), west.occupancy_hash());
    assert_eq!(east.first(), west.first());
    assert_eq!(east.second(), west.second());
    assert_eq!(east.occupied_cells(), west.occupied_cells());
}

#[test]
fn drainage_aquifer_and_lava_never_mix_in_one_cell() {
    let plan = occupancy_plan(42, false);
    let mut saw_water = false;
    let mut saw_lava = false;
    let mut saw_drainage = false;
    let mut saw_channel = false;
    for z in -48..48 {
        for x in -48..48 {
            let drainage = plan.drainage_sample(x, z).expect("drainage sample");
            let aquifer = plan.aquifer_sample(x, z).expect("aquifer sample");
            let river = plan.river_sample(x, z).expect("river sample");
            assert_eq!(drainage.is_connected(), river.in_channel());
            assert!(aquifer.water_table_y() > aquifer.lava_table_y());
            let height = plan.terrain_height(x, z);
            let sample_y = [
                i64::from(aquifer.lava_table_y()),
                i64::from(aquifer.water_table_y()),
                i64::from(height).saturating_sub(8),
                i64::from(height),
                i64::from(height).saturating_add(1),
                i64::from(height).saturating_add(2),
            ];
            for y in sample_y {
                let sample = plan
                    .hydrology_occupancy_sample(x, y, z)
                    .expect("occupancy sample");
                match sample.kind() {
                    HydrologyOccupancyKindV1::Empty => assert!(!sample.is_occupied()),
                    HydrologyOccupancyKindV1::SurfaceChannel => {
                        saw_channel = true;
                        assert_eq!(
                            sample.fluid().map(StableId::as_str),
                            Some("fixture:fluid/water")
                        );
                    }
                    HydrologyOccupancyKindV1::Drainage => {
                        saw_drainage = true;
                        saw_water = true;
                        assert_eq!(
                            sample.fluid().map(StableId::as_str),
                            Some("fixture:fluid/water")
                        );
                    }
                    HydrologyOccupancyKindV1::SurfaceWater | HydrologyOccupancyKindV1::Aquifer => {
                        saw_water = true;
                        assert_eq!(
                            sample.fluid().map(StableId::as_str),
                            Some("fixture:fluid/water")
                        );
                    }
                    HydrologyOccupancyKindV1::LavaPool => {
                        saw_lava = true;
                        assert_eq!(
                            sample.fluid().map(StableId::as_str),
                            Some("fixture:fluid/lava")
                        );
                    }
                }
            }
        }
    }
    assert!(saw_channel, "fixture seed must place surface channel water");
    assert!(
        saw_drainage,
        "fixture seed must drain a river into cave voids"
    );
    assert!(
        saw_water && saw_lava,
        "fixture seed must place both water and lava"
    );
}

#[test]
fn sea_level_fills_only_open_surface_columns_with_still_water() {
    let config = HydrologyOccupancyConfigV1 {
        sea_level_y: Some(30),
        max_cells_per_chunk: 32_768,
        max_in_flight_bytes: 8 * 1_024 * 1_024,
        ..HydrologyOccupancyConfigV1::default()
    };
    let plan = occupancy_plan_with_config(42, false, config);
    let (x, z, surface_y) = (-128_i64..=128)
        .flat_map(|z| (-128_i64..=128).map(move |x| (x, z)))
        .find_map(|(x, z)| {
            let surface_y = plan.terrain_height(x, z);
            let in_channel = plan
                .river_sample(x, z)
                .is_some_and(latticeaxiom_worldgen::RiverSampleV1::in_channel);
            (surface_y < 30 && !in_channel).then_some((x, z, surface_y))
        })
        .expect("fixture contains non-river terrain below sea level");

    for y in i64::from(surface_y).saturating_add(1)..=30 {
        let sample = plan
            .hydrology_occupancy_sample(x, y, z)
            .expect("hydrology sample");
        assert_eq!(sample.kind(), HydrologyOccupancyKindV1::SurfaceWater);
        assert_eq!(sample.flow(), latticeaxiom_worldgen::HydrologyFlowV1::Still);
        assert_eq!(
            sample.fluid().map(StableId::as_str),
            Some("fixture:fluid/water")
        );
    }
    assert_ne!(
        plan.hydrology_occupancy_sample(x, i64::from(surface_y), z)
            .expect("surface sample")
            .kind(),
        HydrologyOccupancyKindV1::SurfaceWater
    );
    assert_ne!(
        plan.hydrology_occupancy_sample(x, 31, z)
            .expect("above-sea sample")
            .kind(),
        HydrologyOccupancyKindV1::SurfaceWater
    );
}

#[test]
fn sea_level_must_stay_inside_the_world_column() {
    let world = WorldgenConfigV1::default();
    for sea_level_y in [world.world_floor_y - 1, world.world_ceiling_y + 1] {
        let config = HydrologyOccupancyConfigV1 {
            sea_level_y: Some(sea_level_y),
            ..HydrologyOccupancyConfigV1::default()
        };
        assert!(matches!(
            config.validate_for_world(&world),
            Err(WorldgenError::InvalidHydrologyOccupancy {
                field: "sea_level_y",
                ..
            })
        ));
    }
}

#[test]
fn occupancy_accounting_rejects_over_budget_chunks() {
    let config = HydrologyOccupancyConfigV1 {
        max_cells_per_chunk: 1,
        ..HydrologyOccupancyConfigV1::default()
    };
    let plan = occupancy_plan_with_config(7, false, config);
    let error = plan
        .hydrology_occupancy_candidate(ChunkCoordinate::new(0, 0, 0))
        .expect_err("a 16^3 chunk exceeds a one-cell occupancy budget");
    assert!(matches!(
        error,
        WorldgenError::BudgetExceeded {
            budget: "hydrology occupancy cells",
            ..
        }
    ));
}

fn occupancy_plan(seed: i64, reverse: bool) -> GenerationPlanV1 {
    occupancy_plan_with_config(seed, reverse, HydrologyOccupancyConfigV1::default())
}

fn occupancy_plan_with_config(
    seed: i64,
    reverse: bool,
    config: HydrologyOccupancyConfigV1,
) -> GenerationPlanV1 {
    let bindings = authored_bindings();
    GenerationPlanV1::compile(
        GenerationPlanInputV1::new(
            dimension_id(),
            WorldSeedV1::from_integer(seed),
            WorldgenConfigV1::default(),
            7,
            PlanActivationIdV1::from_hash(CanonicalHash::digest(b"hydro-activation")),
            d4_provider_offers(reverse),
            bindings.d4_vocabulary().expect("D4 vocabulary"),
            bindings.role_bindings().expect("role bindings"),
            bindings.catalog_closure().expect("catalog"),
            CanonicalHash::digest(b"authoritative-semantic-image"),
            vec![CanonicalHash::digest(b"lock-a")],
            WorldgenLimitsV1::default(),
        )
        .with_natural_layer(NaturalLayerInputV1::new(
            NaturalLayerConfigV1::default(),
            bindings.natural_vocabulary().expect("natural vocabulary"),
            natural_provider_offers(reverse),
        ))
        .with_hydrology_occupancy(occupancy_input(config)),
    )
    .expect("hydrology occupancy plan compiles")
}

fn natural_plan(seed: i64, reverse: bool) -> GenerationPlanV1 {
    let bindings = authored_bindings();
    GenerationPlanV1::compile(
        GenerationPlanInputV1::new(
            dimension_id(),
            WorldSeedV1::from_integer(seed),
            WorldgenConfigV1::default(),
            7,
            PlanActivationIdV1::from_hash(CanonicalHash::digest(b"hydro-activation")),
            d4_provider_offers(reverse),
            bindings.d4_vocabulary().expect("D4 vocabulary"),
            bindings.role_bindings().expect("role bindings"),
            bindings.catalog_closure().expect("catalog"),
            CanonicalHash::digest(b"authoritative-semantic-image"),
            vec![CanonicalHash::digest(b"lock-a")],
            WorldgenLimitsV1::default(),
        )
        .with_natural_layer(NaturalLayerInputV1::new(
            NaturalLayerConfigV1::default(),
            bindings.natural_vocabulary().expect("natural vocabulary"),
            natural_provider_offers(reverse),
        )),
    )
    .expect("natural plan compiles")
}

fn occupancy_input(config: HydrologyOccupancyConfigV1) -> HydrologyOccupancyInputV1 {
    let bindings = authored_bindings();
    HydrologyOccupancyInputV1::new(
        config,
        HydrologyFluidBindingsV1::new(
            stable_id("fixture:fluid/water"),
            stable_id("fixture:fluid/lava"),
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
    )
}

fn snapshot_bytes(plan: &GenerationPlanV1, coordinate: ChunkCoordinate) -> Vec<u8> {
    let outcome = plan
        .generate(plan.vacant_generation_request(coordinate).unwrap())
        .expect("chunk generates");
    let ChunkGenerationOutcomeV1::Prepared(candidate) = outcome else {
        panic!("expected a prepared snapshot candidate");
    };
    candidate.snapshot_bytes().to_vec()
}

fn authored_bindings() -> AuthoredWorldgenBindingsV1 {
    AuthoredWorldgenBindingsV1::from_json(AUTHORED_BINDINGS_JSON.as_bytes())
        .expect("authored bindings decode")
}

fn d4_provider_offers(reverse: bool) -> Vec<ProviderOfferV1> {
    let mut offers = slot_offers([
        (ProviderSlotV1::GenerationCoordinator, "coordinator", 7),
        (ProviderSlotV1::StyleSelector, "selector", 7),
        (ProviderSlotV1::TemperateTerrain, "temperate", 7),
        (ProviderSlotV1::AridTerrain, "arid", 7),
        (ProviderSlotV1::TerrainTransition, "transition", 7),
        (ProviderSlotV1::CaveTopology, "cave", 8),
        (ProviderSlotV1::Materializer, "materializer", 7),
    ]);
    if reverse {
        offers.reverse();
    }
    offers
}

fn natural_provider_offers(reverse: bool) -> Vec<ProviderOfferV1> {
    let mut offers = slot_offers([
        (ProviderSlotV1::Geology, "geology", 1),
        (ProviderSlotV1::Hydrology, "hydrology", 1),
        (ProviderSlotV1::Resources, "resources", 1),
        (ProviderSlotV1::Vegetation, "vegetation", 1),
        (ProviderSlotV1::BorealTerrain, "boreal", 1),
    ]);
    if reverse {
        offers.reverse();
    }
    offers
}

fn slot_offers<const N: usize>(
    slots: [(ProviderSlotV1, &'static str, u32); N],
) -> Vec<ProviderOfferV1> {
    slots
        .into_iter()
        .map(|(slot, path, revision)| {
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
        .collect()
}

fn dimension_id() -> DimensionId {
    "fixture:dimension/natural"
        .parse()
        .expect("fixture dimension is valid")
}

fn stable_id(value: &str) -> StableId {
    value.parse().expect("fixture stable IDs are valid")
}
