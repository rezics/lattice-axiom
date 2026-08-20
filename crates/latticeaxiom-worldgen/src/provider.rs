use std::{collections::BTreeMap, num::NonZeroU32};

use latticeaxiom_core::{CanonicalHash, StableId, canonical_json_bytes};
use serde::{Deserialize, Serialize};

use crate::{
    GeneratorFingerprintV1, WorldgenError, WorldgenLimitsV1, WorldgenResult, hashes::domain_hash,
};

const GENERATOR_FINGERPRINT_DOMAIN: &[u8] = b"latticeaxiom.generator-fingerprint.v1\0";

/// Exclusive provider slots required by the minimal D4 generation DAG.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ProviderSlotV1 {
    /// Per-dimension compiler of ownership, budgets, and boundaries.
    GenerationCoordinator,
    /// Coarse deterministic two-style selector with the D7 query shape.
    StyleSelector,
    /// Primary terrain owner for temperate woodland domains.
    TemperateTerrain,
    /// Primary terrain owner for arid badlands domains.
    AridTerrain,
    /// Named deterministic transition between the two terrain styles.
    TerrainTransition,
    /// Primary minimal cave topology/void owner.
    CaveTopology,
    /// Role-driven final occupancy materializer.
    Materializer,
}

impl ProviderSlotV1 {
    /// Canonical order of required D4 exclusive slots.
    pub const ALL: [Self; 7] = [
        Self::GenerationCoordinator,
        Self::StyleSelector,
        Self::TemperateTerrain,
        Self::AridTerrain,
        Self::TerrainTransition,
        Self::CaveTopology,
        Self::Materializer,
    ];

    /// Returns the stable channel/domain label.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::GenerationCoordinator => "generation.coordinator/dimension",
            Self::StyleSelector => "territory.selector/dimension",
            Self::TemperateTerrain => "terrain.base/temperate-woodland",
            Self::AridTerrain => "terrain.base/arid-badlands",
            Self::TerrainTransition => "terrain.boundary/woodland-badlands",
            Self::CaveTopology => "cave.topology/dimension-default",
            Self::Materializer => "terrain.materializer/dimension",
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
    fingerprint: GeneratorFingerprintV1,
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
            let mut candidates = grouped.remove(&slot).unwrap_or_default();
            candidates.sort();
            match candidates.as_slice() {
                [] => return Err(WorldgenError::MissingProvider { slot }),
                [identity] => {
                    identities.insert(slot, identity.clone());
                }
                _ => {
                    return Err(WorldgenError::ConflictingProviders {
                        slot,
                        providers: candidates
                            .into_iter()
                            .map(|candidate| candidate.provider_stable_id)
                            .collect(),
                    });
                }
            }
        }

        let canonical = canonical_json_bytes(&identities).map_err(|error| {
            WorldgenError::CanonicalEncoding {
                kind: "resolved provider identities",
                reason: error.to_string(),
            }
        })?;
        let fingerprint = GeneratorFingerprintV1::from_hash(domain_hash(
            GENERATOR_FINGERPRINT_DOMAIN,
            &[canonical.as_slice()],
        ));
        Ok(Self {
            identities,
            fingerprint,
        })
    }

    pub(crate) fn identity(&self, slot: ProviderSlotV1) -> &ProviderGenerationIdentityV1 {
        self.identities
            .get(&slot)
            .unwrap_or_else(|| missing_resolved_provider(slot))
    }

    pub(crate) fn ordered(&self) -> Vec<(ProviderSlotV1, ProviderGenerationIdentityV1)> {
        ProviderSlotV1::ALL
            .into_iter()
            .map(|slot| (slot, self.identity(slot).clone()))
            .collect()
    }

    pub(crate) const fn fingerprint(&self) -> GeneratorFingerprintV1 {
        self.fingerprint
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
