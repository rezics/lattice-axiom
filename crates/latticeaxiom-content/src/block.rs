use std::collections::BTreeSet;

use latticeaxiom_core::StableId;
use serde::{Deserialize, Deserializer, Serialize, de};

use crate::{
    BLOCK_DEFINITION_SCHEMA_V1, BlockStatePropertyV1, BlockStateV1, BlockStateValueV1,
    ContentError, ContentHeaderV1, ContentResult, header::validate_header,
    header::validate_optional_exact_reference, state::definition_token,
    state::normalize_state_schema, state::validate_state,
};

/// Opaque definition-scoped geometric form selected by one block-state row.
///
/// `cube`, `slab`, `stair`, and `wall` are intentionally not platform enum
/// variants. The owning definition can use those tokens, but a public shared
/// form vocabulary remains gated on real mesh, collision, and placement
/// consumers. A form token does not create another content `StableId`.
#[repr(transparent)]
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct BlockFormV1(String);

impl BlockFormV1 {
    /// Creates a bounded definition-scoped form token.
    ///
    /// # Errors
    ///
    /// Returns an error when `value` is not a lower-kebab ASCII token.
    pub fn new(value: impl Into<String>) -> ContentResult<Self> {
        definition_token(value, "block form").map(Self)
    }

    /// Returns the definition-scoped token.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for BlockFormV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::new(value).map_err(de::Error::custom)
    }
}

macro_rules! define_policy_reference {
    ($(#[$metadata:meta])* $name:ident, $context:literal) => {
        $(#[$metadata])*
        #[repr(transparent)]
        #[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
        #[serde(transparent)]
        pub struct $name(StableId);

        impl $name {
            /// Creates a package-owned versioned policy reference.
            ///
            /// # Errors
            ///
            /// Returns an error when the identity omits its contract major.
            pub fn new(id: StableId) -> ContentResult<Self> {
                validate_policy_reference(&id, $context)?;
                Ok(Self(id))
            }

            /// Returns the versioned policy identity.
            #[must_use]
            pub const fn as_stable_id(&self) -> &StableId {
                &self.0
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: Deserializer<'de>,
            {
                let id = StableId::deserialize(deserializer)?;
                Self::new(id).map_err(de::Error::custom)
            }
        }
    };
}

define_policy_reference!(
    /// Versioned authoritative solid-occupancy semantics for one state.
    SolidOccupancyV1,
    "block solid occupancy"
);
define_policy_reference!(
    /// Versioned authoritative collision-shape semantics for one state.
    CollisionShapeV1,
    "block collision shape"
);
define_policy_reference!(
    /// Versioned authoritative selection-shape semantics for one state.
    SelectionShapeV1,
    "block selection shape"
);
define_policy_reference!(
    /// Versioned authoritative face-occlusion semantics for one state.
    OcclusionV1,
    "block occlusion"
);
define_policy_reference!(
    /// Versioned authoritative replacement semantics for one state.
    ReplaceabilityV1,
    "block replaceability"
);
define_policy_reference!(
    /// Versioned authoritative orthogonal fluid-occupancy semantics.
    FluidOccupancyPolicyV1,
    "block fluid occupancy"
);

/// Intrinsic semantics for one explicitly allowed discrete block state.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BlockStateSemanticsV1 {
    /// Complete concrete state key/value mapping.
    pub state: BlockStateV1,
    /// Finite geometric form.
    pub form: Option<BlockFormV1>,
    /// Authoritative solid occupancy.
    pub solid_occupancy: SolidOccupancyV1,
    /// Collision policy.
    pub collision: CollisionShapeV1,
    /// Selection policy.
    pub selection: SelectionShapeV1,
    /// Face-occlusion policy.
    pub occlusion: OcclusionV1,
    /// Block replacement policy.
    pub replaceability: ReplaceabilityV1,
    /// Orthogonal fluid-layer policy.
    pub fluid_occupancy: FluidOccupancyPolicyV1,
}

/// References from intrinsic block data to independently owned gameplay rows.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BlockRuleReferencesV1 {
    /// Optional versioned narrow mining-rule registration.
    pub mining_rule: Option<StableId>,
    /// Optional versioned drop-table registration.
    pub drop_table: Option<StableId>,
    /// Optional exact placement item.
    pub placement_item: Option<StableId>,
}

/// Authored v1 intrinsic block definition.
///
/// `states` is the explicit finite palette of legal combinations; state values
/// are not expanded into content identities. The catalog compiler sorts these
/// rows by canonical state bytes and rejects missing, duplicate, or undeclared
/// combinations.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BlockDefinitionV1 {
    /// Shared owner-aware header.
    pub header: ContentHeaderV1,
    /// Supported versioned state keys and each definition's allowed subset.
    pub state_schema: Vec<BlockStatePropertyV1>,
    /// Complete finite set of legal discrete states and their semantics.
    pub states: Vec<BlockStateSemanticsV1>,
    /// Materialized canonical default state.
    pub default_state: BlockStateV1,
    /// References to separately typed gameplay rules.
    pub rules: BlockRuleReferencesV1,
    /// Optional exact presentation asset; excluded from authoritative hash.
    pub presentation_binding: Option<StableId>,
}

/// Validated block definition with a canonical state order.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ValidatedBlockDefinitionV1 {
    definition: BlockDefinitionV1,
    default_state_index: u16,
}

impl ValidatedBlockDefinitionV1 {
    /// Returns the canonical normalized definition.
    #[must_use]
    pub const fn definition(&self) -> &BlockDefinitionV1 {
        &self.definition
    }

    /// Returns the canonical state palette.
    #[must_use]
    pub fn states(&self) -> &[BlockStateSemanticsV1] {
        &self.definition.states
    }

    /// Returns the index of the materialized default state.
    #[must_use]
    pub const fn default_state_index(&self) -> u16 {
        self.default_state_index
    }

    /// Returns whether this definition explicitly permits `state`.
    #[must_use]
    pub fn contains_state(&self, state: &BlockStateV1) -> bool {
        self.semantics_for(state).is_some()
    }

    /// Returns intrinsic semantics for an explicit palette state.
    #[must_use]
    pub fn semantics_for(&self, state: &BlockStateV1) -> Option<&BlockStateSemanticsV1> {
        self.definition
            .states
            .iter()
            .find(|candidate| &candidate.state == state)
    }
}

pub(crate) fn validate_block_definition(
    mut definition: BlockDefinitionV1,
    max_properties: usize,
    max_values_per_property: usize,
    max_states: usize,
) -> ContentResult<ValidatedBlockDefinitionV1> {
    validate_header(&definition.header, "block", BLOCK_DEFINITION_SCHEMA_V1)?;
    validate_optional_exact_reference(
        definition.presentation_binding.as_ref(),
        "asset",
        "block presentation binding",
    )?;
    validate_rule_references(&definition)?;

    let block = &definition.header.stable_id;
    definition.state_schema = normalize_state_schema(
        block,
        &definition.state_schema,
        max_properties,
        max_values_per_property,
    )?;
    if definition.states.is_empty() {
        return Err(ContentError::InvalidBlockState {
            block: block.clone(),
            reason: "state palette must contain at least one row",
        });
    }
    if definition.states.len() > max_states {
        return Err(ContentError::LimitExceeded {
            resource: "block_state_palette",
            actual: definition.states.len(),
            limit: max_states,
        });
    }

    validate_state(block, &definition.default_state, &definition.state_schema)?;
    let schema_default = BlockStateV1 {
        values: definition
            .state_schema
            .iter()
            .map(|property| (property.key.clone(), property.default_value.clone()))
            .collect(),
    };
    if definition.default_state != schema_default {
        return Err(ContentError::InvalidBlockState {
            block: block.clone(),
            reason: "definition default differs from materialized property defaults",
        });
    }

    let mut keyed_states = Vec::with_capacity(definition.states.len());
    let mut seen = BTreeSet::new();
    for state in definition.states {
        validate_state(block, &state.state, &definition.state_schema)?;
        validate_semantics(block, &state, &definition.state_schema)?;
        let key = state.state.canonical_bytes()?;
        if !seen.insert(key.clone()) {
            return Err(ContentError::Duplicate {
                resource: "block_state",
                id: format!("{block}:{}", String::from_utf8_lossy(&key)),
            });
        }
        keyed_states.push((key, state));
    }
    keyed_states.sort_by(|left, right| left.0.cmp(&right.0));
    definition.states = keyed_states.into_iter().map(|(_, state)| state).collect();
    let default_state_index = definition
        .states
        .iter()
        .position(|state| state.state == definition.default_state)
        .ok_or_else(|| ContentError::InvalidBlockState {
            block: block.clone(),
            reason: "default state is absent from the discrete palette",
        })?;
    let default_state_index =
        u16::try_from(default_state_index).map_err(|_| ContentError::LimitExceeded {
            resource: "block_default_state_index",
            actual: default_state_index,
            limit: usize::from(u16::MAX),
        })?;
    Ok(ValidatedBlockDefinitionV1 {
        definition,
        default_state_index,
    })
}

fn validate_rule_references(definition: &BlockDefinitionV1) -> ContentResult<()> {
    validate_optional_contract_reference(
        definition.rules.mining_rule.as_ref(),
        "mining-rule",
        "block mining rule",
    )?;
    validate_optional_contract_reference(
        definition.rules.drop_table.as_ref(),
        "drop-table",
        "block drop table",
    )?;
    validate_optional_exact_reference(
        definition.rules.placement_item.as_ref(),
        "item",
        "block placement item",
    )
}

fn validate_optional_contract_reference(
    reference: Option<&StableId>,
    expected_kind: &'static str,
    context: &'static str,
) -> ContentResult<()> {
    if let Some(reference) = reference {
        if reference.kind() != expected_kind {
            return Err(ContentError::WrongIdentityKind {
                context,
                id: reference.clone(),
                expected: expected_kind,
                actual: reference.kind().to_owned(),
            });
        }
        if reference.major().is_none() {
            return Err(ContentError::ContractIdentityMissingMajor {
                context,
                id: reference.clone(),
            });
        }
    }
    Ok(())
}

fn validate_semantics(
    block: &StableId,
    semantics: &BlockStateSemanticsV1,
    schema: &[BlockStatePropertyV1],
) -> ContentResult<()> {
    validate_policy_reference(
        semantics.solid_occupancy.as_stable_id(),
        "block solid occupancy",
    )?;
    validate_policy_reference(semantics.collision.as_stable_id(), "block collision shape")?;
    validate_policy_reference(semantics.selection.as_stable_id(), "block selection shape")?;
    validate_policy_reference(semantics.occlusion.as_stable_id(), "block occlusion")?;
    validate_policy_reference(
        semantics.replaceability.as_stable_id(),
        "block replaceability",
    )?;
    validate_policy_reference(
        semantics.fluid_occupancy.as_stable_id(),
        "block fluid occupancy",
    )?;

    let mut form_properties = schema
        .iter()
        .filter(|property| property.value_kind == crate::BlockStateValueKindV1::Form);
    if let Some(property) = form_properties.next() {
        if form_properties.next().is_some() {
            return Err(ContentError::InvalidBlockState {
                block: block.clone(),
                reason: "a definition may declare at most one form property",
            });
        }
        let Some(BlockStateValueV1::Form(form)) = semantics.state.values.get(&property.key) else {
            return Err(ContentError::InvalidBlockState {
                block: block.clone(),
                reason: "form property did not contain a form value",
            });
        };
        if semantics.form.as_ref() != Some(form) {
            return Err(ContentError::InvalidBlockSemantics {
                block: block.clone(),
                reason: "form state and intrinsic form semantics disagree",
            });
        }
    }
    Ok(())
}

fn validate_policy_reference(id: &StableId, context: &'static str) -> ContentResult<()> {
    if id.major().is_none() {
        return Err(ContentError::ContractIdentityMissingMajor {
            context,
            id: id.clone(),
        });
    }
    Ok(())
}
