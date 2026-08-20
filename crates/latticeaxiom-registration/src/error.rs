//! Stable registration compiler failures.

use latticeaxiom_compose::{
    CompositionError, GraphHashError, RegistrationHashError, RegistrationKind, RoleOfferClass,
    TargetKind, ValueValidationError,
};
use latticeaxiom_core::{
    CanonicalHash, CanonicalJsonError, CapabilityId, NamespaceGrantPattern, PackageName,
    PackageVersion, RegistrationNamespace, SchemaId, StableId,
};
use thiserror::Error;

/// A deterministic registration compile failure emitted before code activation.
#[derive(Debug, Error)]
pub enum RegistrationCompileError {
    /// The evaluated composition is structurally invalid.
    #[error("composition validation failed: {0}")]
    Composition(#[from] CompositionError),
    /// The locked graph fails its claimed canonical hashes.
    #[error("locked graph verification failed: {0}")]
    GraphHash(#[from] GraphHashError),
    /// A manifest fails its claimed semantic hash.
    #[error("registration manifest verification failed: {0}")]
    ManifestHash(#[from] RegistrationHashError),
    /// Canonical JSON encoding failed.
    #[error(transparent)]
    Canonical(#[from] CanonicalJsonError),
    /// A finite schema or typed value is invalid.
    #[error("typed value validation failed: {0}")]
    Value(#[from] ValueValidationError),
    /// The lock schema is not supported by this compiler.
    #[error("unsupported locked graph schema {found}; supported schema is {supported}")]
    UnsupportedLockSchema {
        /// Encountered schema version.
        found: u32,
        /// Supported schema version.
        supported: u32,
    },
    /// Composition semantic identity differs from the lock.
    #[error("composition hash mismatch: locked {locked}, recomputed {actual}")]
    CompositionHashMismatch {
        /// Hash frozen by the lock.
        locked: CanonicalHash,
        /// Recomputed composition hash.
        actual: CanonicalHash,
    },
    /// Composition provenance differs from the lock.
    #[error("composition provenance mismatch: locked {locked}, recomputed {actual}")]
    CompositionProvenanceMismatch {
        /// Hash frozen by the lock.
        locked: CanonicalHash,
        /// Recomputed provenance hash.
        actual: CanonicalHash,
    },
    /// Graph roots differ from the composition roots.
    #[error("locked graph roots do not match the evaluated composition roots")]
    RootSetMismatch,
    /// A declared root is absent from the locked package closure.
    #[error("root package {package} is missing from the locked closure")]
    MissingRoot {
        /// Missing root package.
        package: PackageName,
    },
    /// A package map key does not equal the embedded package name.
    #[error("package map key {key} does not match embedded package {embedded}")]
    PackageKeyMismatch {
        /// Map key.
        key: PackageName,
        /// Embedded package name.
        embedded: PackageName,
    },
    /// A dependency target is absent from the locked closure.
    #[error("package {package} depends on missing package {dependency}")]
    MissingDependency {
        /// Requiring package.
        package: PackageName,
        /// Missing dependency.
        dependency: PackageName,
    },
    /// A locked dependency edge disagrees with the target node version.
    #[error(
        "package {package} locks dependency {dependency} at {edge_version}, but the node is {node_version}"
    )]
    DependencyVersionMismatch {
        /// Requiring package.
        package: PackageName,
        /// Dependency package.
        dependency: PackageName,
        /// Version recorded on the edge.
        edge_version: PackageVersion,
        /// Version recorded on the node.
        node_version: PackageVersion,
    },
    /// A selected capability provider is absent from the closure.
    #[error("capability {capability} selects missing provider {provider}")]
    MissingCapabilityProvider {
        /// Capability being provided.
        capability: CapabilityId,
        /// Missing provider package.
        provider: PackageName,
    },
    /// Capability providers are duplicated or not canonically ordered.
    #[error("capability {capability} providers are not unique canonical package order")]
    NonCanonicalCapabilityProviders {
        /// Capability with an empty or duplicated provider list.
        capability: CapabilityId,
    },
    /// A package cannot be reached from a root, dependency, or provider edge.
    #[error("package {package} is not reachable in the frozen closure")]
    UnreachablePackage {
        /// Unreachable package.
        package: PackageName,
    },
    /// A locked package has no registration input.
    #[error("locked package {package} has no registration input")]
    MissingPackageInput {
        /// Missing package input.
        package: PackageName,
    },
    /// A registration input is not part of the locked closure.
    #[error("registration input for {package} is outside the locked closure")]
    UnexpectedPackageInput {
        /// Unexpected package input.
        package: PackageName,
    },
    /// Manifest package identity differs from its map key.
    #[error("registration input key {key} contains manifest for {manifest}")]
    ManifestPackageMismatch {
        /// Input map key.
        key: PackageName,
        /// Manifest package.
        manifest: PackageName,
    },
    /// Manifest version differs from the exact locked node.
    #[error("manifest for {package} is {manifest}, but the lock selected {locked}")]
    ManifestVersionMismatch {
        /// Package with mismatch.
        package: PackageName,
        /// Manifest version.
        manifest: PackageVersion,
        /// Locked version.
        locked: PackageVersion,
    },
    /// Manifest schema is unsupported.
    #[error("manifest for {package} uses unsupported schema {found}")]
    UnsupportedManifestSchema {
        /// Package with unsupported manifest.
        package: PackageName,
        /// Encountered manifest schema.
        found: u32,
    },
    /// Lock manifest hash differs from the verified semantic manifest hash.
    #[error("manifest hash for {package} differs from the locked receipt")]
    LockedManifestMismatch {
        /// Package with mismatch.
        package: PackageName,
    },
    /// A map key differs from the stable ID stored in its row.
    #[error("{catalog} map key {key} does not match row ID {row}")]
    CatalogKeyMismatch {
        /// Stable catalog name.
        catalog: &'static str,
        /// Map key.
        key: String,
        /// Embedded row identity.
        row: String,
    },
    /// A catalog row violates a closed structural invariant.
    #[error("invalid {catalog} row {id}: {reason}")]
    InvalidCatalogRow {
        /// Stable catalog family.
        catalog: &'static str,
        /// Invalid row ID.
        id: StableId,
        /// Stable invariant description.
        reason: &'static str,
    },
    /// A namespace grant carries no bounded permission patterns.
    #[error("namespace grant to {grantee} has no patterns")]
    EmptyNamespaceGrant {
        /// Package receiving the empty grant.
        grantee: PackageName,
    },
    /// A pattern names a different namespace than its grant envelope.
    #[error("namespace grant for {namespace} contains pattern {pattern}")]
    NamespacePatternMismatch {
        /// Grant envelope namespace.
        namespace: RegistrationNamespace,
        /// Mismatched pattern.
        pattern: NamespaceGrantPattern,
    },
    /// Locked profile grants differ from the trusted composition policy rows.
    #[error("locked profile namespace grants differ from trusted composition policy")]
    ProfileNamespaceGrantMismatch,
    /// A profile or package cannot issue the requested namespace grant.
    #[error("namespace grant to {grantee} has no authorized grantor chain")]
    UnauthorizedNamespaceGrant {
        /// Package receiving the unauthorized grant.
        grantee: PackageName,
    },
    /// Package delegation does not follow a direct locked dependency edge.
    #[error("namespace grantor {grantor} cannot delegate to non-dependency {grantee}")]
    NamespaceDelegationOutsideDependency {
        /// Delegating package.
        grantor: PackageName,
        /// Proposed grantee.
        grantee: PackageName,
    },
    /// Delegation expands beyond the grantor's effective permission patterns.
    #[error("namespace delegation from {grantor} to {grantee} expands authority or forms a cycle")]
    NamespaceDelegationExpansion {
        /// Delegating package.
        grantor: PackageName,
        /// Proposed grantee.
        grantee: PackageName,
    },
    /// A package authors an ID outside its effective owner-bound grant.
    #[error("package {package} has no namespace grant covering {id}")]
    MissingNamespaceGrant {
        /// Package authoring the ID.
        package: PackageName,
        /// Unauthorized stable ID.
        id: StableId,
    },
    /// A row claims ownership by a package other than its containing manifest.
    #[error("{catalog} row {id} declares {declared_by}, but is contained by {package}")]
    ForeignOwner {
        /// Stable catalog name.
        catalog: &'static str,
        /// Row ID.
        id: StableId,
        /// Claimed owner.
        declared_by: PackageName,
        /// Containing package.
        package: PackageName,
    },
    /// An exact registration ID has the wrong grammar kind.
    #[error(
        "registration {id} has kind `{actual}`, expected `{expected}` for {registration_kind:?}"
    )]
    RegistrationKindMismatch {
        /// Invalid registration ID.
        id: StableId,
        /// Declared registry kind.
        registration_kind: RegistrationKind,
        /// Required grammar kind.
        expected: &'static str,
        /// Encountered grammar kind.
        actual: String,
    },
    /// A stable catalog ID is defined more than once.
    #[error("stable ID {id} is defined by both {first} and {second}")]
    DuplicateCatalogId {
        /// Duplicated ID.
        id: StableId,
        /// First catalog family.
        first: &'static str,
        /// Second catalog family.
        second: &'static str,
    },
    /// A public schema omits a contract major.
    #[error("public schema {schema} must include a positive @major suffix")]
    UnversionedSchema {
        /// Invalid public schema.
        schema: SchemaId,
    },
    /// A schema declaration is not present in the locked package schema set.
    #[error("package {package} declares schema {schema} outside its locked schema set")]
    UnlockedSchemaDeclaration {
        /// Package carrying the extra declaration.
        package: PackageName,
        /// Schema absent from the lock node.
        schema: SchemaId,
    },
    /// A schema contract lacks a matching exact schema registration.
    #[error("schema contract {schema} has no exact schema registration")]
    MissingSchemaRegistration {
        /// Missing exact schema row.
        schema: SchemaId,
    },
    /// A locked package schema is absent from the package contract.
    #[error("locked schema {schema} owned by {package} is not declared")]
    LockedSchemaUndeclared {
        /// Package expected to own the schema.
        package: PackageName,
        /// Missing schema.
        schema: SchemaId,
    },
    /// A schema reference cannot be resolved in the active closure.
    #[error("{consumer} references undeclared schema {schema}")]
    UndeclaredSchema {
        /// Stable consumer ID.
        consumer: StableId,
        /// Missing schema.
        schema: SchemaId,
    },
    /// A system exact registration lacks a generated system declaration.
    #[error("system registration {system} has no system declaration")]
    MissingSystemDeclaration {
        /// Missing system declaration.
        system: StableId,
    },
    /// A generated system declaration has no exact system registration.
    #[error("system declaration {system} has no exact system registration")]
    UnexpectedSystemDeclaration {
        /// Unexpected system declaration.
        system: StableId,
    },
    /// A callback key is malformed or unversioned.
    #[error("callback key {callback} must use the versioned `callback` kind")]
    InvalidCallbackId {
        /// Invalid callback key.
        callback: StableId,
    },
    /// A callback consumer references no declared callback.
    #[error("consumer {consumer} references undeclared callback {callback}")]
    UndeclaredCallback {
        /// Callback consumer.
        consumer: StableId,
        /// Missing callback.
        callback: StableId,
    },
    /// A callback declaration has no active or declared consumer.
    #[error("callback {callback} is declared but unused")]
    UnusedCallback {
        /// Unused callback key.
        callback: StableId,
    },
    /// System and callback signatures disagree.
    #[error("system {system} signature differs from callback {callback}")]
    CallbackSignatureMismatch {
        /// System registration.
        system: StableId,
        /// Callback key.
        callback: StableId,
    },
    /// A system uses a stage outside the accepted stage catalog.
    #[error("system {system} uses unknown stage {stage}")]
    UnknownStage {
        /// System registration.
        system: StableId,
        /// Unknown stage.
        stage: StableId,
    },
    /// A schedule edge references no system in the active closure.
    #[error("system {system} orders against unknown system {target}")]
    UnknownScheduleTarget {
        /// Declaring system.
        system: StableId,
        /// Missing target system.
        target: StableId,
    },
    /// A schedule edge crosses semantic stages.
    #[error("system {system} in {stage} orders against {target} in {target_stage}")]
    CrossStageScheduleEdge {
        /// Declaring system.
        system: StableId,
        /// Declaring system stage.
        stage: StableId,
        /// Target system.
        target: StableId,
        /// Target system stage.
        target_stage: StableId,
    },
    /// A schedule stage contains a correctness cycle.
    #[error("schedule stage {stage} contains a cycle among {systems:?}")]
    ScheduleCycle {
        /// Cyclic stage.
        stage: StableId,
        /// Canonically sorted systems remaining in the cycle.
        systems: Vec<StableId>,
    },
    /// A compiler hard limit was exceeded before image construction.
    #[error("registration limit `{limit}` exceeded: observed {observed}, maximum {maximum}")]
    LimitExceeded {
        /// Stable limit name.
        limit: &'static str,
        /// Observed count or byte size.
        observed: usize,
        /// Maximum accepted value.
        maximum: usize,
    },
    /// A semantic definition or contribution references no contract.
    #[error("semantic contract {contract} is not defined")]
    UnknownSemanticContract {
        /// Missing contract ID.
        contract: StableId,
    },
    /// Semantic target kinds disagree.
    #[error("semantic contract {contract} expects {expected:?}, but target {target} is {actual:?}")]
    SemanticTargetKindMismatch {
        /// Semantic contract.
        contract: StableId,
        /// Concrete target.
        target: StableId,
        /// Required target kind.
        expected: TargetKind,
        /// Actual target kind.
        actual: TargetKind,
    },
    /// A package lacks owner authority for a semantic contribution.
    #[error("package {contributor} is not authorized to contribute target {target} to {contract}")]
    UnauthorizedSemanticContribution {
        /// Contract receiving the contribution.
        contract: StableId,
        /// Contributing package.
        contributor: PackageName,
        /// Contributed target.
        target: StableId,
    },
    /// Multiple unequal Map values target the same slot.
    #[error("semantic Map {map} has conflicting values for target {target}")]
    MapConflict {
        /// Map contract.
        map: StableId,
        /// Conflicting target.
        target: StableId,
    },
    /// A semantic row is duplicated where exactly one definition is required.
    #[error("semantic definition {id} is duplicated")]
    DuplicateSemanticDefinition {
        /// Duplicated semantic ID.
        id: StableId,
    },
    /// A nested bundle would require a fallback fixed point.
    #[error("content bundle {bundle} contains nested bundle {nested}")]
    NestedBundle {
        /// Outer bundle.
        bundle: StableId,
        /// Nested bundle.
        nested: StableId,
    },
    /// A bundle references an exact registration outside its owning package.
    #[error(
        "bundle {bundle} in {package} references foreign or missing registration {registration}"
    )]
    InvalidBundleRegistration {
        /// Bundle ID.
        bundle: StableId,
        /// Owning package.
        package: PackageName,
        /// Invalid registration.
        registration: StableId,
    },
    /// A fallback bundle conditionally owns a schema required by closure validation.
    #[error("fallback bundle {bundle} conditionally owns schema registration {schema}")]
    ConditionalSchemaRegistration {
        /// Conditional bundle.
        bundle: StableId,
        /// Schema registration that must remain unconditional.
        schema: StableId,
    },
    /// Conditional ownership of one exact registration is ambiguous.
    #[error("registration {registration} belongs to more than one fallback bundle")]
    DuplicateFallbackRegistration {
        /// Multiply owned conditional registration.
        registration: StableId,
    },
    /// A fallback-class Role offer appeared outside a conditional bundle.
    #[error("top-level Role offer {role} -> {target} is classified as fallback")]
    InvalidTopLevelRoleOffer {
        /// Role receiving the invalid offer.
        role: StableId,
        /// Concrete target carried by the invalid offer.
        target: StableId,
    },
    /// A bundle carries a Role offer with the wrong activation class.
    #[error("bundle {bundle} has {actual:?} Role offer; expected {expected:?}")]
    InvalidRoleOfferClass {
        /// Bundle containing the invalid offer.
        bundle: StableId,
        /// Required offer class for the activation mode.
        expected: RoleOfferClass,
        /// Encountered offer class.
        actual: RoleOfferClass,
    },
    /// A fallback guard is empty, unknown in the unconditional base, or optional.
    #[error("fallback bundle {bundle} has invalid guard role {role:?}")]
    InvalidFallbackGuard {
        /// Bundle carrying the invalid guard.
        bundle: StableId,
        /// Invalid Role, or `None` when the guard set is empty.
        role: Option<StableId>,
    },
    /// A public semantic contract ID has the wrong family kind or no major.
    #[error("semantic contract {id} must use kind {expected_kind} and a positive major")]
    InvalidSemanticId {
        /// Invalid public contract ID.
        id: StableId,
        /// Canonical family kind required by its typed definition.
        expected_kind: String,
    },
    /// A semantic definition targets a registry absent from the v1 exact catalog.
    #[error("semantic contract {contract} uses unsupported target kind {target_kind:?}")]
    UnsupportedSemanticDefinitionTarget {
        /// Unsupported semantic contract.
        contract: StableId,
        /// Target kind without an exact registry.
        target_kind: TargetKind,
    },
    /// A semantic contract targets an exact registry without a v1 target mapping.
    #[error("semantic target {target} uses unsupported registry kind {kind:?}")]
    UnsupportedSemanticTarget {
        /// Exact registration used as a semantic target.
        target: StableId,
        /// Registry kind lacking a typed semantic mapping.
        kind: RegistrationKind,
    },
    /// A first-version affordance requests an executable callback policy.
    #[error("affordance {affordance} requests a deferred callback policy")]
    AffordanceCallbackDeferred {
        /// Unsupported affordance.
        affordance: StableId,
    },
    /// A Role predicate uses a contextual operation unavailable to registration selection.
    #[error("role {role} uses contextual predicate operation `{operation}`")]
    ContextualRolePredicate {
        /// Role being compiled.
        role: StableId,
        /// Unsupported predicate operation.
        operation: &'static str,
    },
    /// A Role offer is contributed by a package that does not own the target.
    #[error("package {package} offers foreign target {target} to role {role}")]
    ForeignRoleOffer {
        /// Role receiving the offer.
        role: StableId,
        /// Offered target.
        target: StableId,
        /// Offering package.
        package: PackageName,
    },
    /// An explicit profile binding is invalid for the Role contract.
    #[error("profile binding {role} -> {target} is not an accepted candidate")]
    BindingRejected {
        /// Bound Role.
        role: StableId,
        /// Rejected target.
        target: StableId,
    },
    /// A required Role has no candidate after fallback.
    #[error("required role {role} is unsatisfied")]
    RoleUnsatisfied {
        /// Unsatisfied Role.
        role: StableId,
    },
    /// A Role has multiple candidates without explicit authority.
    #[error("role {role} is ambiguous among {candidates:?}")]
    RoleAmbiguous {
        /// Ambiguous Role.
        role: StableId,
        /// Canonically sorted candidates.
        candidates: Vec<StableId>,
    },
    /// Multiple fallback bundles can satisfy the same missing Role.
    #[error("role {role} has competing fallback bundles {bundles:?}")]
    FallbackAmbiguous {
        /// Missing Role.
        role: StableId,
        /// Competing bundle IDs.
        bundles: Vec<StableId>,
    },
    /// A fallback guard depends on a Role that was not missing at wave start.
    #[error("fallback bundle {bundle} has an invalid or cyclic guard on role {role}")]
    FallbackCycle {
        /// Invalid fallback bundle.
        bundle: StableId,
        /// Guard Role.
        role: StableId,
    },
    /// A selected capability provider did not declare the provided contract.
    #[error("package {provider} does not declare selected capability {capability}")]
    UndeclaredCapabilityProvider {
        /// Capability selected by the lock.
        capability: CapabilityId,
        /// Selected provider missing the declaration.
        provider: PackageName,
    },
    /// A freshly built receipt failed its own activation verification.
    #[error("compiled registration receipt failed verification: {0}")]
    CompiledReceipt(String),
    /// The flattened image cannot represent a numeric assignment.
    #[error("numeric registration ID overflow for {kind:?}")]
    NumericIdOverflow {
        /// Overflowing registry kind.
        kind: RegistrationKind,
    },
}

impl RegistrationCompileError {
    /// Returns the stable machine diagnostic code for this failure family.
    #[must_use]
    pub const fn code(&self) -> &'static str {
        match self {
            Self::Composition(_)
            | Self::CompositionHashMismatch { .. }
            | Self::CompositionProvenanceMismatch { .. }
            | Self::RootSetMismatch => "registration.composition_mismatch",
            Self::GraphHash(_)
            | Self::UnsupportedLockSchema { .. }
            | Self::MissingRoot { .. }
            | Self::PackageKeyMismatch { .. }
            | Self::MissingDependency { .. }
            | Self::DependencyVersionMismatch { .. }
            | Self::MissingCapabilityProvider { .. }
            | Self::NonCanonicalCapabilityProviders { .. }
            | Self::UnreachablePackage { .. } => "registration.invalid_closure",
            Self::MissingPackageInput { .. }
            | Self::UnexpectedPackageInput { .. }
            | Self::ManifestPackageMismatch { .. }
            | Self::ManifestVersionMismatch { .. }
            | Self::UnsupportedManifestSchema { .. }
            | Self::LockedManifestMismatch { .. }
            | Self::ManifestHash(_) => "registration.manifest_mismatch",
            Self::CatalogKeyMismatch { .. } => "registration.map_key_mismatch",
            Self::InvalidCatalogRow { .. } => "registration.catalog_invalid",
            Self::EmptyNamespaceGrant { .. }
            | Self::NamespacePatternMismatch { .. }
            | Self::ProfileNamespaceGrantMismatch
            | Self::UnauthorizedNamespaceGrant { .. }
            | Self::NamespaceDelegationOutsideDependency { .. }
            | Self::NamespaceDelegationExpansion { .. }
            | Self::MissingNamespaceGrant { .. } => "registration.namespace_unauthorized",
            Self::ForeignOwner { .. } => "registration.foreign_owner",
            Self::RegistrationKindMismatch { .. } => "registration.id_kind_mismatch",
            Self::DuplicateCatalogId { .. } | Self::DuplicateSemanticDefinition { .. } => {
                "registration.duplicate_id"
            }
            Self::UnversionedSchema { .. }
            | Self::UnlockedSchemaDeclaration { .. }
            | Self::MissingSchemaRegistration { .. }
            | Self::LockedSchemaUndeclared { .. }
            | Self::UndeclaredSchema { .. } => "registration.schema_invalid",
            Self::MissingSystemDeclaration { .. }
            | Self::UnexpectedSystemDeclaration { .. }
            | Self::InvalidCallbackId { .. }
            | Self::UndeclaredCallback { .. }
            | Self::UnusedCallback { .. }
            | Self::CallbackSignatureMismatch { .. } => "registration.callback_invalid",
            Self::UnknownStage { .. }
            | Self::UnknownScheduleTarget { .. }
            | Self::CrossStageScheduleEdge { .. }
            | Self::ScheduleCycle { .. } => "registration.schedule_invalid",
            Self::LimitExceeded { .. } | Self::NumericIdOverflow { .. } => {
                "registration.limit_exceeded"
            }
            Self::UnknownSemanticContract { .. } => "semantic.unknown_contract",
            Self::InvalidSemanticId { .. } => "semantic.contract_id_invalid",
            Self::SemanticTargetKindMismatch { .. }
            | Self::UnsupportedSemanticDefinitionTarget { .. }
            | Self::UnsupportedSemanticTarget { .. } => "semantic.target_kind_mismatch",
            Self::UnauthorizedSemanticContribution { .. } | Self::ForeignRoleOffer { .. } => {
                "semantic.unauthorized_contribution"
            }
            Self::MapConflict { .. } => "semantic.map_conflict",
            Self::NestedBundle { .. }
            | Self::InvalidBundleRegistration { .. }
            | Self::ConditionalSchemaRegistration { .. }
            | Self::DuplicateFallbackRegistration { .. }
            | Self::InvalidTopLevelRoleOffer { .. }
            | Self::InvalidRoleOfferClass { .. }
            | Self::InvalidFallbackGuard { .. }
            | Self::FallbackCycle { .. } => "semantic.fallback_cycle",
            Self::AffordanceCallbackDeferred { .. } | Self::ContextualRolePredicate { .. } => {
                "semantic.affordance_unavailable"
            }
            Self::BindingRejected { .. } => "semantic.binding_rejected",
            Self::RoleUnsatisfied { .. } => "semantic.role_unsatisfied",
            Self::RoleAmbiguous { .. } => "semantic.role_ambiguous",
            Self::FallbackAmbiguous { .. } => "semantic.fallback_ambiguous",
            Self::UndeclaredCapabilityProvider { .. } => "registration.capability_undeclared",
            Self::CompiledReceipt(_) | Self::Canonical(_) | Self::Value(_) => {
                "registration.encoding_invalid"
            }
        }
    }
}
