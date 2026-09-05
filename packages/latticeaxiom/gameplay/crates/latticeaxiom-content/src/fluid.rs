use std::num::{NonZeroU16, NonZeroU32};

use latticeaxiom_core::{SchemaId, StableId};
use serde::{Deserialize, Deserializer, Serialize, de};

use crate::{
    ContentError, ContentHeaderV1, ContentResult, FLUID_DEFINITION_SCHEMA_V1,
    header::{validate_contract_id, validate_header, validate_optional_exact_reference},
};

#[path = "fluid_semantics.rs"]
pub mod fluid_semantics;
#[path = "solid_fluid_volume.rs"]
pub mod solid_fluid_volume;

/// Frozen v1 fluid level in the inclusive range `0..=7`.
///
/// Level zero is a full/source cell. Increasing values represent decreasing
/// fill. The level belongs only to [`FluidStateV1`], never to a fluid
/// definition or identity.
#[repr(transparent)]
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct FluidLevelV1(u8);

impl FluidLevelV1 {
    /// Full/source fluid level.
    pub const SOURCE: Self = Self(0);
    /// Greatest valid v1 level value.
    pub const MAX: u8 = 7;

    /// Creates a validated v1 level.
    ///
    /// # Errors
    ///
    /// Returns [`crate::FluidLevelError`] when `value` is greater than seven.
    pub const fn new(value: u8) -> Result<Self, crate::FluidLevelError> {
        if value <= Self::MAX {
            Ok(Self(value))
        } else {
            Err(crate::FluidLevelError { actual: value })
        }
    }

    /// Returns the canonical numeric level.
    #[must_use]
    pub const fn get(self) -> u8 {
        self.0
    }

    /// Returns whether this is a full/source state.
    #[must_use]
    pub const fn is_source(self) -> bool {
        self.0 == 0
    }
}

impl TryFrom<u8> for FluidLevelV1 {
    type Error = crate::FluidLevelError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl From<FluidLevelV1> for u8 {
    fn from(value: FluidLevelV1) -> Self {
        value.get()
    }
}

impl<'de> Deserialize<'de> for FluidLevelV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = u8::deserialize(deserializer)?;
        Self::new(value).map_err(de::Error::custom)
    }
}

/// Frozen v1 authoritative flow direction.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum FluidFlowV1 {
    /// No directional flow.
    Still,
    /// Negative Y flow.
    Down,
    /// Positive X flow.
    East,
    /// Negative X flow.
    West,
    /// Positive Z flow.
    South,
    /// Negative Z flow.
    North,
}

impl FluidFlowV1 {
    /// Closed v1 flow set in canonical order.
    pub const ALL: [Self; 6] = [
        Self::Still,
        Self::Down,
        Self::East,
        Self::West,
        Self::South,
        Self::North,
    ];
}

/// Complete authoritative v1 per-cell fluid state.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FluidStateV1 {
    /// Full/source level zero through thinnest level seven.
    pub level: FluidLevelV1,
    /// Explicit flow; presentation adapters must not infer it.
    pub flow: FluidFlowV1,
}

/// Versioned collision-policy reference without prematurely freezing policy
/// discriminants in the content contract.
#[repr(transparent)]
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct FluidCollisionPolicyV1(StableId);

impl FluidCollisionPolicyV1 {
    /// Creates a versioned `fluid-collision-policy` reference.
    ///
    /// # Errors
    ///
    /// Returns an error for the wrong registration kind or a missing major.
    pub fn new(id: StableId) -> ContentResult<Self> {
        validate_contract_id(&id, "fluid-collision-policy", "fluid collision policy")?;
        Ok(Self(id))
    }

    /// Returns the policy contract identity.
    #[must_use]
    pub const fn as_stable_id(&self) -> &StableId {
        &self.0
    }
}

impl<'de> Deserialize<'de> for FluidCollisionPolicyV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let id = StableId::deserialize(deserializer)?;
        Self::new(id).map_err(de::Error::custom)
    }
}

/// Versioned selection-policy reference without prematurely freezing policy
/// discriminants in the content contract.
#[repr(transparent)]
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct FluidSelectionPolicyV1(StableId);

impl FluidSelectionPolicyV1 {
    /// Creates a versioned `fluid-selection-policy` reference.
    ///
    /// # Errors
    ///
    /// Returns an error for the wrong registration kind or a missing major.
    pub fn new(id: StableId) -> ContentResult<Self> {
        validate_contract_id(&id, "fluid-selection-policy", "fluid selection policy")?;
        Ok(Self(id))
    }

    /// Returns the policy contract identity.
    #[must_use]
    pub const fn as_stable_id(&self) -> &StableId {
        &self.0
    }
}

impl<'de> Deserialize<'de> for FluidSelectionPolicyV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let id = StableId::deserialize(deserializer)?;
        Self::new(id).map_err(de::Error::custom)
    }
}

/// Hard per-definition work bounds for a fixed fluid update tick.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BoundedFluidUpdatePolicyV1 {
    /// Maximum cells examined or changed in one tick.
    pub max_cells_per_tick: NonZeroU32,
    /// Maximum queued frontier entries.
    pub max_queue_depth: NonZeroU32,
    /// Maximum input plus output bytes in flight.
    pub max_in_flight_bytes: NonZeroU32,
}

/// Authored v1 fluid definition.
///
/// Per-cell `level` and `flow` are deliberately absent; they exist only in
/// [`FluidStateV1`]. Upward/diagonal flow, pressure, mixing, and unbounded
/// propagation are outside this version.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FluidDefinitionV1 {
    /// Shared owner-aware header.
    pub header: ContentHeaderV1,
    /// Explicit frozen per-cell state schema.
    pub state_schema: SchemaId,
    /// Versioned predicate deciding authoritative replacement/displacement.
    pub replace_or_displace_predicate: StableId,
    /// Versioned collision policy.
    pub collision_policy: FluidCollisionPolicyV1,
    /// Versioned selection policy.
    pub selection_policy: FluidSelectionPolicyV1,
    /// Fixed simulation tick period in authoritative ticks.
    pub fixed_tick_period: NonZeroU16,
    /// Per-tick queue, cell, and byte bounds.
    pub update_policy: BoundedFluidUpdatePolicyV1,
    /// Optional exact presentation asset; excluded from authoritative hash.
    pub presentation_binding: Option<StableId>,
}

pub(crate) fn validate_fluid_definition(
    definition: &FluidDefinitionV1,
    max_tick_period: u16,
    max_cells_per_tick: u32,
    max_queue_depth: u32,
    max_in_flight_bytes: u32,
) -> ContentResult<()> {
    validate_header(&definition.header, "fluid", FLUID_DEFINITION_SCHEMA_V1)?;
    if definition.state_schema.as_str() != crate::FLUID_STATE_SCHEMA_V1 {
        return Err(ContentError::WrongDefinitionSchema {
            kind: "fluid state",
            id: definition.header.stable_id.clone(),
            expected: crate::FLUID_STATE_SCHEMA_V1,
            actual: definition.state_schema.to_string(),
        });
    }
    validate_contract_id(
        &definition.replace_or_displace_predicate,
        "predicate",
        "fluid replace-or-displace predicate",
    )?;
    validate_contract_id(
        definition.collision_policy.as_stable_id(),
        "fluid-collision-policy",
        "fluid collision policy",
    )?;
    validate_contract_id(
        definition.selection_policy.as_stable_id(),
        "fluid-selection-policy",
        "fluid selection policy",
    )?;
    validate_optional_exact_reference(
        definition.presentation_binding.as_ref(),
        "asset",
        "fluid presentation binding",
    )?;
    bounded_u32(
        "fluid_tick_period",
        u32::from(definition.fixed_tick_period.get()),
        u32::from(max_tick_period),
    )?;
    bounded_u32(
        "fluid_cells_per_tick",
        definition.update_policy.max_cells_per_tick.get(),
        max_cells_per_tick,
    )?;
    bounded_u32(
        "fluid_queue_depth",
        definition.update_policy.max_queue_depth.get(),
        max_queue_depth,
    )?;
    bounded_u32(
        "fluid_in_flight_bytes",
        definition.update_policy.max_in_flight_bytes.get(),
        max_in_flight_bytes,
    )
}

fn bounded_u32(resource: &'static str, actual: u32, limit: u32) -> ContentResult<()> {
    if actual > limit {
        return Err(ContentError::LimitExceeded {
            resource,
            actual: usize::try_from(actual).unwrap_or(usize::MAX),
            limit: usize::try_from(limit).unwrap_or(usize::MAX),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fluid_level_rejects_eight() {
        assert_eq!(
            FluidLevelV1::new(8),
            Err(crate::FluidLevelError { actual: 8 })
        );
    }

    #[test]
    fn fluid_level_json_revalidates_range() {
        let decoded = serde_json::from_str::<FluidLevelV1>("7")
            .unwrap_or_else(|error| panic!("level seven JSON failed: {error}"));
        let expected = FluidLevelV1::new(7)
            .unwrap_or_else(|error| panic!("level seven constructor failed: {error}"));
        assert_eq!(decoded, expected);
        assert!(serde_json::from_str::<FluidLevelV1>("8").is_err());
    }
}
