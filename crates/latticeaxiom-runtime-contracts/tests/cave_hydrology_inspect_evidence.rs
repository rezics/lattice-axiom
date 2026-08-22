//! Deterministic V6 cave/hydrology inspect and command-path fixtures.

#![allow(
    clippy::expect_used,
    reason = "integration fixtures state their construction invariants"
)]

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs;

use latticeaxiom_core::{CanonicalHash, SchemaId, StableId};
use latticeaxiom_runtime_contracts::{
    CAVE_COMMAND_PATH_SCHEMA_V1, CaveCommandActionV1, CaveCommandPathFixtureV1,
    CaveCommandResourceClassV1, CaveCommandStageV1, CaveCommandStepV1, CaveCommandWaypointV1,
    CaveConnectivityInspectFactsV1, CaveInspectAxisV1, CaveInspectRejectV1,
    CaveOwnershipInspectFactsV1, CaveRejectionInspectFactsV1, CaveSdfInspectFactsV1, EngineEpoch,
    EntranceInspectFactsV1, FluidDecisionInspectFactsV1, FluidOccupancyInspectV1,
    PlanningSeamInspectFactsV1, PortalInspectFactsV1, PortalInspectFluidV1,
    WORLDGEN_INSPECT_REPORT_SCHEMA_V1, WorldEpoch, WorldgenInspectBodyV1,
    WorldgenInspectCollectionV1, WorldgenInspectError, WorldgenInspectKindV1,
    WorldgenInspectLimits, WorldgenInspectQueryV1, WorldgenInspectRecordV1,
    WorldgenInspectSamplesV1, cave_command_path_schema, compile_worldgen_inspect_report,
};
use serde::Deserialize;

const AUTHORED_BINDINGS: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../packages/terrenia/worldgen/data/authored-block-bindings-v1.json"
);
const AUTHORED_BIOMES: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../packages/terrenia/worldgen/data/authored-biomes-v1.json"
);
const AUTHORED_LAYERS: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../packages/terrenia/worldgen/data/authored-natural-layers-v1.json"
);
const GOLDEN_REPORT: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/worldgen-inspect/cave-hydrology-negative-origin.json"
);
const GOLDEN_COMMAND_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/worldgen-inspect/cave-command-path-v1.json"
);

#[derive(Deserialize)]
struct AuthoredBindings {
    roles: Vec<AuthoredRole>,
    predicates: Vec<AuthoredPredicate>,
}

#[derive(Deserialize)]
struct AuthoredRole {
    id: String,
    candidate: String,
}

#[derive(Deserialize)]
struct AuthoredPredicate {
    id: String,
}

struct FrozenBindings {
    roles: BTreeMap<String, (StableId, StableId)>,
    predicates: BTreeMap<String, StableId>,
}

impl FrozenBindings {
    fn load() -> Self {
        let document: AuthoredBindings = serde_json::from_slice(&read(AUTHORED_BINDINGS))
            .unwrap_or_else(|error| panic!("authored bindings: {error}"));
        let mut roles = BTreeMap::new();
        for row in document.roles {
            roles.insert(row.id.clone(), (stable(&row.id), stable(&row.candidate)));
        }
        let mut predicates = BTreeMap::new();
        for row in document.predicates {
            predicates.insert(row.id.clone(), stable(&row.id));
        }
        Self { roles, predicates }
    }

    fn role(&self, id: &str) -> (StableId, StableId) {
        self.roles
            .get(id)
            .cloned()
            .unwrap_or_else(|| panic!("missing frozen Role `{id}`"))
    }

    fn predicate(&self, id: &str) -> StableId {
        self.predicates
            .get(id)
            .cloned()
            .unwrap_or_else(|| panic!("missing frozen Predicate `{id}`"))
    }
}

fn read(path: &str) -> Vec<u8> {
    fs::read(path).unwrap_or_else(|error| panic!("fixture `{path}` must be readable: {error}"))
}

fn stable(value: &str) -> StableId {
    value
        .parse()
        .unwrap_or_else(|error| panic!("stable ID `{value}`: {error}"))
}

fn schema(value: &str) -> SchemaId {
    value
        .parse()
        .unwrap_or_else(|error| panic!("schema ID `{value}`: {error}"))
}

fn hash(label: &str) -> CanonicalHash {
    CanonicalHash::digest(label.as_bytes())
}

fn cave_query() -> WorldgenInspectQueryV1 {
    WorldgenInspectQueryV1 {
        engine_epoch: EngineEpoch::new(7),
        world_epoch: Some(WorldEpoch::new(11)),
        generation_input_hash: hash("cave-evidence-input"),
        generation_provenance_hash: hash("cave-evidence-provenance"),
        origin_cell_x: -3,
        origin_cell_z: -4,
        radius_cells: 3,
        requested: BTreeSet::from(WorldgenInspectKindV1::CAVE_HYDROLOGY),
    }
}

#[allow(
    clippy::too_many_lines,
    reason = "the eight V6 inspect channels are listed explicitly as golden evidence"
)]
fn cave_hydrology_records(bindings: &FrozenBindings) -> Vec<WorldgenInspectRecordV1> {
    let (empty_role, _) = bindings.role("terrenia:block-role/empty@1");
    let water_predicate = bindings.predicate("terrenia:predicate/place-water@1");
    let limestone = stable("terrenia:cave-topology-domain/limestone");
    let crystal = stable("terrenia:cave-topology-domain/crystal");
    let default_domain = stable("terrenia:cave-topology-domain/dimension-default");

    vec![
        WorldgenInspectRecordV1::new(
            WorldgenInspectKindV1::Portal,
            stable("latticeaxiom:cave-portal/limestone-crystal@1"),
            -4,
            -3,
            WorldgenInspectBodyV1::Portal(PortalInspectFactsV1 {
                portal_hash: hash("portal-limestone-crystal"),
                first_domain: crystal.clone(),
                second_domain: limestone.clone(),
                neighbor_cell_x: 1,
                neighbor_cell_z: 0,
                position_millimeters: [-48_000, 8_000, -40_000],
                tangent_axis: CaveInspectAxisV1::Y,
                clearance_width_millimeters: 2_000,
                clearance_height_millimeters: 3_000,
                fluid: PortalInspectFluidV1::Drainage {
                    connection_id: stable("latticeaxiom:hydrology-link/limestone-crystal@1"),
                },
                dependency_receipt: hash("portal-receipt"),
            }),
        )
        .expect("portal record"),
        WorldgenInspectRecordV1::new(
            WorldgenInspectKindV1::Entrance,
            stable("latticeaxiom:cave-entrance/west-surface@1"),
            -5,
            -4,
            WorldgenInspectBodyV1::Entrance(EntranceInspectFactsV1 {
                voxel_x: -72,
                voxel_y: 18,
                voxel_z: -56,
                cell_count: 4,
                domains: vec![default_domain.clone(), limestone.clone()],
                portals: {
                    let mut portals = vec![hash("portal-a"), hash("portal-b")];
                    portals.sort();
                    portals
                },
                destination_domain: limestone.clone(),
                destination_cell_x: -2,
                destination_cell_z: -4,
                fluid: PortalInspectFluidV1::Dry,
                ready: true,
                dependency_receipt: hash("entrance-receipt"),
            }),
        )
        .expect("entrance record"),
        WorldgenInspectRecordV1::new(
            WorldgenInspectKindV1::CaveOwnership,
            limestone.clone(),
            -3,
            -4,
            WorldgenInspectBodyV1::CaveOwnership(CaveOwnershipInspectFactsV1 {
                winner: limestone.clone(),
                runner_up: default_domain,
                primary_owner: stable("terrenia:worldgen/cave-limestone@1"),
                secondary_owner: stable("terrenia:worldgen/cave-default@1"),
                channel: stable("latticeaxiom:generation-channel/cave-topology@1"),
                boundary_distance_voxels: 12,
                in_core: true,
                provenance: hash("cave-ownership-provenance"),
            }),
        )
        .expect("ownership record"),
        WorldgenInspectRecordV1::new(
            WorldgenInspectKindV1::CaveRejection,
            stable("terrenia:cave-contributor/side-passage@1"),
            -2,
            -5,
            WorldgenInspectBodyV1::CaveRejection(CaveRejectionInspectFactsV1 {
                contributor: stable("terrenia:cave-contributor/side-passage@1"),
                reason: CaveInspectRejectV1::BudgetExceeded,
                budget_consumed: 24,
                budget_limit: 32,
                sort_key: hash("rejection-sort"),
            }),
        )
        .expect("rejection record"),
        WorldgenInspectRecordV1::new(
            WorldgenInspectKindV1::CaveSdf,
            stable("latticeaxiom:cave-sdf/limestone-sample@1"),
            -3,
            -2,
            WorldgenInspectBodyV1::CaveSdf(CaveSdfInspectFactsV1 {
                voxel_x: -40,
                voxel_y: 6,
                voxel_z: -24,
                evaluations: 64,
                local_signed_distance: -2,
                branch_signed_distance: 4,
                portal_signed_distance: -1,
                raw_signed_distance: -2,
                finally_void: true,
                generation_time_micros: 250,
                memory_bytes: 1_024,
                branch_contributor: stable("terrenia:cave-contributor/side-passage@1"),
            }),
        )
        .expect("sdf record"),
        WorldgenInspectRecordV1::new(
            WorldgenInspectKindV1::CaveConnectivity,
            stable("latticeaxiom:cave-connectivity/west-path@1"),
            -4,
            -4,
            WorldgenInspectBodyV1::CaveConnectivity(CaveConnectivityInspectFactsV1 {
                graph_reachable: true,
                passable: true,
                loop_count: 1,
                dead_end_count: 2,
                must_connect_satisfied: true,
                clearance_intact: true,
                graph_samples: 48,
                passable_samples: 44,
                connected_domains: vec![crystal.clone(), limestone.clone()],
            }),
        )
        .expect("connectivity record"),
        WorldgenInspectRecordV1::new(
            WorldgenInspectKindV1::FluidDecision,
            stable("latticeaxiom:fluid-decision/limestone-aquifer@1"),
            -5,
            -2,
            WorldgenInspectBodyV1::FluidDecision(FluidDecisionInspectFactsV1 {
                occupancy: FluidOccupancyInspectV1::Water,
                role: empty_role,
                predicate: water_predicate,
                candidate: stable("terrenia:fluid/water"),
                aquifer: true,
                drainage_connection: Some(stable(
                    "latticeaxiom:hydrology-link/limestone-crystal@1",
                )),
                cave_finally_void: true,
                continuous_across_seam: true,
            }),
        )
        .expect("fluid record"),
        WorldgenInspectRecordV1::new(
            WorldgenInspectKindV1::PlanningSeam,
            stable("latticeaxiom:planning-seam/limestone-crystal@1"),
            -3,
            -1,
            WorldgenInspectBodyV1::PlanningSeam(PlanningSeamInspectFactsV1 {
                neighbor_cell_x: 1,
                neighbor_cell_z: 0,
                shared_face_match: true,
                required_cave_portals: {
                    let mut portals = vec![hash("seam-portal-a"), hash("seam-portal-b")];
                    portals.sort();
                    portals
                },
                fluid_discontinuity: false,
                seam_signature: hash("seam-signature"),
            }),
        )
        .expect("seam record"),
    ]
}

fn cave_hydrology_report() -> latticeaxiom_runtime_contracts::WorldgenInspectReportV1 {
    let bindings = FrozenBindings::load();
    let biomes = String::from_utf8(read(AUTHORED_BIOMES))
        .unwrap_or_else(|error| panic!("biome catalog utf-8: {error}"));
    assert!(
        biomes.contains("terrenia:biome/limestone-caverns"),
        "evidence must bind the limestone underground biome"
    );
    assert!(
        biomes.contains("terrenia:biome/crystal-depths"),
        "evidence must bind the crystal underground biome"
    );
    let layers = String::from_utf8(read(AUTHORED_LAYERS))
        .unwrap_or_else(|error| panic!("natural layers utf-8: {error}"));
    assert!(
        layers.contains("terrenia:cave-topology-domain/limestone"),
        "evidence must bind the limestone topology domain"
    );
    assert!(
        layers.contains("terrenia:cave-topology-domain/crystal"),
        "evidence must bind the crystal topology domain"
    );
    let mut shuffled = HashMap::new();
    for record in cave_hydrology_records(&bindings) {
        shuffled.insert(record.id.to_string(), record);
    }
    let samples = WorldgenInspectSamplesV1::new(shuffled.into_values())
        .unwrap_or_else(|error| panic!("samples: {error}"));
    let collection =
        WorldgenInspectCollectionV1::new(BTreeSet::from(WorldgenInspectKindV1::CAVE_HYDROLOGY));
    compile_worldgen_inspect_report(
        &cave_query(),
        &samples,
        &collection,
        WorldgenInspectLimits::default(),
    )
    .unwrap_or_else(|error| panic!("report: {error}"))
}

#[allow(
    clippy::too_many_lines,
    reason = "the surface-to-cave command path lists waypoints, resource classes, and commands explicitly"
)]
fn command_path(bindings: &FrozenBindings) -> CaveCommandPathFixtureV1 {
    let (wood_role, wood_block) = bindings.role("terrenia:block-role/woodland-log@1");
    let (stone_role, stone_block) = bindings.role("terrenia:block-role/temperate-base-rock@1");
    let (ore_role, ore_block) = bindings.role("terrenia:block-role/copper-resource@1");
    let vegetation_predicate = bindings.predicate("terrenia:predicate/place-vegetation@1");
    let solid_predicate = bindings.predicate("terrenia:predicate/place-solid@1");
    let limestone = stable("terrenia:cave-topology-domain/limestone");
    let crystal = stable("terrenia:cave-topology-domain/crystal");
    CaveCommandPathFixtureV1 {
        schema: cave_command_path_schema().unwrap_or_else(|error| panic!("schema: {error}")),
        schema_version: 1,
        generation_input_hash: hash("cave-evidence-input"),
        generation_provenance_hash: hash("cave-evidence-provenance"),
        origin_cell_x: -3,
        origin_cell_z: -4,
        entrance: stable("latticeaxiom:cave-waypoint/west-entrance@1"),
        waypoints: vec![
            CaveCommandWaypointV1 {
                id: stable("latticeaxiom:cave-waypoint/surface-spawn@1"),
                stage: CaveCommandStageV1::Surface,
                cell_x: -5,
                cell_z: -4,
                voxel_x: -72,
                voxel_y: 20,
                voxel_z: -56,
                territory: stable("terrenia:biome/temperate-woodland"),
            },
            CaveCommandWaypointV1 {
                id: stable("latticeaxiom:cave-waypoint/west-entrance@1"),
                stage: CaveCommandStageV1::Entrance,
                cell_x: -5,
                cell_z: -4,
                voxel_x: -72,
                voxel_y: 18,
                voxel_z: -56,
                territory: stable("terrenia:cave-topology-domain/dimension-default"),
            },
            CaveCommandWaypointV1 {
                id: stable("latticeaxiom:cave-waypoint/limestone@1"),
                stage: CaveCommandStageV1::Underground,
                cell_x: -4,
                cell_z: -3,
                voxel_x: -48,
                voxel_y: 8,
                voxel_z: -40,
                territory: limestone.clone(),
            },
            CaveCommandWaypointV1 {
                id: stable("latticeaxiom:cave-waypoint/crystal@1"),
                stage: CaveCommandStageV1::Underground,
                cell_x: -3,
                cell_z: -3,
                voxel_x: -32,
                voxel_y: 6,
                voxel_z: -40,
                territory: crystal.clone(),
            },
            CaveCommandWaypointV1 {
                id: stable("latticeaxiom:cave-waypoint/wood@1"),
                stage: CaveCommandStageV1::Resource,
                cell_x: -5,
                cell_z: -5,
                voxel_x: -70,
                voxel_y: 21,
                voxel_z: -68,
                territory: stable("terrenia:biome/temperate-woodland"),
            },
            CaveCommandWaypointV1 {
                id: stable("latticeaxiom:cave-waypoint/stone@1"),
                stage: CaveCommandStageV1::Resource,
                cell_x: -4,
                cell_z: -3,
                voxel_x: -46,
                voxel_y: 7,
                voxel_z: -38,
                territory: limestone,
            },
            CaveCommandWaypointV1 {
                id: stable("latticeaxiom:cave-waypoint/ore@1"),
                stage: CaveCommandStageV1::Resource,
                cell_x: -3,
                cell_z: -3,
                voxel_x: -30,
                voxel_y: 5,
                voxel_z: -38,
                territory: crystal,
            },
        ],
        underground_territories: vec![
            stable("terrenia:cave-topology-domain/crystal"),
            stable("terrenia:cave-topology-domain/limestone"),
        ],
        resource_classes: vec![
            CaveCommandResourceClassV1 {
                class: stable("terrenia:resource-class/ore@1"),
                role: ore_role,
                predicate: solid_predicate.clone(),
                candidate: ore_block.clone(),
                cell_x: -3,
                cell_z: -3,
                voxel_x: -30,
                voxel_y: 5,
                voxel_z: -38,
            },
            CaveCommandResourceClassV1 {
                class: stable("terrenia:resource-class/stone@1"),
                role: stone_role,
                predicate: solid_predicate,
                candidate: stone_block.clone(),
                cell_x: -4,
                cell_z: -3,
                voxel_x: -46,
                voxel_y: 7,
                voxel_z: -38,
            },
            CaveCommandResourceClassV1 {
                class: stable("terrenia:resource-class/wood@1"),
                role: wood_role,
                predicate: vegetation_predicate,
                candidate: wood_block.clone(),
                cell_x: -5,
                cell_z: -5,
                voxel_x: -70,
                voxel_y: 21,
                voxel_z: -68,
            },
        ],
        commands: vec![
            CaveCommandStepV1 {
                action: CaveCommandActionV1::Move,
                waypoint: stable("latticeaxiom:cave-waypoint/surface-spawn@1"),
                target: None,
            },
            CaveCommandStepV1 {
                action: CaveCommandActionV1::Move,
                waypoint: stable("latticeaxiom:cave-waypoint/wood@1"),
                target: None,
            },
            CaveCommandStepV1 {
                action: CaveCommandActionV1::BreakBlock,
                waypoint: stable("latticeaxiom:cave-waypoint/wood@1"),
                target: Some(wood_block),
            },
            CaveCommandStepV1 {
                action: CaveCommandActionV1::Move,
                waypoint: stable("latticeaxiom:cave-waypoint/west-entrance@1"),
                target: None,
            },
            CaveCommandStepV1 {
                action: CaveCommandActionV1::Inspect,
                waypoint: stable("latticeaxiom:cave-waypoint/west-entrance@1"),
                target: Some(stable("latticeaxiom:cave-entrance/west-surface@1")),
            },
            CaveCommandStepV1 {
                action: CaveCommandActionV1::Move,
                waypoint: stable("latticeaxiom:cave-waypoint/limestone@1"),
                target: None,
            },
            CaveCommandStepV1 {
                action: CaveCommandActionV1::BreakBlock,
                waypoint: stable("latticeaxiom:cave-waypoint/stone@1"),
                target: Some(stone_block),
            },
            CaveCommandStepV1 {
                action: CaveCommandActionV1::Move,
                waypoint: stable("latticeaxiom:cave-waypoint/crystal@1"),
                target: None,
            },
            CaveCommandStepV1 {
                action: CaveCommandActionV1::Look,
                waypoint: stable("latticeaxiom:cave-waypoint/ore@1"),
                target: Some(ore_block.clone()),
            },
            CaveCommandStepV1 {
                action: CaveCommandActionV1::BreakBlock,
                waypoint: stable("latticeaxiom:cave-waypoint/ore@1"),
                target: Some(ore_block),
            },
        ],
    }
}

fn assert_golden_bytes(path: &str, actual: &[u8]) {
    let expected = read(path);
    assert_eq!(actual, expected.as_slice(), "golden `{path}` drifted");
}

#[test]
fn cave_hydrology_negative_origin_report_matches_golden_bytes() {
    let report = cave_hydrology_report();
    assert_eq!(report.schema.to_string(), WORLDGEN_INSPECT_REPORT_SCHEMA_V1);
    assert_eq!(
        report.records.len(),
        WorldgenInspectKindV1::CAVE_HYDROLOGY.len()
    );
    assert!(report.query.origin_cell_x < 0);
    assert!(report.query.origin_cell_z < 0);
    assert!(report.skipped_kinds.is_empty());
    assert_eq!(report.truncated_records, 0);
    let kinds: Vec<WorldgenInspectKindV1> = report.records.iter().map(|row| row.kind).collect();
    assert_eq!(kinds, WorldgenInspectKindV1::CAVE_HYDROLOGY.to_vec());
    let actual = report
        .canonical_bytes()
        .unwrap_or_else(|error| panic!("canonical bytes: {error}"));
    assert_golden_bytes(GOLDEN_REPORT, &actual);
}

#[test]
fn hashmap_shuffled_cave_hydrology_samples_are_permutation_stable() {
    let first = cave_hydrology_report();
    let second = cave_hydrology_report();
    assert_eq!(
        first
            .canonical_hash()
            .unwrap_or_else(|error| panic!("first hash: {error}")),
        second
            .canonical_hash()
            .unwrap_or_else(|error| panic!("second hash: {error}"))
    );
}

#[test]
fn command_path_fixture_reaches_both_territories_and_three_resource_classes() {
    let bindings = FrozenBindings::load();
    let fixture = command_path(&bindings);
    fixture
        .validate()
        .unwrap_or_else(|error| panic!("command path: {error}"));
    assert_eq!(fixture.schema.to_string(), CAVE_COMMAND_PATH_SCHEMA_V1);
    assert_eq!(fixture.schema, schema(CAVE_COMMAND_PATH_SCHEMA_V1));
    assert!(fixture.origin_cell_x < 0);
    assert!(fixture.origin_cell_z < 0);
    assert_eq!(fixture.underground_territories.len(), 2);
    assert_eq!(fixture.resource_classes.len(), 3);
    assert_eq!(
        CaveCommandPathFixtureV1::inspect_kinds(),
        BTreeSet::from(WorldgenInspectKindV1::CAVE_HYDROLOGY)
    );
    let actual = fixture
        .canonical_bytes()
        .unwrap_or_else(|error| panic!("canonical bytes: {error}"));
    assert_golden_bytes(GOLDEN_COMMAND_PATH, &actual);
}

#[test]
fn hashmap_shuffled_command_path_is_permutation_stable() {
    let bindings = FrozenBindings::load();
    let first = command_path(&bindings);
    let mut reversed = first.clone();
    reversed.waypoints.reverse();
    reversed.resource_classes.reverse();
    reversed.commands.reverse();
    first
        .validate()
        .unwrap_or_else(|error| panic!("first: {error}"));
    reversed
        .validate()
        .expect_err("reversed resource classes must fail canonical order");
    let mut shuffled_commands = HashMap::new();
    for (index, step) in first.commands.iter().enumerate() {
        shuffled_commands.insert(index, step.clone());
    }
    let mut restored = first.clone();
    restored.commands = shuffled_commands.into_values().collect();
    restored.commands.sort_by(|left, right| {
        first
            .commands
            .iter()
            .position(|step| step == left)
            .cmp(&first.commands.iter().position(|step| step == right))
    });
    assert_eq!(
        first
            .canonical_hash()
            .unwrap_or_else(|error| panic!("first hash: {error}")),
        restored
            .canonical_hash()
            .unwrap_or_else(|error| panic!("restored hash: {error}"))
    );
}

#[test]
fn unsubscribed_cave_kinds_are_not_collected() {
    let bindings = FrozenBindings::load();
    let samples = WorldgenInspectSamplesV1::new(
        cave_hydrology_records(&bindings)
            .into_iter()
            .filter(|record| record.kind == WorldgenInspectKindV1::Portal),
    )
    .unwrap_or_else(|error| panic!("samples: {error}"));
    let collection =
        WorldgenInspectCollectionV1::new(BTreeSet::from([WorldgenInspectKindV1::Portal]));
    let report = compile_worldgen_inspect_report(
        &cave_query(),
        &samples,
        &collection,
        WorldgenInspectLimits::default(),
    )
    .unwrap_or_else(|error| panic!("report: {error}"));
    assert!(
        report
            .skipped_kinds
            .contains(&WorldgenInspectKindV1::Entrance)
    );
    assert_eq!(report.records.len(), 1);
    assert_eq!(report.records[0].kind, WorldgenInspectKindV1::Portal);
}

#[test]
fn zero_portal_clearance_fails_closed() {
    let error = WorldgenInspectRecordV1::new(
        WorldgenInspectKindV1::Portal,
        stable("latticeaxiom:cave-portal/invalid@1"),
        -3,
        -4,
        WorldgenInspectBodyV1::Portal(PortalInspectFactsV1 {
            portal_hash: hash("invalid-portal"),
            first_domain: stable("terrenia:cave-topology-domain/crystal"),
            second_domain: stable("terrenia:cave-topology-domain/limestone"),
            neighbor_cell_x: 1,
            neighbor_cell_z: 0,
            position_millimeters: [0, 0, 0],
            tangent_axis: CaveInspectAxisV1::Y,
            clearance_width_millimeters: 0,
            clearance_height_millimeters: 3_000,
            fluid: PortalInspectFluidV1::Dry,
            dependency_receipt: hash("invalid-receipt"),
        }),
    )
    .expect_err("zero clearance must fail closed");
    assert!(matches!(error, WorldgenInspectError::ZeroClearance { .. }));
}
