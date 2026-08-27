//! Deterministic spawn selection against package-authored Role/Predicate JSON.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "test fixtures fail immediately when authored IDs or invariants are invalid"
)]

mod support;

use std::num::NonZeroU32;

use latticeaxiom_core::{CanonicalHash, StableId};
use latticeaxiom_worldgen::{
    AuthoredWorldgenBindingsV1, ChunkGenerationOutcomeV1, D4MaterialRoleV1, DimensionId,
    GenerationPlanInputV1, GenerationPlanV1, PlanActivationIdV1, ProviderGenerationIdentityV1,
    ProviderOfferV1, ProviderSlotV1, SpawnCellOverrideV1, SpawnOccupancyViewV1,
    SpawnSearchBoundsV1, WorldSeedV1, WorldgenConfigV1, WorldgenError, WorldgenLimitsV1,
    evaluate_spawn_column, inspect_spawn_cell, required_spawn_chunks, select_safe_spawn,
};
use support::surface_terrain_programs;

const AUTHORED_BINDINGS_JSON: &str =
    include_str!("../../../packages/terrenia/worldgen/data/authored-block-bindings-v1.json");

#[test]
fn authored_terrenia_bindings_close_roles_and_spawn_predicates() {
    let bindings = authored_bindings();
    let vocabulary = bindings.d4_vocabulary().expect("D4 vocabulary is complete");
    let catalog = bindings
        .catalog_closure()
        .expect("authored candidates form a D4 catalog");
    assert_eq!(vocabulary.iter().count(), D4MaterialRoleV1::ALL.len());
    assert!(catalog.blocks().len() >= 18);
    for path in [
        "place-empty",
        "place-solid",
        "place-surface",
        "place-water",
        "place-lava",
        "fluid-replaceable",
    ] {
        let predicate = bindings
            .predicate(path)
            .unwrap_or_else(|| panic!("missing authored Predicate `{path}`"));
        assert_eq!(predicate.kind(), "predicate");
        assert_eq!(predicate.path(), path);
    }
    assert_eq!(
        bindings.cactus_block().map(StableId::as_str),
        Some("terrenia:block/cactus")
    );
}

#[test]
fn same_seed_selects_the_same_spawn() {
    let bindings = authored_bindings();
    let bounds = SpawnSearchBoundsV1::origin_neighborhood();
    let first_plan = fixture_plan(42);
    let second_plan = fixture_plan(42);
    let first_occupancy = ready_occupancy(&first_plan, bounds);
    let second_occupancy = ready_occupancy(&second_plan, bounds);
    let first = select_safe_spawn(&first_plan, &bindings, &first_occupancy, bounds)
        .expect("seed 42 has a safe spawn");
    let second = select_safe_spawn(&second_plan, &bindings, &second_occupancy, bounds)
        .expect("independent seed 42 run has a safe spawn");
    assert_eq!(first, second);
    assert_eq!(first.footing()[0], second.footing()[0]);
    assert_eq!(first.footing()[2], second.footing()[2]);

    let divergent_plan = fixture_plan(43);
    let divergent_occupancy = ready_occupancy(&divergent_plan, bounds);
    let divergent = select_safe_spawn(&divergent_plan, &bindings, &divergent_occupancy, bounds)
        .expect("seed 43 has a safe spawn");
    let again = select_safe_spawn(&divergent_plan, &bindings, &divergent_occupancy, bounds)
        .expect("seed 43 remains deterministic");
    assert_eq!(divergent, again);
}

#[test]
fn unsafe_fluid_and_cave_cells_are_rejected() {
    let bindings = authored_bindings();
    let bounds = SpawnSearchBoundsV1::origin_neighborhood();
    let plan = fixture_plan(42);
    let occupancy = ready_occupancy(&plan, bounds);
    let spawn =
        select_safe_spawn(&plan, &bindings, &occupancy, bounds).expect("baseline spawn is safe");
    let [x, footing_y, z] = spawn.footing();
    let feet_y = spawn.feet()[1];

    let mut fluid_occupancy = occupancy.clone();
    fluid_occupancy.overlay(x, feet_y, z, SpawnCellOverrideV1::fluid());
    assert!(matches!(
        evaluate_spawn_column(
            &plan,
            &bindings,
            &fluid_occupancy,
            x,
            z,
            bounds.clearance_voxels()
        ),
        Err(WorldgenError::UnsafeSpawnCell {
            reason: "fluid",
            ..
        })
    ));

    let mut cave_occupancy = occupancy.clone();
    cave_occupancy.overlay(x, footing_y, z, SpawnCellOverrideV1::cave());
    assert!(matches!(
        evaluate_spawn_column(
            &plan,
            &bindings,
            &cave_occupancy,
            x,
            z,
            bounds.clearance_voxels()
        ),
        Err(WorldgenError::UnsafeSpawnCell { reason: "cave", .. })
    ));
    let cave_cell = inspect_spawn_cell(&plan, &bindings, &cave_occupancy, x, footing_y, z)
        .expect("overlaid cave cell stays in a ready chunk");
    assert!(cave_cell.is_cave());
    assert!(!cave_cell.is_solid());
}

#[test]
fn missing_chunk_receipt_fails_closed_as_unready() {
    let bindings = authored_bindings();
    let plan = fixture_plan(42);
    let empty = SpawnOccupancyViewV1::new();
    assert!(matches!(
        evaluate_spawn_column(&plan, &bindings, &empty, 0, 0, 2),
        Err(WorldgenError::UnreadySpawnChunk { .. })
    ));
}

fn authored_bindings() -> AuthoredWorldgenBindingsV1 {
    AuthoredWorldgenBindingsV1::from_json(AUTHORED_BINDINGS_JSON.as_bytes())
        .expect("@terrenia/worldgen authored bindings must decode")
}

fn fixture_plan(seed: i64) -> GenerationPlanV1 {
    let bindings = authored_bindings();
    GenerationPlanV1::compile(
        GenerationPlanInputV1::new(
            dimension_id(),
            WorldSeedV1::from_integer(seed),
            WorldgenConfigV1::default(),
            7,
            PlanActivationIdV1::from_hash(CanonicalHash::digest(b"spawn-fixture-activation")),
            provider_offers(),
            bindings.d4_vocabulary().expect("authored D4 vocabulary"),
            bindings.role_bindings().expect("authored role bindings"),
            bindings
                .catalog_closure()
                .expect("authored catalog closure"),
            CanonicalHash::digest(b"authoritative-semantic-image"),
            vec![CanonicalHash::digest(b"lock-a")],
            WorldgenLimitsV1::default(),
        )
        .with_surface_biome_terrain_programs(surface_terrain_programs(false, false)),
    )
    .expect("authored spawn fixture plan compiles")
}

fn ready_occupancy(plan: &GenerationPlanV1, bounds: SpawnSearchBoundsV1) -> SpawnOccupancyViewV1 {
    let mut occupancy = SpawnOccupancyViewV1::new();
    for coordinate in required_spawn_chunks(plan, bounds).expect("spawn chunks are in range") {
        let request = plan
            .vacant_generation_request(coordinate)
            .expect("vacant request is representable");
        let ChunkGenerationOutcomeV1::Prepared(candidate) = plan
            .generate(request)
            .expect("spawn-neighborhood chunk generates")
        else {
            panic!("spawn occupancy requires a new snapshot candidate, not reused evidence");
        };
        occupancy.insert_ready_draft(coordinate, candidate.draft().clone());
    }
    occupancy
}

fn provider_offers() -> Vec<ProviderOfferV1> {
    [
        (ProviderSlotV1::GenerationCoordinator, "coordinator"),
        (ProviderSlotV1::StyleSelector, "selector"),
        (ProviderSlotV1::TerrainTransition, "transition"),
        (ProviderSlotV1::CaveTopology, "cave"),
        (ProviderSlotV1::Materializer, "materializer"),
    ]
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
                format!("fixture:worldgen-provider/{path}@1")
                    .parse()
                    .expect("fixture provider IDs are valid"),
                NonZeroU32::MIN,
                revision,
                CanonicalHash::digest(format!("{path}-implementation-v{revision}")),
            ),
        )
    })
    .collect()
}

fn dimension_id() -> DimensionId {
    "terrenia:dimension/terrenia"
        .parse()
        .expect("fixture dimension is valid")
}
