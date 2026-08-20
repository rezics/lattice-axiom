//! Conformance for shipped D0 authoring roots through the verified source table.

#![cfg(feature = "nickel-evaluator")]
#![allow(
    clippy::expect_used,
    reason = "integration fixtures state their construction invariants"
)]

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use latticeaxiom_compose::{
    AuthorizedRoot, AuthorizedRootKind, CapabilityCardinality, CompositionError, GameProfileSpec,
    NICKEL_LIBRARY_CONTRACT_MAJOR, NickelEvaluationLimits, PACKAGE_MODEL_VERSION, PackageAlias,
    PackageDomain, PackageSpec, ProfileKind, R0_AUTHORING_CORPUS_MAJOR, R0_LIBRARY_PACKAGE_ALIAS,
    SourceAddress, SourceClosureError, SourceClosureRequest, SourceRootGrant, SourceScanLimits,
    SourceSnapshot, TrustedStagedEvaluation, TrustedStagedEvaluationError,
    evaluate_trusted_staged_nickel_function, evaluate_trusted_staged_package, scan_source_snapshot,
};
use latticeaxiom_core::{
    CanonicalHash, NamespaceGrantPattern, PackageName, SourceId, SourceProvenance,
    canonical_json_bytes, canonical_json_hash,
};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

const WORKSPACE_ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");
const LIBRARY_SOURCE_ID: &str = "latticeaxiom:source/library-v3";
const PROFILE_SOURCE_ID: &str = "latticeaxiom:source/shipped-profiles";
const FIXTURE_PREFIX: &str = "latticeaxiom-shipped-source-fixture";
static FIXTURE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Copy)]
struct PackageSource {
    logical_dir: &'static str,
    source_id: &'static str,
    package_name: &'static str,
}

const PACKAGE_SOURCES: [PackageSource; 13] = [
    PackageSource {
        logical_dir: "packages/latticeaxiom/settings",
        source_id: "latticeaxiom:source/settings",
        package_name: "@latticeaxiom/settings",
    },
    PackageSource {
        logical_dir: "packages/latticeaxiom/settings-ui",
        source_id: "latticeaxiom:source/settings-ui",
        package_name: "@latticeaxiom/settings-ui",
    },
    PackageSource {
        logical_dir: "packages/latticeaxiom/observability",
        source_id: "latticeaxiom:source/observability",
        package_name: "@latticeaxiom/observability",
    },
    PackageSource {
        logical_dir: "packages/latticeaxiom/inspect",
        source_id: "latticeaxiom:source/inspect",
        package_name: "@latticeaxiom/inspect",
    },
    PackageSource {
        logical_dir: "packages/latticeaxiom/dev-tools",
        source_id: "latticeaxiom:source/dev-tools",
        package_name: "@latticeaxiom/dev-tools",
    },
    PackageSource {
        logical_dir: "packages/latticeaxiom/front-end",
        source_id: "latticeaxiom:source/front-end",
        package_name: "@latticeaxiom/front-end",
    },
    PackageSource {
        logical_dir: "packages/latticeaxiom/world-library",
        source_id: "latticeaxiom:source/world-library",
        package_name: "@latticeaxiom/world-library",
    },
    PackageSource {
        logical_dir: "packages/terrenia/main",
        source_id: "terrenia:source/main",
        package_name: "terrenia",
    },
    PackageSource {
        logical_dir: "packages/terrenia/blocks",
        source_id: "terrenia:source/blocks",
        package_name: "@terrenia/blocks",
    },
    PackageSource {
        logical_dir: "packages/terrenia/worldgen",
        source_id: "terrenia:source/worldgen",
        package_name: "@terrenia/worldgen",
    },
    PackageSource {
        logical_dir: "packages/terrenia/gameplay",
        source_id: "terrenia:source/gameplay",
        package_name: "@terrenia/gameplay",
    },
    PackageSource {
        logical_dir: "packages/terrenia/tools",
        source_id: "terrenia:source/tools",
        package_name: "@terrenia/tools",
    },
    PackageSource {
        logical_dir: "packages/terrenia/presentation",
        source_id: "terrenia:source/presentation",
        package_name: "@terrenia/presentation",
    },
];

const PROFILE_SOURCES: [(&str, ProfileKind); 4] = [
    ("shell.ncl", ProfileKind::ClientShell),
    ("dev.ncl", ProfileKind::ClientWorld),
    ("headless.ncl", ProfileKind::DedicatedServer),
    ("test.ncl", ProfileKind::HeadlessTest),
];

#[test]
fn shipped_packages_use_verified_alias_closures_and_raw_entry_provenance() {
    let library = library_snapshot();
    let package_snapshots = package_snapshots();
    assert_non_overlapping_package_roots();

    let mut evaluated = BTreeMap::new();
    for source in PACKAGE_SOURCES {
        let expected_name = package_name(source.package_name);
        let snapshot = package_snapshots
            .get(&expected_name)
            .expect("every shipped package has a snapshot");
        let result = evaluate_package(snapshot, &library);
        let package = result.value;
        package
            .validate()
            .unwrap_or_else(|error| panic!("{} is invalid: {error}", package.name));

        assert_eq!(package.model_version, PACKAGE_MODEL_VERSION);
        assert_eq!(package.name, expected_name);
        assert_eq!(package.version.to_string(), "0.1.0");
        assert_eq!(package.trust, latticeaxiom_compose::TrustClass::DataOnly);
        assert_eq!(
            result.staged_addresses.len(),
            result.source_closure.sources.len()
        );
        assert!(
            result
                .source_closure
                .sources
                .iter()
                .any(|member| member.address.source_id() == library.source_id()),
            "{} did not close over the versioned library alias",
            package.name
        );
        assert!(!result.staging_path.exists());

        let entry_provenance = entry_provenance(snapshot, "package.ncl");
        assert_eq!(package.provenance, entry_provenance);
        assert_ne!(
            package.provenance.content_hash(),
            snapshot.source_hash(),
            "{} raw package.ncl and whole-root receipts were conflated",
            package.name
        );
        for registration in &package.registration.registrations {
            assert_eq!(registration.declared_by, package.name);
            assert_eq!(registration.provenance, package.provenance);
        }
        let normalized_fragment_hash = canonical_json_hash(&package.registration)
            .expect("registration fragment has a canonical Rust representation");
        for realization in package.realizations.values() {
            assert_eq!(
                realization.registration_fragment, normalized_fragment_hash,
                "{} Nickel registration-fragment receipt drifted from Rust canonical JSON",
                package.name
            );
        }
        assert!(
            package
                .requires
                .values()
                .all(|row| row.cardinality == CapabilityCardinality::ExactlyOne)
        );
        assert!(
            package
                .provides
                .values()
                .all(|row| row.cardinality == CapabilityCardinality::ExactlyOne)
        );
        assert!(evaluated.insert(package.name.clone(), package).is_none());
    }

    validate_shipped_package_delegations(&evaluated);

    let settings_name = package_name("@latticeaxiom/settings");
    let settings_snapshot = package_snapshots
        .get(&settings_name)
        .expect("settings is shipped");
    let first = evaluate_package(settings_snapshot, &library);
    let second = evaluate_package(settings_snapshot, &library);
    assert_ne!(first.staging_path, second.staging_path);
    assert_eq!(first.source_closure, second.source_closure);
    assert_eq!(
        canonical_json_bytes(&first.value).expect("settings output is canonical"),
        canonical_json_bytes(&second.value).expect("settings output is canonical")
    );
}

#[test]
fn shipped_profiles_bind_whole_root_and_raw_file_receipts_separately() {
    let library = library_snapshot();
    let profiles = scan_workspace_root(PROFILE_SOURCE_ID, AuthorizedRootKind::Test, "profiles");
    let snapshots = package_snapshots();
    let packages = evaluated_packages(&snapshots, &library);
    let binder = profile_receipt_binder(&snapshots);
    let mut authoritative_by_profile = BTreeMap::new();

    for (entry, expected_projection) in PROFILE_SOURCES {
        let result = evaluate_profile(&profiles, entry, &library, &binder);
        let profile = result.value;
        profile
            .validate()
            .unwrap_or_else(|error| panic!("profile {entry} is invalid: {error}"));
        assert_eq!(profile.projection, expected_projection);
        assert!(!result.staging_path.exists());
        assert_eq!(
            result.staged_addresses.len(),
            result.source_closure.sources.len()
        );

        let mut authoritative = BTreeSet::new();
        for candidate in &profile.source_universe {
            let package = packages.get(&candidate.package).unwrap_or_else(|| {
                panic!(
                    "profile {entry} references unknown package {}",
                    candidate.package
                )
            });
            let snapshot = snapshots
                .get(&candidate.package)
                .expect("evaluated package and snapshot maps share keys");
            let raw_entry = entry_provenance(snapshot, "package.ncl");
            assert_eq!(candidate.source_id, *snapshot.source_id());
            assert_eq!(candidate.version, package.version);
            assert_eq!(candidate.content_hash, snapshot.source_hash());
            assert_eq!(candidate.provenance, raw_entry);
            assert_ne!(
                candidate.content_hash,
                candidate.provenance.content_hash(),
                "profile {entry} conflated root and raw entry receipts for {}",
                candidate.package
            );
            assert_eq!(candidate.path, logical_dir(&candidate.package));
            if package.domains.contains(&PackageDomain::Authoritative) {
                authoritative.insert(candidate.package.clone());
            }
        }

        validate_profile_capability_receipts(&profile, &packages, entry);
        validate_profile_namespace_authority(&profile, &packages).unwrap_or_else(|error| {
            panic!("profile {entry} has an invalid owner-bound authority chain: {error}")
        });
        if expected_projection == ProfileKind::ClientShell {
            assert!(
                profile.source_universe.iter().all(|candidate| {
                    candidate.package.as_str() != "terrenia"
                        && !candidate.package.as_str().starts_with("@terrenia/")
                }),
                "shell must not carry a hidden Terrenia source universe"
            );
        }
        authoritative_by_profile.insert(entry, authoritative);
    }

    assert_eq!(
        authoritative_by_profile.get("dev.ncl"),
        authoritative_by_profile.get("headless.ncl"),
        "dev and headless must expose the same authoritative package candidates"
    );
    assert_eq!(
        authoritative_by_profile.get("headless.ncl"),
        authoritative_by_profile.get("test.ncl"),
        "headless and test must expose the same authoritative package candidates"
    );
}

#[test]
fn staged_adapter_ignores_moved_and_poisoned_origins_and_unreachable_files() {
    let fixture = FixtureTree::new(&[
        ("package.ncl", "fun _ => { value = 7 }\n"),
        ("poison.ncl", "import \"C:/ambient-poison.ncl\"\n"),
    ]);
    let snapshot = scan_fixture_root("example:source/move-proof", &fixture.root);
    let request = single_root_request(&snapshot, "package.ncl");
    let moved = fixture.base.join("moved-origin");
    fs::rename(&fixture.root, &moved).expect("fixture origin can be moved after acquisition");
    fs::create_dir(&fixture.root).expect("poison replacement origin can be created");
    fs::write(
        fixture.root.join("package.ncl"),
        "fun _ => import \"C:/ambient-poison.ncl\"\n",
    )
    .expect("replacement origin can be poisoned");

    let first: TrustedStagedEvaluation<SmallOutput> = evaluate_trusted_staged_nickel_function(
        &request,
        std::slice::from_ref(&snapshot),
        &Value::Null,
    )
    .expect("frozen source bytes evaluate after their origin moves");
    let second: TrustedStagedEvaluation<SmallOutput> = evaluate_trusted_staged_nickel_function(
        &request,
        std::slice::from_ref(&snapshot),
        &Value::Null,
    )
    .expect("the same frozen source bytes evaluate again");

    assert_eq!(first.value, SmallOutput { value: 7 });
    assert_eq!(first.value, second.value);
    assert_eq!(first.source_closure, second.source_closure);
    assert_ne!(first.staging_path, second.staging_path);
    assert!(!first.staging_path.exists());
    assert!(!second.staging_path.exists());
    assert_eq!(first.staged_addresses.len(), 1);
    assert_eq!(first.staged_addresses[0].logical_path(), "package.ncl");
    assert!(
        snapshot.files().contains_key("poison.ncl"),
        "whole-root acquisition includes the unreachable poison fixture"
    );
    assert!(
        first
            .staged_addresses
            .iter()
            .all(|address| address.logical_path() != "poison.ncl"),
        "the staged adapter must materialize the closure, not the whole root"
    );
}

#[test]
fn source_closure_rejects_escape_absolute_missing_alias_and_missing_file() {
    for (label, source) in [
        ("parent escape", "fun _ => import \"../outside.ncl\"\n"),
        ("absolute path", "fun _ => import \"C:/ambient.ncl\"\n"),
        ("missing alias", "fun _ => import undeclared_package\n"),
        ("missing file", "fun _ => import \"missing.ncl\"\n"),
    ] {
        let fixture = FixtureTree::new(&[("package.ncl", source)]);
        let snapshot = scan_fixture_root(
            &format!("example:source/{}", label.replace(' ', "-")),
            &fixture.root,
        );
        let request = single_root_request(&snapshot, "package.ncl");
        let error = evaluate_trusted_staged_nickel_function::<SmallOutput>(
            &request,
            &[snapshot],
            &Value::Null,
        )
        .expect_err("unauthorized import must fail before staging evaluation");
        let TrustedStagedEvaluationError::SourceClosure(error) = error else {
            panic!("{label} reached the evaluator instead of closure authorization");
        };
        assert!(
            matches!(
                error,
                SourceClosureError::InvalidLogicalPath { .. }
                    | SourceClosureError::PackageAliasNotGranted { .. }
                    | SourceClosureError::SourceNotFound { .. }
            ),
            "unexpected {label} error: {error}"
        );
        assert_eq!(error.code(), "compose.import_denied");
    }
}

#[test]
fn owner_bound_authority_rejects_wrong_grantee_nondependency_and_widening() {
    let library = library_snapshot();
    let profiles = scan_workspace_root(PROFILE_SOURCE_ID, AuthorizedRootKind::Test, "profiles");
    let snapshots = package_snapshots();
    let packages = evaluated_packages(&snapshots, &library);
    let binder = profile_receipt_binder(&snapshots);
    let mut shell = evaluate_profile(&profiles, "shell.ncl", &library, &binder).value;

    shell.policy.namespace_grants.insert(
        package_name("@terrenia/blocks"),
        BTreeSet::from([grant_pattern("terrenia:block/**")]),
    );
    assert!(matches!(
        shell.validate(),
        Err(CompositionError::ProfileNamespaceGrantToNonRoot { .. })
    ));

    let mut settings = packages
        .get(&package_name("@latticeaxiom/settings"))
        .expect("settings is shipped")
        .clone();
    settings.namespace_delegations.insert(
        package_name("@terrenia/blocks"),
        BTreeSet::from([grant_pattern("latticeaxiom:capability/settings-registry")]),
    );
    assert!(matches!(
        settings.validate(),
        Err(CompositionError::NamespaceDelegationToNonDependency { .. })
    ));

    let mut front_end = packages
        .get(&package_name("@latticeaxiom/front-end"))
        .expect("front-end is shipped")
        .clone();
    front_end
        .namespace_delegations
        .entry(package_name("@latticeaxiom/world-library"))
        .or_default()
        .insert(grant_pattern("latticeaxiom:capability/debug-workbench"));
    assert!(matches!(
        front_end.validate(),
        Err(CompositionError::NamespaceDelegationWidensAuthority { .. })
    ));

    let mut shell = evaluate_profile(&profiles, "shell.ncl", &library, &binder).value;
    shell.policy.namespace_grants.insert(
        package_name("@latticeaxiom/front-end"),
        BTreeSet::from([grant_pattern("latticeaxiom:capability/client-shell")]),
    );
    assert!(validate_profile_namespace_authority(&shell, &packages).is_err());
}

#[test]
fn fixture_layout_uses_semantic_roots_instead_of_roadmap_versions() {
    let fixtures = workspace_path("fixtures");
    assert!(
        fixtures.join("composition").is_dir(),
        "fixtures/composition must remain the semantic composition corpus root"
    );
    assert!(
        !fixtures.join("r0").exists(),
        "roadmap stages such as r0 must never become physical fixture paths"
    );
    for entry in fs::read_dir(&fixtures).expect("the fixture root is readable") {
        let entry = entry.expect("fixture root entries are readable");
        if !entry
            .file_type()
            .expect("fixture entry type is readable")
            .is_dir()
        {
            continue;
        }
        let name = entry.file_name();
        let name = name.to_string_lossy().to_ascii_lowercase();
        let version = name.strip_prefix('r').or_else(|| name.strip_prefix('d'));
        assert!(
            !version.is_some_and(|suffix| {
                !suffix.is_empty() && suffix.bytes().all(|byte| byte.is_ascii_digit())
            }),
            "roadmap stage `{name}` must not become a direct physical fixture root"
        );
    }
}

fn validate_shipped_package_delegations(packages: &BTreeMap<PackageName, PackageSpec>) {
    let expected = BTreeMap::from([
        (
            package_name("@latticeaxiom/front-end"),
            BTreeSet::from([
                package_name("@latticeaxiom/observability"),
                package_name("@latticeaxiom/settings-ui"),
                package_name("@latticeaxiom/world-library"),
            ]),
        ),
        (
            package_name("@latticeaxiom/settings-ui"),
            BTreeSet::from([package_name("@latticeaxiom/settings")]),
        ),
        (
            package_name("terrenia"),
            BTreeSet::from([
                package_name("@terrenia/blocks"),
                package_name("@terrenia/gameplay"),
                package_name("@terrenia/presentation"),
                package_name("@terrenia/tools"),
                package_name("@terrenia/worldgen"),
            ]),
        ),
    ]);
    for (name, package) in packages {
        let actual = package
            .namespace_delegations
            .keys()
            .cloned()
            .collect::<BTreeSet<_>>();
        assert_eq!(
            actual,
            expected.get(name).cloned().unwrap_or_default(),
            "unexpected direct-dependency namespace delegation set for {name}"
        );
    }
}

fn validate_profile_namespace_authority(
    profile: &GameProfileSpec,
    packages: &BTreeMap<PackageName, PackageSpec>,
) -> Result<BTreeSet<PackageName>, String> {
    let selected = select_profile_package_closure(profile, packages)?;
    let effective = resolve_effective_namespace_grants(profile, packages, &selected)?;
    validate_selected_package_authority(packages, &selected, &effective)?;
    Ok(selected)
}

fn select_profile_package_closure(
    profile: &GameProfileSpec,
    packages: &BTreeMap<PackageName, PackageSpec>,
) -> Result<BTreeSet<PackageName>, String> {
    let mut selected = BTreeSet::new();
    let mut pending = profile.roots.keys().cloned().collect::<BTreeSet<_>>();
    while let Some(name) = pending.iter().next().cloned() {
        pending.remove(&name);
        if !selected.insert(name.clone()) {
            continue;
        }
        let package = packages
            .get(&name)
            .ok_or_else(|| format!("selected package {name} is not shipped"))?;
        let mut active_features = profile.features.get(&name).cloned().unwrap_or_default();
        if let Some(root) = profile.roots.get(&name) {
            active_features.extend(root.features.iter().cloned());
        }
        for (dependency_name, dependency) in &package.dependencies {
            let feature_active = dependency
                .when_features
                .iter()
                .any(|feature| active_features.contains(feature));
            if (!dependency.optional || feature_active)
                && !dependency.domains.is_disjoint(&profile.projection_domains)
            {
                pending.insert(dependency_name.clone());
            }
        }
    }
    Ok(selected)
}

fn resolve_effective_namespace_grants(
    profile: &GameProfileSpec,
    packages: &BTreeMap<PackageName, PackageSpec>,
    selected: &BTreeSet<PackageName>,
) -> Result<BTreeMap<PackageName, BTreeSet<NamespaceGrantPattern>>, String> {
    let mut effective = profile.policy.namespace_grants.clone();
    loop {
        let mut changed = false;
        for name in selected {
            let Some(incoming) = effective.get(name).cloned() else {
                continue;
            };
            let package = packages
                .get(name)
                .ok_or_else(|| format!("selected package {name} is not shipped"))?;
            if !requests_are_covered(package, &incoming) {
                continue;
            }
            for (grantee, patterns) in &package.namespace_delegations {
                if !selected.contains(grantee) {
                    continue;
                }
                for pattern in patterns {
                    if !incoming.iter().any(|authority| authority.covers(pattern)) {
                        return Err(format!(
                            "{name} widens authority delegated to {grantee} with {pattern}"
                        ));
                    }
                    changed |= effective
                        .entry(grantee.clone())
                        .or_default()
                        .insert(pattern.clone());
                }
            }
        }
        if !changed {
            return Ok(effective);
        }
    }
}

fn requests_are_covered(package: &PackageSpec, incoming: &BTreeSet<NamespaceGrantPattern>) -> bool {
    package
        .namespace_requests
        .iter()
        .all(|request| incoming.iter().any(|authority| authority.covers(request)))
}

fn validate_selected_package_authority(
    packages: &BTreeMap<PackageName, PackageSpec>,
    selected: &BTreeSet<PackageName>,
    effective: &BTreeMap<PackageName, BTreeSet<NamespaceGrantPattern>>,
) -> Result<(), String> {
    for name in selected {
        let package = packages
            .get(name)
            .ok_or_else(|| format!("selected package {name} is not shipped"))?;
        let incoming = effective.get(name);
        for request in &package.namespace_requests {
            if !incoming
                .is_some_and(|patterns| patterns.iter().any(|authority| authority.covers(request)))
            {
                return Err(format!(
                    "{name} requests {request} without owner-bound incoming authority"
                ));
            }
        }
        validate_package_registrations(name, package, incoming)?;
    }
    Ok(())
}

fn validate_package_registrations(
    name: &PackageName,
    package: &PackageSpec,
    incoming: Option<&BTreeSet<NamespaceGrantPattern>>,
) -> Result<(), String> {
    for registration in &package.registration.registrations {
        if registration.declared_by != *name {
            return Err(format!(
                "registration {} is carried by {name} but declares {}",
                registration.id, registration.declared_by
            ));
        }
        if !package
            .namespace_requests
            .iter()
            .any(|request| request.matches(&registration.id))
        {
            return Err(format!(
                "registration {} has no request owned by {name}",
                registration.id
            ));
        }
        if !incoming.is_some_and(|patterns| {
            patterns
                .iter()
                .any(|authority| authority.matches(&registration.id))
        }) {
            return Err(format!(
                "registration {} has no effective grant for {name}",
                registration.id
            ));
        }
    }
    Ok(())
}
fn grant_pattern(value: &str) -> NamespaceGrantPattern {
    NamespaceGrantPattern::new(value)
        .unwrap_or_else(|error| panic!("invalid fixture namespace grant pattern {value}: {error}"))
}
fn validate_profile_capability_receipts(
    profile: &GameProfileSpec,
    packages: &BTreeMap<PackageName, PackageSpec>,
    entry: &str,
) {
    for (capability, requirement) in &profile.capabilities {
        assert_eq!(requirement.cardinality, CapabilityCardinality::ExactlyOne);
        let provider_name = requirement
            .provider
            .as_ref()
            .unwrap_or_else(|| panic!("profile {entry} has no exact provider for {capability}"));
        let provider = packages
            .get(provider_name)
            .unwrap_or_else(|| panic!("profile {entry} provider {provider_name} is not shipped"));
        let provision = provider.provides.get(capability).unwrap_or_else(|| {
            panic!(
                "profile {entry} names {provider_name} for {capability}, but it does not provide it"
            )
        });
        assert!(
            requirement.version.matches(&provision.version),
            "profile {entry} requires {capability} {}, provider has {}",
            requirement.version,
            provision.version
        );
        assert!(
            requirement.domains.is_subset(&provision.domains),
            "profile {entry} provider {provider_name} lacks a required domain for {capability}"
        );
    }
}

fn evaluated_packages(
    snapshots: &BTreeMap<PackageName, SourceSnapshot>,
    library: &SourceSnapshot,
) -> BTreeMap<PackageName, PackageSpec> {
    snapshots
        .iter()
        .map(|(name, snapshot)| {
            let package = evaluate_package(snapshot, library).value;
            assert_eq!(&package.name, name);
            (name.clone(), package)
        })
        .collect()
}

fn evaluate_package(
    snapshot: &SourceSnapshot,
    library: &SourceSnapshot,
) -> TrustedStagedEvaluation<PackageSpec> {
    let request = source_request(snapshot, "package.ncl", library);
    evaluate_trusted_staged_package(
        &request,
        &[snapshot.clone(), library.clone()],
        &entry_provenance(snapshot, "package.ncl"),
    )
    .unwrap_or_else(|error| {
        panic!(
            "package {} failed bound staged evaluation: {error}",
            snapshot.source_id()
        )
    })
}

fn evaluate_profile(
    profiles: &SourceSnapshot,
    entry: &str,
    library: &SourceSnapshot,
    binder: &Value,
) -> TrustedStagedEvaluation<GameProfileSpec> {
    let request = source_request(profiles, entry, library);
    evaluate_trusted_staged_nickel_function(&request, &[profiles.clone(), library.clone()], binder)
        .unwrap_or_else(|error| panic!("profile {entry} failed staged evaluation: {error}"))
}

fn source_request(
    entry_snapshot: &SourceSnapshot,
    entry_path: &str,
    library: &SourceSnapshot,
) -> SourceClosureRequest {
    let entry = source_address(entry_snapshot.source_id(), entry_path);
    let library_entry = source_address(library.source_id(), "main.ncl");
    let mut root_grants = vec![
        SourceRootGrant {
            source_id: entry_snapshot.source_id().clone(),
            root_kind: entry_snapshot.root_kind(),
            source_hash: entry_snapshot.source_hash(),
            entry: entry.clone(),
            package_alias: None,
        },
        SourceRootGrant {
            source_id: library.source_id().clone(),
            root_kind: library.root_kind(),
            source_hash: library.source_hash(),
            entry: library_entry,
            package_alias: Some(
                PackageAlias::new(R0_LIBRARY_PACKAGE_ALIAS)
                    .expect("the frozen library alias is valid"),
            ),
        },
    ];
    root_grants.sort_by(|left, right| left.source_id.cmp(&right.source_id));
    SourceClosureRequest {
        library_contract_major: NICKEL_LIBRARY_CONTRACT_MAJOR,
        corpus_major: R0_AUTHORING_CORPUS_MAJOR,
        entry,
        root_grants,
        package_instances: BTreeMap::new(),
        alias_edges: BTreeMap::new(),
        limits: NickelEvaluationLimits::default(),
    }
}

fn single_root_request(snapshot: &SourceSnapshot, entry_path: &str) -> SourceClosureRequest {
    let entry = source_address(snapshot.source_id(), entry_path);
    SourceClosureRequest {
        library_contract_major: NICKEL_LIBRARY_CONTRACT_MAJOR,
        corpus_major: R0_AUTHORING_CORPUS_MAJOR,
        entry: entry.clone(),
        root_grants: vec![SourceRootGrant {
            source_id: snapshot.source_id().clone(),
            root_kind: snapshot.root_kind(),
            source_hash: snapshot.source_hash(),
            entry,
            package_alias: None,
        }],
        package_instances: BTreeMap::new(),
        alias_edges: BTreeMap::new(),
        limits: NickelEvaluationLimits::default(),
    }
}

fn package_snapshots() -> BTreeMap<PackageName, SourceSnapshot> {
    PACKAGE_SOURCES
        .iter()
        .map(|source| {
            let snapshot = scan_workspace_root(
                source.source_id,
                AuthorizedRootKind::Package,
                source.logical_dir,
            );
            (package_name(source.package_name), snapshot)
        })
        .collect()
}

fn library_snapshot() -> SourceSnapshot {
    scan_workspace_root(
        LIBRARY_SOURCE_ID,
        AuthorizedRootKind::Library,
        "nickel/latticeaxiom",
    )
}

fn scan_workspace_root(
    source: &str,
    kind: AuthorizedRootKind,
    logical_dir: &str,
) -> SourceSnapshot {
    scan_root(source, kind, &workspace_path(logical_dir))
}

fn scan_fixture_root(source: &str, root: &Path) -> SourceSnapshot {
    scan_root(source, AuthorizedRootKind::Test, root)
}

fn scan_root(source: &str, kind: AuthorizedRootKind, root: &Path) -> SourceSnapshot {
    let authorized = AuthorizedRoot::new(source_id(source), kind, root)
        .unwrap_or_else(|error| panic!("invalid authorized root {}: {error}", root.display()));
    scan_source_snapshot(
        &authorized,
        SourceScanLimits {
            maximum_files: 128,
            maximum_bytes: 4 * 1024 * 1024,
        },
    )
    .unwrap_or_else(|error| panic!("could not scan {}: {error}", root.display()))
}

fn profile_receipt_binder(snapshots: &BTreeMap<PackageName, SourceSnapshot>) -> Value {
    let mut values = Map::new();
    for snapshot in snapshots.values() {
        values.insert(
            snapshot.source_id().to_string(),
            json!({
                "source_hash": snapshot.source_hash(),
                "entry_provenance": entry_provenance(snapshot, "package.ncl"),
            }),
        );
    }
    Value::Object(values)
}

fn entry_provenance(snapshot: &SourceSnapshot, logical_path: &str) -> SourceProvenance {
    let file = snapshot
        .files()
        .get(logical_path)
        .unwrap_or_else(|| panic!("{} lacks {logical_path}", snapshot.source_id()));
    SourceProvenance::new(
        snapshot.source_id().clone(),
        logical_path,
        file.receipt().content_hash(),
        None,
        Vec::new(),
    )
    .expect("canonical snapshot paths produce canonical provenance")
}

fn assert_non_overlapping_package_roots() {
    let roots = PACKAGE_SOURCES
        .iter()
        .map(|source| workspace_path(source.logical_dir))
        .collect::<Vec<_>>();
    for left in 0..roots.len() {
        for right in left + 1..roots.len() {
            assert!(
                !roots[left].starts_with(&roots[right]) && !roots[right].starts_with(&roots[left]),
                "package roots overlap: {} and {}",
                roots[left].display(),
                roots[right].display()
            );
        }
    }
}

fn logical_dir(package: &PackageName) -> String {
    PACKAGE_SOURCES
        .iter()
        .find(|source| package_name(source.package_name) == *package)
        .map_or_else(
            || panic!("no shipped logical directory for {package}"),
            |source| source.logical_dir.to_owned(),
        )
}

fn workspace_path(logical_path: impl AsRef<Path>) -> PathBuf {
    Path::new(WORKSPACE_ROOT).join(logical_path)
}

fn source_address(source: &SourceId, path: &str) -> SourceAddress {
    SourceAddress::new(source.clone(), path)
        .unwrap_or_else(|error| panic!("invalid source address {source}#{path}: {error}"))
}

fn source_id(value: &str) -> SourceId {
    value
        .parse()
        .unwrap_or_else(|error| panic!("invalid source ID {value}: {error}"))
}

fn package_name(value: &str) -> PackageName {
    value
        .parse()
        .unwrap_or_else(|error| panic!("invalid package name {value}: {error}"))
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct SmallOutput {
    value: u32,
}

struct FixtureTree {
    base: PathBuf,
    root: PathBuf,
}

impl FixtureTree {
    fn new(files: &[(&str, &str)]) -> Self {
        let parent = std::env::temp_dir();
        let mut allocated = None;
        for _ in 0..128 {
            let sequence = FIXTURE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
            let base = parent.join(format!(
                "{FIXTURE_PREFIX}-{}-{sequence:016x}",
                std::process::id()
            ));
            match fs::create_dir(&base) {
                Ok(()) => {
                    allocated = Some(base);
                    break;
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(error) => panic!("could not create fixture root: {error}"),
            }
        }
        let base = allocated.expect("a unique fixture directory is available");
        let root = base.join("root");
        fs::create_dir(&root).expect("fixture source root can be created");
        for (logical_path, contents) in files {
            let destination = root.join(logical_path);
            if let Some(parent) = destination.parent() {
                fs::create_dir_all(parent).expect("fixture parent can be created");
            }
            fs::write(&destination, contents).expect("fixture source can be written");
        }
        Self { base, root }
    }
}

impl Drop for FixtureTree {
    fn drop(&mut self) {
        let Some(name) = self.base.file_name().and_then(|name| name.to_str()) else {
            return;
        };
        if name.starts_with(FIXTURE_PREFIX)
            && self
                .base
                .parent()
                .is_some_and(|parent| parent == std::env::temp_dir())
        {
            let _ = fs::remove_dir_all(&self.base);
        }
    }
}

#[allow(
    dead_code,
    reason = "keeps receipt type intent explicit in diagnostics"
)]
fn _hash_type_is_not_provenance(hash: CanonicalHash) -> CanonicalHash {
    hash
}
