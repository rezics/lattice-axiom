//! Conformance for shipped D0 authoring roots through the verified source table.

#![cfg(feature = "nickel-evaluator")]
#![allow(
    clippy::expect_used,
    reason = "integration fixtures state their construction invariants"
)]

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use latticeaxiom_compose::{
    AuthorizedRoot, AuthorizedRootKind, BootstrapSourceProviderV1, CapabilityCardinality,
    CompositionBootstrapV1, CompositionError, GameProfileSpec, NICKEL_LIBRARY_CONTRACT_MAJOR,
    NickelEvaluationLimits, PACKAGE_MODEL_VERSION, PackageAlias, PackageDomain, PackageSpec,
    ProfileKind, R0_AUTHORING_CORPUS_MAJOR, R0_LIBRARY_PACKAGE_ALIAS, RealizationKind,
    SourceAddress, SourceClosureError, SourceClosureRequest, SourceRootGrant, SourceScanLimits,
    SourceSnapshot, TrustedStagedEvaluation, TrustedStagedEvaluationError,
    evaluate_trusted_staged_nickel_function, evaluate_trusted_staged_package, scan_source_snapshot,
};
use latticeaxiom_core::{
    CanonicalHash, CapabilityId, NamespaceGrantPattern, PackageName, SourceId, SourceProvenance,
    canonical_json_bytes, canonical_json_hash,
};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

const WORKSPACE_ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../../../..");
const LIBRARY_SOURCE_ID: &str = "latticeaxiom:source/library-v3";
const PROFILE_SOURCE_ID: &str = "latticeaxiom:source/shipped-profiles";
const FIXTURE_PREFIX: &str = "latticeaxiom-shipped-source-fixture";

#[derive(Clone, Debug, Eq, PartialEq)]
struct PackageSource {
    logical_dir: String,
    source_id: String,
    package_name: PackageName,
}

const MAX_SHIPPED_NAMESPACES: usize = 16;
const MAX_PACKAGES_PER_NAMESPACE: usize = 64;
const MAX_WORKSPACE_MEMBERS: usize = 128;

const PROFILE_SOURCES: [(&str, ProfileKind); 4] = [
    ("shell.ncl", ProfileKind::ClientShell),
    ("dev.ncl", ProfileKind::ClientWorld),
    ("headless.ncl", ProfileKind::DedicatedServer),
    ("test.ncl", ProfileKind::HeadlessTest),
];

#[test]
fn shipped_packages_use_verified_alias_closures_and_raw_entry_provenance() {
    let library = library_snapshot();
    let package_sources = package_sources();
    let package_snapshots = package_snapshots();
    assert_non_overlapping_package_roots(&package_sources);

    let mut evaluated = BTreeMap::new();
    for source in &package_sources {
        let expected_name = &source.package_name;
        let snapshot = package_snapshots
            .get(expected_name)
            .expect("every shipped package has a snapshot");
        let result = evaluate_package(snapshot, &library);
        let package = result.value;
        package
            .validate()
            .unwrap_or_else(|error| panic!("{} is invalid: {error}", package.name));

        assert_eq!(package.model_version, PACKAGE_MODEL_VERSION);
        assert_eq!(&package.name, expected_name);
        assert_eq!(package.version.to_string(), "0.1.0");
        let manifest = latticeaxiom_compose::PackageSourceManifestV1::from_toml_str(
            &fs::read_to_string(workspace_path(format!(
                "{}/latticeaxiom-package.toml",
                source.logical_dir
            )))
            .expect("shipped source manifest exists"),
        )
        .expect("shipped source manifest validates");
        assert_eq!(package.trust, manifest.trust);
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
                .provides
                .values()
                .all(|row| row.cardinality == CapabilityCardinality::ExactlyOne)
        );
        assert!(evaluated.insert(package.name.clone(), package).is_none());
    }

    validate_shipped_package_delegations(&evaluated);
    validate_terrenia_presentation_capability(&evaluated);

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
fn license_policy_covers_the_manifest_derived_workspace() {
    let workspace_members = workspace_member_paths();
    let about_path = workspace_path("supply-chain/about.toml");
    let about_text = fs::read_to_string(&about_path)
        .unwrap_or_else(|error| panic!("could not read {}: {error}", about_path.display()));
    let about = toml::from_str::<toml::Value>(&about_text)
        .unwrap_or_else(|error| panic!("could not parse {}: {error}", about_path.display()));
    let about_table = about
        .as_table()
        .expect("supply-chain/about.toml has a top-level table");
    for crate_name in workspace_members.keys() {
        let accepted = about_table
            .get(crate_name)
            .and_then(|entry| entry.get("accepted"))
            .and_then(toml::Value::as_array)
            .unwrap_or_else(|| panic!("{crate_name} lacks an explicit accepted-license row"));
        assert_eq!(
            accepted.as_slice(),
            [toml::Value::String("AGPL-3.0-only".to_owned())],
            "{crate_name} must explicitly retain the workspace license"
        );
    }
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
fn root_bootstrap_matches_its_evaluated_nickel_profile() {
    let library = library_snapshot();
    let profiles = scan_workspace_root(PROFILE_SOURCE_ID, AuthorizedRootKind::Test, "profiles");
    let snapshots = package_snapshots();
    let binder = profile_receipt_binder(&snapshots);
    let bootstrap_path = workspace_path("latticeaxiom.toml");
    let bootstrap_text = fs::read_to_string(&bootstrap_path)
        .unwrap_or_else(|error| panic!("could not read {}: {error}", bootstrap_path.display()));
    let bootstrap =
        CompositionBootstrapV1::from_toml_str(&bootstrap_text).unwrap_or_else(|error| {
            panic!(
                "{} must parse as a composition bootstrap: {error}",
                bootstrap_path.display()
            )
        });
    let nickel_entry = bootstrap
        .nickel_profile_entry
        .as_str()
        .strip_prefix("profiles/")
        .expect("the shipped root bootstrap Nickel entry is rooted in profiles/");
    let profile = evaluate_profile(&profiles, nickel_entry, &library, &binder).value;

    profile
        .validate()
        .unwrap_or_else(|error| panic!("profile {nickel_entry} is invalid: {error}"));
    assert_eq!(bootstrap.projection, profile.projection);
    assert_eq!(bootstrap.projection_domains, profile.projection_domains);
    assert_eq!(bootstrap.roots, profile.roots);
    assert_eq!(bootstrap.features, profile.features);
    assert_eq!(bootstrap.parameters, profile.parameters);
    assert_eq!(bootstrap.realization_policy, profile.realization_policy);
    assert_eq!(bootstrap.evaluation_policy, profile.evaluation_policy);
    assert_eq!(bootstrap.evaluation_limits, profile.evaluation_limits);
    assert_eq!(bootstrap.realization_policy, [RealizationKind::Data]);
    assert_eq!(profile.policy.maximum_trust, bootstrap.maximum_trust);

    let bootstrap_sources = bootstrap
        .sources
        .iter()
        .map(|source| match source {
            BootstrapSourceProviderV1::Path { package, path } => (package.clone(), path.as_str()),
            _ => panic!(
                "shipped root bootstrap source {} must be a path",
                source.package()
            ),
        })
        .collect::<BTreeMap<_, _>>();
    for candidate in &profile.source_universe {
        assert_eq!(
            bootstrap_sources.get(&candidate.package).copied(),
            Some(candidate.path.as_str()),
            "Nickel source {} must match its root-bootstrap package and path authorization",
            candidate.package
        );
    }
    let nickel_source_packages = profile
        .source_universe
        .iter()
        .map(|candidate| candidate.package.clone())
        .collect::<BTreeSet<_>>();
    for root in profile.roots.keys() {
        assert!(
            nickel_source_packages.contains(root),
            "Nickel root {root} must have a source candidate"
        );
    }
    assert!(
        profile
            .roots
            .contains_key(&package_name("@latticeaxiom/input")),
        "the dedicated-server profile must select the data-only input action catalog"
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
    let moved = fixture.base.path().join("moved-origin");
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
            package_name("@latticeaxiom/input"),
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

fn validate_terrenia_presentation_capability(packages: &BTreeMap<PackageName, PackageSpec>) {
    let capability = "latticeaxiom:capability/content-presentation@1"
        .parse::<CapabilityId>()
        .expect("content-presentation capability is canonical");
    let presentation_name = package_name("@terrenia/presentation");
    let presentation = packages
        .get(&presentation_name)
        .expect("Terrenia presentation is shipped");
    let provision = presentation
        .provides
        .get(&capability)
        .expect("Terrenia presentation provides content-presentation");
    assert_eq!(provision.capability, capability);
    assert_eq!(provision.version.to_string(), "1.0.0");
    assert_eq!(provision.cardinality, CapabilityCardinality::ExactlyOne);
    assert_eq!(provision.domains, BTreeSet::from([PackageDomain::Client]));

    let root = packages
        .get(&package_name("terrenia"))
        .expect("Terrenia root is shipped");
    let requirement = root
        .requires
        .get(&capability)
        .expect("Terrenia root requires content-presentation");
    assert_eq!(requirement.capability, capability);
    assert_eq!(requirement.provider.as_ref(), Some(&presentation_name));
    assert_eq!(requirement.cardinality, CapabilityCardinality::ExactlyOne);
    assert_eq!(requirement.domains, BTreeSet::from([PackageDomain::Client]));
    assert!(requirement.version.matches(&provision.version));
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

fn workspace_member_paths() -> BTreeMap<String, String> {
    let workspace_root = canonical_workspace_root();
    let root_manifest = workspace_root.join("Cargo.toml");
    assert_canonical_file_within(&root_manifest, &workspace_root);
    let root_text = fs::read_to_string(&root_manifest)
        .unwrap_or_else(|error| panic!("could not read {}: {error}", root_manifest.display()));
    let root = toml::from_str::<toml::Value>(&root_text)
        .unwrap_or_else(|error| panic!("could not parse {}: {error}", root_manifest.display()));
    let members = root
        .get("workspace")
        .and_then(|workspace| workspace.get("members"))
        .and_then(toml::Value::as_array)
        .expect("workspace.members is an array");
    assert!(
        members.len() <= MAX_WORKSPACE_MEMBERS,
        "workspace member count exceeds {MAX_WORKSPACE_MEMBERS}"
    );

    let mut paths = BTreeMap::new();
    for member in members {
        let declared = member
            .as_str()
            .expect("every workspace member is an explicit string path");
        assert!(
            !declared.contains('*') && !declared.contains('?'),
            "workspace completeness requires explicit member paths, found {declared}"
        );
        let canonical_member = fs::canonicalize(workspace_root.join(declared))
            .unwrap_or_else(|error| panic!("could not canonicalize member {declared}: {error}"));
        assert!(
            canonical_member.starts_with(&workspace_root),
            "workspace member {declared} escapes the workspace"
        );
        let member_manifest = canonical_member.join("Cargo.toml");
        assert_canonical_file_within(&member_manifest, &canonical_member);
        let member_text = fs::read_to_string(&member_manifest).unwrap_or_else(|error| {
            panic!("could not read {}: {error}", member_manifest.display())
        });
        let member_value = toml::from_str::<toml::Value>(&member_text).unwrap_or_else(|error| {
            panic!("could not parse {}: {error}", member_manifest.display())
        });
        let crate_name = member_value
            .get("package")
            .and_then(|package| package.get("name"))
            .and_then(toml::Value::as_str)
            .unwrap_or_else(|| panic!("{} lacks package.name", member_manifest.display()));
        let logical_dir = logical_workspace_dir(&workspace_root, &canonical_member);
        assert_eq!(
            logical_dir,
            declared.replace('\\', "/"),
            "workspace member path must already be canonical"
        );
        assert!(
            paths.insert(crate_name.to_owned(), logical_dir).is_none(),
            "duplicate workspace crate name {crate_name}"
        );
    }
    paths
}

fn package_sources() -> Vec<PackageSource> {
    let workspace_root = canonical_workspace_root();
    let packages_root = fs::canonicalize(workspace_root.join("packages"))
        .expect("the shipped packages root is canonicalizable");
    assert!(
        packages_root.starts_with(&workspace_root),
        "the shipped packages root must remain inside the workspace"
    );

    let mut by_name = BTreeMap::new();
    let mut source_ids = BTreeSet::new();
    for namespace in bounded_child_directories(
        &packages_root,
        MAX_SHIPPED_NAMESPACES,
        "shipped package namespaces",
    ) {
        for root in bounded_child_directories(
            &namespace,
            MAX_PACKAGES_PER_NAMESPACE,
            "shipped packages in one namespace",
        ) {
            let entry = root.join("package.ncl");
            let manifest = root.join("latticeaxiom-package.toml");
            let has_entry = entry.is_file();
            let has_manifest = manifest.is_file();
            assert_eq!(
                has_entry,
                has_manifest,
                "{} must carry package.ncl and latticeaxiom-package.toml together",
                root.display()
            );
            if !has_entry {
                continue;
            }

            assert_canonical_file_within(&entry, &root);
            assert_canonical_file_within(&manifest, &root);
            let logical_dir = logical_workspace_dir(&workspace_root, &root);
            let manifest_text = fs::read_to_string(&manifest)
                .unwrap_or_else(|error| panic!("could not read {}: {error}", manifest.display()));
            let manifest_value = toml::from_str::<toml::Value>(&manifest_text)
                .unwrap_or_else(|error| panic!("could not parse {}: {error}", manifest.display()));
            let raw_name = manifest_value
                .get("name")
                .and_then(toml::Value::as_str)
                .unwrap_or_else(|| panic!("{} lacks a string package name", manifest.display()));
            let package_name = package_name(raw_name);
            let source_id = package_source_id(&package_name);
            assert!(
                source_ids.insert(source_id.clone()),
                "derived source ID {source_id} is not unique"
            );
            let previous = by_name.insert(
                package_name.clone(),
                PackageSource {
                    logical_dir,
                    source_id,
                    package_name,
                },
            );
            assert!(
                previous.is_none(),
                "duplicate shipped package name {raw_name}"
            );
        }
    }

    assert!(
        !by_name.is_empty(),
        "the shipped package set cannot be empty"
    );
    by_name.into_values().collect()
}

fn bounded_child_directories(root: &Path, maximum_entries: usize, label: &str) -> Vec<PathBuf> {
    let canonical_root = fs::canonicalize(root)
        .unwrap_or_else(|error| panic!("could not canonicalize {}: {error}", root.display()));
    let entries = fs::read_dir(&canonical_root)
        .unwrap_or_else(|error| panic!("could not read {}: {error}", canonical_root.display()))
        .collect::<Result<Vec<_>, _>>()
        .unwrap_or_else(|error| {
            panic!("could not enumerate {}: {error}", canonical_root.display())
        });
    assert!(
        entries.len() <= maximum_entries,
        "{label} exceeds the bounded entry limit {maximum_entries}"
    );

    let mut directories = Vec::new();
    for entry in entries {
        let file_type = entry.file_type().unwrap_or_else(|error| {
            panic!("could not inspect {}: {error}", entry.path().display())
        });
        assert!(
            !file_type.is_symlink(),
            "{} must not be a symlink",
            entry.path().display()
        );
        if !file_type.is_dir() {
            continue;
        }
        let canonical = fs::canonicalize(entry.path()).unwrap_or_else(|error| {
            panic!("could not canonicalize {}: {error}", entry.path().display())
        });
        assert!(
            canonical.starts_with(&canonical_root),
            "{} escapes {}",
            canonical.display(),
            canonical_root.display()
        );
        directories.push(canonical);
    }
    directories.sort();
    directories
}

fn assert_canonical_file_within(path: &Path, root: &Path) {
    let metadata = fs::symlink_metadata(path)
        .unwrap_or_else(|error| panic!("could not inspect {}: {error}", path.display()));
    assert!(
        metadata.file_type().is_file(),
        "{} must be a regular file",
        path.display()
    );
    let canonical = fs::canonicalize(path)
        .unwrap_or_else(|error| panic!("could not canonicalize {}: {error}", path.display()));
    assert!(
        canonical.starts_with(root),
        "{} escapes package root {}",
        canonical.display(),
        root.display()
    );
}

fn canonical_workspace_root() -> PathBuf {
    fs::canonicalize(WORKSPACE_ROOT)
        .unwrap_or_else(|error| panic!("could not canonicalize {WORKSPACE_ROOT}: {error}"))
}

fn logical_workspace_dir(workspace_root: &Path, canonical_path: &Path) -> String {
    canonical_path
        .strip_prefix(workspace_root)
        .unwrap_or_else(|_| {
            panic!(
                "{} escapes workspace {}",
                canonical_path.display(),
                workspace_root.display()
            )
        })
        .components()
        .map(|component| {
            component
                .as_os_str()
                .to_str()
                .expect("workspace package paths are UTF-8")
        })
        .collect::<Vec<_>>()
        .join("/")
}

fn package_source_id(package: &PackageName) -> String {
    let unscoped = package
        .as_str()
        .strip_prefix('@')
        .unwrap_or(package.as_str());
    let (owner, leaf) = unscoped
        .split_once('/')
        .map_or((unscoped, "main"), |(owner, leaf)| (owner, leaf));
    let value = format!("{owner}:source/{leaf}");
    let _ = source_id(&value);
    value
}
fn package_snapshots() -> BTreeMap<PackageName, SourceSnapshot> {
    package_sources()
        .into_iter()
        .map(|source| {
            let snapshot = scan_workspace_root(
                &source.source_id,
                AuthorizedRootKind::Package,
                &source.logical_dir,
            );
            (source.package_name, snapshot)
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
            maximum_files: 4_096,
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

fn assert_non_overlapping_package_roots(sources: &[PackageSource]) {
    let roots = sources
        .iter()
        .map(|source| workspace_path(&source.logical_dir))
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
    package_sources()
        .into_iter()
        .find(|source| source.package_name == *package)
        .map_or_else(
            || panic!("no shipped logical directory for {package}"),
            |source| source.logical_dir,
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
    base: tempfile::TempDir,
    root: PathBuf,
}

impl FixtureTree {
    fn new(files: &[(&str, &str)]) -> Self {
        let base = tempfile::Builder::new()
            .prefix(FIXTURE_PREFIX)
            .tempdir()
            .expect("a unique fixture directory is available");
        let root = base.path().join("root");
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

#[allow(
    dead_code,
    reason = "keeps receipt type intent explicit in diagnostics"
)]
fn _hash_type_is_not_provenance(hash: CanonicalHash) -> CanonicalHash {
    hash
}
