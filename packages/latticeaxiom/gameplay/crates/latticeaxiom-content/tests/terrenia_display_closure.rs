//! Display closure for every authored Terrenia content identity.

use std::collections::BTreeSet;

use serde_json::Value;

const CATALOG_JSON: &str =
    include_str!("../../../../../terrenia/blocks/data/authored-catalog-v1.json");
const BLOCK_DISPLAY_JSON: &str =
    include_str!("../../../../../terrenia/blocks/data/authored-display-v1.json");
const TOOL_CATALOG_JSON: &str =
    include_str!("../../../../../terrenia/tools/data/authored-tools-v1.json");
const TOOL_DISPLAY_JSON: &str =
    include_str!("../../../../../terrenia/tools/data/authored-display-v1.json");
const PRESENTATION_DISPLAY_JSON: &str =
    include_str!("../../../../../terrenia/presentation/data/authored-display-v1.json");
const PRESENTATION_ASSETS_JSON: &str =
    include_str!("../../../../../terrenia/presentation/data/authored-assets-v1.json");

#[test]
fn every_block_and_fluid_has_one_display_row() {
    assert_display_closure(BLOCK_DISPLAY_JSON, &locked_blocks_and_fluids(), false);
}

#[test]
fn every_tool_has_one_display_row() {
    assert_display_closure(TOOL_DISPLAY_JSON, &locked_tools(), false);
}

#[test]
fn every_locked_identity_has_a_presentation_name_and_icon() {
    let locked = locked_blocks_and_fluids()
        .into_iter()
        .chain(locked_tools())
        .collect::<BTreeSet<_>>();
    let icons = assert_display_closure(PRESENTATION_DISPLAY_JSON, &locked, true);
    let assets: Value = serde_json::from_str(PRESENTATION_ASSETS_JSON)
        .unwrap_or_else(|error| panic!("Terrenia presentation assets are invalid: {error}"));
    let asset_ids = assets["assets"]
        .as_array()
        .unwrap_or_else(|| panic!("presentation assets is not an array"))
        .iter()
        .map(|row| {
            row["id"]
                .as_str()
                .unwrap_or_else(|| panic!("presentation asset ID is not text"))
                .to_owned()
        })
        .collect::<BTreeSet<_>>();
    let missing = icons
        .into_iter()
        .filter(|icon| !asset_ids.contains(icon))
        .collect::<Vec<_>>();
    assert!(
        missing.is_empty(),
        "presentation icons missing from assets: {missing:?}"
    );
}

fn locked_blocks_and_fluids() -> BTreeSet<String> {
    let catalog: Value = serde_json::from_str(CATALOG_JSON)
        .unwrap_or_else(|error| panic!("Terrenia authored catalog is invalid: {error}"));
    catalog["blocks"]
        .as_array()
        .unwrap_or_else(|| panic!("blocks is not an array"))
        .iter()
        .map(|row| {
            row["definition"]["header"]["stable_id"]
                .as_str()
                .unwrap_or_else(|| panic!("block ID is not text"))
                .to_owned()
        })
        .chain(
            catalog["fluids"]
                .as_array()
                .unwrap_or_else(|| panic!("fluids is not an array"))
                .iter()
                .map(|row| {
                    row["definition"]["header"]["stable_id"]
                        .as_str()
                        .unwrap_or_else(|| panic!("fluid ID is not text"))
                        .to_owned()
                }),
        )
        .collect()
}

fn locked_tools() -> BTreeSet<String> {
    let tools: Value = serde_json::from_str(TOOL_CATALOG_JSON)
        .unwrap_or_else(|error| panic!("Terrenia authored tools are invalid: {error}"));
    tools["items"]
        .as_array()
        .unwrap_or_else(|| panic!("tools items is not an array"))
        .iter()
        .map(|row| {
            row["id"]
                .as_str()
                .unwrap_or_else(|| panic!("tool item ID is not text"))
                .to_owned()
        })
        .collect()
}

fn assert_display_closure(
    source: &str,
    expected: &BTreeSet<String>,
    require_icon: bool,
) -> BTreeSet<String> {
    let display: Value = serde_json::from_str(source)
        .unwrap_or_else(|error| panic!("Terrenia authored display data is invalid: {error}"));
    let entries = display["entries"]
        .as_array()
        .unwrap_or_else(|| panic!("display entries is not an array"));
    let mut icons = BTreeSet::new();
    let actual = entries
        .iter()
        .map(|row| {
            assert!(
                !row["display_key"].as_str().unwrap_or_default().is_empty(),
                "display key is empty"
            );
            assert!(
                !row["fallback"].as_str().unwrap_or_default().is_empty(),
                "display fallback is empty"
            );
            if require_icon {
                let icon = row["icon"]
                    .as_str()
                    .unwrap_or_else(|| panic!("display icon is not text"));
                assert!(!icon.is_empty(), "display icon is empty");
                assert!(
                    icons.insert(icon.to_owned()),
                    "display icon is reused: {icon}"
                );
            }
            row["content"]
                .as_str()
                .unwrap_or_else(|| panic!("display content ID is not text"))
                .to_owned()
        })
        .collect::<BTreeSet<_>>();

    assert_eq!(entries.len(), expected.len());
    assert_eq!(&actual, expected);
    icons
}
