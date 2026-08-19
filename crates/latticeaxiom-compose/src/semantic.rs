//! Typed semantic registration authoring models.

use std::collections::{BTreeMap, BTreeSet};

use latticeaxiom_core::{PackageName, SchemaId, SourceProvenance, StableId};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Registry kind targeted by a semantic contract.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum TargetKind {
    /// Voxel block definitions.
    Block,
    /// Inventory item definitions.
    Item,
    /// Fluid definitions stored independently from blocks.
    Fluid,
    /// Biome definitions.
    Biome,
    /// Persistent or runtime entity definitions.
    Entity,
    /// Dimension definitions.
    Dimension,
}

/// A stable semantic ID paired with its target registry kind.
#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TypedSemanticId {
    /// Stable semantic registration ID.
    pub id: StableId,
    /// Target registry kind enforced by the semantic compiler.
    pub target_kind: TargetKind,
}

/// Definition of an additive semantic Tag contract.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SemanticTagDefinition {
    /// Typed Tag ID.
    pub tag: TypedSemanticId,
    /// Whether authorized foreign packages may contribute their own targets.
    pub extensible: bool,
}

/// One target contributed to a semantic Tag.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SemanticTagContribution {
    /// Typed Tag ID receiving the target.
    pub tag: TypedSemanticId,
    /// Concrete target registration.
    pub target: StableId,
    /// Package owning the contribution.
    pub declared_by: PackageName,
    /// Source location retained for conflict diagnostics.
    pub provenance: SourceProvenance,
}

/// Merge behavior for a typed semantic Map.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum SemanticMapMerger {
    /// Reject multiple unequal values for the same target.
    Conflict,
}

/// Definition of a typed semantic Map contract.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SemanticMapDefinition {
    /// Typed Map ID.
    pub map: TypedSemanticId,
    /// Schema of each Map value.
    pub value_schema: SchemaId,
    /// Deterministic merge rule.
    pub merger: SemanticMapMerger,
    /// Whether authorized foreign packages may contribute values for their targets.
    pub extensible: bool,
}

/// One target-to-value semantic Map contribution.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SemanticMapContribution {
    /// Typed Map ID receiving the value.
    pub map: TypedSemanticId,
    /// Concrete target registration.
    pub target: StableId,
    /// Value validated against the Map schema.
    pub value: Value,
    /// Package owning the contribution.
    pub declared_by: PackageName,
    /// Source location retained for conflict diagnostics.
    pub provenance: SourceProvenance,
}

/// Definition of a shared finite state property.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct StatePropertyDefinition {
    /// Stable state-property contract ID.
    pub id: StableId,
    /// Target registry kind whose placed instances may carry the state.
    pub target_kind: TargetKind,
    /// Schema of the state value.
    pub value_schema: SchemaId,
    /// Closed set of canonical values.
    pub allowed_values: Vec<Value>,
}

/// Runtime policy for an affordance definition.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum AffordancePolicy {
    /// Pure static data compiled into a direct predicate plan.
    StaticData,
    /// A typed static handler registered by a static realization.
    StaticHandler,
    /// A versioned callback invoked in portable batches.
    PortableBatch,
}

/// Definition of a contextual behavior escape hatch.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AffordanceDefinition {
    /// Stable affordance ID.
    pub id: StableId,
    /// Target registry kind queried by the affordance.
    pub target_kind: TargetKind,
    /// Request schema.
    pub request_schema: SchemaId,
    /// Response schema.
    pub response_schema: SchemaId,
    /// Runtime policy selected by the manifest.
    pub policy: AffordancePolicy,
    /// Stable callback key for executable policies.
    pub callback: Option<StableId>,
}

/// Typed value condition used by semantic predicates.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(
    deny_unknown_fields,
    rename_all = "kebab-case",
    tag = "op",
    content = "value"
)]
pub enum TypedCondition {
    /// Exact typed equality.
    Equal(Value),
    /// Membership in a closed typed set.
    In(Vec<Value>),
}

/// Closed, serializable semantic predicate expression.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case", tag = "op")]
pub enum PredicateExpression {
    /// Match one concrete registration.
    Exact {
        /// Concrete registration ID.
        target: StableId,
    },
    /// Match membership in a semantic Tag.
    InTag {
        /// Typed Tag ID.
        tag: TypedSemanticId,
    },
    /// Match presence in a semantic Map.
    MapPresent {
        /// Typed Map ID.
        map: TypedSemanticId,
    },
    /// Match a typed Map value condition.
    MapMatches {
        /// Typed Map ID.
        map: TypedSemanticId,
        /// Condition applied to the value.
        condition: TypedCondition,
    },
    /// Match a placed-instance state property.
    StateMatches {
        /// State-property contract ID.
        property: StableId,
        /// Condition applied to the current state.
        condition: TypedCondition,
    },
    /// Match a contextual affordance.
    Supports {
        /// Affordance contract ID.
        affordance: StableId,
        /// Typed request parameters.
        parameters: Value,
    },
    /// Require every child predicate to match.
    All {
        /// Child predicates with the same target kind.
        predicates: Vec<Self>,
    },
    /// Require at least one child predicate to match.
    Any {
        /// Child predicates with the same target kind.
        predicates: Vec<Self>,
    },
    /// Invert a child predicate.
    Not {
        /// Child predicate.
        predicate: Box<Self>,
    },
}

/// A predicate paired with its enforced target kind.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ContentPredicate {
    /// Registry kind against which the expression is type-checked.
    pub target_kind: TargetKind,
    /// Closed predicate expression.
    pub expression: PredicateExpression,
}

/// Number of concrete targets required by a content Role.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum RoleCardinality {
    /// Exactly one concrete target is required.
    ExactlyOne,
    /// Zero or one concrete target may be selected.
    ZeroOrOne,
    /// At least one concrete target is required.
    OneOrMore,
}

/// Authority allowed to select a content Role binding.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum RoleAuthority {
    /// Selection is made by deterministic graph rules.
    Graph,
    /// A composition profile may select the binding.
    Profile,
    /// An existing world freezes the binding.
    FrozenWorld,
}

/// Definition of a concrete-output selection point.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ContentRoleDefinition {
    /// Stable Role ID.
    pub id: StableId,
    /// Predicate accepted by Role candidates.
    pub accepts: ContentPredicate,
    /// Required number of selected targets.
    pub cardinality: RoleCardinality,
    /// Binding authority.
    pub authority: RoleAuthority,
}

/// Classification of a Role offer.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum RoleOfferClass {
    /// Normal candidate considered before fallback activation.
    Normal,
    /// Candidate contributed only by an activated fallback bundle.
    Fallback,
}

/// One concrete target offered to a content Role.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RoleOffer {
    /// Role receiving the candidate.
    pub role: StableId,
    /// Concrete target offered by the package.
    pub target: StableId,
    /// Normal or fallback classification.
    pub class: RoleOfferClass,
}

/// Activation policy for a content bundle.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(
    deny_unknown_fields,
    rename_all = "kebab-case",
    tag = "mode",
    content = "roles"
)]
pub enum BundleActivation {
    /// Always merge the bundle into the registration image.
    Always,
    /// Activate atomically when the listed Roles remain missing.
    FallbackForMissingRoles(BTreeSet<StableId>),
}

/// An atomic collection of fallback or unconditional content.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ContentBundle {
    /// Stable content-bundle ID.
    pub id: StableId,
    /// Activation policy.
    pub activation: BundleActivation,
    /// Concrete registration IDs contained by the bundle.
    pub registrations: BTreeSet<StableId>,
    /// Semantic fragments contained by the bundle.
    pub semantics: Vec<SemanticFragment>,
    /// Role offers contained by the bundle.
    pub role_offers: Vec<RoleOffer>,
}

/// One semantic registration fragment.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(
    deny_unknown_fields,
    rename_all = "kebab-case",
    tag = "kind",
    content = "value"
)]
pub enum SemanticFragment {
    /// Tag contract definition.
    TagDefinition(SemanticTagDefinition),
    /// Tag membership contribution.
    TagContribution(SemanticTagContribution),
    /// Map contract definition.
    MapDefinition(SemanticMapDefinition),
    /// Map value contribution.
    MapContribution(SemanticMapContribution),
    /// Shared state-property definition.
    StateProperty(StatePropertyDefinition),
    /// Contextual affordance definition.
    Affordance(AffordanceDefinition),
    /// Content Role definition.
    Role(ContentRoleDefinition),
    /// Concrete Role offer.
    RoleOffer(RoleOffer),
    /// Atomic content bundle.
    Bundle(ContentBundle),
}

/// Resolved semantic catalog stored in a registration image.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SemanticCatalog {
    /// Expanded Tag memberships keyed by typed Tag ID.
    pub tags: BTreeMap<TypedSemanticId, BTreeSet<StableId>>,
    /// Resolved Map values keyed by typed Map ID then concrete target.
    pub maps: BTreeMap<TypedSemanticId, BTreeMap<StableId, Value>>,
    /// Concrete Role bindings keyed by stable Role ID.
    pub role_bindings: BTreeMap<StableId, Vec<StableId>>,
    /// Active content bundles frozen by composition.
    pub active_bundles: BTreeSet<StableId>,
}
