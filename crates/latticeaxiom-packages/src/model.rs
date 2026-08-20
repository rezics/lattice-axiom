//! Versioned resolver input, explanation, receipt, and build-intent models.

use std::collections::{BTreeMap, BTreeSet};

use latticeaxiom_compose::{
    ArtifactIntent, CapabilityCardinality, InterfaceRequirement, NickelEvaluationLimits,
    PackageDomain, PackageSpec, ProfileKind, RealizationId, RealizationKind, SourceCandidate,
    TrustClass,
};
use latticeaxiom_core::{
    CanonicalHash, CanonicalJsonError, CanonicalLogicalPath, CapabilityId, NamespaceGrant,
    NamespaceGrantPattern, NamespaceGrantorRef, PackageName, PackageVersion, PackageVersionReq,
    SchemaId, SourceId, StableId, TargetTriple, canonical_json_bytes, canonical_json_hash,
};
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Current schema version for resolver-stage resolution receipts.
pub const RESOLUTION_RECEIPT_SCHEMA_VERSION: u32 = 1;

/// Current schema version for resolver-stage build intents.
pub const BUILD_INTENT_SCHEMA_VERSION: u32 = 1;

/// Current schema version for package resolver safety limits.
pub const RESOLUTION_LIMITS_SCHEMA_VERSION: u32 = 1;

/// Finite deterministic safety limits for one resolution attempt.
///
/// These are implementation safety guards, not ADR performance budgets. They
/// bound logical work only; wall-clock time and machine-specific byte counts
/// never participate in resolution.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ResolutionLimits {
    /// Limits schema version.
    pub schema_version: u32,
    /// Maximum rows accepted in the controlled source universe.
    pub max_source_candidates: u64,
    /// Maximum compatible candidates at one package or provider decision.
    pub max_candidates_per_decision: u64,
    /// Maximum distinct normalized search states.
    pub max_unique_states: u64,
    /// Maximum transitions between normalized search states.
    pub max_state_transitions: u64,
    /// Maximum nested decision depth.
    pub max_decision_depth: u64,
    /// Maximum packages simultaneously present in one analyzed state.
    pub max_live_packages: u64,
    /// Maximum capabilities simultaneously present in one analyzed state.
    pub max_active_capabilities: u64,
    /// Maximum active dependency and capability edges in one analyzed state.
    pub max_active_edges: u64,
    /// Maximum fixed-point analysis steps across the resolution attempt.
    pub max_analysis_steps: u64,
    /// Maximum structured explanation decisions in the result.
    pub max_explanation_decisions: u64,
}

impl Default for ResolutionLimits {
    fn default() -> Self {
        Self {
            schema_version: RESOLUTION_LIMITS_SCHEMA_VERSION,
            max_source_candidates: 4_096,
            max_candidates_per_decision: 1_024,
            max_unique_states: 65_536,
            max_state_transitions: 262_144,
            max_decision_depth: 256,
            max_live_packages: 2_048,
            max_active_capabilities: 2_048,
            max_active_edges: 16_384,
            max_analysis_steps: 262_144,
            max_explanation_decisions: 262_144,
        }
    }
}

impl ResolutionLimits {
    /// Validates the limits schema and requires every bound to be positive.
    ///
    /// # Errors
    ///
    /// Returns an error for an unsupported schema or a zero bound.
    pub fn validate(self) -> Result<(), ResolutionLimitsError> {
        if self.schema_version != RESOLUTION_LIMITS_SCHEMA_VERSION {
            return Err(ResolutionLimitsError::UnsupportedSchema {
                found: self.schema_version,
                supported: RESOLUTION_LIMITS_SCHEMA_VERSION,
            });
        }
        for (budget, limit) in [
            (
                ResolutionBudget::SourceCandidates,
                self.max_source_candidates,
            ),
            (
                ResolutionBudget::CandidatesPerDecision,
                self.max_candidates_per_decision,
            ),
            (ResolutionBudget::UniqueStates, self.max_unique_states),
            (
                ResolutionBudget::StateTransitions,
                self.max_state_transitions,
            ),
            (ResolutionBudget::DecisionDepth, self.max_decision_depth),
            (ResolutionBudget::LivePackages, self.max_live_packages),
            (
                ResolutionBudget::ActiveCapabilities,
                self.max_active_capabilities,
            ),
            (ResolutionBudget::ActiveEdges, self.max_active_edges),
            (ResolutionBudget::AnalysisSteps, self.max_analysis_steps),
            (
                ResolutionBudget::ExplanationDecisions,
                self.max_explanation_decisions,
            ),
        ] {
            if limit == 0 {
                return Err(ResolutionLimitsError::ZeroLimit { budget });
            }
        }
        Ok(())
    }
}

/// Logical work counter guarded by `ResolutionLimits`.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ResolutionBudget {
    /// Controlled source-universe rows.
    SourceCandidates,
    /// Compatible candidates at one decision.
    CandidatesPerDecision,
    /// Distinct normalized search states.
    UniqueStates,
    /// Search-state transitions.
    StateTransitions,
    /// Nested decision depth.
    DecisionDepth,
    /// Packages in an analyzed state.
    LivePackages,
    /// Capabilities in an analyzed state.
    ActiveCapabilities,
    /// Active graph edges in an analyzed state.
    ActiveEdges,
    /// Fixed-point analysis steps.
    AnalysisSteps,
    /// Structured explanation decisions.
    ExplanationDecisions,
}

/// Invalid package resolver safety limits.
#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum ResolutionLimitsError {
    /// The limits schema is unsupported.
    #[error("unsupported resolution-limits schema {found}; supported schema is {supported}")]
    UnsupportedSchema {
        /// Schema version supplied by the caller.
        found: u32,
        /// Schema version supported by this crate.
        supported: u32,
    },
    /// One logical work bound is zero.
    #[error("resolution limit {budget:?} must be greater than zero")]
    ZeroLimit {
        /// Invalid logical work counter.
        budget: ResolutionBudget,
    },
}
/// Evaluated composition surfaces that require a later semantic compiler.
///
/// The R1 package-selection receipt fails closed when one of these surfaces is
/// non-empty because silently ignoring it would produce an incomplete graph.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum PreBuildSurface {
    /// Graph-affecting composition parameter values.
    CompositionParameters,
    /// Evaluated package parameter declarations or values.
    PackageParameters,
}

/// Kernel-owned source class used by the ADR-0021 tie-break.
///
/// Declaration order is the normative ascending rank after descending source
/// priority. In-memory fixtures assign the source class they emulate.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum PackageSourceKind {
    /// A package in the active workspace.
    Workspace,
    /// A package acquired from a controlled local directory.
    LocalDirectory,
    /// A package backed by a controlled local prebuilt artifact.
    LocalPrebuilt,
}

/// A source-universe row bound to its fully evaluated package manifest.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PackageCandidate {
    /// Stable acquisition class.
    pub source_kind: PackageSourceKind,
    /// Kernel-owned source identity and content receipt.
    pub source: SourceCandidate,
    /// Fully evaluated package contract.
    pub package: PackageSpec,
}

/// Stable identity of one source candidate.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CandidateIdentityV1 {
    /// Logical package name.
    pub package: PackageName,
    /// Exact package version.
    pub version: PackageVersion,
    /// Stable source identity.
    pub source_id: SourceId,
    /// Source content hash.
    pub source_hash: CanonicalHash,
    /// Kernel-owned source class.
    pub source_kind: PackageSourceKind,
    /// Explicit source priority.
    pub source_priority: i32,
}

impl From<&PackageCandidate> for CandidateIdentityV1 {
    fn from(candidate: &PackageCandidate) -> Self {
        Self {
            package: candidate.package.name.clone(),
            version: candidate.package.version.clone(),
            source_id: candidate.source.source_id.clone(),
            source_hash: candidate.source.content_hash,
            source_kind: candidate.source_kind,
            source_priority: candidate.source.priority,
        }
    }
}

/// Typed subject of a resolution explanation decision.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case", tag = "layer")]
pub enum ResolutionSubjectV1 {
    /// One package source/version candidate.
    Package {
        /// Candidate being evaluated.
        candidate: CandidateIdentityV1,
    },
    /// One realization belonging to a package candidate.
    Realization {
        /// Owning package.
        package: PackageName,
        /// Owning source candidate.
        candidate: CandidateIdentityV1,
        /// Package-local realization ID.
        realization: RealizationId,
        /// Realization family.
        kind: RealizationKind,
    },
    /// One candidate provider for a capability.
    CapabilityProvider {
        /// Required capability.
        capability: CapabilityId,
        /// Provider package candidate.
        candidate: CandidateIdentityV1,
        /// Exact provided interface version.
        provided_version: PackageVersion,
    },
}

/// Selected or discarded outcome for one explanation decision.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ResolutionOutcomeV1 {
    /// The candidate is part of the final resolution.
    Selected,
    /// The candidate was rejected or ranked below a winner.
    Discarded,
}

/// Stable typed category for a failed backtracking branch.
#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case", tag = "code")]
pub enum ResolutionFailureCodeV1 {
    /// A required logical package was absent.
    MissingPackage {
        /// Missing package.
        package: PackageName,
    },
    /// No single version satisfied all active requirements.
    VersionConflict {
        /// Conflicting package.
        package: PackageName,
    },
    /// An activated feature was not declared.
    UnknownFeature {
        /// Package declaring features.
        package: PackageName,
        /// Unknown feature.
        feature: String,
    },
    /// Active graph edges formed a cycle.
    DependencyCycle,
    /// A normalized search state recurred on the active branch.
    SearchStateCycle,
    /// No compatible capability provider existed.
    MissingCapability {
        /// Missing capability.
        capability: CapabilityId,
    },
    /// Provider count could not satisfy the capability contract.
    CapabilityCardinalityConflict {
        /// Conflicting capability.
        capability: CapabilityId,
    },
    /// Multiple providers remained for an exclusive capability.
    ConflictingCapabilityProviders {
        /// Conflicting capability.
        capability: CapabilityId,
    },
    /// No realization satisfied the active policy.
    RealizationUnavailable {
        /// Package without an eligible realization.
        package: PackageName,
    },
    /// An exact source pin was unavailable.
    PinnedSourceUnavailable {
        /// Package whose pin was unavailable.
        package: PackageName,
    },
    /// A required dynamic interface was unavailable.
    InterfaceRequirementUnavailable {
        /// Required interface.
        interface: StableId,
    },
    /// An engine-coupled realization did not match the host build.
    EngineBuildMismatch {
        /// Package requiring an exact engine build.
        package: PackageName,
    },
    /// A selected package graph could not prove its owner-qualified namespace authority.
    NamespaceAuthorization,
}

/// Stable terminal failure attached to a discarded search branch.
#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BacktrackingFailureV1 {
    /// Typed terminal failure.
    pub code: ResolutionFailureCodeV1,
    /// Deterministic root-to-failure package chain.
    pub package_chain: Vec<PackageName>,
    /// Normalized requirements active at the failure.
    pub requested: Vec<String>,
}
/// Stable reason attached to a resolution decision.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case", tag = "reason")]
pub enum ResolutionReasonV1 {
    /// The candidate was introduced by a profile root.
    Root,
    /// The candidate was introduced by an active dependency.
    Dependency {
        /// Requiring package.
        required_by: PackageName,
    },
    /// The candidate was introduced as a provider.
    Capability {
        /// Required capability.
        capability: CapabilityId,
    },
    /// The version failed one or more normalized requirements.
    VersionMismatch {
        /// Normalized requirement strings.
        requirements: Vec<String>,
    },
    /// No package or realization domains intersect the active projection.
    DomainMismatch,
    /// Requested feature names are not declared.
    UnknownFeatures {
        /// Unknown feature names.
        features: BTreeSet<String>,
    },
    /// A trust requirement exceeds profile policy.
    TrustExceeded {
        /// Required trust.
        required: TrustClass,
        /// Maximum accepted trust.
        allowed: TrustClass,
    },
    /// A realization does not support the selected target.
    TargetMismatch {
        /// Selected target.
        target: TargetTriple,
    },
    /// An explicit realization family excluded this candidate.
    ExplicitKindMismatch {
        /// Required realization family.
        required: RealizationKind,
    },
    /// The automatic realization policy does not contain this family.
    AutomaticKindDisabled,
    /// Capability cardinality contracts disagree.
    CardinalityMismatch {
        /// Cardinality required by consumers.
        required: CapabilityCardinality,
        /// Cardinality declared by the provider.
        provided: CapabilityCardinality,
    },
    /// A capability interface version failed active requirements.
    InterfaceVersionMismatch {
        /// Normalized interface version requirements.
        requirements: Vec<String>,
    },
    /// A higher-ranked branch was tried and ended in a typed failure.
    Backtracked {
        /// Stable terminal failure reached below this decision.
        failure: BacktrackingFailureV1,
    },
    /// No realization belonging to this package candidate was eligible.
    NoEligibleRealization,
    /// The candidate did not match all exact source pins.
    PinnedSourceMismatch {
        /// Required source identities.
        required: BTreeSet<SourceId>,
    },
    /// A mandatory dynamic interface requirement could not be satisfied.
    InterfaceRequirementUnavailable {
        /// Required interface.
        interface: StableId,
        /// Normalized acceptable interface versions.
        requirements: Vec<String>,
    },
    /// An engine-coupled realization did not match the available host build.
    EngineBuildMismatch {
        /// Exact engine build required by the realization.
        required: CanonicalHash,
        /// Available host build, or none when the host environment is unknown.
        available: Option<CanonicalHash>,
    },
    /// The provider package was not selected into the closure.
    PackageNotSelected,
    /// The candidate was compatible but ranked below a selected candidate.
    LowerPreference,
}

/// One structured and deterministic resolution explanation row.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ResolutionDecisionV1 {
    /// Package, realization, or provider being evaluated.
    pub subject: ResolutionSubjectV1,
    /// Final selected or discarded outcome.
    pub outcome: ResolutionOutcomeV1,
    /// Stable ordered reasons for the outcome.
    pub reasons: Vec<ResolutionReasonV1>,
}

/// Deterministic explanation of the evaluated decision frontier.
///
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ResolutionExplanationV1 {
    /// Decisions sorted by layer and the corresponding normative tie-break.
    pub decisions: Vec<ResolutionDecisionV1>,
}

/// One exact active dependency edge.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ResolvedDependencyV1 {
    /// Exact selected dependency version.
    pub version: PackageVersion,
    /// Features forwarded by this edge.
    pub features: BTreeSet<String>,
}

/// Selected realization and its pre-build artifact intent.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ResolvedRealizationV1 {
    /// Package-local realization ID.
    pub id: RealizationId,
    /// Realization family.
    pub kind: RealizationKind,
    /// Exact selected target.
    pub target: TargetTriple,
    /// Source-build, local-prebuilt, or data-root intent.
    pub artifact: ArtifactIntent,
    /// Dynamic interface requirements.
    pub interfaces: BTreeMap<StableId, InterfaceRequirement>,
    /// Trust required by this realization.
    pub trust: TrustClass,
    /// Exact engine build for an engine-coupled realization.
    pub engine_build_id: Option<CanonicalHash>,
    /// Expected registration-fragment identity.
    pub registration_fragment: CanonicalHash,
}

/// One exact selected package node before artifact production.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ResolvedPackageV1 {
    /// Exact package version.
    pub version: PackageVersion,
    /// Selected source class.
    pub source_kind: PackageSourceKind,
    /// Explicit source priority used by deterministic selection.
    pub source_priority: i32,
    /// Stable selected source ID.
    pub source_id: SourceId,
    /// Exact normalized source content hash.
    pub source_hash: CanonicalHash,
    /// Source-declaration provenance receipt.
    pub source_provenance_hash: CanonicalHash,
    /// Fully evaluated package provenance receipt.
    pub package_provenance_hash: CanonicalHash,
    /// Canonical logical source path retained for diagnostics.
    pub source_path: CanonicalLogicalPath,
    /// Activated package features.
    pub features: BTreeSet<String>,
    /// Activated package domains.
    pub domains: BTreeSet<PackageDomain>,
    /// Exact active dependency edges.
    pub dependencies: BTreeMap<PackageName, ResolvedDependencyV1>,
    /// Complete registration patterns requested from incoming authority.
    pub namespace_requests: BTreeSet<NamespaceGrantPattern>,
    /// Persistent schemas declared by this package.
    pub schemas: BTreeSet<SchemaId>,
    /// Selected realization and pre-build artifact intent.
    pub realization: ResolvedRealizationV1,
}

/// One active capability demand captured by a resolution receipt.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CapabilityDemandReceiptV1 {
    /// Requiring package, or none for a profile-level demand.
    pub required_by: Option<PackageName>,
    /// Accepted interface-version range.
    pub range: PackageVersionReq,
    /// Explicit provider package, when requested.
    pub explicit_provider: Option<PackageName>,
    /// Required provider cardinality.
    pub cardinality: CapabilityCardinality,
    /// Projection domains in which this demand is active.
    pub domains: BTreeSet<PackageDomain>,
}

/// One selected capability provider captured by a resolution receipt.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CapabilityProviderReceiptV1 {
    /// Selected provider package.
    pub package: PackageName,
    /// Exact provided interface version.
    pub provided_version: PackageVersion,
    /// Cardinality declared by the provider.
    pub cardinality: CapabilityCardinality,
}

/// Active demands and selected providers for one capability.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CapabilityResolutionReceiptV1 {
    /// Active demands in deterministic order.
    pub demands: Vec<CapabilityDemandReceiptV1>,
    /// Selected providers in deterministic execution order.
    pub providers: Vec<CapabilityProviderReceiptV1>,
}

/// Versioned deterministic resolver-stage receipt.
///
/// This is an intermediate selection receipt used for conformance and exact
/// selection replay. It is not `latticeaxiom.lock`, `LockedGameGraph`, `BuildPlan`,
/// or a world frozen-lock receipt. It contains pre-build artifact intent but no
/// produced or verified artifact identity and must not be persisted as
/// authoritative runtime or world state.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ResolutionReceiptV1 {
    /// Receipt schema version.
    pub schema_version: u32,
    /// Semantic identity of the complete evaluated composition.
    pub composition_hash: CanonicalHash,
    /// Provenance identity of the complete evaluated composition.
    pub composition_provenance_hash: CanonicalHash,
    /// Semantic resolution intent excluding the mutable source universe.
    pub resolution_intent_hash: CanonicalHash,
    /// Profile stable ID.
    pub profile: StableId,
    /// Profile projection.
    pub profile_kind: ProfileKind,
    /// Exact target.
    pub target: TargetTriple,
    /// Versioned Nickel evaluation policy.
    pub evaluation_policy: StableId,
    /// Exact effective evaluator limits.
    pub evaluation_limits: NickelEvaluationLimits,
    /// Root package identities.
    pub roots: BTreeSet<PackageName>,
    /// Canonical owner-qualified namespace authorization chain.
    pub namespace_grants: BTreeSet<NamespaceGrant>,
    /// Exact selected package closure.
    pub packages: BTreeMap<PackageName, ResolvedPackageV1>,
    /// Capability demands and providers keyed by capability.
    pub capabilities: BTreeMap<CapabilityId, CapabilityResolutionReceiptV1>,
    /// Structured decisions for the evaluated resolution frontier.
    pub explanation: ResolutionExplanationV1,
    /// Semantic selected-resolution hash.
    pub resolution_hash: CanonicalHash,
    /// Exact receipt hash.
    pub receipt_hash: CanonicalHash,
}

impl ResolutionReceiptV1 {
    /// Encodes this receipt using canonical JSON.
    ///
    /// # Errors
    ///
    /// Returns an error when JSON encoding or canonicalization fails.
    pub fn canonical_bytes(&self) -> Result<Vec<u8>, CanonicalJsonError> {
        canonical_json_bytes(self)
    }

    /// Recomputes the semantic selected-resolution hash.
    ///
    /// # Errors
    ///
    /// Returns an error when the semantic identity cannot be encoded.
    pub fn recompute_resolution_hash(&self) -> Result<CanonicalHash, CanonicalJsonError> {
        canonical_json_hash(&ResolutionIdentity::from(self))
    }

    /// Recomputes the exact receipt hash.
    ///
    /// # Errors
    ///
    /// Returns an error when the receipt identity cannot be encoded.
    pub fn recompute_receipt_hash(&self) -> Result<CanonicalHash, CanonicalJsonError> {
        canonical_json_hash(&ReceiptIdentity::from(self))
    }

    /// Validates schema, structural references, and both stored hash claims.
    ///
    /// # Errors
    ///
    /// Returns an error for an unsupported schema, malformed structure, or a
    /// mismatched hash.
    pub fn validate(&self) -> Result<(), ResolutionReceiptError> {
        if self.schema_version != RESOLUTION_RECEIPT_SCHEMA_VERSION {
            return Err(ResolutionReceiptError::UnsupportedSchema {
                found: self.schema_version,
                supported: RESOLUTION_RECEIPT_SCHEMA_VERSION,
            });
        }
        self.validate_structure()?;

        let resolution_hash = self.recompute_resolution_hash()?;
        if resolution_hash != self.resolution_hash {
            return Err(ResolutionReceiptError::ResolutionHashMismatch {
                expected: self.resolution_hash,
                actual: resolution_hash,
            });
        }
        let receipt_hash = self.recompute_receipt_hash()?;
        if receipt_hash != self.receipt_hash {
            return Err(ResolutionReceiptError::ReceiptHashMismatch {
                expected: self.receipt_hash,
                actual: receipt_hash,
            });
        }
        Ok(())
    }

    /// Decodes canonical JSON and validates the resulting receipt.
    ///
    /// # Errors
    ///
    /// Returns an error for malformed or non-canonical JSON, an unsupported
    /// schema, malformed structure, or a mismatched hash.
    pub fn from_json_slice(bytes: &[u8]) -> Result<Self, ResolutionReceiptError> {
        let receipt = serde_json::from_slice::<Self>(bytes)?;
        if receipt.canonical_bytes()?.as_slice() != bytes {
            return Err(ResolutionReceiptError::NonCanonicalEncoding);
        }
        receipt.validate()?;
        Ok(receipt)
    }

    fn validate_structure(&self) -> Result<(), ResolutionReceiptError> {
        self.validate_package_structure()?;
        self.validate_capability_structure()?;
        self.validate_namespace_structure()?;
        self.validate_explanation_structure()
    }

    fn validate_package_structure(&self) -> Result<(), ResolutionReceiptError> {
        if self.roots.is_empty() {
            return Err(invalid_receipt(
                "resolution receipt root set must not be empty".to_owned(),
            ));
        }
        let mut source_ids = BTreeSet::new();
        let mut schema_owners = BTreeMap::<SchemaId, PackageName>::new();

        for root in &self.roots {
            if !self.packages.contains_key(root) {
                return Err(invalid_receipt(format!(
                    "root package {root} is absent from the selected closure"
                )));
            }
        }

        for (name, package) in &self.packages {
            if !source_ids.insert(package.source_id.clone()) {
                return Err(invalid_receipt(format!(
                    "selected source {} is reused by multiple packages",
                    package.source_id
                )));
            }
            if package.realization.target != self.target {
                return Err(invalid_receipt(format!(
                    "package {name} realization target {} differs from receipt target {}",
                    package.realization.target, self.target
                )));
            }
            for schema in &package.schemas {
                if let Some(owner) = schema_owners.insert(schema.clone(), name.clone()) {
                    return Err(invalid_receipt(format!(
                        "schema {schema} is owned by both {owner} and {name}"
                    )));
                }
            }
            for (dependency_name, dependency) in &package.dependencies {
                let Some(selected) = self.packages.get(dependency_name) else {
                    return Err(invalid_receipt(format!(
                        "package {name} depends on absent package {dependency_name}"
                    )));
                };
                if dependency.version != selected.version {
                    return Err(invalid_receipt(format!(
                        "package {name} records dependency {dependency_name} at {}, selected version is {}",
                        dependency.version, selected.version
                    )));
                }
            }
        }
        Ok(())
    }

    fn validate_capability_structure(&self) -> Result<(), ResolutionReceiptError> {
        for (capability, resolution) in &self.capabilities {
            let mut providers = BTreeSet::new();
            for provider in &resolution.providers {
                if !providers.insert(provider.package.clone()) {
                    return Err(invalid_receipt(format!(
                        "capability {capability} repeats provider {}",
                        provider.package
                    )));
                }
                if !self.packages.contains_key(&provider.package) {
                    return Err(invalid_receipt(format!(
                        "capability {capability} selects absent provider {}",
                        provider.package
                    )));
                }
            }

            for demand in &resolution.demands {
                if let Some(required_by) = &demand.required_by
                    && !self.packages.contains_key(required_by)
                {
                    return Err(invalid_receipt(format!(
                        "capability {capability} has demand from absent package {required_by}"
                    )));
                }

                let provider_count = resolution.providers.len();
                let cardinality_satisfied = match demand.cardinality {
                    CapabilityCardinality::ExactlyOne => provider_count == 1,
                    CapabilityCardinality::OneOrMore => provider_count >= 1,
                    CapabilityCardinality::ZeroOrMore => true,
                };
                if !cardinality_satisfied {
                    return Err(invalid_receipt(format!(
                        "capability {capability} provider count {provider_count} violates {:?}",
                        demand.cardinality
                    )));
                }

                for provider in &resolution.providers {
                    validate_provider_for_demand(capability, demand, provider)?;
                }
            }
        }
        Ok(())
    }
    #[allow(clippy::too_many_lines)]
    fn validate_namespace_structure(&self) -> Result<(), ResolutionReceiptError> {
        let mut effective = self
            .packages
            .keys()
            .cloned()
            .map(|package| (package, BTreeSet::<NamespaceGrantPattern>::new()))
            .collect::<BTreeMap<_, _>>();
        let mut seen_rows = BTreeSet::new();
        let mut delegations = Vec::new();

        for grant in &self.namespace_grants {
            if !self.packages.contains_key(grant.grantee()) {
                return Err(invalid_receipt(format!(
                    "namespace grant selects absent grantee {}",
                    grant.grantee()
                )));
            }
            let row_key = (
                grant.grantor().clone(),
                grant.grantee().clone(),
                grant.namespace().clone(),
            );
            if !seen_rows.insert(row_key) {
                return Err(invalid_receipt(format!(
                    "namespace grant row for {} and {} is not canonically grouped",
                    grant.grantee(),
                    grant.namespace()
                )));
            }

            match grant.grantor().as_ref() {
                NamespaceGrantorRef::Profile { profile } => {
                    if profile != &self.profile || !self.roots.contains(grant.grantee()) {
                        return Err(invalid_receipt(format!(
                            "profile namespace grant to {} is not bound to this profile root set",
                            grant.grantee()
                        )));
                    }
                    let Some(patterns) = effective.get_mut(grant.grantee()) else {
                        return Err(invalid_receipt(format!(
                            "profile namespace grant selects absent grantee {}",
                            grant.grantee()
                        )));
                    };
                    patterns.extend(grant.patterns().iter().cloned());
                }
                NamespaceGrantorRef::Package { package } => {
                    let Some(grantor) = self.packages.get(package) else {
                        return Err(invalid_receipt(format!(
                            "namespace grantor package {package} is absent"
                        )));
                    };
                    if !grantor.dependencies.contains_key(grant.grantee()) {
                        return Err(invalid_receipt(format!(
                            "namespace grant from {package} to {} is not an active direct dependency edge",
                            grant.grantee()
                        )));
                    }
                    if let Some(pattern) = grant.patterns().iter().find(|pattern| {
                        !grantor
                            .namespace_requests
                            .iter()
                            .any(|request| request.covers(pattern))
                    }) {
                        return Err(invalid_receipt(format!(
                            "namespace delegation from {package} to {} widens beyond declared request {pattern}",
                            grant.grantee()
                        )));
                    }
                    delegations.push(grant);
                }
            }
        }

        let mut pending = delegations;
        while !pending.is_empty() {
            let mut next = Vec::new();
            let mut progress = false;
            for grant in pending {
                let NamespaceGrantorRef::Package { package } = grant.grantor().as_ref() else {
                    continue;
                };
                let Some(parent_patterns) = effective.get(package) else {
                    return Err(invalid_receipt(format!(
                        "namespace grantor package {package} is absent"
                    )));
                };
                if grant
                    .patterns()
                    .iter()
                    .all(|child| parent_patterns.iter().any(|parent| parent.covers(child)))
                {
                    let Some(child_patterns) = effective.get_mut(grant.grantee()) else {
                        return Err(invalid_receipt(format!(
                            "namespace delegation selects absent grantee {}",
                            grant.grantee()
                        )));
                    };
                    child_patterns.extend(grant.patterns().iter().cloned());
                    progress = true;
                } else {
                    next.push(grant);
                }
            }
            if !progress {
                let Some(grant) = next.first() else {
                    return Err(invalid_receipt(
                        "namespace delegation fixed point made no progress".to_owned(),
                    ));
                };
                let grantor = match grant.grantor().as_ref() {
                    NamespaceGrantorRef::Package { package } => package,
                    NamespaceGrantorRef::Profile { .. } => grant.grantee(),
                };
                return Err(invalid_receipt(format!(
                    "namespace delegation from {grantor} to {} is not covered by effective authority",
                    grant.grantee()
                )));
            }
            pending = next;
        }

        for (package, resolved) in &self.packages {
            let Some(patterns) = effective.get(package) else {
                return Err(invalid_receipt(format!(
                    "selected package {package} has no namespace authority entry"
                )));
            };
            if let Some(request) = resolved
                .namespace_requests
                .iter()
                .find(|request| !patterns.iter().any(|grant| grant.covers(request)))
            {
                return Err(invalid_receipt(format!(
                    "namespace request {request} from package {package} is not covered",
                )));
            }
        }
        Ok(())
    }
}

impl ResolutionReceiptV1 {
    #[allow(clippy::too_many_lines)]
    fn validate_explanation_structure(&self) -> Result<(), ResolutionReceiptError> {
        if self.explanation.decisions.is_empty() {
            return Err(invalid_receipt(
                "resolution explanation must not be empty".to_owned(),
            ));
        }

        let selected_sources = self
            .packages
            .iter()
            .map(|(name, package)| (package.source_id.clone(), (name, package)))
            .collect::<BTreeMap<_, _>>();
        let mut candidate_identities = BTreeMap::<SourceId, CandidateIdentityV1>::new();
        let mut selected_packages = BTreeSet::new();
        let mut selected_realizations = BTreeSet::new();
        let mut selected_roots = BTreeSet::new();
        let mut package_decision_sources = BTreeSet::new();
        let mut realization_decision_sources = Vec::new();
        let mut selected_providers = Vec::new();

        for (index, decision) in self.explanation.decisions.iter().enumerate() {
            if self.explanation.decisions[..index]
                .iter()
                .any(|previous| previous.subject == decision.subject)
            {
                return Err(invalid_receipt(format!(
                    "resolution explanation repeats subject {:?}",
                    decision.subject
                )));
            }
            if decision
                .reasons
                .iter()
                .enumerate()
                .any(|(reason_index, reason)| decision.reasons[..reason_index].contains(reason))
            {
                return Err(invalid_receipt(format!(
                    "resolution explanation subject {:?} repeats a reason",
                    decision.subject
                )));
            }
            if decision.outcome == ResolutionOutcomeV1::Discarded && decision.reasons.is_empty() {
                return Err(invalid_receipt(format!(
                    "discarded explanation subject {:?} has no reason",
                    decision.subject
                )));
            }

            let candidate = explanation_candidate(&decision.subject);
            if let Some(existing) = candidate_identities.get(&candidate.source_id) {
                if existing != candidate {
                    return Err(invalid_receipt(format!(
                        "source {} has inconsistent candidate identities in the explanation",
                        candidate.source_id
                    )));
                }
            } else {
                candidate_identities.insert(candidate.source_id.clone(), candidate.clone());
            }
            if let Some((selected_name, selected)) = selected_sources.get(&candidate.source_id)
                && !candidate_matches_resolved(selected_name, selected, candidate)
            {
                return Err(invalid_receipt(format!(
                    "explanation identity for selected source {} differs from package {selected_name}",
                    candidate.source_id
                )));
            }

            match &decision.subject {
                ResolutionSubjectV1::Package { candidate } => {
                    package_decision_sources.insert(candidate.source_id.clone());
                    let Some(package) = self.packages.get(&candidate.package) else {
                        return Err(invalid_receipt(format!(
                            "package explanation references logical package {} outside the selected closure",
                            candidate.package
                        )));
                    };
                    let matches_selected =
                        candidate_matches_resolved(&candidate.package, package, candidate);
                    match decision.outcome {
                        ResolutionOutcomeV1::Selected => {
                            if !matches_selected {
                                return Err(invalid_receipt(format!(
                                    "selected package explanation for {} differs from its receipt",
                                    candidate.package
                                )));
                            }
                            if !selected_packages.insert(candidate.package.clone()) {
                                return Err(invalid_receipt(format!(
                                    "package {} has multiple selected explanation rows",
                                    candidate.package
                                )));
                            }
                            self.validate_selected_package_reasons(
                                &candidate.package,
                                &decision.reasons,
                                &mut selected_roots,
                            )?;
                        }
                        ResolutionOutcomeV1::Discarded => {
                            if matches_selected {
                                return Err(invalid_receipt(format!(
                                    "selected package {} is marked discarded in the explanation",
                                    candidate.package
                                )));
                            }
                            validate_discarded_package_reasons(&decision.reasons)?;
                        }
                    }
                }
                ResolutionSubjectV1::Realization {
                    package,
                    candidate,
                    realization,
                    kind,
                } => {
                    if package != &candidate.package {
                        return Err(invalid_receipt(format!(
                            "realization explanation package {package} differs from candidate {}",
                            candidate.package
                        )));
                    }
                    let Some(selected_package) = self.packages.get(package) else {
                        return Err(invalid_receipt(format!(
                            "realization explanation references package {package} outside the selected closure"
                        )));
                    };
                    realization_decision_sources.push(candidate.source_id.clone());
                    let selected_candidate =
                        candidate_matches_resolved(package, selected_package, candidate);
                    let selected_id = realization == &selected_package.realization.id;
                    let matches_selected = selected_candidate
                        && selected_id
                        && kind == &selected_package.realization.kind;
                    if selected_candidate && selected_id && !matches_selected {
                        return Err(invalid_receipt(format!(
                            "realization {realization} for package {package} has a kind inconsistent with its receipt"
                        )));
                    }
                    match decision.outcome {
                        ResolutionOutcomeV1::Selected => {
                            if !matches_selected || !decision.reasons.is_empty() {
                                return Err(invalid_receipt(format!(
                                    "selected realization explanation for package {package} differs from its receipt"
                                )));
                            }
                            if !selected_realizations.insert(package.clone()) {
                                return Err(invalid_receipt(format!(
                                    "package {package} has multiple selected realization rows"
                                )));
                            }
                        }
                        ResolutionOutcomeV1::Discarded => {
                            if matches_selected {
                                return Err(invalid_receipt(format!(
                                    "selected realization {realization} for package {package} is marked discarded"
                                )));
                            }
                            validate_discarded_realization_reasons(&decision.reasons)?;
                        }
                    }
                }
                ResolutionSubjectV1::CapabilityProvider {
                    capability,
                    candidate,
                    provided_version,
                } => {
                    let Some(resolution) = self.capabilities.get(capability) else {
                        return Err(invalid_receipt(format!(
                            "provider explanation references inactive capability {capability}"
                        )));
                    };
                    let matches_selected =
                        self.packages
                            .get(&candidate.package)
                            .is_some_and(|package| {
                                candidate_matches_resolved(&candidate.package, package, candidate)
                                    && resolution.providers.iter().any(|provider| {
                                        provider.package == candidate.package
                                            && provider.provided_version == *provided_version
                                    })
                            });
                    match decision.outcome {
                        ResolutionOutcomeV1::Selected => {
                            if !matches_selected || !decision.reasons.is_empty() {
                                return Err(invalid_receipt(format!(
                                    "selected provider explanation for capability {capability} differs from its receipt"
                                )));
                            }
                            selected_providers.push((
                                capability.clone(),
                                candidate.package.clone(),
                                provided_version.clone(),
                            ));
                        }
                        ResolutionOutcomeV1::Discarded => {
                            if matches_selected {
                                return Err(invalid_receipt(format!(
                                    "selected provider {} for capability {capability} is marked discarded",
                                    candidate.package
                                )));
                            }
                            validate_discarded_provider_reasons(&decision.reasons)?;
                        }
                    }
                }
            }
        }

        let expected_packages = self.packages.keys().cloned().collect::<BTreeSet<_>>();
        if selected_packages != expected_packages {
            return Err(invalid_receipt(
                "selected package explanation rows do not exactly match the package closure"
                    .to_owned(),
            ));
        }
        if selected_realizations != expected_packages {
            return Err(invalid_receipt(
                "selected realization explanation rows do not exactly match the package closure"
                    .to_owned(),
            ));
        }
        if selected_roots != self.roots {
            return Err(invalid_receipt(
                "root package set does not exactly match selected explanation root reasons"
                    .to_owned(),
            ));
        }
        if let Some(source) = realization_decision_sources
            .iter()
            .find(|source| !package_decision_sources.contains(*source))
        {
            return Err(invalid_receipt(format!(
                "realization explanation for source {source} has no matching package decision"
            )));
        }

        let expected_providers = self
            .capabilities
            .iter()
            .flat_map(|(capability, resolution)| {
                resolution.providers.iter().map(|provider| {
                    (
                        capability.clone(),
                        provider.package.clone(),
                        provider.provided_version.clone(),
                    )
                })
            })
            .collect::<Vec<_>>();
        if selected_providers.len() != expected_providers.len()
            || selected_providers
                .iter()
                .any(|provider| !expected_providers.contains(provider))
            || expected_providers
                .iter()
                .any(|provider| !selected_providers.contains(provider))
        {
            return Err(invalid_receipt(
                "selected provider explanation rows do not exactly match capability receipts"
                    .to_owned(),
            ));
        }
        Ok(())
    }

    fn validate_selected_package_reasons(
        &self,
        package: &PackageName,
        reasons: &[ResolutionReasonV1],
        selected_roots: &mut BTreeSet<PackageName>,
    ) -> Result<(), ResolutionReceiptError> {
        if reasons.is_empty() {
            return Err(invalid_receipt(format!(
                "selected package {package} has no introduction reason"
            )));
        }
        for reason in reasons {
            match reason {
                ResolutionReasonV1::Root => {
                    selected_roots.insert(package.clone());
                }
                ResolutionReasonV1::Dependency { required_by } => {
                    if !self
                        .packages
                        .get(required_by)
                        .is_some_and(|dependency| dependency.dependencies.contains_key(package))
                    {
                        return Err(invalid_receipt(format!(
                            "selected package {package} claims a nonexistent dependency edge from {required_by}"
                        )));
                    }
                }
                ResolutionReasonV1::Capability { capability } => {
                    if !self.capabilities.get(capability).is_some_and(|resolution| {
                        resolution
                            .providers
                            .iter()
                            .any(|provider| &provider.package == package)
                    }) {
                        return Err(invalid_receipt(format!(
                            "selected package {package} claims a nonexistent provider edge for {capability}"
                        )));
                    }
                }
                _ => {
                    return Err(invalid_receipt(format!(
                        "selected package {package} contains a rejection reason"
                    )));
                }
            }
        }
        Ok(())
    }
}

/// One exact pre-build input derived from a selected package.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PackageBuildIntentV1 {
    /// Exact package version.
    pub version: PackageVersion,
    /// Selected source identity.
    pub source_id: SourceId,
    /// Exact source content hash.
    pub source_hash: CanonicalHash,
    /// Activated features.
    pub features: BTreeSet<String>,
    /// Selected realization and artifact intent.
    pub realization: ResolvedRealizationV1,
}

/// Versioned resolver-stage pre-build intent.
///
/// This is not the authoritative `latticeaxiom_compose::BuildPlan`. It has no
/// toolchain, executable build unit, generated shared-schema unit, expected
/// produced artifact hash, or verified artifact identity.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BuildIntentV1 {
    /// Build-intent schema version.
    pub schema_version: u32,
    /// Exact resolution receipt consumed by this intent.
    pub resolution_receipt_hash: CanonicalHash,
    /// Exact build target.
    pub target: TargetTriple,
    /// Owner-qualified namespace authorization rows required during registration compilation.
    pub namespace_grants: BTreeSet<NamespaceGrant>,
    /// Pre-build inputs keyed by package name.
    pub packages: BTreeMap<PackageName, PackageBuildIntentV1>,
    /// Canonical build-intent hash.
    pub build_intent_hash: CanonicalHash,
}

impl BuildIntentV1 {
    /// Recomputes the build-intent hash.
    ///
    /// # Errors
    ///
    /// Returns an error when the build intent cannot be encoded.
    pub fn recompute_hash(&self) -> Result<CanonicalHash, CanonicalJsonError> {
        canonical_json_hash(&BuildIntentIdentity::from(self))
    }

    /// Encodes this build intent using canonical JSON.
    ///
    /// # Errors
    ///
    /// Returns an error when JSON encoding or canonicalization fails.
    pub fn canonical_bytes(&self) -> Result<Vec<u8>, CanonicalJsonError> {
        canonical_json_bytes(self)
    }

    /// Verifies schema, package targets, and the stored build-intent hash.
    ///
    /// # Errors
    ///
    /// Returns an error for an unsupported schema, malformed structure, or a
    /// mismatched hash.
    pub fn verify(&self) -> Result<(), BuildIntentError> {
        if self.schema_version != BUILD_INTENT_SCHEMA_VERSION {
            return Err(BuildIntentError::UnsupportedSchema {
                found: self.schema_version,
                supported: BUILD_INTENT_SCHEMA_VERSION,
            });
        }
        for (name, package) in &self.packages {
            if package.realization.target != self.target {
                return Err(BuildIntentError::InvalidStructure {
                    reason: format!(
                        "package {name} realization target {} differs from build target {}",
                        package.realization.target, self.target
                    ),
                });
            }
        }
        let actual = self.recompute_hash()?;
        for grant in &self.namespace_grants {
            if !self.packages.contains_key(grant.grantee()) {
                return Err(BuildIntentError::InvalidStructure {
                    reason: format!("namespace grant selects absent grantee {}", grant.grantee()),
                });
            }
            if let NamespaceGrantorRef::Package { package } = grant.grantor().as_ref()
                && !self.packages.contains_key(package)
            {
                return Err(BuildIntentError::InvalidStructure {
                    reason: format!("namespace grantor package {package} is absent"),
                });
            }
        }
        if actual != self.build_intent_hash {
            return Err(BuildIntentError::HashMismatch {
                expected: self.build_intent_hash,
                actual,
            });
        }
        Ok(())
    }

    /// Verifies that this build intent exactly mirrors a resolution receipt.
    ///
    /// # Errors
    ///
    /// Returns an error when either object is invalid or their receipt, target,
    /// package set, or per-package pre-build inputs differ.
    pub fn verify_against(&self, receipt: &ResolutionReceiptV1) -> Result<(), BuildIntentError> {
        self.verify()?;
        receipt
            .validate()
            .map_err(|source| BuildIntentError::InvalidReceipt {
                source: Box::new(source),
            })?;
        if self.resolution_receipt_hash != receipt.receipt_hash {
            return Err(BuildIntentError::ReceiptMismatch {
                reason: "resolution receipt hash differs".to_owned(),
            });
        }
        if self.target != receipt.target {
            return Err(BuildIntentError::ReceiptMismatch {
                reason: "build target differs from the resolution receipt".to_owned(),
            });
        }
        if self.namespace_grants != receipt.namespace_grants {
            return Err(BuildIntentError::ReceiptMismatch {
                reason: "namespace grant chains differ".to_owned(),
            });
        }
        if self.packages.len() != receipt.packages.len() {
            return Err(BuildIntentError::ReceiptMismatch {
                reason: "selected package sets differ".to_owned(),
            });
        }
        for (name, package) in &receipt.packages {
            let Some(intent) = self.packages.get(name) else {
                return Err(BuildIntentError::ReceiptMismatch {
                    reason: format!("build intent omits selected package {name}"),
                });
            };
            let expected = PackageBuildIntentV1 {
                version: package.version.clone(),
                source_id: package.source_id.clone(),
                source_hash: package.source_hash,
                features: package.features.clone(),
                realization: package.realization.clone(),
            };
            if intent != &expected {
                return Err(BuildIntentError::ReceiptMismatch {
                    reason: format!("build intent for package {name} differs from its receipt"),
                });
            }
        }
        Ok(())
    }

    /// Decodes canonical JSON and verifies the resulting build intent.
    ///
    /// # Errors
    ///
    /// Returns an error for malformed or non-canonical JSON, an unsupported
    /// schema, malformed structure, or a mismatched hash.
    pub fn from_json_slice(bytes: &[u8]) -> Result<Self, BuildIntentError> {
        let intent = serde_json::from_slice::<Self>(bytes)?;
        if intent.canonical_bytes()?.as_slice() != bytes {
            return Err(BuildIntentError::NonCanonicalEncoding);
        }
        intent.verify()?;
        Ok(intent)
    }
}

/// Complete deterministic resolver-stage output.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PackageResolutionV1 {
    /// Deterministic selection receipt.
    pub receipt: ResolutionReceiptV1,
    /// Matching pre-build artifact intent.
    pub build_intent: BuildIntentV1,
}

/// A malformed or unverifiable resolver-stage receipt.
#[derive(Debug, Error)]
pub enum ResolutionReceiptError {
    /// JSON decoding failed.
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    /// Canonical encoding failed.
    #[error(transparent)]
    Canonical(#[from] CanonicalJsonError),
    /// JSON input was valid but not the unique canonical encoding.
    #[error("resolution receipt JSON is not canonical")]
    NonCanonicalEncoding,
    /// The schema version is unsupported.
    #[error("unsupported resolution receipt schema {found}; supported schema is {supported}")]
    UnsupportedSchema {
        /// Schema version found in the receipt.
        found: u32,
        /// Schema version supported by this crate.
        supported: u32,
    },
    /// Cross-reference or cardinality validation failed.
    #[error("invalid resolution receipt structure: {reason}")]
    InvalidStructure {
        /// Stable structural failure description.
        reason: String,
    },
    /// The semantic resolution hash differs from its payload.
    #[error("resolution hash mismatch: expected {expected}, recomputed {actual}")]
    ResolutionHashMismatch {
        /// Stored resolution hash.
        expected: CanonicalHash,
        /// Recomputed resolution hash.
        actual: CanonicalHash,
    },
    /// The exact receipt hash differs from its payload.
    #[error("receipt hash mismatch: expected {expected}, recomputed {actual}")]
    ReceiptHashMismatch {
        /// Stored receipt hash.
        expected: CanonicalHash,
        /// Recomputed receipt hash.
        actual: CanonicalHash,
    },
}

/// A malformed, unverifiable, or mismatched resolver-stage build intent.
#[derive(Debug, Error)]
pub enum BuildIntentError {
    /// JSON decoding failed.
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    /// Canonical encoding failed.
    #[error(transparent)]
    Canonical(#[from] CanonicalJsonError),
    /// JSON input was valid but not the unique canonical encoding.
    #[error("build-intent JSON is not canonical")]
    NonCanonicalEncoding,
    /// The schema version is unsupported.
    #[error("unsupported build-intent schema {found}; supported schema is {supported}")]
    UnsupportedSchema {
        /// Schema version found in the intent.
        found: u32,
        /// Schema version supported by this crate.
        supported: u32,
    },
    /// Build-intent structure is internally inconsistent.
    #[error("invalid build-intent structure: {reason}")]
    InvalidStructure {
        /// Stable structural failure description.
        reason: String,
    },
    /// The stored build-intent hash differs from its payload.
    #[error("build-intent hash mismatch: expected {expected}, recomputed {actual}")]
    HashMismatch {
        /// Stored build-intent hash.
        expected: CanonicalHash,
        /// Recomputed build-intent hash.
        actual: CanonicalHash,
    },
    /// The referenced resolution receipt is invalid.
    #[error("invalid referenced resolution receipt: {source}")]
    InvalidReceipt {
        /// Receipt validation failure.
        #[source]
        source: Box<ResolutionReceiptError>,
    },
    /// The build intent does not exactly mirror its receipt.
    #[error("build intent differs from its resolution receipt: {reason}")]
    ReceiptMismatch {
        /// Exact mismatch.
        reason: String,
    },
}

fn explanation_candidate(subject: &ResolutionSubjectV1) -> &CandidateIdentityV1 {
    match subject {
        ResolutionSubjectV1::Package { candidate }
        | ResolutionSubjectV1::Realization { candidate, .. }
        | ResolutionSubjectV1::CapabilityProvider { candidate, .. } => candidate,
    }
}

fn candidate_matches_resolved(
    name: &PackageName,
    package: &ResolvedPackageV1,
    candidate: &CandidateIdentityV1,
) -> bool {
    candidate.package == *name
        && candidate.version == package.version
        && candidate.source_id == package.source_id
        && candidate.source_hash == package.source_hash
        && candidate.source_kind == package.source_kind
        && candidate.source_priority == package.source_priority
}

fn validate_discarded_package_reasons(
    reasons: &[ResolutionReasonV1],
) -> Result<(), ResolutionReceiptError> {
    validate_lower_preference(reasons, "package")?;
    for reason in reasons {
        match reason {
            ResolutionReasonV1::PinnedSourceMismatch { .. }
            | ResolutionReasonV1::VersionMismatch { .. }
            | ResolutionReasonV1::DomainMismatch
            | ResolutionReasonV1::UnknownFeatures { .. }
            | ResolutionReasonV1::TrustExceeded { .. }
            | ResolutionReasonV1::NoEligibleRealization
            | ResolutionReasonV1::LowerPreference => {}
            ResolutionReasonV1::Backtracked { failure } => {
                validate_backtracking_failure(failure)?;
            }
            _ => {
                return Err(invalid_receipt(
                    "discarded package explanation contains a reason from another decision layer"
                        .to_owned(),
                ));
            }
        }
    }
    Ok(())
}

fn validate_discarded_realization_reasons(
    reasons: &[ResolutionReasonV1],
) -> Result<(), ResolutionReceiptError> {
    validate_lower_preference(reasons, "realization")?;
    if reasons.iter().any(|reason| {
        !matches!(
            reason,
            ResolutionReasonV1::ExplicitKindMismatch { .. }
                | ResolutionReasonV1::AutomaticKindDisabled
                | ResolutionReasonV1::DomainMismatch
                | ResolutionReasonV1::TargetMismatch { .. }
                | ResolutionReasonV1::UnknownFeatures { .. }
                | ResolutionReasonV1::TrustExceeded { .. }
                | ResolutionReasonV1::InterfaceRequirementUnavailable { .. }
                | ResolutionReasonV1::EngineBuildMismatch { .. }
                | ResolutionReasonV1::LowerPreference
        )
    }) {
        return Err(invalid_receipt(
            "discarded realization explanation contains a reason from another decision layer"
                .to_owned(),
        ));
    }
    Ok(())
}

fn validate_discarded_provider_reasons(
    reasons: &[ResolutionReasonV1],
) -> Result<(), ResolutionReceiptError> {
    validate_lower_preference(reasons, "capability provider")?;
    for reason in reasons {
        match reason {
            ResolutionReasonV1::CardinalityMismatch { .. }
            | ResolutionReasonV1::DomainMismatch
            | ResolutionReasonV1::InterfaceVersionMismatch { .. }
            | ResolutionReasonV1::PackageNotSelected
            | ResolutionReasonV1::LowerPreference => {}
            ResolutionReasonV1::Backtracked { failure } => {
                validate_backtracking_failure(failure)?;
            }
            _ => {
                return Err(invalid_receipt(
                    "discarded capability-provider explanation contains a reason from another decision layer"
                        .to_owned(),
                ));
            }
        }
    }
    Ok(())
}

fn validate_lower_preference(
    reasons: &[ResolutionReasonV1],
    layer: &str,
) -> Result<(), ResolutionReceiptError> {
    if reasons.len() > 1
        && reasons
            .iter()
            .any(|reason| matches!(reason, ResolutionReasonV1::LowerPreference))
    {
        return Err(invalid_receipt(format!(
            "{layer} explanation combines lower-preference with a concrete rejection"
        )));
    }
    Ok(())
}

fn validate_backtracking_failure(
    failure: &BacktrackingFailureV1,
) -> Result<(), ResolutionReceiptError> {
    if failure.package_chain.is_empty() {
        return Err(invalid_receipt(
            "backtracking explanation has an empty package chain".to_owned(),
        ));
    }
    Ok(())
}

fn validate_provider_for_demand(
    capability: &CapabilityId,
    demand: &CapabilityDemandReceiptV1,
    provider: &CapabilityProviderReceiptV1,
) -> Result<(), ResolutionReceiptError> {
    if provider.cardinality != demand.cardinality {
        return Err(invalid_receipt(format!(
            "capability {capability} provider {} declares {:?}, demand requires {:?}",
            provider.package, provider.cardinality, demand.cardinality
        )));
    }
    if !demand.range.matches(&provider.provided_version) {
        return Err(invalid_receipt(format!(
            "capability {capability} provider {} version {} does not satisfy {}",
            provider.package, provider.provided_version, demand.range
        )));
    }
    if demand
        .explicit_provider
        .as_ref()
        .is_some_and(|required| required != &provider.package)
    {
        return Err(invalid_receipt(format!(
            "capability {capability} provider {} differs from explicit provider",
            provider.package
        )));
    }
    Ok(())
}
fn invalid_receipt(reason: String) -> ResolutionReceiptError {
    ResolutionReceiptError::InvalidStructure { reason }
}

#[derive(Serialize)]
struct ResolutionIdentity<'a> {
    schema_version: u32,
    composition_hash: CanonicalHash,
    resolution_intent_hash: CanonicalHash,
    profile: &'a StableId,
    profile_kind: ProfileKind,
    target: &'a TargetTriple,
    roots: &'a BTreeSet<PackageName>,
    packages: BTreeMap<&'a PackageName, ResolutionPackageIdentity<'a>>,
    capabilities: &'a BTreeMap<CapabilityId, CapabilityResolutionReceiptV1>,
    namespace_grants: &'a BTreeSet<NamespaceGrant>,
}

impl<'a> From<&'a ResolutionReceiptV1> for ResolutionIdentity<'a> {
    fn from(receipt: &'a ResolutionReceiptV1) -> Self {
        Self {
            schema_version: receipt.schema_version,
            composition_hash: receipt.composition_hash,
            resolution_intent_hash: receipt.resolution_intent_hash,
            profile: &receipt.profile,
            profile_kind: receipt.profile_kind,
            target: &receipt.target,
            roots: &receipt.roots,
            namespace_grants: &receipt.namespace_grants,
            packages: receipt
                .packages
                .iter()
                .map(|(name, package)| (name, ResolutionPackageIdentity::from(package)))
                .collect(),
            capabilities: &receipt.capabilities,
        }
    }
}

#[derive(Serialize)]
struct ResolutionPackageIdentity<'a> {
    version: &'a PackageVersion,
    features: &'a BTreeSet<String>,
    domains: &'a BTreeSet<PackageDomain>,
    dependencies: &'a BTreeMap<PackageName, ResolvedDependencyV1>,
    namespace_requests: &'a BTreeSet<NamespaceGrantPattern>,
    schemas: &'a BTreeSet<SchemaId>,
    realization: &'a ResolvedRealizationV1,
}

impl<'a> From<&'a ResolvedPackageV1> for ResolutionPackageIdentity<'a> {
    fn from(package: &'a ResolvedPackageV1) -> Self {
        Self {
            version: &package.version,
            features: &package.features,
            domains: &package.domains,
            dependencies: &package.dependencies,
            namespace_requests: &package.namespace_requests,
            schemas: &package.schemas,
            realization: &package.realization,
        }
    }
}

#[derive(Serialize)]
struct ReceiptIdentity<'a> {
    schema_version: u32,
    composition_hash: CanonicalHash,
    composition_provenance_hash: CanonicalHash,
    resolution_intent_hash: CanonicalHash,
    profile: &'a StableId,
    profile_kind: ProfileKind,
    target: &'a TargetTriple,
    evaluation_policy: &'a StableId,
    evaluation_limits: NickelEvaluationLimits,
    roots: &'a BTreeSet<PackageName>,
    packages: &'a BTreeMap<PackageName, ResolvedPackageV1>,
    capabilities: &'a BTreeMap<CapabilityId, CapabilityResolutionReceiptV1>,
    namespace_grants: &'a BTreeSet<NamespaceGrant>,
    explanation: &'a ResolutionExplanationV1,
    resolution_hash: CanonicalHash,
}

impl<'a> From<&'a ResolutionReceiptV1> for ReceiptIdentity<'a> {
    fn from(receipt: &'a ResolutionReceiptV1) -> Self {
        Self {
            schema_version: receipt.schema_version,
            composition_hash: receipt.composition_hash,
            composition_provenance_hash: receipt.composition_provenance_hash,
            resolution_intent_hash: receipt.resolution_intent_hash,
            profile: &receipt.profile,
            profile_kind: receipt.profile_kind,
            target: &receipt.target,
            evaluation_policy: &receipt.evaluation_policy,
            evaluation_limits: receipt.evaluation_limits,
            roots: &receipt.roots,
            packages: &receipt.packages,
            capabilities: &receipt.capabilities,
            namespace_grants: &receipt.namespace_grants,
            explanation: &receipt.explanation,
            resolution_hash: receipt.resolution_hash,
        }
    }
}

#[derive(Serialize)]
struct BuildIntentIdentity<'a> {
    schema_version: u32,
    resolution_receipt_hash: CanonicalHash,
    target: &'a TargetTriple,
    namespace_grants: &'a BTreeSet<NamespaceGrant>,
    packages: &'a BTreeMap<PackageName, PackageBuildIntentV1>,
}

impl<'a> From<&'a BuildIntentV1> for BuildIntentIdentity<'a> {
    fn from(intent: &'a BuildIntentV1) -> Self {
        Self {
            schema_version: intent.schema_version,
            resolution_receipt_hash: intent.resolution_receipt_hash,
            target: &intent.target,
            namespace_grants: &intent.namespace_grants,
            packages: &intent.packages,
        }
    }
}
