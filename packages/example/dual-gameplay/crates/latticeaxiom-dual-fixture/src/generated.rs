//! Deterministic SDK artifacts and typed package contributions.

use std::collections::{BTreeMap, BTreeSet};

use latticeaxiom_compose::{
    CostClass, DiagnosticMetricSpec, DisclosureLevel, InfoItemSpec, InspectFragmentProviderSpec,
    InspectTargetKind, MetricAggregation, ObservabilityCatalog, RuntimeApplyImpact,
    SettingAuthority, SettingScope, SettingSensitivity, SettingSpec, UpdatePolicy, ValueType,
};
use latticeaxiom_core::{
    CanonicalHash, PackageName, SourceId, SourceProvenance, StableId, canonical_json_hash,
};
use latticeaxiom_sdk::{
    GeneratedRegistrationArtifacts, ProducerInput, ProvenanceCatalog, RegistrationIr,
};
use serde::{Deserialize, Serialize};

use crate::{FixtureError, gameplay::registration_ir};

/// Contribution callback family generated beside the gameplay registration.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum CallbackContributionKind {
    /// Typed runtime setting read.
    SettingRead,
    /// Batched diagnostic metric sample.
    MetricSample,
    /// Batched target-inspection fragment sample.
    InspectSample,
}

/// One generated static/dynamic callback-map row for package contributions.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CallbackContribution {
    /// Stable versioned callback key.
    pub callback: StableId,
    /// Contribution callback family.
    pub kind: CallbackContributionKind,
    /// Canonical signature hash shared by both realizations.
    pub signature_hash: CanonicalHash,
    /// Whether the callback consumes a batch instead of one item per call.
    pub batched: bool,
}

/// Generated typed setting, metric, inspect provider, and callback maps.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GeneratedContributions {
    /// Package typed setting declaration.
    pub setting: SettingSpec,
    /// Package diagnostic metric declaration.
    pub metric: DiagnosticMetricSpec,
    /// Package information item declaration.
    pub info_item: InfoItemSpec,
    /// Package inspect fragment provider declaration.
    pub inspect: InspectFragmentProviderSpec,
    /// Static callback bindings generated from the declarations.
    pub static_callbacks: BTreeMap<StableId, CallbackContribution>,
    /// Portable callback bindings generated from the same declarations.
    pub dynamic_callbacks: BTreeMap<StableId, CallbackContribution>,
}

impl GeneratedContributions {
    /// Converts observability contributions into the shared declaration catalog.
    #[must_use]
    pub fn observability_catalog(&self) -> ObservabilityCatalog {
        ObservabilityCatalog {
            info_items: BTreeMap::from([(self.info_item.id.clone(), self.info_item.clone())]),
            metrics: BTreeMap::from([(self.metric.id.clone(), self.metric.clone())]),
            inspect: BTreeMap::from([(self.inspect.id.clone(), self.inspect.clone())]),
            visualizers: BTreeMap::new(),
        }
    }
}

/// Builds the sealed registration IR declared by the fixture macros.
///
/// # Errors
///
/// Returns an SDK error when a declaration violates identifier, schema,
/// parameter, or scheduling contracts.
pub fn fixture_registration_ir() -> Result<RegistrationIr, FixtureError> {
    Ok(registration_ir()?)
}

/// Generates manifest, callback map, static adapter, and portable batch plans.
///
/// # Errors
///
/// Returns a fixture error if IR, provenance, or artifact hashes are invalid.
pub fn fixture_artifacts() -> Result<GeneratedRegistrationArtifacts, FixtureError> {
    let ir = fixture_registration_ir()?;
    let producer = ProducerInput::current(CanonicalHash::digest(b"d1-dual-gameplay-source-v1"))?;
    let source_id: SourceId = "latticeaxiom:source/d1-dual-gameplay".parse()?;
    let source = SourceProvenance::new(
        source_id,
        "packages/example/dual-gameplay/src/gameplay.rs",
        CanonicalHash::digest(include_bytes!("gameplay.rs")),
        None,
        Vec::new(),
    )?;
    let rows = ir
        .components()
        .keys()
        .chain(ir.systems().keys())
        .map(|id| (id.clone(), source.clone()))
        .collect();
    let artifacts = ir.generate(&producer, &ProvenanceCatalog::new(rows))?;
    artifacts.verify()?;
    Ok(artifacts)
}

/// Returns the generated C ABI binding emitted from the ABI schema.
#[must_use]
pub fn generated_c_binding() -> &'static str {
    latticeaxiom_abi::GENERATED_C_HEADER
}

/// Generates typed setting, metric, information, inspect, and callback rows.
///
/// # Errors
///
/// Returns an identifier or canonical encoding error if a declaration is no
/// longer accepted by shared contracts.
pub fn generated_contributions() -> Result<GeneratedContributions, FixtureError> {
    let owner: PackageName = "@example/dual-gameplay".parse()?;
    let setting = SettingSpec {
        id: stable("example:setting/max-mining-effort")?,
        declared_by: owner.clone(),
        schema_version: 1,
        value_type: ValueType::Integer {
            min: Some(0),
            max: Some(1_000),
            step: Some(1),
        },
        default: serde_json::json!(64),
        allowed_scopes: BTreeSet::from([SettingScope::World]),
        default_scope: SettingScope::World,
        authority: SettingAuthority::WorldOwner,
        apply_impact: RuntimeApplyImpact::Immediate,
        category: stable("example:setting-category/gameplay")?,
        order: 10,
        label_key: "example.dual-gameplay.max-mining-effort.label".to_owned(),
        description_key: "example.dual-gameplay.max-mining-effort.description".to_owned(),
        visibility: None,
        enabled_when: None,
        sensitivity: SettingSensitivity::Ordinary,
        replacement: None,
    };
    let metric = DiagnosticMetricSpec {
        id: stable("example:metric/break-command-count")?,
        declared_by: owner.clone(),
        unit: "commands".to_owned(),
        aggregation: MetricAggregation::Latest,
        sampling_interval_ms: 100,
        history_limit: 128,
        cost: CostClass::Trivial,
    };
    let info_item = InfoItemSpec {
        id: stable("example:info-item/agent-stamina")?,
        declared_by: owner.clone(),
        label_key: "example.dual-gameplay.agent-stamina.label".to_owned(),
        unit: Some("work-units".to_owned()),
        value_schema: "example:schema/agent-stamina-sample@1".parse()?,
        disclosure: DisclosureLevel::Detail,
        update: UpdatePolicy::FixedInterval(100),
        cost: CostClass::Trivial,
        callback: stable("example:callback/sample-agent-stamina@1")?,
    };
    let inspect = InspectFragmentProviderSpec {
        id: stable("example:inspect-provider/agent-gameplay")?,
        declared_by: owner,
        target_kinds: BTreeSet::from([InspectTargetKind::Entity]),
        primary_keys: BTreeSet::from(["example.agent-gameplay".to_owned()]),
        extension_slots: BTreeSet::new(),
        after: BTreeSet::new(),
        callback: stable("example:callback/inspect-agent-gameplay@1")?,
    };
    let callback_specs = [
        (
            "example:callback/read-max-mining-effort@1",
            CallbackContributionKind::SettingRead,
            false,
            "setting-read(max-mining-effort)->u32@1",
        ),
        (
            "example:callback/sample-break-command-count@1",
            CallbackContributionKind::MetricSample,
            true,
            "metric-sample(batch)->u64@1",
        ),
        (
            "example:callback/inspect-agent-gameplay@1",
            CallbackContributionKind::InspectSample,
            true,
            "inspect-sample(entity-batch)->fragment@1",
        ),
    ];
    let mut callback_map = BTreeMap::new();
    for (id, kind, batched, signature) in callback_specs {
        let callback = stable(id)?;
        let row = CallbackContribution {
            callback: callback.clone(),
            kind,
            signature_hash: canonical_json_hash(&signature)?,
            batched,
        };
        callback_map.insert(callback, row);
    }
    Ok(GeneratedContributions {
        setting,
        metric,
        info_item,
        inspect,
        static_callbacks: callback_map.clone(),
        dynamic_callbacks: callback_map,
    })
}

fn stable(value: &str) -> Result<StableId, FixtureError> {
    Ok(value.parse()?)
}
