//! Optional V6 cave-topology realization layer compiled on the D4 coordinator.
//!
//! The layer is package-owned geometry. D4 plans omit it and keep the coarse
//! cell/branch/portal sampler. Host crates never supply Terrenia biome IDs.

use latticeaxiom_core::{StableId, canonical_json_bytes};
use latticeaxiom_storage::ChunkCoordinate;
use serde::{Deserialize, Serialize};

use crate::{
    CaveTopologyLayerHashV1, ChunkFaceV1, WorldgenConfigV1, WorldgenError, WorldgenResult,
    hashes::domain_hash,
};

const TOPOLOGY_LAYER_DOMAIN: &[u8] = b"latticeaxiom.cave-topology-layer.v1\0";

const COARSE_RADIUS_VOXELS: i64 = 3;
const GRAPH_RADIUS_VOXELS: i64 = 2;
const GROWTH_RADIUS_VOXELS: i64 = 1;
const INFLUENCE_RADIUS_VOXELS: i64 = 4;

/// Distinct cave-topology algorithms used by the default domain and two children.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum CaveTopologyAlgorithmV1 {
    /// Dimension-default coarse chambers along planned corridors.
    CoarseCell,
    /// Karst shortest-path capsules owned by one underground subdomain.
    ConstrainedGraph,
    /// Anisotropic field-growth tubes owned by the other underground subdomain.
    FieldGrowth,
}

impl CaveTopologyAlgorithmV1 {
    /// Returns the normative algorithm name.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::CoarseCell => "coarse-cell",
            Self::ConstrainedGraph => "constrained-graph",
            Self::FieldGrowth => "field-growth",
        }
    }

    const fn radius_voxels(self) -> i64 {
        match self {
            Self::CoarseCell => COARSE_RADIUS_VOXELS,
            Self::ConstrainedGraph => GRAPH_RADIUS_VOXELS,
            Self::FieldGrowth => GROWTH_RADIUS_VOXELS,
        }
    }
}

/// One bounded topology ownership domain with a unique local algorithm.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CaveOwnedDomainV1 {
    domain: StableId,
    algorithm: CaveTopologyAlgorithmV1,
    min_cell: [i64; 2],
    max_cell_exclusive: [i64; 2],
    min_y: i32,
    max_y_exclusive: i32,
}

impl CaveOwnedDomainV1 {
    /// Creates a finite underground-owned topology domain.
    ///
    /// # Errors
    ///
    /// Returns an error unless the identity is a cave-topology-domain and the
    /// bounds are non-empty.
    pub fn new(
        domain: StableId,
        algorithm: CaveTopologyAlgorithmV1,
        min_cell: [i64; 2],
        max_cell_exclusive: [i64; 2],
        min_y: i32,
        max_y_exclusive: i32,
    ) -> WorldgenResult<Self> {
        if domain.kind() != "cave-topology-domain" {
            return Err(invalid_topology(
                "owned domain must be a cave-topology-domain",
            ));
        }
        if min_cell[0] >= max_cell_exclusive[0] || min_cell[1] >= max_cell_exclusive[1] {
            return Err(invalid_topology(
                "owned domain planning-cell bounds are empty",
            ));
        }
        if min_y >= max_y_exclusive {
            return Err(invalid_topology("owned domain vertical range is empty"));
        }
        Ok(Self {
            domain,
            algorithm,
            min_cell,
            max_cell_exclusive,
            min_y,
            max_y_exclusive,
        })
    }

    /// Returns the topology ownership identity.
    #[must_use]
    pub const fn domain(&self) -> &StableId {
        &self.domain
    }

    /// Returns the unique local topology algorithm.
    #[must_use]
    pub const fn algorithm(&self) -> CaveTopologyAlgorithmV1 {
        self.algorithm
    }

    #[must_use]
    fn contains_cell(&self, cell_x: i64, cell_z: i64, y: i32) -> bool {
        cell_x >= self.min_cell[0]
            && cell_x < self.max_cell_exclusive[0]
            && cell_z >= self.min_cell[1]
            && cell_z < self.max_cell_exclusive[1]
            && y >= self.min_y
            && y < self.max_y_exclusive
    }
}

/// Finite corridor that realizes one topology-graph edge.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CaveLayerCorridorV1 {
    start_voxels: [i64; 3],
    end_voxels: [i64; 3],
}

impl CaveLayerCorridorV1 {
    /// Creates a finite corridor between two voxel anchors.
    #[must_use]
    pub fn new(start_voxels: [i64; 3], end_voxels: [i64; 3]) -> Self {
        let (start_voxels, end_voxels) = if start_voxels <= end_voxels {
            (start_voxels, end_voxels)
        } else {
            (end_voxels, start_voxels)
        };
        Self {
            start_voxels,
            end_voxels,
        }
    }

    /// Returns the canonical start voxel.
    #[must_use]
    pub const fn start_voxels(&self) -> [i64; 3] {
        self.start_voxels
    }

    /// Returns the canonical end voxel.
    #[must_use]
    pub const fn end_voxels(&self) -> [i64; 3] {
        self.end_voxels
    }
}

/// Required cross-domain portal realized as a local clearance field.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
#[allow(
    clippy::struct_field_names,
    reason = "voxel-unit suffixes match the corridor and occupancy contracts"
)]
pub struct CaveLayerPortalV1 {
    anchor_voxels: [i64; 3],
    clearance_width_voxels: u16,
    clearance_height_voxels: u16,
}

impl CaveLayerPortalV1 {
    /// Creates a finite portal clearance.
    ///
    /// # Errors
    ///
    /// Returns an error unless both clearance axes are non-zero.
    pub fn new(
        anchor_voxels: [i64; 3],
        clearance_width_voxels: u16,
        clearance_height_voxels: u16,
    ) -> WorldgenResult<Self> {
        if clearance_width_voxels == 0 || clearance_height_voxels == 0 {
            return Err(invalid_topology("portal clearance must be non-zero"));
        }
        Ok(Self {
            anchor_voxels,
            clearance_width_voxels,
            clearance_height_voxels,
        })
    }

    /// Returns the portal aperture in world voxels.
    #[must_use]
    pub const fn anchor_voxels(&self) -> [i64; 3] {
        self.anchor_voxels
    }

    /// Returns the horizontal clearance in voxels.
    #[must_use]
    pub const fn clearance_width_voxels(&self) -> u16 {
        self.clearance_width_voxels
    }

    /// Returns the vertical clearance in voxels.
    #[must_use]
    pub const fn clearance_height_voxels(&self) -> u16 {
        self.clearance_height_voxels
    }
}

/// Ordered planning-cell path from a surface opening to a must-connect destination.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CaveLayerEntranceV1 {
    cells: Vec<[i64; 2]>,
    y_voxel: i64,
    destination_cell: [i64; 2],
}

impl CaveLayerEntranceV1 {
    /// Creates a surface-to-destination entrance path.
    ///
    /// # Errors
    ///
    /// Returns an error unless the path has four cells, consecutive cells are
    /// cardinal neighbors, and the last cell is the destination.
    pub fn new(
        cells: Vec<[i64; 2]>,
        y_voxel: i64,
        destination_cell: [i64; 2],
    ) -> WorldgenResult<Self> {
        if cells.len() < 4 {
            return Err(invalid_topology(
                "surface entrance must cross four planning cells",
            ));
        }
        if cells.last().copied() != Some(destination_cell) {
            return Err(invalid_topology(
                "surface entrance destination must be the final path cell",
            ));
        }
        for window in cells.windows(2) {
            let dx = window[0][0].saturating_sub(window[1][0]).saturating_abs();
            let dz = window[0][1].saturating_sub(window[1][1]).saturating_abs();
            if dx.saturating_add(dz) != 1 {
                return Err(invalid_topology(
                    "surface entrance cells must be cardinal neighbors",
                ));
            }
        }
        Ok(Self {
            cells,
            y_voxel,
            destination_cell,
        })
    }

    /// Returns the ordered planning-cell path.
    #[must_use]
    pub fn cells(&self) -> &[[i64; 2]] {
        &self.cells
    }

    /// Returns the destination planning cell.
    #[must_use]
    pub const fn destination_cell(&self) -> [i64; 2] {
        self.destination_cell
    }

    /// Returns the planned corridor height in world voxels.
    #[must_use]
    pub const fn y_voxel(&self) -> i64 {
        self.y_voxel
    }
}

/// One bounded branch contributor that cannot own topology.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CaveBranchContributorV1 {
    domain: StableId,
    min_cell: [i64; 2],
    max_cell_exclusive: [i64; 2],
    min_y: i32,
    max_y_exclusive: i32,
}

impl CaveBranchContributorV1 {
    /// Creates a finite branch-contributor envelope.
    ///
    /// # Errors
    ///
    /// Returns an error unless the identity is a cave-topology-domain and the
    /// envelope is non-empty.
    pub fn new(
        domain: StableId,
        min_cell: [i64; 2],
        max_cell_exclusive: [i64; 2],
        min_y: i32,
        max_y_exclusive: i32,
    ) -> WorldgenResult<Self> {
        if domain.kind() != "cave-topology-domain" {
            return Err(invalid_topology(
                "branch contributor must target a cave-topology-domain",
            ));
        }
        if min_cell[0] >= max_cell_exclusive[0] || min_cell[1] >= max_cell_exclusive[1] {
            return Err(invalid_topology("branch contributor bounds are empty"));
        }
        if min_y >= max_y_exclusive {
            return Err(invalid_topology(
                "branch contributor vertical range is empty",
            ));
        }
        Ok(Self {
            domain,
            min_cell,
            max_cell_exclusive,
            min_y,
            max_y_exclusive,
        })
    }

    /// Returns the topology domain this contributor is attached to.
    #[must_use]
    pub const fn domain(&self) -> &StableId {
        &self.domain
    }

    #[must_use]
    fn contains(&self, cell_x: i64, cell_z: i64, y: i32) -> bool {
        cell_x >= self.min_cell[0]
            && cell_x < self.max_cell_exclusive[0]
            && cell_z >= self.min_cell[1]
            && cell_z < self.max_cell_exclusive[1]
            && y >= self.min_y
            && y < self.max_y_exclusive
    }
}

/// Voxel-scale passability evidence for one planned surface entrance.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CaveVoxelPassabilityReceiptV1 {
    samples: u32,
    finally_void_samples: u32,
    destination_void: bool,
}

impl CaveVoxelPassabilityReceiptV1 {
    /// Returns inspected corridor voxels.
    #[must_use]
    pub const fn samples(&self) -> u32 {
        self.samples
    }

    /// Returns inspected voxels that are finally void after occupancy.
    #[must_use]
    pub const fn finally_void_samples(&self) -> u32 {
        self.finally_void_samples
    }

    /// Returns whether the must-connect destination voxel is finally void.
    #[must_use]
    pub const fn destination_void(&self) -> bool {
        self.destination_void
    }

    /// Returns whether every sampled corridor voxel is finally passable.
    #[must_use]
    pub const fn is_passable(&self) -> bool {
        self.samples > 0 && self.finally_void_samples == self.samples && self.destination_void
    }
}

/// Immutable inputs that enable V6 topology realization on a D4 plan.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CaveTopologyLayerInputV1 {
    default_domain: StableId,
    default_algorithm: CaveTopologyAlgorithmV1,
    domains: Vec<CaveOwnedDomainV1>,
    corridors: Vec<CaveLayerCorridorV1>,
    portals: Vec<CaveLayerPortalV1>,
    entrances: Vec<CaveLayerEntranceV1>,
    branch: CaveBranchContributorV1,
}

impl CaveTopologyLayerInputV1 {
    /// Validates a complete V6 topology realization layer.
    ///
    /// # Errors
    ///
    /// Returns an error unless two underground domains use distinct algorithms,
    /// a four-cell two-domain entrance exists, a must-connect destination is
    /// present, and exactly one branch contributor is supplied.
    pub fn new(
        default_domain: StableId,
        default_algorithm: CaveTopologyAlgorithmV1,
        mut domains: Vec<CaveOwnedDomainV1>,
        mut corridors: Vec<CaveLayerCorridorV1>,
        mut portals: Vec<CaveLayerPortalV1>,
        mut entrances: Vec<CaveLayerEntranceV1>,
        branch: CaveBranchContributorV1,
    ) -> WorldgenResult<Self> {
        if default_domain.kind() != "cave-topology-domain" {
            return Err(invalid_topology(
                "default domain must be a cave-topology-domain",
            ));
        }
        if default_algorithm != CaveTopologyAlgorithmV1::CoarseCell {
            return Err(invalid_topology(
                "dimension-default topology uses the coarse-cell algorithm",
            ));
        }
        domains.sort_by(|left, right| left.domain.cmp(&right.domain));
        if domains.len() < 2 {
            return Err(invalid_topology(
                "V6 cave layer requires two underground-owned subdomains",
            ));
        }
        if domains
            .windows(2)
            .any(|pair| pair[0].domain == pair[1].domain)
            || domains.iter().any(|domain| domain.domain == default_domain)
        {
            return Err(invalid_topology(
                "topology ownership domain identities must be unique",
            ));
        }
        let child_algorithms = domains
            .iter()
            .map(CaveOwnedDomainV1::algorithm)
            .collect::<Vec<_>>();
        if child_algorithms[0] == child_algorithms[1] {
            return Err(invalid_topology(
                "underground-owned subdomains must use distinct algorithms",
            ));
        }
        if !child_algorithms.contains(&CaveTopologyAlgorithmV1::ConstrainedGraph)
            || !child_algorithms.contains(&CaveTopologyAlgorithmV1::FieldGrowth)
        {
            return Err(invalid_topology(
                "underground subdomains must realize constrained-graph and field-growth algorithms",
            ));
        }
        if !domains
            .iter()
            .any(|domain| domain.domain == *branch.domain())
        {
            return Err(invalid_topology(
                "branch contributor must attach to an owned underground domain",
            ));
        }
        corridors.sort_by_key(|corridor| corridor.start_voxels);
        portals.sort_by_key(|portal| portal.anchor_voxels);
        entrances.sort_by(|left, right| left.cells.cmp(&right.cells));
        if portals.is_empty() {
            return Err(invalid_topology(
                "V6 cave layer requires a cross-domain portal",
            ));
        }
        if !entrances.iter().any(|entrance| entrance.cells.len() >= 4) {
            return Err(invalid_topology(
                "V6 cave layer requires a surface entrance across four cells",
            ));
        }
        Ok(Self {
            default_domain,
            default_algorithm,
            domains,
            corridors,
            portals,
            entrances,
            branch,
        })
    }

    /// Returns the dimension-default topology domain.
    #[must_use]
    pub const fn default_domain(&self) -> &StableId {
        &self.default_domain
    }

    /// Returns canonically ordered underground-owned domains.
    #[must_use]
    pub fn domains(&self) -> &[CaveOwnedDomainV1] {
        &self.domains
    }

    /// Returns the bounded branch contributor.
    #[must_use]
    pub const fn branch(&self) -> &CaveBranchContributorV1 {
        &self.branch
    }

    /// Returns planned surface entrances.
    #[must_use]
    pub fn entrances(&self) -> &[CaveLayerEntranceV1] {
        &self.entrances
    }

    /// Returns planned corridors in canonical order.
    #[must_use]
    pub fn corridors(&self) -> &[CaveLayerCorridorV1] {
        &self.corridors
    }

    /// Returns planned cross-domain portals in canonical order.
    #[must_use]
    pub fn portals(&self) -> &[CaveLayerPortalV1] {
        &self.portals
    }

    /// Returns canonical compact JSON with defaults materialized.
    ///
    /// # Errors
    ///
    /// Returns a canonical encoding error if serialization fails.
    pub fn canonical_bytes(&self) -> WorldgenResult<Vec<u8>> {
        canonical_json_bytes(self).map_err(|error| WorldgenError::CanonicalEncoding {
            kind: "CaveTopologyLayerInputV1",
            reason: error.to_string(),
        })
    }

    /// Returns the canonical layer hash folded into generation identity.
    ///
    /// # Errors
    ///
    /// Returns a canonical encoding error if serialization fails.
    pub fn canonical_hash(&self) -> WorldgenResult<CaveTopologyLayerHashV1> {
        Ok(CaveTopologyLayerHashV1::from_hash(domain_hash(
            TOPOLOGY_LAYER_DOMAIN,
            &[&self.canonical_bytes()?],
        )))
    }
}

#[derive(Clone, Debug)]
pub(crate) struct TopologyFieldV1 {
    config: WorldgenConfigV1,
    default_domain: StableId,
    default_algorithm: CaveTopologyAlgorithmV1,
    domains: Vec<CaveOwnedDomainV1>,
    corridors: Vec<CaveLayerCorridorV1>,
    portals: Vec<CaveLayerPortalV1>,
    entrances: Vec<CaveLayerEntranceV1>,
    branch: CaveBranchContributorV1,
}

impl TopologyFieldV1 {
    pub(crate) fn compile(config: WorldgenConfigV1, layer: CaveTopologyLayerInputV1) -> Self {
        Self {
            config,
            default_domain: layer.default_domain,
            default_algorithm: layer.default_algorithm,
            domains: layer.domains,
            corridors: layer.corridors,
            portals: layer.portals,
            entrances: layer.entrances,
            branch: layer.branch,
        }
    }

    pub(crate) fn algorithm_at(&self, x: i64, y: i64, z: i64) -> CaveTopologyAlgorithmV1 {
        self.domain_at(x, y, z).1
    }

    pub(crate) const fn default_domain(&self) -> &StableId {
        &self.default_domain
    }

    pub(crate) fn domain_id_at(&self, x: i64, y: i64, z: i64) -> &StableId {
        self.domain_at(x, y, z).0
    }

    pub(crate) fn owned_domains(&self) -> &[CaveOwnedDomainV1] {
        &self.domains
    }

    pub(crate) fn portals(&self) -> &[CaveLayerPortalV1] {
        &self.portals
    }

    pub(crate) fn entrances(&self) -> &[CaveLayerEntranceV1] {
        &self.entrances
    }

    pub(crate) const fn branch(&self) -> &CaveBranchContributorV1 {
        &self.branch
    }

    pub(crate) fn in_declared_influence(&self, x: i64, y: i64, z: i64) -> bool {
        let y_i32 = clamp_i64_to_i32(y);
        let (cell_x, cell_z) = self.planning_cell(x, z);
        if self.branch.contains(cell_x, cell_z, y_i32) {
            return true;
        }
        if self.portals.iter().any(|portal| {
            chebyshev(x, y, z, portal.anchor_voxels)
                <= i64::from(
                    portal
                        .clearance_width_voxels
                        .max(portal.clearance_height_voxels),
                )
        }) {
            return true;
        }
        self.corridors.iter().any(|corridor| {
            linf_to_segment([x, y, z], corridor.start_voxels, corridor.end_voxels)
                <= INFLUENCE_RADIUS_VOXELS
        })
    }

    pub(crate) fn local_signed_distance(&self, x: i64, y: i64, z: i64) -> i32 {
        if !self.in_declared_influence(x, y, z) {
            return inactive_distance(&self.config);
        }
        let algorithm = self.algorithm_at(x, y, z);
        let mut distance = inactive_distance(&self.config);
        for corridor in &self.corridors {
            distance = distance.min(algorithm_distance(
                algorithm,
                [x, y, z],
                corridor.start_voxels,
                corridor.end_voxels,
            ));
        }
        distance
    }

    pub(crate) fn branch_contains(&self, x: i64, y: i64, z: i64) -> bool {
        let (cell_x, cell_z) = self.planning_cell(x, z);
        self.branch.contains(cell_x, cell_z, clamp_i64_to_i32(y))
    }

    pub(crate) fn portal_signed_distance(&self, x: i64, y: i64, z: i64) -> i32 {
        let mut distance = i32::MAX;
        for portal in &self.portals {
            let radius = i64::from(
                portal
                    .clearance_width_voxels
                    .max(portal.clearance_height_voxels),
            );
            let sample = chebyshev(x, y, z, portal.anchor_voxels).saturating_sub(radius);
            distance = distance.min(i32::try_from(sample).unwrap_or(i32::MAX));
        }
        distance
    }

    pub(crate) fn face_portal(
        &self,
        chunk: ChunkCoordinate,
        face: ChunkFaceV1,
    ) -> Option<(u16, u16, u16)> {
        let edge = i64::from(self.config.chunk_edge_voxels);
        let origin_x = i64::from(chunk.x).saturating_mul(edge);
        let origin_y = i64::from(chunk.y).saturating_mul(edge);
        let origin_z = i64::from(chunk.z).saturating_mul(edge);
        let mut hit: Option<(i64, i64, i64)> = None;
        for corridor in &self.corridors {
            if let Some(local) = face_hit(
                face,
                origin_x,
                origin_y,
                origin_z,
                edge,
                corridor.start_voxels,
                corridor.end_voxels,
            ) {
                hit = Some(match hit {
                    Some(existing) if existing <= local => existing,
                    _ => local,
                });
            }
        }
        for portal in &self.portals {
            if let Some(local) = face_anchor_hit(
                face,
                origin_x,
                origin_y,
                origin_z,
                edge,
                portal.anchor_voxels,
            ) {
                hit = Some(match hit {
                    Some(existing) if existing <= local => existing,
                    _ => local,
                });
            }
        }
        let (u, v, radius) = hit?;
        let radius = u16::try_from(radius.max(1)).unwrap_or(1);
        Some((
            u16::try_from(u.clamp(0, edge.saturating_sub(1))).unwrap_or(0),
            u16::try_from(v.clamp(0, edge.saturating_sub(1))).unwrap_or(0),
            radius,
        ))
    }

    pub(crate) fn passability_receipts(
        &self,
        mut occupancy: impl FnMut(i64, i64, i64) -> bool,
    ) -> Vec<CaveVoxelPassabilityReceiptV1> {
        let mut receipts = Vec::new();
        for entrance in &self.entrances {
            let mut samples = 0_u32;
            let mut finally_void_samples = 0_u32;
            let mut destination_void = false;
            for cell in &entrance.cells {
                let (x, z) = self.cell_center(cell[0], cell[1]);
                let y = entrance.y_voxel;
                samples = samples.saturating_add(1);
                if occupancy(x, y, z) {
                    finally_void_samples = finally_void_samples.saturating_add(1);
                }
                if *cell == entrance.destination_cell {
                    destination_void = occupancy(x, y, z);
                }
            }
            receipts.push(CaveVoxelPassabilityReceiptV1 {
                samples,
                finally_void_samples,
                destination_void,
            });
        }
        receipts
    }

    fn domain_at(&self, x: i64, y: i64, z: i64) -> (&StableId, CaveTopologyAlgorithmV1) {
        let (cell_x, cell_z) = self.planning_cell(x, z);
        let y_i32 = clamp_i64_to_i32(y);
        for domain in &self.domains {
            if domain.contains_cell(cell_x, cell_z, y_i32) {
                return (&domain.domain, domain.algorithm);
            }
        }
        (&self.default_domain, self.default_algorithm)
    }

    fn planning_cell(&self, x: i64, z: i64) -> (i64, i64) {
        let edge = planning_edge(&self.config);
        (x.div_euclid(edge), z.div_euclid(edge))
    }

    fn cell_center(&self, cell_x: i64, cell_z: i64) -> (i64, i64) {
        let edge = planning_edge(&self.config);
        let half = edge.saturating_div(2);
        (
            cell_x.saturating_mul(edge).saturating_add(half),
            cell_z.saturating_mul(edge).saturating_add(half),
        )
    }
}

fn planning_edge(config: &WorldgenConfigV1) -> i64 {
    i64::from(config.chunk_edge_voxels).saturating_mul(i64::from(config.planning_cell_edge_chunks))
}

fn inactive_distance(config: &WorldgenConfigV1) -> i32 {
    i32::from(config.cave_cell_edge_voxels).saturating_mul(2)
}

fn algorithm_distance(
    algorithm: CaveTopologyAlgorithmV1,
    point: [i64; 3],
    start: [i64; 3],
    end: [i64; 3],
) -> i32 {
    match algorithm {
        CaveTopologyAlgorithmV1::CoarseCell | CaveTopologyAlgorithmV1::ConstrainedGraph => {
            let radius = algorithm.radius_voxels();
            i32::try_from(linf_to_segment(point, start, end).saturating_sub(radius))
                .unwrap_or(i32::MAX)
        }
        CaveTopologyAlgorithmV1::FieldGrowth => i32::try_from(
            anisotropic_to_segment(point, start, end).saturating_sub(GROWTH_RADIUS_VOXELS),
        )
        .unwrap_or(i32::MAX),
    }
}

fn linf_to_segment(point: [i64; 3], start: [i64; 3], end: [i64; 3]) -> i64 {
    let closest = closest_on_segment(point, start, end);
    chebyshev_points(point, closest)
}

fn anisotropic_to_segment(point: [i64; 3], start: [i64; 3], end: [i64; 3]) -> i64 {
    let delta = [
        end[0].saturating_sub(start[0]).saturating_abs(),
        end[1].saturating_sub(start[1]).saturating_abs(),
        end[2].saturating_sub(start[2]).saturating_abs(),
    ];
    let closest = closest_on_segment(point, start, end);
    let off = [
        point[0].saturating_sub(closest[0]).saturating_abs(),
        point[1].saturating_sub(closest[1]).saturating_abs(),
        point[2].saturating_sub(closest[2]).saturating_abs(),
    ];
    if delta[0] >= delta[1] && delta[0] >= delta[2] {
        off[1].max(off[2])
    } else if delta[1] >= delta[2] {
        off[0].max(off[2])
    } else {
        off[0].max(off[1])
    }
}

fn closest_on_segment(point: [i64; 3], start: [i64; 3], end: [i64; 3]) -> [i64; 3] {
    let ab = [
        end[0].saturating_sub(start[0]),
        end[1].saturating_sub(start[1]),
        end[2].saturating_sub(start[2]),
    ];
    let ap = [
        point[0].saturating_sub(start[0]),
        point[1].saturating_sub(start[1]),
        point[2].saturating_sub(start[2]),
    ];
    let denom = ab[0]
        .saturating_mul(ab[0])
        .saturating_add(ab[1].saturating_mul(ab[1]))
        .saturating_add(ab[2].saturating_mul(ab[2]));
    if denom == 0 {
        return start;
    }
    let numer = ap[0]
        .saturating_mul(ab[0])
        .saturating_add(ap[1].saturating_mul(ab[1]))
        .saturating_add(ap[2].saturating_mul(ab[2]));
    let t = numer.clamp(0, denom);
    [
        start[0].saturating_add(ab[0].saturating_mul(t).saturating_div(denom)),
        start[1].saturating_add(ab[1].saturating_mul(t).saturating_div(denom)),
        start[2].saturating_add(ab[2].saturating_mul(t).saturating_div(denom)),
    ]
}

fn chebyshev(x: i64, y: i64, z: i64, anchor: [i64; 3]) -> i64 {
    chebyshev_points([x, y, z], anchor)
}

fn chebyshev_points(left: [i64; 3], right: [i64; 3]) -> i64 {
    left[0]
        .saturating_sub(right[0])
        .saturating_abs()
        .max(left[1].saturating_sub(right[1]).saturating_abs())
        .max(left[2].saturating_sub(right[2]).saturating_abs())
}

fn face_hit(
    face: ChunkFaceV1,
    origin_x: i64,
    origin_y: i64,
    origin_z: i64,
    edge: i64,
    start: [i64; 3],
    end: [i64; 3],
) -> Option<(i64, i64, i64)> {
    let (axis, plane, u_origin, v_origin) = match face {
        ChunkFaceV1::NegativeX => (0_usize, origin_x, origin_y, origin_z),
        ChunkFaceV1::PositiveX => (
            0,
            origin_x.saturating_add(edge).saturating_sub(1),
            origin_y,
            origin_z,
        ),
        ChunkFaceV1::NegativeY => (1, origin_y, origin_x, origin_z),
        ChunkFaceV1::PositiveY => (
            1,
            origin_y.saturating_add(edge).saturating_sub(1),
            origin_x,
            origin_z,
        ),
        ChunkFaceV1::NegativeZ => (2, origin_z, origin_x, origin_y),
        ChunkFaceV1::PositiveZ => (
            2,
            origin_z.saturating_add(edge).saturating_sub(1),
            origin_x,
            origin_y,
        ),
    };
    let start_axis = start[axis];
    let end_axis = end[axis];
    if (start_axis.saturating_sub(plane)).saturating_mul(end_axis.saturating_sub(plane)) > 0
        && start_axis != plane
        && end_axis != plane
    {
        return None;
    }
    let hit = closest_on_segment(
        match axis {
            0 => [
                plane,
                (start[1].saturating_add(end[1])).saturating_div(2),
                (start[2].saturating_add(end[2])).saturating_div(2),
            ],
            1 => [
                (start[0].saturating_add(end[0])).saturating_div(2),
                plane,
                (start[2].saturating_add(end[2])).saturating_div(2),
            ],
            _ => [
                (start[0].saturating_add(end[0])).saturating_div(2),
                (start[1].saturating_add(end[1])).saturating_div(2),
                plane,
            ],
        },
        start,
        end,
    );
    if hit[axis] != plane && hit[axis].saturating_sub(plane).saturating_abs() > 1 {
        return None;
    }
    if linf_to_segment(hit, start, end) > GROWTH_RADIUS_VOXELS {
        return None;
    }
    let (u_world, v_world) = match axis {
        0 => (hit[1], hit[2]),
        1 => (hit[0], hit[2]),
        _ => (hit[0], hit[1]),
    };
    let u = u_world.saturating_sub(u_origin);
    let v = v_world.saturating_sub(v_origin);
    if !(0..edge).contains(&u) || !(0..edge).contains(&v) {
        return None;
    }
    Some((u, v, GROWTH_RADIUS_VOXELS))
}

fn face_anchor_hit(
    face: ChunkFaceV1,
    origin_x: i64,
    origin_y: i64,
    origin_z: i64,
    edge: i64,
    anchor: [i64; 3],
) -> Option<(i64, i64, i64)> {
    let (axis, plane, u_origin, v_origin) = match face {
        ChunkFaceV1::NegativeX => (0_usize, origin_x, origin_y, origin_z),
        ChunkFaceV1::PositiveX => (
            0,
            origin_x.saturating_add(edge).saturating_sub(1),
            origin_y,
            origin_z,
        ),
        ChunkFaceV1::NegativeY => (1, origin_y, origin_x, origin_z),
        ChunkFaceV1::PositiveY => (
            1,
            origin_y.saturating_add(edge).saturating_sub(1),
            origin_x,
            origin_z,
        ),
        ChunkFaceV1::NegativeZ => (2, origin_z, origin_x, origin_y),
        ChunkFaceV1::PositiveZ => (
            2,
            origin_z.saturating_add(edge).saturating_sub(1),
            origin_x,
            origin_y,
        ),
    };
    if anchor[axis].saturating_sub(plane).saturating_abs() > 1 {
        return None;
    }
    let (u_world, v_world) = match axis {
        0 => (anchor[1], anchor[2]),
        1 => (anchor[0], anchor[2]),
        _ => (anchor[0], anchor[1]),
    };
    let u = u_world.saturating_sub(u_origin);
    let v = v_world.saturating_sub(v_origin);
    if !(0..edge).contains(&u) || !(0..edge).contains(&v) {
        return None;
    }
    Some((u, v, GROWTH_RADIUS_VOXELS))
}

fn clamp_i64_to_i32(value: i64) -> i32 {
    i32::try_from(value).unwrap_or(if value < 0 { i32::MIN } else { i32::MAX })
}

fn invalid_topology(reason: &'static str) -> WorldgenError {
    WorldgenError::InvalidCaveTopology {
        reason: reason.to_owned(),
    }
}

/// Converts world millimeters to voxels without rounding toward the origin.
#[must_use]
pub fn millimeters_to_voxels(millimeters: [i64; 3]) -> [i64; 3] {
    [
        millimeters[0].div_euclid(1_000),
        millimeters[1].div_euclid(1_000),
        millimeters[2].div_euclid(1_000),
    ]
}

/// Converts a planning cell and a y-millimeter height into a voxel center.
#[must_use]
pub fn cell_center_voxels(
    cell_x: i64,
    cell_z: i64,
    y_millimeters: i64,
    config: &WorldgenConfigV1,
) -> [i64; 3] {
    let edge = planning_edge(config);
    let half = edge.saturating_div(2);
    [
        cell_x.saturating_mul(edge).saturating_add(half),
        y_millimeters.div_euclid(1_000),
        cell_z.saturating_mul(edge).saturating_add(half),
    ]
}
