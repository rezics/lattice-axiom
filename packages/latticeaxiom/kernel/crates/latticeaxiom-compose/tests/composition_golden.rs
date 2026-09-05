//! Initial R0 authoring corpus checked through the trusted ambient adapter.
//!
//! These fixtures pin contracts and typed DTO conversion. They are not the
//! controlled-import or worker resource-policy gate.

#![cfg(feature = "nickel-evaluator")]

use std::{fs, path::Path};

use latticeaxiom_compose::{
    COMPOSITION_SCHEMA_VERSION, CompositionSpec, GAME_PROFILE_MODEL_VERSION, GameProfileSpec,
    NICKEL_LIBRARY_CONTRACT_MAJOR, NickelEvaluationError, NickelEvaluationLimits,
    PACKAGE_MODEL_VERSION, PackageSpec, R0_AUTHORING_CORPUS_MAJOR,
    REGISTRATION_MANIFEST_SCHEMA_VERSION, TrustClass, evaluate_trusted_nickel_source,
};
use latticeaxiom_core::{CanonicalHash, SourceProvenance, TargetTriple, canonical_json_bytes};
use serde::{Serialize, de::DeserializeOwned};

const WORKSPACE_ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../../../..");

#[test]
fn embedded_version_axes_match_rust_constants_and_the_golden() {
    let actual: serde_json::Value =
        evaluate_fixture("fixtures/composition/positive/version-axes.ncl");
    let expected: serde_json::Value =
        read_json("fixtures/composition/positive/version-axes.golden.json");
    assert_canonical_equal(&actual, &expected);

    for (field, version) in [
        ("library_contract_major", NICKEL_LIBRARY_CONTRACT_MAJOR),
        ("corpus_major", R0_AUTHORING_CORPUS_MAJOR),
        ("package_model", PACKAGE_MODEL_VERSION),
        ("game_profile_model", GAME_PROFILE_MODEL_VERSION),
        ("composition_schema", COMPOSITION_SCHEMA_VERSION),
        (
            "registration_manifest_schema",
            REGISTRATION_MANIFEST_SCHEMA_VERSION,
        ),
    ] {
        assert_eq!(
            actual.get(field).and_then(serde_json::Value::as_u64),
            Some(u64::from(version)),
            "Nickel version axis `{field}` drifted from Rust"
        );
    }
    assert_eq!(
        actual
            .get("contract_major_alias")
            .and_then(serde_json::Value::as_u64),
        Some(u64::from(NICKEL_LIBRARY_CONTRACT_MAJOR))
    );
}

#[test]
fn embedded_core_package_matches_the_normative_golden() {
    let actual: PackageSpec =
        evaluate_fixture("fixtures/composition/positive/core-empty-package.ncl");
    actual
        .validate()
        .unwrap_or_else(|error| panic!("evaluated package failed normative validation: {error}"));
    let expected: PackageSpec =
        read_json("fixtures/composition/positive/core-empty-package.golden.json");
    assert_canonical_equal(&actual, &expected);
}

#[test]
fn embedded_package_accepts_nfc_unicode_provenance() {
    let actual: PackageSpec =
        evaluate_fixture("fixtures/composition/positive/unicode-provenance-package.ncl");
    actual
        .validate()
        .unwrap_or_else(|error| panic!("Unicode package failed normative validation: {error}"));
    let expected: PackageSpec =
        read_json("fixtures/composition/positive/unicode-provenance-package.golden.json");
    assert_canonical_equal(&actual, &expected);
}

#[test]
fn embedded_headless_profile_matches_the_normative_golden() {
    let actual: GameProfileSpec =
        evaluate_fixture("fixtures/composition/positive/headless-profile.ncl");
    actual
        .validate()
        .unwrap_or_else(|error| panic!("evaluated profile failed normative validation: {error}"));
    assert!(
        actual
            .features
            .values()
            .any(|features| !features.is_empty())
    );
    assert!(!actual.policy.namespace_grants.is_empty());
    assert_eq!(actual.policy.maximum_trust, TrustClass::Build);
    assert!(actual.policy.allow_force_override);
    assert!(actual.policy.allow_recovery);
    let expected: GameProfileSpec =
        read_json("fixtures/composition/positive/headless-profile.golden.json");
    assert_canonical_equal(&actual, &expected);
}

#[test]
fn embedded_headless_profile_normalizes_to_the_composition_golden() {
    let logical_path = "fixtures/composition/positive/headless-profile.ncl";
    let profile: GameProfileSpec = evaluate_fixture(logical_path);
    let source = read_source(logical_path);
    let source_id = profile.source_universe.first().map_or_else(
        || panic!("headless profile fixture must declare one source"),
        |candidate| candidate.source_id.clone(),
    );
    let provenance = SourceProvenance::new(
        source_id,
        logical_path,
        CanonicalHash::digest(source.as_bytes()),
        None,
        Vec::new(),
    )
    .unwrap_or_else(|error| panic!("profile fixture provenance is invalid: {error}"));
    let actual = profile
        .into_composition(target("x86_64-unknown-linux-gnu"), provenance)
        .unwrap_or_else(|error| panic!("profile did not normalize: {error}"));
    let expected: CompositionSpec =
        read_json("fixtures/composition/positive/headless-composition.golden.json");
    assert_canonical_equal(&actual, &expected);
}

#[test]
fn embedded_tool_profile_accepts_an_explicit_versioned_policy() {
    let actual: GameProfileSpec =
        evaluate_fixture("fixtures/composition/positive/tool-profile.ncl");
    actual
        .validate()
        .unwrap_or_else(|error| panic!("evaluated tool profile failed validation: {error}"));
    let expected: GameProfileSpec =
        read_json("fixtures/composition/positive/tool-profile.golden.json");
    assert_canonical_equal(&actual, &expected);
}

#[test]
fn embedded_negative_corpus_retains_its_stable_diagnostic_intent() {
    assert_negative::<PackageSpec>("unknown-package-field");
    assert_negative::<PackageSpec>("malformed-package-version");
    assert_negative::<GameProfileSpec>("malformed-version-range");
    assert_negative::<GameProfileSpec>("unsupported-evaluation-policy");
    assert_negative::<PackageSpec>("feature-domain-outside-package");
    assert_negative::<PackageSpec>("noncanonical-logical-path");
    assert_negative::<GameProfileSpec>("invalid-tool-evaluation-policy");
    assert_negative::<GameProfileSpec>("foreign-tool-evaluation-policy");
    assert_negative::<GameProfileSpec>("unversioned-tool-evaluation-policy");
    assert_negative::<PackageSpec>("invalid-target-triple");
    assert_negative::<PackageSpec>("source-build-with-path");
    assert_negative::<PackageSpec>("data-root-without-path");
    assert_negative::<GameProfileSpec>("auto-realization-with-kind");
    assert_negative::<GameProfileSpec>("exact-realization-without-kind");
}

#[test]
fn embedded_typed_boundary_rejects_decomposed_unicode_provenance() {
    let path = "fixtures/composition/typed-negative/non-nfc-provenance.ncl";
    let Err(error) = try_evaluate_fixture::<PackageSpec>(path) else {
        panic!("typed negative fixture `{path}` unexpectedly evaluated");
    };
    assert_eq!(error.code(), "compose.schema_mismatch");
    assert!(matches!(error, NickelEvaluationError::Schema { .. }));
}

fn evaluate_fixture<T>(logical_path: &str) -> T
where
    T: DeserializeOwned + Serialize,
{
    try_evaluate_fixture(logical_path)
        .unwrap_or_else(|error| panic!("fixture `{logical_path}` did not evaluate: {error}"))
}

fn try_evaluate_fixture<T>(logical_path: &str) -> Result<T, NickelEvaluationError>
where
    T: DeserializeOwned + Serialize,
{
    let physical_path = Path::new(WORKSPACE_ROOT).join(logical_path);
    let source = fs::read_to_string(&physical_path).unwrap_or_else(|error| {
        panic!(
            "failed to read fixture source {}: {error}",
            physical_path.display()
        )
    });
    evaluate_trusted_nickel_source(
        &source,
        physical_path.to_string_lossy(),
        NickelEvaluationLimits::default(),
    )
}

fn read_source(logical_path: &str) -> String {
    let physical_path = Path::new(WORKSPACE_ROOT).join(logical_path);
    fs::read_to_string(&physical_path).unwrap_or_else(|error| {
        panic!(
            "failed to read fixture source {}: {error}",
            physical_path.display()
        )
    })
}

fn target(value: &str) -> TargetTriple {
    value
        .parse()
        .unwrap_or_else(|error| panic!("fixture target `{value}` is invalid: {error}"))
}

fn read_json<T>(logical_path: &str) -> T
where
    T: DeserializeOwned,
{
    let physical_path = Path::new(WORKSPACE_ROOT).join(logical_path);
    let source = fs::read_to_string(&physical_path).unwrap_or_else(|error| {
        panic!(
            "failed to read golden JSON {}: {error}",
            physical_path.display()
        )
    });
    serde_json::from_str(&source).unwrap_or_else(|error| {
        panic!(
            "failed to decode golden JSON {}: {error}",
            physical_path.display()
        )
    })
}

fn assert_canonical_equal<T>(actual: &T, expected: &T)
where
    T: Serialize,
{
    let actual = canonical_json_bytes(actual)
        .unwrap_or_else(|error| panic!("actual fixture did not canonicalize: {error}"));
    let expected = canonical_json_bytes(expected)
        .unwrap_or_else(|error| panic!("golden fixture did not canonicalize: {error}"));
    assert_eq!(actual, expected);
}

fn assert_negative<T>(fixture_name: &str)
where
    T: DeserializeOwned + Serialize,
{
    let logical_path = format!("fixtures/composition/negative/{fixture_name}.ncl");
    let Err(error) = try_evaluate_fixture::<T>(&logical_path) else {
        panic!("negative fixture `{fixture_name}` unexpectedly evaluated");
    };
    assert_eq!(error.code(), "compose.evaluation_failed");

    let expected_path = Path::new(WORKSPACE_ROOT)
        .join("fixtures/composition/negative")
        .join(format!("{fixture_name}.expected.txt"));
    let expected = fs::read_to_string(&expected_path).unwrap_or_else(|read_error| {
        panic!(
            "failed to read expected diagnostic {}: {read_error}",
            expected_path.display()
        )
    });
    let rendered = error.to_string();
    for line in expected.lines().filter(|line| !line.is_empty()) {
        assert!(
            rendered.contains(line),
            "fixture `{fixture_name}` diagnostic did not contain `{line}`:\n{rendered}"
        );
    }
}
