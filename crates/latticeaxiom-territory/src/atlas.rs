//! Deterministic multi-scale surface and underground territory planning.

use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    num::NonZeroU32,
};

use latticeaxiom_core::{StableId, canonical_json_hash};
use latticeaxiom_worldgen::{
    DimensionId, PlanningCellCoordinateV1, ProviderGenerationIdentityV1, WorldSeedV1,
};
use serde::{Deserialize, Serialize};

use crate::{
    AtlasPlanHashV1, CaveAdjacencyV1, CavePortalV1, CaveTopologyDomainIdV1, CaveTopologyParentV1,
    ContributionTargetV1, CoordinatorOfferV1, HydrologyPlanV1, PlanningCellBoundsV1,
    PrimaryOwnershipDomainV1, PrimaryProviderOfferV1, ResolvedPrimaryOwnerV1,
    SpatialContributionV1, TerritoryDomainIdV1, TerritoryError, TerritoryLimitsV1,
    TerritoryPlanReceiptV1, TerritoryResult, UndergroundTerritoryV1,
    cave::{validate_portals, validate_underground},
    hashes::hash_u64,
    provider::{
        resolve_coordinator, select_primary, validate_contributions, validate_provider_fingerprints,
    },
};

/// One level in the coarse-to-fine surface Atlas hierarchy.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AtlasScaleV1 {
    level: u8,
    edge_cells: NonZeroU32,
}

impl AtlasScaleV1 {
    /// Creates a scale level with a positive tile edge.
    #[must_use]
    pub const fn new(level: u8, edge_cells: NonZeroU32) -> Self {
        Self { level, edge_cells }
    }

    /// Returns its zero-based hierarchy level.
    #[must_use]
    pub const fn level(self) -> u8 {
        self.level
    }

    /// Returns its planning-cell tile edge.
    #[must_use]
    pub const fn edge_cells(self) -> NonZeroU32 {
        self.edge_cells
    }
}

/// Validated Atlas hierarchy and transition width.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AtlasConfigV1 {
    scales: Vec<AtlasScaleV1>,
    transition_width_cells: NonZeroU32,
}

impl AtlasConfigV1 {
    /// Validates at least three contiguous, divisible coarse-to-fine scales.
    ///
    /// # Errors
    ///
    /// Returns an error unless levels start at zero, strictly decrease in
    /// divisible edge length, and terminate at one planning cell.
    pub fn new(
        mut scales: Vec<AtlasScaleV1>,
        transition_width_cells: NonZeroU32,
    ) -> TerritoryResult<Self> {
        scales.sort_by_key(|scale| scale.level);
        if scales.len() < 3 {
            return Err(TerritoryError::InvalidAtlasScale {
                reason: "D7 requires at least three Atlas scales".to_owned(),
            });
        }
        for (index, scale) in scales.iter().enumerate() {
            if usize::from(scale.level) != index {
                return Err(TerritoryError::InvalidAtlasScale {
                    reason: "scale levels must be contiguous from zero".to_owned(),
                });
            }
            if let Some(previous) = index.checked_sub(1).map(|value| scales[value])
                && (previous.edge_cells <= scale.edge_cells
                    || previous.edge_cells.get() % scale.edge_cells.get() != 0)
            {
                return Err(TerritoryError::InvalidAtlasScale {
                    reason: "scale edges must strictly decrease by integral subdivision".to_owned(),
                });
            }
        }
        if scales.last().map(|scale| scale.edge_cells.get()) != Some(1) {
            return Err(TerritoryError::InvalidAtlasScale {
                reason: "finest Atlas scale must be one planning cell".to_owned(),
            });
        }
        Ok(Self {
            scales,
            transition_width_cells,
        })
    }

    /// Returns scales in coarse-to-fine order.
    #[must_use]
    pub fn scales(&self) -> &[AtlasScaleV1] {
        &self.scales
    }

    /// Returns the finite transition width.
    #[must_use]
    pub const fn transition_width_cells(&self) -> NonZeroU32 {
        self.transition_width_cells
    }
}

/// One weighted deterministic candidate at an Atlas scale.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SurfaceTerritoryCandidateV1 {
    candidate_id: StableId,
    domain: TerritoryDomainIdV1,
    parent_domain: TerritoryDomainIdV1,
    scale_level: u8,
    anchor: PlanningCellCoordinateV1,
    weight: NonZeroU32,
}

impl SurfaceTerritoryCandidateV1 {
    /// Creates one surface territory candidate.
    ///
    /// # Errors
    ///
    /// Returns an error unless the ID kind is territory-candidate and child and
    /// parent domains differ.
    pub fn new(
        candidate_id: StableId,
        domain: TerritoryDomainIdV1,
        parent_domain: TerritoryDomainIdV1,
        scale_level: u8,
        anchor: PlanningCellCoordinateV1,
        weight: NonZeroU32,
    ) -> TerritoryResult<Self> {
        if candidate_id.kind() != "territory-candidate" {
            return Err(TerritoryError::InvalidStableKind {
                value: candidate_id.to_string(),
                expected: "territory-candidate",
            });
        }
        if domain == parent_domain {
            return Err(TerritoryError::InvalidTerritoryDomain {
                domain: domain.to_string(),
                reason: "a territory cannot parent itself".to_owned(),
            });
        }
        Ok(Self {
            candidate_id,
            domain,
            parent_domain,
            scale_level,
            anchor,
            weight,
        })
    }

    /// Returns its stable candidate identity.
    #[must_use]
    pub const fn candidate_id(&self) -> &StableId {
        &self.candidate_id
    }

    /// Returns the selected ownership domain.
    #[must_use]
    pub const fn domain(&self) -> &TerritoryDomainIdV1 {
        &self.domain
    }

    /// Returns the parent ownership domain required by this candidate.
    #[must_use]
    pub const fn parent_domain(&self) -> &TerritoryDomainIdV1 {
        &self.parent_domain
    }

    /// Returns the Atlas scale that may select this candidate.
    #[must_use]
    pub const fn scale_level(&self) -> u8 {
        self.scale_level
    }

    /// Returns the candidate's planning-cell anchor.
    #[must_use]
    pub const fn anchor(&self) -> PlanningCellCoordinateV1 {
        self.anchor
    }

    /// Returns the positive selection weight.
    #[must_use]
    pub const fn weight(&self) -> NonZeroU32 {
        self.weight
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct AtlasCandidateSelectorV1 {
    scale_level: u8,
    parent_domain: TerritoryDomainIdV1,
    candidate_indices: Vec<u32>,
}

/// Full normalized input to territory-plan compilation.
#[derive(Clone, Debug)]
pub struct TerritoryPlanInputV1 {
    /// Dimension being planned.
    pub dimension: DimensionId,
    /// Stable world seed.
    pub world_seed: WorldSeedV1,
    /// Multi-scale Atlas configuration.
    pub atlas: AtlasConfigV1,
    /// Dimension-default terrain ownership domain.
    pub default_terrain_domain: TerritoryDomainIdV1,
    /// Dimension-default cave-topology ownership domain.
    pub default_cave_domain: CaveTopologyDomainIdV1,
    /// Exclusive coordinator offers.
    pub coordinators: Vec<CoordinatorOfferV1>,
    /// Exclusive primary-owner offers.
    pub primary_offers: Vec<PrimaryProviderOfferV1>,
    /// Surface Atlas candidates.
    pub surface_candidates: Vec<SurfaceTerritoryCandidateV1>,
    /// Bounded underground child territories.
    pub underground_territories: Vec<UndergroundTerritoryV1>,
    /// Layered bounded contributions.
    pub contributions: Vec<SpatialContributionV1>,
    /// Required cave portals.
    pub cave_portals: Vec<CavePortalV1>,
    /// Abstract hydrology plan.
    pub hydrology: HydrologyPlanV1,
    /// Compilation and query hard limits.
    pub limits: TerritoryLimitsV1,
}

/// Winner evidence at one Atlas scale.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TerritoryQueryLevelV1 {
    scale: AtlasScaleV1,
    domain: TerritoryDomainIdV1,
    winning_candidate: Option<StableId>,
    runner_up_candidate: Option<StableId>,
    boundary_distance_cells: u32,
    in_transition_band: bool,
}

impl TerritoryQueryLevelV1 {
    /// Returns the Atlas scale that produced this decision.
    #[must_use]
    pub const fn scale(&self) -> AtlasScaleV1 {
        self.scale
    }

    /// Returns the selected domain after this scale.
    #[must_use]
    pub const fn domain(&self) -> &TerritoryDomainIdV1 {
        &self.domain
    }

    /// Returns the winning candidate identity, if this scale selected one.
    #[must_use]
    pub const fn winning_candidate(&self) -> Option<&StableId> {
        self.winning_candidate.as_ref()
    }

    /// Returns the runner-up candidate identity, if a second candidate scored.
    #[must_use]
    pub const fn runner_up_candidate(&self) -> Option<&StableId> {
        self.runner_up_candidate.as_ref()
    }

    /// Returns conservative distance to the nearest tile boundary.
    #[must_use]
    pub const fn boundary_distance_cells(&self) -> u32 {
        self.boundary_distance_cells
    }

    /// Returns local tile offsets for `cell` at this scale.
    #[must_use]
    pub fn local_offset_cells(&self, cell: PlanningCellCoordinateV1) -> (i64, i64) {
        let edge = i64::from(self.scale.edge_cells.get());
        (cell.x.rem_euclid(edge), cell.z.rem_euclid(edge))
    }

    /// Returns whether this non-unit scale is inside its transition band.
    #[must_use]
    pub const fn in_transition_band(&self) -> bool {
        self.in_transition_band
    }
}

/// Deterministic surface Atlas query result.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TerritoryQueryV1 {
    cell: PlanningCellCoordinateV1,
    terrain_domain: TerritoryDomainIdV1,
    levels: Vec<TerritoryQueryLevelV1>,
    contribution_ids: Vec<StableId>,
    in_transition_band: bool,
}

impl TerritoryQueryV1 {
    /// Returns the queried planning cell.
    #[must_use]
    pub const fn cell(&self) -> PlanningCellCoordinateV1 {
        self.cell
    }

    /// Returns the final surface terrain domain.
    #[must_use]
    pub const fn terrain_domain(&self) -> &TerritoryDomainIdV1 {
        &self.terrain_domain
    }

    /// Returns coarse-to-fine decision evidence.
    #[must_use]
    pub fn levels(&self) -> &[TerritoryQueryLevelV1] {
        &self.levels
    }

    /// Returns sorted layered contributions affecting this cell.
    #[must_use]
    pub fn contribution_ids(&self) -> &[StableId] {
        &self.contribution_ids
    }

    /// Returns whether any non-unit scale is within its transition band.
    #[must_use]
    pub const fn in_transition_band(&self) -> bool {
        self.in_transition_band
    }
}

/// Area and fragmentation statistics for one selected terrain domain.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TerritoryAreaStatisticsV1 {
    domain: TerritoryDomainIdV1,
    cell_count: u64,
    connected_components: u64,
    boundary_edges: u64,
}

impl TerritoryAreaStatisticsV1 {
    /// Returns the sampled ownership domain.
    #[must_use]
    pub const fn domain(&self) -> &TerritoryDomainIdV1 {
        &self.domain
    }

    /// Returns the sampled cell count.
    #[must_use]
    pub const fn cell_count(&self) -> u64 {
        self.cell_count
    }

    /// Returns the connected-component count.
    #[must_use]
    pub const fn connected_components(&self) -> u64 {
        self.connected_components
    }

    /// Returns the sampled domain-boundary edge count.
    #[must_use]
    pub const fn boundary_edges(&self) -> u64 {
        self.boundary_edges
    }
}

/// Bounded distribution, fragmentation, transition, and connectivity evidence.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AtlasStatisticsV1 {
    bounds: PlanningCellBoundsV1,
    sampled_cells: u64,
    transition_cells: u64,
    areas: Vec<TerritoryAreaStatisticsV1>,
    connectivity_edges: Vec<(TerritoryDomainIdV1, TerritoryDomainIdV1)>,
}

impl AtlasStatisticsV1 {
    /// Returns the sampled planning-cell rectangle.
    #[must_use]
    pub const fn bounds(&self) -> PlanningCellBoundsV1 {
        self.bounds
    }

    /// Returns the number of sampled planning cells.
    #[must_use]
    pub const fn sampled_cells(&self) -> u64 {
        self.sampled_cells
    }

    /// Returns sorted per-domain area statistics.
    #[must_use]
    pub fn areas(&self) -> &[TerritoryAreaStatisticsV1] {
        &self.areas
    }

    /// Returns the number of transition-band samples.
    #[must_use]
    pub const fn transition_cells(&self) -> u64 {
        self.transition_cells
    }

    /// Returns sorted observed domain adjacency pairs.
    #[must_use]
    pub fn connectivity_edges(&self) -> &[(TerritoryDomainIdV1, TerritoryDomainIdV1)] {
        &self.connectivity_edges
    }
}

#[derive(Serialize)]
struct PlanHashPayloadV1<'a> {
    dimension: &'a DimensionId,
    world_seed: WorldSeedV1,
    atlas: &'a AtlasConfigV1,
    default_terrain_domain: &'a TerritoryDomainIdV1,
    default_cave_domain: &'a CaveTopologyDomainIdV1,
    coordinator: &'a ProviderGenerationIdentityV1,
    surface_candidates: &'a [SurfaceTerritoryCandidateV1],
    candidate_selectors: &'a [AtlasCandidateSelectorV1],
    underground_territories: &'a [UndergroundTerritoryV1],
    primary_owners: &'a [ResolvedPrimaryOwnerV1],
    contributions: &'a [SpatialContributionV1],
    cave_portals: &'a [CavePortalV1],
    cave_adjacencies: &'a [CaveAdjacencyV1],
    hydrology: &'a HydrologyPlanV1,
    limits: TerritoryLimitsV1,
}

/// Immutable compiled territory plan with pure, thread-safe queries.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TerritoryPlanV1 {
    dimension: DimensionId,
    world_seed: WorldSeedV1,
    atlas: AtlasConfigV1,
    default_terrain_domain: TerritoryDomainIdV1,
    default_cave_domain: CaveTopologyDomainIdV1,
    coordinator: ProviderGenerationIdentityV1,
    surface_candidates: Vec<SurfaceTerritoryCandidateV1>,
    candidate_selectors: Vec<AtlasCandidateSelectorV1>,
    underground_territories: Vec<UndergroundTerritoryV1>,
    primary_owners: Vec<ResolvedPrimaryOwnerV1>,
    contributions: Vec<SpatialContributionV1>,
    cave_portals: Vec<CavePortalV1>,
    cave_adjacencies: Vec<CaveAdjacencyV1>,
    hydrology: HydrologyPlanV1,
    limits: TerritoryLimitsV1,
    plan_hash: AtlasPlanHashV1,
}

impl TerritoryPlanV1 {
    /// Compiles a bounded, deterministic D7 territory and cave skeleton.
    ///
    /// # Errors
    ///
    /// Returns an error for invalid hierarchy, limits, provider ownership,
    /// underground delegation, portal continuity, hydrology, or hashing.
    pub fn compile(mut input: TerritoryPlanInputV1) -> TerritoryResult<Self> {
        input.limits.check_count(
            "surface candidates",
            input.surface_candidates.len(),
            input.limits.max_surface_candidates,
        )?;
        input.limits.check_count(
            "layered contributions",
            input.contributions.len(),
            input.limits.max_contributions,
        )?;
        validate_surface_candidates(
            &input.default_terrain_domain,
            &input.atlas,
            &mut input.surface_candidates,
            input.limits,
        )?;
        let candidate_selectors =
            build_candidate_selectors(&input.surface_candidates, input.limits)?;
        if input.underground_territories.len() < 2 {
            return Err(TerritoryError::InvalidUndergroundTerritory {
                territory: input.default_cave_domain.to_string(),
                reason: "D7 skeleton requires at least two underground child territories"
                    .to_owned(),
            });
        }
        validate_underground(
            &input.default_cave_domain,
            &mut input.underground_territories,
            input.limits,
        )?;
        input.hydrology.validate(input.limits)?;
        validate_contributions(&mut input.contributions, input.limits)?;
        validate_hydrology_targets(
            &input.default_cave_domain,
            &input.underground_territories,
            &input.hydrology,
        )?;
        validate_contribution_targets(
            &input.default_terrain_domain,
            &input.default_cave_domain,
            &input.surface_candidates,
            &input.underground_territories,
            &input.contributions,
        )?;
        {
            let identities = input
                .coordinators
                .iter()
                .map(CoordinatorOfferV1::identity)
                .chain(
                    input
                        .primary_offers
                        .iter()
                        .map(PrimaryProviderOfferV1::identity),
                )
                .collect::<Vec<_>>();
            validate_provider_fingerprints(&identities)?;
        }
        let coordinator = resolve_coordinator(input.coordinators)?;
        let primary_owners = resolve_all_owners(
            &input.default_terrain_domain,
            &input.default_cave_domain,
            &input.surface_candidates,
            &input.underground_territories,
            &input.primary_offers,
            input.limits,
        )?;
        let cave_adjacencies = validate_portals(
            &input.default_cave_domain,
            &input.underground_territories,
            &mut input.cave_portals,
            &input.hydrology,
            input.limits,
        )?;
        validate_d7_portal_coverage(&input.cave_portals)?;
        let mut plan = Self {
            dimension: input.dimension,
            world_seed: input.world_seed,
            atlas: input.atlas,
            default_terrain_domain: input.default_terrain_domain,
            default_cave_domain: input.default_cave_domain,
            coordinator,
            surface_candidates: input.surface_candidates,
            candidate_selectors,
            underground_territories: input.underground_territories,
            primary_owners,
            contributions: input.contributions,
            cave_portals: input.cave_portals,
            cave_adjacencies,
            hydrology: input.hydrology,
            limits: input.limits,
            plan_hash: AtlasPlanHashV1::from_hash(latticeaxiom_core::CanonicalHash::digest([])),
        };
        plan.plan_hash = AtlasPlanHashV1::from_hash(plan.recompute_hash()?);
        Ok(plan)
    }

    /// Returns the canonical plan hash.
    #[must_use]
    pub const fn plan_hash(&self) -> AtlasPlanHashV1 {
        self.plan_hash
    }

    /// Returns the planned dimension.
    #[must_use]
    pub const fn dimension(&self) -> &DimensionId {
        &self.dimension
    }

    /// Returns the exact persisted world seed.
    #[must_use]
    pub const fn world_seed(&self) -> WorldSeedV1 {
        self.world_seed
    }

    /// Returns a finite identity receipt for this compiled plan.
    ///
    /// # Errors
    ///
    /// Returns an error if collection lengths overflow or hashing fails.
    pub fn receipt(&self) -> TerritoryResult<TerritoryPlanReceiptV1> {
        TerritoryPlanReceiptV1::from_plan(self)
    }

    /// Returns the compilation and query hard limits.
    #[must_use]
    pub const fn limits(&self) -> TerritoryLimitsV1 {
        self.limits
    }

    /// Returns the selected coordinator.
    #[must_use]
    pub const fn coordinator(&self) -> &ProviderGenerationIdentityV1 {
        &self.coordinator
    }

    /// Returns sorted resolved exclusive owners.
    #[must_use]
    pub fn primary_owners(&self) -> &[ResolvedPrimaryOwnerV1] {
        &self.primary_owners
    }

    /// Returns the compiled Atlas hierarchy.
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

    /// Returns canonically ordered surface Atlas candidates.
    #[must_use]
    pub fn surface_candidates(&self) -> &[SurfaceTerritoryCandidateV1] {
        &self.surface_candidates
    }

    /// Returns canonically ordered underground child territories.
    #[must_use]
    pub fn underground_territories(&self) -> &[UndergroundTerritoryV1] {
        &self.underground_territories
    }

    /// Returns canonically ordered cave portals.
    #[must_use]
    pub fn cave_portals(&self) -> &[CavePortalV1] {
        &self.cave_portals
    }

    /// Returns canonically ordered cave adjacency evidence.
    #[must_use]
    pub fn cave_adjacencies(&self) -> &[CaveAdjacencyV1] {
        &self.cave_adjacencies
    }

    /// Returns canonically ordered layered contributions.
    #[must_use]
    pub fn contributions(&self) -> &[SpatialContributionV1] {
        &self.contributions
    }

    /// Returns the compiled abstract hydrology plan.
    #[must_use]
    pub const fn hydrology(&self) -> &HydrologyPlanV1 {
        &self.hydrology
    }

    /// Runs a pure deterministic surface query.
    #[must_use]
    pub fn query(&self, cell: PlanningCellCoordinateV1) -> TerritoryQueryV1 {
        let mut current = self.default_terrain_domain.clone();
        let mut levels = Vec::with_capacity(self.atlas.scales.len());
        let mut in_transition_band = false;
        for scale in &self.atlas.scales {
            let edge = i64::from(scale.edge_cells.get());
            let tile_x = cell.x.div_euclid(edge);
            let tile_z = cell.z.div_euclid(edge);
            let selector = self
                .candidate_selectors
                .binary_search_by(|selector| {
                    selector
                        .scale_level
                        .cmp(&scale.level)
                        .then_with(|| selector.parent_domain.cmp(&current))
                })
                .ok()
                .map(|index| &self.candidate_selectors[index]);
            let mut winner = None;
            let mut runner_up = None;
            if let Some(selector) = selector {
                for candidate_index in &selector.candidate_indices {
                    let Ok(candidate_index) = usize::try_from(*candidate_index) else {
                        continue;
                    };
                    let Some(candidate) = self.surface_candidates.get(candidate_index) else {
                        continue;
                    };
                    let score = u128::from(hash_u64(
                        b"latticeaxiom.territory-atlas-candidate.v1\0",
                        &[
                            self.world_seed.as_bytes(),
                            &scale.level.to_be_bytes(),
                            &tile_x.to_be_bytes(),
                            &tile_z.to_be_bytes(),
                            candidate.candidate_id.as_str().as_bytes(),
                            &candidate.anchor.x.to_be_bytes(),
                            &candidate.anchor.z.to_be_bytes(),
                        ],
                    )) * u128::from(candidate.weight.get());
                    if winner.is_none_or(|(best_score, best_candidate)| {
                        candidate_precedes(score, candidate, best_score, best_candidate)
                    }) {
                        runner_up = winner;
                        winner = Some((score, candidate));
                    } else if runner_up.is_none_or(|(best_score, best_candidate)| {
                        candidate_precedes(score, candidate, best_score, best_candidate)
                    }) {
                        runner_up = Some((score, candidate));
                    }
                }
            }
            let winner = winner.map(|(_, candidate)| candidate);
            let runner_up = runner_up.map(|(_, candidate)| candidate.candidate_id.clone());
            if let Some(candidate) = winner {
                current = candidate.domain.clone();
            }
            let local_x = cell.x.rem_euclid(edge);
            let local_z = cell.z.rem_euclid(edge);
            let boundary_distance = local_x
                .min(edge - 1 - local_x)
                .min(local_z.min(edge - 1 - local_z));
            let boundary_distance_cells = u32::try_from(boundary_distance).unwrap_or(u32::MAX);
            let level_transition = scale.edge_cells.get() > 1
                && boundary_distance_cells < self.atlas.transition_width_cells.get();
            in_transition_band |= level_transition;
            levels.push(TerritoryQueryLevelV1 {
                scale: *scale,
                domain: current.clone(),
                winning_candidate: winner.map(|candidate| candidate.candidate_id.clone()),
                runner_up_candidate: runner_up,
                boundary_distance_cells,
                in_transition_band: level_transition,
            });
        }
        let mut contribution_ids = self
            .contributions
            .iter()
            .filter(|contribution| {
                contribution.bounds().contains(cell)
                    && matches!(
                        contribution.target(),
                        ContributionTargetV1::Terrain(domain) if domain == &current
                    )
            })
            .map(|contribution| contribution.contribution_id().clone())
            .collect::<Vec<_>>();
        contribution_ids.sort();
        TerritoryQueryV1 {
            cell,
            terrain_domain: current,
            levels,
            contribution_ids,
            in_transition_band,
        }
    }

    /// Returns the deepest nested cave topology domain containing a point.
    #[must_use]
    pub fn cave_domain_at(
        &self,
        cell: PlanningCellCoordinateV1,
        y: i32,
    ) -> &CaveTopologyDomainIdV1 {
        let mut current = &self.default_cave_domain;
        loop {
            let child = self.underground_territories.iter().find(|territory| {
                let parent_matches = match territory.parent() {
                    CaveTopologyParentV1::DimensionDefault => current == &self.default_cave_domain,
                    CaveTopologyParentV1::Territory(parent) => parent == current,
                };
                parent_matches
                    && territory.bounds().contains(cell)
                    && territory.vertical_range().contains(y)
            });
            match child {
                Some(territory) => current = territory.domain(),
                None => return current,
            }
        }
    }

    /// Returns canonical cave adjacency evidence for a cell pair.
    #[must_use]
    pub fn cave_adjacency(
        &self,
        first: PlanningCellCoordinateV1,
        second: PlanningCellCoordinateV1,
    ) -> Option<&CaveAdjacencyV1> {
        let pair = if first <= second {
            (first, second)
        } else {
            (second, first)
        };
        self.cave_adjacencies
            .binary_search_by_key(&pair, CaveAdjacencyV1::cells)
            .ok()
            .map(|index| &self.cave_adjacencies[index])
    }

    /// Computes bounded area, fragmentation, transition, and connectivity stats.
    ///
    /// # Errors
    ///
    /// Returns an error if the requested sample exceeds configured bounds.
    pub fn statistics(&self, bounds: PlanningCellBoundsV1) -> TerritoryResult<AtlasStatisticsV1> {
        let sampled_cells = bounds.area()?;
        if sampled_cells > self.limits.max_statistics_cells {
            return Err(TerritoryError::StatisticsRegionTooLarge {
                actual: sampled_cells,
                limit: self.limits.max_statistics_cells,
            });
        }
        let mut cells = BTreeMap::new();
        let mut transition_cells = 0_u64;
        for z in bounds.min_z()..bounds.max_z_exclusive() {
            for x in bounds.min_x()..bounds.max_x_exclusive() {
                let cell = PlanningCellCoordinateV1::new(x, z);
                let query = self.query(cell);
                transition_cells += u64::from(query.in_transition_band());
                cells.insert(cell, query.terrain_domain);
            }
        }
        let mut counts = BTreeMap::<TerritoryDomainIdV1, u64>::new();
        let mut boundaries = BTreeMap::<TerritoryDomainIdV1, u64>::new();
        let mut connectivity = BTreeSet::new();
        for (cell, domain) in &cells {
            *counts.entry(domain.clone()).or_default() += 1;
            for neighbor in [
                PlanningCellCoordinateV1::new(cell.x.saturating_add(1), cell.z),
                PlanningCellCoordinateV1::new(cell.x, cell.z.saturating_add(1)),
            ] {
                if let Some(other) = cells.get(&neighbor)
                    && other != domain
                {
                    *boundaries.entry(domain.clone()).or_default() += 1;
                    *boundaries.entry(other.clone()).or_default() += 1;
                    let pair = if domain <= other {
                        (domain.clone(), other.clone())
                    } else {
                        (other.clone(), domain.clone())
                    };
                    connectivity.insert(pair);
                }
            }
        }
        let mut visited = BTreeSet::new();
        let mut components = BTreeMap::<TerritoryDomainIdV1, u64>::new();
        for (start, domain) in &cells {
            if !visited.insert(*start) {
                continue;
            }
            *components.entry(domain.clone()).or_default() += 1;
            let mut queue = VecDeque::from([*start]);
            while let Some(cell) = queue.pop_front() {
                for neighbor in cardinal_neighbors(cell) {
                    if !visited.contains(&neighbor)
                        && cells.get(&neighbor).is_some_and(|other| other == domain)
                    {
                        visited.insert(neighbor);
                        queue.push_back(neighbor);
                    }
                }
            }
        }
        let areas = counts
            .into_iter()
            .map(|(domain, cell_count)| TerritoryAreaStatisticsV1 {
                boundary_edges: boundaries.get(&domain).copied().unwrap_or_default(),
                connected_components: components.get(&domain).copied().unwrap_or_default(),
                domain,
                cell_count,
            })
            .collect();
        Ok(AtlasStatisticsV1 {
            bounds,
            sampled_cells,
            transition_cells,
            areas,
            connectivity_edges: connectivity.into_iter().collect(),
        })
    }

    /// Verifies deterministic ordering and the stored plan hash.
    ///
    /// # Errors
    ///
    /// Returns an error if canonical plan identity is stale.
    pub fn validate_hash(&self) -> TerritoryResult<()> {
        let expected_selectors = build_candidate_selectors(&self.surface_candidates, self.limits)?;
        if expected_selectors != self.candidate_selectors {
            return Err(TerritoryError::InvalidAtlasScale {
                reason: "serialized Atlas candidate selector index is stale".to_owned(),
            });
        }
        if self.recompute_hash()? != *self.plan_hash.as_hash() {
            return Err(TerritoryError::InvalidAtlasScale {
                reason: "serialized territory plan hash is stale".to_owned(),
            });
        }
        Ok(())
    }

    fn recompute_hash(&self) -> TerritoryResult<latticeaxiom_core::CanonicalHash> {
        canonical_json_hash(&PlanHashPayloadV1 {
            dimension: &self.dimension,
            world_seed: self.world_seed,
            atlas: &self.atlas,
            default_terrain_domain: &self.default_terrain_domain,
            default_cave_domain: &self.default_cave_domain,
            coordinator: &self.coordinator,
            surface_candidates: &self.surface_candidates,
            candidate_selectors: &self.candidate_selectors,
            underground_territories: &self.underground_territories,
            primary_owners: &self.primary_owners,
            contributions: &self.contributions,
            cave_portals: &self.cave_portals,
            cave_adjacencies: &self.cave_adjacencies,
            hydrology: &self.hydrology,
            limits: self.limits,
        })
        .map_err(|error| TerritoryError::CanonicalEncoding {
            kind: "territory plan",
            reason: error.to_string(),
        })
    }
}

fn candidate_precedes(
    score: u128,
    candidate: &SurfaceTerritoryCandidateV1,
    best_score: u128,
    best_candidate: &SurfaceTerritoryCandidateV1,
) -> bool {
    score > best_score
        || (score == best_score && candidate.candidate_id < best_candidate.candidate_id)
}

fn build_candidate_selectors(
    candidates: &[SurfaceTerritoryCandidateV1],
    limits: TerritoryLimitsV1,
) -> TerritoryResult<Vec<AtlasCandidateSelectorV1>> {
    if candidates.is_empty()
        || candidates
            .windows(2)
            .any(|pair| pair[0].candidate_id >= pair[1].candidate_id)
    {
        return Err(TerritoryError::InvalidAtlasScale {
            reason: "Atlas candidates must be in canonical identity order".to_owned(),
        });
    }
    let mut grouped = BTreeMap::<(u8, TerritoryDomainIdV1), Vec<u32>>::new();
    for (index, candidate) in candidates.iter().enumerate() {
        let candidate_index =
            u32::try_from(index).map_err(|_| TerritoryError::ArithmeticOverflow {
                kind: "Atlas candidate selector index",
            })?;
        grouped
            .entry((candidate.scale_level, candidate.parent_domain.clone()))
            .or_default()
            .push(candidate_index);
    }
    grouped
        .into_iter()
        .map(|((scale_level, parent_domain), candidate_indices)| {
            limits.check_count(
                "candidates per Atlas selector",
                candidate_indices.len(),
                limits.max_candidates_per_selector,
            )?;
            Ok(AtlasCandidateSelectorV1 {
                scale_level,
                parent_domain,
                candidate_indices,
            })
        })
        .collect()
}

fn validate_d7_portal_coverage(portals: &[CavePortalV1]) -> TerritoryResult<()> {
    let touched_cells = portals
        .iter()
        .flat_map(|portal| {
            let (first, second) = portal.cells();
            [first, second]
        })
        .collect::<BTreeSet<_>>();
    let touched_domains = portals
        .iter()
        .flat_map(|portal| {
            let (first, second) = portal.domains();
            [first.clone(), second.clone()]
        })
        .collect::<BTreeSet<_>>();
    if touched_cells.len() < 4 || touched_domains.len() < 2 {
        return Err(TerritoryError::InvalidCaveAdjacency {
            reason: "D7 skeleton requires portals across at least four cells and two domains"
                .to_owned(),
        });
    }
    Ok(())
}

fn validate_surface_candidates(
    default_domain: &TerritoryDomainIdV1,
    atlas: &AtlasConfigV1,
    candidates: &mut [SurfaceTerritoryCandidateV1],
    limits: TerritoryLimitsV1,
) -> TerritoryResult<()> {
    candidates.sort_by(|left, right| left.candidate_id.cmp(&right.candidate_id));
    if candidates.is_empty()
        || candidates
            .windows(2)
            .any(|pair| pair[0].candidate_id == pair[1].candidate_id)
    {
        return Err(TerritoryError::InvalidTerritoryDomain {
            domain: default_domain.to_string(),
            reason: "surface candidate identities must be non-empty and unique".to_owned(),
        });
    }
    let mut domain_parent = BTreeMap::new();
    let mut domain_level = BTreeMap::new();
    for candidate in candidates.iter() {
        if usize::from(candidate.scale_level) >= atlas.scales.len() {
            return Err(TerritoryError::InvalidAtlasScale {
                reason: format!(
                    "candidate {} references unknown scale {}",
                    candidate.candidate_id, candidate.scale_level
                ),
            });
        }
        if &candidate.domain == default_domain {
            return Err(TerritoryError::InvalidTerritoryDomain {
                domain: candidate.domain.to_string(),
                reason: "dimension default cannot also be a child candidate".to_owned(),
            });
        }
        if let Some(previous) =
            domain_parent.insert(candidate.domain.clone(), candidate.parent_domain.clone())
            && previous != candidate.parent_domain
        {
            return Err(TerritoryError::InvalidTerritoryDomain {
                domain: candidate.domain.to_string(),
                reason: "one domain declares multiple parents".to_owned(),
            });
        }
        if let Some(previous) = domain_level.insert(candidate.domain.clone(), candidate.scale_level)
            && previous != candidate.scale_level
        {
            return Err(TerritoryError::InvalidTerritoryDomain {
                domain: candidate.domain.to_string(),
                reason: "one ownership domain cannot span multiple Atlas scales".to_owned(),
            });
        }
    }
    limits.check_count(
        "terrain domains",
        domain_parent.len() + 1,
        limits.max_domains,
    )?;
    for (domain, parent) in &domain_parent {
        let parent_level = if parent == default_domain {
            None
        } else {
            Some(*domain_level.get(parent).ok_or_else(|| {
                TerritoryError::InvalidTerritoryDomain {
                    domain: domain.to_string(),
                    reason: format!("parent domain {parent} has no candidate"),
                }
            })?)
        };
        let level = domain_level[domain];
        if parent_level.is_some_and(|parent_level| parent_level >= level) {
            return Err(TerritoryError::InvalidTerritoryDomain {
                domain: domain.to_string(),
                reason: "child domain must first appear below its parent scale".to_owned(),
            });
        }
    }
    for scale in &atlas.scales {
        if !candidates
            .iter()
            .any(|candidate| candidate.scale_level == scale.level)
        {
            return Err(TerritoryError::InvalidAtlasScale {
                reason: format!("scale {} has no candidate", scale.level),
            });
        }
    }
    Ok(())
}

fn resolve_all_owners(
    default_terrain: &TerritoryDomainIdV1,
    default_cave: &CaveTopologyDomainIdV1,
    candidates: &[SurfaceTerritoryCandidateV1],
    underground: &[UndergroundTerritoryV1],
    offers: &[PrimaryProviderOfferV1],
    limits: TerritoryLimitsV1,
) -> TerritoryResult<Vec<ResolvedPrimaryOwnerV1>> {
    let mut resolved = BTreeMap::<PrimaryOwnershipDomainV1, ResolvedPrimaryOwnerV1>::new();
    let terrain_default = PrimaryOwnershipDomainV1::Terrain(default_terrain.clone());
    let cave_default = PrimaryOwnershipDomainV1::Cave(default_cave.clone());
    let terrain_owner = select_primary(terrain_default.clone(), offers, None)?;
    let cave_owner = select_primary(cave_default.clone(), offers, None)?;
    resolved.insert(terrain_default, terrain_owner);
    resolved.insert(cave_default, cave_owner);

    let mut terrain_parents = BTreeMap::new();
    for candidate in candidates {
        terrain_parents.insert(candidate.domain.clone(), candidate.parent_domain.clone());
    }
    while !terrain_parents.is_empty() {
        let ready = terrain_parents
            .iter()
            .find_map(|(domain, parent)| {
                let parent_key = PrimaryOwnershipDomainV1::Terrain(parent.clone());
                resolved
                    .get(&parent_key)
                    .map(|owner| (domain.clone(), parent.clone(), owner.clone()))
            })
            .ok_or_else(|| TerritoryError::InvalidTerritoryDomain {
                domain: default_terrain.to_string(),
                reason: "terrain parent graph contains a cycle".to_owned(),
            })?;
        let key = PrimaryOwnershipDomainV1::Terrain(ready.0.clone());
        let owner = select_primary(key.clone(), offers, Some(&ready.2))?;
        resolved.insert(key, owner);
        terrain_parents.remove(&ready.0);
    }

    let mut cave_parents = underground
        .iter()
        .map(|territory| {
            (
                territory.domain().clone(),
                match territory.parent() {
                    CaveTopologyParentV1::DimensionDefault => default_cave.clone(),
                    CaveTopologyParentV1::Territory(parent) => parent.clone(),
                },
            )
        })
        .collect::<BTreeMap<_, _>>();
    while !cave_parents.is_empty() {
        let ready = cave_parents
            .iter()
            .find_map(|(domain, parent)| {
                let parent_key = PrimaryOwnershipDomainV1::Cave(parent.clone());
                resolved
                    .get(&parent_key)
                    .map(|owner| (domain.clone(), owner.clone()))
            })
            .ok_or_else(|| TerritoryError::InvalidUndergroundTerritory {
                territory: default_cave.to_string(),
                reason: "cave parent graph contains a cycle".to_owned(),
            })?;
        let key = PrimaryOwnershipDomainV1::Cave(ready.0.clone());
        let owner = select_primary(key.clone(), offers, Some(&ready.1))?;
        resolved.insert(key, owner);
        cave_parents.remove(&ready.0);
    }
    limits.check_count(
        "resolved ownership domains",
        resolved.len(),
        limits.max_domains,
    )?;
    let known = resolved.keys().collect::<BTreeSet<_>>();
    if let Some(unknown) = offers.iter().find(|offer| !known.contains(offer.domain())) {
        return Err(TerritoryError::InvalidTerritoryDomain {
            domain: unknown.domain().as_str().to_owned(),
            reason: "primary offer targets an unknown ownership domain".to_owned(),
        });
    }
    Ok(resolved.into_values().collect())
}

fn validate_hydrology_targets(
    default_cave: &CaveTopologyDomainIdV1,
    underground: &[UndergroundTerritoryV1],
    hydrology: &HydrologyPlanV1,
) -> TerritoryResult<()> {
    let by_domain = underground
        .iter()
        .map(|territory| (territory.domain(), territory))
        .collect::<BTreeMap<_, _>>();
    for basin in hydrology.basins() {
        if basin.topology_domain() == default_cave {
            continue;
        }
        let territory = by_domain.get(basin.topology_domain()).ok_or_else(|| {
            TerritoryError::InvalidHydrologyPlan {
                reason: format!(
                    "basin {} targets unknown cave domain {}",
                    basin.basin_id(),
                    basin.topology_domain()
                ),
            }
        })?;
        if !territory.bounds().contains_bounds(basin.bounds())
            || !territory
                .vertical_range()
                .contains_range(basin.vertical_range())
        {
            return Err(TerritoryError::InvalidHydrologyPlan {
                reason: format!(
                    "basin {} exceeds ownership domain {}",
                    basin.basin_id(),
                    basin.topology_domain()
                ),
            });
        }
    }
    Ok(())
}

fn validate_contribution_targets(
    default_terrain: &TerritoryDomainIdV1,
    default_cave: &CaveTopologyDomainIdV1,
    candidates: &[SurfaceTerritoryCandidateV1],
    underground: &[UndergroundTerritoryV1],
    contributions: &[SpatialContributionV1],
) -> TerritoryResult<()> {
    let terrain = candidates
        .iter()
        .map(|candidate| candidate.domain.clone())
        .chain(std::iter::once(default_terrain.clone()))
        .collect::<BTreeSet<_>>();
    let cave = underground
        .iter()
        .map(|territory| territory.domain().clone())
        .chain(std::iter::once(default_cave.clone()))
        .collect::<BTreeSet<_>>();
    for contribution in contributions {
        let known = match contribution.target() {
            ContributionTargetV1::Terrain(domain) => terrain.contains(domain),
            ContributionTargetV1::Cave(domain) => cave.contains(domain),
        };
        if !known {
            return Err(TerritoryError::InvalidContribution {
                contribution: contribution.contribution_id().to_string(),
                reason: "target ownership domain is not declared".to_owned(),
            });
        }
    }
    Ok(())
}

fn cardinal_neighbors(cell: PlanningCellCoordinateV1) -> [PlanningCellCoordinateV1; 4] {
    [
        PlanningCellCoordinateV1::new(cell.x.saturating_sub(1), cell.z),
        PlanningCellCoordinateV1::new(cell.x.saturating_add(1), cell.z),
        PlanningCellCoordinateV1::new(cell.x, cell.z.saturating_sub(1)),
        PlanningCellCoordinateV1::new(cell.x, cell.z.saturating_add(1)),
    ]
}
