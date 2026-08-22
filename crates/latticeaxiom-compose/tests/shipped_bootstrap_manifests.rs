//! Shipped human-authored composition bootstraps and package source manifests.

use std::fs;
use std::path::{Path, PathBuf};

use latticeaxiom_compose::{
    COMPOSITION_BOOTSTRAP_FILE_NAME, CompositionBootstrapV1, PACKAGE_SOURCE_MANIFEST_FILE_NAME,
    PackageSourceManifestV1,
};

const WORKSPACE_ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");
const PACKAGE_ENTRY: &str = "package.ncl";

#[test]
fn shipped_bootstrap_and_package_manifests_parse() {
    let workspace = Path::new(WORKSPACE_ROOT);
    let package_manifests = shipped_package_manifests(workspace);
    let bootstraps = shipped_bootstraps(workspace);

    assert_eq!(
        package_manifests.len(),
        20,
        "every shipped package.ncl must have a sibling {PACKAGE_SOURCE_MANIFEST_FILE_NAME}"
    );
    assert!(
        !bootstraps.is_empty(),
        "expected at least one {COMPOSITION_BOOTSTRAP_FILE_NAME} or profiles/*.toml"
    );

    for path in package_manifests {
        let text = read_manifest(&path);
        PackageSourceManifestV1::from_toml_str(&text).unwrap_or_else(|error| {
            panic!(
                "{} must parse as PackageSourceManifestV1: {error}",
                path.display()
            )
        });
    }

    for path in bootstraps {
        let text = read_manifest(&path);
        CompositionBootstrapV1::from_toml_str(&text).unwrap_or_else(|error| {
            panic!(
                "{} must parse as CompositionBootstrapV1: {error}",
                path.display()
            )
        });
    }
}

#[test]
fn terrenia_tools_package_source_manifest_parses() {
    let path = Path::new(WORKSPACE_ROOT)
        .join("packages/terrenia/tools")
        .join(PACKAGE_SOURCE_MANIFEST_FILE_NAME);
    let text = read_manifest(&path);
    let manifest = PackageSourceManifestV1::from_toml_str(&text).unwrap_or_else(|error| {
        panic!(
            "{} must parse as PackageSourceManifestV1: {error}",
            path.display()
        )
    });
    assert_eq!(manifest.name.as_str(), "@terrenia/tools");
    assert_eq!(manifest.version.to_string(), "0.1.0");
    assert!(
        manifest
            .dependencies
            .values()
            .any(|dependency| dependency.package.as_str() == "@terrenia/blocks"),
        "@terrenia/tools must depend on Terrenia materials/items"
    );
}

fn shipped_package_manifests(workspace: &Path) -> Vec<PathBuf> {
    let mut entries = Vec::new();
    collect_named_files(&workspace.join("packages"), PACKAGE_ENTRY, &mut entries);
    entries.sort();
    entries
        .into_iter()
        .map(|package_ncl| {
            let manifest = package_ncl
                .parent()
                .unwrap_or_else(|| panic!("{} has no parent directory", package_ncl.display()))
                .join(PACKAGE_SOURCE_MANIFEST_FILE_NAME);
            assert!(
                manifest.is_file(),
                "{} is missing next to {}",
                manifest.display(),
                package_ncl.display()
            );
            manifest
        })
        .collect()
}

fn shipped_bootstraps(workspace: &Path) -> Vec<PathBuf> {
    let mut bootstraps = Vec::new();
    let root = workspace.join(COMPOSITION_BOOTSTRAP_FILE_NAME);
    assert!(
        root.is_file(),
        "workspace root is missing {}",
        root.display()
    );
    bootstraps.push(root);

    let profiles = workspace.join("profiles");
    for entry in fs::read_dir(&profiles).unwrap_or_else(|error| {
        panic!(
            "profiles directory {} is readable: {error}",
            profiles.display()
        )
    }) {
        let entry = entry
            .unwrap_or_else(|error| panic!("profiles directory entries are readable: {error}"));
        let path = entry.path();
        if path
            .extension()
            .is_some_and(|extension| extension == "toml")
            && path.is_file()
        {
            bootstraps.push(path);
        }
    }

    bootstraps.sort();
    bootstraps.dedup();
    bootstraps
}

fn collect_named_files(root: &Path, file_name: &str, out: &mut Vec<PathBuf>) {
    let entries = fs::read_dir(root)
        .unwrap_or_else(|error| panic!("directory {} is readable: {error}", root.display()));
    for entry in entries {
        let entry = entry.unwrap_or_else(|error| {
            panic!(
                "directory entries under {} are readable: {error}",
                root.display()
            )
        });
        let path = entry.path();
        let name = entry.file_name();
        if name == ".git" || name == "target" {
            continue;
        }
        let file_type = entry
            .file_type()
            .unwrap_or_else(|error| panic!("file type of {} is readable: {error}", path.display()));
        if file_type.is_dir() {
            collect_named_files(&path, file_name, out);
        } else if name == file_name {
            out.push(path);
        }
    }
}

fn read_manifest(path: &Path) -> String {
    fs::read_to_string(path)
        .unwrap_or_else(|error| panic!("{} must be readable: {error}", path.display()))
}
