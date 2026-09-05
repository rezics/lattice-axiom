//! Underground ownership domains and validated cave portal contracts.

use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
    str::FromStr,
};

use latticeaxiom_core::{CanonicalHash, StableId, canonical_json_hash};
use latticeaxiom_worldgen::{
    CaveBranchContributorV1, CaveLayerCorridorV1, CaveLayerEntranceV1, CaveLayerPortalV1,
    CaveOwnedDomainV1, CaveTopologyAlgorithmV1, CaveTopologyLayerInputV1, ChunkCoordinate,
    PlanningCellCoordinateV1, WorldSeedV1, WorldgenConfigV1, millimeters_to_voxels,
};
use serde::{Deserialize, Deserializer, Serialize, de};

use crate::{
    AtlasPlanHashV1, CaveEntranceIdV1, CavePassabilityReceiptHashV1, CavePortalIdV1,
    CaveTopologyNodeIdV1, CaveTopologyPlanHashV1, ContributionChannelV1, HydrologyPlanV1,
    PlanningCellBoundsV1, SpatialContributionV1, TerritoryError, TerritoryLimitsV1,
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

    /// Returns the portal aperture position in world millimeters.
    #[must_use]
    pub const fn anchor_millimeters(&self) -> [i64; 3] {
        self.anchor_millimeters
    }

    /// Returns the portal tangent lying in the shared planning-cell plane.
    #[must_use]
    pub const fn tangent_axis(&self) -> AxisV1 {
        self.tangent_axis
    }

    /// Returns the non-zero portal clearance width in millimeters.
    #[must_use]
    pub const fn clearance_width_millimeters(&self) -> u32 {
        self.clearance_width_millimeters
    }

    /// Returns the non-zero portal clearance height in millimeters.
    #[must_use]
    pub const fn clearance_height_millimeters(&self) -> u32 {
        self.clearance_height_millimeters
    }

    /// Returns machine-readable position, tangent, clearance, and fluid evidence.
    #[must_use]
    pub fn assertion(&self) -> PortalAssertionV1 {
        PortalAssertionV1 {
            portal_id: self.portal_id,
            position_millimeters: self.anchor_millimeters,
            tangent_axis: self.tangent_axis,
            clearance_width_millimeters: self.clearance_width_millimeters,
            clearance_height_millimeters: self.clearance_height_millimeters,
            fluid: self.hydrology.clone(),
        }
    }
}

/// Machine-readable cave portal evidence reused by V6 topology planning.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PortalAssertionV1 {
    portal_id: CavePortalIdV1,
    position_millimeters: [i64; 3],
    tangent_axis: AxisV1,
    clearance_width_millimeters: u32,
    clearance_height_millimeters: u32,
    fluid: PortalHydrologyContractV1,
}

impl PortalAssertionV1 {
    /// Returns the direction-independent portal identity.
    #[must_use]
    pub const fn portal_id(&self) -> CavePortalIdV1 {
        self.portal_id
    }

    /// Returns the portal aperture position in world millimeters.
    #[must_use]
    pub const fn position_millimeters(&self) -> [i64; 3] {
        self.position_millimeters
    }

    /// Returns the portal tangent lying in the shared planning-cell plane.
    #[must_use]
    pub const fn tangent_axis(&self) -> AxisV1 {
        self.tangent_axis
    }

    /// Returns the non-zero portal clearance width in millimeters.
    #[must_use]
    pub const fn clearance_width_millimeters(&self) -> u32 {
        self.clearance_width_millimeters
    }

    /// Returns the non-zero portal clearance height in millimeters.
    #[must_use]
    pub const fn clearance_height_millimeters(&self) -> u32 {
        self.clearance_height_millimeters
    }

    /// Returns abstract fluid compatibility. Hydrology constrains drainage only.
    #[must_use]
    pub const fn fluid(&self) -> &PortalHydrologyContractV1 {
        &self.fluid
    }
}

/// Kind of a compiled cave-topology graph node.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum CaveTopologyNodeKindV1 {
    /// Surface opening in the dimension-default domain.
    SurfaceEntrance,
    /// Cross-domain portal endpoint.
    PortalEndpoint,
    /// Must-connect underground destination.
    Destination,
    /// Intermediate corridor junction.
    Junction,
}

/// One node in the compiled cave-topology graph.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CaveTopologyNodeV1 {
    node_id: CaveTopologyNodeIdV1,
    domain: CaveTopologyDomainIdV1,
    cell: PlanningCellCoordinateV1,
    anchor_millimeters: [i64; 3],
    kind: CaveTopologyNodeKindV1,
}

impl CaveTopologyNodeV1 {
    /// Returns the direction-independent node identity.
    #[must_use]
    pub const fn node_id(&self) -> CaveTopologyNodeIdV1 {
        self.node_id
    }

    /// Returns the topology domain that owns this node.
    #[must_use]
    pub const fn domain(&self) -> &CaveTopologyDomainIdV1 {
        &self.domain
    }

    /// Returns the planning cell containing this node.
    #[must_use]
    pub const fn cell(&self) -> PlanningCellCoordinateV1 {
        self.cell
    }

    /// Returns the node position in world millimeters.
    #[must_use]
    pub const fn anchor_millimeters(&self) -> [i64; 3] {
        self.anchor_millimeters
    }

    /// Returns the node role in the compiled graph.
    #[must_use]
    pub const fn kind(&self) -> CaveTopologyNodeKindV1 {
        self.kind
    }
}

/// One undirected edge in the compiled cave-topology graph.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CaveTopologyEdgeV1 {
    first: CaveTopologyNodeIdV1,
    second: CaveTopologyNodeIdV1,
    algorithm: CaveTopologyAlgorithmV1,
}

impl CaveTopologyEdgeV1 {
    /// Returns the canonical first endpoint.
    #[must_use]
    pub const fn first(&self) -> CaveTopologyNodeIdV1 {
        self.first
    }

    /// Returns the canonical second endpoint.
    #[must_use]
    pub const fn second(&self) -> CaveTopologyNodeIdV1 {
        self.second
    }

    /// Returns the domain algorithm used to realize this edge.
    #[must_use]
    pub const fn algorithm(&self) -> CaveTopologyAlgorithmV1 {
        self.algorithm
    }
}

/// Graph-level passability evidence for one surface entrance.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CavePassabilityReceiptV1 {
    entrance_id: CaveEntranceIdV1,
    destination: CaveMustConnectDestinationV1,
    graph_connected: bool,
    hop_count: u32,
    dead_end_count: u32,
    loop_edge_count: u32,
    receipt_hash: CavePassabilityReceiptHashV1,
}

impl CavePassabilityReceiptV1 {
    /// Returns the surface entrance this receipt validates.
    #[must_use]
    pub const fn entrance_id(&self) -> CaveEntranceIdV1 {
        self.entrance_id
    }

    /// Returns the must-connect destination.
    #[must_use]
    pub const fn destination(&self) -> &CaveMustConnectDestinationV1 {
        &self.destination
    }

    /// Returns whether the compiled graph reaches the destination.
    #[must_use]
    pub const fn graph_connected(&self) -> bool {
        self.graph_connected
    }

    /// Returns the shortest hop count from the surface opening to the destination.
    #[must_use]
    pub const fn hop_count(&self) -> u32 {
        self.hop_count
    }

    /// Returns degree-1 nodes that are not surface openings.
    #[must_use]
    pub const fn dead_end_count(&self) -> u32 {
        self.dead_end_count
    }

    /// Returns extra edges that close a local loop in constrained-graph domains.
    #[must_use]
    pub const fn loop_edge_count(&self) -> u32 {
        self.loop_edge_count
    }

    /// Returns the canonical receipt hash.
    #[must_use]
    pub const fn receipt_hash(&self) -> CavePassabilityReceiptHashV1 {
        self.receipt_hash
    }
}

#[derive(Serialize)]
struct PassabilityHashPayloadV1<'a> {
    entrance_id: CaveEntranceIdV1,
    destination_domain: &'a CaveTopologyDomainIdV1,
    destination_cell: PlanningCellCoordinateV1,
    graph_connected: bool,
    hop_count: u32,
    dead_end_count: u32,
    loop_edge_count: u32,
}

/// Algorithm selected for one topology ownership domain.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CaveDomainAlgorithmV1 {
    domain: CaveTopologyDomainIdV1,
    algorithm: CaveTopologyAlgorithmV1,
    bounds: Option<PlanningCellBoundsV1>,
    vertical_range: Option<VerticalRangeV1>,
}

impl CaveDomainAlgorithmV1 {
    /// Returns the topology ownership domain.
    #[must_use]
    pub const fn domain(&self) -> &CaveTopologyDomainIdV1 {
        &self.domain
    }

    /// Returns the unique local algorithm.
    #[must_use]
    pub const fn algorithm(&self) -> CaveTopologyAlgorithmV1 {
        self.algorithm
    }

    /// Returns finite child-domain bounds, or `None` for the dimension default.
    #[must_use]
    pub const fn bounds(&self) -> Option<PlanningCellBoundsV1> {
        self.bounds
    }

    /// Returns the finite child-domain vertical range, or `None` for the default.
    #[must_use]
    pub const fn vertical_range(&self) -> Option<VerticalRangeV1> {
        self.vertical_range
    }
}

/// Required underground destination that a surface entrance must reach.
#[derive(Clone, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CaveMustConnectDestinationV1 {
    domain: CaveTopologyDomainIdV1,
    cell: PlanningCellCoordinateV1,
    anchor_millimeters: [i64; 3],
}

impl CaveMustConnectDestinationV1 {
    /// Returns the underground topology domain that must remain connected.
    #[must_use]
    pub const fn domain(&self) -> &CaveTopologyDomainIdV1 {
        &self.domain
    }

    /// Returns the destination planning cell.
    #[must_use]
    pub const fn cell(&self) -> PlanningCellCoordinateV1 {
        self.cell
    }

    /// Returns the destination position in world millimeters.
    #[must_use]
    pub const fn anchor_millimeters(&self) -> [i64; 3] {
        self.anchor_millimeters
    }
}

/// A seed-stable cave opening from the dimension-default domain into a child.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CaveSurfaceEntranceV1 {
    entrance_id: CaveEntranceIdV1,
    surface_cell: PlanningCellCoordinateV1,
    surface_anchor_millimeters: [i64; 3],
    cells: Vec<PlanningCellCoordinateV1>,
    domains: Vec<CaveTopologyDomainIdV1>,
    portals: Vec<CavePortalIdV1>,
    destination: CaveMustConnectDestinationV1,
}

impl CaveSurfaceEntranceV1 {
    /// Returns the direction-independent entrance identity.
    #[must_use]
    pub const fn entrance_id(&self) -> CaveEntranceIdV1 {
        self.entrance_id
    }

    /// Returns the default-domain planning cell where the cave meets the surface skeleton.
    #[must_use]
    pub const fn surface_cell(&self) -> PlanningCellCoordinateV1 {
        self.surface_cell
    }

    /// Returns the surface opening position in world millimeters.
    #[must_use]
    pub const fn surface_anchor_millimeters(&self) -> [i64; 3] {
        self.surface_anchor_millimeters
    }

    /// Returns the ordered planning-cell path from the surface opening to the destination.
    #[must_use]
    pub fn cells(&self) -> &[PlanningCellCoordinateV1] {
        &self.cells
    }

    /// Returns distinct topology domains visited by the path, in first-seen order.
    #[must_use]
    pub fn domains(&self) -> &[CaveTopologyDomainIdV1] {
        &self.domains
    }

    /// Returns sorted cross-domain portals used by this entrance.
    #[must_use]
    pub fn portals(&self) -> &[CavePortalIdV1] {
        &self.portals
    }

    /// Returns the underground destination this entrance must connect.
    #[must_use]
    pub const fn destination(&self) -> &CaveMustConnectDestinationV1 {
        &self.destination
    }
}

#[derive(Serialize)]
struct EntranceHashPayloadV1<'a> {
    world_seed: WorldSeedV1,
    surface_cell: PlanningCellCoordinateV1,
    cells: &'a [PlanningCellCoordinateV1],
    domains: &'a [CaveTopologyDomainIdV1],
    portals: &'a [CavePortalIdV1],
    destination_domain: &'a CaveTopologyDomainIdV1,
    destination_cell: PlanningCellCoordinateV1,
}

/// Compiled V6 cave topology plan. Hydrology may constrain drainage portals but
/// never owns cave topology.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CaveTopologyPlanV1 {
    world_seed: WorldSeedV1,
    plan_hash: AtlasPlanHashV1,
    topology_hash: CaveTopologyPlanHashV1,
    default_cave_domain: CaveTopologyDomainIdV1,
    underground_domains: Vec<CaveTopologyDomainIdV1>,
    algorithms: Vec<CaveDomainAlgorithmV1>,
    nodes: Vec<CaveTopologyNodeV1>,
    edges: Vec<CaveTopologyEdgeV1>,
    surface_entrances: Vec<CaveSurfaceEntranceV1>,
    portals: Vec<CavePortalV1>,
    assertions: Vec<PortalAssertionV1>,
    must_connect: Vec<CaveMustConnectDestinationV1>,
    branch: SpatialContributionV1,
    passability: Vec<CavePassabilityReceiptV1>,
}

impl CaveTopologyPlanV1 {
    /// Compiles the default cave domain, underground children, surface
    /// entrances, and must-connect destinations from a validated skeleton.
    ///
    /// # Errors
    ///
    /// Returns an error unless two underground children exist, at least one
    /// surface entrance crosses four planning cells and two topology domains,
    /// and every entrance portal is a compiled cross-domain portal.
    #[allow(
        clippy::too_many_arguments,
        reason = "compile inputs are explicit hash and ownership boundaries"
    )]
    pub fn compile(
        world_seed: WorldSeedV1,
        plan_hash: AtlasPlanHashV1,
        default_cave_domain: &CaveTopologyDomainIdV1,
        underground: &[UndergroundTerritoryV1],
        portals: &[CavePortalV1],
        contributions: &[SpatialContributionV1],
        config: &WorldgenConfigV1,
    ) -> TerritoryResult<Self> {
        if underground.len() < 2 {
            return Err(TerritoryError::InvalidUndergroundTerritory {
                territory: default_cave_domain.to_string(),
                reason: "V6 cave plan requires two underground-owned subdomains".to_owned(),
            });
        }
        let underground_domains = underground
            .iter()
            .map(|territory| territory.domain().clone())
            .collect::<Vec<_>>();
        let mut surface_entrances = Vec::new();
        for territory in underground {
            if let Some(entrance) = plan_surface_entrance(
                world_seed,
                default_cave_domain,
                underground,
                portals,
                territory,
                config,
            )? {
                surface_entrances.push(entrance);
            }
        }
        surface_entrances.sort_by_key(CaveSurfaceEntranceV1::entrance_id);
        if !surface_entrances.iter().any(|entrance| {
            entrance.cells.len() >= 4 && entrance.domains.len() >= 2 && !entrance.portals.is_empty()
        }) {
            return Err(TerritoryError::InvalidCaveAdjacency {
                reason:
                    "V6 cave plan requires a surface entrance across four cells and two domains"
                        .to_owned(),
            });
        }
        assemble_cave_topology_plan(
            world_seed,
            plan_hash,
            default_cave_domain.clone(),
            underground,
            underground_domains,
            surface_entrances,
            portals,
            contributions,
            config,
        )
    }

    /// Collects the seed-stable cave plan for unique planning cells of `chunks`.
    ///
    /// Chunk order cannot change the compiled bytes. Duplicate chunk coordinates
    /// collapse to one planning cell.
    ///
    /// # Errors
    ///
    /// Returns an error if no chunk is supplied or the full V6 skeleton cannot
    /// be compiled.
    #[allow(
        clippy::too_many_arguments,
        reason = "chunk coverage is an explicit extra input over compile"
    )]
    pub fn from_chunks(
        world_seed: WorldSeedV1,
        plan_hash: AtlasPlanHashV1,
        default_cave_domain: &CaveTopologyDomainIdV1,
        underground: &[UndergroundTerritoryV1],
        portals: &[CavePortalV1],
        contributions: &[SpatialContributionV1],
        config: &WorldgenConfigV1,
        chunks: impl IntoIterator<Item = ChunkCoordinate>,
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
                kind: "cave topology plan range",
                minimum: 0,
                maximum: 0,
            });
        }
        let compiled = Self::compile(
            world_seed,
            plan_hash,
            default_cave_domain,
            underground,
            portals,
            contributions,
            config,
        )?;
        Ok(compiled.restrict_to_cells(&cells))
    }

    /// Returns the world seed frozen into this plan.
    #[must_use]
    pub const fn world_seed(&self) -> WorldSeedV1 {
        self.world_seed
    }

    /// Returns the Atlas plan hash this cave plan was compiled from.
    #[must_use]
    pub const fn plan_hash(&self) -> AtlasPlanHashV1 {
        self.plan_hash
    }

    /// Returns the canonical topology-graph hash.
    #[must_use]
    pub const fn topology_hash(&self) -> CaveTopologyPlanHashV1 {
        self.topology_hash
    }

    /// Returns the dimension-default cave topology domain.
    #[must_use]
    pub const fn default_cave_domain(&self) -> &CaveTopologyDomainIdV1 {
        &self.default_cave_domain
    }

    /// Returns canonically ordered underground-owned subdomains.
    #[must_use]
    pub fn underground_domains(&self) -> &[CaveTopologyDomainIdV1] {
        &self.underground_domains
    }

    /// Returns domain-owned topology algorithms, including the dimension default.
    #[must_use]
    pub fn algorithms(&self) -> &[CaveDomainAlgorithmV1] {
        &self.algorithms
    }

    /// Returns compiled topology-graph nodes.
    #[must_use]
    pub fn nodes(&self) -> &[CaveTopologyNodeV1] {
        &self.nodes
    }

    /// Returns compiled topology-graph edges.
    #[must_use]
    pub fn edges(&self) -> &[CaveTopologyEdgeV1] {
        &self.edges
    }

    /// Returns seed-stable surface entrances.
    #[must_use]
    pub fn surface_entrances(&self) -> &[CaveSurfaceEntranceV1] {
        &self.surface_entrances
    }

    /// Returns the cross-domain portals used by the planned entrances.
    #[must_use]
    pub fn portals(&self) -> &[CavePortalV1] {
        &self.portals
    }

    /// Returns machine-readable portal assertions.
    #[must_use]
    pub fn assertions(&self) -> &[PortalAssertionV1] {
        &self.assertions
    }

    /// Returns must-connect underground destinations.
    #[must_use]
    pub fn must_connect(&self) -> &[CaveMustConnectDestinationV1] {
        &self.must_connect
    }

    /// Returns the unique bounded branch contributor.
    #[must_use]
    pub const fn branch(&self) -> &SpatialContributionV1 {
        &self.branch
    }

    /// Returns graph-level passability receipts for compiled entrances.
    #[must_use]
    pub fn passability(&self) -> &[CavePassabilityReceiptV1] {
        &self.passability
    }

    /// Converts this plan into the worldgen realization layer.
    ///
    /// # Errors
    ///
    /// Returns an error if corridor, portal, or branch conversion fails closed.
    pub fn realization_layer(
        &self,
        config: &WorldgenConfigV1,
    ) -> TerritoryResult<CaveTopologyLayerInputV1> {
        realization_layer_from_plan(self, config)
    }

    fn restrict_to_cells(&self, cells: &BTreeSet<PlanningCellCoordinateV1>) -> Self {
        let surface_entrances = self
            .surface_entrances
            .iter()
            .filter(|entrance| entrance.cells.iter().any(|cell| cells.contains(cell)))
            .cloned()
            .collect::<Vec<_>>();
        let mut portal_ids = surface_entrances
            .iter()
            .flat_map(|entrance| entrance.portals.iter().copied())
            .collect::<BTreeSet<_>>();
        portal_ids.extend(self.portals.iter().filter_map(|portal| {
            let (first, second) = portal.cells();
            (cells.contains(&first) || cells.contains(&second)).then_some(portal.portal_id())
        }));
        let portals = self
            .portals
            .iter()
            .filter(|portal| portal_ids.contains(&portal.portal_id()))
            .cloned()
            .collect::<Vec<_>>();
        let assertions = portals
            .iter()
            .map(CavePortalV1::assertion)
            .collect::<Vec<_>>();
        let must_connect = surface_entrances
            .iter()
            .map(|entrance| entrance.destination.clone())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        let node_ids = self
            .nodes
            .iter()
            .filter(|node| cells.contains(&node.cell))
            .map(CaveTopologyNodeV1::node_id)
            .collect::<BTreeSet<_>>();
        let nodes = self
            .nodes
            .iter()
            .filter(|node| node_ids.contains(&node.node_id))
            .cloned()
            .collect::<Vec<_>>();
        let edges = self
            .edges
            .iter()
            .filter(|edge| node_ids.contains(&edge.first) && node_ids.contains(&edge.second))
            .cloned()
            .collect::<Vec<_>>();
        let passability = self
            .passability
            .iter()
            .filter(|receipt| {
                surface_entrances
                    .iter()
                    .any(|entrance| entrance.entrance_id() == receipt.entrance_id())
            })
            .cloned()
            .collect::<Vec<_>>();
        Self {
            world_seed: self.world_seed,
            plan_hash: self.plan_hash,
            topology_hash: self.topology_hash,
            default_cave_domain: self.default_cave_domain.clone(),
            underground_domains: self.underground_domains.clone(),
            algorithms: self.algorithms.clone(),
            nodes,
            edges,
            surface_entrances,
            portals,
            assertions,
            must_connect,
            branch: self.branch.clone(),
            passability,
        }
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

#[allow(
    clippy::too_many_arguments,
    reason = "compile inputs are explicit hash and ownership boundaries"
)]
fn assemble_cave_topology_plan(
    world_seed: WorldSeedV1,
    plan_hash: AtlasPlanHashV1,
    default_cave_domain: CaveTopologyDomainIdV1,
    underground: &[UndergroundTerritoryV1],
    underground_domains: Vec<CaveTopologyDomainIdV1>,
    surface_entrances: Vec<CaveSurfaceEntranceV1>,
    portals: &[CavePortalV1],
    contributions: &[SpatialContributionV1],
    config: &WorldgenConfigV1,
) -> TerritoryResult<CaveTopologyPlanV1> {
    let portal_ids = surface_entrances
        .iter()
        .flat_map(|entrance| entrance.portals.iter().copied())
        .collect::<BTreeSet<_>>();
    if portal_ids.is_empty() {
        return Err(TerritoryError::InvalidCaveAdjacency {
            reason: "V6 cave plan requires a cross-domain portal".to_owned(),
        });
    }
    let planned_portals = portals
        .iter()
        .filter(|portal| portal_ids.contains(&portal.portal_id()))
        .cloned()
        .collect::<Vec<_>>();
    if planned_portals.len() != portal_ids.len() {
        return Err(TerritoryError::InvalidCaveAdjacency {
            reason: "entrance portal is missing from the compiled cave skeleton".to_owned(),
        });
    }
    let assertions = planned_portals
        .iter()
        .map(CavePortalV1::assertion)
        .collect::<Vec<_>>();
    let must_connect = surface_entrances
        .iter()
        .map(|entrance| entrance.destination.clone())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    if must_connect.is_empty() {
        return Err(TerritoryError::InvalidCaveAdjacency {
            reason: "V6 cave plan requires a must-connect destination".to_owned(),
        });
    }
    let branch = select_branch_contributor(&default_cave_domain, underground, contributions)?;
    let algorithms = assign_domain_algorithms(&default_cave_domain, underground);
    if algorithms
        .iter()
        .filter(|entry| entry.domain != default_cave_domain)
        .map(CaveDomainAlgorithmV1::algorithm)
        .collect::<BTreeSet<_>>()
        .len()
        < 2
    {
        return Err(TerritoryError::InvalidCaveAdjacency {
            reason: "underground-owned subdomains must use distinct algorithms".to_owned(),
        });
    }
    let (nodes, edges, loop_edge_count) = compile_topology_graph(
        &default_cave_domain,
        underground,
        &algorithms,
        &surface_entrances,
        config,
    )?;
    let passability = compile_passability(&surface_entrances, &nodes, &edges, loop_edge_count)?;
    let mut plan = CaveTopologyPlanV1 {
        world_seed,
        plan_hash,
        topology_hash: CaveTopologyPlanHashV1::from_hash(CanonicalHash::digest([])),
        default_cave_domain,
        underground_domains,
        algorithms,
        nodes,
        edges,
        surface_entrances,
        portals: planned_portals,
        assertions,
        must_connect,
        branch,
        passability,
    };
    plan.topology_hash = CaveTopologyPlanHashV1::from_hash(
        canonical_json_hash(&TopologyHashPayloadV1 {
            world_seed: plan.world_seed,
            plan_hash: plan.plan_hash,
            default_cave_domain: &plan.default_cave_domain,
            algorithms: &plan.algorithms,
            nodes: &plan.nodes,
            edges: &plan.edges,
            surface_entrances: &plan.surface_entrances,
            portals: &plan.portals,
            branch_id: plan.branch.contribution_id(),
            passability: &plan.passability,
        })
        .map_err(|error| TerritoryError::CanonicalEncoding {
            kind: "cave topology plan",
            reason: error.to_string(),
        })?,
    );
    Ok(plan)
}

#[derive(Serialize)]
struct TopologyHashPayloadV1<'a> {
    world_seed: WorldSeedV1,
    plan_hash: AtlasPlanHashV1,
    default_cave_domain: &'a CaveTopologyDomainIdV1,
    algorithms: &'a [CaveDomainAlgorithmV1],
    nodes: &'a [CaveTopologyNodeV1],
    edges: &'a [CaveTopologyEdgeV1],
    surface_entrances: &'a [CaveSurfaceEntranceV1],
    portals: &'a [CavePortalV1],
    branch_id: &'a latticeaxiom_core::StableId,
    passability: &'a [CavePassabilityReceiptV1],
}

fn select_branch_contributor(
    default_domain: &CaveTopologyDomainIdV1,
    underground: &[UndergroundTerritoryV1],
    contributions: &[SpatialContributionV1],
) -> TerritoryResult<SpatialContributionV1> {
    let mut branches = contributions
        .iter()
        .filter(|contribution| contribution.channel() == ContributionChannelV1::CaveBranch)
        .cloned()
        .collect::<Vec<_>>();
    branches.sort_by(|left, right| left.contribution_id().cmp(right.contribution_id()));
    let [branch] = branches.as_slice() else {
        return Err(TerritoryError::InvalidContribution {
            contribution: default_domain.to_string(),
            reason: "V6 cave plan requires exactly one bounded branch contributor".to_owned(),
        });
    };
    let crate::ContributionTargetV1::Cave(target) = branch.target() else {
        return Err(TerritoryError::InvalidContribution {
            contribution: branch.contribution_id().to_string(),
            reason: "branch contributor must target a cave topology domain".to_owned(),
        });
    };
    if target == default_domain
        || !underground
            .iter()
            .any(|territory| territory.domain() == target)
    {
        return Err(TerritoryError::InvalidContribution {
            contribution: branch.contribution_id().to_string(),
            reason: "branch contributor must attach to an underground-owned subdomain".to_owned(),
        });
    }
    Ok(branch.clone())
}

fn assign_domain_algorithms(
    default_domain: &CaveTopologyDomainIdV1,
    underground: &[UndergroundTerritoryV1],
) -> Vec<CaveDomainAlgorithmV1> {
    let mut children = underground
        .iter()
        .map(|territory| {
            (
                territory.domain().clone(),
                territory.bounds(),
                territory.vertical_range(),
            )
        })
        .collect::<Vec<_>>();
    children.sort_by(|left, right| left.0.cmp(&right.0));
    let mut algorithms = vec![CaveDomainAlgorithmV1 {
        domain: default_domain.clone(),
        algorithm: CaveTopologyAlgorithmV1::CoarseCell,
        bounds: None,
        vertical_range: None,
    }];
    for (index, (domain, bounds, vertical_range)) in children.into_iter().enumerate() {
        algorithms.push(CaveDomainAlgorithmV1 {
            domain,
            algorithm: if index == 0 {
                CaveTopologyAlgorithmV1::ConstrainedGraph
            } else {
                CaveTopologyAlgorithmV1::FieldGrowth
            },
            bounds: Some(bounds),
            vertical_range: Some(vertical_range),
        });
    }
    algorithms
}

fn algorithm_for_domain(
    algorithms: &[CaveDomainAlgorithmV1],
    domain: &CaveTopologyDomainIdV1,
) -> CaveTopologyAlgorithmV1 {
    algorithms
        .iter()
        .find(|entry| entry.domain() == domain)
        .map_or(
            CaveTopologyAlgorithmV1::CoarseCell,
            CaveDomainAlgorithmV1::algorithm,
        )
}

fn compile_topology_graph(
    default_domain: &CaveTopologyDomainIdV1,
    underground: &[UndergroundTerritoryV1],
    algorithms: &[CaveDomainAlgorithmV1],
    surface_entrances: &[CaveSurfaceEntranceV1],
    config: &WorldgenConfigV1,
) -> TerritoryResult<(Vec<CaveTopologyNodeV1>, Vec<CaveTopologyEdgeV1>, u32)> {
    let mut nodes =
        BTreeMap::<(PlanningCellCoordinateV1, CaveTopologyDomainIdV1), CaveTopologyNodeV1>::new();
    let mut edges =
        BTreeMap::<(CaveTopologyNodeIdV1, CaveTopologyNodeIdV1), CaveTopologyEdgeV1>::new();
    let mut loop_edge_count = 0_u32;
    for entrance in surface_entrances {
        let y_millimeters = entrance.surface_anchor_millimeters[1];
        let portal_y = i32::try_from(y_millimeters.div_euclid(1_000)).map_err(|_| {
            TerritoryError::InvalidCaveAdjacency {
                reason: "entrance y does not fit the world-coordinate contract".to_owned(),
            }
        })?;
        let mut path_ids = Vec::new();
        for (index, cell) in entrance.cells.iter().enumerate() {
            let domain = topology_domain_at(default_domain, underground, *cell, portal_y).clone();
            let kind = if index == 0 {
                CaveTopologyNodeKindV1::SurfaceEntrance
            } else if index + 1 == entrance.cells.len() {
                CaveTopologyNodeKindV1::Destination
            } else if index == 1 {
                CaveTopologyNodeKindV1::PortalEndpoint
            } else {
                CaveTopologyNodeKindV1::Junction
            };
            let node = insert_node(&mut nodes, domain, *cell, y_millimeters, kind, config)?;
            path_ids.push(node);
        }
        for window in path_ids.windows(2) {
            insert_edge(&mut edges, algorithms, &nodes, window[0], window[1]);
        }
        if path_ids.len() >= 4 {
            let last = path_ids[path_ids.len() - 1];
            let loop_to = path_ids[path_ids.len() - 3];
            if algorithm_for_domain(algorithms, entrance.destination.domain())
                == CaveTopologyAlgorithmV1::ConstrainedGraph
            {
                let before = edges.len();
                insert_edge(&mut edges, algorithms, &nodes, last, loop_to);
                if edges.len() > before {
                    loop_edge_count = loop_edge_count.saturating_add(1);
                }
            }
        }
    }
    Ok((
        nodes.into_values().collect(),
        edges.into_values().collect(),
        loop_edge_count,
    ))
}

fn insert_node(
    nodes: &mut BTreeMap<(PlanningCellCoordinateV1, CaveTopologyDomainIdV1), CaveTopologyNodeV1>,
    domain: CaveTopologyDomainIdV1,
    cell: PlanningCellCoordinateV1,
    y_millimeters: i64,
    kind: CaveTopologyNodeKindV1,
    config: &WorldgenConfigV1,
) -> TerritoryResult<CaveTopologyNodeIdV1> {
    if let Some(existing) = nodes.get(&(cell, domain.clone())) {
        return Ok(existing.node_id);
    }
    let payload = NodeHashPayloadV1 {
        domain: &domain,
        cell,
        kind,
        y_millimeters,
    };
    let hash =
        canonical_json_hash(&payload).map_err(|error| TerritoryError::CanonicalEncoding {
            kind: "cave topology node",
            reason: error.to_string(),
        })?;
    let node = CaveTopologyNodeV1 {
        node_id: CaveTopologyNodeIdV1::from_hash(hash),
        domain: domain.clone(),
        cell,
        anchor_millimeters: cell_anchor_millimeters(cell, y_millimeters, config),
        kind,
    };
    let node_id = node.node_id;
    nodes.insert((cell, domain), node);
    Ok(node_id)
}

#[derive(Serialize)]
struct NodeHashPayloadV1<'a> {
    domain: &'a CaveTopologyDomainIdV1,
    cell: PlanningCellCoordinateV1,
    kind: CaveTopologyNodeKindV1,
    y_millimeters: i64,
}

fn insert_edge(
    edges: &mut BTreeMap<(CaveTopologyNodeIdV1, CaveTopologyNodeIdV1), CaveTopologyEdgeV1>,
    algorithms: &[CaveDomainAlgorithmV1],
    nodes: &BTreeMap<(PlanningCellCoordinateV1, CaveTopologyDomainIdV1), CaveTopologyNodeV1>,
    first: CaveTopologyNodeIdV1,
    second: CaveTopologyNodeIdV1,
) {
    if first == second {
        return;
    }
    let (first, second) = if first <= second {
        (first, second)
    } else {
        (second, first)
    };
    let algorithm = nodes
        .values()
        .find(|node| node.node_id == first)
        .map_or(CaveTopologyAlgorithmV1::CoarseCell, |node| {
            algorithm_for_domain(algorithms, node.domain())
        });
    edges.entry((first, second)).or_insert(CaveTopologyEdgeV1 {
        first,
        second,
        algorithm,
    });
}

fn compile_passability(
    surface_entrances: &[CaveSurfaceEntranceV1],
    nodes: &[CaveTopologyNodeV1],
    edges: &[CaveTopologyEdgeV1],
    loop_edge_count: u32,
) -> TerritoryResult<Vec<CavePassabilityReceiptV1>> {
    let mut adjacency = BTreeMap::<CaveTopologyNodeIdV1, BTreeSet<CaveTopologyNodeIdV1>>::new();
    for edge in edges {
        adjacency.entry(edge.first).or_default().insert(edge.second);
        adjacency.entry(edge.second).or_default().insert(edge.first);
    }
    let mut by_cell_domain = BTreeMap::new();
    for node in nodes {
        by_cell_domain.insert((node.cell, node.domain.clone()), node.node_id);
    }
    let mut dead_end_count = 0_u32;
    for node in nodes {
        let degree = adjacency.get(&node.node_id).map_or(0, BTreeSet::len);
        if degree <= 1 && node.kind != CaveTopologyNodeKindV1::SurfaceEntrance {
            dead_end_count = dead_end_count.saturating_add(1);
        }
    }
    let mut receipts = Vec::new();
    for entrance in surface_entrances {
        let Some(start) = nodes.iter().find(|node| {
            node.cell == entrance.surface_cell
                && node.kind == CaveTopologyNodeKindV1::SurfaceEntrance
        }) else {
            return Err(TerritoryError::InvalidCaveAdjacency {
                reason: "surface entrance is missing from the topology graph".to_owned(),
            });
        };
        let Some(goal) = by_cell_domain.get(&(
            entrance.destination.cell,
            entrance.destination.domain.clone(),
        )) else {
            return Err(TerritoryError::InvalidCaveAdjacency {
                reason: "must-connect destination is missing from the topology graph".to_owned(),
            });
        };
        let (graph_connected, hop_count) = shortest_hops(&adjacency, start.node_id, *goal);
        if !graph_connected {
            return Err(TerritoryError::InvalidCaveAdjacency {
                reason: "must-connect destination is not reachable from a surface entrance"
                    .to_owned(),
            });
        }
        let payload = PassabilityHashPayloadV1 {
            entrance_id: entrance.entrance_id,
            destination_domain: entrance.destination.domain(),
            destination_cell: entrance.destination.cell(),
            graph_connected,
            hop_count,
            dead_end_count,
            loop_edge_count,
        };
        let hash =
            canonical_json_hash(&payload).map_err(|error| TerritoryError::CanonicalEncoding {
                kind: "cave passability receipt",
                reason: error.to_string(),
            })?;
        receipts.push(CavePassabilityReceiptV1 {
            entrance_id: entrance.entrance_id,
            destination: entrance.destination.clone(),
            graph_connected,
            hop_count,
            dead_end_count,
            loop_edge_count,
            receipt_hash: CavePassabilityReceiptHashV1::from_hash(hash),
        });
    }
    receipts.sort_by_key(CavePassabilityReceiptV1::entrance_id);
    Ok(receipts)
}

fn shortest_hops(
    adjacency: &BTreeMap<CaveTopologyNodeIdV1, BTreeSet<CaveTopologyNodeIdV1>>,
    start: CaveTopologyNodeIdV1,
    goal: CaveTopologyNodeIdV1,
) -> (bool, u32) {
    if start == goal {
        return (true, 0);
    }
    let mut seen = BTreeSet::from([start]);
    let mut frontier = vec![(start, 0_u32)];
    let mut index = 0;
    while index < frontier.len() {
        let (node, hops) = frontier[index];
        index = index.saturating_add(1);
        let Some(neighbors) = adjacency.get(&node) else {
            continue;
        };
        for neighbor in neighbors {
            if !seen.insert(*neighbor) {
                continue;
            }
            let next = hops.saturating_add(1);
            if *neighbor == goal {
                return (true, next);
            }
            frontier.push((*neighbor, next));
        }
    }
    (false, 0)
}

#[allow(
    clippy::too_many_lines,
    reason = "realization conversion keeps corridor, portal, and branch contracts in one transaction"
)]
fn realization_layer_from_plan(
    plan: &CaveTopologyPlanV1,
    _config: &WorldgenConfigV1,
) -> TerritoryResult<CaveTopologyLayerInputV1> {
    let mut domains = Vec::new();
    for entry in &plan.algorithms {
        let (Some(bounds), Some(vertical)) = (entry.bounds(), entry.vertical_range()) else {
            continue;
        };
        domains.push(
            CaveOwnedDomainV1::new(
                entry.domain().as_stable_id().clone(),
                entry.algorithm(),
                [bounds.min_x(), bounds.min_z()],
                [bounds.max_x_exclusive(), bounds.max_z_exclusive()],
                vertical.min_y(),
                vertical.max_y_exclusive(),
            )
            .map_err(|error| TerritoryError::InvalidCaveAdjacency {
                reason: error.to_string(),
            })?,
        );
    }
    let mut corridors = Vec::new();
    let by_id = plan
        .nodes
        .iter()
        .map(|node| (node.node_id, node))
        .collect::<BTreeMap<_, _>>();
    for edge in &plan.edges {
        let Some(first) = by_id.get(&edge.first) else {
            continue;
        };
        let Some(second) = by_id.get(&edge.second) else {
            continue;
        };
        corridors.push(CaveLayerCorridorV1::new(
            millimeters_to_voxels(first.anchor_millimeters),
            millimeters_to_voxels(second.anchor_millimeters),
        ));
    }
    let mut portals = Vec::new();
    for portal in &plan.portals {
        portals.push(
            CaveLayerPortalV1::new(
                millimeters_to_voxels(portal.anchor_millimeters()),
                millimeters_to_u16(portal.clearance_width_millimeters())?,
                millimeters_to_u16(portal.clearance_height_millimeters())?,
            )
            .map_err(|error| TerritoryError::InvalidCaveAdjacency {
                reason: error.to_string(),
            })?,
        );
    }
    let mut entrances = Vec::new();
    for entrance in &plan.surface_entrances {
        let cells = entrance
            .cells
            .iter()
            .map(|cell| [cell.x, cell.z])
            .collect::<Vec<_>>();
        entrances.push(
            CaveLayerEntranceV1::new(
                cells,
                entrance.surface_anchor_millimeters[1].div_euclid(1_000),
                [entrance.destination.cell.x, entrance.destination.cell.z],
            )
            .map_err(|error| TerritoryError::InvalidCaveAdjacency {
                reason: error.to_string(),
            })?,
        );
    }
    let crate::ContributionTargetV1::Cave(branch_domain) = plan.branch.target() else {
        return Err(TerritoryError::InvalidContribution {
            contribution: plan.branch.contribution_id().to_string(),
            reason: "branch contributor must target a cave topology domain".to_owned(),
        });
    };
    let vertical =
        plan.branch
            .vertical_range()
            .ok_or_else(|| TerritoryError::InvalidContribution {
                contribution: plan.branch.contribution_id().to_string(),
                reason: "branch contributor requires finite vertical influence".to_owned(),
            })?;
    let branch = CaveBranchContributorV1::new(
        branch_domain.as_stable_id().clone(),
        [plan.branch.bounds().min_x(), plan.branch.bounds().min_z()],
        [
            plan.branch.bounds().max_x_exclusive(),
            plan.branch.bounds().max_z_exclusive(),
        ],
        vertical.min_y(),
        vertical.max_y_exclusive(),
    )
    .map_err(|error| TerritoryError::InvalidCaveAdjacency {
        reason: error.to_string(),
    })?;
    let default_algorithm = algorithm_for_domain(&plan.algorithms, &plan.default_cave_domain);
    CaveTopologyLayerInputV1::new(
        plan.default_cave_domain.as_stable_id().clone(),
        default_algorithm,
        domains,
        corridors,
        portals,
        entrances,
        branch,
    )
    .map_err(|error| TerritoryError::InvalidCaveAdjacency {
        reason: error.to_string(),
    })
}

fn millimeters_to_u16(millimeters: u32) -> TerritoryResult<u16> {
    let voxels = millimeters.saturating_add(999) / 1_000;
    u16::try_from(voxels.max(1)).map_err(|_| TerritoryError::ArithmeticOverflow {
        kind: "portal clearance voxels",
    })
}

fn plan_surface_entrance(
    world_seed: WorldSeedV1,
    default_domain: &CaveTopologyDomainIdV1,
    underground: &[UndergroundTerritoryV1],
    portals: &[CavePortalV1],
    territory: &UndergroundTerritoryV1,
    config: &WorldgenConfigV1,
) -> TerritoryResult<Option<CaveSurfaceEntranceV1>> {
    for portal in portals {
        let Some((surface_cell, underground_cell)) =
            default_child_portal_cells(portal, default_domain, territory)
        else {
            continue;
        };
        let Some(cells) = inward_entrance_path(surface_cell, underground_cell, territory.bounds())
        else {
            continue;
        };
        let portal_y = portal_anchor_y(portal)?;
        let mut domains = Vec::new();
        let mut valid = true;
        for (index, cell) in cells.iter().enumerate() {
            let domain = topology_domain_at(default_domain, underground, *cell, portal_y);
            if index == 0 {
                if domain != default_domain {
                    valid = false;
                    break;
                }
            } else if domain != territory.domain() {
                valid = false;
                break;
            }
            if !domains.iter().any(|seen| seen == domain) {
                domains.push(domain.clone());
            }
        }
        if !valid || cells.len() < 4 || domains.len() < 2 {
            continue;
        }
        let destination_cell = cells[cells.len() - 1];
        let destination = CaveMustConnectDestinationV1 {
            domain: territory.domain().clone(),
            cell: destination_cell,
            anchor_millimeters: cell_anchor_millimeters(
                destination_cell,
                portal.anchor_millimeters[1],
                config,
            ),
        };
        let mut portal_ids = vec![portal.portal_id()];
        portal_ids.sort();
        let payload = EntranceHashPayloadV1 {
            world_seed,
            surface_cell,
            cells: &cells,
            domains: &domains,
            portals: &portal_ids,
            destination_domain: destination.domain(),
            destination_cell: destination.cell(),
        };
        let hash =
            canonical_json_hash(&payload).map_err(|error| TerritoryError::CanonicalEncoding {
                kind: "cave surface entrance",
                reason: error.to_string(),
            })?;
        return Ok(Some(CaveSurfaceEntranceV1 {
            entrance_id: CaveEntranceIdV1::from_hash(hash),
            surface_cell,
            surface_anchor_millimeters: cell_anchor_millimeters(
                surface_cell,
                portal.anchor_millimeters[1],
                config,
            ),
            cells,
            domains,
            portals: portal_ids,
            destination,
        }));
    }
    Ok(None)
}

fn default_child_portal_cells(
    portal: &CavePortalV1,
    default_domain: &CaveTopologyDomainIdV1,
    territory: &UndergroundTerritoryV1,
) -> Option<(PlanningCellCoordinateV1, PlanningCellCoordinateV1)> {
    let (first_domain, second_domain) = portal.domains();
    let (first_cell, second_cell) = portal.cells();
    if first_domain == default_domain
        && second_domain == territory.domain()
        && territory.bounds().contains(second_cell)
    {
        Some((first_cell, second_cell))
    } else if second_domain == default_domain
        && first_domain == territory.domain()
        && territory.bounds().contains(first_cell)
    {
        Some((second_cell, first_cell))
    } else {
        None
    }
}

fn inward_entrance_path(
    surface: PlanningCellCoordinateV1,
    underground: PlanningCellCoordinateV1,
    bounds: PlanningCellBoundsV1,
) -> Option<Vec<PlanningCellCoordinateV1>> {
    let step_x = underground.x.saturating_sub(surface.x);
    let step_z = underground.z.saturating_sub(surface.z);
    if step_x.saturating_abs() + step_z.saturating_abs() != 1 {
        return None;
    }
    let mut cells = vec![surface, underground];
    let mut cursor = underground;
    for _ in 0..2 {
        cursor = PlanningCellCoordinateV1::new(
            cursor.x.saturating_add(step_x),
            cursor.z.saturating_add(step_z),
        );
        if !bounds.contains(cursor) {
            return None;
        }
        cells.push(cursor);
    }
    Some(cells)
}

fn topology_domain_at<'a>(
    default_domain: &'a CaveTopologyDomainIdV1,
    underground: &'a [UndergroundTerritoryV1],
    cell: PlanningCellCoordinateV1,
    y: i32,
) -> &'a CaveTopologyDomainIdV1 {
    let mut current = default_domain;
    loop {
        let child = underground.iter().find(|territory| {
            let parent_matches = match territory.parent() {
                CaveTopologyParentV1::DimensionDefault => current == default_domain,
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

fn portal_anchor_y(portal: &CavePortalV1) -> TerritoryResult<i32> {
    i32::try_from(portal.anchor_millimeters[1].div_euclid(1_000)).map_err(|_| {
        TerritoryError::InvalidCaveAdjacency {
            reason: "portal anchor y does not fit the world-coordinate contract".to_owned(),
        }
    })
}

fn cell_anchor_millimeters(
    cell: PlanningCellCoordinateV1,
    y_millimeters: i64,
    config: &WorldgenConfigV1,
) -> [i64; 3] {
    let edge = i64::from(config.chunk_edge_voxels)
        .saturating_mul(i64::from(config.planning_cell_edge_chunks.max(1)));
    let half = edge.saturating_div(2);
    [
        cell.x
            .saturating_mul(edge)
            .saturating_add(half)
            .saturating_mul(1_000),
        y_millimeters,
        cell.z
            .saturating_mul(edge)
            .saturating_add(half)
            .saturating_mul(1_000),
    ]
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
