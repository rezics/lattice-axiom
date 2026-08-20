use std::collections::{BTreeMap, BTreeSet};

use latticeaxiom_core::{StableId, canonical_json_bytes};
use serde::{Deserialize, Deserializer, Serialize, de};

use crate::{BlockFormV1, ContentError, ContentResult};

/// Shared major-1 block-state key for log and pillar axes.
pub const AXIS_STATE_KEY_V1: &str = "latticeaxiom:block-state/axis@1";
/// Shared major-1 block-state key for six Y-up directions.
pub const FACING_STATE_KEY_V1: &str = "latticeaxiom:block-state/facing@1";
/// Shared major-1 block-state key for an authoritative boolean lit state.
pub const LIT_STATE_KEY_V1: &str = "latticeaxiom:block-state/lit@1";

/// Canonical axis values in Bevy-native Y-up coordinates.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum AxisV1 {
    /// X axis.
    X,
    /// Y axis, the vertical axis.
    Y,
    /// Z axis.
    Z,
}

/// Canonical six-way facing values in Bevy-native Y-up coordinates.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum FacingV1 {
    /// Positive X.
    East,
    /// Negative X.
    West,
    /// Positive Y.
    Up,
    /// Negative Y.
    Down,
    /// Positive Z.
    South,
    /// Negative Z.
    North,
}

/// Definition-scoped finite half token.
///
/// Values such as `lower` and `upper` are owned by the definition. This type
/// deliberately does not promote them to a shared platform state vocabulary.
#[repr(transparent)]
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct BlockHalfV1(String);

impl BlockHalfV1 {
    /// Creates a bounded definition-scoped half token.
    ///
    /// # Errors
    ///
    /// Returns an error when `value` is not a lower-kebab ASCII token.
    pub fn new(value: impl Into<String>) -> ContentResult<Self> {
        definition_token(value, "block half").map(Self)
    }

    /// Returns the definition-scoped token.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for BlockHalfV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::new(value).map_err(de::Error::custom)
    }
}

/// Definition-scoped finite variant token.
#[repr(transparent)]
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct BlockStateVariantV1(String);

impl BlockStateVariantV1 {
    /// Creates a bounded definition-scoped variant token.
    ///
    /// # Errors
    ///
    /// Returns an error when `value` is not a lower-kebab ASCII token.
    pub fn new(value: impl Into<String>) -> ContentResult<Self> {
        definition_token(value, "block state variant").map(Self)
    }

    /// Returns the definition-scoped token.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for BlockStateVariantV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::new(value).map_err(de::Error::custom)
    }
}

/// Closed value family of one block-state property.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum BlockStateValueKindV1 {
    /// One of the three canonical axes.
    Axis,
    /// One of the six canonical Y-up directions.
    Facing,
    /// Boolean value.
    Bool,
    /// Bounded unsigned byte whose allowed subset is declared per definition.
    UnsignedByte,
    /// Lower or upper half.
    Half,
    /// Finite block form proven by the owning definition.
    Form,
    /// Definition-scoped versioned enum member.
    Variant,
}

/// One finite, canonically encoded block-state value.
#[derive(Clone, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "kebab-case",
    deny_unknown_fields
)]
pub enum BlockStateValueV1 {
    /// Axis value.
    Axis(AxisV1),
    /// Facing value.
    Facing(FacingV1),
    /// Boolean value.
    Bool(bool),
    /// Definition-bounded unsigned value, suitable for finite growth stages.
    UnsignedByte(u8),
    /// Lower or upper half.
    Half(BlockHalfV1),
    /// Finite form value.
    Form(BlockFormV1),
    /// Versioned definition-scoped enum member.
    Variant(BlockStateVariantV1),
}

impl BlockStateValueV1 {
    /// Returns the closed family of this value.
    #[must_use]
    pub const fn value_kind(&self) -> BlockStateValueKindV1 {
        match self {
            Self::Axis(_) => BlockStateValueKindV1::Axis,
            Self::Facing(_) => BlockStateValueKindV1::Facing,
            Self::Bool(_) => BlockStateValueKindV1::Bool,
            Self::UnsignedByte(_) => BlockStateValueKindV1::UnsignedByte,
            Self::Half(_) => BlockStateValueKindV1::Half,
            Self::Form(_) => BlockStateValueKindV1::Form,
            Self::Variant(_) => BlockStateValueKindV1::Variant,
        }
    }
}

/// Per-definition declaration of one supported state key.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BlockStatePropertyV1 {
    /// Versioned block-state key.
    pub key: StableId,
    /// Required value family.
    pub value_kind: BlockStateValueKindV1,
    /// Complete allowed subset for this definition.
    pub allowed_values: Vec<BlockStateValueV1>,
    /// Materialized default value.
    pub default_value: BlockStateValueV1,
}

/// One concrete block state represented only by supported stable keys.
#[derive(Clone, Debug, Default, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BlockStateV1 {
    /// Stable state key to finite value mapping.
    pub values: BTreeMap<StableId, BlockStateValueV1>,
}

impl BlockStateV1 {
    /// Creates the stateless canonical block state.
    #[must_use]
    pub const fn empty() -> Self {
        Self {
            values: BTreeMap::new(),
        }
    }

    /// Returns deterministic compact JSON bytes used as the state sort key.
    ///
    /// # Errors
    ///
    /// Returns an error if canonical JSON encoding fails.
    pub fn canonical_bytes(&self) -> ContentResult<Vec<u8>> {
        canonical_json_bytes(self).map_err(ContentError::from)
    }
}

pub(crate) fn normalize_state_schema(
    block: &StableId,
    properties: &[BlockStatePropertyV1],
    max_properties: usize,
    max_values_per_property: usize,
) -> ContentResult<Vec<BlockStatePropertyV1>> {
    if properties.len() > max_properties {
        return Err(ContentError::LimitExceeded {
            resource: "block_state_properties",
            actual: properties.len(),
            limit: max_properties,
        });
    }
    for property in properties {
        if property.allowed_values.len() > max_values_per_property {
            return Err(ContentError::LimitExceeded {
                resource: "block_state_allowed_values",
                actual: property.allowed_values.len(),
                limit: max_values_per_property,
            });
        }
    }
    let mut normalized = properties.to_vec();
    normalized.sort_by(|left, right| left.key.cmp(&right.key));
    for pair in normalized.windows(2) {
        if pair[0].key == pair[1].key {
            return Err(ContentError::Duplicate {
                resource: "block_state_property",
                id: pair[0].key.to_string(),
            });
        }
    }
    for property in &mut normalized {
        validate_property(block, property, max_values_per_property)?;
        property.allowed_values.sort();
    }
    Ok(normalized)
}

fn validate_property(
    block: &StableId,
    property: &BlockStatePropertyV1,
    max_values: usize,
) -> ContentResult<()> {
    if property.key.kind() != "block-state" || property.key.major().is_none() {
        return Err(ContentError::InvalidStateProperty {
            block: block.clone(),
            key: property.key.clone(),
            reason: "state key must be a versioned `block-state` identity",
        });
    }
    if property.allowed_values.is_empty() {
        return Err(ContentError::InvalidStateProperty {
            block: block.clone(),
            key: property.key.clone(),
            reason: "allowed subset must not be empty",
        });
    }
    if property.allowed_values.len() > max_values {
        return Err(ContentError::LimitExceeded {
            resource: "block_state_allowed_values",
            actual: property.allowed_values.len(),
            limit: max_values,
        });
    }
    if property.default_value.value_kind() != property.value_kind {
        return Err(ContentError::InvalidStateProperty {
            block: block.clone(),
            key: property.key.clone(),
            reason: "default value family does not match the property",
        });
    }
    let mut values = BTreeSet::new();
    for value in &property.allowed_values {
        if value.value_kind() != property.value_kind {
            return Err(ContentError::InvalidStateProperty {
                block: block.clone(),
                key: property.key.clone(),
                reason: "allowed value family does not match the property",
            });
        }
        if !values.insert(value) {
            return Err(ContentError::InvalidStateProperty {
                block: block.clone(),
                key: property.key.clone(),
                reason: "allowed subset contains a duplicate value",
            });
        }
    }
    if !values.contains(&property.default_value) {
        return Err(ContentError::InvalidStateProperty {
            block: block.clone(),
            key: property.key.clone(),
            reason: "default value is outside the allowed subset",
        });
    }
    validate_shared_key_meaning(block, property)
}

pub(crate) fn definition_token(
    value: impl Into<String>,
    context: &'static str,
) -> ContentResult<String> {
    let value = value.into();
    if value.is_empty() || value.len() > 64 {
        return Err(ContentError::InvalidDefinitionToken {
            context,
            value,
            reason: "token length must be in 1..=64 bytes",
        });
    }
    let bytes = value.as_bytes();
    let edge_is_alphanumeric = |byte: u8| byte.is_ascii_lowercase() || byte.is_ascii_digit();
    if !edge_is_alphanumeric(bytes[0]) || !edge_is_alphanumeric(bytes[bytes.len() - 1]) {
        return Err(ContentError::InvalidDefinitionToken {
            context,
            value,
            reason: "token must begin and end with a lowercase ASCII letter or digit",
        });
    }
    if !bytes
        .iter()
        .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'-')
    {
        return Err(ContentError::InvalidDefinitionToken {
            context,
            value,
            reason: "token may contain only lowercase ASCII letters, digits, or `-`",
        });
    }
    if value.split('-').any(str::is_empty) {
        return Err(ContentError::InvalidDefinitionToken {
            context,
            value,
            reason: "lower-kebab tokens must not contain empty segments",
        });
    }
    Ok(value)
}

fn validate_shared_key_meaning(
    block: &StableId,
    property: &BlockStatePropertyV1,
) -> ContentResult<()> {
    let expected = match property.key.as_str() {
        AXIS_STATE_KEY_V1 => Some(BlockStateValueKindV1::Axis),
        FACING_STATE_KEY_V1 => Some(BlockStateValueKindV1::Facing),
        LIT_STATE_KEY_V1 => Some(BlockStateValueKindV1::Bool),
        _ => None,
    };
    if expected.is_some_and(|kind| kind != property.value_kind) {
        return Err(ContentError::InvalidStateProperty {
            block: block.clone(),
            key: property.key.clone(),
            reason: "shared state key uses the wrong frozen value family",
        });
    }
    Ok(())
}

pub(crate) fn validate_state(
    block: &StableId,
    state: &BlockStateV1,
    schema: &[BlockStatePropertyV1],
) -> ContentResult<()> {
    if state.values.len() != schema.len() {
        return Err(ContentError::InvalidBlockState {
            block: block.clone(),
            reason: "state must contain every supported key exactly once",
        });
    }
    for property in schema {
        let Some(value) = state.values.get(&property.key) else {
            return Err(ContentError::InvalidBlockState {
                block: block.clone(),
                reason: "state is missing a supported key",
            });
        };
        if !property.allowed_values.contains(value) {
            return Err(ContentError::InvalidBlockState {
                block: block.clone(),
                reason: "state value is outside the definition's allowed subset",
            });
        }
    }
    Ok(())
}
