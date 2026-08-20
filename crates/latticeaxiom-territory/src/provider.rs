//! Provider ownership and bounded contribution contracts.

use std::{collections::BTreeMap, num::NonZeroU32};

use latticeaxiom_core::{CanonicalHash, StableId};
use latticeaxiom_worldgen::ProviderGenerationIdentityV1;
use serde::{Deserialize, Serialize};

use crate::{
    CaveTopologyDomainIdV1, PlanningCellBoundsV1, TerritoryDomainIdV1, TerritoryError,
    TerritoryResult, VerticalRangeV1,
};

/// Hard limits applied before compiling untrusted territory registrations.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TerritoryLimitsV1 {
    /// Maximum surface candidates.
    pub max_surface_candidates: usize,
    /// Maximum candidates examined by one compiled Atlas selector.
    pub max_candidates_per_selector: usize,
    /// Maximum ownership domains.
    pub max_domains: usize,
    /// Maximum layered contributions.
    pub max_contributions: usize,
    /// Maximum underground territories.
    pub max_underground_territories: usize,
    /// Maximum cave portals.
    pub max_portals: usize,
    /// Maximum abstract hydrology basins.
    pub max_hydrology_basins: usize,
    /// Maximum abstract hydrology connections.
    pub max_hydrology_connections: usize,
    /// Maximum cells in one statistics sample.
    pub max_statistics_cells: u64,
    /// Aggregate declared contribution work-unit budget.
    pub max_contribution_work_units: u64,
    /// Aggregate declared contribution memory budget.
    pub max_contribution_memory_bytes: u64,
}

impl Default for TerritoryLimitsV1 {
    fn default() -> Self {
        Self {
            max_surface_candidates: 4_096,
            max_candidates_per_selector: 512,
            max_domains: 512,
            max_contributions: 4_096,
            max_underground_territories: 512,
            max_portals: 8_192,
            max_hydrology_basins: 2_048,
            max_hydrology_connections: 4_096,
            max_statistics_cells: 1_000_000,
            max_contribution_work_units: 10_000_000,
            max_contribution_memory_bytes: 256 * 1024 * 1024,
        }
    }
}

impl TerritoryLimitsV1 {
    #[allow(
        clippy::unused_self,
        reason = "preflight remains anchored to the active limit set"
    )]
    pub(crate) fn check_count(
        self,
        kind: &'static str,
        actual: usize,
        limit: usize,
    ) -> TerritoryResult<()> {
        if actual <= limit {
            Ok(())
        } else {
            Err(TerritoryError::CollectionLimitExceeded {
                kind,
                actual,
                limit,
            })
        }
    }
}

/// One candidate for the exclusive dimension generation coordinator.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CoordinatorOfferV1 {
    identity: ProviderGenerationIdentityV1,
}

impl CoordinatorOfferV1 {
    /// Creates a coordinator offer.
    #[must_use]
    pub const fn new(identity: ProviderGenerationIdentityV1) -> Self {
        Self { identity }
    }

    /// Returns the output-affecting provider identity.
    #[must_use]
    pub const fn identity(&self) -> &ProviderGenerationIdentityV1 {
        &self.identity
    }
}

/// Exclusive primary generation channels.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum PrimaryChannelV1 {
    /// Primary terrain density and material intent before layered detail.
    TerrainBase,
    /// Primary ownership of cave connectivity and void topology.
    CaveTopology,
}

impl PrimaryChannelV1 {
    /// Returns the normative channel name.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::TerrainBase => "terrain.base",
            Self::CaveTopology => "cave.topology",
        }
    }
}

/// A typed ownership domain, keeping terrain and cave topology identities distinct.
#[derive(Clone, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum PrimaryOwnershipDomainV1 {
    /// Surface terrain ownership domain.
    Terrain(TerritoryDomainIdV1),
    /// Cave-topology ownership domain.
    Cave(CaveTopologyDomainIdV1),
}

impl PrimaryOwnershipDomainV1 {
    /// Returns the channel compatible with this domain kind.
    #[must_use]
    pub const fn channel(&self) -> PrimaryChannelV1 {
        match self {
            Self::Terrain(_) => PrimaryChannelV1::TerrainBase,
            Self::Cave(_) => PrimaryChannelV1::CaveTopology,
        }
    }

    /// Returns canonical domain identity text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        match self {
            Self::Terrain(value) => value.as_str(),
            Self::Cave(value) => value.as_str(),
        }
    }
}

/// One explicit primary-owner offer for a typed ownership domain.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PrimaryProviderOfferV1 {
    domain: PrimaryOwnershipDomainV1,
    identity: ProviderGenerationIdentityV1,
}

impl PrimaryProviderOfferV1 {
    /// Creates a terrain-base owner offer.
    #[must_use]
    pub const fn terrain(
        domain: TerritoryDomainIdV1,
        identity: ProviderGenerationIdentityV1,
    ) -> Self {
        Self {
            domain: PrimaryOwnershipDomainV1::Terrain(domain),
            identity,
        }
    }

    /// Creates a cave-topology owner offer.
    #[must_use]
    pub const fn cave(
        domain: CaveTopologyDomainIdV1,
        identity: ProviderGenerationIdentityV1,
    ) -> Self {
        Self {
            domain: PrimaryOwnershipDomainV1::Cave(domain),
            identity,
        }
    }

    /// Returns the typed ownership domain.
    #[must_use]
    pub const fn domain(&self) -> &PrimaryOwnershipDomainV1 {
        &self.domain
    }

    /// Returns the exclusive channel.
    #[must_use]
    pub const fn channel(&self) -> PrimaryChannelV1 {
        self.domain.channel()
    }

    /// Returns the output-affecting provider identity.
    #[must_use]
    pub const fn identity(&self) -> &ProviderGenerationIdentityV1 {
        &self.identity
    }
}

/// The exactly-one primary owner selected for an ownership domain.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ResolvedPrimaryOwnerV1 {
    channel: PrimaryChannelV1,
    domain: PrimaryOwnershipDomainV1,
    provider: ProviderGenerationIdentityV1,
    inherited_from: Option<PrimaryOwnershipDomainV1>,
}

impl ResolvedPrimaryOwnerV1 {
    pub(crate) const fn new(
        domain: PrimaryOwnershipDomainV1,
        provider: ProviderGenerationIdentityV1,
        inherited_from: Option<PrimaryOwnershipDomainV1>,
    ) -> Self {
        Self {
            channel: domain.channel(),
            domain,
            provider,
            inherited_from,
        }
    }

    /// Returns the exclusive channel.
    #[must_use]
    pub const fn channel(&self) -> PrimaryChannelV1 {
        self.channel
    }

    /// Returns the owned domain.
    #[must_use]
    pub const fn domain(&self) -> &PrimaryOwnershipDomainV1 {
        &self.domain
    }

    /// Returns the selected provider.
    #[must_use]
    pub const fn provider(&self) -> &ProviderGenerationIdentityV1 {
        &self.provider
    }

    /// Returns the ancestor domain supplying an inherited owner.
    #[must_use]
    pub const fn inherited_from(&self) -> Option<&PrimaryOwnershipDomainV1> {
        self.inherited_from.as_ref()
    }
}

/// Hard work and memory declaration for one layered contribution.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ContributionBudgetV1 {
    work_units: u64,
    memory_bytes: u64,
}

impl ContributionBudgetV1 {
    /// Creates a non-zero finite budget.
    ///
    /// # Errors
    ///
    /// Returns an error if either dimension is zero.
    pub fn new(work_units: u64, memory_bytes: u64) -> TerritoryResult<Self> {
        if work_units == 0 || memory_bytes == 0 {
            return Err(TerritoryError::InvalidContribution {
                contribution: "<budget>".to_owned(),
                reason: "work and memory budgets must both be non-zero".to_owned(),
            });
        }
        Ok(Self {
            work_units,
            memory_bytes,
        })
    }

    /// Returns declared work units.
    #[must_use]
    pub const fn work_units(self) -> u64 {
        self.work_units
    }

    /// Returns declared peak memory bytes.
    #[must_use]
    pub const fn memory_bytes(self) -> u64 {
        self.memory_bytes
    }
}

/// Layered non-primary contribution channels.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ContributionChannelV1 {
    /// Surface terrain detail.
    TerrainDetail,
    /// Surface terrain hard constraint.
    TerrainConstraint,
    /// Additional cave branch proposals.
    CaveBranch,
    /// Cave topology clipping constraint.
    CaveConstraint,
    /// Portal clearance constraint.
    PortalConstraint,
    /// Abstract hydrology constraint.
    HydrologyConstraint,
}

/// Typed deterministic compositor used by a contribution channel.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ContributionCompositorV1 {
    /// Stable priority followed by stable contribution identity.
    PriorityStack,
    /// Union finite branch proposals.
    Union,
    /// Intersect hard constraints.
    Intersection,
    /// Select the strictest clearance.
    Maximum,
}

/// Typed spatial target for a layered contribution.
#[derive(Clone, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ContributionTargetV1 {
    /// Surface terrain domain.
    Terrain(TerritoryDomainIdV1),
    /// Cave topology domain.
    Cave(CaveTopologyDomainIdV1),
}

/// A finite, budgeted contribution layered after exclusive primary ownership.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SpatialContributionV1 {
    contribution_id: StableId,
    provider: ProviderGenerationIdentityV1,
    channel: ContributionChannelV1,
    compositor: ContributionCompositorV1,
    target: ContributionTargetV1,
    bounds: PlanningCellBoundsV1,
    vertical_range: Option<VerticalRangeV1>,
    influence_radius_cells: u32,
    priority: i32,
    budget: ContributionBudgetV1,
    payload_hash: CanonicalHash,
}

impl SpatialContributionV1 {
    /// Validates a bounded layered contribution.
    ///
    /// # Errors
    ///
    /// Returns an error for the wrong stable-ID kind, a channel/compositor
    /// mismatch, or a cave contribution without a vertical range.
    #[allow(clippy::too_many_arguments, reason = "persisted contract is explicit")]
    pub fn new(
        contribution_id: StableId,
        provider: ProviderGenerationIdentityV1,
        channel: ContributionChannelV1,
        compositor: ContributionCompositorV1,
        target: ContributionTargetV1,
        bounds: PlanningCellBoundsV1,
        vertical_range: Option<VerticalRangeV1>,
        influence_radius_cells: u32,
        priority: i32,
        budget: ContributionBudgetV1,
        payload_hash: CanonicalHash,
    ) -> TerritoryResult<Self> {
        if contribution_id.kind() != "contribution" {
            return Err(TerritoryError::InvalidStableKind {
                value: contribution_id.to_string(),
                expected: "contribution",
            });
        }
        let valid_compositor = matches!(
            (channel, compositor),
            (
                ContributionChannelV1::TerrainDetail,
                ContributionCompositorV1::PriorityStack
            ) | (
                ContributionChannelV1::TerrainConstraint
                    | ContributionChannelV1::CaveConstraint
                    | ContributionChannelV1::HydrologyConstraint,
                ContributionCompositorV1::Intersection
            ) | (
                ContributionChannelV1::CaveBranch,
                ContributionCompositorV1::Union
            ) | (
                ContributionChannelV1::PortalConstraint,
                ContributionCompositorV1::Maximum
            )
        );
        if !valid_compositor {
            return Err(TerritoryError::InvalidContribution {
                contribution: contribution_id.to_string(),
                reason: "channel requires its normative typed compositor".to_owned(),
            });
        }
        let cave_channel = matches!(
            channel,
            ContributionChannelV1::CaveBranch
                | ContributionChannelV1::CaveConstraint
                | ContributionChannelV1::PortalConstraint
                | ContributionChannelV1::HydrologyConstraint
        );
        if cave_channel != matches!(target, ContributionTargetV1::Cave(_)) {
            return Err(TerritoryError::InvalidContribution {
                contribution: contribution_id.to_string(),
                reason: "channel and target ownership-domain kinds differ".to_owned(),
            });
        }
        if cave_channel && vertical_range.is_none() {
            return Err(TerritoryError::InvalidContribution {
                contribution: contribution_id.to_string(),
                reason: "cave contributions require finite vertical influence".to_owned(),
            });
        }
        Ok(Self {
            contribution_id,
            provider,
            channel,
            compositor,
            target,
            bounds,
            vertical_range,
            influence_radius_cells,
            priority,
            budget,
            payload_hash,
        })
    }

    /// Returns the stable contribution identity.
    #[must_use]
    pub const fn contribution_id(&self) -> &StableId {
        &self.contribution_id
    }

    /// Returns the target ownership domain.
    #[must_use]
    pub const fn target(&self) -> &ContributionTargetV1 {
        &self.target
    }

    /// Returns its finite horizontal bounds.
    #[must_use]
    pub const fn bounds(&self) -> PlanningCellBoundsV1 {
        self.bounds
    }

    /// Returns the hard contribution budget.
    #[must_use]
    pub const fn budget(&self) -> ContributionBudgetV1 {
        self.budget
    }
}

pub(crate) fn resolve_coordinator(
    mut offers: Vec<CoordinatorOfferV1>,
) -> TerritoryResult<ProviderGenerationIdentityV1> {
    validate_provider_fingerprints(
        &offers
            .iter()
            .map(CoordinatorOfferV1::identity)
            .collect::<Vec<_>>(),
    )?;
    offers.sort_by(|left, right| left.identity.cmp(&right.identity));
    match offers.as_slice() {
        [] => Err(TerritoryError::MissingCoordinator),
        [offer] => Ok(offer.identity.clone()),
        _ => Err(TerritoryError::ConflictingCoordinators {
            providers: offers
                .iter()
                .map(|offer| offer.identity.provider_stable_id().to_string())
                .collect(),
        }),
    }
}

pub(crate) fn select_primary(
    domain: PrimaryOwnershipDomainV1,
    offers: &[PrimaryProviderOfferV1],
    inherited: Option<&ResolvedPrimaryOwnerV1>,
) -> TerritoryResult<ResolvedPrimaryOwnerV1> {
    let matching = offers
        .iter()
        .filter(|offer| offer.domain == domain)
        .collect::<Vec<_>>();
    validate_provider_fingerprints(
        &matching
            .iter()
            .map(|offer| offer.identity())
            .collect::<Vec<_>>(),
    )?;
    match matching.as_slice() {
        [offer] => Ok(ResolvedPrimaryOwnerV1::new(
            domain,
            offer.identity.clone(),
            None,
        )),
        [] => inherited.map_or_else(
            || {
                Err(TerritoryError::MissingPrimaryOwner {
                    channel: domain.channel().as_str(),
                    domain: domain.as_str().to_owned(),
                })
            },
            |owner| {
                Ok(ResolvedPrimaryOwnerV1::new(
                    domain.clone(),
                    owner.provider.clone(),
                    Some(owner.domain.clone()),
                ))
            },
        ),
        _ => {
            let mut providers = matching
                .iter()
                .map(|offer| offer.identity.provider_stable_id().to_string())
                .collect::<Vec<_>>();
            providers.sort();
            Err(TerritoryError::ConflictingPrimaryOwners {
                channel: domain.channel().as_str(),
                domain: domain.as_str().to_owned(),
                providers,
            })
        }
    }
}

pub(crate) fn validate_contributions(
    contributions: &mut [SpatialContributionV1],
    limits: TerritoryLimitsV1,
) -> TerritoryResult<()> {
    contributions.sort_by(|left, right| {
        (
            &left.target,
            left.channel,
            left.priority,
            &left.contribution_id,
        )
            .cmp(&(
                &right.target,
                right.channel,
                right.priority,
                &right.contribution_id,
            ))
    });
    if contributions
        .windows(2)
        .any(|pair| pair[0].contribution_id == pair[1].contribution_id)
    {
        return Err(TerritoryError::InvalidContribution {
            contribution: "<duplicate>".to_owned(),
            reason: "contribution identities must be unique".to_owned(),
        });
    }
    let total_work = contributions.iter().try_fold(0_u64, |total, item| {
        total
            .checked_add(item.budget.work_units)
            .ok_or(TerritoryError::ArithmeticOverflow {
                kind: "contribution work budget",
            })
    })?;
    let total_memory = contributions.iter().try_fold(0_u64, |total, item| {
        total
            .checked_add(item.budget.memory_bytes)
            .ok_or(TerritoryError::ArithmeticOverflow {
                kind: "contribution memory budget",
            })
    })?;
    if total_work > limits.max_contribution_work_units {
        return Err(TerritoryError::BudgetExceeded {
            kind: "contribution work",
            actual: total_work,
            limit: limits.max_contribution_work_units,
        });
    }
    if total_memory > limits.max_contribution_memory_bytes {
        return Err(TerritoryError::BudgetExceeded {
            kind: "contribution memory",
            actual: total_memory,
            limit: limits.max_contribution_memory_bytes,
        });
    }
    Ok(())
}

pub(crate) fn validate_provider_fingerprints(
    identities: &[&ProviderGenerationIdentityV1],
) -> TerritoryResult<()> {
    let mut seen = BTreeMap::<(StableId, NonZeroU32, u32), CanonicalHash>::new();
    for identity in identities {
        let key = (
            identity.provider_stable_id().clone(),
            identity.contract_major(),
            identity.algorithm_revision(),
        );
        let fingerprint = *identity.implementation_fingerprint();
        if let Some(previous) = seen.insert(key.clone(), fingerprint)
            && previous != fingerprint
        {
            return Err(TerritoryError::ProviderFingerprintMismatch {
                provider: key.0.to_string(),
            });
        }
    }
    Ok(())
}
