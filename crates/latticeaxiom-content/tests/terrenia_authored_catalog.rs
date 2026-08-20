//! Contract tests for the owner-authored Terrenia package data.

use std::collections::BTreeSet;

use latticeaxiom_content::{ContentCatalogInputV1, ContentCatalogLimitsV1, ContentCatalogV1};
use serde_json::{Value, json};

const CATALOG_JSON: &str =
    include_str!("../../../packages/terrenia/blocks/data/authored-catalog-v1.json");
const SEMANTIC_JSON: &str =
    include_str!("../../../packages/terrenia/blocks/data/semantic-registry-v1.json");
const GAMEPLAY_JSON: &str =
    include_str!("../../../packages/terrenia/gameplay/data/authored-rules-v1.json");
const WORLDGEN_JSON: &str =
    include_str!("../../../packages/terrenia/worldgen/data/authored-block-bindings-v1.json");
const PRESENTATION_JSON: &str =
    include_str!("../../../packages/terrenia/presentation/data/authored-assets-v1.json");
const D3_IDS: &str =
    include_str!("../../../packages/terrenia/blocks/data/goldens/d3-block-ids.txt");
const D4_IDS: &str =
    include_str!("../../../packages/terrenia/blocks/data/goldens/d4-block-ids.txt");
const D7_IDS: &str =
    include_str!("../../../packages/terrenia/blocks/data/goldens/d7-block-ids.txt");
const D9_IDS: &str =
    include_str!("../../../packages/terrenia/blocks/data/goldens/d9-block-ids.txt");
const FLUID_IDS: &str =
    include_str!("../../../packages/terrenia/blocks/data/goldens/fluid-ids.txt");

fn parse_fixture(source: &str, context: &str) -> Value {
    serde_json::from_str(source)
        .unwrap_or_else(|error| panic!("{context} is not valid JSON: {error}"))
}

fn array<'a>(value: &'a Value, key: &str) -> &'a [Value] {
    value[key]
        .as_array()
        .unwrap_or_else(|| panic!("fixture field '{key}' is not an array"))
}

fn text<'a>(value: &'a Value, key: &str) -> &'a str {
    value[key]
        .as_str()
        .unwrap_or_else(|| panic!("fixture field '{key}' is not a string"))
}

fn golden_ids(source: &str) -> BTreeSet<String> {
    source
        .lines()
        .filter(|line| !line.is_empty())
        .map(str::to_owned)
        .collect()
}

fn authored_input() -> ContentCatalogInputV1 {
    let authored = parse_fixture(CATALOG_JSON, "Terrenia authored catalog");
    let blocks = array(&authored, "blocks")
        .iter()
        .map(|row| row["definition"].clone())
        .collect::<Vec<_>>();
    let fluids = array(&authored, "fluids")
        .iter()
        .map(|row| row["definition"].clone())
        .collect::<Vec<_>>();
    serde_json::from_value(json!({
        "schema_major": 1,
        "blocks": blocks,
        "fluids": fluids,
        "biomes": [],
        "material_role_bindings": authored["material_role_bindings"].clone()
    }))
    .unwrap_or_else(|error| panic!("Terrenia compiler-shaped definitions are invalid: {error}"))
}

fn compile(input: ContentCatalogInputV1) -> ContentCatalogV1 {
    ContentCatalogV1::compile(input, ContentCatalogLimitsV1::default())
        .unwrap_or_else(|error| panic!("Terrenia authored catalog did not compile: {error}"))
}

fn ids_named(rows: &[Value], key: &str) -> BTreeSet<String> {
    rows.iter().map(|row| text(row, key).to_owned()).collect()
}

fn string_array(value: &Value, key: &str) -> BTreeSet<String> {
    array(value, key)
        .iter()
        .map(|entry| {
            entry
                .as_str()
                .unwrap_or_else(|| panic!("'{key}' member is not text"))
                .to_owned()
        })
        .collect()
}

#[test]
fn staged_id_goldens_are_exact_and_monotonic() {
    let authored = parse_fixture(CATALOG_JSON, "Terrenia authored catalog");
    let stages = [
        ("D3", D3_IDS),
        ("D4", D4_IDS),
        ("D7", D7_IDS),
        ("D9", D9_IDS),
    ];
    let stage_rank = |stage: &str| match stage {
        "D3" => 0,
        "D4" => 1,
        "D7" => 2,
        "D9" => 3,
        other => panic!("unknown authored stage '{other}'"),
    };
    let mut previous = BTreeSet::new();
    for (stage, golden) in stages {
        let expected = golden_ids(golden);
        let actual = array(&authored, "blocks")
            .iter()
            .filter(|row| stage_rank(text(row, "stage")) <= stage_rank(stage))
            .map(|row| text(&row["definition"]["header"], "stable_id").to_owned())
            .collect::<BTreeSet<_>>();
        assert_eq!(actual, expected, "{stage} exact block inventory changed");
        assert!(previous.is_subset(&actual), "{stage} removed an earlier ID");
        previous = actual;
    }
    assert_eq!(golden_ids(D3_IDS).len(), 6);
    assert_eq!(golden_ids(D4_IDS).len(), 18);
    assert_eq!(golden_ids(D7_IDS).len(), 40);
    assert_eq!(golden_ids(D9_IDS).len(), 72);
    assert_eq!(
        golden_ids(D4_IDS).difference(&golden_ids(D3_IDS)).count(),
        12
    );
    assert_eq!(
        golden_ids(D7_IDS).difference(&golden_ids(D4_IDS)).count(),
        22
    );
    assert_eq!(
        golden_ids(D9_IDS).difference(&golden_ids(D7_IDS)).count(),
        32
    );
    let fluids = array(&authored, "fluids")
        .iter()
        .map(|row| text(&row["definition"]["header"], "stable_id").to_owned())
        .collect::<BTreeSet<_>>();
    assert_eq!(fluids, golden_ids(FLUID_IDS));
}

#[test]
fn full_catalog_compiles_with_finite_state_palettes_not_variant_ids() {
    let catalog = compile(authored_input());
    assert_eq!(catalog.blocks().len(), 72);
    assert_eq!(catalog.fluids().len(), 2);
    assert_eq!(catalog.material_role_bindings().len(), 16);
    for (id, expected_states) in [
        ("terrenia:block/oak-log", 3),
        ("terrenia:block/tall-grass", 6),
        ("terrenia:block/oak-planks", 4),
        ("terrenia:block/torch", 8),
        ("terrenia:block/furnace", 8),
        ("terrenia:block/chest", 4),
    ] {
        let block = catalog
            .blocks()
            .iter()
            .find(|block| block.definition().header.stable_id.as_str() == id)
            .unwrap_or_else(|| panic!("stateful definition '{id}' is missing"));
        assert_eq!(block.states().len(), expected_states, "{id}");
    }
    let exact_ids = catalog
        .blocks()
        .iter()
        .map(|block| block.definition().header.stable_id.as_str().to_owned())
        .collect::<BTreeSet<_>>();
    assert_eq!(exact_ids, golden_ids(D9_IDS));
}

#[test]
fn every_d9_golden_id_is_present_in_compiled_content_catalog() {
    let catalog = compile(authored_input());
    let compiled = catalog
        .blocks()
        .iter()
        .map(|block| block.definition().header.stable_id.as_str().to_owned())
        .collect::<BTreeSet<_>>();
    let expected = golden_ids(D9_IDS);
    let missing = expected
        .iter()
        .filter(|id| !compiled.contains(*id))
        .cloned()
        .collect::<Vec<_>>();
    assert!(
        missing.is_empty(),
        "compiled catalog is missing golden IDs: {missing:?}"
    );
    assert_eq!(expected.len(), 72);
    assert_eq!(compiled, expected);
}

#[test]
fn discovery_permutations_and_headless_projection_preserve_authoritative_hash() {
    let input = authored_input();
    let expected = compile(input.clone());
    let expected_hash = expected
        .canonical_authoritative_hash()
        .unwrap_or_else(|error| panic!("authoritative hash failed: {error}"));
    let expected_bytes = expected
        .canonical_authoritative_bytes()
        .unwrap_or_else(|error| panic!("authoritative bytes failed: {error}"));

    let mut permuted = input.clone();
    permuted.blocks.reverse();
    permuted.fluids.reverse();
    permuted.material_role_bindings.reverse();
    for (index, block) in permuted.blocks.iter_mut().enumerate() {
        block.state_schema.reverse();
        if index % 2 == 0 {
            block.states.reverse();
        } else if block.states.len() > 1 {
            block.states.rotate_left(1);
        }
    }
    let permuted = compile(permuted);
    assert_eq!(
        permuted
            .canonical_authoritative_hash()
            .unwrap_or_else(|error| panic!("permuted hash failed: {error}")),
        expected_hash
    );
    assert_eq!(
        permuted
            .canonical_authoritative_bytes()
            .unwrap_or_else(|error| panic!("permuted bytes failed: {error}")),
        expected_bytes
    );

    let mut headless = input;
    for block in &mut headless.blocks {
        block.presentation_binding = None;
    }
    for fluid in &mut headless.fluids {
        fluid.presentation_binding = None;
    }
    assert_eq!(
        compile(headless)
            .canonical_authoritative_hash()
            .unwrap_or_else(|error| panic!("headless hash failed: {error}")),
        expected_hash
    );
}

#[test]
#[allow(
    clippy::too_many_lines,
    reason = "the conformance test audits the complete cross-package reference closure"
)]
fn every_authored_cross_package_reference_is_closed() {
    let catalog = parse_fixture(CATALOG_JSON, "Terrenia authored catalog");
    let semantic = parse_fixture(SEMANTIC_JSON, "Terrenia semantic registry");
    let gameplay = parse_fixture(GAMEPLAY_JSON, "Terrenia gameplay rules");
    let worldgen = parse_fixture(WORLDGEN_JSON, "Terrenia worldgen bindings");
    let presentation = parse_fixture(PRESENTATION_JSON, "Terrenia presentation assets");

    let block_ids = array(&catalog, "blocks")
        .iter()
        .map(|row| text(&row["definition"]["header"], "stable_id").to_owned())
        .collect::<BTreeSet<_>>();
    let policies = string_array(&catalog["semantic_registry"], "policy_ids");
    let state_schemas = string_array(&catalog["semantic_registry"], "state_schema_ids");
    let semantic_tags = string_array(&catalog["semantic_registry"], "tag_ids");
    let semantic_maps = string_array(&catalog["semantic_registry"], "map_ids");
    let physical_profiles = string_array(&semantic, "physical_profile_ids");
    let fluid_policies = string_array(&semantic, "fluid_policy_ids");
    let mining_rules = ids_named(array(&gameplay, "mining_rules"), "id");
    let tool_requirements = ids_named(array(&gameplay, "tool_requirements"), "id");
    let drop_tables = ids_named(array(&gameplay, "drop_tables"), "id");
    let items = ids_named(array(&gameplay, "items"), "id");
    let recipes = ids_named(array(&gameplay, "recipes"), "id");
    let roles = ids_named(array(&worldgen, "roles"), "id");
    let predicates = ids_named(array(&worldgen, "predicates"), "id");
    let provenance = ids_named(array(&worldgen, "provenance"), "id");
    let assets = ids_named(array(&presentation, "assets"), "id");

    for row in array(&catalog, "blocks") {
        let definition = &row["definition"];
        let block = text(&definition["header"], "stable_id");
        assert!(
            physical_profiles.contains(text(&row["physical"], "profile")),
            "{block}"
        );
        assert!(
            tool_requirements.contains(text(row, "tool_requirement")),
            "{block}"
        );
        assert!(
            mining_rules.contains(text(&definition["rules"], "mining_rule")),
            "{block}"
        );
        assert!(
            drop_tables.contains(text(&definition["rules"], "drop_table")),
            "{block}"
        );
        if let Some(item) = definition["rules"]["placement_item"].as_str() {
            assert!(items.contains(item), "{block}");
        }
        for recipe in array(row, "recipe_refs") {
            assert!(
                recipes.contains(
                    recipe
                        .as_str()
                        .unwrap_or_else(|| panic!("recipe ref is not text"))
                ),
                "{block}"
            );
        }
        for tag in array(&row["semantic"], "tags") {
            assert!(
                semantic_tags.contains(
                    tag.as_str()
                        .unwrap_or_else(|| panic!("tag ref is not text"))
                ),
                "{block}"
            );
        }
        for map in row["semantic"]["map_values"]
            .as_object()
            .unwrap_or_else(|| panic!("{block} map_values is not an object"))
            .keys()
        {
            assert!(semantic_maps.contains(map), "{block}");
        }
        for role in array(&row["worldgen"], "role_candidates") {
            assert!(
                roles.contains(
                    role.as_str()
                        .unwrap_or_else(|| panic!("Role ref is not text"))
                ),
                "{block}"
            );
        }
        for predicate in array(&row["worldgen"], "placement_predicates") {
            assert!(
                predicates.contains(
                    predicate
                        .as_str()
                        .unwrap_or_else(|| panic!("Predicate ref is not text"))
                ),
                "{block}"
            );
        }
        assert!(
            provenance.contains(text(&row["worldgen"], "provenance")),
            "{block}"
        );
        assert!(assets.contains(text(row, "presentation_asset")), "{block}");
        assert_eq!(
            definition["presentation_binding"].as_str(),
            row["presentation_asset"].as_str(),
            "{block}"
        );
        for property in array(definition, "state_schema") {
            assert!(state_schemas.contains(text(property, "key")), "{block}");
        }
        for state in array(definition, "states") {
            for key in [
                "solid_occupancy",
                "collision",
                "selection",
                "occlusion",
                "replaceability",
                "fluid_occupancy",
            ] {
                assert!(policies.contains(text(state, key)), "{block} {key}");
            }
        }
    }

    for binding in array(&catalog, "material_role_bindings") {
        assert!(roles.contains(text(binding, "role")));
        assert!(block_ids.contains(text(binding, "block")));
    }
    for fluid in array(&catalog, "fluids") {
        let definition = &fluid["definition"];
        let id = text(&definition["header"], "stable_id");
        assert!(
            predicates.contains(text(definition, "replace_or_displace_predicate")),
            "{id}"
        );
        assert!(
            fluid_policies.contains(text(definition, "collision_policy")),
            "{id}"
        );
        assert!(
            fluid_policies.contains(text(definition, "selection_policy")),
            "{id}"
        );
        assert!(assets.contains(text(fluid, "presentation_asset")), "{id}");
        for predicate in array(&fluid["worldgen"], "placement_predicates") {
            assert!(
                predicates.contains(
                    predicate
                        .as_str()
                        .unwrap_or_else(|| panic!("fluid predicate is not text"))
                ),
                "{id}"
            );
        }
        assert!(
            provenance.contains(text(&fluid["worldgen"], "provenance")),
            "{id}"
        );
    }
    for drop_table in array(&gameplay, "drop_tables") {
        for output in array(drop_table, "outputs") {
            assert!(items.contains(text(output, "item")));
        }
    }
    for item in array(&gameplay, "items") {
        assert!(block_ids.contains(text(item, "placement_block")));
    }
    for recipe in array(&gameplay, "recipes") {
        for input in array(recipe, "inputs") {
            assert!(items.contains(text(input, "item")));
        }
        assert!(items.contains(text(&recipe["output"], "item")));
    }
}

#[test]
fn sampled_break_drop_place_paths_and_required_recipes_round_trip() {
    let catalog = parse_fixture(CATALOG_JSON, "Terrenia authored catalog");
    let gameplay = parse_fixture(GAMEPLAY_JSON, "Terrenia gameplay rules");
    let block_ids = array(&catalog, "blocks")
        .iter()
        .map(|row| text(&row["definition"]["header"], "stable_id").to_owned())
        .collect::<BTreeSet<_>>();
    for name in [
        "grass",
        "obsidian",
        "copper-ore",
        "oak-log",
        "moss",
        "crystal-cluster",
        "pine-leaves",
        "marble",
        "roof-tiles",
        "torch",
        "furnace",
        "chest",
    ] {
        let drop_id = format!("terrenia:drop-table/{name}@1");
        let table = array(&gameplay, "drop_tables")
            .iter()
            .find(|row| text(row, "id") == drop_id)
            .unwrap_or_else(|| panic!("sample '{name}' has no drop table"));
        let dropped_item = text(
            array(table, "outputs")
                .first()
                .unwrap_or_else(|| panic!("sample '{name}' drops nothing")),
            "item",
        );
        let item = array(&gameplay, "items")
            .iter()
            .find(|row| text(row, "id") == dropped_item)
            .unwrap_or_else(|| panic!("sample '{name}' drop item is undefined"));
        assert!(
            block_ids.contains(text(item, "placement_block")),
            "sample '{name}' cannot be placed into a known definition"
        );
    }

    for output in [
        "oak-planks",
        "pine-planks",
        "stone-bricks",
        "mossy-stone-bricks",
        "bricks",
        "polished-stone",
        "polished-granite",
        "polished-slate",
        "basalt-tiles",
        "glass",
        "copper-grating",
        "roof-tiles",
        "torch",
        "workbench",
        "furnace",
        "chest",
    ] {
        let item = format!("terrenia:item/{output}");
        assert!(
            array(&gameplay, "recipes")
                .iter()
                .any(|recipe| text(&recipe["output"], "item") == item),
            "required recipe output '{item}' is missing"
        );
    }
}
