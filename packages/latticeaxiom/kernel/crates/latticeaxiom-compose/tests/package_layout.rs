//! Verify actual shipped package source manifests and their internal Rust entries.

use std::{collections::BTreeSet, fs, path::Path};

use latticeaxiom_compose::PackageSourceManifestV1;

#[test]
fn workspace_members_have_valid_package_source_ownership() {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../../..");
    let text = fs::read_to_string(workspace.join("Cargo.toml")).expect("workspace manifest");
    let cargo: toml::Value = toml::from_str(&text).expect("workspace TOML");
    let mut owners = BTreeSet::new();
    for member in cargo["workspace"]["members"].as_array().expect("members") {
        let member = member.as_str().expect("member path");
        let crate_path = workspace.join(member);
        let package_root = crate_path
            .parent()
            .expect("crates folder")
            .parent()
            .expect("package root");
        let manifest = fs::read_to_string(package_root.join("latticeaxiom-package.toml"))
            .expect("package source manifest");
        let package = PackageSourceManifestV1::from_toml_str(&manifest)
            .unwrap_or_else(|error| panic!("{member}: {error}"));
        let rust = package.rust.expect("code package declares Rust sources");
        assert!(package_root.join(rust.entry.as_str()).is_file());
        for entry in &rust.members {
            assert!(package_root.join(entry.as_str()).is_file());
        }
        owners.insert(package.name);
    }
    assert!(
        owners.len() > 1,
        "workspace must exercise distinct package owners"
    );
}
