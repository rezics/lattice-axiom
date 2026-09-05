//! Shipped dual-track package data is present and parseable without joining the lock.

#![allow(clippy::expect_used)]

use std::fs;
use std::path::PathBuf;

use serde_json::Value;

fn workspace_file(relative: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../../../..")
        .join(relative)
}

fn read_json(relative: &str) -> Value {
    let path = workspace_file(relative);
    let bytes = fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("{} must exist: {error}", path.display()));
    serde_json::from_str(&bytes)
        .unwrap_or_else(|error| panic!("{} must be JSON: {error}", path.display()))
}

#[test]
fn metallurgy_lists_copper_alloys_as_stable_ids() {
    let value = read_json("packages/terrenia/metallurgy/data/materials-v1.json");
    let ids: Vec<&str> = value["materials"]
        .as_array()
        .expect("materials array")
        .iter()
        .map(|row| row["id"].as_str().expect("material id"))
        .collect();
    assert_eq!(
        ids,
        [
            "terrenia:material/copper",
            "terrenia:material/annealed-copper",
            "terrenia:material/bronze",
            "terrenia:material/brass",
        ]
    );
}

#[test]
fn thaumaturgy_mana_is_not_voltage() {
    let value = read_json("packages/terrenia/thaumaturgy/data/mana-v1.json");
    assert_eq!(value["mana_property"].as_str(), Some("terrenia:state/mana"));
    assert_eq!(
        value["not_voltage_property"].as_str(),
        Some("terrenia:state/voltage")
    );
}

#[test]
fn relations_kinds_are_platform_ids() {
    let value = read_json("packages/latticeaxiom/relations/data/kinds-v1.json");
    let ids: Vec<&str> = value["kinds"]
        .as_array()
        .expect("kinds array")
        .iter()
        .map(|row| row["id"].as_str().expect("kind id"))
        .collect();
    assert_eq!(
        ids,
        [
            "latticeaxiom:relation/pet",
            "latticeaxiom:relation/friend",
            "latticeaxiom:relation/subject",
            "latticeaxiom:relation/companion",
        ]
    );
}

#[test]
fn progress_graph_fails_cycles_at_compile_and_starts_empty() {
    let value = read_json("packages/latticeaxiom/progress/data/graph-v1.json");
    assert_eq!(value["cycle_policy"].as_str(), Some("compile-fail"));
    assert_eq!(value["chapters"].as_array().map(Vec::len), Some(0));
}

#[test]
fn dual_track_manifests_exist() {
    for relative in [
        "packages/terrenia/metallurgy/latticeaxiom-package.toml",
        "packages/terrenia/science/latticeaxiom-package.toml",
        "packages/terrenia/thaumaturgy/latticeaxiom-package.toml",
        "packages/terrenia/journey/latticeaxiom-package.toml",
        "packages/latticeaxiom/progress/latticeaxiom-package.toml",
        "packages/latticeaxiom/relations/latticeaxiom-package.toml",
    ] {
        assert!(
            workspace_file(relative).is_file(),
            "{relative} must be a shipped package manifest"
        );
    }
}
