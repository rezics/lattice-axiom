//! Underground ownership domains and validated cave portal contracts.

use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
    str::FromStr,
};

use latticeaxiom_core::{StableId, canonical_json_hash};
use latticeaxiom_worldgen::PlanningCellCoordinateV1;
use serde::{Deserialize, Deserializer, Serialize, de};

use crate::{
    CavePortalIdV1, HydrologyPlanV1, PlanningCellBoundsV1, TerritoryError, TerritoryLimitsV1,
    TerritoryResult, VerticalRangeV1,
};

/// Stable cave-topology ownership domain identity.
#[repr(transparent)]
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct CaveTopologyDomainIdV1(StableId);

impl CaveTopologyDomainIdV1 {
    /// Validates a stable ID as a cave topology domain.
    ///
    /// # Errors
    ///
    /// Returns an error unless the registration kind is cave-topology-domain.
    pub fn new(value: StableId) -> TerritoryResult<Self> {
        if value.kind() != "cave-topology-domain" {
            return Err(TerritoryError::InvalidStableKind {
                value: value.to_string(),
                expected: "cave-topology-domain",
            });
        }
        Ok(Self(value))
    }

    /// Returns the underlying stable identifier.
    #[must_use]
    pub const fn as_stable_id(&self) -> &StableId {
        &self.0
    }

    /// Returns canonical identity text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

impl fmt::Display for CaveTopologyDomainIdV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

impl FromStr for CaveTopologyDomainIdV1 {
    type Err = TerritoryError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::new(value.parse()?)
    }
}

impl<'de> Deserialize<'de> for CaveTopologyDomainIdV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = StableId::deserialize(deserializer)?;
        Self::new(value).map_err(de::Error::custom)
    }
}

/// Parent reference for an underground child territory.
#[derive(Clone, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum CaveTopologyParentV1 {
    /// Inherit directly from the dimension default cave domain.
    DimensionDefault,
    /// Inherit from another bounded underground territory.
    Territory(CaveTopologyDomainIdV1),
}

/// One bounded underground territory delegated from the dimension default.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct UndergroundTerritoryV1 {
    domain: CaveTopologyDomainIdV1,
    parent: CaveTopologyParentV1,
    bounds: PlanningCellBoundsV1,
    vertical_range: VerticalRangeV1,
}

impl UndergroundTerritoryV1 {
    /// Creates a bounded underground child territory.
    #[must_use]
    pub const fn new(
        domain: CaveTopologyDomainIdV1,
        parent: CaveTopologyParentV1,
        bounds: PlanningCellBoundsV1,
        vertical_range: VerticalRangeV1,
    ) -> Self {
        Self {
            domain,
            parent,
            bounds,
            vertical_range,
        }
    }

    /// Returns its topology ownership domain.
    #[must_use]
    pub const fn domain(&self) -> &CaveTopologyDomainIdV1 {
        &self.domain
    }

    /// Returns its parent.
    #[must_use]
    pub const fn parent(&self) -> &CaveTopologyParentV1 {
        &self.parent
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
}

/// World axis used as a cave portal tangent.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum AxisV1 {
    /// Positive or negative x alignment.
    X,
    /// Positive or negative y alignment.
    Y,
    /// Positive or negative z alignment.
    Z,
}

/// Abstract water compatibility at a cave portal.
#[derive(Clone, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum PortalHydrologyContractV1 {
    /// No planned cross-boundary drainage.
    Dry,
    /// Topology must remain sealed to abstract hydrology.
    Sealed,
    /// Portal carries one declared abstract drainage connection.
    Drainage {
        /// Stable hydrology-link identity.
        connection_id: StableId,
    },
}

/// A direction-independent, finite portal across a cave ownership boundary.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CavePortalV1 {
    portal_id: CavePortalIdV1,
    first_domain: CaveTopologyDomainIdV1,
    second_domain: CaveTopologyDomainIdV1,
    first_cell: PlanningCellCoordinateV1,
    second_cell: PlanningCellCoordinateV1,
    anchor_millimeters: [i64; 3],
    tangent_axis: AxisV1,
    clearance_width_millimeters: u32,
    clearance_height_millimeters: u32,
    hydrology: PortalHydrologyContractV1,
}

#[derive(Serialize)]
struct PortalHashPayloadV1<'a> {
    first_domain: &'a CaveTopologyDomainIdV1,
    second_domain: &'a CaveTopologyDomainIdV1,
    first_cell: PlanningCellCoordinateV1,
    second_cell: PlanningCellCoordinateV1,
    anchor_millimeters: [i64; 3],
    tangent_axis: AxisV1,
    clearance_width_millimeters: u32,
    clearance_height_millimeters: u32,
    hydrology: &'a PortalHydrologyContractV1,
}

impl CavePortalV1 {
    /// Validates and canonically orders a cave ownership-boundary portal.
    ///
    /// # Errors
    ///
    /// Returns an error unless endpoints are cardinal neighbors in distinct
    /// domains, clearance is non-zero, the tangent lies in the boundary plane,
    /// and a drainage contract names a hydrology-link ID.
    #[allow(
        clippy::too_many_arguments,
        reason = "portal evidence is deliberately explicit"
    )]
    pub fn new(
        first_domain: CaveTopologyDomainIdV1,
        first_cell: PlanningCellCoordinateV1,
        second_domain: CaveTopologyDomainIdV1,
        second_cell: PlanningCellCoordinateV1,
        anchor_millimeters: [i64; 3],
        tangent_axis: AxisV1,
        clearance_width_millimeters: u32,
        clearance_height_millimeters: u32,
        hydrology: PortalHydrologyContractV1,
    ) -> TerritoryResult<Self> {
        validate_cardinal(first_cell, second_cell)?;
        if first_domain == second_domain {
            return Err(TerritoryError::InvalidCaveAdjacency {
                reason: "portal endpoints must cross distinct topology ownership domains"
                    .to_owned(),
            });
        }
        if clearance_width_millimeters == 0 || clearance_height_millimeters == 0 {
            return Err(TerritoryError::InvalidCaveAdjacency {
                reason: "portal clearance must be non-zero".to_owned(),
            });
        }
        let normal = if first_cell.x == second_cell.x {
            AxisV1::Z
        } else {
            AxisV1::X
        };
        if tangent_axis == normal {
            return Err(TerritoryError::InvalidCaveAdjacency {
                reason: "portal tangent must lie in the planning-cell boundary plane".to_owned(),
            });
        }
        if let PortalHydrologyContractV1::Drainage { connection_id } = &hydrology
            && connection_id.kind() != "hydrology-link"
        {
            return Err(TerritoryError::InvalidStableKind {
                value: connection_id.to_string(),
                expected: "hydrology-link",
            });
        }
        let ((first_domain, first_cell), (second_domain, second_cell)) =
            if (first_cell, &first_domain) <= (second_cell, &second_domain) {
                ((first_domain, first_cell), (second_domain, second_cell))
            } else {
                ((second_domain, second_cell), (first_domain, first_cell))
            };
        let payload = PortalHashPayloadV1 {
            first_domain: &first_domain,
            second_domain: &second_domain,
            first_cell,
            second_cell,
            anchor_millimeters,
            tangent_axis,
            clearance_width_millimeters,
            clearance_height_millimeters,
            hydrology: &hydrology,
        };
        let hash =
            canonical_json_hash(&payload).map_err(|error| TerritoryError::CanonicalEncoding {
                kind: "cave portal",
                reason: error.to_string(),
            })?;
        Ok(Self {
            portal_id: CavePortalIdV1::from_hash(hash),
            first_domain,
            second_domain,
            first_cell,
            second_cell,
            anchor_millimeters,
            tangent_axis,
            clearance_width_millimeters,
            clearance_height_millimeters,
            hydrology,
        })
    }

    /// Returns the direction-independent portal identity.
    #[must_use]
    pub const fn portal_id(&self) -> CavePortalIdV1 {
        self.portal_id
    }

    /// Returns canonical endpoint cells.
    #[must_use]
    pub const fn cells(&self) -> (PlanningCellCoordinateV1, PlanningCellCoordinateV1) {
        (self.first_cell, self.second_cell)
    }

    /// Returns canonical endpoint domains.
    #[must_use]
    pub const fn domains(&self) -> (&CaveTopologyDomainIdV1, &CaveTopologyDomainIdV1) {
        (&self.first_domain, &self.second_domain)
    }

    /// Returns abstract hydrology compatibility.
    #[must_use]
    pub const fn hydrology(&self) -> &PortalHydrologyContractV1 {
        &self.hydrology
    }
}

/// Canonical portal evidence between two adjacent planning cells.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CaveAdjacencyV1 {
    first_cell: PlanningCellCoordinateV1,
    second_cell: PlanningCellCoordinateV1,
    portal_ids: Vec<CavePortalIdV1>,
}

impl CaveAdjacencyV1 {
    /// Returns the canonical adjacent cell pair.
    #[must_use]
    pub const fn cells(&self) -> (PlanningCellCoordinateV1, PlanningCellCoordinateV1) {
        (self.first_cell, self.second_cell)
    }

    /// Returns sorted required portal identities.
    #[must_use]
    pub fn portal_ids(&self) -> &[CavePortalIdV1] {
        &self.portal_ids
    }

    /// Revalidates canonical cell and portal ordering after deserialization.
    ///
    /// # Errors
    ///
    /// Returns an error for non-adjacent or reversed cells, or an empty,
    /// duplicated, or unsorted portal list.
    pub fn validate(&self) -> TerritoryResult<()> {
        validate_cardinal(self.first_cell, self.second_cell)?;
        if self.first_cell >= self.second_cell
            || self.portal_ids.is_empty()
            || self.portal_ids.windows(2).any(|pair| pair[0] >= pair[1])
        {
            return Err(TerritoryError::InvalidCaveAdjacency {
                reason: "cave adjacency evidence is not canonical".to_owned(),
            });
        }
        Ok(())
    }
}

pub(crate) fn validate_underground(
    default_domain: &CaveTopologyDomainIdV1,
    territories: &mut [UndergroundTerritoryV1],
    limits: TerritoryLimitsV1,
) -> TerritoryResult<()> {
    limits.check_count(
        "underground territories",
        territories.len(),
        limits.max_underground_territories,
    )?;
    territories.sort_by(|left, right| left.domain.cmp(&right.domain));
    if territories
        .windows(2)
        .any(|pair| pair[0].domain == pair[1].domain)
        || territories
            .iter()
            .any(|territory| &territory.domain == default_domain)
    {
        return Err(TerritoryError::InvalidUndergroundTerritory {
            territory: default_domain.to_string(),
            reason: "topology ownership domain identities must be unique".to_owned(),
        });
    }
    let by_domain = territories
        .iter()
        .map(|territory| (&territory.domain, territory))
        .collect::<BTreeMap<_, _>>();
    for territory in territories.iter() {
        let mut cursor = territory;
        let mut visited = BTreeSet::new();
        while let CaveTopologyParentV1::Territory(parent) = &cursor.parent {
            if !visited.insert(parent) {
                return Err(TerritoryError::InvalidUndergroundTerritory {
                    territory: territory.domain.to_string(),
                    reason: "parent chain contains a cycle".to_owned(),
                });
            }
            let parent_territory = by_domain.get(parent).ok_or_else(|| {
                TerritoryError::InvalidUndergroundTerritory {
                    territory: territory.domain.to_string(),
                    reason: format!("parent domain {parent} is not declared"),
                }
            })?;
            if !parent_territory.bounds.contains_bounds(cursor.bounds)
                || !parent_territory
                    .vertical_range
                    .contains_range(cursor.vertical_range)
            {
                return Err(TerritoryError::InvalidUndergroundTerritory {
                    territory: territory.domain.to_string(),
                    reason: format!("child extent is not contained by parent {parent}"),
                });
            }
            cursor = parent_territory;
        }
    }
    for (index, left) in territories.iter().enumerate() {
        for right in &territories[index + 1..] {
            if left.parent == right.parent
                && left.bounds.overlaps(right.bounds)
                && left.vertical_range.overlaps(right.vertical_range)
            {
                return Err(TerritoryError::InvalidUndergroundTerritory {
                    territory: right.domain.to_string(),
                    reason: format!("overlaps sibling ownership domain {}", left.domain),
                });
            }
        }
    }
    Ok(())
}

pub(crate) fn validate_portals(
    default_domain: &CaveTopologyDomainIdV1,
    territories: &[UndergroundTerritoryV1],
    portals: &mut [CavePortalV1],
    hydrology: &HydrologyPlanV1,
    limits: TerritoryLimitsV1,
) -> TerritoryResult<Vec<CaveAdjacencyV1>> {
    limits.check_count("cave portals", portals.len(), limits.max_portals)?;
    portals.sort_by_key(CavePortalV1::portal_id);
    if portals
        .windows(2)
        .any(|pair| pair[0].portal_id == pair[1].portal_id)
    {
        return Err(TerritoryError::InvalidCaveAdjacency {
            reason: "portal identities must be unique".to_owned(),
        });
    }
    for portal in portals.iter() {
        validate_portal(default_domain, territories, portal, hydrology)?;
    }
    let mut grouped =
        BTreeMap::<(PlanningCellCoordinateV1, PlanningCellCoordinateV1), Vec<CavePortalIdV1>>::new(
        );
    for portal in portals.iter() {
        grouped
            .entry((portal.first_cell, portal.second_cell))
            .or_default()
            .push(portal.portal_id);
    }
    Ok(grouped
        .into_iter()
        .map(|((first_cell, second_cell), mut portal_ids)| {
            portal_ids.sort();
            CaveAdjacencyV1 {
                first_cell,
                second_cell,
                portal_ids,
            }
        })
        .collect())
}

fn validate_portal(
    default_domain: &CaveTopologyDomainIdV1,
    territories: &[UndergroundTerritoryV1],
    portal: &CavePortalV1,
    hydrology: &HydrologyPlanV1,
) -> TerritoryResult<()> {
    let portal_y = i32::try_from(portal.anchor_millimeters[1].div_euclid(1_000)).map_err(|_| {
        TerritoryError::InvalidCaveAdjacency {
            reason: "portal anchor y does not fit the world-coordinate contract".to_owned(),
        }
    })?;
    for (domain, cell) in [
        (&portal.first_domain, portal.first_cell),
        (&portal.second_domain, portal.second_cell),
    ] {
        if domain == default_domain {
            if territories.iter().any(|territory| {
                territory.bounds.contains(cell) && territory.vertical_range.contains(portal_y)
            }) {
                return Err(TerritoryError::InvalidCaveAdjacency {
                    reason: format!(
                        "default-domain portal endpoint {cell:?} is shadowed by a child territory"
                    ),
                });
            }
        } else {
            let territory = territories
                .iter()
                .find(|territory| territory.domain() == domain)
                .ok_or_else(|| TerritoryError::InvalidCaveAdjacency {
                    reason: format!("portal references unknown domain {domain}"),
                })?;
            if !territory.bounds.contains(cell) || !territory.vertical_range.contains(portal_y) {
                return Err(TerritoryError::InvalidCaveAdjacency {
                    reason: format!(
                        "portal anchor at cell {cell:?}, y={portal_y} lies outside domain {domain}"
                    ),
                });
            }
        }
    }
    if let PortalHydrologyContractV1::Drainage { connection_id } = &portal.hydrology {
        let connection = hydrology.connection(connection_id).ok_or_else(|| {
            TerritoryError::InvalidCaveAdjacency {
                reason: format!("portal references unknown hydrology link {connection_id}"),
            }
        })?;
        let source = hydrology.basin(connection.source_basin()).ok_or_else(|| {
            TerritoryError::InvalidHydrologyPlan {
                reason: format!("connection {connection_id} references an unknown source basin"),
            }
        })?;
        let destination = hydrology
            .basin(connection.destination_basin())
            .ok_or_else(|| TerritoryError::InvalidHydrologyPlan {
                reason: format!(
                    "connection {connection_id} references an unknown destination basin"
                ),
            })?;
        let mut hydrology_domains = [
            source.topology_domain().clone(),
            destination.topology_domain().clone(),
        ];
        hydrology_domains.sort();
        let mut portal_domains = [portal.first_domain.clone(), portal.second_domain.clone()];
        portal_domains.sort();
        if hydrology_domains != portal_domains {
            return Err(TerritoryError::InvalidCaveAdjacency {
                reason: format!(
                    "hydrology link {connection_id} does not connect the portal endpoint domains"
                ),
            });
        }
    }
    Ok(())
}

pub(crate) fn validate_required_portals(
    required: &[CavePortalIdV1],
    available: &[CavePortalIdV1],
) -> TerritoryResult<()> {
    let available = available.iter().copied().collect::<BTreeSet<_>>();
    for portal in required {
        if !available.contains(portal) {
            return Err(TerritoryError::MissingRequiredPortal {
                portal: portal.to_string(),
            });
        }
    }
    Ok(())
}

pub(crate) fn validate_cardinal(
    first: PlanningCellCoordinateV1,
    second: PlanningCellCoordinateV1,
) -> TerritoryResult<()> {
    let delta_x = (i128::from(first.x) - i128::from(second.x)).abs();
    let delta_z = (i128::from(first.z) - i128::from(second.z)).abs();
    if delta_x + delta_z == 1 {
        Ok(())
    } else {
        Err(TerritoryError::non_adjacent(first, second))
    }
}
