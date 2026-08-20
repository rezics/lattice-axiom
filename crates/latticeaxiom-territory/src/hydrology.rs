//! Bounded abstract hydrology planning without fluid realization.

use std::collections::BTreeMap;

use latticeaxiom_core::{StableId, canonical_json_hash};
use serde::{Deserialize, Serialize};

use crate::{
    CaveTopologyDomainIdV1, HydrologyPlanHashV1, PlanningCellBoundsV1, TerritoryError,
    TerritoryLimitsV1, TerritoryResult, VerticalRangeV1,
};

/// Cardinal direction used by an abstract drainage connection.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum CardinalDirectionV1 {
    /// Decreasing z.
    North,
    /// Increasing x.
    East,
    /// Increasing z.
    South,
    /// Decreasing x.
    West,
}

/// One finite basin in an abstract cave hydrology plan.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HydrologyBasinV1 {
    basin_id: StableId,
    topology_domain: CaveTopologyDomainIdV1,
    bounds: PlanningCellBoundsV1,
    vertical_range: VerticalRangeV1,
    elevation_rank: i32,
    capacity_units: u64,
}

impl HydrologyBasinV1 {
    /// Creates a finite abstract basin.
    ///
    /// # Errors
    ///
    /// Returns an error unless the ID kind is hydrology-basin and capacity is non-zero.
    pub fn new(
        basin_id: StableId,
        topology_domain: CaveTopologyDomainIdV1,
        bounds: PlanningCellBoundsV1,
        vertical_range: VerticalRangeV1,
        elevation_rank: i32,
        capacity_units: u64,
    ) -> TerritoryResult<Self> {
        if basin_id.kind() != "hydrology-basin" {
            return Err(TerritoryError::InvalidStableKind {
                value: basin_id.to_string(),
                expected: "hydrology-basin",
            });
        }
        if capacity_units == 0 {
            return Err(TerritoryError::InvalidHydrologyPlan {
                reason: format!("basin {basin_id} has zero capacity"),
            });
        }
        Ok(Self {
            basin_id,
            topology_domain,
            bounds,
            vertical_range,
            elevation_rank,
            capacity_units,
        })
    }

    /// Returns the stable basin identity.
    #[must_use]
    pub const fn basin_id(&self) -> &StableId {
        &self.basin_id
    }

    /// Returns its cave topology ownership domain.
    #[must_use]
    pub const fn topology_domain(&self) -> &CaveTopologyDomainIdV1 {
        &self.topology_domain
    }

    /// Returns its finite horizontal bounds.
    #[must_use]
    pub const fn bounds(&self) -> PlanningCellBoundsV1 {
        self.bounds
    }

    /// Returns its finite vertical range.
    #[must_use]
    pub const fn vertical_range(&self) -> VerticalRangeV1 {
        self.vertical_range
    }

    /// Returns the monotonic drainage rank.
    #[must_use]
    pub const fn elevation_rank(&self) -> i32 {
        self.elevation_rank
    }
}

/// A directed connection between two abstract hydrology basins.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HydrologyConnectionV1 {
    connection_id: StableId,
    source_basin: StableId,
    destination_basin: StableId,
    direction: CardinalDirectionV1,
    maximum_flow_units: u64,
}

impl HydrologyConnectionV1 {
    /// Creates a directed, capacity-bounded abstract connection.
    ///
    /// # Errors
    ///
    /// Returns an error for the wrong ID kind, a self-loop, or zero capacity.
    pub fn new(
        connection_id: StableId,
        source_basin: StableId,
        destination_basin: StableId,
        direction: CardinalDirectionV1,
        maximum_flow_units: u64,
    ) -> TerritoryResult<Self> {
        if connection_id.kind() != "hydrology-link" {
            return Err(TerritoryError::InvalidStableKind {
                value: connection_id.to_string(),
                expected: "hydrology-link",
            });
        }
        if source_basin == destination_basin || maximum_flow_units == 0 {
            return Err(TerritoryError::InvalidHydrologyPlan {
                reason: format!(
                    "connection {connection_id} must join distinct basins with non-zero capacity"
                ),
            });
        }
        Ok(Self {
            connection_id,
            source_basin,
            destination_basin,
            direction,
            maximum_flow_units,
        })
    }

    /// Returns the stable connection identity.
    #[must_use]
    pub const fn connection_id(&self) -> &StableId {
        &self.connection_id
    }

    /// Returns the source basin identity.
    #[must_use]
    pub const fn source_basin(&self) -> &StableId {
        &self.source_basin
    }

    /// Returns the destination basin identity.
    #[must_use]
    pub const fn destination_basin(&self) -> &StableId {
        &self.destination_basin
    }
}

#[derive(Serialize)]
#[serde(deny_unknown_fields)]
struct HydrologyHashPayloadV1<'a> {
    basins: &'a [HydrologyBasinV1],
    connections: &'a [HydrologyConnectionV1],
}

/// A validated, deterministic abstract hydrology plan.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HydrologyPlanV1 {
    basins: Vec<HydrologyBasinV1>,
    connections: Vec<HydrologyConnectionV1>,
    plan_hash: HydrologyPlanHashV1,
}

impl HydrologyPlanV1 {
    /// Compiles sorted basins and monotonic connections into an abstract plan.
    ///
    /// This contract carries topology and capacities only. It never represents
    /// fluid blocks, simulation state, pressure, or per-frame flow.
    ///
    /// # Errors
    ///
    /// Returns an error for limits, duplicates, unknown endpoints, uphill
    /// connections, or canonical encoding failure.
    pub fn compile(
        mut basins: Vec<HydrologyBasinV1>,
        mut connections: Vec<HydrologyConnectionV1>,
        limits: TerritoryLimitsV1,
    ) -> TerritoryResult<Self> {
        limits.check_count(
            "hydrology basins",
            basins.len(),
            limits.max_hydrology_basins,
        )?;
        limits.check_count(
            "hydrology connections",
            connections.len(),
            limits.max_hydrology_connections,
        )?;
        basins.sort_by(|left, right| left.basin_id.cmp(&right.basin_id));
        connections.sort_by(|left, right| left.connection_id.cmp(&right.connection_id));
        if basins
            .windows(2)
            .any(|pair| pair[0].basin_id == pair[1].basin_id)
        {
            return Err(TerritoryError::InvalidHydrologyPlan {
                reason: "basin identities must be unique".to_owned(),
            });
        }
        if connections
            .windows(2)
            .any(|pair| pair[0].connection_id == pair[1].connection_id)
        {
            return Err(TerritoryError::InvalidHydrologyPlan {
                reason: "connection identities must be unique".to_owned(),
            });
        }
        let by_id = basins
            .iter()
            .map(|basin| (&basin.basin_id, basin))
            .collect::<BTreeMap<_, _>>();
        for connection in &connections {
            let source = by_id.get(&connection.source_basin).ok_or_else(|| {
                TerritoryError::InvalidHydrologyPlan {
                    reason: format!(
                        "connection {} has unknown source basin {}",
                        connection.connection_id, connection.source_basin
                    ),
                }
            })?;
            let destination = by_id.get(&connection.destination_basin).ok_or_else(|| {
                TerritoryError::InvalidHydrologyPlan {
                    reason: format!(
                        "connection {} has unknown destination basin {}",
                        connection.connection_id, connection.destination_basin
                    ),
                }
            })?;
            if source.elevation_rank <= destination.elevation_rank {
                return Err(TerritoryError::InvalidHydrologyPlan {
                    reason: format!(
                        "connection {} must descend from a greater elevation rank",
                        connection.connection_id
                    ),
                });
            }
        }
        let canonical = canonical_json_hash(&HydrologyHashPayloadV1 {
            basins: &basins,
            connections: &connections,
        })
        .map_err(|error| TerritoryError::CanonicalEncoding {
            kind: "abstract hydrology plan",
            reason: error.to_string(),
        })?;
        Ok(Self {
            basins,
            connections,
            plan_hash: HydrologyPlanHashV1::from_hash(canonical),
        })
    }

    /// Revalidates the serialized plan and its canonical hash.
    ///
    /// # Errors
    ///
    /// Returns an error when normalized content or the stored hash differs.
    pub fn validate(&self, limits: TerritoryLimitsV1) -> TerritoryResult<()> {
        let rebuilt = Self::compile(self.basins.clone(), self.connections.clone(), limits)?;
        if rebuilt != *self {
            return Err(TerritoryError::InvalidHydrologyPlan {
                reason: "serialized plan is not canonical or its hash is stale".to_owned(),
            });
        }
        Ok(())
    }

    /// Returns sorted basins.
    #[must_use]
    pub fn basins(&self) -> &[HydrologyBasinV1] {
        &self.basins
    }

    /// Returns sorted directed connections.
    #[must_use]
    pub fn connections(&self) -> &[HydrologyConnectionV1] {
        &self.connections
    }

    /// Finds an abstract basin by stable identity.
    #[must_use]
    pub fn basin(&self, basin_id: &StableId) -> Option<&HydrologyBasinV1> {
        self.basins
            .binary_search_by(|basin| basin.basin_id.cmp(basin_id))
            .ok()
            .map(|index| &self.basins[index])
    }

    /// Finds an abstract drainage connection by stable identity.
    #[must_use]
    pub fn connection(&self, connection_id: &StableId) -> Option<&HydrologyConnectionV1> {
        self.connections
            .binary_search_by(|connection| connection.connection_id.cmp(connection_id))
            .ok()
            .map(|index| &self.connections[index])
    }

    /// Returns the canonical abstract-plan hash.
    #[must_use]
    pub const fn plan_hash(&self) -> HydrologyPlanHashV1 {
        self.plan_hash
    }
}
