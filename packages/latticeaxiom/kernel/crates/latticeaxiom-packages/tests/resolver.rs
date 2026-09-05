//! Integration coverage for deterministic package resolution and frozen receipts.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Debug;
use std::str::FromStr;

use latticeaxiom_compose::{
    ArtifactIntent, COMPOSITION_SCHEMA_VERSION, CapabilityCardinality, CapabilityProvision,
    CapabilityRequirement, CompositionPolicy, CompositionSpec, FeatureSpec, InterfaceRequirement,
    NickelEvaluationLimits, PACKAGE_MODEL_VERSION, PackageDependency, PackageDomain,
    PackageMetadata, PackageRequest, PackageSpec, ProfileKind, R0_NICKEL_EVALUATION_POLICY,
    RealizationId, RealizationKind, RealizationPreference, RealizationSpec, RegistrationFragment,
    SourceCandidate, TrustClass,
};
use latticeaxiom_core::{
    CanonicalHash, CapabilityId, NamespaceGrantPattern, NamespaceGrantorRef, PackageName,
    PackageVersion, PackageVersionReq, SourceId, SourceProvenance, StableId,
};
use latticeaxiom_packages::{
    HOST_COMPATIBILITY_SCHEMA_VERSION, HostCompatibilityV1, HostInterfaceV1, PackageCandidate,
    PackageResolutionV1, PackageResolver, PackageSourceKind, ResolutionBudget, ResolutionError,
    ResolutionFailureCodeV1, ResolutionLimits, ResolutionOutcomeV1, ResolutionReasonV1,
    ResolutionReceiptError, ResolutionReceiptV1, ResolutionSubjectV1,
};

const HOST_TARGET: &str = "x86_64-pc-windows-msvc";

fn parsed<T>(value: &str) -> T
where
    T: FromStr,
    T::Err: Debug,
{
    value
        .parse()
        .unwrap_or_else(|error| panic!("failed to parse `{value}`: {error:?}"))
}

fn succeeded<T, E>(result: Result<T, E>) -> T
where
    E: Debug,
{
    result.unwrap_or_else(|error| panic!("operation unexpectedly failed: {error:?}"))
}

fn package_name(value: &str) -> PackageName {
    parsed(value)
}

fn version(value: &str) -> PackageVersion {
    parsed(value)
}

fn version_requirement(value: &str) -> PackageVersionReq {
    parsed(value)
}

fn source_id(label: &str) -> SourceId {
    parsed(&format!("test:source/{label}"))
}

fn capability_id(label: &str) -> CapabilityId {
    parsed(&format!("test:capability/{label}@1"))
}

fn namespace_pattern(value: &str) -> NamespaceGrantPattern {
    parsed(value)
}

fn authoritative_domains() -> BTreeSet<PackageDomain> {
    BTreeSet::from([PackageDomain::Authoritative])
}

fn provenance(source: &SourceId, path: &str, hash: CanonicalHash) -> SourceProvenance {
    succeeded(SourceProvenance::new(
        source.clone(),
        path,
        hash,
        None,
        Vec::new(),
    ))
}

fn data_realization(label: &str) -> RealizationSpec {
    let id = succeeded(RealizationId::new(label));
    RealizationSpec {
        id,
        kind: RealizationKind::Data,
        domains: authoritative_domains(),
        targets: BTreeSet::new(),
        interfaces: BTreeMap::new(),
        required_features: BTreeSet::new(),
        artifact: ArtifactIntent::DataRoot {
            path: parsed("data"),
        },
        trust: TrustClass::DataOnly,
        engine_build: None,
        registration_fragment: CanonicalHash::digest(format!("fragment:{label}")),
    }
}

fn native_realization(label: &str, target: &str, trust: TrustClass) -> RealizationSpec {
    let id = succeeded(RealizationId::new(label));
    RealizationSpec {
        id,
        kind: RealizationKind::NativeStatic,
        domains: authoritative_domains(),
        targets: BTreeSet::from([parsed(target)]),
        interfaces: BTreeMap::new(),
        required_features: BTreeSet::new(),
        artifact: ArtifactIntent::SourceBuild,
        trust,
        engine_build: None,
        registration_fragment: CanonicalHash::digest(format!("fragment:{label}:{target}")),
    }
}

fn portable_realization(
    label: &str,
    interface: &StableId,
    requirement: &str,
    optional: bool,
) -> RealizationSpec {
    let id = succeeded(RealizationId::new(label));
    RealizationSpec {
        id,
        kind: RealizationKind::PortableNative,
        domains: authoritative_domains(),
        targets: BTreeSet::from([parsed(HOST_TARGET)]),
        interfaces: BTreeMap::from([(
            interface.clone(),
            InterfaceRequirement {
                version: version_requirement(requirement),
                optional,
            },
        )]),
        required_features: BTreeSet::new(),
        artifact: ArtifactIntent::SourceBuild,
        trust: TrustClass::TrustedNative,
        engine_build: None,
        registration_fragment: CanonicalHash::digest(format!("fragment:{label}:portable")),
    }
}

fn host_compatibility(
    target: &str,
    interface: Option<(&StableId, &str, &str)>,
    engine_build_id: Option<CanonicalHash>,
) -> HostCompatibilityV1 {
    let interfaces = interface.map_or_else(BTreeMap::new, |(id, version_value, descriptor)| {
        BTreeMap::from([(
            id.clone(),
            HostInterfaceV1 {
                version: version(version_value),
                descriptor_hash: CanonicalHash::digest(descriptor),
            },
        )])
    });
    HostCompatibilityV1 {
        schema_version: HOST_COMPATIBILITY_SCHEMA_VERSION,
        target: parsed(target),
        engine_build_id,
        interfaces,
    }
}

fn candidate(
    package: &str,
    package_version: &str,
    source_label: &str,
    priority: i32,
    source_kind: PackageSourceKind,
) -> PackageCandidate {
    let name = package_name(package);
    let version = version(package_version);
    let source_id = source_id(source_label);
    let content_hash =
        CanonicalHash::digest(format!("source:{package}:{package_version}:{source_label}"));
    let provenance = provenance(
        &source_id,
        &format!("packages/{source_label}/package.ncl"),
        content_hash,
    );
    let realization = data_realization("data");

    PackageCandidate {
        source_kind,
        source: SourceCandidate {
            source_id,
            package: name.clone(),
            version: version.clone(),
            path: format!("packages/{source_label}"),
            content_hash,
            priority,
            provenance: provenance.clone(),
        },
        package: PackageSpec {
            model_version: PACKAGE_MODEL_VERSION,
            name,
            version,
            metadata: PackageMetadata {
                display_name: package.to_owned(),
                documentation: None,
                license: "MIT".to_owned(),
            },
            features: BTreeMap::new(),
            dependencies: BTreeMap::new(),
            requires: BTreeMap::new(),
            provides: BTreeMap::new(),
            realizations: BTreeMap::from([(realization.id.clone(), realization)]),
            domains: authoritative_domains(),
            parameters: BTreeMap::new(),
            namespace_requests: BTreeSet::new(),
            namespace_delegations: BTreeMap::new(),
            trust: TrustClass::DataOnly,
            registration: RegistrationFragment::default(),
            provenance,
        },
    }
}

fn package_request(
    requirement: &str,
    features: impl IntoIterator<Item = &'static str>,
    realization: RealizationPreference,
) -> PackageRequest {
    PackageRequest {
        version: version_requirement(requirement),
        features: features.into_iter().map(ToOwned::to_owned).collect(),
        realization,
    }
}

fn composition(candidates: &[PackageCandidate]) -> CompositionSpec {
    let profile_source = source_id("resolver-profile");
    let profile_hash = CanonicalHash::digest(b"resolver-profile");
    CompositionSpec {
        schema_version: COMPOSITION_SCHEMA_VERSION,
        profile: parsed("test:profile/resolver"),
        profile_kind: ProfileKind::HeadlessTest,
        projection_domains: authoritative_domains(),
        roots: BTreeMap::new(),
        capabilities: BTreeMap::new(),
        features: BTreeMap::new(),
        parameters: BTreeMap::new(),
        semantic_bindings: BTreeMap::new(),
        overlays: Vec::new(),
        sources: candidates
            .iter()
            .map(|candidate| candidate.source.clone())
            .collect(),
        policy: CompositionPolicy {
            target: parsed(HOST_TARGET),
            realization_order: vec![
                RealizationKind::Data,
                RealizationKind::NativeStatic,
                RealizationKind::PortableNative,
                RealizationKind::EngineCoupledNative,
            ],
            namespace_grants: BTreeMap::new(),
            maximum_trust: TrustClass::TrustedNative,
            allow_force_override: false,
            allow_recovery: false,
            evaluation_policy: parsed(R0_NICKEL_EVALUATION_POLICY),
            evaluation_limits: NickelEvaluationLimits::default(),
        },
        provenance: provenance(&profile_source, "profiles/resolver.ncl", profile_hash),
    }
}

fn root(composition: &mut CompositionSpec, package: &str, requirement: &str) {
    composition.roots.insert(
        package_name(package),
        package_request(requirement, [], RealizationPreference::Auto),
    );
}

fn resolve(
    candidates: Vec<PackageCandidate>,
    composition: &CompositionSpec,
) -> PackageResolutionV1 {
    let resolver = succeeded(PackageResolver::new(candidates));
    succeeded(resolver.resolve(composition))
}

type ReceiptMutation = fn(&mut ResolutionReceiptV1);

fn rehash_receipt(receipt: &mut ResolutionReceiptV1) {
    receipt.resolution_hash = succeeded(receipt.recompute_resolution_hash());
    receipt.receipt_hash = succeeded(receipt.recompute_receipt_hash());
}

fn resolved_package<'a>(
    resolution: &'a PackageResolutionV1,
    package: &str,
) -> &'a latticeaxiom_packages::ResolvedPackageV1 {
    let name = package_name(package);
    resolution
        .receipt
        .packages
        .get(&name)
        .unwrap_or_else(|| panic!("resolved package `{package}` was absent"))
}
fn resolved_providers(
    resolution: &PackageResolutionV1,
    capability: &CapabilityId,
) -> Vec<PackageName> {
    resolution
        .receipt
        .capabilities
        .get(capability)
        .map_or_else(Vec::new, |receipt| {
            receipt
                .providers
                .iter()
                .map(|provider| provider.package.clone())
                .collect()
        })
}

fn add_feature(candidate: &mut PackageCandidate, feature: &str, default: bool) {
    candidate.package.features.insert(
        feature.to_owned(),
        FeatureSpec {
            default,
            domains: authoritative_domains(),
        },
    );
}

fn dependency(
    requirement: &str,
    optional: bool,
    when_features: impl IntoIterator<Item = &'static str>,
    features: impl IntoIterator<Item = &'static str>,
) -> PackageDependency {
    PackageDependency {
        version: version_requirement(requirement),
        optional,
        when_features: when_features.into_iter().map(ToOwned::to_owned).collect(),
        features: features.into_iter().map(ToOwned::to_owned).collect(),
        domains: authoritative_domains(),
    }
}

fn provide(
    candidate: &mut PackageCandidate,
    capability: &CapabilityId,
    provided_version: &str,
    cardinality: CapabilityCardinality,
) {
    candidate.package.provides.insert(
        capability.clone(),
        CapabilityProvision {
            capability: capability.clone(),
            version: version(provided_version),
            cardinality,
            domains: authoritative_domains(),
        },
    );
}

fn capability_requirement(
    capability: &CapabilityId,
    requirement: &str,
    provider: Option<&str>,
    cardinality: CapabilityCardinality,
) -> CapabilityRequirement {
    CapabilityRequirement {
        capability: capability.clone(),
        version: version_requirement(requirement),
        provider: provider.map(package_name),
        cardinality,
        domains: authoritative_domains(),
    }
}
fn require(
    candidate: &mut PackageCandidate,
    capability: &CapabilityId,
    requirement: &str,
    provider: Option<&str>,
    cardinality: CapabilityCardinality,
) {
    candidate.package.requires.insert(
        capability.clone(),
        capability_requirement(capability, requirement, provider, cardinality),
    );
}

#[test]
fn selects_highest_compatible_version_and_applies_strict_prerelease_matching() {
    let stable_candidates = vec![
        candidate(
            "terrain",
            "1.2.0",
            "terrain-1-2",
            0,
            PackageSourceKind::Workspace,
        ),
        candidate(
            "terrain",
            "2.0.0",
            "terrain-2",
            100,
            PackageSourceKind::Workspace,
        ),
        candidate(
            "terrain",
            "1.9.3",
            "terrain-1-9",
            -10,
            PackageSourceKind::LocalPrebuilt,
        ),
    ];
    let mut stable_composition = composition(&stable_candidates);
    root(&mut stable_composition, "terrain", ">=1.0.0 <2.0.0");
    let stable = resolve(stable_candidates, &stable_composition);
    assert_eq!(
        resolved_package(&stable, "terrain").version,
        version("1.9.3")
    );

    let prerelease_candidates = vec![
        candidate(
            "terrain-preview",
            "1.2.0-beta.1",
            "terrain-preview-other-core",
            100,
            PackageSourceKind::Workspace,
        ),
        candidate(
            "terrain-preview",
            "1.1.0-beta.2",
            "terrain-preview-compatible",
            0,
            PackageSourceKind::Workspace,
        ),
        candidate(
            "terrain-preview",
            "1.1.0-alpha.9",
            "terrain-preview-lower",
            0,
            PackageSourceKind::Workspace,
        ),
    ];
    let mut prerelease_composition = composition(&prerelease_candidates);
    root(
        &mut prerelease_composition,
        "terrain-preview",
        ">=1.1.0-beta.1 <2.0.0",
    );
    let prerelease = resolve(prerelease_candidates, &prerelease_composition);
    assert_eq!(
        resolved_package(&prerelease, "terrain-preview").version,
        version("1.1.0-beta.2")
    );
}

#[test]
fn source_tie_break_is_priority_then_kind_then_source_id() {
    let priority_candidates = vec![
        candidate(
            "priority-order",
            "1.0.0",
            "priority-low-workspace",
            0,
            PackageSourceKind::Workspace,
        ),
        candidate(
            "priority-order",
            "1.0.0",
            "priority-high-prebuilt",
            5,
            PackageSourceKind::LocalPrebuilt,
        ),
    ];
    let mut priority_composition = composition(&priority_candidates);
    root(&mut priority_composition, "priority-order", "=1.0.0");
    let priority = resolve(priority_candidates, &priority_composition);
    assert_eq!(
        resolved_package(&priority, "priority-order").source_id,
        source_id("priority-high-prebuilt")
    );

    let kind_candidates = vec![
        candidate(
            "kind-order",
            "1.0.0",
            "z-workspace",
            0,
            PackageSourceKind::Workspace,
        ),
        candidate(
            "kind-order",
            "1.0.0",
            "a-prebuilt",
            0,
            PackageSourceKind::LocalPrebuilt,
        ),
    ];
    let mut kind_composition = composition(&kind_candidates);
    root(&mut kind_composition, "kind-order", "=1.0.0");
    let kind = resolve(kind_candidates, &kind_composition);
    assert_eq!(
        resolved_package(&kind, "kind-order").source_id,
        source_id("z-workspace")
    );

    let id_candidates = vec![
        candidate(
            "id-order",
            "1.0.0",
            "z-source",
            0,
            PackageSourceKind::LocalDirectory,
        ),
        candidate(
            "id-order",
            "1.0.0",
            "a-source",
            0,
            PackageSourceKind::LocalDirectory,
        ),
    ];
    let mut id_composition = composition(&id_candidates);
    root(&mut id_composition, "id-order", "=1.0.0");
    let id = resolve(id_candidates, &id_composition);
    assert_eq!(
        resolved_package(&id, "id-order").source_id,
        source_id("a-source")
    );
}

#[test]
#[allow(clippy::too_many_lines)]
fn default_profile_root_and_forwarded_features_activate_optional_dependencies() {
    let mut default_root = candidate(
        "default-root",
        "1.0.0",
        "default-root",
        0,
        PackageSourceKind::Workspace,
    );
    add_feature(&mut default_root, "default-on", true);
    default_root.package.dependencies.insert(
        package_name("default-extra"),
        dependency("=1.0.0", true, ["default-on"], []),
    );

    let mut profile_root = candidate(
        "profile-root",
        "1.0.0",
        "profile-root",
        0,
        PackageSourceKind::Workspace,
    );
    add_feature(&mut profile_root, "profile-on", false);
    profile_root.package.dependencies.insert(
        package_name("profile-extra"),
        dependency("=1.0.0", true, ["profile-on"], []),
    );

    let mut selected_root = candidate(
        "selected-root",
        "1.0.0",
        "selected-root",
        0,
        PackageSourceKind::Workspace,
    );
    add_feature(&mut selected_root, "root-on", false);
    selected_root.package.dependencies.insert(
        package_name("root-extra"),
        dependency("=1.0.0", true, ["root-on"], []),
    );

    let mut forwarding_root = candidate(
        "forwarding-root",
        "1.0.0",
        "forwarding-root",
        0,
        PackageSourceKind::Workspace,
    );
    forwarding_root.package.dependencies.insert(
        package_name("forwarded-child"),
        dependency("=1.0.0", false, [], ["forwarded-on"]),
    );

    let mut forwarded_child = candidate(
        "forwarded-child",
        "1.0.0",
        "forwarded-child",
        0,
        PackageSourceKind::Workspace,
    );
    add_feature(&mut forwarded_child, "forwarded-on", false);
    forwarded_child.package.dependencies.insert(
        package_name("forwarded-extra"),
        dependency("=1.0.0", true, ["forwarded-on"], []),
    );

    let candidates = vec![
        default_root,
        profile_root,
        selected_root,
        forwarding_root,
        forwarded_child,
        candidate(
            "default-extra",
            "1.0.0",
            "default-extra",
            0,
            PackageSourceKind::Workspace,
        ),
        candidate(
            "profile-extra",
            "1.0.0",
            "profile-extra",
            0,
            PackageSourceKind::Workspace,
        ),
        candidate(
            "root-extra",
            "1.0.0",
            "root-extra",
            0,
            PackageSourceKind::Workspace,
        ),
        candidate(
            "forwarded-extra",
            "1.0.0",
            "forwarded-extra",
            0,
            PackageSourceKind::Workspace,
        ),
    ];
    let mut spec = composition(&candidates);
    root(&mut spec, "default-root", "=1.0.0");
    root(&mut spec, "profile-root", "=1.0.0");
    root(&mut spec, "forwarding-root", "=1.0.0");
    spec.roots.insert(
        package_name("selected-root"),
        package_request("=1.0.0", ["root-on"], RealizationPreference::Auto),
    );
    spec.features.insert(
        package_name("profile-root"),
        BTreeSet::from(["profile-on".to_owned()]),
    );

    let resolution = resolve(candidates, &spec);
    assert_eq!(resolution.receipt.packages.len(), 9);
    for package in [
        "default-extra",
        "profile-extra",
        "root-extra",
        "forwarded-extra",
    ] {
        assert!(
            resolution
                .receipt
                .packages
                .contains_key(&package_name(package))
        );
    }
    assert_eq!(
        resolved_package(&resolution, "default-root").features,
        BTreeSet::from(["default-on".to_owned()])
    );
    assert_eq!(
        resolved_package(&resolution, "profile-root").features,
        BTreeSet::from(["profile-on".to_owned()])
    );
    assert_eq!(
        resolved_package(&resolution, "selected-root").features,
        BTreeSet::from(["root-on".to_owned()])
    );
    assert_eq!(
        resolved_package(&resolution, "forwarded-child").features,
        BTreeSet::from(["forwarded-on".to_owned()])
    );
}

#[test]
fn reports_missing_packages_version_conflicts_and_active_cycles() {
    let mut missing_root = candidate(
        "missing-root",
        "1.0.0",
        "missing-root",
        0,
        PackageSourceKind::Workspace,
    );
    missing_root.package.dependencies.insert(
        package_name("absent-package"),
        dependency("=1.0.0", false, [], []),
    );
    let missing_candidates = vec![missing_root];
    let mut missing_spec = composition(&missing_candidates);
    root(&mut missing_spec, "missing-root", "=1.0.0");
    let missing_resolver = succeeded(PackageResolver::new(missing_candidates));
    assert!(matches!(
        missing_resolver.resolve(&missing_spec),
        Err(ResolutionError::MissingPackage { package, .. })
            if package == package_name("absent-package")
    ));

    let mut version_root = candidate(
        "version-root",
        "1.0.0",
        "version-root",
        0,
        PackageSourceKind::Workspace,
    );
    version_root.package.dependencies.insert(
        package_name("versioned-dependency"),
        dependency(">=2.0.0 <3.0.0", false, [], []),
    );
    let version_candidates = vec![
        version_root,
        candidate(
            "versioned-dependency",
            "1.9.9",
            "versioned-dependency",
            0,
            PackageSourceKind::Workspace,
        ),
    ];
    let mut version_spec = composition(&version_candidates);
    root(&mut version_spec, "version-root", "=1.0.0");
    let version_resolver = succeeded(PackageResolver::new(version_candidates));
    assert!(matches!(
        version_resolver.resolve(&version_spec),
        Err(ResolutionError::VersionConflict { package, .. })
            if package == package_name("versioned-dependency")
    ));

    let mut cycle_a = candidate(
        "cycle-a",
        "1.0.0",
        "cycle-a",
        0,
        PackageSourceKind::Workspace,
    );
    cycle_a
        .package
        .dependencies
        .insert(package_name("cycle-b"), dependency("=1.0.0", false, [], []));
    let mut cycle_b = candidate(
        "cycle-b",
        "1.0.0",
        "cycle-b",
        0,
        PackageSourceKind::Workspace,
    );
    cycle_b
        .package
        .dependencies
        .insert(package_name("cycle-a"), dependency("=1.0.0", false, [], []));
    let cycle_candidates = vec![cycle_a, cycle_b];
    let mut cycle_spec = composition(&cycle_candidates);
    root(&mut cycle_spec, "cycle-a", "=1.0.0");
    let cycle_resolver = succeeded(PackageResolver::new(cycle_candidates));
    assert!(matches!(
        cycle_resolver.resolve(&cycle_spec),
        Err(ResolutionError::DependencyCycle { context })
            if context.package_chain
                == vec![
                    package_name("cycle-a"),
                    package_name("cycle-b"),
                    package_name("cycle-a"),
                ]
    ));
}

#[test]
fn exactly_one_capability_honors_explicit_provider_and_rejects_two_selected_providers() {
    let capability = capability_id("terrain-generator");
    let mut low = candidate(
        "provider-low",
        "1.0.0",
        "provider-low",
        0,
        PackageSourceKind::Workspace,
    );
    provide(
        &mut low,
        &capability,
        "1.1.0",
        CapabilityCardinality::ExactlyOne,
    );
    let mut high = candidate(
        "provider-high",
        "2.0.0",
        "provider-high",
        0,
        PackageSourceKind::Workspace,
    );
    provide(
        &mut high,
        &capability,
        "1.2.0",
        CapabilityCardinality::ExactlyOne,
    );
    let candidates = vec![high, low];

    let mut explicit = composition(&candidates);
    explicit.capabilities.insert(
        capability.clone(),
        capability_requirement(
            &capability,
            ">=1.0.0 <2.0.0",
            Some("provider-low"),
            CapabilityCardinality::ExactlyOne,
        ),
    );
    root(&mut explicit, "provider-low", "=1.0.0");
    let resolution = resolve(candidates.clone(), &explicit);
    assert_eq!(
        resolved_providers(&resolution, &capability),
        vec![package_name("provider-low")]
    );
    assert!(
        resolution
            .receipt
            .packages
            .contains_key(&package_name("provider-low"))
    );
    assert!(
        !resolution
            .receipt
            .packages
            .contains_key(&package_name("provider-high"))
    );

    let mut conflict = composition(&candidates);
    conflict.capabilities.insert(
        capability.clone(),
        capability_requirement(
            &capability,
            ">=1.0.0 <2.0.0",
            None,
            CapabilityCardinality::ExactlyOne,
        ),
    );
    root(&mut conflict, "provider-low", "=1.0.0");
    root(&mut conflict, "provider-high", "=2.0.0");
    let resolver = succeeded(PackageResolver::new(candidates));
    assert!(matches!(
        resolver.resolve(&conflict),
        Err(ResolutionError::ConflictingCapabilityProviders {
            capability: failed,
            ..
        }) if failed == capability
    ));
}

#[test]
fn one_or_more_capability_keeps_all_selected_providers_and_rejects_cardinality_mismatch() {
    let capability = capability_id("world-decoration");
    let mut provider_a = candidate(
        "decoration-a",
        "1.0.0",
        "decoration-a",
        0,
        PackageSourceKind::Workspace,
    );
    provide(
        &mut provider_a,
        &capability,
        "1.0.0",
        CapabilityCardinality::OneOrMore,
    );
    let mut provider_b = candidate(
        "decoration-b",
        "2.0.0",
        "decoration-b",
        0,
        PackageSourceKind::Workspace,
    );
    provide(
        &mut provider_b,
        &capability,
        "1.1.0",
        CapabilityCardinality::OneOrMore,
    );
    let candidates = vec![provider_a.clone(), provider_b.clone()];
    let mut spec = composition(&candidates);
    spec.capabilities.insert(
        capability.clone(),
        capability_requirement(
            &capability,
            ">=1.0.0 <2.0.0",
            None,
            CapabilityCardinality::OneOrMore,
        ),
    );
    root(&mut spec, "decoration-a", "=1.0.0");
    root(&mut spec, "decoration-b", "=2.0.0");
    let resolution = resolve(candidates, &spec);
    assert_eq!(
        resolved_providers(&resolution, &capability),
        vec![package_name("decoration-b"), package_name("decoration-a"),]
    );

    let mut incompatible = provider_a;
    incompatible.package.provides.clear();
    provide(
        &mut incompatible,
        &capability,
        "1.0.0",
        CapabilityCardinality::ExactlyOne,
    );
    let mismatch_candidates = vec![incompatible];
    let mut mismatch_spec = composition(&mismatch_candidates);
    mismatch_spec.capabilities.insert(
        capability.clone(),
        capability_requirement(
            &capability,
            "=1.0.0",
            None,
            CapabilityCardinality::OneOrMore,
        ),
    );
    root(&mut mismatch_spec, "decoration-a", "=1.0.0");
    let resolver = succeeded(PackageResolver::new(mismatch_candidates));
    assert!(matches!(
        resolver.resolve(&mismatch_spec),
        Err(ResolutionError::CapabilityCardinalityConflict {
            capability: failed,
            required: CapabilityCardinality::OneOrMore,
            ..
        }) if failed == capability
    ));
}

#[test]
#[allow(clippy::too_many_lines)]
fn realization_selection_obeys_auto_and_explicit_kind_target_and_trust() {
    let mut multi = candidate(
        "multi-realization",
        "1.0.0",
        "multi-realization",
        0,
        PackageSourceKind::Workspace,
    );
    let data = data_realization("data-choice");
    let native = native_realization("native-choice", HOST_TARGET, TrustClass::Build);
    multi.package.realizations =
        BTreeMap::from([(data.id.clone(), data), (native.id.clone(), native)]);
    multi.package.trust = TrustClass::Build;
    let candidates = vec![multi];

    let mut automatic = composition(&candidates);
    automatic.policy.realization_order = vec![RealizationKind::NativeStatic, RealizationKind::Data];
    root(&mut automatic, "multi-realization", "=1.0.0");
    let automatic_resolution = resolve(candidates.clone(), &automatic);
    assert_eq!(
        resolved_package(&automatic_resolution, "multi-realization")
            .realization
            .id,
        succeeded(RealizationId::new("native-choice"))
    );

    let mut explicit = automatic.clone();
    explicit.roots.insert(
        package_name("multi-realization"),
        package_request(
            "=1.0.0",
            [],
            RealizationPreference::Exact(RealizationKind::Data),
        ),
    );
    let explicit_resolution = resolve(candidates, &explicit);
    assert_eq!(
        resolved_package(&explicit_resolution, "multi-realization")
            .realization
            .id,
        succeeded(RealizationId::new("data-choice"))
    );

    let mut wrong_target = candidate(
        "target-sensitive",
        "2.0.0",
        "target-sensitive-wrong",
        0,
        PackageSourceKind::Workspace,
    );
    let wrong = native_realization(
        "native-wrong-target",
        "aarch64-unknown-linux-gnu",
        TrustClass::Build,
    );
    wrong_target.package.realizations = BTreeMap::from([(wrong.id.clone(), wrong)]);
    wrong_target.package.trust = TrustClass::Build;
    let mut correct_target = candidate(
        "target-sensitive",
        "1.0.0",
        "target-sensitive-correct",
        0,
        PackageSourceKind::Workspace,
    );
    let correct = native_realization("native-correct-target", HOST_TARGET, TrustClass::Build);
    correct_target.package.realizations = BTreeMap::from([(correct.id.clone(), correct)]);
    correct_target.package.trust = TrustClass::Build;
    let target_candidates = vec![wrong_target, correct_target];
    let mut target_spec = composition(&target_candidates);
    target_spec.policy.realization_order = vec![RealizationKind::NativeStatic];
    root(&mut target_spec, "target-sensitive", ">=1.0.0 <3.0.0");
    let target_resolution = resolve(target_candidates, &target_spec);
    assert_eq!(
        resolved_package(&target_resolution, "target-sensitive").version,
        version("1.0.0")
    );
    assert!(target_resolution.receipt.explanation.decisions.iter().any(|decision| {
        matches!(
            (&decision.subject, decision.outcome, decision.reasons.as_slice()),
            (
                ResolutionSubjectV1::Realization { realization, .. },
                ResolutionOutcomeV1::Discarded,
                reasons,
            ) if realization.as_str() == "native-wrong-target"
                && reasons.iter().any(|reason| matches!(
                    reason,
                    ResolutionReasonV1::TargetMismatch { target } if target.as_str() == HOST_TARGET
                ))
        )
    }));

    let trusted = candidate(
        "trust-sensitive",
        "1.0.0",
        "trust-sensitive-data",
        0,
        PackageSourceKind::Workspace,
    );
    let mut untrusted = candidate(
        "trust-sensitive",
        "2.0.0",
        "trust-sensitive-native",
        0,
        PackageSourceKind::Workspace,
    );
    let privileged = native_realization("trusted-native", HOST_TARGET, TrustClass::TrustedNative);
    untrusted.package.realizations = BTreeMap::from([(privileged.id.clone(), privileged)]);
    untrusted.package.trust = TrustClass::TrustedNative;
    let trust_candidates = vec![untrusted, trusted];
    let mut trust_spec = composition(&trust_candidates);
    trust_spec.policy.maximum_trust = TrustClass::DataOnly;
    root(&mut trust_spec, "trust-sensitive", ">=1.0.0 <3.0.0");
    let trust_resolution = resolve(trust_candidates, &trust_spec);
    assert_eq!(
        resolved_package(&trust_resolution, "trust-sensitive").version,
        version("1.0.0")
    );
    assert!(
        trust_resolution
            .receipt
            .explanation
            .decisions
            .iter()
            .any(|decision| {
                matches!(
                    (&decision.subject, decision.outcome),
                    (
                        ResolutionSubjectV1::Package { candidate },
                        ResolutionOutcomeV1::Discarded,
                    ) if candidate.version == version("2.0.0")
                ) && decision.reasons.iter().any(|reason| {
                    matches!(
                        reason,
                        ResolutionReasonV1::TrustExceeded {
                            required: TrustClass::TrustedNative,
                            allowed: TrustClass::DataOnly,
                        }
                    )
                })
            })
    );
}

fn shuffled<T>(mut values: Vec<T>, seed: u64) -> Vec<T> {
    let mut state = seed.wrapping_add(0x9e37_79b9_7f4a_7c15);
    for index in (1..values.len()).rev() {
        state = state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        let modulus = succeeded(u64::try_from(index + 1));
        let swap_with = succeeded(usize::try_from(state % modulus));
        values.swap(index, swap_with);
    }
    values
}

fn determinism_candidates(reverse_dependencies: bool) -> Vec<PackageCandidate> {
    let mut root_high = candidate(
        "deterministic-root",
        "1.1.0",
        "deterministic-root-high",
        0,
        PackageSourceKind::Workspace,
    );
    let dependency_rows = [
        (
            package_name("deterministic-a"),
            dependency("=1.0.0", false, [], ["forwarded-a"]),
        ),
        (
            package_name("deterministic-b"),
            dependency("=1.0.0", false, [], []),
        ),
    ];
    if reverse_dependencies {
        for (name, dependency) in dependency_rows.into_iter().rev() {
            root_high.package.dependencies.insert(name, dependency);
        }
    } else {
        for (name, dependency) in dependency_rows {
            root_high.package.dependencies.insert(name, dependency);
        }
    }

    let mut dependency_a = candidate(
        "deterministic-a",
        "1.0.0",
        "deterministic-a",
        0,
        PackageSourceKind::Workspace,
    );
    add_feature(&mut dependency_a, "forwarded-a", false);

    vec![
        root_high,
        candidate(
            "deterministic-root",
            "1.0.0",
            "deterministic-root-low",
            100,
            PackageSourceKind::LocalPrebuilt,
        ),
        dependency_a,
        candidate(
            "deterministic-b",
            "1.0.0",
            "deterministic-b",
            0,
            PackageSourceKind::LocalDirectory,
        ),
        candidate(
            "unselected-noise",
            "9.9.9",
            "unselected-noise",
            99,
            PackageSourceKind::Workspace,
        ),
    ]
}

#[test]
fn candidate_shuffle_and_dependency_order_produce_byte_identical_receipts() {
    let baseline_candidates = determinism_candidates(false);
    let mut baseline_spec = composition(&baseline_candidates);
    root(&mut baseline_spec, "deterministic-root", ">=1.0.0 <2.0.0");
    let baseline = resolve(baseline_candidates, &baseline_spec);
    let baseline_receipt = succeeded(baseline.receipt.canonical_bytes());
    let baseline_plan = succeeded(baseline.build_intent.canonical_bytes());

    for seed in 0..40 {
        let candidates = shuffled(determinism_candidates(seed % 2 == 1), seed);
        let mut spec = composition(&candidates);
        spec.sources = shuffled(spec.sources, seed.rotate_left(17));
        root(&mut spec, "deterministic-root", ">=1.0.0 <2.0.0");
        let resolution = resolve(candidates, &spec);
        assert_eq!(
            succeeded(resolution.receipt.canonical_bytes()),
            baseline_receipt,
            "receipt bytes changed for shuffle seed {seed}"
        );
        assert_eq!(
            succeeded(resolution.build_intent.canonical_bytes()),
            baseline_plan,
            "plan bytes changed for shuffle seed {seed}"
        );
    }
}

#[test]
fn frozen_resolution_reuses_exact_receipt_and_rejects_tampering() {
    let candidates = vec![candidate(
        "frozen-root",
        "1.0.0",
        "frozen-root",
        0,
        PackageSourceKind::Workspace,
    )];
    let mut spec = composition(&candidates);
    root(&mut spec, "frozen-root", "=1.0.0");
    let resolver = succeeded(PackageResolver::new(candidates));
    let initial = succeeded(resolver.resolve(&spec));
    let frozen_bytes = succeeded(initial.receipt.canonical_bytes());
    let reused = succeeded(resolver.resolve_frozen(&spec, &initial.receipt));
    assert_eq!(succeeded(reused.receipt.canonical_bytes()), frozen_bytes);

    let mut receipt_tampered = initial.receipt.clone();
    let package = receipt_tampered
        .packages
        .get_mut(&package_name("frozen-root"))
        .unwrap_or_else(|| panic!("frozen root package was absent"));
    let tampered_source_id = package.source_id.clone();
    let tampered_source_hash = CanonicalHash::digest(b"tampered-source-receipt");
    package.source_hash = tampered_source_hash;
    for decision in &mut receipt_tampered.explanation.decisions {
        let candidate = match &mut decision.subject {
            ResolutionSubjectV1::Package { candidate }
            | ResolutionSubjectV1::Realization { candidate, .. }
            | ResolutionSubjectV1::CapabilityProvider { candidate, .. } => candidate,
        };
        if candidate.source_id == tampered_source_id {
            candidate.source_hash = tampered_source_hash;
        }
    }
    receipt_tampered.receipt_hash = succeeded(receipt_tampered.recompute_receipt_hash());
    succeeded(receipt_tampered.validate());
    assert!(matches!(
        resolver.resolve_frozen(&spec, &receipt_tampered),
        Err(ResolutionError::ResolutionReceiptMismatch { .. })
    ));

    let mut hash_tampered = initial.receipt;
    hash_tampered.receipt_hash = CanonicalHash::digest(b"tampered-receipt-hash");
    assert!(matches!(
        resolver.resolve_frozen(&spec, &hash_tampered),
        Err(ResolutionError::InvalidResolutionReceipt { .. })
    ));
}
#[test]
fn receipt_validation_rejects_empty_roots_after_hash_recomputation() {
    let candidates = vec![candidate(
        "empty-root-tamper",
        "1.0.0",
        "empty-root-tamper",
        0,
        PackageSourceKind::Workspace,
    )];
    let mut spec = composition(&candidates);
    root(&mut spec, "empty-root-tamper", "=1.0.0");
    let resolver = succeeded(PackageResolver::new(candidates));
    let initial = succeeded(resolver.resolve(&spec));

    let mut tampered = initial.receipt;
    tampered.roots.clear();
    rehash_receipt(&mut tampered);
    assert!(matches!(
        tampered.validate(),
        Err(ResolutionReceiptError::InvalidStructure { .. })
    ));
    assert!(matches!(
        resolver.resolve_frozen(&spec, &tampered),
        Err(ResolutionError::InvalidResolutionReceipt { .. })
    ));
}

#[test]
fn receipt_validation_rejects_a_selected_package_marked_discarded_after_rehash() {
    let candidates = vec![candidate(
        "explanation-tamper",
        "1.0.0",
        "explanation-tamper",
        0,
        PackageSourceKind::Workspace,
    )];
    let mut spec = composition(&candidates);
    root(&mut spec, "explanation-tamper", "=1.0.0");
    let resolver = succeeded(PackageResolver::new(candidates));
    let initial = succeeded(resolver.resolve(&spec));

    let mut tampered = initial.receipt;
    let decision = tampered
        .explanation
        .decisions
        .iter_mut()
        .find(|decision| {
            matches!(
                &decision.subject,
                ResolutionSubjectV1::Package { candidate }
                    if candidate.package == package_name("explanation-tamper")
                        && decision.outcome == ResolutionOutcomeV1::Selected
            )
        })
        .unwrap_or_else(|| panic!("selected package explanation row was absent"));
    decision.outcome = ResolutionOutcomeV1::Discarded;
    rehash_receipt(&mut tampered);
    assert!(matches!(
        tampered.validate(),
        Err(ResolutionReceiptError::InvalidStructure { .. })
    ));
    assert!(matches!(
        resolver.resolve_frozen(&spec, &tampered),
        Err(ResolutionError::InvalidResolutionReceipt { .. })
    ));
}

#[test]
#[allow(clippy::too_many_lines)]
fn frozen_replay_rejects_rehashed_source_independent_header_tampering() {
    let mut root_candidate = candidate(
        "header-root",
        "1.0.0",
        "header-root",
        0,
        PackageSourceKind::Workspace,
    );
    root_candidate.package.dependencies.insert(
        package_name("header-child"),
        dependency("=1.0.0", false, [], []),
    );
    let child_candidate = candidate(
        "header-child",
        "1.0.0",
        "header-child",
        0,
        PackageSourceKind::Workspace,
    );
    let candidates = vec![root_candidate, child_candidate];
    let mut spec = composition(&candidates);
    root(&mut spec, "header-root", "=1.0.0");
    let resolver = succeeded(PackageResolver::new(candidates));
    let initial = succeeded(resolver.resolve(&spec));

    let mutations: [(&str, ReceiptMutation); 7] = [
        ("resolution-intent-hash", |receipt| {
            receipt.resolution_intent_hash = CanonicalHash::digest(b"tampered-resolution-intent");
        }),
        ("profile", |receipt| {
            receipt.profile = parsed("test:profile/tampered");
        }),
        ("profile-kind", |receipt| {
            receipt.profile_kind = ProfileKind::Tool;
        }),
        ("target", |receipt| {
            receipt.target = parsed("aarch64-unknown-linux-gnu");
            let target = receipt.target.clone();
            for package in receipt.packages.values_mut() {
                package.realization.target = target.clone();
            }
        }),
        ("evaluation-policy", |receipt| {
            receipt.evaluation_policy = parsed("test:nickel-evaluation-policy/tampered@1");
        }),
        ("evaluation-limits", |receipt| {
            receipt.evaluation_limits.wall_clock_ms += 1;
        }),
        ("roots", |receipt| {
            let child = package_name("header-child");
            receipt.roots.insert(child.clone());
            let decision = receipt
                .explanation
                .decisions
                .iter_mut()
                .find(|decision| {
                    matches!(
                        &decision.subject,
                        ResolutionSubjectV1::Package { candidate }
                            if candidate.package == child
                                && decision.outcome == ResolutionOutcomeV1::Selected
                    )
                })
                .unwrap_or_else(|| panic!("selected child explanation row was absent"));
            decision.reasons.push(ResolutionReasonV1::Root);
        }),
    ];

    for (name, mutate) in mutations {
        let mut tampered = initial.receipt.clone();
        mutate(&mut tampered);
        rehash_receipt(&mut tampered);
        succeeded(tampered.validate());
        assert!(
            matches!(
                resolver.resolve_frozen(&spec, &tampered),
                Err(ResolutionError::ResolutionReceiptMismatch { .. })
            ),
            "frozen replay accepted tampered {name}"
        );
    }
}

fn assert_budget_exceeded(
    result: Result<PackageResolutionV1, ResolutionError>,
    expected_budget: ResolutionBudget,
    expected_limit: u64,
    expected_observed: u64,
) {
    match result {
        Err(ResolutionError::BudgetExceeded {
            budget,
            limit,
            observed,
            ..
        }) => {
            assert_eq!(budget, expected_budget);
            assert_eq!(limit, expected_limit);
            assert_eq!(observed, expected_observed);
        }
        other => panic!("expected budget failure, got {other:?}"),
    }
}

#[test]
fn provider_source_oscillation_terminates_without_exhausting_limits() {
    let capability_a = capability_id("oscillation-a");
    let capability_b = capability_id("oscillation-b");
    let mut source_a = candidate(
        "dual-provider",
        "1.0.0",
        "dual-provider-a",
        0,
        PackageSourceKind::Workspace,
    );
    provide(
        &mut source_a,
        &capability_a,
        "1.0.0",
        CapabilityCardinality::ExactlyOne,
    );
    let mut source_b = candidate(
        "dual-provider",
        "1.0.0",
        "dual-provider-b",
        0,
        PackageSourceKind::Workspace,
    );
    provide(
        &mut source_b,
        &capability_b,
        "1.0.0",
        CapabilityCardinality::ExactlyOne,
    );
    let candidates = vec![source_a, source_b];
    let mut spec = composition(&candidates);
    spec.capabilities.insert(
        capability_a.clone(),
        capability_requirement(
            &capability_a,
            "^1.0.0",
            None,
            CapabilityCardinality::ExactlyOne,
        ),
    );
    spec.capabilities.insert(
        capability_b.clone(),
        capability_requirement(
            &capability_b,
            "^1.0.0",
            None,
            CapabilityCardinality::ExactlyOne,
        ),
    );
    let resolver = succeeded(PackageResolver::new(candidates));
    let limits = ResolutionLimits {
        max_unique_states: 4,
        max_state_transitions: 4,
        max_decision_depth: 4,
        ..ResolutionLimits::default()
    };
    assert!(matches!(
        resolver.resolve_with_limits(&spec, limits),
        Err(ResolutionError::MissingCapability { capability, .. })
            if capability == capability_b
    ));
}

#[test]
fn backtracking_prunes_stale_provider_closure_and_records_real_failure() {
    let capability_x = capability_id("closure-x");
    let capability_y = capability_id("closure-y");
    let mut switch_v2 = candidate(
        "switch",
        "2.0.0",
        "switch-v2",
        0,
        PackageSourceKind::Workspace,
    );
    require(
        &mut switch_v2,
        &capability_x,
        "^1.0.0",
        None,
        CapabilityCardinality::ExactlyOne,
    );
    let mut switch_v1 = candidate(
        "switch",
        "1.0.0",
        "switch-v1",
        0,
        PackageSourceKind::Workspace,
    );
    provide(
        &mut switch_v1,
        &capability_y,
        "1.0.0",
        CapabilityCardinality::ExactlyOne,
    );
    let mut bridge = candidate("bridge", "1.0.0", "bridge", 0, PackageSourceKind::Workspace);
    provide(
        &mut bridge,
        &capability_x,
        "1.0.0",
        CapabilityCardinality::ExactlyOne,
    );
    require(
        &mut bridge,
        &capability_y,
        "^1.0.0",
        None,
        CapabilityCardinality::ExactlyOne,
    );
    let candidates = vec![switch_v2.clone(), switch_v1, bridge];
    let mut spec = composition(&candidates);
    root(&mut spec, "switch", ">=1.0.0 <3.0.0");
    let resolution = resolve(candidates, &spec);
    assert_eq!(resolution.receipt.packages.len(), 1);
    assert_eq!(
        resolved_package(&resolution, "switch").version,
        version("1.0.0")
    );
    assert!(resolution.receipt.capabilities.is_empty());
    assert!(
        resolution
            .receipt
            .explanation
            .decisions
            .iter()
            .any(|decision| {
                matches!(
                    (&decision.subject, &decision.outcome),
                    (
                        ResolutionSubjectV1::Package { candidate },
                        ResolutionOutcomeV1::Discarded
                    ) if candidate.source_id == switch_v2.source.source_id
                ) && decision.reasons.iter().any(|reason| {
                    matches!(
                        reason,
                        ResolutionReasonV1::Backtracked { failure }
                            if matches!(
                                &failure.code,
                                latticeaxiom_packages::ResolutionFailureCodeV1::MissingCapability {
                                    capability
                                } if capability == &capability_y
                            )
                    )
                })
            })
    );
}

#[test]
fn explicit_provider_disambiguates_when_other_provider_is_also_rooted() {
    let capability = capability_id("rooted-explicit");
    let mut selected = candidate(
        "selected-provider",
        "1.0.0",
        "selected-provider",
        0,
        PackageSourceKind::Workspace,
    );
    let mut other = candidate(
        "other-provider",
        "2.0.0",
        "other-provider",
        0,
        PackageSourceKind::Workspace,
    );
    provide(
        &mut selected,
        &capability,
        "1.0.0",
        CapabilityCardinality::ExactlyOne,
    );
    provide(
        &mut other,
        &capability,
        "1.0.0",
        CapabilityCardinality::ExactlyOne,
    );
    let candidates = vec![selected, other];
    let mut spec = composition(&candidates);
    root(&mut spec, "selected-provider", "=1.0.0");
    root(&mut spec, "other-provider", "=2.0.0");
    spec.capabilities.insert(
        capability.clone(),
        capability_requirement(
            &capability,
            "^1.0.0",
            Some("selected-provider"),
            CapabilityCardinality::ExactlyOne,
        ),
    );
    let resolution = resolve(candidates, &spec);
    assert_eq!(resolution.receipt.packages.len(), 2);
    assert_eq!(
        resolved_providers(&resolution, &capability),
        vec![package_name("selected-provider")]
    );
}

#[test]
fn profile_feature_override_is_filtered_by_active_domains() {
    let mut root_package = candidate(
        "domain-feature-root",
        "1.0.0",
        "domain-feature-root",
        0,
        PackageSourceKind::Workspace,
    );
    root_package.package.domains.insert(PackageDomain::Client);
    root_package.package.features.insert(
        "client-only".to_owned(),
        FeatureSpec {
            default: false,
            domains: BTreeSet::from([PackageDomain::Client]),
        },
    );
    root_package.package.dependencies.insert(
        package_name("client-only-dependency"),
        dependency("=1.0.0", true, ["client-only"], []),
    );
    let dependency_candidate = candidate(
        "client-only-dependency",
        "1.0.0",
        "client-only-dependency",
        0,
        PackageSourceKind::Workspace,
    );
    let candidates = vec![root_package, dependency_candidate];
    let mut spec = composition(&candidates);
    root(&mut spec, "domain-feature-root", "=1.0.0");
    spec.features.insert(
        package_name("domain-feature-root"),
        BTreeSet::from(["client-only".to_owned()]),
    );
    let resolution = resolve(candidates, &spec);
    assert_eq!(resolution.receipt.packages.len(), 1);
    assert!(
        resolved_package(&resolution, "domain-feature-root")
            .features
            .is_empty()
    );
}

#[test]
#[allow(clippy::too_many_lines)]
fn deterministic_resolution_limits_accept_boundary_and_reject_limit_plus_one() {
    let single = candidate(
        "bounded-root",
        "1.0.0",
        "bounded-root",
        0,
        PackageSourceKind::Workspace,
    );
    let single_candidates = vec![single];
    let mut single_spec = composition(&single_candidates);
    root(&mut single_spec, "bounded-root", "=1.0.0");
    let single_resolver = succeeded(PackageResolver::new(single_candidates));
    let mut limits = ResolutionLimits {
        max_unique_states: 2,
        ..ResolutionLimits::default()
    };
    succeeded(single_resolver.resolve_with_limits(&single_spec, limits));
    limits.max_unique_states = 1;
    assert_budget_exceeded(
        single_resolver.resolve_with_limits(&single_spec, limits),
        ResolutionBudget::UniqueStates,
        1,
        2,
    );

    let mut app = candidate(
        "bounded-app",
        "1.0.0",
        "bounded-app",
        0,
        PackageSourceKind::Workspace,
    );
    app.package.dependencies.insert(
        package_name("bounded-leaf"),
        dependency("=1.0.0", false, [], []),
    );
    let leaf = candidate(
        "bounded-leaf",
        "1.0.0",
        "bounded-leaf",
        0,
        PackageSourceKind::Workspace,
    );
    let chain_candidates = vec![app, leaf];
    let mut chain_spec = composition(&chain_candidates);
    root(&mut chain_spec, "bounded-app", "=1.0.0");
    let chain_resolver = succeeded(PackageResolver::new(chain_candidates));
    let mut depth_limits = ResolutionLimits {
        max_decision_depth: 2,
        ..ResolutionLimits::default()
    };
    succeeded(chain_resolver.resolve_with_limits(&chain_spec, depth_limits));
    depth_limits.max_decision_depth = 1;
    assert_budget_exceeded(
        chain_resolver.resolve_with_limits(&chain_spec, depth_limits),
        ResolutionBudget::DecisionDepth,
        1,
        2,
    );
    let mut transition_limits = ResolutionLimits {
        max_state_transitions: 2,
        ..ResolutionLimits::default()
    };
    succeeded(chain_resolver.resolve_with_limits(&chain_spec, transition_limits));
    transition_limits.max_state_transitions = 1;
    assert_budget_exceeded(
        chain_resolver.resolve_with_limits(&chain_spec, transition_limits),
        ResolutionBudget::StateTransitions,
        1,
        2,
    );
    let mut package_limits = ResolutionLimits {
        max_live_packages: 2,
        ..ResolutionLimits::default()
    };
    succeeded(chain_resolver.resolve_with_limits(&chain_spec, package_limits));
    package_limits.max_live_packages = 1;
    assert_budget_exceeded(
        chain_resolver.resolve_with_limits(&chain_spec, package_limits),
        ResolutionBudget::LivePackages,
        1,
        2,
    );

    let choices = vec![
        candidate(
            "bounded-choice",
            "2.0.0",
            "bounded-choice-v2",
            0,
            PackageSourceKind::Workspace,
        ),
        candidate(
            "bounded-choice",
            "1.0.0",
            "bounded-choice-v1",
            0,
            PackageSourceKind::Workspace,
        ),
    ];
    let mut choice_spec = composition(&choices);
    root(&mut choice_spec, "bounded-choice", ">=1.0.0 <3.0.0");
    let choice_resolver = succeeded(PackageResolver::new(choices));
    let mut candidate_limits = ResolutionLimits {
        max_candidates_per_decision: 2,
        ..ResolutionLimits::default()
    };
    succeeded(choice_resolver.resolve_with_limits(&choice_spec, candidate_limits));
    candidate_limits.max_candidates_per_decision = 1;
    assert_budget_exceeded(
        choice_resolver.resolve_with_limits(&choice_spec, candidate_limits),
        ResolutionBudget::CandidatesPerDecision,
        1,
        2,
    );
    let mut source_limits = ResolutionLimits {
        max_source_candidates: 2,
        ..ResolutionLimits::default()
    };
    succeeded(choice_resolver.resolve_with_limits(&choice_spec, source_limits));
    source_limits.max_source_candidates = 1;
    assert_budget_exceeded(
        choice_resolver.resolve_with_limits(&choice_spec, source_limits),
        ResolutionBudget::SourceCandidates,
        1,
        2,
    );
}

#[test]
#[allow(clippy::too_many_lines)]
fn frozen_replay_ignores_new_versions_sources_and_irrelevant_catalog_rows() {
    let original = candidate(
        "frozen-universe",
        "1.0.0",
        "frozen-universe-v1",
        0,
        PackageSourceKind::Workspace,
    );
    let original_candidates = vec![original.clone()];
    let mut original_spec = composition(&original_candidates);
    root(&mut original_spec, "frozen-universe", ">=1.0.0 <3.0.0");
    let original_resolver = succeeded(PackageResolver::new(original_candidates));
    let initial = succeeded(original_resolver.resolve(&original_spec));
    let receipt_bytes = succeeded(initial.receipt.canonical_bytes());
    let build_intent_bytes = succeeded(initial.build_intent.canonical_bytes());

    let higher = candidate(
        "frozen-universe",
        "2.0.0",
        "frozen-universe-v2",
        0,
        PackageSourceKind::Workspace,
    );
    let irrelevant = candidate(
        "irrelevant-catalog",
        "1.0.0",
        "irrelevant-catalog",
        100,
        PackageSourceKind::Workspace,
    );
    let expanded_candidates = vec![original.clone(), higher, irrelevant];
    let mut expanded_spec = composition(&expanded_candidates);
    root(&mut expanded_spec, "frozen-universe", ">=1.0.0 <3.0.0");
    let expanded_resolver = succeeded(PackageResolver::new(expanded_candidates));
    let normal = succeeded(expanded_resolver.resolve(&expanded_spec));
    assert_eq!(
        resolved_package(&normal, "frozen-universe").version,
        version("2.0.0")
    );
    let replayed = succeeded(expanded_resolver.resolve_frozen(&expanded_spec, &initial.receipt));
    assert_eq!(succeeded(replayed.receipt.canonical_bytes()), receipt_bytes);
    assert_eq!(
        succeeded(replayed.build_intent.canonical_bytes()),
        build_intent_bytes
    );

    let preferred_source = candidate(
        "frozen-universe",
        "1.0.0",
        "frozen-universe-preferred",
        100,
        PackageSourceKind::Workspace,
    );
    let source_candidates = vec![original, preferred_source.clone()];
    let mut source_spec = composition(&source_candidates);
    root(&mut source_spec, "frozen-universe", ">=1.0.0 <3.0.0");
    let source_resolver = succeeded(PackageResolver::new(source_candidates));
    let normal = succeeded(source_resolver.resolve(&source_spec));
    assert_eq!(
        resolved_package(&normal, "frozen-universe").source_id,
        preferred_source.source.source_id
    );
    let replayed = succeeded(source_resolver.resolve_frozen(&source_spec, &initial.receipt));
    assert_eq!(succeeded(replayed.receipt.canonical_bytes()), receipt_bytes);
    assert_eq!(
        succeeded(replayed.build_intent.canonical_bytes()),
        build_intent_bytes
    );
}

#[test]
#[allow(clippy::too_many_lines)]
fn namespace_authority_chain_is_canonical_and_structurally_verified() {
    let root_name = package_name("namespace-root");
    let child_name = package_name("namespace-child");
    let leaf_name = package_name("namespace-leaf");
    let root_pattern = namespace_pattern("test:content/**");
    let child_pattern = namespace_pattern("test:content/child/**");
    let leaf_pattern = namespace_pattern("test:content/child/leaf/**");

    let mut root_candidate = candidate(
        "namespace-root",
        "1.0.0",
        "namespace-root",
        0,
        PackageSourceKind::Workspace,
    );
    root_candidate
        .package
        .dependencies
        .insert(child_name.clone(), dependency("=1.0.0", false, [], []));
    root_candidate
        .package
        .namespace_requests
        .insert(root_pattern.clone());
    root_candidate
        .package
        .namespace_delegations
        .insert(child_name.clone(), BTreeSet::from([child_pattern.clone()]));

    let mut child_candidate = candidate(
        "namespace-child",
        "1.0.0",
        "namespace-child",
        0,
        PackageSourceKind::Workspace,
    );
    child_candidate
        .package
        .dependencies
        .insert(leaf_name.clone(), dependency("=1.0.0", false, [], []));
    child_candidate
        .package
        .namespace_requests
        .insert(child_pattern.clone());
    child_candidate
        .package
        .namespace_delegations
        .insert(leaf_name.clone(), BTreeSet::from([leaf_pattern.clone()]));

    let mut leaf_candidate = candidate(
        "namespace-leaf",
        "1.0.0",
        "namespace-leaf",
        0,
        PackageSourceKind::Workspace,
    );
    leaf_candidate
        .package
        .namespace_requests
        .insert(leaf_pattern.clone());

    let candidates = vec![root_candidate, child_candidate, leaf_candidate];
    let mut spec = composition(&candidates);
    root(&mut spec, "namespace-root", "=1.0.0");
    spec.policy
        .namespace_grants
        .insert(root_name.clone(), BTreeSet::from([root_pattern.clone()]));
    let resolution = resolve(candidates.clone(), &spec);
    succeeded(resolution.receipt.validate());
    succeeded(resolution.build_intent.verify_against(&resolution.receipt));
    assert_eq!(resolution.receipt.namespace_grants.len(), 3);
    assert_eq!(
        resolution.build_intent.namespace_grants,
        resolution.receipt.namespace_grants
    );
    assert!(resolution.receipt.namespace_grants.iter().any(|grant| {
        matches!(
            grant.grantor().as_ref(),
            NamespaceGrantorRef::Profile { profile } if profile == &spec.profile
        ) && grant.grantee() == &root_name
            && grant.patterns() == &BTreeSet::from([root_pattern.clone()])
    }));
    assert!(resolution.receipt.namespace_grants.iter().any(|grant| {
        matches!(
            grant.grantor().as_ref(),
            NamespaceGrantorRef::Package { package } if package == &root_name
        ) && grant.grantee() == &child_name
            && grant.patterns() == &BTreeSet::from([child_pattern.clone()])
    }));
    assert!(resolution.receipt.namespace_grants.iter().any(|grant| {
        matches!(
            grant.grantor().as_ref(),
            NamespaceGrantorRef::Package { package } if package == &child_name
        ) && grant.grantee() == &leaf_name
            && grant.patterns() == &BTreeSet::from([leaf_pattern.clone()])
    }));

    let receipt_bytes = succeeded(resolution.receipt.canonical_bytes());
    let build_intent_bytes = succeeded(resolution.build_intent.canonical_bytes());
    let mut shuffled_candidates = candidates;
    shuffled_candidates.reverse();
    let mut shuffled_spec = composition(&shuffled_candidates);
    root(&mut shuffled_spec, "namespace-root", "=1.0.0");
    shuffled_spec
        .policy
        .namespace_grants
        .insert(root_name, BTreeSet::from([root_pattern]));
    let shuffled = resolve(shuffled_candidates, &shuffled_spec);
    assert_eq!(succeeded(shuffled.receipt.canonical_bytes()), receipt_bytes);
    assert_eq!(
        succeeded(shuffled.build_intent.canonical_bytes()),
        build_intent_bytes
    );

    let mut tampered = resolution.receipt.clone();
    tampered.namespace_grants.retain(|grant| {
        !matches!(
            grant.grantor().as_ref(),
            NamespaceGrantorRef::Profile { .. }
        )
    });
    tampered.resolution_hash = succeeded(tampered.recompute_resolution_hash());
    tampered.receipt_hash = succeeded(tampered.recompute_receipt_hash());
    assert!(matches!(
        tampered.validate(),
        Err(ResolutionReceiptError::InvalidStructure { .. })
    ));

    let mut noncanonical = receipt_bytes;
    noncanonical.push(b'\n');
    assert!(matches!(
        ResolutionReceiptV1::from_json_slice(&noncanonical),
        Err(ResolutionReceiptError::NonCanonicalEncoding)
    ));
}

#[test]
fn inactive_optional_namespace_delegation_is_excluded() {
    let root_name = package_name("inactive-grant-root");
    let child_name = package_name("inactive-grant-child");
    let root_pattern = namespace_pattern("test:content/**");
    let child_pattern = namespace_pattern("test:content/child/**");
    let mut root_candidate = candidate(
        "inactive-grant-root",
        "1.0.0",
        "inactive-grant-root",
        0,
        PackageSourceKind::Workspace,
    );
    add_feature(&mut root_candidate, "delegate", false);
    root_candidate
        .package
        .dependencies
        .insert(child_name, dependency("=1.0.0", true, ["delegate"], []));
    root_candidate
        .package
        .namespace_requests
        .insert(root_pattern.clone());
    root_candidate.package.namespace_delegations.insert(
        package_name("inactive-grant-child"),
        BTreeSet::from([child_pattern]),
    );
    let child_candidate = candidate(
        "inactive-grant-child",
        "1.0.0",
        "inactive-grant-child",
        0,
        PackageSourceKind::Workspace,
    );
    let candidates = vec![root_candidate, child_candidate];
    let mut spec = composition(&candidates);
    root(&mut spec, "inactive-grant-root", "=1.0.0");
    root(&mut spec, "inactive-grant-child", "=1.0.0");
    spec.policy
        .namespace_grants
        .insert(root_name, BTreeSet::from([root_pattern]));
    let resolution = resolve(candidates, &spec);
    assert_eq!(resolution.receipt.namespace_grants.len(), 1);
    assert!(resolution.receipt.namespace_grants.iter().all(|grant| {
        matches!(
            grant.grantor().as_ref(),
            NamespaceGrantorRef::Profile { .. }
        )
    }));
    succeeded(resolution.receipt.validate());
}

#[test]
fn namespace_authorization_backtracks_and_missing_chain_fails_closed() {
    let request = namespace_pattern("test:content/choice/**");
    let mut high = candidate(
        "namespace-choice",
        "2.0.0",
        "namespace-choice-v2",
        0,
        PackageSourceKind::Workspace,
    );
    high.package.namespace_requests.insert(request);
    let low = candidate(
        "namespace-choice",
        "1.0.0",
        "namespace-choice-v1",
        0,
        PackageSourceKind::Workspace,
    );
    let candidates = vec![high.clone(), low];
    let mut spec = composition(&candidates);
    root(&mut spec, "namespace-choice", ">=1.0.0 <3.0.0");
    let resolution = resolve(candidates, &spec);
    assert_eq!(
        resolved_package(&resolution, "namespace-choice").version,
        version("1.0.0")
    );
    assert!(
        resolution
            .receipt
            .explanation
            .decisions
            .iter()
            .any(|decision| {
                matches!(
                    (&decision.subject, decision.outcome),
                    (
                        ResolutionSubjectV1::Package { candidate },
                        ResolutionOutcomeV1::Discarded
                    ) if candidate.source_id == high.source.source_id
                ) && decision.reasons.iter().any(|reason| {
                    matches!(
                        reason,
                        ResolutionReasonV1::Backtracked { failure }
                            if failure.code == ResolutionFailureCodeV1::NamespaceAuthorization
                    )
                })
            })
    );

    let exact_candidates = vec![high];
    let mut exact_spec = composition(&exact_candidates);
    root(&mut exact_spec, "namespace-choice", "=2.0.0");
    let resolver = succeeded(PackageResolver::new(exact_candidates));
    assert!(matches!(
        resolver.resolve(&exact_spec),
        Err(ResolutionError::NamespaceAuthorization { .. })
    ));
}

#[test]
#[allow(clippy::too_many_lines)]
fn trusted_host_interfaces_gate_portable_resolution_and_frozen_replay() {
    let interface: StableId = parsed("test:interface/gameplay");
    let mut required = candidate(
        "portable-host-gated",
        "1.0.0",
        "portable-host-gated",
        0,
        PackageSourceKind::Workspace,
    );
    let realization = portable_realization("portable", &interface, "=0.1.0", false);
    required.package.realizations = BTreeMap::from([(realization.id.clone(), realization)]);
    required.package.trust = TrustClass::TrustedNative;
    let candidates = vec![required];
    let mut spec = composition(&candidates);
    spec.policy.realization_order = vec![RealizationKind::PortableNative];
    root(&mut spec, "portable-host-gated", "=1.0.0");

    let absent = succeeded(PackageResolver::new(candidates.clone()));
    assert!(matches!(
        absent.resolve(&spec),
        Err(ResolutionError::RealizationUnavailable { .. })
    ));

    let incompatible_host = host_compatibility(
        HOST_TARGET,
        Some((&interface, "0.2.0", "descriptor-v2")),
        None,
    );
    let incompatible = succeeded(PackageResolver::new_with_host_compatibility(
        candidates.clone(),
        incompatible_host,
    ));
    assert!(matches!(
        incompatible.resolve(&spec),
        Err(ResolutionError::RealizationUnavailable { .. })
    ));

    let host_v1 = host_compatibility(
        HOST_TARGET,
        Some((&interface, "0.1.0", "descriptor-v1")),
        None,
    );
    let expected_host_hash = succeeded(host_v1.compatibility_hash());
    let compatible = succeeded(PackageResolver::new_with_host_compatibility(
        candidates.clone(),
        host_v1,
    ));
    let initial = succeeded(compatible.resolve(&spec));
    assert_eq!(
        initial.receipt.host_compatibility_hash,
        Some(expected_host_hash)
    );

    let initial_bytes = succeeded(initial.receipt.canonical_bytes());
    let mut legacy_value = succeeded(serde_json::from_slice::<serde_json::Value>(&initial_bytes));
    {
        let Some(legacy_fields) = legacy_value.as_object_mut() else {
            panic!("resolution receipts must encode as JSON objects");
        };
        legacy_fields.remove("host_compatibility_hash");
    }
    let missing_current_field_bytes = succeeded(serde_json::to_vec(&legacy_value));
    assert!(matches!(
        ResolutionReceiptV1::from_json_slice(&missing_current_field_bytes),
        Err(ResolutionReceiptError::NonCanonicalEncoding)
    ));
    let Some(legacy_fields) = legacy_value.as_object_mut() else {
        panic!("resolution receipts must encode as JSON objects");
    };
    legacy_fields.insert("schema_version".to_owned(), serde_json::Value::from(1));
    let legacy_bytes = succeeded(serde_json::to_vec(&legacy_value));
    assert!(matches!(
        ResolutionReceiptV1::from_json_slice(&legacy_bytes),
        Err(ResolutionReceiptError::UnsupportedSchema {
            found: 1,
            supported,
        }) if supported == latticeaxiom_packages::RESOLUTION_RECEIPT_SCHEMA_VERSION
    ));

    let changed_host = host_compatibility(
        HOST_TARGET,
        Some((&interface, "0.1.0", "descriptor-v1-rebuilt")),
        None,
    );
    let changed = succeeded(PackageResolver::new_with_host_compatibility(
        candidates.clone(),
        changed_host,
    ));
    assert!(matches!(
        changed.resolve_frozen(&spec, &initial.receipt),
        Err(ResolutionError::ResolutionReceiptMismatch { .. })
    ));

    let wrong_target = host_compatibility(
        "x86_64-unknown-linux-gnu",
        Some((&interface, "0.1.0", "descriptor-v1")),
        None,
    );
    let wrong_target_resolver = succeeded(PackageResolver::new_with_host_compatibility(
        candidates,
        wrong_target,
    ));
    assert!(matches!(
        wrong_target_resolver.resolve(&spec),
        Err(ResolutionError::HostTargetMismatch { .. })
    ));

    let mut optional = candidate(
        "portable-optional",
        "1.0.0",
        "portable-optional",
        0,
        PackageSourceKind::Workspace,
    );
    let optional_realization = portable_realization("portable", &interface, "=0.1.0", true);
    optional.package.realizations =
        BTreeMap::from([(optional_realization.id.clone(), optional_realization)]);
    optional.package.trust = TrustClass::TrustedNative;
    let optional_candidates = vec![optional];
    let mut optional_spec = composition(&optional_candidates);
    optional_spec.policy.realization_order = vec![RealizationKind::PortableNative];
    root(&mut optional_spec, "portable-optional", "=1.0.0");
    let optional_resolver = succeeded(PackageResolver::new(optional_candidates));
    let optional_resolution = succeeded(optional_resolver.resolve(&optional_spec));
    assert_eq!(optional_resolution.receipt.host_compatibility_hash, None);
}

#[test]
fn engine_coupled_realization_requires_exact_trusted_host_build() {
    let required_build = CanonicalHash::digest(b"engine-build-required");
    let mut package = candidate(
        "engine-coupled",
        "1.0.0",
        "engine-coupled",
        0,
        PackageSourceKind::Workspace,
    );
    let id = succeeded(RealizationId::new("engine-coupled"));
    let realization = RealizationSpec {
        id: id.clone(),
        kind: RealizationKind::EngineCoupledNative,
        domains: authoritative_domains(),
        targets: BTreeSet::from([parsed(HOST_TARGET)]),
        interfaces: BTreeMap::new(),
        required_features: BTreeSet::new(),
        artifact: ArtifactIntent::SourceBuild,
        trust: TrustClass::TrustedNative,
        engine_build: Some(required_build),
        registration_fragment: CanonicalHash::digest(b"engine-coupled-fragment"),
    };
    package.package.realizations = BTreeMap::from([(id, realization)]);
    package.package.trust = TrustClass::TrustedNative;
    let candidates = vec![package];
    let mut spec = composition(&candidates);
    spec.policy.realization_order = vec![RealizationKind::EngineCoupledNative];
    root(&mut spec, "engine-coupled", "=1.0.0");

    let absent = succeeded(PackageResolver::new(candidates.clone()));
    assert!(matches!(
        absent.resolve(&spec),
        Err(ResolutionError::RealizationUnavailable { .. })
    ));

    let wrong = host_compatibility(
        HOST_TARGET,
        None,
        Some(CanonicalHash::digest(b"engine-build-wrong")),
    );
    let wrong_resolver = succeeded(PackageResolver::new_with_host_compatibility(
        candidates.clone(),
        wrong,
    ));
    assert!(matches!(
        wrong_resolver.resolve(&spec),
        Err(ResolutionError::RealizationUnavailable { .. })
    ));

    let exact = host_compatibility(HOST_TARGET, None, Some(required_build));
    let exact_resolver = succeeded(PackageResolver::new_with_host_compatibility(
        candidates, exact,
    ));
    let resolution = succeeded(exact_resolver.resolve(&spec));
    assert_eq!(
        resolved_package(&resolution, "engine-coupled")
            .realization
            .engine_build_id,
        Some(required_build)
    );
}
