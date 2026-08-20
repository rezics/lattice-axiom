//! Authoring output accepted from Nickel profiles and package manifests.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::str::FromStr;

use latticeaxiom_core::{
    CanonicalHash, CanonicalJsonError, CanonicalLogicalPath, NamespaceGrantPattern, PackageName,
    PackageVersion, PackageVersionReq, SourceId, SourceProvenance, StableId, TargetTriple,
    canonical_json_hash,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;
use unicode_normalization::UnicodeNormalization;

/// Current schema version for [`CompositionSpec`].
pub const COMPOSITION_SCHEMA_VERSION: u32 = 3;

/// Current model version for [`PackageSpec`].
pub const PACKAGE_MODEL_VERSION: u32 = 3;

/// Current model version for [`GameProfileSpec`].
pub const GAME_PROFILE_MODEL_VERSION: u32 = 3;

/// Major version of the `latticeaxiom.lib` Nickel contract surface.
pub const NICKEL_LIBRARY_CONTRACT_MAJOR: u32 = 3;

/// Major version of the executable R0 authoring corpus.
pub const R0_AUTHORING_CORPUS_MAJOR: u32 = 3;

/// Exact Nickel policy implemented by the R0 evaluator contract.
pub const R0_NICKEL_EVALUATION_POLICY: &str = "latticeaxiom:nickel-evaluation-policy/r0@1";

/// Package-local stable identity of one realization candidate.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct RealizationId(String);

impl RealizationId {
    /// Parses a lowercase package-local realization identifier.
    ///
    /// # Errors
    ///
    /// Returns [`CompositionError::InvalidRealizationId`] when the value is
    /// empty, starts or ends with punctuation, or contains unsupported bytes.
    pub fn new(value: impl Into<String>) -> Result<Self, CompositionError> {
        let value = value.into();
        let valid = value.len() <= 64
            && value
                .bytes()
                .next()
                .is_some_and(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
            && value
                .bytes()
                .next_back()
                .is_some_and(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
            && value.bytes().all(|byte| {
                byte.is_ascii_lowercase()
                    || byte.is_ascii_digit()
                    || matches!(byte, b'-' | b'_' | b'.')
            });
        if valid {
            Ok(Self(value))
        } else {
            Err(CompositionError::InvalidRealizationId { value })
        }
    }

    /// Returns the package-local identifier text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for RealizationId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::new(value).map_err(serde::de::Error::custom)
    }
}

impl fmt::Display for RealizationId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl FromStr for RealizationId {
    type Err = CompositionError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::new(value)
    }
}

/// Execution domains to which a package contributes.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum PackageDomain {
    /// State that must agree across every authoritative participant.
    Authoritative,
    /// Client-only presentation or interaction surface.
    Client,
    /// Dedicated-server-only behavior.
    Server,
    /// Developer tool behavior excluded from normal play closures.
    Tool,
}

/// A profile projection used to select package domains.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ProfileKind {
    /// Package-driven world catalog and recovery shell.
    ClientShell,
    /// Interactive world client with authoritative and presentation rows.
    ClientWorld,
    /// Dedicated server with authoritative and server rows.
    DedicatedServer,
    /// Deterministic GPU-free conformance-test closure.
    HeadlessTest,
    /// Developer tooling profile.
    Tool,
}

/// Supported realization families.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum RealizationKind {
    /// Pure data with no executable package code.
    Data,
    /// Rust code compiled directly into the host.
    NativeStatic,
    /// Trusted native code using the portable Lattice ABI.
    PortableNative,
    /// Native code tied to an exact engine build identity.
    EngineCoupledNative,
}

/// A profile's realization selection intent.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(
    deny_unknown_fields,
    rename_all = "kebab-case",
    tag = "mode",
    content = "kind"
)]
pub enum RealizationPreference {
    /// Select according to the profile's ordered policy.
    Auto,
    /// Require one realization family.
    Exact(RealizationKind),
}

/// Cardinality of a versioned capability provider set.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum CapabilityCardinality {
    /// Exactly one provider is required.
    ExactlyOne,
    /// Any number of providers may be selected.
    ZeroOrMore,
    /// At least one provider is required.
    OneOrMore,
}

/// A versioned capability required by a package or profile.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CapabilityRequirement {
    /// Capability stable ID.
    pub capability: latticeaxiom_core::CapabilityId,
    /// Compatible interface versions.
    pub version: PackageVersionReq,
    /// Optional explicit provider package.
    pub provider: Option<PackageName>,
    /// Provider cardinality required by the consumer.
    pub cardinality: CapabilityCardinality,
    /// Domains in which the requirement is active.
    pub domains: BTreeSet<PackageDomain>,
}

/// A versioned capability offered by a package.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CapabilityProvision {
    /// Capability stable ID.
    pub capability: latticeaxiom_core::CapabilityId,
    /// Exact provided interface version.
    pub version: PackageVersion,
    /// Provider cardinality contract.
    pub cardinality: CapabilityCardinality,
    /// Domains in which the provider exists.
    pub domains: BTreeSet<PackageDomain>,
}

/// One root-package request in a profile.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PackageRequest {
    /// Compatible package versions.
    pub version: PackageVersionReq,
    /// Features enabled on the package.
    #[serde(default)]
    pub features: BTreeSet<String>,
    /// Requested realization.
    pub realization: RealizationPreference,
}

/// A candidate local package source.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SourceCandidate {
    /// Stable source-universe identity independent of its local path.
    pub source_id: SourceId,
    /// Logical package contained by this source root.
    pub package: PackageName,
    /// Exact source version.
    pub version: PackageVersion,
    /// NFC, slash-separated, root-relative path used for acquisition and diagnostics.
    pub path: String,
    /// SHA-256 identity of the normalized source content.
    pub content_hash: CanonicalHash,
    /// Explicit resolver priority. Discovery order never supplies priority.
    pub priority: i32,
    /// Source declaration provenance.
    pub provenance: SourceProvenance,
}

/// One ordered profile overlay with separate semantic and source receipts.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct OverlaySpec {
    /// Hash of the normalized evaluated overlay intent.
    pub semantic_hash: CanonicalHash,
    /// Source provenance excluded from semantic compatibility.
    pub provenance: SourceProvenance,
}

/// Policy inputs that affect graph construction.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CompositionPolicy {
    /// Target triple used for realization planning.
    pub target: TargetTriple,
    /// Ordered automatic realization preference.
    pub realization_order: Vec<RealizationKind>,
    /// Registration namespace grants keyed by the root package receiving authority.
    pub namespace_grants: BTreeMap<PackageName, BTreeSet<NamespaceGrantPattern>>,
    /// Highest package trust accepted by this composition.
    pub maximum_trust: TrustClass,
    /// Whether trusted policy overlays may force a conflicting value.
    pub allow_force_override: bool,
    /// Whether read-only recovery actions are allowed.
    pub allow_recovery: bool,
    /// Evaluation policy whose exact effective limits are recorded in the lock.
    pub evaluation_policy: StableId,
    /// Effective evaluator resource limits used for this composition.
    pub evaluation_limits: NickelEvaluationLimits,
}

/// Effective resource limits for one deterministic Nickel evaluation.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct NickelEvaluationLimits {
    /// Maximum wall-clock evaluation time in milliseconds.
    pub wall_clock_ms: u64,
    /// Maximum evaluator memory in bytes.
    pub memory_bytes: u64,
    /// Maximum expression-recursion depth.
    pub recursion_depth: u32,
    /// Maximum nested import depth.
    pub import_depth: u32,
    /// Maximum number of imported source files.
    pub imported_files: u32,
    /// Maximum aggregate source bytes.
    pub source_bytes: u64,
    /// Maximum evaluated output bytes.
    pub output_bytes: u64,
    /// Maximum number of retained diagnostics.
    pub retained_diagnostics: u32,
}

impl NickelEvaluationLimits {
    /// Validates that every effective limit is nonzero.
    ///
    /// # Errors
    ///
    /// Returns [`CompositionError::ZeroEvaluationLimit`] for a disabled limit.
    pub fn validate(&self) -> Result<(), CompositionError> {
        let limits = [
            ("wall_clock_ms", self.wall_clock_ms),
            ("memory_bytes", self.memory_bytes),
            ("recursion_depth", u64::from(self.recursion_depth)),
            ("import_depth", u64::from(self.import_depth)),
            ("imported_files", u64::from(self.imported_files)),
            ("source_bytes", self.source_bytes),
            ("output_bytes", self.output_bytes),
            ("retained_diagnostics", u64::from(self.retained_diagnostics)),
        ];
        if let Some((field, _)) = limits.into_iter().find(|(_, value)| *value == 0) {
            Err(CompositionError::ZeroEvaluationLimit { field })
        } else {
            Ok(())
        }
    }
}

impl Default for NickelEvaluationLimits {
    fn default() -> Self {
        Self {
            wall_clock_ms: 10_000,
            memory_bytes: 256 * 1024 * 1024,
            recursion_depth: 512,
            import_depth: 64,
            imported_files: 4_096,
            source_bytes: 16 * 1024 * 1024,
            output_bytes: 16 * 1024 * 1024,
            retained_diagnostics: 256,
        }
    }
}

/// Profile intent after complete Nickel evaluation.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CompositionSpec {
    /// Schema version owned by the Lattice composition contract.
    pub schema_version: u32,
    /// Stable profile registration ID.
    pub profile: StableId,
    /// Profile projection.
    pub profile_kind: ProfileKind,
    /// Exact domains selected by the projection.
    pub projection_domains: BTreeSet<PackageDomain>,
    /// Root requests keyed by logical package name.
    pub roots: BTreeMap<PackageName, PackageRequest>,
    /// Versioned capabilities requested directly by the profile.
    #[serde(default)]
    pub capabilities: BTreeMap<latticeaxiom_core::CapabilityId, CapabilityRequirement>,
    /// Package-qualified feature selections.
    #[serde(default)]
    pub features: BTreeMap<PackageName, BTreeSet<String>>,
    /// Graph-affecting, fully evaluated finite parameters.
    #[serde(default)]
    pub parameters: BTreeMap<StableId, Value>,
    /// Explicit semantic Role bindings.
    #[serde(default)]
    pub semantic_bindings: BTreeMap<StableId, StableId>,
    /// Ordered overlays. Only each semantic hash enters composition identity.
    #[serde(default)]
    pub overlays: Vec<OverlaySpec>,
    /// Candidate source universe, separate from root requests.
    pub sources: Vec<SourceCandidate>,
    /// Graph construction policy.
    pub policy: CompositionPolicy,
    /// Provenance of the evaluated profile root.
    pub provenance: SourceProvenance,
}

impl CompositionSpec {
    /// Validates schema and deterministic collection constraints.
    ///
    /// # Errors
    ///
    /// Returns [`CompositionError`] when the schema is unsupported, a source
    /// candidate is duplicated or malformed, or a policy has no automatic
    /// realization.
    pub fn validate(&self) -> Result<(), CompositionError> {
        if self.schema_version != COMPOSITION_SCHEMA_VERSION {
            return Err(CompositionError::UnsupportedSchema {
                found: self.schema_version,
                supported: COMPOSITION_SCHEMA_VERSION,
            });
        }
        if self.policy.realization_order.is_empty() {
            return Err(CompositionError::EmptyRealizationOrder);
        }
        validate_projection_domains(self.profile_kind, &self.projection_domains)?;
        validate_evaluation_policy(
            self.profile_kind,
            &self.policy.evaluation_policy,
            &self.policy.evaluation_limits,
        )?;

        let mut source_ids = BTreeSet::new();
        for source in &self.sources {
            if !source_ids.insert(&source.source_id) {
                return Err(CompositionError::DuplicateSourceId {
                    source_id: source.source_id.clone(),
                });
            }
            validate_source_candidate(source)?;
        }
        let mut realization_kinds = BTreeSet::new();
        for kind in &self.policy.realization_order {
            if !realization_kinds.insert(kind) {
                return Err(CompositionError::DuplicateRealizationPreference { kind: *kind });
            }
        }
        for (key, requirement) in &self.capabilities {
            if key != &requirement.capability {
                return Err(CompositionError::CapabilityKeyMismatch {
                    key: key.to_string(),
                    value: requirement.capability.to_string(),
                });
            }
        }
        validate_profile_namespace_grants(
            &self.profile,
            &self.roots,
            &self.policy.namespace_grants,
        )?;
        Ok(())
    }

    /// Hashes normalized semantic intent.
    ///
    /// Source paths, IDs, content receipts, and provenance are excluded, so
    /// relocating an identical source root cannot change semantic identity.
    /// Resolver-semantic package, version, and priority fields are included in
    /// deterministic order.
    ///
    /// # Errors
    ///
    /// Returns [`CanonicalJsonError`] if the canonical JSON payload cannot be
    /// encoded.
    pub fn semantic_hash(&self) -> Result<CanonicalHash, CanonicalJsonError> {
        let mut sources = self
            .sources
            .iter()
            .map(|source| CompositionSourceIdentity {
                package: &source.package,
                version: &source.version,
                priority: source.priority,
            })
            .collect::<Vec<_>>();
        sources.sort_unstable_by(|left, right| {
            left.package
                .cmp(right.package)
                .then_with(|| left.version.exact_cmp(right.version))
                .then_with(|| left.priority.cmp(&right.priority))
        });
        canonical_json_hash(&CompositionIdentity {
            schema_version: self.schema_version,
            profile: &self.profile,
            profile_kind: self.profile_kind,
            projection_domains: &self.projection_domains,
            roots: &self.roots,
            capabilities: &self.capabilities,
            features: &self.features,
            sources,
            parameters: &self.parameters,
            semantic_bindings: &self.semantic_bindings,
            overlay_hashes: self
                .overlays
                .iter()
                .map(|overlay| &overlay.semantic_hash)
                .collect(),
            policy: &self.policy,
        })
    }

    /// Hashes source, import, overlay, and profile provenance for audit use.
    ///
    /// # Errors
    ///
    /// Returns [`CanonicalJsonError`] if the provenance payload cannot be encoded.
    pub fn provenance_hash(&self) -> Result<CanonicalHash, CanonicalJsonError> {
        let mut sources = self.sources.iter().collect::<Vec<_>>();
        sources.sort_unstable_by(|left, right| {
            left.source_id
                .cmp(&right.source_id)
                .then_with(|| left.package.cmp(&right.package))
                .then_with(|| left.version.exact_cmp(&right.version))
                .then_with(|| left.content_hash.cmp(&right.content_hash))
        });
        canonical_json_hash(&CompositionProvenanceIdentity {
            profile: &self.provenance,
            sources,
            overlays: &self.overlays,
        })
    }
}

#[derive(Serialize)]
struct CompositionIdentity<'a> {
    schema_version: u32,
    profile: &'a StableId,
    profile_kind: ProfileKind,
    projection_domains: &'a BTreeSet<PackageDomain>,
    roots: &'a BTreeMap<PackageName, PackageRequest>,
    capabilities: &'a BTreeMap<latticeaxiom_core::CapabilityId, CapabilityRequirement>,
    features: &'a BTreeMap<PackageName, BTreeSet<String>>,
    sources: Vec<CompositionSourceIdentity<'a>>,
    parameters: &'a BTreeMap<StableId, Value>,
    semantic_bindings: &'a BTreeMap<StableId, StableId>,
    overlay_hashes: Vec<&'a CanonicalHash>,
    policy: &'a CompositionPolicy,
}

#[derive(Serialize)]
struct CompositionSourceIdentity<'a> {
    package: &'a PackageName,
    version: &'a PackageVersion,
    priority: i32,
}

#[derive(Serialize)]
struct CompositionProvenanceIdentity<'a> {
    profile: &'a SourceProvenance,
    sources: Vec<&'a SourceCandidate>,
    overlays: &'a [OverlaySpec],
}

/// A package dependency declaration.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PackageDependency {
    /// Compatible versions.
    pub version: PackageVersionReq,
    /// Whether the dependency is activated only by a feature.
    pub optional: bool,
    /// Package-local features, any one of which activates this optional edge.
    #[serde(default)]
    pub when_features: BTreeSet<String>,
    /// Feature names forwarded to the dependency.
    #[serde(default)]
    pub features: BTreeSet<String>,
    /// Domains in which this edge is active.
    pub domains: BTreeSet<PackageDomain>,
}

/// One package feature declaration.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FeatureSpec {
    /// Whether the feature is enabled when no profile override exists.
    pub default: bool,
    /// Domains activated by the feature.
    pub domains: BTreeSet<PackageDomain>,
}

/// Trust needed to evaluate, build, or activate package content.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum TrustClass {
    /// Pure evaluated data without package build scripts or native code.
    DataOnly,
    /// Local build execution is required.
    Build,
    /// Full-process trusted native code is required.
    TrustedNative,
}

/// Source or artifact intent authored by a realization.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case", tag = "kind")]
pub enum ArtifactIntent {
    /// Build from the package source root.
    SourceBuild,
    /// Select a local prebuilt artifact by a package-relative path.
    LocalPrebuilt {
        /// Slash-normalized package-relative artifact path.
        path: CanonicalLogicalPath,
    },
    /// Load a pure-data root.
    DataRoot {
        /// Slash-normalized package-relative data root.
        path: CanonicalLogicalPath,
    },
}

/// Non-semantic package display and legal metadata.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PackageMetadata {
    /// Human-readable display name.
    pub display_name: String,
    /// Optional documentation URL.
    pub documentation: Option<String>,
    /// SPDX license expression.
    pub license: String,
}

/// One realization published by a package.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RealizationSpec {
    /// Package-local stable realization ID.
    pub id: RealizationId,
    /// Realization family.
    pub kind: RealizationKind,
    /// Package domains implemented by this realization.
    pub domains: BTreeSet<PackageDomain>,
    /// Target triples supported by the realization, empty for target-neutral data.
    #[serde(default)]
    pub targets: BTreeSet<TargetTriple>,
    /// Required and optional versioned dynamic interfaces.
    pub interfaces: BTreeMap<StableId, InterfaceRequirement>,
    /// Features that must be active before this candidate is eligible.
    pub required_features: BTreeSet<String>,
    /// Source build, local prebuilt, or data-root intent.
    pub artifact: ArtifactIntent,
    /// Trust required to prepare and activate the realization.
    pub trust: TrustClass,
    /// Exact engine build required only by engine-coupled realizations.
    pub engine_build: Option<CanonicalHash>,
    /// Expected generated or data registration-fragment identity.
    pub registration_fragment: CanonicalHash,
}

/// One versioned dynamic interface used by a realization.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct InterfaceRequirement {
    /// Compatible interface version range.
    pub version: PackageVersionReq,
    /// Whether activation can proceed without this interface.
    pub optional: bool,
}

/// Evaluated `package.ncl` output before graph resolution.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PackageSpec {
    /// Package contract model version.
    pub model_version: u32,
    /// Logical package identity.
    pub name: PackageName,
    /// Exact package version.
    pub version: PackageVersion,
    /// Non-semantic display and legal metadata.
    pub metadata: PackageMetadata,
    /// Feature declarations keyed by package-local stable names.
    #[serde(default)]
    pub features: BTreeMap<String, FeatureSpec>,
    /// Dependencies keyed by logical package name.
    #[serde(default)]
    pub dependencies: BTreeMap<PackageName, PackageDependency>,
    /// Versioned capabilities required by the package.
    #[serde(default)]
    pub requires: BTreeMap<latticeaxiom_core::CapabilityId, CapabilityRequirement>,
    /// Versioned capability IDs provided by the package.
    #[serde(default)]
    pub provides: BTreeMap<latticeaxiom_core::CapabilityId, CapabilityProvision>,
    /// Published realization candidates keyed by a stable local label.
    pub realizations: BTreeMap<RealizationId, RealizationSpec>,
    /// Package domains used to project client, server, and authoritative closures.
    pub domains: BTreeSet<PackageDomain>,
    /// Graph-affecting parameter schema as finite normalized JSON data.
    #[serde(default)]
    pub parameters: BTreeMap<StableId, crate::CompositionParameterSpec>,
    /// Complete registration patterns requested from incoming owner-bound authority.
    #[serde(default)]
    pub namespace_requests: BTreeSet<NamespaceGrantPattern>,
    /// Registration authority delegated to direct dependencies, keyed by grantee.
    #[serde(default)]
    pub namespace_delegations: BTreeMap<PackageName, BTreeSet<NamespaceGrantPattern>>,
    /// Highest trust class required by any package operation.
    pub trust: TrustClass,
    /// Registration fragments emitted by this data package.
    #[serde(default)]
    pub registration: crate::RegistrationFragment,
    /// Source declaration provenance.
    pub provenance: SourceProvenance,
}

impl PackageSpec {
    /// Validates package-wide domain, capability, trust, and realization rules.
    ///
    /// # Errors
    ///
    /// Returns a deterministic [`CompositionError`] for the first invalid row
    /// in stable map order.
    pub fn validate(&self) -> Result<(), CompositionError> {
        if self.model_version != PACKAGE_MODEL_VERSION {
            return Err(CompositionError::UnsupportedPackageSchema {
                found: self.model_version,
                supported: PACKAGE_MODEL_VERSION,
            });
        }
        if self.domains.is_empty() {
            return Err(CompositionError::EmptyPackageDomains {
                package: self.name.clone(),
            });
        }
        if self.realizations.is_empty() {
            return Err(CompositionError::EmptyRealizations {
                package: self.name.clone(),
            });
        }

        self.validate_dependencies_and_features()?;
        self.validate_capabilities_and_namespaces()?;
        for (key, parameter) in &self.parameters {
            if key != &parameter.id || parameter.declared_by != self.name {
                return Err(CompositionError::ParameterDeclarationMismatch {
                    package: self.name.clone(),
                    key: key.to_string(),
                    value: parameter.id.to_string(),
                });
            }
            if let Err(error) = parameter.validate() {
                return Err(CompositionError::InvalidParameterDefault {
                    package: self.name.clone(),
                    parameter: key.to_string(),
                    reason: error.to_string(),
                });
            }
        }
        self.validate_realizations()
    }

    fn validate_dependencies_and_features(&self) -> Result<(), CompositionError> {
        for (name, dependency) in &self.dependencies {
            ensure_domain_subset(
                &self.name,
                name.as_str(),
                &dependency.domains,
                &self.domains,
            )?;
            if dependency.optional == dependency.when_features.is_empty() {
                return Err(CompositionError::InvalidDependencyActivation {
                    package: self.name.clone(),
                    dependency: name.clone(),
                });
            }
            for feature in &dependency.when_features {
                if !self.features.contains_key(feature) {
                    return Err(CompositionError::UnknownDependencyFeature {
                        package: self.name.clone(),
                        dependency: name.clone(),
                        feature: feature.clone(),
                    });
                }
            }
        }
        for (name, feature) in &self.features {
            ensure_domain_subset(
                &self.name,
                &format!("feature:{name}"),
                &feature.domains,
                &self.domains,
            )?;
        }
        Ok(())
    }

    fn validate_capabilities_and_namespaces(&self) -> Result<(), CompositionError> {
        for (key, requirement) in &self.requires {
            if key != &requirement.capability {
                return Err(CompositionError::CapabilityKeyMismatch {
                    key: key.to_string(),
                    value: requirement.capability.to_string(),
                });
            }
            ensure_domain_subset(
                &self.name,
                key.as_str(),
                &requirement.domains,
                &self.domains,
            )?;
        }
        for (key, provision) in &self.provides {
            if key != &provision.capability {
                return Err(CompositionError::CapabilityKeyMismatch {
                    key: key.to_string(),
                    value: provision.capability.to_string(),
                });
            }
            ensure_domain_subset(&self.name, key.as_str(), &provision.domains, &self.domains)?;
        }
        for (grantee, patterns) in &self.namespace_delegations {
            if !self.dependencies.contains_key(grantee) {
                return Err(CompositionError::NamespaceDelegationToNonDependency {
                    package: self.name.clone(),
                    grantee: grantee.clone(),
                });
            }
            for pattern in patterns {
                if !self
                    .namespace_requests
                    .iter()
                    .any(|request| request.covers(pattern))
                {
                    return Err(CompositionError::NamespaceDelegationWidensAuthority {
                        package: self.name.clone(),
                        grantee: grantee.clone(),
                        pattern: pattern.clone(),
                    });
                }
            }
        }
        Ok(())
    }

    fn validate_realizations(&self) -> Result<(), CompositionError> {
        for (key, realization) in &self.realizations {
            if key != &realization.id {
                return Err(CompositionError::RealizationKeyMismatch {
                    key: key.clone(),
                    value: realization.id.clone(),
                });
            }
            if realization.domains.is_empty() || !realization.domains.is_subset(&self.domains) {
                return Err(CompositionError::InvalidRealizationDomains {
                    package: self.name.clone(),
                    realization: key.clone(),
                });
            }
            if realization.trust > self.trust {
                return Err(CompositionError::RealizationTrustExceedsPackage {
                    package: self.name.clone(),
                    realization: key.clone(),
                });
            }
            for feature in &realization.required_features {
                if !self.features.contains_key(feature) {
                    return Err(CompositionError::UnknownRealizationFeature {
                        package: self.name.clone(),
                        realization: key.clone(),
                        feature: feature.clone(),
                    });
                }
            }
            let valid_boundary = match realization.kind {
                RealizationKind::Data | RealizationKind::NativeStatic => {
                    realization.interfaces.is_empty() && realization.engine_build.is_none()
                }
                RealizationKind::PortableNative => {
                    !realization.interfaces.is_empty() && realization.engine_build.is_none()
                }
                RealizationKind::EngineCoupledNative => realization.engine_build.is_some(),
            };
            if !valid_boundary {
                return Err(CompositionError::InvalidRealizationBoundary {
                    package: self.name.clone(),
                    realization: key.clone(),
                    kind: realization.kind,
                });
            }
        }
        Ok(())
    }
}

fn ensure_domain_subset(
    package: &PackageName,
    row: &str,
    row_domains: &BTreeSet<PackageDomain>,
    package_domains: &BTreeSet<PackageDomain>,
) -> Result<(), CompositionError> {
    if row_domains.is_subset(package_domains) {
        Ok(())
    } else {
        Err(CompositionError::DomainOutsidePackage {
            package: package.clone(),
            row: row.to_owned(),
        })
    }
}

/// Fully evaluated profile authoring contract before normalization.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GameProfileSpec {
    /// Profile contract model version.
    pub model_version: u32,
    /// Stable profile ID.
    pub profile: StableId,
    /// Domain projection.
    pub projection: ProfileKind,
    /// Exact domains selected by the projection.
    pub projection_domains: BTreeSet<PackageDomain>,
    /// Root package requirements.
    pub roots: BTreeMap<PackageName, PackageRequest>,
    /// Versioned capabilities requested directly by the profile.
    #[serde(default)]
    pub capabilities: BTreeMap<latticeaxiom_core::CapabilityId, CapabilityRequirement>,
    /// Controlled candidate source universe.
    pub source_universe: Vec<SourceCandidate>,
    /// Package-qualified feature selections.
    #[serde(default)]
    pub features: BTreeMap<PackageName, BTreeSet<String>>,
    /// Graph-affecting finite parameter values.
    #[serde(default)]
    pub parameters: BTreeMap<StableId, Value>,
    /// Ordered automatic realization preference.
    pub realization_policy: Vec<RealizationKind>,
    /// Explicit content Role bindings.
    #[serde(default)]
    pub semantic_bindings: BTreeMap<StableId, StableId>,
    /// Ordered overlay provenance. Order is semantically meaningful.
    #[serde(default)]
    pub overlays: Vec<OverlaySpec>,
    /// Versioned Nickel evaluation policy.
    pub evaluation_policy: StableId,
    /// Effective evaluator limits requested by the profile.
    pub evaluation_limits: NickelEvaluationLimits,
    /// Namespace, trust, force-override, and recovery permissions.
    pub policy: ProfilePolicy,
}

impl GameProfileSpec {
    /// Validates profile schema, sources, capabilities, and evaluation policy.
    ///
    /// # Errors
    ///
    /// Returns a deterministic [`CompositionError`] for the first invalid row
    /// in source order or stable map order.
    pub fn validate(&self) -> Result<(), CompositionError> {
        if self.model_version != GAME_PROFILE_MODEL_VERSION {
            return Err(CompositionError::UnsupportedProfileSchema {
                found: self.model_version,
                supported: GAME_PROFILE_MODEL_VERSION,
            });
        }
        if self.realization_policy.is_empty() {
            return Err(CompositionError::EmptyRealizationOrder);
        }
        validate_projection_domains(self.projection, &self.projection_domains)?;
        validate_evaluation_policy(
            self.projection,
            &self.evaluation_policy,
            &self.evaluation_limits,
        )?;

        let mut source_ids = BTreeSet::new();
        for source in &self.source_universe {
            if !source_ids.insert(&source.source_id) {
                return Err(CompositionError::DuplicateSourceId {
                    source_id: source.source_id.clone(),
                });
            }
            validate_source_candidate(source)?;
        }
        let mut realization_kinds = BTreeSet::new();
        for kind in &self.realization_policy {
            if !realization_kinds.insert(kind) {
                return Err(CompositionError::DuplicateRealizationPreference { kind: *kind });
            }
        }
        for (key, requirement) in &self.capabilities {
            if key != &requirement.capability {
                return Err(CompositionError::CapabilityKeyMismatch {
                    key: key.to_string(),
                    value: requirement.capability.to_string(),
                });
            }
        }
        validate_profile_namespace_grants(
            &self.profile,
            &self.roots,
            &self.policy.namespace_grants,
        )?;
        Ok(())
    }

    /// Normalizes a validated authoring profile into resolver input.
    ///
    /// The target and profile provenance are kernel-owned inputs rather than
    /// author-authored identity. All graph-affecting profile fields are carried
    /// into the resulting [`CompositionSpec`].
    ///
    /// # Errors
    ///
    /// Returns [`CompositionError`] if either the authoring profile or the
    /// normalized composition violates its schema invariants.
    pub fn into_composition(
        self,
        target: TargetTriple,
        provenance: SourceProvenance,
    ) -> Result<CompositionSpec, CompositionError> {
        self.validate()?;
        let Self {
            model_version: _,
            profile,
            projection,
            projection_domains,
            roots,
            capabilities,
            source_universe,
            features,
            parameters,
            realization_policy,
            semantic_bindings,
            overlays,
            evaluation_policy,
            evaluation_limits,
            policy,
        } = self;
        let composition = CompositionSpec {
            schema_version: COMPOSITION_SCHEMA_VERSION,
            profile,
            profile_kind: projection,
            projection_domains,
            roots,
            capabilities,
            features,
            parameters,
            semantic_bindings,
            overlays,
            sources: source_universe,
            policy: CompositionPolicy {
                target,
                realization_order: realization_policy,
                namespace_grants: policy.namespace_grants,
                maximum_trust: policy.maximum_trust,
                allow_force_override: policy.allow_force_override,
                allow_recovery: policy.allow_recovery,
                evaluation_policy,
                evaluation_limits,
            },
            provenance,
        };
        composition.validate()?;
        Ok(composition)
    }
}

fn validate_profile_namespace_grants(
    profile: &StableId,
    roots: &BTreeMap<PackageName, PackageRequest>,
    grants: &BTreeMap<PackageName, BTreeSet<NamespaceGrantPattern>>,
) -> Result<(), CompositionError> {
    if let Some(grantee) = grants.keys().find(|grantee| !roots.contains_key(*grantee)) {
        Err(CompositionError::ProfileNamespaceGrantToNonRoot {
            profile: profile.clone(),
            grantee: grantee.clone(),
        })
    } else {
        Ok(())
    }
}

fn validate_source_candidate(source: &SourceCandidate) -> Result<(), CompositionError> {
    validate_source_candidate_path(&source.path).map_err(|reason| {
        CompositionError::InvalidSourceCandidatePath {
            source_id: source.source_id.clone(),
            path: source.path.clone(),
            reason,
        }
    })?;
    if source.provenance.source_id() != &source.source_id {
        return Err(CompositionError::SourceProvenanceMismatch {
            candidate: source.source_id.to_string(),
            provenance: source.provenance.source_id().to_string(),
        });
    }
    Ok(())
}

fn validate_source_candidate_path(path: &str) -> Result<(), &'static str> {
    if path.is_empty() {
        return Err("the path cannot be empty");
    }
    if path.starts_with('/') {
        return Err("the path must be root-relative");
    }
    let bytes = path.as_bytes();
    if bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' {
        return Err("the path cannot contain a Windows drive prefix");
    }
    if path.contains('\\') {
        return Err("the path must use `/` separators");
    }
    if path.contains('\0') {
        return Err("the path cannot contain NUL");
    }
    if !path.nfc().eq(path.chars()) {
        return Err("the path must already be NFC");
    }
    if path
        .split('/')
        .any(|segment| segment.is_empty() || matches!(segment, "." | ".."))
    {
        return Err("path segments cannot be empty, `.` or `..`");
    }
    Ok(())
}

fn validate_projection_domains(
    projection: ProfileKind,
    domains: &BTreeSet<PackageDomain>,
) -> Result<(), CompositionError> {
    let expected = match projection {
        ProfileKind::ClientShell => BTreeSet::from([PackageDomain::Client]),
        ProfileKind::ClientWorld => {
            BTreeSet::from([PackageDomain::Authoritative, PackageDomain::Client])
        }
        ProfileKind::DedicatedServer => {
            BTreeSet::from([PackageDomain::Authoritative, PackageDomain::Server])
        }
        ProfileKind::Tool => BTreeSet::from([PackageDomain::Tool]),
        ProfileKind::HeadlessTest if !domains.is_empty() => return Ok(()),
        ProfileKind::HeadlessTest => BTreeSet::new(),
    };
    if domains == &expected && !domains.is_empty() {
        Ok(())
    } else {
        Err(CompositionError::InvalidProjectionDomains { projection })
    }
}

fn validate_evaluation_policy(
    projection: ProfileKind,
    policy: &StableId,
    limits: &NickelEvaluationLimits,
) -> Result<(), CompositionError> {
    if policy.namespace() != "latticeaxiom" {
        return Err(CompositionError::UnsupportedEvaluationPolicy {
            policy: policy.to_string(),
        });
    }
    if policy.as_str() == R0_NICKEL_EVALUATION_POLICY {
        if limits == &NickelEvaluationLimits::default() {
            Ok(())
        } else {
            Err(CompositionError::R0EvaluationLimitsMismatch)
        }
    } else if projection == ProfileKind::Tool
        && policy.kind() == "nickel-evaluation-policy"
        && policy.major().is_some()
    {
        limits.validate()
    } else {
        Err(CompositionError::UnsupportedEvaluationPolicy {
            policy: policy.to_string(),
        })
    }
}

/// Authorities granted to a composition profile.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProfilePolicy {
    /// Trusted registration grants keyed by the root package receiving authority.
    pub namespace_grants: BTreeMap<PackageName, BTreeSet<NamespaceGrantPattern>>,
    /// Highest package trust accepted by this profile.
    pub maximum_trust: TrustClass,
    /// Whether trusted policy overlays may force a conflicting value.
    pub allow_force_override: bool,
    /// Whether read-only recovery actions are allowed.
    pub allow_recovery: bool,
}

/// Composition model validation failures.
#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum CompositionError {
    /// A package-local realization ID is malformed.
    #[error("invalid realization identifier `{value}`")]
    InvalidRealizationId {
        /// Rejected identifier text.
        value: String,
    },
    /// The package authoring schema version is unsupported.
    #[error("unsupported package schema {found}; supported schema is {supported}")]
    UnsupportedPackageSchema {
        /// Version found in the package.
        found: u32,
        /// Version implemented by this crate.
        supported: u32,
    },
    /// The profile authoring schema version is unsupported.
    #[error("unsupported profile schema {found}; supported schema is {supported}")]
    UnsupportedProfileSchema {
        /// Version found in the profile.
        found: u32,
        /// Version implemented by this crate.
        supported: u32,
    },
    /// A package declares no execution domains.
    #[error("package {package} must declare at least one domain")]
    EmptyPackageDomains {
        /// Invalid package.
        package: PackageName,
    },
    /// A package declares no realization candidates.
    #[error("package {package} must declare at least one realization")]
    EmptyRealizations {
        /// Invalid package.
        package: PackageName,
    },
    /// A conditional row references a domain not declared by its package.
    #[error("row `{row}` uses a domain outside package {package}")]
    DomainOutsidePackage {
        /// Invalid package.
        package: PackageName,
        /// Stable row key.
        row: String,
    },
    /// A profile attempts to grant registration authority to a non-root package.
    #[error("profile {profile} grants namespace authority to non-root package {grantee}")]
    ProfileNamespaceGrantToNonRoot {
        /// Profile issuing the trusted grant.
        profile: StableId,
        /// Package that is not a root request.
        grantee: PackageName,
    },
    /// A package attempts to delegate registration authority to a non-dependency.
    #[error("package {package} delegates namespace authority to non-dependency {grantee}")]
    NamespaceDelegationToNonDependency {
        /// Package issuing the delegation.
        package: PackageName,
        /// Package that is not a direct manifest dependency.
        grantee: PackageName,
    },
    /// A package delegation is not covered by the package's own authority request.
    #[error("package {package} widens namespace authority for {grantee} with `{pattern}`")]
    NamespaceDelegationWidensAuthority {
        /// Package issuing the delegation.
        package: PackageName,
        /// Direct dependency receiving the invalid pattern.
        grantee: PackageName,
        /// Pattern outside the package's own requested authority.
        pattern: NamespaceGrantPattern,
    },
    /// Realization map key and row identity disagree.
    #[error("realization map key {key} does not match row value {value}")]
    RealizationKeyMismatch {
        /// Map key.
        key: RealizationId,
        /// ID stored in the row.
        value: RealizationId,
    },
    /// Realization domains are empty or escape package domains.
    #[error("realization {realization} has invalid domains for package {package}")]
    InvalidRealizationDomains {
        /// Invalid package.
        package: PackageName,
        /// Invalid realization.
        realization: RealizationId,
    },
    /// Realization trust exceeds the package trust declaration.
    #[error("realization {realization} exceeds the trust declared by package {package}")]
    RealizationTrustExceedsPackage {
        /// Invalid package.
        package: PackageName,
        /// Invalid realization.
        realization: RealizationId,
    },
    /// Realization interfaces or engine-build receipt conflict with its kind.
    #[error("realization {realization} in package {package} has an invalid {kind:?} boundary")]
    InvalidRealizationBoundary {
        /// Invalid package.
        package: PackageName,
        /// Invalid realization.
        realization: RealizationId,
        /// Declared realization kind.
        kind: RealizationKind,
    },
    /// The input uses an unsupported schema version.
    #[error("unsupported composition schema {found}; supported schema is {supported}")]
    UnsupportedSchema {
        /// Version found in the input.
        found: u32,
        /// Version implemented by this crate.
        supported: u32,
    },
    /// Automatic realization selection has no candidates.
    #[error("composition policy must contain at least one realization preference")]
    EmptyRealizationOrder,
    /// The same stable source ID appears more than once.
    #[error("duplicate source identifier {source_id}")]
    DuplicateSourceId {
        /// Duplicated source ID.
        source_id: SourceId,
    },
    /// A source candidate and its diagnostic provenance disagree on identity.
    #[error("source candidate {candidate} carries provenance for {provenance}")]
    SourceProvenanceMismatch {
        /// Candidate source-universe ID.
        candidate: String,
        /// Source ID recorded by provenance.
        provenance: String,
    },
    /// A source candidate used a non-canonical or escaping logical root path.
    #[error("source candidate {source_id} has invalid path `{path}`: {reason}")]
    InvalidSourceCandidatePath {
        /// Source candidate carrying the invalid path.
        source_id: SourceId,
        /// Rejected logical root path.
        path: String,
        /// Violated canonical-path rule.
        reason: &'static str,
    },
    /// One effective evaluator limit was zero.
    #[error("Nickel evaluation limit `{field}` must be nonzero")]
    ZeroEvaluationLimit {
        /// Invalid limit field.
        field: &'static str,
    },
    /// A fixed profile projection carried the wrong domains or a headless one was empty.
    #[error("profile projection {projection:?} has invalid explicit domains")]
    InvalidProjectionDomains {
        /// Projection whose domains were invalid.
        projection: ProfileKind,
    },
    /// The fixed R0 policy was paired with non-default effective limits.
    #[error("the R0 Nickel policy requires its exact frozen effective limits")]
    R0EvaluationLimitsMismatch,
    /// The selected Nickel policy is not implemented by this schema version.
    #[error("unsupported Nickel evaluation policy {policy}")]
    UnsupportedEvaluationPolicy {
        /// Rejected policy ID.
        policy: String,
    },
    /// An optional dependency has no activation feature or a required edge has one.
    #[error("dependency {dependency} in package {package} has inconsistent optional activation")]
    InvalidDependencyActivation {
        /// Declaring package.
        package: PackageName,
        /// Dependency package.
        dependency: PackageName,
    },
    /// An optional dependency activation references an undeclared feature.
    #[error("dependency {dependency} in package {package} references unknown feature `{feature}`")]
    UnknownDependencyFeature {
        /// Declaring package.
        package: PackageName,
        /// Dependency package.
        dependency: PackageName,
        /// Unknown package-local feature.
        feature: String,
    },
    /// A realization eligibility condition references an undeclared feature.
    #[error(
        "realization {realization} in package {package} references unknown feature `{feature}`"
    )]
    UnknownRealizationFeature {
        /// Declaring package.
        package: PackageName,
        /// Realization with the invalid condition.
        realization: RealizationId,
        /// Unknown package-local feature.
        feature: String,
    },
    /// A parameter map key, row ID, or declaring package disagrees.
    #[error("parameter `{key}` in package {package} disagrees with row `{value}` or owner")]
    ParameterDeclarationMismatch {
        /// Declaring package.
        package: PackageName,
        /// Parameter map key.
        key: String,
        /// Parameter row ID.
        value: String,
    },
    /// A typed composition-parameter schema or default was invalid.
    #[error("parameter `{parameter}` in package {package} is invalid: {reason}")]
    InvalidParameterDefault {
        /// Declaring package.
        package: PackageName,
        /// Invalid parameter ID.
        parameter: String,
        /// Typed value validation diagnostic.
        reason: String,
    },
    /// Automatic realization policy repeats one kind.
    #[error("duplicate realization preference {kind:?}")]
    DuplicateRealizationPreference {
        /// Repeated realization kind.
        kind: RealizationKind,
    },
    /// A capability map key disagrees with the row value.
    #[error("capability map key {key} does not match row value {value}")]
    CapabilityKeyMismatch {
        /// Map key.
        key: String,
        /// Capability stored in the row.
        value: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use latticeaxiom_core::{CanonicalHash, CapabilityId, SourceSpan};

    #[test]
    fn composition_hash_separates_semantics_from_provenance() {
        let first = composition(
            "latticeaxiom:source/one",
            "profiles/dev.ncl",
            "packages/one",
        );
        let second = composition(
            "latticeaxiom:source/two",
            "moved/dev.ncl",
            "moved/packages/one",
        );

        assert!(first.validate().is_ok());
        assert!(second.validate().is_ok());
        assert_eq!(first.semantic_hash().ok(), second.semantic_hash().ok());
        assert_ne!(first.provenance_hash().ok(), second.provenance_hash().ok());

        let mut changed_features = first.clone();
        changed_features.features.insert(
            package_name("terrenia"),
            BTreeSet::from(["worldgen".to_owned()]),
        );
        assert_ne!(
            first.semantic_hash().ok(),
            changed_features.semantic_hash().ok()
        );

        let mut changed_permissions = first.clone();
        changed_permissions.policy.maximum_trust = TrustClass::DataOnly;
        assert_ne!(
            first.semantic_hash().ok(),
            changed_permissions.semantic_hash().ok()
        );

        let mut changed_source_package = first.clone();
        changed_source_package.sources[0].package = package_name("@terrenia/alternate");
        assert_ne!(
            first.semantic_hash().ok(),
            changed_source_package.semantic_hash().ok()
        );

        let mut changed_source_version = first.clone();
        changed_source_version.sources[0].version = version("0.2.0");
        assert_ne!(
            first.semantic_hash().ok(),
            changed_source_version.semantic_hash().ok()
        );

        let mut changed_source_priority = first.clone();
        changed_source_priority.sources[0].priority = 10;
        assert_ne!(
            first.semantic_hash().ok(),
            changed_source_priority.semantic_hash().ok()
        );

        let mut ordered_sources = first.clone();
        let mut additional_source = ordered_sources.sources[0].clone();
        additional_source.source_id = parse_source_id("latticeaxiom:source/additional");
        additional_source.package = package_name("@terrenia/additional");
        additional_source.version = version("1.0.0");
        additional_source.path = "packages/additional".to_owned();
        additional_source.priority = -5;
        additional_source.provenance = provenance(
            "latticeaxiom:source/additional",
            "packages/additional/package.ncl",
        );
        ordered_sources.sources.push(additional_source);
        assert!(ordered_sources.validate().is_ok());
        let mut reversed_sources = ordered_sources.clone();
        reversed_sources.sources.reverse();
        assert_eq!(
            ordered_sources.semantic_hash().ok(),
            reversed_sources.semantic_hash().ok()
        );
    }

    #[test]
    fn breaking_model_boundaries_use_coordinated_majors() {
        assert_eq!(COMPOSITION_SCHEMA_VERSION, 3);
        assert_eq!(PACKAGE_MODEL_VERSION, 3);
        assert_eq!(GAME_PROFILE_MODEL_VERSION, 3);
        assert_eq!(NICKEL_LIBRARY_CONTRACT_MAJOR, 3);
        assert_eq!(R0_AUTHORING_CORPUS_MAJOR, 3);
    }

    #[test]
    fn composition_round_trips_and_rejects_unknown_fields() {
        let spec = composition(
            "latticeaxiom:source/one",
            "profiles/dev.ncl",
            "packages/one",
        );
        let encoded = serde_json::to_string(&spec)
            .unwrap_or_else(|error| panic!("composition did not serialize: {error}"));
        let decoded = serde_json::from_str::<CompositionSpec>(&encoded)
            .unwrap_or_else(|error| panic!("composition did not deserialize: {error}"));
        assert_eq!(decoded, spec);

        let mut value = serde_json::to_value(spec)
            .unwrap_or_else(|error| panic!("composition did not convert to JSON: {error}"));
        let serde_json::Value::Object(fields) = &mut value else {
            panic!("composition must serialize as an object");
        };
        fields.insert("unknown".to_owned(), serde_json::Value::Bool(true));
        assert!(serde_json::from_value::<CompositionSpec>(value).is_err());
    }

    #[test]
    fn package_empty_collections_have_typed_serde_defaults() {
        let package = data_package();
        let mut value = serde_json::to_value(package)
            .unwrap_or_else(|error| panic!("package did not convert to JSON: {error}"));
        let serde_json::Value::Object(fields) = &mut value else {
            panic!("package must serialize as an object");
        };
        fields.remove("features");
        fields.remove("namespace_requests");
        fields.remove("namespace_delegations");

        let decoded = serde_json::from_value::<PackageSpec>(value)
            .unwrap_or_else(|error| panic!("defaulted package did not deserialize: {error}"));
        assert!(decoded.features.is_empty());
        assert!(decoded.namespace_requests.is_empty());
        assert!(decoded.namespace_delegations.is_empty());
    }

    #[test]
    fn capability_map_key_must_match_its_row() {
        let mut spec = composition(
            "latticeaxiom:source/one",
            "profiles/dev.ncl",
            "packages/one",
        );
        let key = capability("latticeaxiom:capability/settings-registry@1");
        spec.capabilities.insert(
            key,
            CapabilityRequirement {
                capability: capability("latticeaxiom:capability/diagnostic-registry@1"),
                version: version_req("=1.0.0"),
                provider: None,
                cardinality: CapabilityCardinality::ExactlyOne,
                domains: BTreeSet::from([PackageDomain::Authoritative]),
            },
        );
        assert!(matches!(
            spec.validate(),
            Err(CompositionError::CapabilityKeyMismatch { .. })
        ));
    }

    #[test]
    fn package_validation_enforces_realization_boundaries() {
        let mut package = data_package();
        assert!(package.validate().is_ok());

        let realization = package
            .realizations
            .values_mut()
            .next()
            .unwrap_or_else(|| panic!("fixture must contain one realization"));
        realization.kind = RealizationKind::PortableNative;
        assert!(matches!(
            package.validate(),
            Err(CompositionError::InvalidRealizationBoundary { .. })
        ));
    }

    #[test]
    fn realization_id_deserialization_revalidates_grammar() {
        assert!(serde_json::from_str::<RealizationId>(r#""portable-main""#).is_ok());
        assert!(serde_json::from_str::<RealizationId>(r#""Portable Main""#).is_err());
        assert!(
            serde_json::from_value::<ArtifactIntent>(serde_json::json!({
                "kind": "data-root",
                "path": "assets",
                "unknown": true
            }))
            .is_err()
        );
        for path in [
            "../artifact",
            "/absolute/artifact",
            "C:/artifact",
            r"artifacts\plugin",
            "artifacts/./plugin",
            "artifacts/e\u{301}",
        ] {
            assert!(
                serde_json::from_value::<ArtifactIntent>(serde_json::json!({
                    "kind": "local-prebuilt",
                    "path": path
                }))
                .is_err(),
                "expected noncanonical artifact path `{path}` to be rejected"
            );
        }
    }

    #[test]
    fn source_candidate_requires_matching_provenance_identity() {
        let mut spec = composition(
            "latticeaxiom:source/one",
            "profiles/dev.ncl",
            "packages/one",
        );
        spec.sources[0].provenance = provenance("latticeaxiom:source/two", "profiles/dev.ncl");
        assert!(matches!(
            spec.validate(),
            Err(CompositionError::SourceProvenanceMismatch { .. })
        ));
    }

    #[test]
    fn source_candidate_path_must_be_canonical_and_root_relative() {
        let baseline = composition(
            "latticeaxiom:source/one",
            "profiles/dev.ncl",
            "packages/one",
        );
        let mut composed_unicode = baseline.clone();
        composed_unicode.sources[0].path = "packages/café".to_owned();
        assert!(composed_unicode.validate().is_ok());
        for invalid_path in [
            "",
            ".",
            "/packages/one",
            "C:/packages/one",
            "packages\\one",
            "packages/./one",
            "packages/../one",
            "packages//one",
            "packages/\0one",
            "packages/cafe\u{301}",
        ] {
            let mut invalid = baseline.clone();
            invalid.sources[0].path = invalid_path.to_owned();
            assert!(
                matches!(
                    invalid.validate(),
                    Err(CompositionError::InvalidSourceCandidatePath { .. })
                ),
                "accepted invalid source candidate path `{invalid_path}`"
            );
        }
    }

    #[test]
    fn game_profile_validates_policy_limits_and_sources() {
        let composition = composition(
            "latticeaxiom:source/one",
            "profiles/dev.ncl",
            "packages/one",
        );
        let mut profile = GameProfileSpec {
            model_version: GAME_PROFILE_MODEL_VERSION,
            profile: composition.profile,
            projection: composition.profile_kind,
            projection_domains: composition.projection_domains,
            roots: composition.roots,
            capabilities: composition.capabilities,
            source_universe: composition.sources,
            features: BTreeMap::new(),
            parameters: composition.parameters,
            realization_policy: composition.policy.realization_order,
            semantic_bindings: composition.semantic_bindings,
            overlays: composition.overlays,
            evaluation_policy: composition.policy.evaluation_policy,
            evaluation_limits: composition.policy.evaluation_limits,
            policy: ProfilePolicy {
                namespace_grants: BTreeMap::new(),
                maximum_trust: TrustClass::DataOnly,
                allow_force_override: false,
                allow_recovery: true,
            },
        };
        assert!(profile.validate().is_ok());

        let root = profile
            .roots
            .keys()
            .next()
            .cloned()
            .unwrap_or_else(|| panic!("profile fixture must contain one root"));
        profile
            .features
            .insert(root, BTreeSet::from(["worldgen".to_owned()]));
        profile.policy.namespace_grants.insert(
            profile.roots.keys().next().cloned().unwrap_or_else(|| {
                panic!("profile fixture must retain its root namespace grantee")
            }),
            BTreeSet::from([grant_pattern("terrenia:block/**")]),
        );
        let normalized = profile
            .clone()
            .into_composition(
                target("x86_64-unknown-linux-gnu"),
                provenance("latticeaxiom:source/profile", "profiles/headless.ncl"),
            )
            .unwrap_or_else(|error| panic!("valid profile must normalize: {error}"));
        assert_eq!(normalized.features, profile.features);
        assert_eq!(
            normalized.policy.namespace_grants,
            profile.policy.namespace_grants
        );
        assert_eq!(normalized.policy.maximum_trust, TrustClass::DataOnly);
        assert!(!normalized.policy.allow_force_override);
        assert!(normalized.policy.allow_recovery);

        profile.evaluation_limits.import_depth = 0;
        assert!(matches!(
            profile.validate(),
            Err(CompositionError::R0EvaluationLimitsMismatch)
        ));

        profile.evaluation_limits = NickelEvaluationLimits::default();
        profile.projection_domains.clear();
        assert!(matches!(
            profile.validate(),
            Err(CompositionError::InvalidProjectionDomains { .. })
        ));
    }

    #[test]
    fn evaluation_policy_requires_the_latticeaxiom_namespace() {
        let foreign_policy = stable_id("foreign:nickel-evaluation-policy/developer@1");
        assert!(matches!(
            validate_evaluation_policy(
                ProfileKind::Tool,
                &foreign_policy,
                &NickelEvaluationLimits::default(),
            ),
            Err(CompositionError::UnsupportedEvaluationPolicy { .. })
        ));
    }

    #[test]
    fn package_validation_enforces_feature_activation_edges() {
        let mut package = data_package();
        package.features.insert(
            "extras".to_owned(),
            FeatureSpec {
                default: false,
                domains: BTreeSet::from([PackageDomain::Authoritative]),
            },
        );
        package.dependencies.insert(
            package_name("@terrenia/extras"),
            PackageDependency {
                version: version_req("=0.1.0"),
                optional: true,
                when_features: BTreeSet::from(["extras".to_owned()]),
                features: BTreeSet::new(),
                domains: BTreeSet::from([PackageDomain::Authoritative]),
            },
        );
        assert!(package.validate().is_ok());

        package.dependencies.values_mut().for_each(|dependency| {
            dependency.when_features = BTreeSet::from(["missing".to_owned()]);
        });
        assert!(matches!(
            package.validate(),
            Err(CompositionError::UnknownDependencyFeature { .. })
        ));
    }

    #[test]
    fn package_validation_rejects_an_invalid_parameter_default() {
        let mut package = data_package();
        let id = stable_id("terrenia:parameter/ore-density");
        package.parameters.insert(
            id.clone(),
            crate::CompositionParameterSpec {
                id,
                declared_by: package.name.clone(),
                value_type: crate::ValueType::Integer {
                    min: Some(1),
                    max: Some(8),
                    step: Some(1),
                },
                default: serde_json::json!(0),
                apply_impact: crate::CompositionApplyImpact::GraphRecompose,
            },
        );
        assert!(matches!(
            package.validate(),
            Err(CompositionError::InvalidParameterDefault { .. })
        ));
    }

    fn composition(source_id: &str, logical_path: &str, source_path: &str) -> CompositionSpec {
        let package = package_name("terrenia");
        let source_provenance = provenance(source_id, logical_path);
        CompositionSpec {
            schema_version: COMPOSITION_SCHEMA_VERSION,
            profile: stable_id("latticeaxiom:profile/dev"),
            profile_kind: ProfileKind::HeadlessTest,
            projection_domains: BTreeSet::from([PackageDomain::Authoritative]),
            roots: BTreeMap::from([(
                package.clone(),
                PackageRequest {
                    version: version_req("~0.1.0"),
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
                source_id: parse_source_id(source_id),
                package,
                version: version("0.1.0"),
                path: source_path.to_owned(),
                content_hash: CanonicalHash::digest(source_id.as_bytes()),
                priority: 0,
                provenance: source_provenance.clone(),
            }],
            policy: CompositionPolicy {
                target: target("x86_64-pc-windows-msvc"),
                realization_order: vec![RealizationKind::Data, RealizationKind::NativeStatic],
                namespace_grants: BTreeMap::new(),
                maximum_trust: TrustClass::TrustedNative,
                allow_force_override: false,
                allow_recovery: true,
                evaluation_policy: stable_id("latticeaxiom:nickel-evaluation-policy/r0@1"),
                evaluation_limits: NickelEvaluationLimits::default(),
            },
            provenance: source_provenance,
        }
    }

    fn data_package() -> PackageSpec {
        let package = package_name("@terrenia/blocks");
        let realization_id = RealizationId::new("data")
            .unwrap_or_else(|error| panic!("fixture realization ID is invalid: {error}"));
        let realization = RealizationSpec {
            id: realization_id.clone(),
            kind: RealizationKind::Data,
            domains: BTreeSet::from([PackageDomain::Authoritative]),
            targets: BTreeSet::new(),
            required_features: BTreeSet::new(),
            artifact: ArtifactIntent::DataRoot {
                path: CanonicalLogicalPath::new("assets")
                    .unwrap_or_else(|error| panic!("fixture artifact path is invalid: {error}")),
            },
            trust: TrustClass::DataOnly,
            engine_build: None,
            registration_fragment: CanonicalHash::digest(b"registration"),
            interfaces: BTreeMap::new(),
        };
        PackageSpec {
            model_version: PACKAGE_MODEL_VERSION,
            name: package,
            version: version("0.1.0"),
            metadata: PackageMetadata {
                display_name: "Terrenia Blocks".to_owned(),
                documentation: None,
                license: "MIT".to_owned(),
            },
            features: BTreeMap::new(),
            dependencies: BTreeMap::new(),
            requires: BTreeMap::new(),
            provides: BTreeMap::new(),
            realizations: BTreeMap::from([(realization_id, realization)]),
            domains: BTreeSet::from([PackageDomain::Authoritative]),
            parameters: BTreeMap::new(),
            namespace_requests: BTreeSet::new(),
            namespace_delegations: BTreeMap::new(),
            trust: TrustClass::DataOnly,
            registration: crate::RegistrationFragment::default(),
            provenance: provenance("latticeaxiom:source/terrenia-blocks", "package.ncl"),
        }
    }

    fn provenance(source_id: &str, logical_path: &str) -> SourceProvenance {
        SourceProvenance::new(
            parse_source_id(source_id),
            logical_path,
            CanonicalHash::digest(logical_path.as_bytes()),
            Some(
                SourceSpan::new(0, 4)
                    .unwrap_or_else(|error| panic!("fixture span is invalid: {error}")),
            ),
            Vec::new(),
        )
        .unwrap_or_else(|error| panic!("fixture provenance is invalid: {error}"))
    }

    fn grant_pattern(value: &str) -> NamespaceGrantPattern {
        NamespaceGrantPattern::new(value)
            .unwrap_or_else(|error| panic!("fixture namespace pattern is invalid: {error}"))
    }

    fn stable_id(value: &str) -> StableId {
        value
            .parse()
            .unwrap_or_else(|error| panic!("fixture stable ID `{value}` is invalid: {error}"))
    }

    fn parse_source_id(value: &str) -> SourceId {
        value
            .parse()
            .unwrap_or_else(|error| panic!("fixture source ID `{value}` is invalid: {error}"))
    }

    fn capability(value: &str) -> CapabilityId {
        value
            .parse()
            .unwrap_or_else(|error| panic!("fixture capability `{value}` is invalid: {error}"))
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

    fn version_req(value: &str) -> PackageVersionReq {
        value
            .parse()
            .unwrap_or_else(|error| panic!("fixture range `{value}` is invalid: {error}"))
    }

    fn target(value: &str) -> TargetTriple {
        value
            .parse()
            .unwrap_or_else(|error| panic!("fixture target `{value}` is invalid: {error}"))
    }
}
