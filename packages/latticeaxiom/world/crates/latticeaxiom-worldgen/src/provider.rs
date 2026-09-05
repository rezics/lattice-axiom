use std::{collections::BTreeMap, num::NonZeroU32};

use latticeaxiom_core::{CanonicalHash, StableId, canonical_json_bytes};
use serde::{Deserialize, Serialize};

use crate::{WorldgenError, WorldgenLimitsV1, WorldgenResult};

/// Exclusive dimension-wide provider slots required by the generation DAG.
///
/// Surface-biome terrain providers are deliberately not slots: packages bind
/// them through `SurfaceBiomeTerrainProgramV1` after ecological ownership is
/// resolved.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ProviderSlotV1 {
    /// Per-dimension compiler of ownership, budgets, and boundaries.
    GenerationCoordinator,
    /// Coarse deterministic two-style selector with the D7 query shape.
    StyleSelector,
    /// Named deterministic transition between the two terrain styles.
    TerrainTransition,
    /// Primary minimal cave topology/void owner.
    CaveTopology,
    /// Role-driven final occupancy materializer.
    Materializer,
    /// Queryable strata and geologic-field owner used by the V5 natural layer.
    Geology,
    /// Surface river and basin planner used by the V5 natural layer.
    Hydrology,
    /// Stable ore-field owner used by the V5 natural layer.
    Resources,
    /// Exclusion-radius vegetation owner used by the V5 natural layer.
    Vegetation,
}

impl ProviderSlotV1 {
    /// Canonical order of required D4 exclusive slots.
    pub const ALL: [Self; 5] = [
        Self::GenerationCoordinator,
        Self::StyleSelector,
        Self::TerrainTransition,
        Self::CaveTopology,
        Self::Materializer,
    ];

    /// Exclusive V5 natural-layer slots. Absent from D4 plans.
    pub const NATURAL: [Self; 4] = [
        Self::Geology,
        Self::Hydrology,
        Self::Resources,
        Self::Vegetation,
    ];

    /// Returns the stable channel/domain label.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::GenerationCoordinator => "generation.coordinator/dimension",
            Self::StyleSelector => "territory.selector/dimension",
            Self::TerrainTransition => "terrain.boundary/woodland-badlands",
            Self::CaveTopology => "cave.topology/dimension-default",
            Self::Materializer => "terrain.materializer/dimension",
            Self::Geology => "geology.strata/dimension",
            Self::Hydrology => "hydrology.basin/dimension",
            Self::Resources => "terrain.resource/dimension",
            Self::Vegetation => "terrain.vegetation/dimension",
        }
    }
}

impl std::fmt::Display for ProviderSlotV1 {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// Stable output-affecting identity of one generation provider implementation.
#[derive(Clone, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderGenerationIdentityV1 {
    provider_stable_id: StableId,
    contract_major: NonZeroU32,
    algorithm_revision: u32,
    implementation_fingerprint: CanonicalHash,
}

impl ProviderGenerationIdentityV1 {
    /// Creates an output-affecting provider identity.
    #[must_use]
    pub const fn new(
        provider_stable_id: StableId,
        contract_major: NonZeroU32,
        algorithm_revision: u32,
        implementation_fingerprint: CanonicalHash,
    ) -> Self {
        Self {
            provider_stable_id,
            contract_major,
            algorithm_revision,
            implementation_fingerprint,
        }
    }

    /// Returns the provider registration identity.
    #[must_use]
    pub const fn provider_stable_id(&self) -> &StableId {
        &self.provider_stable_id
    }

    /// Returns the provider contract major.
    #[must_use]
    pub const fn contract_major(&self) -> NonZeroU32 {
        self.contract_major
    }

    /// Returns the owner-controlled algorithm revision.
    #[must_use]
    pub const fn algorithm_revision(&self) -> u32 {
        self.algorithm_revision
    }

    /// Returns the verified implementation fingerprint.
    #[must_use]
    pub const fn implementation_fingerprint(&self) -> &CanonicalHash {
        &self.implementation_fingerprint
    }
}

/// One provider candidate for an exclusive D4 channel/domain slot.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderOfferV1 {
    slot: ProviderSlotV1,
    identity: ProviderGenerationIdentityV1,
}

impl ProviderOfferV1 {
    /// Creates an exclusive provider offer.
    #[must_use]
    pub const fn new(slot: ProviderSlotV1, identity: ProviderGenerationIdentityV1) -> Self {
        Self { slot, identity }
    }

    /// Returns the offered slot.
    #[must_use]
    pub const fn slot(&self) -> ProviderSlotV1 {
        self.slot
    }

    /// Returns the output-affecting provider identity.
    #[must_use]
    pub const fn identity(&self) -> &ProviderGenerationIdentityV1 {
        &self.identity
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ResolvedProvidersV1 {
    identities: BTreeMap<ProviderSlotV1, ProviderGenerationIdentityV1>,
}

impl ResolvedProvidersV1 {
    pub(crate) fn resolve(
        offers: Vec<ProviderOfferV1>,
        limits: WorldgenLimitsV1,
    ) -> WorldgenResult<Self> {
        if offers.len() > usize::from(limits.max_provider_offers.get()) {
            return Err(WorldgenError::CollectionLimitExceeded {
                kind: "provider offers",
                actual: offers.len(),
                limit: usize::from(limits.max_provider_offers.get()),
            });
        }

        validate_fingerprint_consistency(&offers)?;
        let mut grouped = BTreeMap::<ProviderSlotV1, Vec<ProviderGenerationIdentityV1>>::new();
        for offer in offers {
            grouped.entry(offer.slot).or_default().push(offer.identity);
        }

        let mut identities = BTreeMap::new();
        for slot in ProviderSlotV1::ALL {
            insert_exclusive_slot(&mut identities, slot, grouped.remove(&slot), true)?;
        }
        for slot in ProviderSlotV1::NATURAL {
            insert_exclusive_slot(&mut identities, slot, grouped.remove(&slot), false)?;
        }

        canonical_json_bytes(&identities).map_err(|error| WorldgenError::CanonicalEncoding {
            kind: "resolved provider identities",
            reason: error.to_string(),
        })?;
        Ok(Self { identities })
    }

    pub(crate) fn identity(&self, slot: ProviderSlotV1) -> &ProviderGenerationIdentityV1 {
        self.identities
            .get(&slot)
            .unwrap_or_else(|| missing_resolved_provider(slot))
    }

    pub(crate) fn try_identity(
        &self,
        slot: ProviderSlotV1,
    ) -> Option<&ProviderGenerationIdentityV1> {
        self.identities.get(&slot)
    }

    pub(crate) fn ordered(&self) -> Vec<(ProviderSlotV1, ProviderGenerationIdentityV1)> {
        ProviderSlotV1::ALL
            .into_iter()
            .chain(ProviderSlotV1::NATURAL)
            .filter_map(|slot| {
                self.identities
                    .get(&slot)
                    .cloned()
                    .map(|identity| (slot, identity))
            })
            .collect()
    }
}

fn insert_exclusive_slot(
    identities: &mut BTreeMap<ProviderSlotV1, ProviderGenerationIdentityV1>,
    slot: ProviderSlotV1,
    candidates: Option<Vec<ProviderGenerationIdentityV1>>,
    required: bool,
) -> WorldgenResult<()> {
    let mut candidates = candidates.unwrap_or_default();
    candidates.sort();
    match candidates.as_slice() {
        [] if required => Err(WorldgenError::MissingProvider { slot }),
        [] => Ok(()),
        [identity] => {
            identities.insert(slot, identity.clone());
            Ok(())
        }
        _ => Err(WorldgenError::ConflictingProviders {
            slot,
            providers: candidates
                .into_iter()
                .map(|candidate| candidate.provider_stable_id)
                .collect(),
        }),
    }
}

fn missing_resolved_provider(slot: ProviderSlotV1) -> &'static ProviderGenerationIdentityV1 {
    panic!("validated provider set lost required slot `{slot}`")
}

fn validate_fingerprint_consistency(offers: &[ProviderOfferV1]) -> WorldgenResult<()> {
    let mut seen = BTreeMap::<(StableId, NonZeroU32, u32), CanonicalHash>::new();
    for offer in offers {
        let identity = &offer.identity;
        let key = (
            identity.provider_stable_id.clone(),
            identity.contract_major,
            identity.algorithm_revision,
        );
        if let Some(previous) = seen.insert(key.clone(), identity.implementation_fingerprint)
            && previous != identity.implementation_fingerprint
        {
            return Err(WorldgenError::ProviderFingerprintConflict {
                provider: key.0,
                contract_major: key.1.get(),
                algorithm_revision: key.2,
            });
        }
    }
    Ok(())
}
