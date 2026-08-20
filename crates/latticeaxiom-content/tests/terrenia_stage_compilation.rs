//! Stage-specific `RegistrationImage` inputs for the authored Terrenia catalog.

use std::collections::BTreeSet;

use latticeaxiom_content::{ContentCatalogInputV1, ContentCatalogLimitsV1, ContentCatalogV1};
use serde_json::{Value, json};

const CATALOG_JSON: &str =
    include_str!("../../../packages/terrenia/blocks/data/authored-catalog-v1.json");
const D3_IDS: &str =
    include_str!("../../../packages/terrenia/blocks/data/goldens/d3-block-ids.txt");
const D4_IDS: &str =
    include_str!("../../../packages/terrenia/blocks/data/goldens/d4-block-ids.txt");
const D7_IDS: &str =
    include_str!("../../../packages/terrenia/blocks/data/goldens/d7-block-ids.txt");
const D9_IDS: &str =
    include_str!("../../../packages/terrenia/blocks/data/goldens/d9-block-ids.txt");

fn authored() -> Value {
    serde_json::from_str(CATALOG_JSON)
        .unwrap_or_else(|error| panic!("Terrenia authored catalog is invalid: {error}"))
}

fn golden(source: &str) -> BTreeSet<&str> {
    source.lines().filter(|line| !line.is_empty()).collect()
}

fn input_for(golden_ids: &BTreeSet<&str>, include_fluids: bool) -> ContentCatalogInputV1 {
    let authored = authored();
    let blocks = authored["blocks"]
        .as_array()
        .unwrap_or_else(|| panic!("blocks is not an array"))
        .iter()
        .filter(|row| {
            row["definition"]["header"]["stable_id"]
                .as_str()
                .is_some_and(|id| golden_ids.contains(id))
        })
        .map(|row| row["definition"].clone())
        .collect::<Vec<_>>();
    let block_ids = blocks
        .iter()
        .map(|block| {
            block["header"]["stable_id"]
                .as_str()
                .unwrap_or_else(|| panic!("block stable ID is not text"))
        })
        .collect::<BTreeSet<_>>();
    let bindings = authored["material_role_bindings"]
        .as_array()
        .unwrap_or_else(|| panic!("material_role_bindings is not an array"))
        .iter()
        .filter(|binding| {
            binding["block"]
                .as_str()
                .is_some_and(|block| block_ids.contains(block))
        })
        .cloned()
        .collect::<Vec<_>>();
    let fluids = if include_fluids {
        authored["fluids"]
            .as_array()
            .unwrap_or_else(|| panic!("fluids is not an array"))
            .iter()
            .map(|row| row["definition"].clone())
            .collect()
    } else {
        Vec::new()
    };
    serde_json::from_value(json!({
        "schema_major": 1,
        "blocks": blocks,
        "fluids": fluids,
        "biomes": [],
        "material_role_bindings": bindings
    }))
    .unwrap_or_else(|error| panic!("stage compiler input is invalid: {error}"))
}

#[test]
fn every_stage_compiles_to_a_discovery_order_independent_image() {
    for (stage, ids, include_fluids) in [
        ("D3", D3_IDS, false),
        ("D4", D4_IDS, false),
        ("D7", D7_IDS, false),
        ("D9", D9_IDS, true),
    ] {
        let ids = golden(ids);
        let input = input_for(&ids, include_fluids);
        let expected = ContentCatalogV1::compile(input.clone(), ContentCatalogLimitsV1::default())
            .unwrap_or_else(|error| panic!("{stage} catalog failed compilation: {error}"));
        let mut reversed = input;
        reversed.blocks.reverse();
        reversed.fluids.reverse();
        reversed.material_role_bindings.reverse();
        for block in &mut reversed.blocks {
            block.state_schema.reverse();
            block.states.reverse();
        }
        let reversed = ContentCatalogV1::compile(reversed, ContentCatalogLimitsV1::default())
            .unwrap_or_else(|error| panic!("{stage} reversed catalog failed: {error}"));
        assert_eq!(expected.blocks().len(), ids.len(), "{stage}");
        assert_eq!(
            expected
                .canonical_authoritative_hash()
                .unwrap_or_else(|error| panic!("{stage} hash failed: {error}")),
            reversed
                .canonical_authoritative_hash()
                .unwrap_or_else(|error| panic!("{stage} reversed hash failed: {error}")),
            "{stage}"
        );
    }
}
