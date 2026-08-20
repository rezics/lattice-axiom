use std::num::{NonZeroU16, NonZeroU32};

use latticeaxiom_core::{SchemaId, StableId};
use serde::{Deserialize, Deserializer, Serialize, de};

use crate::{
    BIOME_DEFINITION_SCHEMA_V1, ContentError, ContentHeaderV1, ContentResult, ContentRevisionV1,
    header::{validate_exact_id, validate_header},
};

/// Package-owned versioned biome scope contract.
///
/// The content crate keeps this identity typed and versioned without inventing
/// temperature/humidity categories that ADR 0028 does not define.
#[repr(transparent)]
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct BiomeScopeV1(StableId);

impl BiomeScopeV1 {
    /// Creates a versioned scope reference.
    ///
    /// # Errors
    ///
    /// Returns an error when the identity has no contract major.
    pub fn new(contract: StableId) -> ContentResult<Self> {
        validate_versioned_reference(&contract, "biome scope")?;
        Ok(Self(contract))
    }

    /// Returns the scope contract identity.
    #[must_use]
    pub const fn as_stable_id(&self) -> &StableId {
        &self.0
    }
}

impl<'de> Deserialize<'de> for BiomeScopeV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let contract = StableId::deserialize(deserializer)?;
        Self::new(contract).map_err(de::Error::custom)
    }
}

/// Finite spatial influence of a biome program, expressed in world cells.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SpatialInfluenceV1 {
    /// Maximum horizontal influence radius.
    pub horizontal_cells: NonZeroU16,
    /// Maximum vertical influence radius.
    pub vertical_cells: NonZeroU16,
}

/// Optional package-owned territory-participation contract.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TerritoryParticipationV1 {
    /// `None` opts out; `Some` selects a versioned participation contract.
    pub contract: Option<StableId>,
}

/// One bounded typed channel offered by a biome.
#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BiomeChannelOfferV1 {
    /// Package-owned versioned channel identity.
    pub channel: StableId,
    /// Schema of each emitted channel value.
    pub value_schema: SchemaId,
    /// Versioned deterministic compositor contract.
    pub compositor: StableId,
    /// Maximum values emitted for one chunk.
    pub max_values_per_chunk: NonZeroU32,
}

/// One bounded typed authoritative intent emitted by a biome.
#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BiomeEmittedIntentV1 {
    /// Package-owned versioned intent identity.
    pub intent: StableId,
    /// Schema of each emitted intent payload.
    pub payload_schema: SchemaId,
    /// Maximum intents emitted for one chunk.
    pub max_per_chunk: NonZeroU32,
}

/// Authored v1 biome definition.
///
/// Concrete algorithms and balance values remain package-owned. This contract
/// freezes their typed references, bounded spatial influence, offers, intents,
/// fallback edge, and generation revision.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BiomeDefinitionV1 {
    /// Shared owner-aware header.
    pub header: ContentHeaderV1,
    /// Versioned biome scope contract.
    pub scope: BiomeScopeV1,
    /// Finite area that the program may influence.
    pub spatial_influence: SpatialInfluenceV1,
    /// Optional versioned territory participation.
    pub territory_participation: TerritoryParticipationV1,
    /// Versioned boundary behavior contract.
    pub boundary_contract: StableId,
    /// Bounded provided channels.
    pub channel_offers: Vec<BiomeChannelOfferV1>,
    /// Bounded typed emitted intents.
    pub emitted_intents: Vec<BiomeEmittedIntentV1>,
    /// Optional exact fallback biome.
    pub fallback: Option<StableId>,
    /// Owner-managed world-generation algorithm revision.
    pub generation_revision: ContentRevisionV1,
}

pub(crate) fn normalize_biome_definition(
    mut definition: BiomeDefinitionV1,
    max_spatial_cells: u16,
    max_channel_offers: usize,
    max_emitted_intents: usize,
    max_emissions_per_chunk: u32,
) -> ContentResult<BiomeDefinitionV1> {
    validate_header(&definition.header, "biome", BIOME_DEFINITION_SCHEMA_V1)?;
    let biome = &definition.header.stable_id;
    validate_versioned_reference(definition.scope.as_stable_id(), "biome scope")?;
    if let Some(contract) = &definition.territory_participation.contract {
        validate_versioned_reference(contract, "biome territory participation")?;
    }
    validate_versioned_reference(&definition.boundary_contract, "biome boundary contract")?;
    if let Some(fallback) = &definition.fallback {
        validate_exact_id(fallback, "biome", "biome fallback")?;
    }
    bounded_u16(
        "biome_horizontal_influence",
        definition.spatial_influence.horizontal_cells.get(),
        max_spatial_cells,
    )?;
    bounded_u16(
        "biome_vertical_influence",
        definition.spatial_influence.vertical_cells.get(),
        max_spatial_cells,
    )?;
    if definition.channel_offers.len() > max_channel_offers {
        return Err(ContentError::LimitExceeded {
            resource: "biome_channel_offers",
            actual: definition.channel_offers.len(),
            limit: max_channel_offers,
        });
    }
    if definition.emitted_intents.len() > max_emitted_intents {
        return Err(ContentError::LimitExceeded {
            resource: "biome_emitted_intents",
            actual: definition.emitted_intents.len(),
            limit: max_emitted_intents,
        });
    }

    definition.channel_offers.sort();
    for offer in &definition.channel_offers {
        validate_versioned_reference(&offer.channel, "biome channel")?;
        validate_versioned_reference(&offer.compositor, "biome channel compositor")?;
        bounded_u32(
            "biome_channel_values_per_chunk",
            offer.max_values_per_chunk.get(),
            max_emissions_per_chunk,
        )?;
    }
    for pair in definition.channel_offers.windows(2) {
        if pair[0].channel == pair[1].channel {
            return Err(ContentError::InvalidBiome {
                biome: biome.clone(),
                reason: "a channel may be offered only once",
            });
        }
    }

    definition.emitted_intents.sort();
    for intent in &definition.emitted_intents {
        validate_versioned_reference(&intent.intent, "biome emitted intent")?;
        bounded_u32(
            "biome_intents_per_chunk",
            intent.max_per_chunk.get(),
            max_emissions_per_chunk,
        )?;
    }
    for pair in definition.emitted_intents.windows(2) {
        if pair[0].intent == pair[1].intent {
            return Err(ContentError::InvalidBiome {
                biome: biome.clone(),
                reason: "an intent may be declared only once",
            });
        }
    }
    Ok(definition)
}

fn validate_versioned_reference(id: &StableId, context: &'static str) -> ContentResult<()> {
    if id.major().is_none() {
        return Err(ContentError::ContractIdentityMissingMajor {
            context,
            id: id.clone(),
        });
    }
    Ok(())
}

fn bounded_u16(resource: &'static str, actual: u16, limit: u16) -> ContentResult<()> {
    if actual > limit {
        return Err(ContentError::LimitExceeded {
            resource,
            actual: usize::from(actual),
            limit: usize::from(limit),
        });
    }
    Ok(())
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
