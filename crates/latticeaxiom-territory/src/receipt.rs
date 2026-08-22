//! Finite, seed-stable receipts for compiled territory plans.

use latticeaxiom_core::{CanonicalHash, canonical_json_hash};
use latticeaxiom_worldgen::{DimensionId, ProviderGenerationIdentityV1, WorldSeedV1};
use serde::{Deserialize, Serialize};

use crate::{
    AtlasConfigV1, AtlasPlanHashV1, CaveTopologyDomainIdV1, HydrologyPlanHashV1,
    TerritoryDomainIdV1, TerritoryError, TerritoryPlanReceiptHashV1, TerritoryPlanV1,
    TerritoryResult,
};

#[derive(Serialize)]
struct PlanReceiptHashPayloadV1<'a> {
    plan_hash: AtlasPlanHashV1,
    world_seed: WorldSeedV1,
    dimension: &'a DimensionId,
    coordinator: &'a ProviderGenerationIdentityV1,
    atlas: &'a AtlasConfigV1,
    default_terrain_domain: &'a TerritoryDomainIdV1,
    default_cave_domain: &'a CaveTopologyDomainIdV1,
    hydrology_plan_hash: HydrologyPlanHashV1,
    surface_candidate_count: u32,
    underground_territory_count: u32,
    primary_owner_count: u32,
    contribution_count: u32,
    cave_portal_count: u32,
}

/// Compact, finite identity of a compiled territory plan.
///
/// The receipt names the frozen seed, coordinator, Atlas hierarchy, default
/// domains, and collection sizes. It does not carry the candidate payload.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TerritoryPlanReceiptV1 {
    plan_hash: AtlasPlanHashV1,
    world_seed: WorldSeedV1,
    dimension: DimensionId,
    coordinator: ProviderGenerationIdentityV1,
    atlas: AtlasConfigV1,
    default_terrain_domain: TerritoryDomainIdV1,
    default_cave_domain: CaveTopologyDomainIdV1,
    hydrology_plan_hash: HydrologyPlanHashV1,
    surface_candidate_count: u32,
    underground_territory_count: u32,
    primary_owner_count: u32,
    contribution_count: u32,
    cave_portal_count: u32,
    receipt_hash: TerritoryPlanReceiptHashV1,
}

impl TerritoryPlanReceiptV1 {
    /// Builds a finite receipt from a compiled plan.
    ///
    /// # Errors
    ///
    /// Returns an error if a collection length overflows `u32` or canonical
    /// hashing fails.
    pub fn from_plan(plan: &TerritoryPlanV1) -> TerritoryResult<Self> {
        let receipt = Self {
            plan_hash: plan.plan_hash(),
            world_seed: plan.world_seed(),
            dimension: plan.dimension().clone(),
            coordinator: plan.coordinator().clone(),
            atlas: plan.atlas().clone(),
            default_terrain_domain: plan.default_terrain_domain().clone(),
            default_cave_domain: plan.default_cave_domain().clone(),
            hydrology_plan_hash: plan.hydrology().plan_hash(),
            surface_candidate_count: count_u32(
                "surface candidate count",
                plan.surface_candidates().len(),
            )?,
            underground_territory_count: count_u32(
                "underground territory count",
                plan.underground_territories().len(),
            )?,
            primary_owner_count: count_u32("primary owner count", plan.primary_owners().len())?,
            contribution_count: count_u32("contribution count", plan.contributions().len())?,
            cave_portal_count: count_u32("cave portal count", plan.cave_portals().len())?,
            receipt_hash: TerritoryPlanReceiptHashV1::from_hash(CanonicalHash::digest([])),
        };
        Ok(Self {
            receipt_hash: receipt.recompute_hash()?,
            ..receipt
        })
    }

    /// Returns the compiled Atlas plan hash.
    #[must_use]
    pub const fn plan_hash(&self) -> AtlasPlanHashV1 {
        self.plan_hash
    }

    /// Returns the frozen world seed.
    #[must_use]
    pub const fn world_seed(&self) -> WorldSeedV1 {
        self.world_seed
    }

    /// Returns the planned dimension.
    #[must_use]
    pub const fn dimension(&self) -> &DimensionId {
        &self.dimension
    }

    /// Returns the exclusive generation coordinator.
    #[must_use]
    pub const fn coordinator(&self) -> &ProviderGenerationIdentityV1 {
        &self.coordinator
    }

    /// Returns the validated Atlas hierarchy copied into this receipt.
    #[must_use]
    pub const fn atlas(&self) -> &AtlasConfigV1 {
        &self.atlas
    }

    /// Returns the dimension-default terrain ownership domain.
    #[must_use]
    pub const fn default_terrain_domain(&self) -> &TerritoryDomainIdV1 {
        &self.default_terrain_domain
    }

    /// Returns the dimension-default cave-topology ownership domain.
    #[must_use]
    pub const fn default_cave_domain(&self) -> &CaveTopologyDomainIdV1 {
        &self.default_cave_domain
    }

    /// Returns the abstract hydrology plan hash.
    #[must_use]
    pub const fn hydrology_plan_hash(&self) -> HydrologyPlanHashV1 {
        self.hydrology_plan_hash
    }

    /// Returns the finite surface-candidate count.
    #[must_use]
    pub const fn surface_candidate_count(&self) -> u32 {
        self.surface_candidate_count
    }

    /// Returns the finite underground-territory count.
    #[must_use]
    pub const fn underground_territory_count(&self) -> u32 {
        self.underground_territory_count
    }

    /// Returns the finite exclusive-owner count.
    #[must_use]
    pub const fn primary_owner_count(&self) -> u32 {
        self.primary_owner_count
    }

    /// Returns the finite layered-contribution count.
    #[must_use]
    pub const fn contribution_count(&self) -> u32 {
        self.contribution_count
    }

    /// Returns the finite cave-portal count.
    #[must_use]
    pub const fn cave_portal_count(&self) -> u32 {
        self.cave_portal_count
    }

    /// Returns the canonical receipt hash.
    #[must_use]
    pub const fn receipt_hash(&self) -> TerritoryPlanReceiptHashV1 {
        self.receipt_hash
    }

    /// Revalidates canonical receipt identity after deserialization.
    ///
    /// # Errors
    ///
    /// Returns an error if the stored hash does not match the payload.
    pub fn validate(&self) -> TerritoryResult<()> {
        if self.recompute_hash()? != self.receipt_hash {
            return Err(TerritoryError::InvalidAtlasScale {
                reason: "serialized territory plan receipt hash is stale".to_owned(),
            });
        }
        Ok(())
    }

    fn recompute_hash(&self) -> TerritoryResult<TerritoryPlanReceiptHashV1> {
        canonical_json_hash(&PlanReceiptHashPayloadV1 {
            plan_hash: self.plan_hash,
            world_seed: self.world_seed,
            dimension: &self.dimension,
            coordinator: &self.coordinator,
            atlas: &self.atlas,
            default_terrain_domain: &self.default_terrain_domain,
            default_cave_domain: &self.default_cave_domain,
            hydrology_plan_hash: self.hydrology_plan_hash,
            surface_candidate_count: self.surface_candidate_count,
            underground_territory_count: self.underground_territory_count,
            primary_owner_count: self.primary_owner_count,
            contribution_count: self.contribution_count,
            cave_portal_count: self.cave_portal_count,
        })
        .map(TerritoryPlanReceiptHashV1::from_hash)
        .map_err(|error| TerritoryError::CanonicalEncoding {
            kind: "territory plan receipt",
            reason: error.to_string(),
        })
    }
}

fn count_u32(kind: &'static str, actual: usize) -> TerritoryResult<u32> {
    u32::try_from(actual).map_err(|_| TerritoryError::ArithmeticOverflow { kind })
}
