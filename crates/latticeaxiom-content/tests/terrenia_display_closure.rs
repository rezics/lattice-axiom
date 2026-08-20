//! Display closure for every authored Terrenia content identity.

use std::collections::BTreeSet;

use serde_json::Value;

const CATALOG_JSON: &str =
    include_str!("../../../packages/terrenia/blocks/data/authored-catalog-v1.json");
const DISPLAY_JSON: &str =
    include_str!("../../../packages/terrenia/blocks/data/authored-display-v1.json");

#[test]
fn every_block_and_fluid_has_one_display_row() {
    let catalog: Value = serde_json::from_str(CATALOG_JSON)
        .unwrap_or_else(|error| panic!("Terrenia authored catalog is invalid: {error}"));
    let display: Value = serde_json::from_str(DISPLAY_JSON)
        .unwrap_or_else(|error| panic!("Terrenia authored display data is invalid: {error}"));

    let expected = catalog["blocks"]
        .as_array()
        .unwrap_or_else(|| panic!("blocks is not an array"))
        .iter()
        .map(|row| {
            row["definition"]["header"]["stable_id"]
                .as_str()
                .unwrap_or_else(|| panic!("block ID is not text"))
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
                }),
        )
        .collect::<BTreeSet<_>>();
    let entries = display["entries"]
        .as_array()
        .unwrap_or_else(|| panic!("display entries is not an array"));
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
            row["content"]
                .as_str()
                .unwrap_or_else(|| panic!("display content ID is not text"))
        })
        .collect::<BTreeSet<_>>();

    assert_eq!(entries.len(), expected.len());
    assert_eq!(actual, expected);
}
