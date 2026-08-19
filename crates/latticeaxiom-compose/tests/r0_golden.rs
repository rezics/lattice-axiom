//! Initial R0 authoring corpus checked through the trusted ambient adapter.
//!
//! These fixtures pin contracts and typed DTO conversion. They are not the
//! controlled-import or worker resource-policy gate.

#![cfg(feature = "nickel-evaluator")]

use std::{fs, path::Path};

use latticeaxiom_compose::{
    GameProfileSpec, NickelEvaluationError, NickelEvaluationLimits, PackageSpec,
    evaluate_trusted_nickel_source,
};
use latticeaxiom_core::canonical_json_bytes;
use serde::{Serialize, de::DeserializeOwned};

const WORKSPACE_ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");

#[test]
fn embedded_core_package_matches_the_normative_golden() {
    let actual: PackageSpec = evaluate_fixture("fixtures/r0/positive/core-empty-package.ncl");
    actual
        .validate()
        .unwrap_or_else(|error| panic!("evaluated package failed normative validation: {error}"));
    let expected: PackageSpec = read_json("fixtures/r0/positive/core-empty-package.golden.json");
    assert_canonical_equal(&actual, &expected);
}

#[test]
fn embedded_package_accepts_nfc_unicode_provenance() {
    let actual: PackageSpec =
        evaluate_fixture("fixtures/r0/positive/unicode-provenance-package.ncl");
    actual
        .validate()
        .unwrap_or_else(|error| panic!("Unicode package failed normative validation: {error}"));
    let expected: PackageSpec =
        read_json("fixtures/r0/positive/unicode-provenance-package.golden.json");
    assert_canonical_equal(&actual, &expected);
}

#[test]
fn embedded_headless_profile_matches_the_normative_golden() {
    let actual: GameProfileSpec = evaluate_fixture("fixtures/r0/positive/headless-profile.ncl");
    actual
        .validate()
        .unwrap_or_else(|error| panic!("evaluated profile failed normative validation: {error}"));
    let expected: GameProfileSpec = read_json("fixtures/r0/positive/headless-profile.golden.json");
    assert_canonical_equal(&actual, &expected);
}

#[test]
fn embedded_tool_profile_accepts_an_explicit_versioned_policy() {
    let actual: GameProfileSpec = evaluate_fixture("fixtures/r0/positive/tool-profile.ncl");
    actual
        .validate()
        .unwrap_or_else(|error| panic!("evaluated tool profile failed validation: {error}"));
    let expected: GameProfileSpec = read_json("fixtures/r0/positive/tool-profile.golden.json");
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
    assert_negative::<GameProfileSpec>("unversioned-tool-evaluation-policy");
}

#[test]
fn embedded_typed_boundary_rejects_decomposed_unicode_provenance() {
    let path = "fixtures/r0/typed-negative/non-nfc-provenance.ncl";
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
    let logical_path = format!("fixtures/r0/negative/{fixture_name}.ncl");
    let Err(error) = try_evaluate_fixture::<T>(&logical_path) else {
        panic!("negative fixture `{fixture_name}` unexpectedly evaluated");
    };
    assert_eq!(error.code(), "compose.evaluation_failed");

    let expected_path = Path::new(WORKSPACE_ROOT)
        .join("fixtures/r0/negative")
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
