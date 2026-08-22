//! Deterministic worldgen inspect evidence bound to frozen Role/Predicate IDs.

#![allow(
    clippy::expect_used,
    reason = "integration fixtures state their construction invariants"
)]

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs;

use latticeaxiom_core::{CanonicalHash, StableId};
use latticeaxiom_runtime_contracts::{
    BoundaryInspectFactsV1, EngineEpoch, GeologyInspectFactsV1, ResourceInspectFactsV1,
    RiverInspectFactsV1, SpawnInspectFactsV1, SpawnInspectRejectV1, TerritoryInspectFactsV1,
    VegetationInspectFactsV1, WORLDGEN_INSPECT_REPORT_SCHEMA_V1, WorldEpoch, WorldgenInspectBodyV1,
    WorldgenInspectBoundsV1, WorldgenInspectCollectionV1, WorldgenInspectKindV1,
    WorldgenInspectLimits, WorldgenInspectQueryV1, WorldgenInspectRecordV1,
    WorldgenInspectSamplesV1, compile_worldgen_inspect_report,
};
use serde::Deserialize;

const AUTHORED_BINDINGS: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../packages/terrenia/worldgen/data/authored-block-bindings-v1.json"
);
const BIOME_CATALOG: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../latticeaxiom-content/fixtures/terrenia/representative-catalog-v1.json"
);
const GOLDEN_REPORT: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/worldgen-inspect/complete-negative-origin.json"
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

fn hash(label: &str) -> CanonicalHash {
    CanonicalHash::digest(label.as_bytes())
}

fn bounds() -> WorldgenInspectBoundsV1 {
    WorldgenInspectBoundsV1::new(-5, -5, -1, -1).unwrap_or_else(|error| panic!("bounds: {error}"))
}

fn complete_query() -> WorldgenInspectQueryV1 {
    WorldgenInspectQueryV1 {
        engine_epoch: EngineEpoch::new(7),
        world_epoch: Some(WorldEpoch::new(11)),
        generation_input_hash: hash("evidence-input"),
        generation_provenance_hash: hash("evidence-provenance"),
        origin_cell_x: -3,
        origin_cell_z: -4,
        radius_cells: 3,
        requested: BTreeSet::from(WorldgenInspectKindV1::NATURAL),
    }
}

#[allow(
    clippy::too_many_lines,
    reason = "the seven inspect channels are listed explicitly as golden evidence"
)]
fn complete_records(bindings: &FrozenBindings) -> Vec<WorldgenInspectRecordV1> {
    let (empty_role, _) = bindings.role("terrenia:block-role/empty@1");
    let (surface_role, _) = bindings.role("terrenia:block-role/temperate-surface@1");
    let (stratum_role, stratum_block) = bindings.role("terrenia:block-role/temperate-base-rock@1");
    let (resource_role, resource_block) = bindings.role("terrenia:block-role/copper-resource@1");
    let (vegetation_role, vegetation_block) =
        bindings.role("terrenia:block-role/woodland-leaves@1");
    let empty_predicate = bindings.predicate("terrenia:predicate/place-empty@1");
    let surface_predicate = bindings.predicate("terrenia:predicate/place-surface@1");
    let solid_predicate = bindings.predicate("terrenia:predicate/place-solid@1");
    let vegetation_predicate = bindings.predicate("terrenia:predicate/place-vegetation@1");
    let woodland = stable("terrenia:biome/temperate-woodland");
    let badlands = stable("terrenia:biome/arid-badlands");

    vec![
        WorldgenInspectRecordV1::new(
            WorldgenInspectKindV1::Spawn,
            stable("latticeaxiom:spawn/negative-footing@1"),
            -5,
            -4,
            WorldgenInspectBodyV1::Spawn(SpawnInspectFactsV1 {
                voxel_x: -80,
                voxel_y: 42,
                voxel_z: -64,
                territory: woodland.clone(),
                empty_role,
                surface_role,
                empty_predicate,
                surface_predicate,
                reject: Some(SpawnInspectRejectV1::InsufficientClearance),
                ready: true,
                receipt_hash: hash("spawn-receipt"),
            }),
        )
        .expect("spawn record"),
        WorldgenInspectRecordV1::new(
            WorldgenInspectKindV1::Resource,
            stable("terrenia:resource-field/copper-west@1"),
            -4,
            -5,
            WorldgenInspectBodyV1::Resource(ResourceInspectFactsV1 {
                field_id: stable("terrenia:resource-field/copper-west@1"),
                role: resource_role,
                predicate: solid_predicate,
                candidate: resource_block,
                samples: 16,
                accepts: 3,
                bounds: bounds(),
                dependency_receipt: hash("resource-receipt"),
            }),
        )
        .expect("resource record"),
        WorldgenInspectRecordV1::new(
            WorldgenInspectKindV1::Territory,
            stable("terrenia:territory-domain/temperate-woodland@1"),
            -3,
            -4,
            WorldgenInspectBodyV1::Territory(TerritoryInspectFactsV1 {
                winner: woodland,
                runner_up: badlands.clone(),
                boundary_distance_voxels: 6,
                primary_owner: stable("terrenia:worldgen/terrain-woodland@1"),
                secondary_owner: stable("terrenia:worldgen/terrain-badlands@1"),
                transition_provider: stable("terrenia:worldgen/transition-woodland-badlands@1"),
                transition_revision: 1,
                transition_width_voxels: 8,
                in_transition_band: true,
                adjacent: badlands,
                provenance: hash("territory-provenance"),
            }),
        )
        .expect("territory record"),
        WorldgenInspectRecordV1::new(
            WorldgenInspectKindV1::Vegetation,
            stable("terrenia:vegetation/oak-canopy@1"),
            -2,
            -3,
            WorldgenInspectBodyV1::Vegetation(VegetationInspectFactsV1 {
                procedure_id: stable("terrenia:vegetation/oak-canopy@1"),
                role: vegetation_role,
                predicate: vegetation_predicate,
                candidate: vegetation_block,
                exclusion_radius_voxels: 4,
                bounds: bounds(),
                dependency_receipt: hash("vegetation-receipt"),
            }),
        )
        .expect("vegetation record"),
        WorldgenInspectRecordV1::new(
            WorldgenInspectKindV1::River,
            stable("latticeaxiom:hydrology-basin/surface-west@1"),
            -5,
            -2,
            WorldgenInspectBodyV1::River(RiverInspectFactsV1 {
                plan_id: stable("latticeaxiom:hydrology-plan/overworld@1"),
                basin_id: stable("latticeaxiom:hydrology-basin/surface-west@1"),
                connection_id: Some(stable("latticeaxiom:hydrology-link/west-east@1")),
                bounds: bounds(),
                elevation_rank: 2,
                capacity_units: 128,
                dependency_receipt: hash("river-receipt"),
            }),
        )
        .expect("river record"),
        WorldgenInspectRecordV1::new(
            WorldgenInspectKindV1::Geology,
            stable("terrenia:geology/limestone-shelf@1"),
            -1,
            -5,
            WorldgenInspectBodyV1::Geology(GeologyInspectFactsV1 {
                body_id: stable("terrenia:geology/limestone-shelf@1"),
                bounds: bounds(),
                min_y: -24,
                max_y_exclusive: 8,
                stratum_role,
                stratum_block,
                dependency_receipt: hash("geology-receipt"),
            }),
        )
        .expect("geology record"),
        WorldgenInspectRecordV1::new(
            WorldgenInspectKindV1::Boundary,
            stable("latticeaxiom:transition-adapter/e0-e1@1"),
            -3,
            -1,
            WorldgenInspectBodyV1::Boundary(BoundaryInspectFactsV1 {
                boundary_hash: hash("boundary-id"),
                epoch_a: hash("epoch-a"),
                epoch_b: hash("epoch-b"),
                adapter_id: stable("latticeaxiom:transition-adapter/e0-e1@1"),
                adapter_version: 1,
                adapter_hash: hash("adapter-artifact"),
                transition_width: 8,
                terrain_boundary_signature: hash("terrain-signature"),
                required_cave_portals: {
                    let mut portals = vec![hash("portal-a"), hash("portal-b")];
                    portals.sort();
                    portals
                },
            }),
        )
        .expect("boundary record"),
    ]
}

fn complete_report() -> latticeaxiom_runtime_contracts::WorldgenInspectReportV1 {
    let bindings = FrozenBindings::load();
    let catalog = String::from_utf8(read(BIOME_CATALOG))
        .unwrap_or_else(|error| panic!("biome catalog utf-8: {error}"));
    assert!(
        catalog.contains("terrenia:biome/temperate-woodland"),
        "evidence must bind the ADR 0028 woodland identity"
    );
    assert!(
        catalog.contains("terrenia:biome/arid-badlands"),
        "evidence must bind the ADR 0028 badlands identity"
    );
    let mut shuffled = HashMap::new();
    for record in complete_records(&bindings) {
        shuffled.insert(record.id.to_string(), record);
    }
    let samples = WorldgenInspectSamplesV1::new(shuffled.into_values())
        .unwrap_or_else(|error| panic!("samples: {error}"));
    let collection =
        WorldgenInspectCollectionV1::new(BTreeSet::from(WorldgenInspectKindV1::NATURAL));
    compile_worldgen_inspect_report(
        &complete_query(),
        &samples,
        &collection,
        WorldgenInspectLimits::default(),
    )
    .unwrap_or_else(|error| panic!("report: {error}"))
}

#[test]
fn complete_negative_origin_report_matches_golden_bytes() {
    let report = complete_report();
    assert_eq!(report.schema.to_string(), WORLDGEN_INSPECT_REPORT_SCHEMA_V1);
    assert_eq!(report.records.len(), WorldgenInspectKindV1::NATURAL.len());
    assert!(report.query.origin_cell_x < 0);
    assert!(report.query.origin_cell_z < 0);
    assert!(report.skipped_kinds.is_empty());
    assert_eq!(report.truncated_records, 0);
    let kinds: Vec<WorldgenInspectKindV1> = report.records.iter().map(|row| row.kind).collect();
    assert_eq!(kinds, WorldgenInspectKindV1::NATURAL.to_vec());

    let actual = report
        .canonical_bytes()
        .unwrap_or_else(|error| panic!("canonical bytes: {error}"));
    let expected = read(GOLDEN_REPORT);
    assert_eq!(actual, expected, "worldgen inspect golden bytes drifted");
}

#[test]
fn hashmap_shuffled_complete_samples_are_permutation_stable() {
    let first = complete_report();
    let second = complete_report();
    assert_eq!(
        first
            .canonical_hash()
            .unwrap_or_else(|error| panic!("first hash: {error}")),
        second
            .canonical_hash()
            .unwrap_or_else(|error| panic!("second hash: {error}"))
    );
}
