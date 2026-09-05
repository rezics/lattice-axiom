//! Per-package registration manifests and closure-wide images.

use std::collections::{BTreeMap, BTreeSet};

use latticeaxiom_core::{
    CanonicalHash, CanonicalJsonError, PackageName, PackageVersion, SchemaId, SourceProvenance,
    StableId, canonical_json_bytes, canonical_json_hash,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

use crate::{
    CompositionParameterSpec, DebugVisualizerSpec, DiagnosticMetricSpec, InfoItemSpec,
    InspectFragmentProviderSpec, ObservabilityCatalog, SemanticCatalog, SemanticFragment,
    SettingSpec, SettingsCatalog,
};

/// Current registration-manifest schema version.
pub const REGISTRATION_MANIFEST_SCHEMA_VERSION: u32 = 1;

/// Kind of an exact stable registration.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum RegistrationKind {
    /// Dimension definition.
    Dimension,
    /// Block definition.
    Block,
    /// Item definition.
    Item,
    /// Fluid definition.
    Fluid,
    /// Biome definition.
    Biome,
    /// Component schema.
    Component,
    /// Message schema.
    Message,
    /// Scheduled system.
    System,
    /// Asset registration.
    Asset,
    /// Capability contract.
    Capability,
    /// Persistent schema.
    Schema,
    /// Render contract.
    Render,
}

/// One exact registration published by a package.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ExactRegistration {
    /// Full stable ID.
    pub id: StableId,
    /// Registration kind.
    pub kind: RegistrationKind,
    /// Package publishing the registration.
    pub declared_by: PackageName,
    /// Optional persistent or ABI schema.
    pub schema: Option<SchemaId>,
    /// Source location retained for diagnostics.
    pub provenance: SourceProvenance,
}

/// Additive manifest fragment emitted by Nickel or generated code.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RegistrationFragment {
    /// Exact registration rows.
    #[serde(default)]
    pub registrations: Vec<ExactRegistration>,
    /// Semantic registration rows.
    #[serde(default)]
    pub semantics: Vec<SemanticFragment>,
    /// Runtime setting rows.
    #[serde(default)]
    pub settings: Vec<SettingSpec>,
    /// Graph-affecting composition parameter rows.
    #[serde(default)]
    pub composition_parameters: Vec<CompositionParameterSpec>,
    /// Information item rows.
    #[serde(default)]
    pub info_items: Vec<InfoItemSpec>,
    /// Diagnostic metric rows.
    #[serde(default)]
    pub metrics: Vec<DiagnosticMetricSpec>,
    /// Target-inspection provider rows.
    #[serde(default)]
    pub inspect: Vec<InspectFragmentProviderSpec>,
    /// Debug visualizer rows.
    #[serde(default)]
    pub visualizers: Vec<DebugVisualizerSpec>,
}

/// Build producer metadata that does not alter registration semantics.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ManifestProducer {
    /// SDK or authoring tool name.
    pub tool: String,
    /// Exact SDK or tool version.
    pub version: PackageVersion,
    /// Hash of the source fragment consumed by the producer.
    pub input_hash: CanonicalHash,
}

/// Canonical registration manifest for one exact package realization.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RegistrationManifest {
    /// Manifest schema version.
    pub schema_version: u32,
    /// Logical package identity.
    pub package: PackageName,
    /// Exact package version.
    pub version: PackageVersion,
    /// Normalized registration rows.
    pub fragment: RegistrationFragment,
    /// Producer metadata excluded from the semantic manifest hash.
    pub producer: ManifestProducer,
    /// Canonical semantic hash shared by all realizations.
    pub semantic_hash: CanonicalHash,
}

impl RegistrationManifest {
    /// Recomputes the semantic manifest hash without producer or source provenance.
    ///
    /// Additive fragment arrays are canonicalized as unordered collections.
    ///
    /// # Errors
    ///
    /// Returns [`CanonicalJsonError`] if the normalized fragment cannot be
    /// represented as canonical JSON.
    pub fn recompute_semantic_hash(&self) -> Result<CanonicalHash, CanonicalJsonError> {
        let mut value = serde_json::to_value(&self.fragment)?;
        normalize_registration_fragment(&mut value)?;
        canonical_json_hash(&ManifestSemanticIdentity {
            schema_version: self.schema_version,
            package: &self.package,
            version: &self.version,
            normalized_fragment: value,
        })
    }

    /// Verifies the claimed semantic hash.
    ///
    /// # Errors
    ///
    /// Returns [`RegistrationHashError`] when encoding fails or the claimed
    /// hash differs from the normalized payload.
    pub fn verify_semantic_hash(&self) -> Result<(), RegistrationHashError> {
        let actual = self.recompute_semantic_hash()?;
        if actual == self.semantic_hash {
            Ok(())
        } else {
            Err(RegistrationHashError::Mismatch {
                expected: self.semantic_hash,
                actual,
            })
        }
    }
}

#[derive(Serialize)]
struct ManifestSemanticIdentity<'a> {
    schema_version: u32,
    package: &'a PackageName,
    version: &'a PackageVersion,
    normalized_fragment: Value,
}

fn normalize_registration_fragment(value: &mut Value) -> Result<(), CanonicalJsonError> {
    let Value::Object(fragment) = value else {
        return Ok(());
    };
    if let Some(Value::Array(registrations)) = fragment.get_mut("registrations") {
        for registration in &mut *registrations {
            if let Value::Object(fields) = registration {
                fields.remove("provenance");
            }
        }
        sort_canonical(registrations)?;
    }
    if let Some(Value::Array(semantics)) = fragment.get_mut("semantics") {
        for semantic in &mut *semantics {
            normalize_semantic_fragment(semantic)?;
        }
        sort_canonical(semantics)?;
    }
    if let Some(Value::Array(settings)) = fragment.get_mut("settings") {
        for setting in &mut *settings {
            normalize_setting(setting)?;
        }
        sort_canonical(settings)?;
    }
    for field in [
        "composition_parameters",
        "info_items",
        "metrics",
        "inspect",
        "visualizers",
    ] {
        if let Some(Value::Array(rows)) = fragment.get_mut(field) {
            sort_canonical(rows)?;
        }
    }
    Ok(())
}

fn normalize_semantic_fragment(value: &mut Value) -> Result<(), CanonicalJsonError> {
    let Value::Object(fragment) = value else {
        return Ok(());
    };
    let kind = fragment
        .get("kind")
        .and_then(Value::as_str)
        .map(str::to_owned);
    let Some(Value::Object(payload)) = fragment.get_mut("value") else {
        return Ok(());
    };
    match kind.as_deref() {
        Some("tag-contribution" | "map-contribution") => {
            payload.remove("provenance");
        }
        Some("state-property") => {
            if let Some(Value::Array(allowed_values)) = payload.get_mut("allowed_values") {
                sort_canonical(allowed_values)?;
            }
        }
        Some("role") => {
            if let Some(Value::Object(accepts)) = payload.get_mut("accepts")
                && let Some(expression) = accepts.get_mut("expression")
            {
                normalize_predicate_expression(expression)?;
            }
        }
        Some("bundle") => {
            if let Some(Value::Array(semantics)) = payload.get_mut("semantics") {
                for semantic in &mut *semantics {
                    normalize_semantic_fragment(semantic)?;
                }
                sort_canonical(semantics)?;
            }
            if let Some(Value::Array(role_offers)) = payload.get_mut("role_offers") {
                sort_canonical(role_offers)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn normalize_predicate_expression(value: &mut Value) -> Result<(), CanonicalJsonError> {
    let Value::Object(expression) = value else {
        return Ok(());
    };
    let operation = expression
        .get("op")
        .and_then(Value::as_str)
        .map(str::to_owned);
    match operation.as_deref() {
        Some("map-matches" | "state-matches") => {
            if let Some(condition) = expression.get_mut("condition") {
                normalize_typed_condition(condition)?;
            }
        }
        Some("all" | "any") => {
            if let Some(Value::Array(predicates)) = expression.get_mut("predicates") {
                for predicate in &mut *predicates {
                    normalize_predicate_expression(predicate)?;
                }
                sort_canonical(predicates)?;
            }
        }
        Some("not") => {
            if let Some(predicate) = expression.get_mut("predicate") {
                normalize_predicate_expression(predicate)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn normalize_typed_condition(value: &mut Value) -> Result<(), CanonicalJsonError> {
    let Value::Object(condition) = value else {
        return Ok(());
    };
    if condition.get("op").and_then(Value::as_str) == Some("in")
        && let Some(Value::Array(values)) = condition.get_mut("value")
    {
        sort_canonical(values)?;
    }
    Ok(())
}

fn normalize_setting(value: &mut Value) -> Result<(), CanonicalJsonError> {
    let Value::Object(setting) = value else {
        return Ok(());
    };
    for field in ["visibility", "enabled_when"] {
        if let Some(predicate) = setting.get_mut(field) {
            normalize_setting_predicate(predicate)?;
        }
    }
    Ok(())
}

fn normalize_setting_predicate(value: &mut Value) -> Result<(), CanonicalJsonError> {
    let Value::Object(predicate) = value else {
        return Ok(());
    };
    let operation = predicate
        .get("op")
        .and_then(Value::as_str)
        .map(str::to_owned);
    match operation.as_deref() {
        Some("all" | "any") => {
            if let Some(Value::Array(predicates)) = predicate.get_mut("predicates") {
                for child in &mut *predicates {
                    normalize_setting_predicate(child)?;
                }
                sort_canonical(predicates)?;
            }
        }
        Some("not") => {
            if let Some(child) = predicate.get_mut("predicate") {
                normalize_setting_predicate(child)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn sort_canonical(values: &mut Vec<Value>) -> Result<(), CanonicalJsonError> {
    let mut keyed = values
        .drain(..)
        .map(|value| canonical_json_bytes(&value).map(|key| (key, value)))
        .collect::<Result<Vec<_>, _>>()?;
    keyed.sort_unstable_by(|left, right| left.0.cmp(&right.0));
    values.extend(keyed.into_iter().map(|(_, value)| value));
    Ok(())
}

/// Numeric ID assigned for one engine instance.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct NumericRegistrationId(pub u32);

/// Closure-wide registration image compiled before package code activation.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RegistrationImage {
    /// Source locked-graph hash.
    pub graph_hash: CanonicalHash,
    /// Stable-to-numeric exact registration table.
    pub numeric_ids: BTreeMap<StableId, NumericRegistrationId>,
    /// Package provenance for each exact registration.
    pub owners: BTreeMap<StableId, PackageName>,
    /// Persistent schema owners.
    pub schema_owners: BTreeMap<SchemaId, PackageName>,
    /// Compiled semantic catalog.
    pub semantics: SemanticCatalog,
    /// Compiled runtime settings and composition parameters.
    pub settings: SettingsCatalog,
    /// Compiled information and diagnostics catalog.
    pub observability: ObservabilityCatalog,
    /// Stable scheduled systems in deterministic order.
    pub schedule: Vec<StableId>,
    /// Authoritative registration IDs contributing to world identity.
    pub authoritative: BTreeSet<StableId>,
    /// Canonical image hash.
    pub image_hash: CanonicalHash,
}

impl RegistrationImage {
    /// Recomputes the image hash while preserving semantically ordered arrays.
    ///
    /// # Errors
    ///
    /// Returns [`CanonicalJsonError`] if the payload cannot be encoded.
    pub fn recompute_image_hash(&self) -> Result<CanonicalHash, CanonicalJsonError> {
        let mut value = serde_json::to_value(self)?;
        if let Value::Object(fields) = &mut value {
            fields.remove("image_hash");
        }
        canonical_json_hash(&value)
    }

    /// Verifies the claimed closure-wide image hash.
    ///
    /// # Errors
    ///
    /// Returns [`RegistrationHashError`] when encoding fails or the claimed
    /// hash differs from the recomputed payload.
    pub fn verify_image_hash(&self) -> Result<(), RegistrationHashError> {
        let actual = self.recompute_image_hash()?;
        if actual == self.image_hash {
            Ok(())
        } else {
            Err(RegistrationHashError::Mismatch {
                expected: self.image_hash,
                actual,
            })
        }
    }
}

/// Registration manifest or image hash verification failure.
#[derive(Debug, Error)]
pub enum RegistrationHashError {
    /// Canonical JSON encoding failed.
    #[error(transparent)]
    Canonical(#[from] CanonicalJsonError),
    /// A claimed hash differs from the normalized payload.
    #[error("registration hash mismatch: expected {expected}, recomputed {actual}")]
    Mismatch {
        /// Claimed hash.
        expected: CanonicalHash,
        /// Recomputed hash.
        actual: CanonicalHash,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use latticeaxiom_core::{SourceSpan, canonical_json_hash};

    #[test]
    fn semantic_manifest_hash_ignores_fragment_order_and_provenance() {
        let first_rows = vec![
            registration("stone", "a.ncl"),
            registration("dirt", "b.ncl"),
        ];
        let second_rows = vec![
            registration("dirt", "moved/dirt.ncl"),
            registration("stone", "moved/stone.ncl"),
        ];
        let first = manifest(first_rows);
        let mut second = manifest(second_rows);
        second.producer.tool = "different-tool".to_owned();

        let expected = first
            .recompute_semantic_hash()
            .unwrap_or_else(|error| panic!("manifest hash failed: {error}"));
        assert_eq!(second.recompute_semantic_hash().ok(), Some(expected));
        assert!(first.verify_semantic_hash().is_err());

        let mut verified = first;
        verified.semantic_hash = expected;
        assert!(verified.verify_semantic_hash().is_ok());
    }

    #[test]
    fn semantic_manifest_hash_preserves_typed_payload_order_and_names() {
        let mut first = manifest(Vec::new());
        first.fragment.semantics.push(map_contribution(
            serde_json::json!({"path": [1, 2, 3], "provenance": "typed-value"}),
            "a.ncl",
        ));
        let mut reordered = first.clone();
        reordered.fragment.semantics.clear();
        reordered.fragment.semantics.push(map_contribution(
            serde_json::json!({"path": [3, 2, 1], "provenance": "typed-value"}),
            "moved/a.ncl",
        ));
        let mut renamed = first.clone();
        renamed.fragment.semantics.clear();
        renamed.fragment.semantics.push(map_contribution(
            serde_json::json!({"path": [1, 2, 3], "provenance": "different-value"}),
            "moved/a.ncl",
        ));

        let first_hash = first.recompute_semantic_hash().ok();
        assert_ne!(first_hash, reordered.recompute_semantic_hash().ok());
        assert_ne!(first_hash, renamed.recompute_semantic_hash().ok());
    }

    #[test]
    fn semantic_manifest_hash_normalizes_known_closed_sets() {
        let mut first = manifest(Vec::new());
        first.fragment.semantics.push(state_property(vec![
            serde_json::json!(1),
            serde_json::json!(2),
        ]));
        let mut reordered = manifest(Vec::new());
        reordered.fragment.semantics.push(state_property(vec![
            serde_json::json!(2),
            serde_json::json!(1),
        ]));

        assert_eq!(
            first.recompute_semantic_hash().ok(),
            reordered.recompute_semantic_hash().ok()
        );
    }

    #[test]
    fn semantic_manifest_hash_includes_package_and_contract_identity() {
        let first = manifest(Vec::new());
        let mut different_package = first.clone();
        different_package.package = package_name("@terrenia/worldgen");
        let mut different_version = first.clone();
        different_version.version = version("0.2.0");
        let mut different_schema = first.clone();
        different_schema.schema_version += 1;

        let first_hash = first.recompute_semantic_hash().ok();
        assert_ne!(first_hash, different_package.recompute_semantic_hash().ok());
        assert_ne!(first_hash, different_version.recompute_semantic_hash().ok());
        assert_ne!(first_hash, different_schema.recompute_semantic_hash().ok());
    }

    fn manifest(registrations: Vec<ExactRegistration>) -> RegistrationManifest {
        RegistrationManifest {
            schema_version: REGISTRATION_MANIFEST_SCHEMA_VERSION,
            package: package_name("@terrenia/blocks"),
            version: version("0.1.0"),
            fragment: RegistrationFragment {
                registrations,
                ..RegistrationFragment::default()
            },
            producer: ManifestProducer {
                tool: "fixture".to_owned(),
                version: version("0.1.0"),
                input_hash: canonical_json_hash(&"input")
                    .unwrap_or_else(|error| panic!("fixture hash failed: {error}")),
            },
            semantic_hash: CanonicalHash::digest(b"unverified"),
        }
    }

    fn registration(name: &str, path: &str) -> ExactRegistration {
        ExactRegistration {
            id: stable_id(&format!("terrenia:block/{name}")),
            kind: RegistrationKind::Block,
            declared_by: package_name("@terrenia/blocks"),
            schema: None,
            provenance: SourceProvenance::new(
                "latticeaxiom:source/terrenia-blocks"
                    .parse()
                    .unwrap_or_else(|error| panic!("fixture source ID is invalid: {error}")),
                path,
                CanonicalHash::digest(path.as_bytes()),
                Some(
                    SourceSpan::new(0, 1)
                        .unwrap_or_else(|error| panic!("fixture span is invalid: {error}")),
                ),
                Vec::new(),
            )
            .unwrap_or_else(|error| panic!("fixture provenance is invalid: {error}")),
        }
    }

    fn map_contribution(value: Value, path: &str) -> SemanticFragment {
        SemanticFragment::MapContribution(crate::SemanticMapContribution {
            map: crate::TypedSemanticId {
                id: stable_id("terrenia:map/mining-path"),
                target_kind: crate::TargetKind::Block,
            },
            target: stable_id("terrenia:block/stone"),
            value,
            declared_by: package_name("@terrenia/blocks"),
            provenance: SourceProvenance::new(
                "latticeaxiom:source/terrenia-blocks"
                    .parse()
                    .unwrap_or_else(|error| panic!("fixture source ID is invalid: {error}")),
                path,
                CanonicalHash::digest(path.as_bytes()),
                None,
                Vec::new(),
            )
            .unwrap_or_else(|error| panic!("fixture provenance is invalid: {error}")),
        })
    }

    fn state_property(allowed_values: Vec<Value>) -> SemanticFragment {
        SemanticFragment::StateProperty(crate::StatePropertyDefinition {
            id: stable_id("terrenia:state/moisture"),
            target_kind: crate::TargetKind::Block,
            value_schema: "terrenia:schema/moisture@1"
                .parse()
                .unwrap_or_else(|error| panic!("fixture schema ID is invalid: {error}")),
            allowed_values,
        })
    }

    fn stable_id(value: &str) -> StableId {
        value
            .parse()
            .unwrap_or_else(|error| panic!("fixture stable ID `{value}` is invalid: {error}"))
    }

    fn package_name(value: &str) -> PackageName {
        value
            .parse()
            .unwrap_or_else(|error| panic!("fixture package `{value}` is invalid: {error}"))
    }

    fn version(value: &str) -> PackageVersion {
        value
            .parse()
            .unwrap_or_else(|error| panic!("fixture version `{value}` is invalid: {error}"))
    }
}
