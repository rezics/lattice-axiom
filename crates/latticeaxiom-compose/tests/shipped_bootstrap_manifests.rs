//! Shipped human-authored composition bootstraps and package source manifests.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use latticeaxiom_compose::{
    ArtifactIntent, AuthorizedRoot, AuthorizedRootKind, COMPOSITION_BOOTSTRAP_FILE_NAME,
    CapabilityCardinality, CompositionBootstrapV1, PACKAGE_SOURCE_MANIFEST_FILE_NAME,
    PackageDomain, PackageSourceManifestV1, RealizationKind, SourceScanLimits, TrustClass,
    scan_included_source_snapshot,
};
use latticeaxiom_core::{CapabilityId, PackageName, SourceId, StableId, canonical_json_bytes};
use serde::{Deserialize, Serialize};

const WORKSPACE_ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");
const PACKAGE_ENTRY: &str = "package.ncl";

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
enum PackagePurposeSchema {
    #[serde(rename = "latticeaxiom.package-purpose.v1")]
    V1,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum PackagePurposeKind {
    Capability,
    Dimension,
}

#[derive(Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct PackagePurposeDescriptorV1 {
    package: PackageName,
    purpose: StableId,
    purpose_kind: PackagePurposeKind,
    schema: PackagePurposeSchema,
}

#[test]
fn shipped_bootstrap_and_package_manifests_parse() {
    let workspace = Path::new(WORKSPACE_ROOT);
    let package_manifests = shipped_package_manifests(workspace);
    let bootstraps = shipped_bootstraps(workspace);

    assert_eq!(
        package_manifests.len(),
        21,
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
fn shipped_data_roots_select_at_least_one_included_file() {
    let workspace = Path::new(WORKSPACE_ROOT);

    for (index, manifest_path) in shipped_package_manifests(workspace).into_iter().enumerate() {
        let text = read_manifest(&manifest_path);
        let manifest = PackageSourceManifestV1::from_toml_str(&text).unwrap_or_else(|error| {
            panic!(
                "{} must parse as PackageSourceManifestV1: {error}",
                manifest_path.display()
            )
        });
        let package_root = manifest_path
            .parent()
            .unwrap_or_else(|| panic!("{} has no package root", manifest_path.display()));
        let source_id = format!("latticeaxiom:source/shipped-data-root-{index}")
            .parse::<SourceId>()
            .expect("the bounded shipped-package index forms a canonical source ID");
        let authorized = AuthorizedRoot::new(source_id, AuthorizedRootKind::Package, package_root)
            .unwrap_or_else(|error| {
                panic!(
                    "{} must be an authorized package root: {error}",
                    package_root.display()
                )
            });
        let included = scan_included_source_snapshot(
            &authorized,
            SourceScanLimits {
                maximum_files: 4_096,
                maximum_bytes: 4 * 1024 * 1024,
            },
            &manifest.source_inclusion,
        )
        .unwrap_or_else(|error| {
            panic!(
                "{} source inclusion must scan: {error}",
                manifest_path.display()
            )
        });

        for realization in manifest.realizations.values() {
            let ArtifactIntent::DataRoot { path: data_root } = &realization.artifact else {
                continue;
            };
            let descendant_prefix = format!("{data_root}/");
            assert!(
                included.files().keys().any(|logical_path| {
                    logical_path == data_root.as_str()
                        || logical_path.starts_with(&descendant_prefix)
                }),
                "{} realization {} declares data root {data_root}, but source inclusion selects no file under it",
                manifest.name,
                realization.id
            );
        }
    }
}

#[test]
fn shipped_package_purpose_descriptors_are_closed_and_canonical() {
    let workspace = Path::new(WORKSPACE_ROOT);
    let fixtures = [
        (
            "packages/latticeaxiom/dev-tools/data/package-purpose-v1.json",
            "@latticeaxiom/dev-tools",
            "latticeaxiom:capability/debug-workbench@1",
            PackagePurposeKind::Capability,
        ),
        (
            "packages/latticeaxiom/inspect/data/package-purpose-v1.json",
            "@latticeaxiom/inspect",
            "latticeaxiom:capability/target-inspect-surface@1",
            PackagePurposeKind::Capability,
        ),
        (
            "packages/latticeaxiom/observability/data/package-purpose-v1.json",
            "@latticeaxiom/observability",
            "latticeaxiom:capability/diagnostic-registry@1",
            PackagePurposeKind::Capability,
        ),
        (
            "packages/terrenia/main/data/package-purpose-v1.json",
            "terrenia",
            "terrenia:dimension/terrenia",
            PackagePurposeKind::Dimension,
        ),
    ];

    for (logical_path, package, purpose, purpose_kind) in fixtures {
        let path = workspace.join(logical_path);
        let bytes = fs::read(&path)
            .unwrap_or_else(|error| panic!("{} must be readable: {error}", path.display()));
        let descriptor = serde_json::from_slice::<PackagePurposeDescriptorV1>(&bytes)
            .unwrap_or_else(|error| {
                panic!("{} must match its closed schema: {error}", path.display())
            });

        assert_eq!(descriptor.schema, PackagePurposeSchema::V1);
        assert_eq!(descriptor.package.as_str(), package);
        assert_eq!(descriptor.purpose.as_str(), purpose);
        assert_eq!(descriptor.purpose_kind, purpose_kind);
        assert_eq!(
            canonical_json_bytes(&descriptor).unwrap_or_else(|error| panic!(
                "{} must encode canonically: {error}",
                path.display()
            )),
            bytes,
            "{} must use byte-canonical compact JSON",
            path.display()
        );
    }
}

#[test]
fn terrenia_source_manifests_bind_presentation_by_capability() {
    let workspace = Path::new(WORKSPACE_ROOT);
    let presentation_path = workspace
        .join("packages/terrenia/presentation")
        .join(PACKAGE_SOURCE_MANIFEST_FILE_NAME);
    let presentation = PackageSourceManifestV1::from_toml_str(&read_manifest(&presentation_path))
        .expect("Terrenia presentation source manifest parses");
    let root_path = workspace
        .join("packages/terrenia/main")
        .join(PACKAGE_SOURCE_MANIFEST_FILE_NAME);
    let root = PackageSourceManifestV1::from_toml_str(&read_manifest(&root_path))
        .expect("Terrenia root source manifest parses");
    let capability = "latticeaxiom:capability/content-presentation@1"
        .parse::<CapabilityId>()
        .expect("content-presentation capability is canonical");

    let provision = presentation
        .provides
        .get(&capability)
        .expect("Terrenia presentation provides content-presentation");
    assert_eq!(provision.version.to_string(), "1.0.0");
    assert_eq!(provision.cardinality, CapabilityCardinality::ExactlyOne);
    assert_eq!(provision.domains, BTreeSet::from([PackageDomain::Client]));

    let requirement = root
        .requires
        .get(&capability)
        .expect("Terrenia root requires content-presentation");
    assert_eq!(
        requirement.provider.as_ref().map(PackageName::as_str),
        Some("@terrenia/presentation")
    );
    assert_eq!(requirement.cardinality, CapabilityCardinality::ExactlyOne);
    assert_eq!(requirement.domains, BTreeSet::from([PackageDomain::Client]));
    assert!(requirement.version.matches(&provision.version));
}

#[test]
fn front_end_colocates_its_rust_source_contract() {
    let workspace = Path::new(WORKSPACE_ROOT);
    let package_root = workspace.join("packages/latticeaxiom/front-end");
    let manifest_path = package_root.join(PACKAGE_SOURCE_MANIFEST_FILE_NAME);
    let manifest = PackageSourceManifestV1::from_toml_str(&read_manifest(&manifest_path))
        .unwrap_or_else(|error| {
            panic!(
                "{} must parse as PackageSourceManifestV1: {error}",
                manifest_path.display()
            )
        });

    assert_eq!(manifest.name.as_str(), "@latticeaxiom/front-end");
    for expected in [
        "Cargo.toml",
        "README.md",
        "data",
        "latticeaxiom-package.toml",
        "package.ncl",
        "src",
        "tests",
    ] {
        assert!(
            manifest
                .source_inclusion
                .include
                .iter()
                .any(|path| path.as_str() == expected),
            "{expected} must participate in the front-end source receipt"
        );
        assert!(
            package_root.join(expected).exists(),
            "included front-end source {expected} must exist"
        );
    }

    let cargo_path = package_root.join("Cargo.toml");
    let cargo_text = read_manifest(&cargo_path);
    let cargo: toml::Value = toml::from_str(&cargo_text).unwrap_or_else(|error| {
        panic!("{} must parse as Cargo TOML: {error}", cargo_path.display())
    });
    assert_eq!(
        cargo
            .get("package")
            .and_then(|package| package.get("name"))
            .and_then(toml::Value::as_str),
        Some("latticeaxiom-start-ui"),
        "the Cargo crate name remains independent from the logical package name"
    );
    assert!(
        !cargo_text.contains("../../../crates"),
        "a package-local crate must not escape to platform crates by relative path"
    );
    for dependency in [
        "latticeaxiom-client-ui",
        "latticeaxiom-core",
        "latticeaxiom-launcher",
        "latticeaxiom-runtime-contracts",
        "latticeaxiom-world-catalog",
        "latticeaxiom-world-db",
    ] {
        assert!(
            cargo_text.contains(&format!("{dependency}.workspace = true")),
            "{dependency} must resolve through the receipt-injectable workspace contract"
        );
    }
    assert!(!workspace.join("crates/latticeaxiom-start-ui").exists());
}

#[test]
fn input_colocates_its_rust_and_data_source_contract() {
    let workspace = Path::new(WORKSPACE_ROOT);
    let package_root = workspace.join("packages/latticeaxiom/input");
    let manifest_path = package_root.join(PACKAGE_SOURCE_MANIFEST_FILE_NAME);
    let manifest = PackageSourceManifestV1::from_toml_str(&read_manifest(&manifest_path))
        .unwrap_or_else(|error| {
            panic!(
                "{} must parse as PackageSourceManifestV1: {error}",
                manifest_path.display()
            )
        });

    assert_eq!(manifest.name.as_str(), "@latticeaxiom/input");
    for expected in [
        "Cargo.toml",
        "README.md",
        "data",
        "latticeaxiom-package.toml",
        "package.ncl",
        "src",
        "tests",
    ] {
        assert!(
            manifest
                .source_inclusion
                .include
                .iter()
                .any(|path| path.as_str() == expected),
            "{expected} must participate in the input source receipt"
        );
        assert!(
            package_root.join(expected).exists(),
            "included input source {expected} must exist"
        );
    }

    let cargo_path = package_root.join("Cargo.toml");
    let cargo_text = read_manifest(&cargo_path);
    let cargo: toml::Value = toml::from_str(&cargo_text).unwrap_or_else(|error| {
        panic!("{} must parse as Cargo TOML: {error}", cargo_path.display())
    });
    assert_eq!(
        cargo
            .get("package")
            .and_then(|package| package.get("name"))
            .and_then(toml::Value::as_str),
        Some("latticeaxiom-input"),
        "the Cargo crate name remains independent from the logical package name"
    );
    assert!(
        cargo_text.contains("latticeaxiom-core.workspace = true"),
        "platform dependencies must resolve through the workspace contract"
    );
    assert!(!cargo_text.contains("../../../crates"));
    assert!(
        !workspace
            .join("crates/latticeaxiom-input/Cargo.toml")
            .exists()
    );
    assert!(!workspace.join("crates/latticeaxiom-input/src").exists());
}

#[test]
fn settings_ui_colocates_its_rust_and_data_source_contract() {
    let workspace = Path::new(WORKSPACE_ROOT);
    let package_root = workspace.join("packages/latticeaxiom/settings-ui");
    let manifest_path = package_root.join(PACKAGE_SOURCE_MANIFEST_FILE_NAME);
    let manifest = PackageSourceManifestV1::from_toml_str(&read_manifest(&manifest_path))
        .unwrap_or_else(|error| {
            panic!(
                "{} must parse as PackageSourceManifestV1: {error}",
                manifest_path.display()
            )
        });

    assert_eq!(manifest.name.as_str(), "@latticeaxiom/settings-ui");
    for expected in [
        "Cargo.toml",
        "README.md",
        "data",
        "latticeaxiom-package.toml",
        "package.ncl",
        "src",
        "tests",
    ] {
        assert!(
            manifest
                .source_inclusion
                .include
                .iter()
                .any(|path| path.as_str() == expected),
            "{expected} must participate in the settings-ui source receipt"
        );
        assert!(
            package_root.join(expected).exists(),
            "included settings-ui source {expected} must exist"
        );
    }

    let cargo_path = package_root.join("Cargo.toml");
    let cargo_text = read_manifest(&cargo_path);
    let cargo: toml::Value = toml::from_str(&cargo_text).unwrap_or_else(|error| {
        panic!("{} must parse as Cargo TOML: {error}", cargo_path.display())
    });
    assert_eq!(
        cargo
            .get("package")
            .and_then(|package| package.get("name"))
            .and_then(toml::Value::as_str),
        Some("latticeaxiom-settings-ui"),
        "the Cargo crate name remains independent from the logical package name"
    );
    assert!(
        !cargo_text.contains("../../../crates"),
        "a package-local crate must not escape to platform crates by relative path"
    );
    for dependency in [
        "latticeaxiom-client-ui",
        "latticeaxiom-core",
        "latticeaxiom-runtime-contracts",
    ] {
        assert!(
            cargo_text.contains(&format!("{dependency}.workspace = true")),
            "{dependency} must resolve through the receipt-injectable workspace contract"
        );
    }
    assert!(!workspace.join("crates/latticeaxiom-settings-ui").exists());
}

#[test]
fn dual_gameplay_colocates_code_but_ships_only_frozen_data() {
    let workspace = Path::new(WORKSPACE_ROOT);
    let package_root = workspace.join("packages/example/dual-gameplay");
    let manifest_path = package_root.join(PACKAGE_SOURCE_MANIFEST_FILE_NAME);
    let manifest = PackageSourceManifestV1::from_toml_str(&read_manifest(&manifest_path))
        .unwrap_or_else(|error| {
            panic!(
                "{} must parse as PackageSourceManifestV1: {error}",
                manifest_path.display()
            )
        });

    assert_eq!(manifest.name.as_str(), "@example/dual-gameplay");
    assert_eq!(manifest.trust, TrustClass::DataOnly);
    assert_eq!(
        manifest.realizations.len(),
        1,
        "the shipped fixture must not claim unrealizable native alternatives"
    );
    let realization = manifest
        .realizations
        .values()
        .next()
        .expect("the shipped fixture has exactly one realization");
    assert_eq!(realization.id.as_str(), "data");
    assert_eq!(realization.kind, RealizationKind::Data);
    assert_eq!(realization.trust, TrustClass::DataOnly);
    assert!(realization.targets.is_empty());
    assert!(realization.interfaces.is_empty());
    assert!(realization.required_features.is_empty());
    match &realization.artifact {
        ArtifactIntent::DataRoot { path } => assert_eq!(path.as_str(), "tests/fixtures"),
        ArtifactIntent::SourceBuild => panic!(
            "the shipped fixture cannot claim SourceBuild before lock activation can consume it"
        ),
        artifact @ ArtifactIntent::LocalPrebuilt { .. } => {
            panic!("the shipped fixture must use its frozen data root, got {artifact:?}")
        }
    }

    for expected in [
        "Cargo.toml",
        "README.md",
        "benches",
        "latticeaxiom-package.toml",
        "package.ncl",
        "src",
        "tests",
    ] {
        assert!(
            manifest
                .source_inclusion
                .include
                .iter()
                .any(|path| path.as_str() == expected),
            "{expected} must participate in the dual-gameplay source receipt"
        );
        assert!(
            package_root.join(expected).exists(),
            "included dual-gameplay source {expected} must exist"
        );
    }

    let cargo_path = package_root.join("Cargo.toml");
    let cargo_text = read_manifest(&cargo_path);
    let cargo: toml::Value = toml::from_str(&cargo_text).unwrap_or_else(|error| {
        panic!("{} must parse as Cargo TOML: {error}", cargo_path.display())
    });
    assert_eq!(
        cargo
            .get("package")
            .and_then(|package| package.get("name"))
            .and_then(toml::Value::as_str),
        Some("latticeaxiom-dual-fixture")
    );
    assert!(!cargo_text.contains("../../../crates"));
    assert!(!workspace.join("crates/latticeaxiom-dual-fixture").exists());
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
