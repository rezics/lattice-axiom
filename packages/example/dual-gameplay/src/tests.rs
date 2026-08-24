//! D1 registration, equivalence, fault, lifecycle, and batching evidence.

use std::collections::BTreeMap;

use latticeaxiom_abi::{AbiContractError, LAX_ABI_MAGIC};
use latticeaxiom_core::{CanonicalHash, PackageName};
use latticeaxiom_runtime_contracts::{
    MetricHistoryPolicy, ObservabilityCatalogPolicy, ObservabilityRuntimePolicies,
    SettingsCatalogFragment, SettingsCatalogPolicy, ValidatedObservabilityCatalog,
    ValidatedSettingsCatalog,
};

use super::*;

#[test]
fn sdk_generates_static_glue_portable_batch_plan_manifest_and_c_binding() {
    let artifacts = fixture_artifacts()
        .unwrap_or_else(|error| panic!("fixture artifacts must generate: {error}"));
    assert!(artifacts.verify().is_ok());
    assert_eq!(artifacts.static_adapters.len(), 1);
    assert_eq!(artifacts.dynamic_batch_shims.len(), 1);
    assert_eq!(artifacts.dynamic_batch_shims[0].columns.len(), 3);
    assert!(artifacts.dynamic_batch_shims[0].command_sink);
    assert!(
        artifacts.static_adapters[0]
            .row_kernel_symbol
            .ends_with("integrate_and_mine")
    );
    let header = generated_c_binding();
    assert!(header.contains("LaxSystemCallV0_1"));
    assert!(header.contains("LaxEcsBatchTableV0_1"));
    assert!(header.contains("LaxCommandBufferTableV0_1"));
}

#[test]
fn typed_setting_metric_info_and_inspect_contributions_validate() {
    let contributions = generated_contributions()
        .unwrap_or_else(|error| panic!("contributions must generate: {error}"));
    assert_eq!(
        contributions.static_callbacks,
        contributions.dynamic_callbacks
    );
    assert_eq!(contributions.static_callbacks.len(), 3);
    assert!(
        contributions
            .static_callbacks
            .values()
            .filter(|row| row.batched)
            .count()
            >= 2
    );

    let owner: PackageName = "@example/dual-gameplay"
        .parse()
        .unwrap_or_else(|error| panic!("owner must parse: {error}"));
    let settings = ValidatedSettingsCatalog::compile(
        [SettingsCatalogFragment::new(
            owner,
            vec![contributions.setting.clone()],
        )],
        SettingsCatalogPolicy::default(),
    );
    assert!(settings.is_ok());

    let runtime = ObservabilityRuntimePolicies {
        metrics: BTreeMap::from([(
            contributions.metric.id.clone(),
            MetricHistoryPolicy {
                max_bytes: 16 * 1_024,
            },
        )]),
        visualizers: BTreeMap::new(),
    };
    assert!(
        ValidatedObservabilityCatalog::compile(
            contributions.observability_catalog(),
            &runtime,
            ObservabilityCatalogPolicy::default(),
        )
        .is_ok()
    );
}

#[test]
fn static_and_portable_receipts_match_for_n_ticks() {
    let static_evidence = run_realization(Realization::StaticDirect, 8, 37)
        .unwrap_or_else(|error| panic!("static fixture must run: {error}"));
    let dynamic_evidence = run_realization(Realization::PortableBatch, 8, 37)
        .unwrap_or_else(|error| panic!("portable fixture must run: {error}"));
    assert_eq!(static_evidence.receipt, dynamic_evidence.receipt);
    assert_eq!(static_evidence.ffi_system_calls, 0);
    assert_eq!(dynamic_evidence.ffi_system_calls, 8);
    assert_eq!(static_evidence.receipt.state_hashes.len(), 8);
    assert!(!static_evidence.receipt.commands.is_empty());
}

#[test]
fn canonical_snapshot_matches_golden_fixture() {
    let receipt = run_equivalence(5, 4)
        .unwrap_or_else(|error| panic!("equivalence fixture must run: {error}"));
    let actual = String::from_utf8_lossy(&receipt.snapshot_bytes);
    assert_eq!(
        actual.trim(),
        include_str!("../tests/goldens/canonical-snapshot-v1.json").trim()
    );
}

#[test]
fn wrong_abi_and_manifest_fail_before_business_callback() {
    let artifacts = fixture_artifacts()
        .unwrap_or_else(|error| panic!("fixture artifacts must generate: {error}"));
    let mut wrong_abi = ReferenceDynamicModule::new()
        .unwrap_or_else(|error| panic!("module must construct: {error}"));
    let mut header = wrong_abi
        .entry_header()
        .unwrap_or_else(|error| panic!("header must construct: {error}"));
    header.magic = LAX_ABI_MAGIC ^ 1;
    let error = wrong_abi
        .entry(
            &header,
            artifacts.registration.manifest.semantic_hash,
            artifacts.callback_map.callback_map_hash,
        )
        .err()
        .unwrap_or_else(|| panic!("wrong ABI must fail"));
    assert!(matches!(
        error,
        FixtureError::Abi(AbiContractError::WrongMagic { .. })
    ));
    assert_eq!(wrong_abi.callback_calls(), 0);

    let mut wrong_manifest = ReferenceDynamicModule::new()
        .unwrap_or_else(|error| panic!("module must construct: {error}"));
    let header = wrong_manifest
        .entry_header()
        .unwrap_or_else(|error| panic!("header must construct: {error}"));
    let error = wrong_manifest
        .entry(
            &header,
            CanonicalHash::digest(b"wrong-manifest"),
            artifacts.callback_map.callback_map_hash,
        )
        .err()
        .unwrap_or_else(|| panic!("wrong manifest must fail"));
    assert!(matches!(error, FixtureError::ManifestMismatch { .. }));
    assert_eq!(wrong_manifest.callback_calls(), 0);
}

#[test]
fn panic_discards_staging_fails_instance_and_stops_callbacks() {
    let artifacts = fixture_artifacts()
        .unwrap_or_else(|error| panic!("fixture artifacts must generate: {error}"));
    let mut module = ReferenceDynamicModule::new()
        .unwrap_or_else(|error| panic!("module must construct: {error}"));
    let header = module
        .entry_header()
        .unwrap_or_else(|error| panic!("header must construct: {error}"));
    assert!(
        module
            .entry(
                &header,
                artifacts.registration.manifest.semantic_hash,
                artifacts.callback_map.callback_map_hash,
            )
            .is_ok()
    );
    assert!(module.start().is_ok());
    let (authoritative, intents, targets) = canonical_fixture_rows(4);
    let mut staged = authoritative.clone();
    let result = GeneratedPortableTable::generated().invoke(
        &mut module,
        &mut staged,
        &intents,
        &targets,
        0,
        FaultInjection::PanicAfterFirstRow,
    );
    assert!(matches!(result, Err(FixtureError::CallbackPanicked)));
    assert_eq!(module.phase(), LifecyclePhase::Failed);
    assert_eq!(module.callback_calls(), 1);
    assert_ne!(staged, authoritative);
    assert_eq!(authoritative, canonical_fixture_rows(4).0);
}

#[test]
fn lifecycle_and_callback_after_stop_fail_deterministically() {
    let artifacts = fixture_artifacts()
        .unwrap_or_else(|error| panic!("fixture artifacts must generate: {error}"));
    let mut module = ReferenceDynamicModule::new()
        .unwrap_or_else(|error| panic!("module must construct: {error}"));
    assert!(matches!(
        module.start(),
        Err(FixtureError::InvalidLifecycle { .. })
    ));
    let header = module
        .entry_header()
        .unwrap_or_else(|error| panic!("header must construct: {error}"));
    assert!(
        module
            .entry(
                &header,
                artifacts.registration.manifest.semantic_hash,
                artifacts.callback_map.callback_map_hash,
            )
            .is_ok()
    );
    assert!(module.start().is_ok());
    assert!(matches!(
        module.start(),
        Err(FixtureError::InvalidLifecycle { .. })
    ));
    assert!(module.quiesce().is_ok());
    assert!(module.stop().is_ok());
    let calls_before = module.callback_calls();
    let (mut states, intents, targets) = canonical_fixture_rows(1);
    let result = GeneratedPortableTable::generated().invoke(
        &mut module,
        &mut states,
        &intents,
        &targets,
        0,
        FaultInjection::None,
    );
    assert!(matches!(result, Err(FixtureError::CallbackAfterStop)));
    assert_eq!(module.callback_calls(), calls_before);
    assert!(module.destroy().is_ok());
    assert_eq!(module.phase(), LifecyclePhase::Destroyed);
}

#[test]
fn portable_calls_scale_with_bounded_batch_groups_not_entities() {
    for entity_count in [1, 64, 256, 1_000, 10_000, 100_000] {
        let diagnostic = ffi_batch_call_diagnostic(entity_count);
        assert_eq!(diagnostic.per_entity_counterexample, entity_count);
        assert!(diagnostic.portable_callback_count <= diagnostic.batch_count);
        if entity_count > 1 {
            assert!(diagnostic.portable_callback_count < entity_count);
        }
    }
    let evidence = run_realization(Realization::PortableBatch, 1, 10_000)
        .unwrap_or_else(|error| panic!("portable scale fixture must run: {error}"));
    assert_eq!(
        evidence.ffi_system_calls,
        u64::try_from(ffi_batch_call_diagnostic(10_000).portable_callback_count)
            .unwrap_or(u64::MAX)
    );
}
