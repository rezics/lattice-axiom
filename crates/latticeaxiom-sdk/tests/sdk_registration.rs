//! SDK registration macro, artifact, determinism, and fault conformance.

use std::collections::BTreeMap;

use latticeaxiom_core::{CanonicalHash, PackageVersion, SourceId, SourceProvenance, StableId};
use latticeaxiom_sdk::{
    CommandSink, FixedTick, NativeStaticOnlyReason, ProducerInput, ProvenanceCatalog, Read,
    RegistrationIr, RegistrationIrError, SystemPortability, With, Write, component,
    registration_ir, system,
};
use proptest::prelude::*;

#[repr(C)]
#[component(
    id = "example:component/position",
    schema = "example:schema/position@1",
    mode = "generated-shared-schema"
)]
struct Position {
    x: i32,
    y: i32,
}

#[repr(C)]
#[component(
    id = "example:component/velocity",
    schema = "example:schema/velocity@1",
    mode = "generated-shared-schema"
)]
struct Velocity {
    x: i32,
    y: i32,
}

#[repr(C)]
#[component(id = "example:component/host-cache", mode = "host-typed")]
struct HostCache {
    value: u64,
}

#[allow(dead_code)]
#[system(
    id = "example:system/integrate",
    callback = "example:callback/integrate@1",
    stage = "latticeaxiom:system-stage/gameplay/fixed@1",
    policy = "dual",
    before("example:system/dampen")
)]
fn integrate(
    _position: Write<'_, Position>,
    _velocity: Read<'_, Velocity>,
    _tick: FixedTick,
    _commands: CommandSink<'_>,
) {
}

#[allow(dead_code)]
#[system(
    id = "example:system/dampen",
    callback = "example:callback/dampen@1",
    stage = "latticeaxiom:system-stage/gameplay/fixed@1",
    policy = "dual",
    after("example:system/integrate")
)]
fn dampen(_velocity: Write<'_, Velocity>, _moving: With<Position>) {}

struct Local<T>(T);

#[allow(dead_code)]
#[system(
    id = "example:system/static-inspection",
    callback = "example:callback/static-inspection@1",
    stage = "latticeaxiom:system-stage/presentation/update@1",
    policy = "auto"
)]
fn static_inspection(_local: Local<u32>) {}

fn dual_ir() -> Result<RegistrationIr, RegistrationIrError> {
    registration_ir! {
        package = "@example/dual-gameplay",
        version = "0.1.0",
        components = [Velocity, Position],
        systems = [integrate, dampen],
    }
}

fn reordered_dual_ir() -> Result<RegistrationIr, RegistrationIrError> {
    registration_ir! {
        package = "@example/dual-gameplay",
        version = "0.1.0",
        components = [Position, Velocity],
        systems = [dampen, integrate],
    }
}

fn static_ir() -> Result<RegistrationIr, RegistrationIrError> {
    registration_ir! {
        package = "@example/static-inspection",
        version = "0.1.0",
        components = [],
        systems = [static_inspection],
    }
}

#[allow(dead_code)]
#[system(
    id = "example:system/host-cache",
    callback = "example:callback/host-cache@1",
    stage = "latticeaxiom:system-stage/gameplay/fixed@1",
    policy = "auto"
)]
fn host_cache(_cache: Read<'_, HostCache>) {}

fn host_component_ir() -> Result<RegistrationIr, RegistrationIrError> {
    registration_ir! {
        package = "@example/host-cache",
        version = "0.1.0",
        components = [HostCache],
        systems = [host_cache],
    }
}

#[test]
fn explicit_lists_compile_to_order_independent_ir_and_artifacts() {
    let first = fixture_artifacts(dual_ir(), "src/gameplay.rs", "toolchain-a");
    let second = fixture_artifacts(reordered_dual_ir(), "src/gameplay.rs", "toolchain-a");
    let (Ok(first), Ok(second)) = (first, second) else {
        panic!("fixture artifacts must compile");
    };

    assert_eq!(first.registration, second.registration);
    assert_eq!(first.callback_map, second.callback_map);
    assert_eq!(first.artifact_hash, second.artifact_hash);
    assert!(first.verify().is_ok());
}

#[test]
fn callback_map_matches_golden() {
    let Ok(artifacts) = fixture_artifacts(dual_ir(), "src/gameplay.rs", "toolchain-a") else {
        panic!("fixture artifacts must compile");
    };
    let encoded = serde_json::to_string_pretty(&artifacts.callback_map)
        .unwrap_or_else(|error| panic!("callback map must serialize: {error}"));

    assert_eq!(
        encoded.trim(),
        include_str!("goldens/callback-map-v1.json").trim()
    );
}

#[test]
fn provenance_and_producer_changes_do_not_change_semantics() {
    let first = fixture_artifacts(dual_ir(), "src/gameplay.rs", "toolchain-a");
    let second = fixture_artifacts(dual_ir(), "generated/gameplay.rs", "toolchain-b");
    let (Ok(first), Ok(second)) = (first, second) else {
        panic!("fixture artifacts must compile");
    };

    assert_eq!(
        first.registration.manifest.semantic_hash,
        second.registration.manifest.semantic_hash
    );
    assert_eq!(first.callback_map, second.callback_map);
    assert_eq!(first.artifact_hash, second.artifact_hash);
    assert_ne!(
        first.provenance.provenance_hash,
        second.provenance.provenance_hash
    );
}

#[test]
fn incomplete_or_extra_provenance_fails_with_stable_code() {
    let Ok(ir) = dual_ir() else {
        panic!("fixture IR must compile");
    };
    let producer = ProducerInput::current(CanonicalHash::digest(b"input"))
        .unwrap_or_else(|error| panic!("producer fixture must compile: {error}"));
    let missing = ProvenanceCatalog::default();
    let error = ir
        .generate(&producer, &missing)
        .err()
        .unwrap_or_else(|| panic!("missing provenance must fail"));
    assert_eq!(error.code(), "LAX-SDK-012");

    let mut rows = provenance_rows(&ir, "src/gameplay.rs", "toolchain-a");
    rows.insert(
        stable_id("example:component/unknown"),
        provenance("src/unknown.rs", "toolchain-a"),
    );
    let error = ir
        .generate(&producer, &ProvenanceCatalog::new(rows))
        .err()
        .unwrap_or_else(|| panic!("unknown provenance must fail"));
    assert_eq!(error.code(), "LAX-SDK-013");
}

#[test]
fn auto_policy_records_static_only_reason_and_emits_no_batch_shim() {
    let Ok(ir) = static_ir() else {
        panic!("static fixture IR must compile");
    };
    let Ok(artifacts) = fixture_artifacts(Ok(ir), "src/inspect.rs", "toolchain-a") else {
        panic!("static artifacts must compile");
    };
    let Some(system) = artifacts.registration.systems.values().next() else {
        panic!("static system must exist");
    };
    let Some(callback) = artifacts.callback_map.callbacks.get(&system.callback) else {
        panic!("callback binding must exist");
    };
    assert_eq!(
        callback.portability,
        SystemPortability::NativeStaticOnly {
            reasons: [NativeStaticOnlyReason::UnsupportedSystemParameter]
                .into_iter()
                .collect()
        }
    );
    assert!(artifacts.dynamic_batch_shims.is_empty());
}

#[test]
fn auto_policy_falls_back_for_package_private_host_components() {
    let Ok(artifacts) = fixture_artifacts(host_component_ir(), "src/host-cache.rs", "toolchain-a")
    else {
        panic!("host component artifacts must compile");
    };
    let Some(binding) = artifacts.callback_map.callbacks.values().next() else {
        panic!("host component callback must exist");
    };
    assert_eq!(
        binding.portability,
        SystemPortability::NativeStaticOnly {
            reasons: [NativeStaticOnlyReason::UnsupportedSystemParameter]
                .into_iter()
                .collect()
        }
    );
    assert!(artifacts.dynamic_batch_shims.is_empty());
}

#[test]
fn tampered_hash_and_callback_binding_fail_before_code_use() {
    let Ok(mut artifacts) = fixture_artifacts(dual_ir(), "src/gameplay.rs", "toolchain-a") else {
        panic!("fixture artifacts must compile");
    };
    artifacts.callback_map.callback_map_hash = CanonicalHash::digest(b"tampered");
    let error = artifacts
        .verify()
        .err()
        .unwrap_or_else(|| panic!("tampered callback map must fail"));
    assert_eq!(error.code(), "LAX-SDK-017");

    let Ok(mut artifacts) = fixture_artifacts(dual_ir(), "src/gameplay.rs", "toolchain-a") else {
        panic!("fixture artifacts must compile");
    };
    let Some(binding) = artifacts.callback_map.callbacks.values_mut().next() else {
        panic!("callback binding must exist");
    };
    binding.signature_hash = CanonicalHash::digest(b"wrong-signature");
    artifacts.callback_map.callback_map_hash = artifacts
        .callback_map
        .recompute_hash()
        .unwrap_or_else(|error| panic!("tampered callback map must rehash: {error}"));
    artifacts.artifact_hash = artifacts
        .recompute_artifact_hash()
        .unwrap_or_else(|error| panic!("tampered artifact must rehash: {error}"));
    let error = artifacts
        .verify()
        .err()
        .unwrap_or_else(|| panic!("wrong signature must fail"));
    assert_eq!(error.code(), "LAX-SDK-019");
}

#[test]
fn nested_artifact_schema_and_producer_contract_are_verified() {
    let Ok(mut artifacts) = fixture_artifacts(dual_ir(), "src/gameplay.rs", "toolchain-a") else {
        panic!("fixture artifacts must compile");
    };
    artifacts.callback_map.schema_version = 2;
    artifacts.callback_map.callback_map_hash = artifacts
        .callback_map
        .recompute_hash()
        .unwrap_or_else(|error| panic!("callback map must rehash: {error}"));
    artifacts.artifact_hash = artifacts
        .recompute_artifact_hash()
        .unwrap_or_else(|error| panic!("artifact must rehash: {error}"));
    let error = artifacts
        .verify()
        .err()
        .unwrap_or_else(|| panic!("nested schema mismatch must fail"));
    assert_eq!(error.code(), "LAX-SDK-016");

    let Ok(mut artifacts) = fixture_artifacts(dual_ir(), "src/gameplay.rs", "toolchain-a") else {
        panic!("fixture artifacts must compile");
    };
    artifacts.provenance.producer.sdk_id = stable_id("latticeaxiom:sdk/registration-authoring");
    let error = artifacts
        .verify()
        .err()
        .unwrap_or_else(|| panic!("unversioned producer must fail"));
    assert_eq!(error.code(), "LAX-SDK-015");
}

#[test]
fn generated_artifacts_reject_unknown_fields() {
    let Ok(artifacts) = fixture_artifacts(dual_ir(), "src/gameplay.rs", "toolchain-a") else {
        panic!("fixture artifacts must compile");
    };
    let mut value = serde_json::to_value(&artifacts.callback_map)
        .unwrap_or_else(|error| panic!("callback map must serialize: {error}"));
    let Some(object) = value.as_object_mut() else {
        panic!("callback map must be an object");
    };
    object.insert("unknown".to_owned(), serde_json::json!(true));
    assert!(serde_json::from_value::<latticeaxiom_sdk::CallbackMapArtifact>(value).is_err());
}

fn fixture_artifacts(
    ir: Result<RegistrationIr, RegistrationIrError>,
    path: &str,
    toolchain: &str,
) -> Result<latticeaxiom_sdk::GeneratedRegistrationArtifacts, RegistrationIrError> {
    let ir = ir?;
    let producer = ProducerInput::current(CanonicalHash::digest(b"input"))?;
    ir.generate(
        &producer,
        &ProvenanceCatalog::new(provenance_rows(&ir, path, toolchain)),
    )
}

fn provenance_rows(
    ir: &RegistrationIr,
    path: &str,
    toolchain: &str,
) -> BTreeMap<StableId, SourceProvenance> {
    ir.components()
        .keys()
        .chain(ir.systems().keys())
        .map(|id| (id.clone(), provenance(path, toolchain)))
        .collect()
}

fn provenance(path: &str, toolchain: &str) -> SourceProvenance {
    SourceProvenance::new(
        "latticeaxiom:source/sdk-test"
            .parse::<SourceId>()
            .unwrap_or_else(|error| panic!("source ID fixture must parse: {error}")),
        path,
        CanonicalHash::digest(path.as_bytes()),
        None,
        Vec::new(),
    )
    .and_then(|source| {
        source.with_generation(
            Some("latticeaxiom-sdk-test".to_owned()),
            Some(toolchain.to_owned()),
            Some("registration-fragment".to_owned()),
        )
    })
    .unwrap_or_else(|error| panic!("provenance fixture must validate: {error}"))
}

fn stable_id(value: &str) -> StableId {
    value
        .parse()
        .unwrap_or_else(|error| panic!("stable ID fixture must parse: {error}"))
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    #[test]
    fn semantic_hash_is_source_provenance_invariant(suffix in "[a-z]{1,12}") {
        let first = fixture_artifacts(dual_ir(), "src/gameplay.rs", "toolchain-a")
            .map_err(|error| TestCaseError::fail(error.to_string()))?;
        let second_path = format!("generated/{suffix}.rs");
        let second = fixture_artifacts(dual_ir(), &second_path, "toolchain-b")
            .map_err(|error| TestCaseError::fail(error.to_string()))?;

        prop_assert_eq!(
            first.registration.manifest.semantic_hash,
            second.registration.manifest.semantic_hash
        );
        prop_assert_eq!(first.callback_map, second.callback_map);
        prop_assert_eq!(first.artifact_hash, second.artifact_hash);
        prop_assert_ne!(first.provenance.provenance_hash, second.provenance.provenance_hash);
    }
}

#[test]
fn producer_version_is_provenance_only() {
    let Ok(ir) = dual_ir() else {
        panic!("fixture IR must compile");
    };
    let mut first = ProducerInput::current(CanonicalHash::digest(b"input"))
        .unwrap_or_else(|error| panic!("producer fixture must compile: {error}"));
    let provenance = ProvenanceCatalog::new(provenance_rows(&ir, "src/gameplay.rs", "toolchain-a"));
    let first_artifacts = ir
        .generate(&first, &provenance)
        .unwrap_or_else(|error| panic!("first artifacts must compile: {error}"));
    first.sdk_version = "0.1.1"
        .parse::<PackageVersion>()
        .unwrap_or_else(|error| panic!("version fixture must parse: {error}"));
    let second_artifacts = ir
        .generate(&first, &provenance)
        .unwrap_or_else(|error| panic!("second artifacts must compile: {error}"));

    assert_eq!(
        first_artifacts.registration.manifest.semantic_hash,
        second_artifacts.registration.manifest.semantic_hash
    );
    assert_ne!(
        first_artifacts.provenance.provenance_hash,
        second_artifacts.provenance.provenance_hash
    );
}
