//! Deterministic compiler golden, permutation, property, and fault conformance.
#![allow(clippy::expect_used)]

use std::collections::{BTreeMap, BTreeSet};

use latticeaxiom_compose::{
    BundleActivation, COMPOSITION_SCHEMA_VERSION, CompositionPolicy, CompositionSpec,
    ContentBundle, ContentPredicate, ContentRoleDefinition, ExactRegistration, LOCK_SCHEMA_VERSION,
    LockedDependency, LockedGameGraph, LockedPackage, ManifestProducer, NickelEvaluationLimits,
    NumericRegistrationId, PackageDomain, PackageRequest, PredicateExpression, ProfileKind,
    REGISTRATION_MANIFEST_SCHEMA_VERSION, RealizationId, RealizationKind, RealizationPreference,
    RegistrationFragment, RegistrationKind, RegistrationManifest, RoleAuthority, RoleCardinality,
    RoleOffer, RoleOfferClass, SemanticFragment, SourceCandidate, TargetKind, TrustClass,
};
use latticeaxiom_core::{
    CanonicalHash, CapabilityId, NamespaceGrant, NamespaceGrantPattern, NamespaceGrantor,
    PackageName, PackageVersion, PackageVersionReq, RegistrationNamespace, SourceId,
    SourceProvenance, StableId, TargetTriple,
};
use latticeaxiom_registration::{
    CallbackDeclaration, PackageRegistrationInput, ReceiptValidationError,
    RegistrationCompileError, RegistrationCompileInput, RegistrationCompiler,
    RegistrationCompilerLimits, RoleSelectionRule, SystemDeclaration,
};
use proptest::collection::btree_set;
use proptest::prelude::*;

const FIXED_STAGE: &str = "latticeaxiom:system-stage/gameplay/fixed@1";
const BLOCK_ROLE: &str = "terrenia:block-role/spawn@1";
const FALLBACK_BUNDLE: &str = "terrenia:content-bundle/spawn-fallback@1";
const NORMAL_BLOCK: &str = "terrenia:block/alpha";
const FALLBACK_BLOCK: &str = "terrenia:block/zeta";

#[derive(Clone)]
struct Fixture {
    root: PackageName,
    composition: CompositionSpec,
    graph: LockedGameGraph,
    packages: BTreeMap<PackageName, PackageRegistrationInput>,
}

impl Fixture {
    // The complete lock, manifest, callback, and schedule fixture stays visible together.
    #[allow(clippy::too_many_lines)]
    fn new() -> Self {
        let root = package_name("terrenia");
        let version = package_version("0.1.0");
        let source_id = source_id("latticeaxiom:source/terrenia");
        let profile = stable_id("latticeaxiom:profile/headless");
        let patterns = grant_patterns();
        let composition = CompositionSpec {
            schema_version: COMPOSITION_SCHEMA_VERSION,
            profile: profile.clone(),
            profile_kind: ProfileKind::HeadlessTest,
            projection_domains: BTreeSet::from([PackageDomain::Authoritative]),
            roots: BTreeMap::from([(
                root.clone(),
                PackageRequest {
                    version: version_requirement("~0.1.0"),
                    features: BTreeSet::new(),
                    realization: RealizationPreference::Auto,
                },
            )]),
            capabilities: BTreeMap::new(),
            features: BTreeMap::new(),
            parameters: BTreeMap::new(),
            semantic_bindings: BTreeMap::new(),
            overlays: Vec::new(),
            sources: vec![SourceCandidate {
                source_id: source_id.clone(),
                package: root.clone(),
                version: version.clone(),
                path: "packages/terrenia".to_owned(),
                content_hash: CanonicalHash::digest(b"terrenia-source"),
                priority: 0,
                provenance: provenance(
                    "latticeaxiom:source/terrenia",
                    "packages/terrenia/package.ncl",
                ),
            }],
            policy: CompositionPolicy {
                target: target("x86_64-unknown-linux-gnu"),
                realization_order: vec![RealizationKind::Data],
                namespace_grants: BTreeMap::from([(root.clone(), patterns.clone())]),
                maximum_trust: TrustClass::DataOnly,
                allow_force_override: false,
                allow_recovery: false,
                evaluation_policy: stable_id("latticeaxiom:nickel-evaluation-policy/r0@1"),
                evaluation_limits: NickelEvaluationLimits::default(),
            },
            provenance: provenance(
                "latticeaxiom:source/profile-headless",
                "profiles/headless.game.ncl",
            ),
        };

        let signature = CanonicalHash::digest(b"fixed-system-signature-v1");
        let alpha_system = stable_id("terrenia:system/alpha");
        let beta_system = stable_id("terrenia:system/beta");
        let alpha_callback = stable_id("terrenia:callback/alpha@1");
        let beta_callback = stable_id("terrenia:callback/beta@1");
        let registrations = vec![
            exact_registration(&root, "terrenia:block/zeta", RegistrationKind::Block),
            exact_registration(&root, beta_system.as_str(), RegistrationKind::System),
            exact_registration(&root, "terrenia:block/alpha", RegistrationKind::Block),
            exact_registration(&root, alpha_system.as_str(), RegistrationKind::System),
        ];
        let manifest = RegistrationManifest {
            schema_version: REGISTRATION_MANIFEST_SCHEMA_VERSION,
            package: root.clone(),
            version: version.clone(),
            fragment: RegistrationFragment {
                registrations,
                ..RegistrationFragment::default()
            },
            producer: ManifestProducer {
                tool: "registration-conformance-fixture".to_owned(),
                version: version.clone(),
                input_hash: CanonicalHash::digest(b"registration-input"),
            },
            semantic_hash: CanonicalHash::digest(b"unsealed-manifest"),
        };
        let systems = BTreeMap::from([
            (
                alpha_system.clone(),
                SystemDeclaration {
                    id: alpha_system.clone(),
                    declared_by: root.clone(),
                    stage: stable_id(FIXED_STAGE),
                    callback: alpha_callback.clone(),
                    signature_hash: signature,
                    after: BTreeSet::new(),
                    before: BTreeSet::new(),
                },
            ),
            (
                beta_system.clone(),
                SystemDeclaration {
                    id: beta_system.clone(),
                    declared_by: root.clone(),
                    stage: stable_id(FIXED_STAGE),
                    callback: beta_callback.clone(),
                    signature_hash: signature,
                    after: BTreeSet::from([alpha_system]),
                    before: BTreeSet::new(),
                },
            ),
        ]);
        let callbacks = BTreeMap::from([
            (
                alpha_callback.clone(),
                CallbackDeclaration {
                    id: alpha_callback,
                    declared_by: root.clone(),
                    signature_hash: signature,
                },
            ),
            (
                beta_callback.clone(),
                CallbackDeclaration {
                    id: beta_callback,
                    declared_by: root.clone(),
                    signature_hash: signature,
                },
            ),
        ]);
        let package_input = PackageRegistrationInput {
            manifest,
            schemas: BTreeMap::new(),
            systems,
            callbacks,
            provided_capabilities: BTreeSet::new(),
            semantic_grants: BTreeSet::new(),
        };
        let locked = LockedPackage {
            name: root.clone(),
            version,
            source_id,
            source_hash: CanonicalHash::digest(b"terrenia-source"),
            provenance_hash: CanonicalHash::digest(b"terrenia-provenance"),
            realization: RealizationKind::Data,
            realization_id: RealizationId::new("data").expect("fixture realization is valid"),
            manifest_hash: CanonicalHash::digest(b"unsealed-manifest"),
            artifact_hash: CanonicalHash::digest(b"terrenia-artifact"),
            interfaces: BTreeMap::new(),
            engine_build_id: None,
            domains: BTreeSet::from([PackageDomain::Authoritative]),
            dependencies: BTreeMap::new(),
            schemas: BTreeSet::new(),
            source_path: "packages/terrenia".to_owned(),
        };
        let graph = LockedGameGraph {
            schema_version: LOCK_SCHEMA_VERSION,
            composition_hash: CanonicalHash::digest(b"unsealed-composition"),
            composition_provenance_hash: CanonicalHash::digest(b"unsealed-provenance"),
            evaluation_policy: composition.policy.evaluation_policy.clone(),
            evaluation_limits: composition.policy.evaluation_limits,
            roots: BTreeSet::from([root.clone()]),
            packages: BTreeMap::from([(root.clone(), locked)]),
            capability_providers: BTreeMap::new(),
            namespace_grants: BTreeSet::new(),
            explanation: Vec::new(),
            graph_hash: CanonicalHash::digest(b"unsealed-graph"),
            lock_hash: CanonicalHash::digest(b"unsealed-lock"),
        };
        let grant = NamespaceGrant::new(
            RegistrationNamespace::new("terrenia").expect("fixture namespace is valid"),
            NamespaceGrantor::profile(profile).expect("fixture profile grantor is valid"),
            root.clone(),
            patterns,
        )
        .expect("fixture namespace grant is valid");
        let mut fixture = Self {
            root: root.clone(),
            composition,
            graph,
            packages: BTreeMap::from([(root, package_input)]),
        };
        fixture.graph.namespace_grants.insert(grant);
        fixture.refresh_hashes();
        fixture
    }

    fn package(&self) -> &PackageRegistrationInput {
        self.packages
            .get(&self.root)
            .expect("fixture root package input exists")
    }

    fn package_mut(&mut self) -> &mut PackageRegistrationInput {
        self.packages
            .get_mut(&self.root)
            .expect("fixture root package input exists")
    }

    fn refresh_hashes(&mut self) {
        let manifest_hash = self
            .package()
            .manifest
            .recompute_semantic_hash()
            .expect("fixture manifest canonicalizes");
        self.package_mut().manifest.semantic_hash = manifest_hash;
        self.graph
            .packages
            .get_mut(&self.root)
            .expect("fixture locked root exists")
            .manifest_hash = manifest_hash;
        self.graph.composition_hash = self
            .composition
            .semantic_hash()
            .expect("fixture composition canonicalizes");
        self.graph.composition_provenance_hash = self
            .composition
            .provenance_hash()
            .expect("fixture provenance canonicalizes");
        self.graph.graph_hash = self
            .graph
            .recompute_graph_hash()
            .expect("fixture graph canonicalizes");
        self.graph.lock_hash = self
            .graph
            .recompute_lock_hash()
            .expect("fixture lock canonicalizes");
    }

    #[allow(clippy::result_large_err)]
    fn compile(
        &self,
    ) -> Result<latticeaxiom_registration::CompiledRegistration, RegistrationCompileError> {
        RegistrationCompiler::default().compile(RegistrationCompileInput {
            composition: &self.composition,
            graph: &self.graph,
            packages: &self.packages,
        })
    }
}

#[test]
fn compiler_output_is_self_verifying_and_uses_dense_canonical_ids() {
    let compiled = Fixture::new().compile().expect("fixture compiles");
    compiled.verify().expect("fresh receipts verify");

    let blocks = compiled
        .image_receipt
        .numeric_ids
        .get(&RegistrationKind::Block)
        .expect("block numeric table exists");
    assert_eq!(
        blocks,
        &BTreeMap::from([
            (stable_id("terrenia:block/alpha"), NumericRegistrationId(0)),
            (stable_id("terrenia:block/zeta"), NumericRegistrationId(1)),
        ])
    );
    assert_eq!(
        compiled.image_receipt.schedule,
        vec![
            stable_id("terrenia:system/alpha"),
            stable_id("terrenia:system/beta")
        ]
    );
    assert_eq!(compiled.callback_receipt.callbacks.len(), 2);
}

#[test]
fn full_compiled_registration_matches_checked_in_golden() {
    let actual = serde_json::to_string(&Fixture::new().compile().expect("fixture compiles"))
        .expect("compiled registration serializes");
    let expected = include_str!("golden/basic_compiled_registration.json").trim_end();
    assert!(expected != "PENDING", "FULL_GOLDEN={actual}");

    assert_eq!(actual, expected);
}

#[test]
fn input_permutations_produce_byte_identical_outputs() {
    let first = Fixture::new();
    let mut permuted = first.clone();
    permuted
        .package_mut()
        .manifest
        .fragment
        .registrations
        .reverse();
    permuted.refresh_hashes();

    let first = first.compile().expect("first fixture compiles");
    let second = permuted.compile().expect("permuted fixture compiles");
    assert_eq!(
        serde_json::to_vec(&first).expect("first output serializes"),
        serde_json::to_vec(&second).expect("second output serializes")
    );
}

#[test]
fn catalog_ownership_schema_and_callback_faults_fail_closed() {
    let mut mismatch = Fixture::new();
    let system = mismatch
        .package_mut()
        .systems
        .remove(&stable_id("terrenia:system/alpha"))
        .expect("fixture system exists");
    mismatch
        .package_mut()
        .systems
        .insert(stable_id("terrenia:system/wrong-key"), system);
    assert_eq!(
        mismatch
            .compile()
            .expect_err("key mismatch must fail")
            .code(),
        "registration.map_key_mismatch"
    );

    let mut foreign = Fixture::new();
    foreign.package_mut().manifest.fragment.registrations[0].declared_by =
        package_name("@example/foreign");
    foreign.refresh_hashes();
    assert_eq!(
        foreign
            .compile()
            .expect_err("foreign owner must fail")
            .code(),
        "registration.foreign_owner"
    );

    let mut schema = Fixture::new();
    schema.package_mut().manifest.fragment.registrations[0].schema = Some(
        "terrenia:schema/block@1"
            .parse()
            .expect("fixture schema ID is valid"),
    );
    schema.refresh_hashes();
    assert_eq!(
        schema
            .compile()
            .expect_err("undeclared schema must fail")
            .code(),
        "registration.schema_invalid"
    );

    let mut callback = Fixture::new();
    callback
        .package_mut()
        .callbacks
        .remove(&stable_id("terrenia:callback/alpha@1"));
    assert_eq!(
        callback
            .compile()
            .expect_err("undeclared callback must fail")
            .code(),
        "registration.callback_invalid"
    );
}

#[test]
fn closure_schedule_authority_and_limit_faults_fail_closed() {
    let mut cycle = Fixture::new();
    cycle
        .package_mut()
        .systems
        .get_mut(&stable_id("terrenia:system/alpha"))
        .expect("fixture system exists")
        .after
        .insert(stable_id("terrenia:system/beta"));
    assert_eq!(
        cycle
            .compile()
            .expect_err("schedule cycle must fail")
            .code(),
        "registration.schedule_invalid"
    );

    let mut no_grant = Fixture::new();
    no_grant.graph.namespace_grants.clear();
    no_grant.refresh_hashes();
    assert_eq!(
        no_grant
            .compile()
            .expect_err("missing namespace grant must fail")
            .code(),
        "registration.namespace_unauthorized"
    );

    let mut invalid_delegation = Fixture::new();
    invalid_delegation.graph.namespace_grants.insert(
        NamespaceGrant::new(
            RegistrationNamespace::new("terrenia").expect("fixture namespace is valid"),
            NamespaceGrantor::package(invalid_delegation.root.clone()),
            invalid_delegation.root.clone(),
            grant_patterns(),
        )
        .expect("well-formed but unauthorized delegation is constructible"),
    );
    invalid_delegation.refresh_hashes();
    assert_eq!(
        invalid_delegation
            .compile()
            .expect_err("non-dependency delegation must fail")
            .code(),
        "registration.namespace_unauthorized"
    );

    let fixture = Fixture::new();
    let limits = RegistrationCompilerLimits {
        registrations_per_kind: 1,
        ..RegistrationCompilerLimits::default()
    };
    let error = RegistrationCompiler::new(limits)
        .compile(RegistrationCompileInput {
            composition: &fixture.composition,
            graph: &fixture.graph,
            packages: &fixture.packages,
        })
        .expect_err("per-kind limit must fail");
    assert_eq!(error.code(), "registration.limit_exceeded");
}

#[test]
fn missing_root_dependency_and_provider_faults_are_distinct_closure_failures() {
    let mut missing_root = Fixture::new();
    let absent = package_name("@example/absent-root");
    let request = missing_root
        .composition
        .roots
        .get(&missing_root.root)
        .expect("fixture request exists")
        .clone();
    missing_root
        .composition
        .roots
        .insert(absent.clone(), request);
    missing_root.graph.roots.insert(absent);
    missing_root.refresh_hashes();
    assert!(matches!(
        missing_root.compile(),
        Err(RegistrationCompileError::MissingRoot { .. })
    ));

    let mut missing_dependency = Fixture::new();
    let absent = package_name("@example/absent-dependency");
    missing_dependency
        .graph
        .packages
        .get_mut(&missing_dependency.root)
        .expect("fixture locked root exists")
        .dependencies
        .insert(
            absent,
            LockedDependency {
                version: package_version("0.1.0"),
                features: BTreeSet::new(),
            },
        );
    missing_dependency.refresh_hashes();
    assert!(matches!(
        missing_dependency.compile(),
        Err(RegistrationCompileError::MissingDependency { .. })
    ));

    let mut missing_provider = Fixture::new();
    missing_provider.graph.capability_providers.insert(
        capability("terrenia:capability/worldgen@1"),
        vec![package_name("@example/absent-provider")],
    );
    missing_provider.refresh_hashes();
    assert!(matches!(
        missing_provider.compile(),
        Err(RegistrationCompileError::MissingCapabilityProvider { .. })
    ));
}

#[test]
fn tampered_numeric_receipt_rejects_non_dense_ids_before_hash_acceptance() {
    let mut compiled = Fixture::new().compile().expect("fixture compiles");
    let blocks = compiled
        .image_receipt
        .numeric_ids
        .get_mut(&RegistrationKind::Block)
        .expect("block numeric table exists");
    *blocks
        .get_mut(&stable_id("terrenia:block/zeta"))
        .expect("second block numeric ID exists") = NumericRegistrationId(7);
    assert!(matches!(
        compiled.image_receipt.verify(),
        Err(ReceiptValidationError::NonDenseNumericIds { .. })
    ));
}

#[test]
fn missing_role_activates_one_fallback_bundle() {
    let compiled = semantic_fixture(RoleAuthority::Graph, false, None)
        .compile()
        .expect("fallback fixture compiles");
    compiled.verify().expect("fallback receipts verify");
    assert_eq!(
        compiled.semantic_receipt.active_bundles,
        BTreeSet::from([stable_id(FALLBACK_BUNDLE)])
    );
    let binding = compiled
        .semantic_receipt
        .role_bindings
        .get(&stable_id(BLOCK_ROLE))
        .expect("role binding exists");
    assert_eq!(binding.targets, vec![stable_id(FALLBACK_BLOCK)]);
    assert_eq!(binding.selected_by, RoleSelectionRule::Fallback);
}

#[test]
fn normal_candidate_keeps_fallback_inactive() {
    let compiled = semantic_fixture(RoleAuthority::Graph, true, None)
        .compile()
        .expect("normal fixture compiles");
    assert!(compiled.semantic_receipt.active_bundles.is_empty());
    let binding = compiled
        .semantic_receipt
        .role_bindings
        .get(&stable_id(BLOCK_ROLE))
        .expect("role binding exists");
    assert_eq!(binding.targets, vec![stable_id(NORMAL_BLOCK)]);
    assert_eq!(binding.selected_by, RoleSelectionRule::Unique);
    assert!(
        !compiled
            .image
            .numeric_ids
            .contains_key(&stable_id(FALLBACK_BLOCK))
    );
}

#[test]
fn profile_binding_can_select_a_fallback_despite_a_normal_candidate() {
    let compiled = semantic_fixture(RoleAuthority::Profile, true, Some(FALLBACK_BLOCK))
        .compile()
        .expect("profile-selected fallback compiles");
    assert_eq!(
        compiled.semantic_receipt.active_bundles,
        BTreeSet::from([stable_id(FALLBACK_BUNDLE)])
    );
    let binding = compiled
        .semantic_receipt
        .role_bindings
        .get(&stable_id(BLOCK_ROLE))
        .expect("role binding exists");
    assert_eq!(binding.targets, vec![stable_id(FALLBACK_BLOCK)]);
    assert_eq!(binding.selected_by, RoleSelectionRule::Profile);
}

#[test]
fn competing_fallback_bundles_are_rejected() {
    let mut fixture = semantic_fixture(RoleAuthority::Graph, false, None);
    let role = stable_id(BLOCK_ROLE);
    fixture
        .package_mut()
        .manifest
        .fragment
        .semantics
        .push(SemanticFragment::Bundle(fallback_bundle(
            "terrenia:content-bundle/spawn-fallback-secondary@1",
            &role,
            stable_id(NORMAL_BLOCK),
        )));
    fixture.refresh_hashes();
    let error = fixture
        .compile()
        .expect_err("competing fallbacks must fail");
    let RegistrationCompileError::FallbackAmbiguous {
        role: actual,
        bundles,
    } = error
    else {
        panic!("unexpected competing-fallback diagnostic: {error}");
    };
    assert_eq!(actual, role);
    assert_eq!(
        bundles.into_iter().collect::<BTreeSet<_>>(),
        BTreeSet::from([
            stable_id(FALLBACK_BUNDLE),
            stable_id("terrenia:content-bundle/spawn-fallback-secondary@1"),
        ])
    );
}

#[test]
fn empty_and_unknown_fallback_guards_are_rejected() {
    let bundle = stable_id(FALLBACK_BUNDLE);
    let mut empty = semantic_fixture(RoleAuthority::Graph, false, None);
    bundle_mut(&mut empty, &bundle).activation =
        BundleActivation::FallbackForMissingRoles(BTreeSet::new());
    empty.refresh_hashes();
    assert!(matches!(
        empty.compile(),
        Err(RegistrationCompileError::InvalidFallbackGuard { role: None, .. })
    ));

    let unknown_role = stable_id("terrenia:block-role/unknown@1");
    let mut unknown = semantic_fixture(RoleAuthority::Graph, false, None);
    bundle_mut(&mut unknown, &bundle).activation =
        BundleActivation::FallbackForMissingRoles(BTreeSet::from([unknown_role.clone()]));
    unknown.refresh_hashes();
    assert!(matches!(
        unknown.compile(),
        Err(RegistrationCompileError::InvalidFallbackGuard { role: Some(role), .. })
            if role == unknown_role
    ));
}

#[test]
fn semantic_ids_and_targets_are_typed_and_versioned() {
    let mut wrong_kind = semantic_fixture(RoleAuthority::Graph, true, None);
    role_mut(&mut wrong_kind).id = stable_id("terrenia:item-role/spawn@1");
    wrong_kind.refresh_hashes();
    assert!(matches!(
        wrong_kind.compile(),
        Err(RegistrationCompileError::InvalidSemanticId { expected_kind, .. })
            if expected_kind == "block-role"
    ));

    let mut unversioned = semantic_fixture(RoleAuthority::Graph, true, None);
    role_mut(&mut unversioned).id = stable_id("terrenia:block-role/spawn");
    unversioned.refresh_hashes();
    assert!(matches!(
        unversioned.compile(),
        Err(RegistrationCompileError::InvalidSemanticId { expected_kind, .. })
            if expected_kind == "block-role"
    ));

    let mut entity = semantic_fixture(RoleAuthority::Graph, true, None);
    let role = role_mut(&mut entity);
    role.id = stable_id("terrenia:entity-role/spawn@1");
    role.accepts.target_kind = TargetKind::Entity;
    entity.refresh_hashes();
    assert!(matches!(
        entity.compile(),
        Err(
            RegistrationCompileError::UnsupportedSemanticDefinitionTarget {
                target_kind: TargetKind::Entity,
                ..
            }
        )
    ));
}

#[test]
fn rehashed_manifest_provenance_tampering_is_rejected_against_lock() {
    let fixture = Fixture::new();
    let mut compiled = fixture.compile().expect("fixture compiles");
    compiled
        .verify_locked_manifests(&fixture.graph)
        .expect("fresh manifest provenance matches the lock");
    compiled
        .provenance_receipt
        .packages
        .get_mut(&fixture.root)
        .expect("root provenance exists")
        .manifest_semantic_hash = CanonicalHash::digest(b"tampered-manifest-semantic-hash");
    compiled.provenance_receipt.provenance_hash = compiled
        .provenance_receipt
        .recompute_hash()
        .expect("tampered provenance receipt canonicalizes");

    assert!(matches!(
        compiled.verify_locked_manifests(&fixture.graph),
        Err(ReceiptValidationError::LockedManifestSemanticHashMismatch {
            package,
            locked,
            receipt,
        }) if package == fixture.root
            && locked == fixture.graph.packages[&fixture.root].manifest_hash
            && receipt == CanonicalHash::digest(b"tampered-manifest-semantic-hash")
    ));
}

#[test]
fn rehashed_system_callback_swap_and_signature_tampering_is_rejected() {
    let mut swapped = Fixture::new().compile().expect("fixture compiles");
    let alpha = stable_id("terrenia:callback/alpha@1");
    let beta = stable_id("terrenia:callback/beta@1");
    let alpha_consumers = swapped.callback_receipt.callbacks[&alpha].consumers.clone();
    let beta_consumers = swapped.callback_receipt.callbacks[&beta].consumers.clone();
    swapped
        .callback_receipt
        .callbacks
        .get_mut(&alpha)
        .expect("alpha callback exists")
        .consumers = beta_consumers;
    swapped
        .callback_receipt
        .callbacks
        .get_mut(&beta)
        .expect("beta callback exists")
        .consumers = alpha_consumers;
    swapped.callback_receipt.callback_map_hash = swapped
        .callback_receipt
        .recompute_hash()
        .expect("swapped callback receipt canonicalizes");
    assert!(matches!(
        swapped.verify(),
        Err(ReceiptValidationError::SystemCallbackProjectionMismatch { .. })
    ));

    let mut signature = Fixture::new().compile().expect("fixture compiles");
    signature
        .callback_receipt
        .callbacks
        .get_mut(&alpha)
        .expect("alpha callback exists")
        .signature_hash = CanonicalHash::digest(b"tampered-system-signature");
    signature.callback_receipt.callback_map_hash = signature
        .callback_receipt
        .recompute_hash()
        .expect("tampered callback receipt canonicalizes");
    assert!(matches!(
        signature.verify(),
        Err(ReceiptValidationError::SystemCallbackProjectionMismatch { system })
            if system == stable_id("terrenia:system/alpha")
    ));
}

#[test]
fn rehashed_owner_and_semantic_receipt_tampering_is_rejected() {
    let mut owner = Fixture::new().compile().expect("fixture compiles");
    assert!(
        owner
            .image
            .owners
            .remove(&stable_id(NORMAL_BLOCK))
            .is_some()
    );
    owner.image.image_hash = owner
        .image
        .recompute_image_hash()
        .expect("tampered image canonicalizes");
    owner.image_receipt.image_hash = owner.image.image_hash;
    owner.image_receipt.receipt_hash = owner
        .image_receipt
        .recompute_hash()
        .expect("tampered image receipt canonicalizes");
    assert!(matches!(
        owner.verify(),
        Err(ReceiptValidationError::CrossReceiptMismatch {
            field: "owner-keys"
        })
    ));

    let mut semantic = Fixture::new().compile().expect("fixture compiles");
    semantic
        .semantic_receipt
        .active_bundles
        .insert(stable_id("terrenia:content-bundle/tampered@1"));
    semantic.semantic_receipt.receipt_hash = semantic
        .semantic_receipt
        .recompute_hash()
        .expect("tampered semantic receipt canonicalizes");
    assert!(matches!(
        semantic.verify(),
        Err(ReceiptValidationError::CrossReceiptMismatch {
            field: "semantic-active-bundles"
        })
    ));
}

#[test]
fn frozen_capability_provider_preference_need_not_be_lexical() {
    let mut fixture = Fixture::new();
    let provided = capability("terrenia:capability/decoration@1");
    let b = add_capability_provider(
        &mut fixture,
        "@example/decoration-b",
        "latticeaxiom:source/decoration-b",
        provided.clone(),
    );
    let a = add_capability_provider(
        &mut fixture,
        "@example/decoration-a",
        "latticeaxiom:source/decoration-a",
        provided.clone(),
    );
    fixture
        .graph
        .capability_providers
        .insert(provided, vec![b, a]);
    fixture.refresh_hashes();
    fixture
        .compile()
        .expect("resolver-frozen non-lexical provider order compiles")
        .verify()
        .expect("provider-order receipts verify");
}

proptest! {
    #[test]
    fn numeric_ids_are_canonical_for_arbitrary_unique_manifest_order(
        values in btree_set(any::<u16>(), 1..32)
    ) {
        let mut fixture = Fixture::new();
        let systems = fixture
            .package()
            .manifest
            .fragment
            .registrations
            .iter()
            .filter(|registration| registration.kind == RegistrationKind::System)
            .cloned()
            .collect::<Vec<_>>();
        let mut blocks = values
            .iter()
            .rev()
            .map(|value| {
                exact_registration(
                    &fixture.root,
                    &format!("terrenia:block/b{value:05}"),
                    RegistrationKind::Block,
                )
            })
            .collect::<Vec<_>>();
        blocks.extend(systems);
        fixture.package_mut().manifest.fragment.registrations = blocks;
        fixture.refresh_hashes();

        let compiled = fixture.compile().expect("generated fixture compiles");
        let numeric = compiled
            .image_receipt
            .numeric_ids
            .get(&RegistrationKind::Block)
            .expect("generated block table exists");
        for (expected, (id, actual)) in numeric.iter().enumerate() {
            prop_assert_eq!(id.as_str(), format!("terrenia:block/b{:05}", values.iter().nth(expected).expect("generated value exists")));
            prop_assert_eq!(actual.0, u32::try_from(expected).expect("test cardinality fits u32"));
        }
    }
}

fn semantic_fixture(
    authority: RoleAuthority,
    normal_candidate: bool,
    profile_target: Option<&str>,
) -> Fixture {
    let mut fixture = Fixture::new();
    let role = stable_id(BLOCK_ROLE);
    let normal = stable_id(NORMAL_BLOCK);
    let fallback = stable_id(FALLBACK_BLOCK);
    let mut semantics = vec![SemanticFragment::Role(ContentRoleDefinition {
        id: role.clone(),
        accepts: ContentPredicate {
            target_kind: TargetKind::Block,
            expression: PredicateExpression::Any {
                predicates: vec![
                    PredicateExpression::Exact {
                        target: normal.clone(),
                    },
                    PredicateExpression::Exact {
                        target: fallback.clone(),
                    },
                ],
            },
        },
        cardinality: RoleCardinality::ExactlyOne,
        authority,
    })];
    if normal_candidate {
        semantics.push(SemanticFragment::RoleOffer(RoleOffer {
            role: role.clone(),
            target: normal,
            class: RoleOfferClass::Normal,
        }));
    }
    semantics.push(SemanticFragment::Bundle(fallback_bundle(
        FALLBACK_BUNDLE,
        &role,
        fallback,
    )));
    fixture.package_mut().manifest.fragment.semantics = semantics;
    if let Some(target) = profile_target {
        fixture
            .composition
            .semantic_bindings
            .insert(role, stable_id(target));
    }
    fixture.refresh_hashes();
    fixture
}

fn fallback_bundle(id: &str, role: &StableId, target: StableId) -> ContentBundle {
    ContentBundle {
        id: stable_id(id),
        activation: BundleActivation::FallbackForMissingRoles(BTreeSet::from([role.clone()])),
        registrations: BTreeSet::from([target.clone()]),
        semantics: Vec::new(),
        role_offers: vec![RoleOffer {
            role: role.clone(),
            target,
            class: RoleOfferClass::Fallback,
        }],
    }
}

fn role_mut(fixture: &mut Fixture) -> &mut ContentRoleDefinition {
    fixture
        .package_mut()
        .manifest
        .fragment
        .semantics
        .iter_mut()
        .find_map(|fragment| match fragment {
            SemanticFragment::Role(role) => Some(role),
            _ => None,
        })
        .expect("semantic fixture contains a role")
}

fn bundle_mut<'a>(fixture: &'a mut Fixture, id: &StableId) -> &'a mut ContentBundle {
    fixture
        .package_mut()
        .manifest
        .fragment
        .semantics
        .iter_mut()
        .find_map(|fragment| match fragment {
            SemanticFragment::Bundle(bundle) if bundle.id == *id => Some(bundle),
            _ => None,
        })
        .expect("semantic fixture contains the requested bundle")
}

#[allow(clippy::too_many_lines)]
fn add_capability_provider(
    fixture: &mut Fixture,
    name: &str,
    source: &str,
    provided: CapabilityId,
) -> PackageName {
    let name = package_name(name);
    let version = package_version("0.1.0");
    let source_id = source_id(source);
    let mut manifest = RegistrationManifest {
        schema_version: REGISTRATION_MANIFEST_SCHEMA_VERSION,
        package: name.clone(),
        version: version.clone(),
        fragment: RegistrationFragment::default(),
        producer: ManifestProducer {
            tool: "registration-conformance-provider".to_owned(),
            version: version.clone(),
            input_hash: CanonicalHash::digest(name.as_str().as_bytes()),
        },
        semantic_hash: CanonicalHash::digest(b"unsealed-provider-manifest"),
    };
    manifest.semantic_hash = manifest
        .recompute_semantic_hash()
        .expect("provider manifest canonicalizes");
    let locked = LockedPackage {
        name: name.clone(),
        version,
        source_id,
        source_hash: CanonicalHash::digest(name.as_str().as_bytes()),
        provenance_hash: CanonicalHash::digest(source.as_bytes()),
        realization: RealizationKind::Data,
        realization_id: RealizationId::new("data").expect("provider realization is valid"),
        manifest_hash: manifest.semantic_hash,
        artifact_hash: CanonicalHash::digest(b"provider-artifact"),
        interfaces: BTreeMap::new(),
        engine_build_id: None,
        domains: BTreeSet::from([PackageDomain::Authoritative]),
        dependencies: BTreeMap::new(),
        schemas: BTreeSet::new(),
        source_path: "packages/provider".to_owned(),
    };
    fixture.graph.packages.insert(name.clone(), locked);
    fixture.packages.insert(
        name.clone(),
        PackageRegistrationInput {
            manifest,
            schemas: BTreeMap::new(),
            systems: BTreeMap::new(),
            callbacks: BTreeMap::new(),
            provided_capabilities: BTreeSet::from([provided]),
            semantic_grants: BTreeSet::new(),
        },
    );
    name
}

fn exact_registration(
    owner: &PackageName,
    value: &str,
    kind: RegistrationKind,
) -> ExactRegistration {
    ExactRegistration {
        id: stable_id(value),
        kind,
        declared_by: owner.clone(),
        schema: None,
        provenance: provenance("latticeaxiom:source/terrenia", "registrations/fixture.ncl"),
    }
}

fn grant_patterns() -> BTreeSet<NamespaceGrantPattern> {
    [
        "terrenia:block/**",
        "terrenia:system/**",
        "terrenia:callback/**",
        "terrenia:schema/**",
        "terrenia:block-role/**",
        "terrenia:item-role/**",
        "terrenia:entity-role/**",
        "terrenia:content-bundle/**",
    ]
    .into_iter()
    .map(|value| value.parse().expect("fixture grant pattern is valid"))
    .collect()
}

fn provenance(source: &str, path: &str) -> SourceProvenance {
    SourceProvenance::new(
        source_id(source),
        path,
        CanonicalHash::digest(path.as_bytes()),
        None,
        Vec::new(),
    )
    .expect("fixture provenance is valid")
}

fn stable_id(value: &str) -> StableId {
    value.parse().expect("fixture stable ID is valid")
}

fn package_name(value: &str) -> PackageName {
    value.parse().expect("fixture package name is valid")
}

fn package_version(value: &str) -> PackageVersion {
    value.parse().expect("fixture package version is valid")
}

fn version_requirement(value: &str) -> PackageVersionReq {
    value.parse().expect("fixture version requirement is valid")
}

fn source_id(value: &str) -> SourceId {
    value.parse().expect("fixture source ID is valid")
}

fn target(value: &str) -> TargetTriple {
    value.parse().expect("fixture target triple is valid")
}

fn capability(value: &str) -> CapabilityId {
    value.parse().expect("fixture capability ID is valid")
}
