//! Package-authored Terrenia presentation asset closure.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value;

const PNG_MAGIC: &[u8] = b"\x89PNG\r\n\x1a\n";

const CATALOG_JSON: &str =
    include_str!("../../../../../terrenia/blocks/data/authored-catalog-v1.json");
const TOOL_CATALOG_JSON: &str =
    include_str!("../../../../../terrenia/tools/data/authored-tools-v1.json");
const PRESENTATION_DISPLAY_JSON: &str =
    include_str!("../../../../../terrenia/presentation/data/authored-display-v1.json");
const PRESENTATION_ASSETS_JSON: &str =
    include_str!("../../../../../terrenia/presentation/data/authored-assets-v1.json");
const PRESENTATION_LAYERS_JSON: &str =
    include_str!("../../../../../terrenia/presentation/data/authored-layers-v1.json");
const INVALID_ASSETS_JSON: &str =
    include_str!("../../../../../../fixtures/presentation/invalid-assets-v1.json");

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../../..")
}

fn presentation_data_root() -> PathBuf {
    workspace_root().join("packages/terrenia/presentation/data")
}

fn fixture_root() -> PathBuf {
    workspace_root().join("fixtures/presentation")
}

fn parse(source: &str, context: &str) -> Value {
    serde_json::from_str(source)
        .unwrap_or_else(|error| panic!("{context} is not valid JSON: {error}"))
}

fn array<'a>(value: &'a Value, key: &str) -> &'a [Value] {
    value[key]
        .as_array()
        .unwrap_or_else(|| panic!("'{key}' is not an array"))
}

fn text<'a>(value: &'a Value, key: &str) -> &'a str {
    value[key]
        .as_str()
        .unwrap_or_else(|| panic!("'{key}' is not text"))
}

fn locked_blocks_and_fluids() -> BTreeSet<String> {
    let catalog = parse(CATALOG_JSON, "Terrenia authored catalog");
    array(&catalog, "blocks")
        .iter()
        .map(|row| text(&row["definition"]["header"], "stable_id").to_owned())
        .chain(
            array(&catalog, "fluids")
                .iter()
                .map(|row| text(&row["definition"]["header"], "stable_id").to_owned()),
        )
        .collect()
}

fn locked_tools() -> BTreeSet<String> {
    let tools = parse(TOOL_CATALOG_JSON, "Terrenia authored tools");
    array(&tools, "items")
        .iter()
        .map(|row| text(row, "id").to_owned())
        .collect()
}

fn is_valid_png(path: &Path) -> bool {
    let Ok(bytes) = fs::read(path) else {
        return false;
    };
    bytes.len() > PNG_MAGIC.len() && bytes.starts_with(PNG_MAGIC)
}

fn resolve_source(root: &Path, asset: &Value) -> PathBuf {
    let source = root.join(text(asset, "source"));
    if is_valid_png(&source) {
        source
    } else {
        root.join(text(asset, "fallback"))
    }
}

#[test]
fn release_catalog_replaces_explicit_placeholders_with_package_authored_sources() {
    let assets = parse(PRESENTATION_ASSETS_JSON, "Terrenia presentation assets");
    assert_eq!(text(&assets, "owner_package"), "@terrenia/presentation");
    assert_eq!(assets["authoritative"], false);
    assert_eq!(assets["headless_removable"], true);
    assert_eq!(text(&assets, "missing_source_policy"), "stable-fallback");
    assert_eq!(text(&assets, "visual_qa"), "not-claimed");
    assert_eq!(
        array(&assets, "material_groups")
            .iter()
            .map(|row| {
                row.as_str()
                    .unwrap_or_else(|| panic!("material group is not text"))
            })
            .collect::<Vec<_>>(),
        ["opaque", "cutout", "translucent", "emissive"]
    );
    assert_eq!(
        array(&assets, "face_map_kinds")
            .iter()
            .map(|row| {
                row.as_str()
                    .unwrap_or_else(|| panic!("face map kind is not text"))
            })
            .collect::<Vec<_>>(),
        ["uniform", "cube-column", "six-face"]
    );
    assert_eq!(text(&assets["sampler"], "filter"), "nearest");
    assert_eq!(text(&assets["sampler"], "address"), "clamp-to-edge");
    assert_eq!(text(&assets["sampler"], "mipmaps"), "none");
    assert_eq!(text(&assets["sampler"], "color-space"), "srgb");

    let placeholders = array(&assets, "assets")
        .iter()
        .filter(|row| text(row, "status") == "explicit-placeholder")
        .map(|row| text(row, "id").to_owned())
        .collect::<Vec<_>>();
    assert!(
        placeholders.is_empty(),
        "release presentation still has explicit placeholders: {placeholders:?}"
    );
    assert!(
        array(&assets, "assets")
            .iter()
            .all(|row| text(row, "status") == "package-authored")
    );
}

#[test]
fn every_locked_block_fluid_and_tool_has_existing_package_authored_assets() {
    let assets = parse(PRESENTATION_ASSETS_JSON, "Terrenia presentation assets");
    let root = presentation_data_root();
    let locked = locked_blocks_and_fluids()
        .into_iter()
        .chain(locked_tools())
        .collect::<BTreeSet<_>>();
    let mut material_sets = BTreeSet::new();
    let mut icons = BTreeSet::new();
    let mut ids = BTreeSet::new();

    for row in array(&assets, "assets") {
        let id = text(row, "id");
        assert!(
            ids.insert(id.to_owned()),
            "duplicate presentation asset {id}"
        );
        assert_eq!(text(row, "status"), "package-authored");
        let resolved = resolve_source(&root, row);
        assert!(
            is_valid_png(&resolved),
            "asset {id} did not resolve to a PNG at {}",
            resolved.display()
        );
        assert!(
            is_valid_png(&root.join(text(row, "fallback"))),
            "asset {id} fallback is missing or invalid"
        );
        match text(row, "asset_kind") {
            "block-material-set" | "fluid-material-set" => {
                let content = text(row, "content");
                assert!(
                    material_sets.insert(content.to_owned()),
                    "duplicate material set for {content}"
                );
                assert!(
                    matches!(
                        text(row, "material_group"),
                        "opaque" | "cutout" | "translucent" | "emissive"
                    ),
                    "{content} material group"
                );
                assert!(
                    matches!(
                        text(row, "face_policy"),
                        "uniform" | "cube-column" | "six-face"
                    ),
                    "{content} face policy"
                );
            }
            "content-icon" => {
                let content = text(row, "content");
                assert!(
                    icons.insert(content.to_owned()),
                    "duplicate icon for {content}"
                );
            }
            "texture-layer" => {
                assert!(
                    matches!(
                        text(row, "face"),
                        "uniform"
                            | "top"
                            | "side"
                            | "bottom"
                            | "east"
                            | "west"
                            | "up"
                            | "down"
                            | "south"
                            | "north"
                    ),
                    "{id} face role"
                );
            }
            other => panic!("unknown asset kind {other} for {id}"),
        }
    }

    let blocks_and_fluids = locked_blocks_and_fluids();
    assert_eq!(material_sets, blocks_and_fluids);
    assert_eq!(icons, locked);
    assert_eq!(blocks_and_fluids.len(), 74);
    assert_eq!(locked_tools().len(), 7);
}

#[test]
fn layer_face_policies_close_against_available_assets_and_stay_non_authoritative() {
    let layers = parse(PRESENTATION_LAYERS_JSON, "Terrenia authored layers");
    let assets = parse(PRESENTATION_ASSETS_JSON, "Terrenia presentation assets");
    assert!(!layers["authoritative"].as_bool().unwrap_or(true));
    assert_eq!(layers["sampler"], assets["sampler"]);

    let available = array(&assets, "assets")
        .iter()
        .filter(|row| {
            matches!(
                text(row, "asset_kind"),
                "block-material-set" | "fluid-material-set" | "texture-layer"
            )
        })
        .map(|row| text(row, "id").to_owned())
        .collect::<BTreeSet<_>>();
    let material_group = array(&assets, "assets")
        .iter()
        .filter(|row| {
            matches!(
                text(row, "asset_kind"),
                "block-material-set" | "fluid-material-set"
            )
        })
        .map(|row| (text(row, "content").to_owned(), text(row, "material_group")))
        .collect::<BTreeMap<_, _>>();

    let mut contents = BTreeSet::new();
    for row in array(&layers, "layers") {
        let content = text(row, "content");
        assert!(
            contents.insert(content.to_owned()),
            "duplicate layer row {content}"
        );
        assert_eq!(
            text(row, "policy"),
            *material_group
                .get(content)
                .unwrap_or_else(|| panic!("{content} is missing a material group"))
        );
        let faces = &row["faces"];
        let referenced = match text(faces, "kind") {
            "uniform" => vec![text(faces, "layer")],
            "cube-column" => vec![
                text(faces, "top"),
                text(faces, "side"),
                text(faces, "bottom"),
            ],
            "six-face" => vec![
                text(faces, "east"),
                text(faces, "west"),
                text(faces, "up"),
                text(faces, "down"),
                text(faces, "south"),
                text(faces, "north"),
            ],
            other => panic!("{content} has unknown face kind {other}"),
        };
        for layer in referenced {
            assert!(
                available.contains(layer),
                "{content} references unknown layer {layer}"
            );
        }
    }
    assert_eq!(contents, locked_blocks_and_fluids());
}

#[test]
fn missing_and_invalid_sources_use_the_same_deterministic_fallback() {
    let fixture = parse(INVALID_ASSETS_JSON, "invalid presentation fixture");
    assert_eq!(text(&fixture, "missing_source_policy"), "stable-fallback");
    assert_eq!(fixture["headless_removable"], true);
    assert_eq!(text(&fixture, "visual_qa"), "not-claimed");

    let root = fixture_root();
    let fallback = root.join("assets/fallback/terrain-layer.png");
    assert!(
        is_valid_png(&fallback),
        "fixture fallback PNG is missing or invalid"
    );
    let expected = fs::read(&fallback).unwrap_or_else(|error| panic!("fallback read: {error}"));

    let mut resolved = Vec::new();
    for row in array(&fixture, "assets") {
        let source = root.join(text(row, "source"));
        assert!(
            !is_valid_png(&source),
            "{} unexpectedly resolved as a valid PNG",
            text(row, "id")
        );
        let path = resolve_source(&root, row);
        assert_eq!(path, fallback);
        let bytes = fs::read(&path).unwrap_or_else(|error| panic!("resolved read: {error}"));
        resolved.push(bytes);
    }
    assert_eq!(resolved.len(), 2);
    assert_eq!(resolved[0], expected);
    assert_eq!(resolved[1], expected);
}

#[test]
fn display_and_assets_declare_headless_removability_without_visual_qa() {
    let display = parse(PRESENTATION_DISPLAY_JSON, "Terrenia presentation display");
    let assets = parse(PRESENTATION_ASSETS_JSON, "Terrenia presentation assets");
    assert_eq!(display["headless_removable"], true);
    assert_eq!(display["authoritative"], false);
    assert_eq!(assets["headless_removable"], true);
    assert_eq!(assets["authoritative"], false);
    assert_ne!(text(&assets, "visual_qa"), "passed");
}
