//! Production surface and underground territory ownership queries.

use std::collections::BTreeSet;

use latticeaxiom_core::StableId;
use latticeaxiom_worldgen::{
    ChunkCoordinate, LockedClosureFingerprintV1, WorldSeedV1, WorldgenConfigHashV1,
    WorldgenConfigV1,
};
use serde::{Deserialize, Serialize};

use crate::{
    AtlasPlanHashV1, CaveTopologyDomainIdV1, CaveTopologyParentV1, CaveTopologyPlanV1,
    PlanningCellBoundsV1, PlanningCellCoordinateV1, PrimaryOwnershipDomainV1,
    ResolvedPrimaryOwnerV1, TerritoryDomainIdV1, TerritoryError, TerritoryPlanV1, TerritoryQueryV1,
    TerritoryResult, UndergroundTerritoryV1,
};

/// One ranked ownership candidate returned by a production territory query.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct OrderedOwnershipCandidateV1<D> {
    rank: u8,
    domain: D,
    owner: ResolvedPrimaryOwnerV1,
    candidate_id: Option<StableId>,
}

impl<D> OrderedOwnershipCandidateV1<D> {
    /// Returns the 1-based rank (primary is 1, secondary is 2).
    #[must_use]
    pub const fn rank(&self) -> u8 {
        self.rank
    }

    /// Returns the ownership domain at this rank.
    #[must_use]
    pub const fn domain(&self) -> &D {
        &self.domain
    }

    /// Returns the exclusive primary owner of this domain.
    #[must_use]
    pub const fn owner(&self) -> &ResolvedPrimaryOwnerV1 {
        &self.owner
    }

    /// Returns the Atlas candidate identity when this rank selected one.
    #[must_use]
    pub const fn candidate_id(&self) -> Option<&StableId> {
        self.candidate_id.as_ref()
    }
}

/// Production surface ownership at one planning cell.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SurfaceTerritoryQueryV1 {
    cell: PlanningCellCoordinateV1,
    primary: TerritoryDomainIdV1,
    secondary: TerritoryDomainIdV1,
    primary_candidate: Option<StableId>,
    secondary_candidate: Option<StableId>,
    primary_owner: ResolvedPrimaryOwnerV1,
    secondary_owner: ResolvedPrimaryOwnerV1,
    boundary_distance_cells: u32,
    in_transition_band: bool,
}

impl SurfaceTerritoryQueryV1 {
    /// Returns the queried planning cell.
    #[must_use]
    pub const fn cell(&self) -> PlanningCellCoordinateV1 {
        self.cell
    }

    /// Returns the winning terrain ownership domain.
    #[must_use]
    pub const fn primary(&self) -> &TerritoryDomainIdV1 {
        &self.primary
    }

    /// Returns the runner-up or parent terrain ownership domain.
    #[must_use]
    pub const fn secondary(&self) -> &TerritoryDomainIdV1 {
        &self.secondary
    }

    /// Returns the winning Atlas candidate identity, if a candidate won.
    #[must_use]
    pub const fn primary_candidate(&self) -> Option<&StableId> {
        self.primary_candidate.as_ref()
    }

    /// Returns the runner-up Atlas candidate identity, if one scored.
    #[must_use]
    pub const fn secondary_candidate(&self) -> Option<&StableId> {
        self.secondary_candidate.as_ref()
    }

    /// Returns primary then secondary ownership candidates in rank order.
    #[must_use]
    pub fn ordered_candidates(&self) -> [OrderedOwnershipCandidateV1<TerritoryDomainIdV1>; 2] {
        [
            OrderedOwnershipCandidateV1 {
                rank: 1,
                domain: self.primary.clone(),
                owner: self.primary_owner.clone(),
                candidate_id: self.primary_candidate.clone(),
            },
            OrderedOwnershipCandidateV1 {
                rank: 2,
                domain: self.secondary.clone(),
                owner: self.secondary_owner.clone(),
                candidate_id: self.secondary_candidate.clone(),
            },
        ]
    }

    /// Returns the exclusive primary owner of the winning domain.
    #[must_use]
    pub const fn primary_owner(&self) -> &ResolvedPrimaryOwnerV1 {
        &self.primary_owner
    }

    /// Returns the exclusive primary owner of the secondary domain.
    #[must_use]
    pub const fn secondary_owner(&self) -> &ResolvedPrimaryOwnerV1 {
        &self.secondary_owner
    }

    /// Returns conservative distance to the nearest Atlas tile boundary.
    #[must_use]
    pub const fn boundary_distance_cells(&self) -> u32 {
        self.boundary_distance_cells
    }

    /// Returns whether primary and secondary domains meet inside the transition width.
    #[must_use]
    pub const fn in_transition_band(&self) -> bool {
        self.in_transition_band
    }
}

/// Production underground ownership at one planning cell and height.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct UndergroundTerritoryQueryV1 {
    cell: PlanningCellCoordinateV1,
    y: i32,
    primary: CaveTopologyDomainIdV1,
    secondary: CaveTopologyDomainIdV1,
    primary_owner: ResolvedPrimaryOwnerV1,
    secondary_owner: ResolvedPrimaryOwnerV1,
    boundary_distance_cells: u32,
    in_transition_band: bool,
}

impl UndergroundTerritoryQueryV1 {
    /// Returns the queried planning cell.
    #[must_use]
    pub const fn cell(&self) -> PlanningCellCoordinateV1 {
        self.cell
    }

    /// Returns the queried world-Y sample.
    #[must_use]
    pub const fn y(&self) -> i32 {
        self.y
    }

    /// Returns the deepest cave-topology ownership domain containing the sample.
    #[must_use]
    pub const fn primary(&self) -> &CaveTopologyDomainIdV1 {
        &self.primary
    }

    /// Returns the parent or nearest sibling cave-topology ownership domain.
    #[must_use]
    pub const fn secondary(&self) -> &CaveTopologyDomainIdV1 {
        &self.secondary
    }

    /// Returns primary then secondary cave-topology owners in rank order.
    #[must_use]
    pub fn ordered_candidates(&self) -> [OrderedOwnershipCandidateV1<CaveTopologyDomainIdV1>; 2] {
        [
            OrderedOwnershipCandidateV1 {
                rank: 1,
                domain: self.primary.clone(),
                owner: self.primary_owner.clone(),
                candidate_id: None,
            },
            OrderedOwnershipCandidateV1 {
                rank: 2,
                domain: self.secondary.clone(),
                owner: self.secondary_owner.clone(),
                candidate_id: None,
            },
        ]
    }

    /// Returns the exclusive primary owner of the winning cave domain.
    #[must_use]
    pub const fn primary_owner(&self) -> &ResolvedPrimaryOwnerV1 {
        &self.primary_owner
    }

    /// Returns the exclusive primary owner of the secondary cave domain.
    #[must_use]
    pub const fn secondary_owner(&self) -> &ResolvedPrimaryOwnerV1 {
        &self.secondary_owner
    }

    /// Returns conservative distance to the nearest underground ownership boundary.
    #[must_use]
    pub const fn boundary_distance_cells(&self) -> u32 {
        self.boundary_distance_cells
    }

    /// Returns whether primary and secondary domains meet inside the transition width.
    #[must_use]
    pub const fn in_transition_band(&self) -> bool {
        self.in_transition_band
    }
}

/// Canonical production-query coverage for one seed, lock, config, and range.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TerritoryQueryCoverageV1 {
    world_seed: WorldSeedV1,
    locked_closure_fingerprint: LockedClosureFingerprintV1,
    config_hash: WorldgenConfigHashV1,
    plan_hash: AtlasPlanHashV1,
    bounds: PlanningCellBoundsV1,
    underground_y: i32,
    surface: Vec<SurfaceTerritoryQueryV1>,
    underground: Vec<UndergroundTerritoryQueryV1>,
}

impl TerritoryQueryCoverageV1 {
    /// Collects dense production queries over a planning-cell rectangle.
    ///
    /// # Errors
    ///
    /// Returns an error if the sample exceeds the plan's statistics bound, the
    /// D4 config cannot be hashed, or a compiled ownership domain is missing.
    pub fn from_bounds(
        plan: &TerritoryPlanV1,
        lock: LockedClosureFingerprintV1,
        config: &WorldgenConfigV1,
        bounds: PlanningCellBoundsV1,
        underground_y: i32,
    ) -> TerritoryResult<Self> {
        let sampled_cells = bounds.area()?;
        check_query_range_len(sampled_cells, plan)?;
        let mut cells = Vec::new();
        for z in bounds.min_z()..bounds.max_z_exclusive() {
            for x in bounds.min_x()..bounds.max_x_exclusive() {
                cells.push(PlanningCellCoordinateV1::new(x, z));
            }
        }
        collect_coverage(plan, lock, config, bounds, underground_y, cells)
    }

    /// Collects production queries for unique planning cells derived from chunks.
    ///
    /// Chunk order cannot change the canonical coverage bytes. Duplicate chunk
    /// coordinates collapse to one planning cell.
    ///
    /// # Errors
    ///
    /// Returns an error if no chunk is supplied, the unique-cell count exceeds
    /// the plan's statistics bound, the D4 config cannot be hashed, a coverage
    /// bound overflows, or a compiled ownership domain is missing.
    #[allow(
        clippy::similar_names,
        reason = "x and z half-open query-range bounds are intentionally symmetric"
    )]
    pub fn from_chunks(
        plan: &TerritoryPlanV1,
        lock: LockedClosureFingerprintV1,
        config: &WorldgenConfigV1,
        chunks: impl IntoIterator<Item = ChunkCoordinate>,
        underground_y: i32,
    ) -> TerritoryResult<Self> {
        let mut cells = BTreeSet::new();
        for chunk in chunks {
            cells.insert(PlanningCellCoordinateV1::from_chunk(
                chunk,
                config.planning_cell_edge_chunks,
            ));
        }
        if cells.is_empty() {
            return Err(TerritoryError::InvalidBounds {
                kind: "territory query range",
                minimum: 0,
                maximum: 0,
            });
        }
        check_query_range_len(u64::try_from(cells.len()).unwrap_or(u64::MAX), plan)?;
        let min_x = cells.iter().map(|cell| cell.x).min().unwrap_or_default();
        let min_z = cells.iter().map(|cell| cell.z).min().unwrap_or_default();
        let max_x = cells.iter().map(|cell| cell.x).max().unwrap_or_default();
        let max_z = cells.iter().map(|cell| cell.z).max().unwrap_or_default();
        let max_x_exclusive = max_x
            .checked_add(1)
            .ok_or(TerritoryError::ArithmeticOverflow {
                kind: "territory query range x bound",
            })?;
        let max_z_exclusive = max_z
            .checked_add(1)
            .ok_or(TerritoryError::ArithmeticOverflow {
                kind: "territory query range z bound",
            })?;
        let bounds = PlanningCellBoundsV1::new(min_x, min_z, max_x_exclusive, max_z_exclusive)?;
        collect_coverage(
            plan,
            lock,
            config,
            bounds,
            underground_y,
            cells.into_iter().collect(),
        )
    }

    /// Returns the world seed frozen into this coverage.
    #[must_use]
    pub const fn world_seed(&self) -> WorldSeedV1 {
        self.world_seed
    }

    /// Returns the locked-closure fingerprint frozen into this coverage.
    #[must_use]
    pub const fn locked_closure_fingerprint(&self) -> LockedClosureFingerprintV1 {
        self.locked_closure_fingerprint
    }

    /// Returns the D4 config hash frozen into this coverage.
    #[must_use]
    pub const fn config_hash(&self) -> WorldgenConfigHashV1 {
        self.config_hash
    }

    /// Returns the compiled Atlas plan hash.
    #[must_use]
    pub const fn plan_hash(&self) -> AtlasPlanHashV1 {
        self.plan_hash
    }

    /// Returns the axis-aligned planning-cell bounds covering the sample.
    #[must_use]
    pub const fn bounds(&self) -> PlanningCellBoundsV1 {
        self.bounds
    }

    /// Returns sorted surface ownership queries.
    #[must_use]
    pub fn surface(&self) -> &[SurfaceTerritoryQueryV1] {
        &self.surface
    }

    /// Returns sorted underground ownership queries.
    #[must_use]
    pub fn underground(&self) -> &[UndergroundTerritoryQueryV1] {
        &self.underground
    }
}

impl TerritoryPlanV1 {
    /// Returns production surface ownership at one planning cell.
    ///
    /// # Errors
    ///
    /// Returns [`TerritoryError::MissingPrimaryOwner`] if a compiled domain has
    /// no resolved owner.
    pub fn surface_territory_query(
        &self,
        cell: PlanningCellCoordinateV1,
    ) -> TerritoryResult<SurfaceTerritoryQueryV1> {
        let atlas_query = self.query(cell);
        let primary = atlas_query.terrain_domain().clone();
        let primary_candidate = atlas_query
            .levels()
            .iter()
            .rev()
            .find_map(crate::TerritoryQueryLevelV1::winning_candidate)
            .cloned();
        let (secondary, secondary_candidate) = surface_secondary(self, &atlas_query);
        let primary_owner =
            resolved_owner(self, &PrimaryOwnershipDomainV1::Terrain(primary.clone()))?.clone();
        let secondary_owner =
            resolved_owner(self, &PrimaryOwnershipDomainV1::Terrain(secondary.clone()))?.clone();
        let boundary_distance_cells = surface_boundary_distance(&atlas_query);
        let in_transition_band = primary != secondary
            && boundary_distance_cells < self.atlas().transition_width_cells().get();
        Ok(SurfaceTerritoryQueryV1 {
            cell,
            primary,
            secondary,
            primary_candidate,
            secondary_candidate,
            primary_owner,
            secondary_owner,
            boundary_distance_cells,
            in_transition_band,
        })
    }

    /// Returns production underground ownership at one planning cell and height.
    ///
    /// # Errors
    ///
    /// Returns [`TerritoryError::MissingPrimaryOwner`] if a compiled cave domain
    /// has no resolved owner.
    pub fn underground_territory_query(
        &self,
        cell: PlanningCellCoordinateV1,
        y: i32,
    ) -> TerritoryResult<UndergroundTerritoryQueryV1> {
        let primary = self.cave_domain_at(cell, y).clone();
        let (secondary, boundary_distance_cells) = underground_secondary(self, cell, y, &primary);
        let primary_owner =
            resolved_owner(self, &PrimaryOwnershipDomainV1::Cave(primary.clone()))?.clone();
        let secondary_owner =
            resolved_owner(self, &PrimaryOwnershipDomainV1::Cave(secondary.clone()))?.clone();
        let in_transition_band = primary != secondary
            && boundary_distance_cells < self.atlas().transition_width_cells().get();
        Ok(UndergroundTerritoryQueryV1 {
            cell,
            y,
            primary,
            secondary,
            primary_owner,
            secondary_owner,
            boundary_distance_cells,
            in_transition_band,
        })
    }

    /// Compiles the V6 cave topology plan from this Atlas skeleton.
    ///
    /// # Errors
    ///
    /// Returns an error unless the compiled skeleton has two underground
    /// subdomains and a surface entrance that crosses four cells and two
    /// topology domains.
    pub fn cave_topology_plan(
        &self,
        config: &WorldgenConfigV1,
    ) -> TerritoryResult<CaveTopologyPlanV1> {
        CaveTopologyPlanV1::compile(
            self.world_seed(),
            self.plan_hash(),
            self.default_cave_domain(),
            self.underground_territories(),
            self.cave_portals(),
            self.contributions(),
            config,
        )
    }

    /// Collects the seed-stable V6 cave topology plan for unique chunk cells.
    ///
    /// Chunk order cannot change the compiled plan.
    ///
    /// # Errors
    ///
    /// Returns an error if no chunk is supplied or the V6 skeleton cannot be
    /// compiled.
    pub fn cave_topology_plan_from_chunks(
        &self,
        config: &WorldgenConfigV1,
        chunks: impl IntoIterator<Item = ChunkCoordinate>,
    ) -> TerritoryResult<CaveTopologyPlanV1> {
        CaveTopologyPlanV1::from_chunks(
            self.world_seed(),
            self.plan_hash(),
            self.default_cave_domain(),
            self.underground_territories(),
            self.cave_portals(),
            self.contributions(),
            config,
            chunks,
        )
    }
}

fn collect_coverage(
    plan: &TerritoryPlanV1,
    lock: LockedClosureFingerprintV1,
    config: &WorldgenConfigV1,
    bounds: PlanningCellBoundsV1,
    underground_y: i32,
    cells: Vec<PlanningCellCoordinateV1>,
) -> TerritoryResult<TerritoryQueryCoverageV1> {
    let config_hash =
        config
            .canonical_hash()
            .map_err(|error| TerritoryError::CanonicalEncoding {
                kind: "worldgen config",
                reason: error.to_string(),
            })?;
    let mut surface = Vec::with_capacity(cells.len());
    let mut underground = Vec::with_capacity(cells.len());
    for cell in cells {
        surface.push(plan.surface_territory_query(cell)?);
        underground.push(plan.underground_territory_query(cell, underground_y)?);
    }
    Ok(TerritoryQueryCoverageV1 {
        world_seed: plan.world_seed(),
        locked_closure_fingerprint: lock,
        config_hash,
        plan_hash: plan.plan_hash(),
        bounds,
        underground_y,
        surface,
        underground,
    })
}

fn check_query_range_len(actual: u64, plan: &TerritoryPlanV1) -> TerritoryResult<()> {
    if actual > plan.limits().max_statistics_cells {
        Err(TerritoryError::StatisticsRegionTooLarge {
            actual,
            limit: plan.limits().max_statistics_cells,
        })
    } else {
        Ok(())
    }
}

fn surface_secondary(
    plan: &TerritoryPlanV1,
    query: &TerritoryQueryV1,
) -> (TerritoryDomainIdV1, Option<StableId>) {
    let primary = query.terrain_domain();
    for level in query.levels().iter().rev() {
        if let Some(runner_up) = level.runner_up_candidate()
            && let Some(domain) = candidate_domain(plan, runner_up)
            && domain != primary
        {
            return (domain.clone(), Some(runner_up.clone()));
        }
    }
    (terrain_parent(plan, primary), None)
}

fn surface_boundary_distance(query: &TerritoryQueryV1) -> u32 {
    query
        .levels()
        .iter()
        .filter(|level| level.scale().edge_cells().get() > 1)
        .map(crate::TerritoryQueryLevelV1::boundary_distance_cells)
        .min()
        .unwrap_or(0)
}

fn underground_secondary(
    plan: &TerritoryPlanV1,
    cell: PlanningCellCoordinateV1,
    y: i32,
    primary: &CaveTopologyDomainIdV1,
) -> (CaveTopologyDomainIdV1, u32) {
    if let Some(territory) = underground_by_domain(plan, primary) {
        let parent = cave_parent(plan, territory);
        return (parent, underground_boundary_distance(territory, cell, y));
    }
    nearest_underground_child(plan, cell, y).map_or_else(
        || (primary.clone(), 0),
        |(territory, distance)| (territory.domain().clone(), distance),
    )
}

fn underground_boundary_distance(
    territory: &UndergroundTerritoryV1,
    cell: PlanningCellCoordinateV1,
    y: i32,
) -> u32 {
    let mut distance = territory.bounds().boundary_distance_cells(cell);
    if y == territory.vertical_range().min_y()
        || y == territory
            .vertical_range()
            .max_y_exclusive()
            .saturating_sub(1)
    {
        distance = 0;
    }
    distance
}

fn nearest_underground_child(
    plan: &TerritoryPlanV1,
    cell: PlanningCellCoordinateV1,
    y: i32,
) -> Option<(&UndergroundTerritoryV1, u32)> {
    plan.underground_territories()
        .iter()
        .filter(|territory| territory.vertical_range().contains(y))
        .map(|territory| (territory, territory.bounds().boundary_distance_cells(cell)))
        .min_by_key(|(territory, distance)| (*distance, territory.domain().as_str()))
}

fn candidate_domain<'a>(
    plan: &'a TerritoryPlanV1,
    candidate_id: &StableId,
) -> Option<&'a TerritoryDomainIdV1> {
    plan.surface_candidates()
        .binary_search_by(|candidate| candidate.candidate_id().cmp(candidate_id))
        .ok()
        .map(|index| plan.surface_candidates()[index].domain())
}

fn terrain_parent(plan: &TerritoryPlanV1, domain: &TerritoryDomainIdV1) -> TerritoryDomainIdV1 {
    plan.surface_candidates()
        .iter()
        .find(|candidate| candidate.domain() == domain)
        .map_or_else(
            || plan.default_terrain_domain().clone(),
            |candidate| candidate.parent_domain().clone(),
        )
}

fn cave_parent(
    plan: &TerritoryPlanV1,
    territory: &UndergroundTerritoryV1,
) -> CaveTopologyDomainIdV1 {
    match territory.parent() {
        CaveTopologyParentV1::DimensionDefault => plan.default_cave_domain().clone(),
        CaveTopologyParentV1::Territory(parent) => parent.clone(),
    }
}

fn underground_by_domain<'a>(
    plan: &'a TerritoryPlanV1,
    domain: &CaveTopologyDomainIdV1,
) -> Option<&'a UndergroundTerritoryV1> {
    plan.underground_territories()
        .iter()
        .find(|territory| territory.domain() == domain)
}

fn resolved_owner<'a>(
    plan: &'a TerritoryPlanV1,
    domain: &PrimaryOwnershipDomainV1,
) -> TerritoryResult<&'a ResolvedPrimaryOwnerV1> {
    plan.primary_owners()
        .iter()
        .find(|owner| owner.domain() == domain)
        .ok_or_else(|| TerritoryError::MissingPrimaryOwner {
            channel: domain.channel().as_str(),
            domain: domain.as_str().to_owned(),
        })
}
