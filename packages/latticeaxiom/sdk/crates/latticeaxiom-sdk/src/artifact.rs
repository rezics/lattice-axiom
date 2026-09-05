//! Deterministic package artifacts derived from sealed registration IR.

use std::collections::{BTreeMap, BTreeSet};

use latticeaxiom_compose::{
    ExactRegistration, ManifestProducer, REGISTRATION_MANIFEST_SCHEMA_VERSION,
    RegistrationFragment, RegistrationKind, RegistrationManifest,
};
use latticeaxiom_core::{
    CanonicalHash, PackageName, PackageVersion, SchemaId, SourceProvenance, StableId,
    canonical_json_hash,
};
use latticeaxiom_registration::{
    CallbackDeclaration, PackageRegistrationInput, SchemaDeclaration, SystemDeclaration,
};
use serde::{Deserialize, Serialize};

use crate::{
    ComponentAccess, RegistrationIr, RegistrationIrError, SystemPortability,
    ir::ProvenanceCatalog,
    system::{QueryFilter, SystemParameter},
};

/// Schema version of SDK-generated package artifacts.
pub const GENERATED_ARTIFACT_SCHEMA_VERSION: u32 = 1;

/// Stable SDK producer identity supplied by build tooling.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProducerInput {
    /// Versioned SDK stable ID.
    pub sdk_id: StableId,
    /// Exact SDK package version.
    pub sdk_version: PackageVersion,
    /// Versioned generator contract.
    pub generator_contract: StableId,
    /// Canonical hash of the source fragment consumed by generation.
    pub input_fragment_hash: CanonicalHash,
}

impl ProducerInput {
    /// Creates the producer input for this SDK release.
    ///
    /// # Errors
    ///
    /// Returns [`RegistrationIrError`] only if crate constants stop satisfying
    /// the canonical identifier or `SemVer` grammar.
    pub fn current(input_fragment_hash: CanonicalHash) -> Result<Self, RegistrationIrError> {
        Ok(Self {
            sdk_id: parse_identifier("sdk-id", "latticeaxiom:sdk/registration-authoring@1")?,
            sdk_version: parse_identifier("sdk-version", env!("CARGO_PKG_VERSION"))?,
            generator_contract: parse_identifier(
                "generator-contract",
                "latticeaxiom:generator/registration-ir@1",
            )?,
            input_fragment_hash,
        })
    }
}

/// Canonical receipt separating generator identity from registration semantics.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProducerReceipt {
    /// Versioned SDK stable ID.
    pub sdk_id: StableId,
    /// Exact SDK package version.
    pub sdk_version: PackageVersion,
    /// Versioned generator contract.
    pub generator_contract: StableId,
    /// Canonical input fragment hash.
    pub input_fragment_hash: CanonicalHash,
    /// Hash of the complete generated registration fragment.
    pub generated_fragment_hash: CanonicalHash,
    /// Content address of every preceding producer field.
    pub receipt_hash: CanonicalHash,
}

impl ProducerReceipt {
    /// Recomputes the producer receipt hash.
    ///
    /// # Errors
    ///
    /// Returns [`RegistrationIrError`] if canonical encoding fails.
    pub fn recompute_hash(&self) -> Result<CanonicalHash, RegistrationIrError> {
        Ok(canonical_json_hash(&ProducerReceiptIdentity::from(self))?)
    }
}

/// Source and generator provenance kept outside registration semantics.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProvenanceArtifact {
    /// Artifact schema version.
    pub schema_version: u32,
    /// Logical package owning every generated row.
    pub package: PackageName,
    /// Complete source provenance keyed by registration or callback ID.
    pub rows: BTreeMap<StableId, SourceProvenance>,
    /// Canonical generator receipt.
    pub producer: ProducerReceipt,
    /// Content address of all preceding provenance fields.
    pub provenance_hash: CanonicalHash,
}

impl ProvenanceArtifact {
    /// Recomputes the provenance artifact hash.
    ///
    /// # Errors
    ///
    /// Returns [`RegistrationIrError`] if canonical encoding fails.
    pub fn recompute_hash(&self) -> Result<CanonicalHash, RegistrationIrError> {
        Ok(canonical_json_hash(&ProvenanceIdentity::from(self))?)
    }
}

/// One callback-map row generated from the same system signature.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CallbackMapEntry {
    /// Package owning the callback.
    pub owner: PackageName,
    /// System consuming the callback.
    pub system: StableId,
    /// Canonical system signature hash.
    pub signature_hash: CanonicalHash,
    /// Whether a portable batch shim is allowed.
    pub portability: SystemPortability,
}

/// Package-local callback map verified before code activation.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CallbackMapArtifact {
    /// Artifact schema version.
    pub schema_version: u32,
    /// Logical package owning all callbacks.
    pub package: PackageName,
    /// Stable callback bindings in canonical key order.
    pub callbacks: BTreeMap<StableId, CallbackMapEntry>,
    /// Content address of all preceding callback-map fields.
    pub callback_map_hash: CanonicalHash,
}

impl CallbackMapArtifact {
    /// Recomputes the callback-map hash.
    ///
    /// # Errors
    ///
    /// Returns [`RegistrationIrError`] if canonical encoding fails.
    pub fn recompute_hash(&self) -> Result<CanonicalHash, RegistrationIrError> {
        Ok(canonical_json_hash(&CallbackMapIdentity::from(self))?)
    }
}

/// Static adapter generation plan; the host maps this directly to Bevy.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct StaticAdapterPlan {
    /// Stable system registration ID.
    pub system: StableId,
    /// Versioned callback key used for equivalence receipts.
    pub callback: StableId,
    /// Canonical system signature hash expected after Bevy access reflection.
    pub signature_hash: CanonicalHash,
    /// Source-level row-kernel symbol; never a function address.
    pub row_kernel_symbol: String,
}

/// One portable archetype column required by a dynamic batch shim.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DynamicBatchColumn {
    /// Source parameter ordinal.
    pub ordinal: u32,
    /// Stable component access contract.
    pub access: ComponentAccess,
}

/// Data-only plan for generating one portable native batch shim.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DynamicBatchShimPlan {
    /// Stable system registration ID.
    pub system: StableId,
    /// Versioned callback key.
    pub callback: StableId,
    /// Canonical system signature hash checked by the loader.
    pub signature_hash: CanonicalHash,
    /// Ordered component columns passed once per archetype batch.
    pub columns: Vec<DynamicBatchColumn>,
    /// Canonically ordered row filters enforced by the host query.
    pub filters: Vec<QueryFilter>,
    /// Whether the validated callback receives an SDK command sink.
    pub command_sink: bool,
}

/// Complete deterministic output of SDK registration generation.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GeneratedRegistrationArtifacts {
    /// Artifact schema version.
    pub schema_version: u32,
    /// Canonical manifest plus schema, system, and callback declarations.
    pub registration: PackageRegistrationInput,
    /// Source and producer provenance kept outside semantic identity.
    pub provenance: ProvenanceArtifact,
    /// Package-local callback map.
    pub callback_map: CallbackMapArtifact,
    /// Full canonical signatures used for static Bevy access reflection.
    pub system_signatures: BTreeMap<StableId, crate::SystemSignature>,
    /// Static adapter plans in system `StableId` order.
    pub static_adapters: Vec<StaticAdapterPlan>,
    /// Portable batch shims in callback `StableId` order.
    pub dynamic_batch_shims: Vec<DynamicBatchShimPlan>,
    /// Rust API fingerprints used by shared-schema build planning.
    pub component_rust_api: BTreeMap<StableId, CanonicalHash>,
    /// Static Rust signatures used only for artifact identity.
    pub system_rust_api: BTreeMap<StableId, CanonicalHash>,
    /// Hash of generated code-facing plans and fingerprints.
    pub artifact_hash: CanonicalHash,
}

impl GeneratedRegistrationArtifacts {
    /// Verifies every self-contained artifact hash and callback cross-binding.
    ///
    /// This method is pure and executes no package callback.
    ///
    /// # Errors
    ///
    /// Returns [`RegistrationIrError`] for unsupported schemas, hash mismatch,
    /// or a callback row that disagrees with its system declaration.
    pub fn verify(&self) -> Result<(), RegistrationIrError> {
        for found in [
            self.schema_version,
            self.provenance.schema_version,
            self.callback_map.schema_version,
            self.registration.manifest.schema_version,
        ] {
            if found != GENERATED_ARTIFACT_SCHEMA_VERSION {
                return Err(RegistrationIrError::UnsupportedArtifactSchema { found });
            }
        }
        validate_contract_major("sdk-id", &self.provenance.producer.sdk_id)?;
        validate_contract_major(
            "generator-contract",
            &self.provenance.producer.generator_contract,
        )?;
        self.registration
            .manifest
            .verify_semantic_hash()
            .map_err(|error| RegistrationIrError::ManifestHashMismatch {
                reason: error.to_string(),
            })?;
        verify_hash(
            "producer-receipt",
            self.provenance.producer.receipt_hash,
            self.provenance.producer.recompute_hash()?,
        )?;
        verify_hash(
            "provenance",
            self.provenance.provenance_hash,
            self.provenance.recompute_hash()?,
        )?;
        verify_hash(
            "callback-map",
            self.callback_map.callback_map_hash,
            self.callback_map.recompute_hash()?,
        )?;
        verify_hash(
            "artifact",
            self.artifact_hash,
            self.recompute_artifact_hash()?,
        )?;
        for (callback, binding) in &self.callback_map.callbacks {
            let Some(system) = self.registration.systems.get(&binding.system) else {
                return Err(RegistrationIrError::CallbackSignatureMismatch {
                    callback: callback.clone(),
                    system: Box::new(binding.system.clone()),
                });
            };
            let Some(signature) = self.system_signatures.get(&binding.system) else {
                return Err(RegistrationIrError::CallbackSignatureMismatch {
                    callback: callback.clone(),
                    system: Box::new(binding.system.clone()),
                });
            };
            if system.callback != *callback
                || system.signature_hash != binding.signature_hash
                || signature.callback != *callback
                || signature.signature_hash != binding.signature_hash
            {
                return Err(RegistrationIrError::CallbackSignatureMismatch {
                    callback: callback.clone(),
                    system: Box::new(binding.system.clone()),
                });
            }
        }
        Ok(())
    }

    /// Recomputes the generated code artifact hash.
    ///
    /// # Errors
    ///
    /// Returns [`RegistrationIrError`] if canonical encoding fails.
    pub fn recompute_artifact_hash(&self) -> Result<CanonicalHash, RegistrationIrError> {
        Ok(canonical_json_hash(&CodeArtifactIdentity::from(self))?)
    }
}

impl RegistrationIr {
    /// Generates canonical package artifacts without executing package code.
    ///
    /// # Errors
    ///
    /// Returns [`RegistrationIrError`] when producer contracts are unversioned,
    /// provenance is incomplete or contains unknown rows, or canonical hashing
    /// fails.
    #[allow(
        clippy::too_many_lines,
        reason = "cold single-pass generation keeps all artifact rows sourced from one canonical IR iteration"
    )]
    pub fn generate(
        &self,
        producer: &ProducerInput,
        provenance: &ProvenanceCatalog,
    ) -> Result<GeneratedRegistrationArtifacts, RegistrationIrError> {
        validate_producer(producer)?;
        validate_provenance(self, provenance)?;
        let mut rows = BTreeMap::new();
        let mut exact = Vec::with_capacity(self.components.len() + self.systems.len());
        let mut schemas = BTreeMap::new();
        let mut component_rust_api = BTreeMap::new();
        for component in self.components.values() {
            let source = required_provenance(provenance, &component.id)?.clone();
            rows.insert(component.id.clone(), source.clone());
            exact.push(ExactRegistration {
                id: component.id.clone(),
                kind: RegistrationKind::Component,
                declared_by: self.package.clone(),
                schema: component.schema.clone(),
                provenance: source,
            });
            component_rust_api.insert(component.id.clone(), component.rust_api_fingerprint);
            if let Some(schema) = &component.schema {
                schemas
                    .entry(schema.clone())
                    .or_insert_with(|| SchemaDeclaration {
                        id: schema.clone(),
                        declared_by: self.package.clone(),
                        value_type: None,
                    });
            }
        }

        let mut systems = BTreeMap::new();
        let mut system_signatures = BTreeMap::new();
        let mut callbacks = BTreeMap::new();
        let mut callback_rows = BTreeMap::new();
        let mut static_adapters = Vec::with_capacity(self.systems.len());
        let mut dynamic_batch_shims = Vec::new();
        let mut system_rust_api = BTreeMap::new();
        for system in self.systems.values() {
            let signature = &system.signature;
            let source = required_provenance(provenance, &signature.system)?.clone();
            rows.insert(signature.system.clone(), source.clone());
            rows.insert(signature.callback.clone(), source.clone());
            exact.push(ExactRegistration {
                id: signature.system.clone(),
                kind: RegistrationKind::System,
                declared_by: self.package.clone(),
                schema: None,
                provenance: source,
            });
            system_signatures.insert(signature.system.clone(), signature.clone());
            systems.insert(
                signature.system.clone(),
                SystemDeclaration {
                    id: signature.system.clone(),
                    declared_by: self.package.clone(),
                    stage: signature.stage.clone(),
                    callback: signature.callback.clone(),
                    signature_hash: signature.signature_hash,
                    after: signature.after.clone(),
                    before: signature.before.clone(),
                },
            );
            callbacks.insert(
                signature.callback.clone(),
                CallbackDeclaration {
                    id: signature.callback.clone(),
                    declared_by: self.package.clone(),
                    signature_hash: signature.signature_hash,
                },
            );
            callback_rows.insert(
                signature.callback.clone(),
                CallbackMapEntry {
                    owner: self.package.clone(),
                    system: signature.system.clone(),
                    signature_hash: signature.signature_hash,
                    portability: signature.portability.clone(),
                },
            );
            static_adapters.push(StaticAdapterPlan {
                system: signature.system.clone(),
                callback: signature.callback.clone(),
                signature_hash: signature.signature_hash,
                row_kernel_symbol: system.row_kernel_symbol.clone(),
            });
            if matches!(signature.portability, SystemPortability::Dual) {
                dynamic_batch_shims.push(dynamic_batch_plan(signature));
            }
            system_rust_api.insert(signature.system.clone(), system.rust_signature_fingerprint);
        }
        dynamic_batch_shims.sort_by(|left, right| left.callback.cmp(&right.callback));

        let fragment = RegistrationFragment {
            registrations: exact,
            ..RegistrationFragment::default()
        };
        let generated_fragment_hash = canonical_json_hash(&GeneratedFragmentIdentity {
            fragment: &fragment,
            schemas: &schemas,
            systems: &systems,
            callbacks: &callbacks,
        })?;
        let producer_receipt = make_producer_receipt(producer, generated_fragment_hash)?;
        let mut manifest = RegistrationManifest {
            schema_version: REGISTRATION_MANIFEST_SCHEMA_VERSION,
            package: self.package.clone(),
            version: self.version.clone(),
            fragment,
            producer: ManifestProducer {
                tool: producer.sdk_id.to_string(),
                version: producer.sdk_version.clone(),
                input_hash: producer.input_fragment_hash,
            },
            semantic_hash: CanonicalHash::digest([]),
        };
        manifest.semantic_hash = manifest.recompute_semantic_hash()?;
        let registration = PackageRegistrationInput {
            manifest,
            schemas,
            systems,
            callbacks,
            provided_capabilities: BTreeSet::new(),
            semantic_grants: BTreeSet::new(),
        };
        let mut provenance_artifact = ProvenanceArtifact {
            schema_version: GENERATED_ARTIFACT_SCHEMA_VERSION,
            package: self.package.clone(),
            rows,
            producer: producer_receipt,
            provenance_hash: CanonicalHash::digest([]),
        };
        provenance_artifact.provenance_hash = provenance_artifact.recompute_hash()?;
        let mut callback_map = CallbackMapArtifact {
            schema_version: GENERATED_ARTIFACT_SCHEMA_VERSION,
            package: self.package.clone(),
            callbacks: callback_rows,
            callback_map_hash: CanonicalHash::digest([]),
        };
        callback_map.callback_map_hash = callback_map.recompute_hash()?;
        let mut artifacts = GeneratedRegistrationArtifacts {
            schema_version: GENERATED_ARTIFACT_SCHEMA_VERSION,
            registration,
            provenance: provenance_artifact,
            callback_map,
            system_signatures,
            static_adapters,
            dynamic_batch_shims,
            component_rust_api,
            system_rust_api,
            artifact_hash: CanonicalHash::digest([]),
        };
        artifacts.artifact_hash = artifacts.recompute_artifact_hash()?;
        Ok(artifacts)
    }
}

fn dynamic_batch_plan(signature: &crate::SystemSignature) -> DynamicBatchShimPlan {
    let mut columns = Vec::new();
    let mut filters = Vec::new();
    let mut command_sink = false;
    for parameter in &signature.parameters {
        match parameter {
            SystemParameter::Component { ordinal, value } => columns.push(DynamicBatchColumn {
                ordinal: *ordinal,
                access: value.clone(),
            }),
            SystemParameter::Filter { value, .. } => filters.push(value.clone()),
            SystemParameter::CommandSink { .. } => command_sink = true,
            SystemParameter::RowEntity { .. }
            | SystemParameter::FixedTick { .. }
            | SystemParameter::StaticOnly { .. } => {}
        }
    }
    filters.sort();
    DynamicBatchShimPlan {
        system: signature.system.clone(),
        callback: signature.callback.clone(),
        signature_hash: signature.signature_hash,
        columns,
        filters,
        command_sink,
    }
}

fn make_producer_receipt(
    producer: &ProducerInput,
    generated_fragment_hash: CanonicalHash,
) -> Result<ProducerReceipt, RegistrationIrError> {
    let mut receipt = ProducerReceipt {
        sdk_id: producer.sdk_id.clone(),
        sdk_version: producer.sdk_version.clone(),
        generator_contract: producer.generator_contract.clone(),
        input_fragment_hash: producer.input_fragment_hash,
        generated_fragment_hash,
        receipt_hash: CanonicalHash::digest([]),
    };
    receipt.receipt_hash = receipt.recompute_hash()?;
    Ok(receipt)
}

fn validate_producer(producer: &ProducerInput) -> Result<(), RegistrationIrError> {
    validate_contract_major("sdk-id", &producer.sdk_id)?;
    validate_contract_major("generator-contract", &producer.generator_contract)
}

fn validate_contract_major(field: &'static str, id: &StableId) -> Result<(), RegistrationIrError> {
    if has_positive_major(id) {
        Ok(())
    } else {
        Err(RegistrationIrError::InvalidProducerContract {
            field,
            id: id.clone(),
        })
    }
}

fn validate_provenance(
    ir: &RegistrationIr,
    provenance: &ProvenanceCatalog,
) -> Result<(), RegistrationIrError> {
    let expected = ir
        .components
        .keys()
        .chain(ir.systems.keys())
        .cloned()
        .collect::<BTreeSet<_>>();
    for id in &expected {
        if provenance.get(id).is_none() {
            return Err(RegistrationIrError::MissingProvenance { id: id.clone() });
        }
    }
    if let Some(id) = provenance.rows().keys().find(|id| !expected.contains(*id)) {
        return Err(RegistrationIrError::UnknownProvenance { id: id.clone() });
    }
    Ok(())
}

fn required_provenance<'a>(
    provenance: &'a ProvenanceCatalog,
    id: &StableId,
) -> Result<&'a SourceProvenance, RegistrationIrError> {
    provenance
        .get(id)
        .ok_or_else(|| RegistrationIrError::MissingProvenance { id: id.clone() })
}

fn has_positive_major(id: &StableId) -> bool {
    id.to_string()
        .rsplit_once('@')
        .and_then(|(_, major)| major.parse::<u64>().ok())
        .is_some_and(|major| major > 0)
}

fn verify_hash(
    artifact: &'static str,
    expected: CanonicalHash,
    actual: CanonicalHash,
) -> Result<(), RegistrationIrError> {
    if expected == actual {
        Ok(())
    } else {
        Err(RegistrationIrError::ArtifactHashMismatch {
            artifact,
            expected,
            actual,
        })
    }
}

fn parse_identifier<T>(field: &'static str, value: &str) -> Result<T, RegistrationIrError>
where
    T: std::str::FromStr,
    T::Err: std::fmt::Display,
{
    value
        .parse()
        .map_err(|error: T::Err| RegistrationIrError::InvalidIdentifier {
            field,
            value: value.to_owned(),
            reason: error.to_string(),
        })
}

#[derive(Serialize)]
struct GeneratedFragmentIdentity<'a> {
    fragment: &'a RegistrationFragment,
    schemas: &'a BTreeMap<SchemaId, SchemaDeclaration>,
    systems: &'a BTreeMap<StableId, SystemDeclaration>,
    callbacks: &'a BTreeMap<StableId, CallbackDeclaration>,
}

#[derive(Serialize)]
struct ProducerReceiptIdentity<'a> {
    sdk_id: &'a StableId,
    sdk_version: &'a PackageVersion,
    generator_contract: &'a StableId,
    input_fragment_hash: CanonicalHash,
    generated_fragment_hash: CanonicalHash,
}

impl<'a> From<&'a ProducerReceipt> for ProducerReceiptIdentity<'a> {
    fn from(receipt: &'a ProducerReceipt) -> Self {
        Self {
            sdk_id: &receipt.sdk_id,
            sdk_version: &receipt.sdk_version,
            generator_contract: &receipt.generator_contract,
            input_fragment_hash: receipt.input_fragment_hash,
            generated_fragment_hash: receipt.generated_fragment_hash,
        }
    }
}

#[derive(Serialize)]
struct ProvenanceIdentity<'a> {
    schema_version: u32,
    package: &'a PackageName,
    rows: &'a BTreeMap<StableId, SourceProvenance>,
    producer: &'a ProducerReceipt,
}

impl<'a> From<&'a ProvenanceArtifact> for ProvenanceIdentity<'a> {
    fn from(artifact: &'a ProvenanceArtifact) -> Self {
        Self {
            schema_version: artifact.schema_version,
            package: &artifact.package,
            rows: &artifact.rows,
            producer: &artifact.producer,
        }
    }
}

#[derive(Serialize)]
struct CallbackMapIdentity<'a> {
    schema_version: u32,
    package: &'a PackageName,
    callbacks: &'a BTreeMap<StableId, CallbackMapEntry>,
}

impl<'a> From<&'a CallbackMapArtifact> for CallbackMapIdentity<'a> {
    fn from(artifact: &'a CallbackMapArtifact) -> Self {
        Self {
            schema_version: artifact.schema_version,
            package: &artifact.package,
            callbacks: &artifact.callbacks,
        }
    }
}

#[derive(Serialize)]
struct CodeArtifactIdentity<'a> {
    schema_version: u32,
    package: &'a PackageName,
    callback_map_hash: CanonicalHash,
    system_signatures: &'a BTreeMap<StableId, crate::SystemSignature>,
    static_adapters: &'a [StaticAdapterPlan],
    dynamic_batch_shims: &'a [DynamicBatchShimPlan],
    component_rust_api: &'a BTreeMap<StableId, CanonicalHash>,
    system_rust_api: &'a BTreeMap<StableId, CanonicalHash>,
}

impl<'a> From<&'a GeneratedRegistrationArtifacts> for CodeArtifactIdentity<'a> {
    fn from(artifacts: &'a GeneratedRegistrationArtifacts) -> Self {
        Self {
            schema_version: artifacts.schema_version,
            package: &artifacts.registration.manifest.package,
            callback_map_hash: artifacts.callback_map.callback_map_hash,
            system_signatures: &artifacts.system_signatures,
            static_adapters: &artifacts.static_adapters,
            dynamic_batch_shims: &artifacts.dynamic_batch_shims,
            component_rust_api: &artifacts.component_rust_api,
            system_rust_api: &artifacts.system_rust_api,
        }
    }
}
