//! Topology-first rivers, retained water bodies, and static reservoir occupancy.

use std::{
    cmp::Reverse,
    collections::{BTreeMap, BTreeSet, BinaryHeap},
    num::NonZeroU16,
};

use latticeaxiom_core::{CanonicalHash, StableId, canonical_json_bytes};
use latticeaxiom_storage::ChunkCoordinate;
use serde::{Deserialize, Serialize};

use crate::{
    DepressionClassV1, GenerationEpochIdV1, HydrologicBasinIdV1, HydrologicDomainPlanV1,
    HydrologicGridCoordinateV1, HydrologicOutletIdV1, HydrologicPortIdV1, HydrologicTopologyHashV1,
    RiverSegmentIdV1, StaticReservoirHashV1, WaterBodyIdV1, WorldgenError, WorldgenResult,
    hashes::domain_hash,
};

const TOPOLOGY_ALGORITHM: &str = "latticeaxiom:hydrologic-topology/channel-dag@1";
const STATIC_RESERVOIR_SCHEMA: &str = "latticeaxiom:static-reservoir-chunk@1";
const MAX_STATIC_RESERVOIR_CHUNK_EDGE: u16 = 128;

/// Closed topology extraction, profile, SDF, and materialization policy.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HydrologicTopologyConfigV1 {
    channel_threshold_q16: u64,
    medium_channel_threshold_q16: u64,
    large_channel_threshold_q16: u64,
    min_width_q8: u16,
    max_width_q8: u16,
    min_depth_q8: u16,
    max_depth_q8: u16,
    bank_slope_per_1024: u16,
    floodplain_multiplier_per_1024: u16,
    waterfall_drop_q8: u16,
    max_segments: u32,
    max_water_bodies: u32,
    max_basins: u32,
    max_profile_points: u32,
    max_spatial_references: u32,
    max_static_cells_per_chunk: u32,
    max_result_bytes: u64,
}

impl Default for HydrologicTopologyConfigV1 {
    fn default() -> Self {
        Self {
            channel_threshold_q16: 16 * 65_536,
            medium_channel_threshold_q16: 64 * 65_536,
            large_channel_threshold_q16: 256 * 65_536,
            min_width_q8: 192,
            max_width_q8: 12 * 256,
            min_depth_q8: 96,
            max_depth_q8: 4 * 256,
            bank_slope_per_1024: 384,
            floodplain_multiplier_per_1024: 2_048,
            waterfall_drop_q8: 2 * 256,
            max_segments: 262_144,
            max_water_bodies: 262_144,
            max_basins: 262_144,
            max_profile_points: 524_288,
            max_spatial_references: 8_388_608,
            max_static_cells_per_chunk: 262_144,
            max_result_bytes: 512 * 1024 * 1024,
        }
    }
}

impl HydrologicTopologyConfigV1 {
    /// Creates a closed topology policy.
    #[allow(
        clippy::too_many_arguments,
        reason = "the constructor closes every persisted topology policy field"
    )]
    #[must_use]
    pub const fn new(
        channel_threshold_q16: u64,
        medium_channel_threshold_q16: u64,
        large_channel_threshold_q16: u64,
        min_width_q8: u16,
        max_width_q8: u16,
        min_depth_q8: u16,
        max_depth_q8: u16,
        bank_slope_per_1024: u16,
        floodplain_multiplier_per_1024: u16,
        waterfall_drop_q8: u16,
        max_segments: u32,
        max_water_bodies: u32,
        max_basins: u32,
        max_profile_points: u32,
        max_spatial_references: u32,
        max_static_cells_per_chunk: u32,
        max_result_bytes: u64,
    ) -> Self {
        Self {
            channel_threshold_q16,
            medium_channel_threshold_q16,
            large_channel_threshold_q16,
            min_width_q8,
            max_width_q8,
            min_depth_q8,
            max_depth_q8,
            bank_slope_per_1024,
            floodplain_multiplier_per_1024,
            waterfall_drop_q8,
            max_segments,
            max_water_bodies,
            max_basins,
            max_profile_points,
            max_spatial_references,
            max_static_cells_per_chunk,
            max_result_bytes,
        }
    }

    fn validate(&self) -> WorldgenResult<()> {
        if self.channel_threshold_q16 == 0
            || self.channel_threshold_q16 > self.medium_channel_threshold_q16
            || self.medium_channel_threshold_q16 > self.large_channel_threshold_q16
        {
            return invalid(
                "topology.channel_thresholds",
                "thresholds must be nonzero and ordered small <= medium <= large",
            );
        }
        if self.min_width_q8 == 0
            || self.min_width_q8 > self.max_width_q8
            || self.min_depth_q8 == 0
            || self.min_depth_q8 > self.max_depth_q8
        {
            return invalid(
                "topology.channel_geometry",
                "width and depth envelopes must be nonzero and ordered",
            );
        }
        if self.max_segments == 0
            || self.max_water_bodies == 0
            || self.max_basins == 0
            || self.max_profile_points == 0
            || self.max_spatial_references == 0
            || self.max_static_cells_per_chunk == 0
            || self.max_result_bytes == 0
        {
            return invalid(
                "topology.limits",
                "every hard topology limit must be nonzero",
            );
        }
        Ok(())
    }

    fn canonical_hash(&self) -> WorldgenResult<CanonicalHash> {
        let bytes =
            canonical_json_bytes(self).map_err(|error| WorldgenError::CanonicalEncoding {
                kind: "HydrologicTopologyConfigV1",
                reason: error.to_string(),
            })?;
        Ok(domain_hash(
            b"latticeaxiom.hydrologic-topology.config.v1\0",
            &[&bytes],
        ))
    }
}

/// Quantized point shared by river centerlines and water profiles.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct QuantizedRiverPointV1 {
    world_x: i64,
    world_z: i64,
    surface_y_q8: i32,
    bed_y_q8: i32,
    waterfall_after: bool,
}

impl QuantizedRiverPointV1 {
    /// Returns world X in voxels.
    #[must_use]
    pub const fn world_x(self) -> i64 {
        self.world_x
    }

    /// Returns world Z in voxels.
    #[must_use]
    pub const fn world_z(self) -> i64 {
        self.world_z
    }

    /// Returns the authoritative Q24.8 water surface.
    #[must_use]
    pub const fn surface_y_q8(self) -> i32 {
        self.surface_y_q8
    }

    /// Returns the protected Q24.8 river bed.
    #[must_use]
    pub const fn bed_y_q8(self) -> i32 {
        self.bed_y_q8
    }

    /// Returns whether the downstream edge is an explicit waterfall.
    #[must_use]
    pub const fn waterfall_after(self) -> bool {
        self.waterfall_after
    }
}

/// Explicit terminal reached by a river segment.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum RiverEndpointV1 {
    /// Another canonical river segment.
    Segment(RiverSegmentIdV1),
    /// A retained lake, wetland, ocean, or river body.
    WaterBody(WaterBodyIdV1),
    /// A declared parent/adjacent-domain boundary port.
    BoundaryPort(HydrologicPortIdV1),
    /// A generated domain/ocean outlet.
    Outlet(HydrologicOutletIdV1),
}

/// One canonical single-receiver river cell represented as a stable segment.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RiverSegmentV1 {
    id: RiverSegmentIdV1,
    upstream: Vec<RiverSegmentIdV1>,
    downstream: Option<RiverSegmentIdV1>,
    endpoint: RiverEndpointV1,
    basin: HydrologicBasinIdV1,
    water_body: WaterBodyIdV1,
    centerline: Vec<QuantizedRiverPointV1>,
    length_q8: u64,
    gradient_per_million: u32,
    discharge_q16: u64,
    strahler_order: u16,
    width_q8: u16,
    bed_depth_q8: u16,
    bank_slope_per_1024: u16,
    floodplain_width_q8: u32,
    sdf_influence_radius_q8: u32,
    flow_tangent_q15: [i16; 2],
}

impl RiverSegmentV1 {
    /// Returns the stable segment ID.
    #[must_use]
    pub const fn id(&self) -> RiverSegmentIdV1 {
        self.id
    }

    /// Returns canonical upstream segment IDs.
    #[must_use]
    pub fn upstream(&self) -> &[RiverSegmentIdV1] {
        &self.upstream
    }

    /// Returns the optional downstream segment ID.
    #[must_use]
    pub const fn downstream(&self) -> Option<RiverSegmentIdV1> {
        self.downstream
    }

    /// Returns the explicit downstream endpoint.
    #[must_use]
    pub const fn endpoint(&self) -> RiverEndpointV1 {
        self.endpoint
    }

    /// Returns the basin ID.
    #[must_use]
    pub const fn basin(&self) -> HydrologicBasinIdV1 {
        self.basin
    }

    /// Returns the immutable water-body ID.
    #[must_use]
    pub const fn water_body(&self) -> WaterBodyIdV1 {
        self.water_body
    }

    /// Returns the two-point centerline/profile in upstream-to-downstream order.
    #[must_use]
    pub fn centerline(&self) -> &[QuantizedRiverPointV1] {
        &self.centerline
    }

    /// Returns horizontal centerline length in Q8 voxels.
    #[must_use]
    pub const fn length_q8(&self) -> u64 {
        self.length_q8
    }

    /// Returns nonnegative downstream water-profile gradient per million.
    #[must_use]
    pub const fn gradient_per_million(&self) -> u32 {
        self.gradient_per_million
    }

    /// Returns effective Q16 discharge from the second conservative pass.
    #[must_use]
    pub const fn discharge_q16(&self) -> u64 {
        self.discharge_q16
    }

    /// Returns canonical Strahler order.
    #[must_use]
    pub const fn strahler_order(&self) -> u16 {
        self.strahler_order
    }

    /// Returns channel width in Q8 voxels.
    #[must_use]
    pub const fn width_q8(&self) -> u16 {
        self.width_q8
    }

    /// Returns protected SDF influence radius in Q8 voxels.
    #[must_use]
    pub const fn sdf_influence_radius_q8(&self) -> u32 {
        self.sdf_influence_radius_q8
    }

    /// Returns presentation flow tangent `(x, z)` in signed Q15.
    #[must_use]
    pub const fn flow_tangent_q15(&self) -> [i16; 2] {
        self.flow_tangent_q15
    }
}

/// Stable kind of a generated water body.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum WaterBodyKindV1 {
    /// Dimension-defined equipotential water.
    Ocean,
    /// Retained depression with one constant level.
    Lake,
    /// Connected channel network with a piecewise profile.
    River,
    /// Bounded shallow saturated terrain.
    Wetland,
}

/// Closed authoritative surface model for one generated water body.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum WaterSurfaceModelV1 {
    /// Constant ocean level.
    Ocean {
        /// Equipotential Q24.8 surface.
        level_q8: i32,
    },
    /// Constant retained-lake level.
    Lake {
        /// Spill/storage-derived Q24.8 surface.
        level_q8: i32,
    },
    /// Piecewise monotone river profiles ordered by segment ID.
    River {
        /// Stable segment/profile pairs.
        profiles: Vec<(RiverSegmentIdV1, Vec<QuantizedRiverPointV1>)>,
    },
    /// Shallow saturated surface and maximum occupancy depth.
    Wetland {
        /// Saturated Q24.8 surface.
        level_q8: i32,
        /// Maximum shallow occupancy depth in Q8 voxels.
        depth_q8: u16,
    },
}

/// Immutable generated ocean, lake, river, or wetland.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WaterBodyV1 {
    id: WaterBodyIdV1,
    kind: WaterBodyKindV1,
    basin: HydrologicBasinIdV1,
    outlet: HydrologicOutletIdV1,
    surface: WaterSurfaceModelV1,
    min_world_x: i64,
    max_world_x: i64,
    min_world_z: i64,
    max_world_z: i64,
    min_y_q8: i32,
    max_y_q8: i32,
    static_reservoir: bool,
    turbidity_per_1024: u16,
    flow_speed_per_1024: u16,
}

impl WaterBodyV1 {
    /// Returns the stable water-body ID.
    #[must_use]
    pub const fn id(&self) -> WaterBodyIdV1 {
        self.id
    }

    /// Returns the body kind.
    #[must_use]
    pub const fn kind(&self) -> WaterBodyKindV1 {
        self.kind
    }

    /// Returns the authoritative surface model.
    #[must_use]
    pub const fn surface(&self) -> &WaterSurfaceModelV1 {
        &self.surface
    }

    /// Returns whether generated occupancy is excluded from runtime ticking.
    #[must_use]
    pub const fn is_static_reservoir(&self) -> bool {
        self.static_reservoir
    }
}

/// Stable kind of a terminal outlet.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum OutletKindV1 {
    /// Dimension base-level/ocean terminal.
    Ocean,
    /// Retained lake or wetland terminal.
    Retained,
    /// Parent or neighboring hydrologic-domain port.
    BoundaryPort,
}

/// One canonical terminal outlet.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct OutletRecordV1 {
    id: HydrologicOutletIdV1,
    kind: OutletKindV1,
    coordinate: HydrologicGridCoordinateV1,
    level_q8: i32,
    boundary_port: Option<HydrologicPortIdV1>,
    terminal_discharge_q16: u64,
}

impl OutletRecordV1 {
    /// Returns the stable outlet ID.
    #[must_use]
    pub const fn id(&self) -> HydrologicOutletIdV1 {
        self.id
    }

    /// Returns terminal kind.
    #[must_use]
    pub const fn kind(&self) -> OutletKindV1 {
        self.kind
    }

    /// Returns the terminal grid coordinate.
    #[must_use]
    pub const fn coordinate(&self) -> HydrologicGridCoordinateV1 {
        self.coordinate
    }
}

/// One deterministic semantic basin selected from distributed MFD routing.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BasinRecordV1 {
    id: HydrologicBasinIdV1,
    outlet: HydrologicOutletIdV1,
    cell_count: u32,
    local_runoff_q16: u64,
    terminal_discharge_q16: u64,
    river_segments: Vec<RiverSegmentIdV1>,
    water_bodies: Vec<WaterBodyIdV1>,
}

impl BasinRecordV1 {
    /// Returns the basin ID.
    #[must_use]
    pub const fn id(&self) -> HydrologicBasinIdV1 {
        self.id
    }

    /// Returns the basin's terminal outlet.
    #[must_use]
    pub const fn outlet(&self) -> HydrologicOutletIdV1 {
        self.outlet
    }

    /// Returns canonically ordered river segment IDs.
    #[must_use]
    pub fn river_segments(&self) -> &[RiverSegmentIdV1] {
        &self.river_segments
    }
}

/// Bounded topology extraction accounting.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HydrologicTopologyAccountingV1 {
    channel_cells: u32,
    segment_count: u32,
    basin_count: u32,
    water_body_count: u32,
    profile_points: u32,
    spatial_references: u32,
    source_runoff_q16: u64,
    terminal_runoff_q16: u64,
    result_bytes: u64,
}

impl HydrologicTopologyAccountingV1 {
    /// Returns extracted channel-cell/segment count.
    #[must_use]
    pub const fn segment_count(self) -> u32 {
        self.segment_count
    }

    /// Returns the bounded segment-to-raster index reference count.
    #[must_use]
    pub const fn spatial_references(self) -> u32 {
        self.spatial_references
    }

    /// Returns input runoff for the hybrid conservative pass.
    #[must_use]
    pub const fn source_runoff_q16(self) -> u64 {
        self.source_runoff_q16
    }

    /// Returns runoff conserved at topology terminals.
    #[must_use]
    pub const fn terminal_runoff_q16(self) -> u64 {
        self.terminal_runoff_q16
    }
}

/// Immutable topology-first river and water-body plan.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HydrologicTopologyPlanV1 {
    algorithm: &'static str,
    domain_plan_hash: crate::HydrologicDomainPlanHashV1,
    config_hash: CanonicalHash,
    generation_epoch: GenerationEpochIdV1,
    segments: Vec<RiverSegmentV1>,
    water_bodies: Vec<WaterBodyV1>,
    basins: Vec<BasinRecordV1>,
    outlets: Vec<OutletRecordV1>,
    channel_segment_by_cell: Vec<Option<u32>>,
    segment_candidates_by_cell: Vec<Vec<u32>>,
    water_body_by_cell: Vec<Option<WaterBodyIdV1>>,
    hybrid_discharge_q16: Vec<u64>,
    accounting: HydrologicTopologyAccountingV1,
}

impl HydrologicTopologyPlanV1 {
    /// Returns canonical segments ordered by stable ID.
    #[must_use]
    pub fn segments(&self) -> &[RiverSegmentV1] {
        &self.segments
    }

    /// Returns canonical water bodies ordered by stable ID.
    #[must_use]
    pub fn water_bodies(&self) -> &[WaterBodyV1] {
        &self.water_bodies
    }

    /// Returns canonical basin records.
    #[must_use]
    pub fn basins(&self) -> &[BasinRecordV1] {
        &self.basins
    }

    /// Returns canonical outlet records.
    #[must_use]
    pub fn outlets(&self) -> &[OutletRecordV1] {
        &self.outlets
    }

    /// Returns extraction and conservation accounting.
    #[must_use]
    pub const fn accounting(&self) -> HydrologicTopologyAccountingV1 {
        self.accounting
    }

    /// Returns canonical persisted bytes.
    ///
    /// # Errors
    ///
    /// Returns a canonical-encoding error if serialization fails.
    pub fn canonical_bytes(&self) -> WorldgenResult<Vec<u8>> {
        canonical_json_bytes(self).map_err(|error| WorldgenError::CanonicalEncoding {
            kind: "HydrologicTopologyPlanV1",
            reason: error.to_string(),
        })
    }

    /// Returns the canonical topology hash.
    ///
    /// # Errors
    ///
    /// Returns a canonical-encoding error if serialization fails.
    pub fn canonical_hash(&self) -> WorldgenResult<HydrologicTopologyHashV1> {
        let bytes = self.canonical_bytes()?;
        Ok(HydrologicTopologyHashV1::from_hash(domain_hash(
            b"latticeaxiom.hydrologic-topology.plan.v1\0",
            &[&bytes],
        )))
    }
}

#[derive(Clone, Debug)]
struct RetainedBodySpec {
    id: WaterBodyIdV1,
    kind: WaterBodyKindV1,
    basin: HydrologicBasinIdV1,
    outlet: HydrologicOutletIdV1,
    level_q8: i32,
    pit_elevation_q8: i32,
    cells: Vec<u32>,
}

fn primary_receiver(cell: &crate::MfdRoutingCellV1) -> Option<usize> {
    cell.receivers()
        .iter()
        .max_by_key(|receiver| {
            (
                receiver.weight_per_million(),
                receiver.slope_numerator(),
                Reverse(receiver.receiver_index()),
            )
        })
        .map(|receiver| receiver.receiver_index() as usize)
}

fn topological_order(graph: &[Vec<HybridReceiver>]) -> WorldgenResult<Vec<usize>> {
    let mut indegree = vec![0_u16; graph.len()];
    for receivers in graph {
        let weight_sum = receivers
            .iter()
            .map(|receiver| receiver.weight)
            .sum::<u32>();
        if !receivers.is_empty() && weight_sum != crate::HYDROLOGIC_WEIGHT_SCALE_V1 {
            return invalid(
                "topology.routing_weight",
                format!("receiver row sums to {weight_sum}, expected 1000000"),
            );
        }
        for receiver in receivers {
            let Some(degree) = indegree.get_mut(receiver.index) else {
                return invalid(
                    "topology.receiver",
                    "receiver index lies outside the raster",
                );
            };
            *degree = degree
                .checked_add(1)
                .ok_or(WorldgenError::ArithmeticOverflow {
                    operation: "topology receiver indegree",
                })?;
        }
    }
    let mut ready = BinaryHeap::new();
    for (index, &degree) in indegree.iter().enumerate() {
        if degree == 0 {
            ready.push(Reverse(index));
        }
    }
    let mut order = Vec::with_capacity(graph.len());
    while let Some(Reverse(index)) = ready.pop() {
        order.push(index);
        for receiver in &graph[index] {
            indegree[receiver.index] =
                indegree[receiver.index].checked_sub(1).ok_or_else(|| {
                    WorldgenError::InvalidHydrologicDomain {
                        field: "topology.receiver",
                        reason: "receiver indegree underflow".to_owned(),
                    }
                })?;
            if indegree[receiver.index] == 0 {
                ready.push(Reverse(receiver.index));
            }
        }
    }
    if order.len() != graph.len() {
        return invalid("topology.receiver", "routing graph contains a cycle");
    }
    Ok(order)
}

fn terminal_indices(primary: &[Option<usize>], order: &[usize]) -> WorldgenResult<Vec<usize>> {
    let mut terminal = vec![usize::MAX; primary.len()];
    for &index in order.iter().rev() {
        terminal[index] =
            match primary[index] {
                Some(receiver) => *terminal.get(receiver).ok_or_else(|| {
                    WorldgenError::InvalidHydrologicDomain {
                        field: "topology.primary_receiver",
                        reason: "primary receiver lies outside the raster".to_owned(),
                    }
                })?,
                None => index,
            };
        if terminal[index] == usize::MAX {
            return invalid(
                "topology.primary_receiver",
                "primary receiver was not downstream in topological order",
            );
        }
    }
    Ok(terminal)
}

fn basin_identities(
    domain: &HydrologicDomainPlanV1,
    terminals: &[usize],
) -> WorldgenResult<BTreeMap<usize, BasinIdentity>> {
    terminals
        .iter()
        .copied()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .map(|terminal| {
            let terminal = index_u32(terminal)?;
            let outlet = HydrologicOutletIdV1::from_hash(domain_hash(
                b"latticeaxiom.hydrologic-outlet.id.v1\0",
                &[domain.domain_id().as_bytes(), &terminal.to_be_bytes()],
            ));
            let basin = HydrologicBasinIdV1::from_hash(domain_hash(
                b"latticeaxiom.hydrologic-basin.id.v1\0",
                &[domain.domain_id().as_bytes(), outlet.as_bytes()],
            ));
            Ok((terminal as usize, BasinIdentity { basin, outlet }))
        })
        .collect()
}

type OutletBuild = (
    Vec<OutletRecordV1>,
    BTreeMap<usize, RetainedBodySpec>,
    BTreeMap<usize, HydrologicPortIdV1>,
);

fn outlet_records(
    domain: &HydrologicDomainPlanV1,
    identities: &BTreeMap<usize, BasinIdentity>,
) -> WorldgenResult<OutletBuild> {
    let mut depression_by_pit = BTreeMap::new();
    for depression in domain.depressions().records() {
        if matches!(
            depression.class(),
            DepressionClassV1::Lake | DepressionClassV1::Endorheic | DepressionClassV1::Wetland
        ) {
            let pit = domain.grid().index_of(depression.pit()).ok_or_else(|| {
                WorldgenError::InvalidHydrologicDomain {
                    field: "topology.depression",
                    reason: "depression pit lies outside the source grid".to_owned(),
                }
            })?;
            depression_by_pit.insert(pit, depression);
        }
    }
    let mut port_by_terminal = BTreeMap::new();
    for port in domain.ports() {
        if port.kind() == crate::HydrologicPortKindV1::Inflow {
            continue;
        }
        let coordinate = port_coordinate(domain, port)?;
        let index = domain.grid().index_of(coordinate).ok_or_else(|| {
            WorldgenError::InvalidHydrologicDomain {
                field: "topology.port",
                reason: "port coordinate lies outside the source grid".to_owned(),
            }
        })?;
        port_by_terminal.insert(index, port.id());
    }
    let mut outlets = Vec::with_capacity(identities.len());
    let mut retained = BTreeMap::new();
    for (&terminal, &identity) in identities {
        let coordinate = domain.grid().coordinate_of(terminal).ok_or_else(|| {
            WorldgenError::InvalidHydrologicDomain {
                field: "topology.outlet",
                reason: "terminal index lies outside the source grid".to_owned(),
            }
        })?;
        let (kind, level_q8, boundary_port) =
            if let Some(depression) = depression_by_pit.get(&terminal) {
                let body_kind = if depression.class() == DepressionClassV1::Wetland {
                    WaterBodyKindV1::Wetland
                } else {
                    WaterBodyKindV1::Lake
                };
                let body_id = WaterBodyIdV1::from_hash(domain_hash(
                    b"latticeaxiom.water-body.retained.id.v1\0",
                    &[
                        domain.domain_id().as_bytes(),
                        depression.id().as_bytes(),
                        &[water_body_kind_tag(body_kind)],
                    ],
                ));
                retained.insert(
                    terminal,
                    RetainedBodySpec {
                        id: body_id,
                        kind: body_kind,
                        basin: identity.basin,
                        outlet: identity.outlet,
                        level_q8: depression.spill_elevation_q8(),
                        pit_elevation_q8: depression.pit_elevation_q8(),
                        cells: depression.cell_indices().to_vec(),
                    },
                );
                (
                    OutletKindV1::Retained,
                    depression.spill_elevation_q8(),
                    None,
                )
            } else if let Some(port_id) = port_by_terminal.get(&terminal) {
                let port = domain
                    .ports()
                    .iter()
                    .find(|port| port.id() == *port_id)
                    .ok_or_else(|| WorldgenError::InvalidHydrologicDomain {
                        field: "topology.port",
                        reason: "terminal port identity has no declaration".to_owned(),
                    })?;
                (OutletKindV1::BoundaryPort, port.level_q8(), Some(*port_id))
            } else {
                (OutletKindV1::Ocean, domain.base_level_q8(), None)
            };
        outlets.push(OutletRecordV1 {
            id: identity.outlet,
            kind,
            coordinate,
            level_q8,
            boundary_port,
            terminal_discharge_q16: domain.routing()[terminal].accumulated_runoff_q16(),
        });
    }
    Ok((outlets, retained, port_by_terminal))
}

fn port_coordinate(
    domain: &HydrologicDomainPlanV1,
    port: &crate::HydrologicBoundaryPortV1,
) -> WorldgenResult<HydrologicGridCoordinateV1> {
    let coordinate = match port.edge() {
        crate::HydrologicBoundaryEdgeV1::North => HydrologicGridCoordinateV1::new(port.offset(), 0),
        crate::HydrologicBoundaryEdgeV1::East => {
            HydrologicGridCoordinateV1::new(domain.grid().width().get() - 1, port.offset())
        }
        crate::HydrologicBoundaryEdgeV1::South => {
            HydrologicGridCoordinateV1::new(port.offset(), domain.grid().height().get() - 1)
        }
        crate::HydrologicBoundaryEdgeV1::West => HydrologicGridCoordinateV1::new(0, port.offset()),
    };
    if domain.grid().index_of(coordinate).is_none() {
        return invalid("topology.port", "port offset lies outside the source grid");
    }
    Ok(coordinate)
}

fn conservative_accumulation(
    source: &[u64],
    graph: &[Vec<HybridReceiver>],
    order: &[usize],
) -> WorldgenResult<(Vec<u64>, u64, u64)> {
    if source.len() != graph.len() || order.len() != graph.len() {
        return invalid(
            "topology.accumulation",
            "source, graph, and order lengths differ",
        );
    }
    let source_sum = source.iter().try_fold(0_u64, |sum, &amount| {
        sum.checked_add(amount)
            .ok_or(WorldgenError::ArithmeticOverflow {
                operation: "topology source runoff sum",
            })
    })?;
    let mut accumulated = source.to_vec();
    for &index in order {
        let allocations = allocate_exact(accumulated[index], &graph[index])?;
        for (receiver, allocation) in graph[index].iter().zip(allocations) {
            accumulated[receiver.index] = accumulated[receiver.index]
                .checked_add(allocation)
                .ok_or(WorldgenError::ArithmeticOverflow {
                    operation: "topology hybrid discharge",
                })?;
        }
    }
    let terminal_sum =
        graph
            .iter()
            .zip(&accumulated)
            .try_fold(0_u64, |sum, (receivers, &amount)| {
                if receivers.is_empty() {
                    sum.checked_add(amount)
                        .ok_or(WorldgenError::ArithmeticOverflow {
                            operation: "topology terminal runoff sum",
                        })
                } else {
                    Ok(sum)
                }
            })?;
    if source_sum != terminal_sum {
        return invalid(
            "topology.runoff_conservation",
            format!("source {source_sum} differs from terminal {terminal_sum}"),
        );
    }
    Ok((accumulated, source_sum, terminal_sum))
}

fn allocate_exact(amount: u64, receivers: &[HybridReceiver]) -> WorldgenResult<Vec<u64>> {
    if receivers.is_empty() {
        return Ok(Vec::new());
    }
    let denominator = u128::from(crate::HYDROLOGIC_WEIGHT_SCALE_V1);
    let mut values = Vec::with_capacity(receivers.len());
    let mut remainders = Vec::with_capacity(receivers.len());
    let mut assigned = 0_u64;
    for (position, receiver) in receivers.iter().enumerate() {
        let numerator = u128::from(amount) * u128::from(receiver.weight);
        let value = u64::try_from(numerator / denominator).map_err(|_| {
            WorldgenError::ArithmeticOverflow {
                operation: "topology runoff allocation",
            }
        })?;
        assigned = assigned
            .checked_add(value)
            .ok_or(WorldgenError::ArithmeticOverflow {
                operation: "topology runoff allocation sum",
            })?;
        values.push(value);
        remainders.push((numerator % denominator, receiver.index, position));
    }
    let residual = usize::try_from(amount.checked_sub(assigned).ok_or(
        WorldgenError::ArithmeticOverflow {
            operation: "topology runoff allocation residual",
        },
    )?)
    .map_err(|_| WorldgenError::ArithmeticOverflow {
        operation: "topology runoff residual count",
    })?;
    remainders.sort_by_key(|&(remainder, index, _)| (Reverse(remainder), index));
    for &(_, _, position) in remainders.iter().take(residual) {
        values[position] =
            values[position]
                .checked_add(1)
                .ok_or(WorldgenError::ArithmeticOverflow {
                    operation: "topology runoff residual assignment",
                })?;
    }
    Ok(values)
}

fn trace_next_channel(
    start: usize,
    primary: &[Option<usize>],
    channel_mask: &[bool],
    hard_limit: usize,
) -> WorldgenResult<Option<usize>> {
    let mut current = primary[start];
    for _ in 0..hard_limit {
        let Some(index) = current else {
            return Ok(None);
        };
        if channel_mask[index] {
            return Ok(Some(index));
        }
        current = primary[index];
    }
    invalid(
        "topology.channel",
        "primary receiver trace exceeded the finite raster",
    )
}

fn strahler_orders(
    channel_indices: &[usize],
    downstream: &BTreeMap<usize, Option<usize>>,
    upstream: &BTreeMap<usize, Vec<usize>>,
) -> WorldgenResult<BTreeMap<usize, u16>> {
    let mut indegree = channel_indices
        .iter()
        .copied()
        .map(|index| (index, upstream.get(&index).map_or(0, Vec::len)))
        .collect::<BTreeMap<_, _>>();
    let mut ready = BinaryHeap::new();
    for (&index, &degree) in &indegree {
        if degree == 0 {
            ready.push(Reverse(index));
        }
    }
    let mut orders = BTreeMap::<usize, u16>::new();
    let mut processed = 0_usize;
    while let Some(Reverse(index)) = ready.pop() {
        let sources: &[usize] = upstream.get(&index).map_or(&[], Vec::as_slice);
        let order = if sources.is_empty() {
            1
        } else {
            let maximum = sources
                .iter()
                .filter_map(|source| orders.get(source).copied())
                .max()
                .ok_or_else(|| WorldgenError::InvalidHydrologicDomain {
                    field: "topology.strahler",
                    reason: "upstream order was not available".to_owned(),
                })?;
            let maximum_count = sources
                .iter()
                .filter(|source| orders.get(*source) == Some(&maximum))
                .count();
            if maximum_count >= 2 {
                maximum
                    .checked_add(1)
                    .ok_or(WorldgenError::ArithmeticOverflow {
                        operation: "river Strahler order",
                    })?
            } else {
                maximum
            }
        };
        orders.insert(index, order);
        processed += 1;
        if let Some(Some(next)) = downstream.get(&index) {
            let degree =
                indegree
                    .get_mut(next)
                    .ok_or_else(|| WorldgenError::InvalidHydrologicDomain {
                        field: "topology.channel",
                        reason: "downstream segment has no indegree record".to_owned(),
                    })?;
            *degree =
                degree
                    .checked_sub(1)
                    .ok_or_else(|| WorldgenError::InvalidHydrologicDomain {
                        field: "topology.channel",
                        reason: "segment indegree underflow".to_owned(),
                    })?;
            if *degree == 0 {
                ready.push(Reverse(*next));
            }
        }
    }
    if processed != channel_indices.len() {
        return invalid(
            "topology.channel",
            "canonical channel graph contains a cycle",
        );
    }
    Ok(orders)
}

fn channel_geometry(
    config: &HydrologicTopologyConfigV1,
    discharge_q16: u64,
    order: u16,
) -> (u16, u16) {
    let ratio = discharge_q16 / config.channel_threshold_q16;
    let root = integer_sqrt_u128(u128::from(ratio));
    let width = u32::from(config.min_width_q8)
        .saturating_add(u32::try_from(root).unwrap_or(u32::MAX).saturating_mul(64))
        .clamp(
            u32::from(config.min_width_q8),
            u32::from(config.max_width_q8),
        );
    let depth = u32::from(config.min_depth_q8)
        .saturating_add(u32::from(order.saturating_sub(1)).saturating_mul(32))
        .clamp(
            u32::from(config.min_depth_q8),
            u32::from(config.max_depth_q8),
        );
    (
        u16::try_from(width).unwrap_or(config.max_width_q8),
        u16::try_from(depth).unwrap_or(config.max_depth_q8),
    )
}

fn tangent_q15(source_x: i64, source_z: i64, target_x: i64, target_z: i64) -> [i16; 2] {
    let dx = i128::from(target_x) - i128::from(source_x);
    let dz = i128::from(target_z) - i128::from(source_z);
    let length = integer_sqrt_u128(dx.unsigned_abs().pow(2) + dz.unsigned_abs().pow(2));
    if length == 0 {
        return [0, 0];
    }
    let x = dx.saturating_mul(32_767) / i128::try_from(length).unwrap_or(i128::MAX);
    let z = dz.saturating_mul(32_767) / i128::try_from(length).unwrap_or(i128::MAX);
    [
        i16::try_from(x).unwrap_or(if x.is_negative() { i16::MIN } else { i16::MAX }),
        i16::try_from(z).unwrap_or(if z.is_negative() { i16::MIN } else { i16::MAX }),
    ]
}

fn segment_length_and_gradient(
    source_x: i64,
    source_z: i64,
    target_x: i64,
    target_z: i64,
    source_surface_q8: i32,
    target_surface_q8: i32,
) -> (u64, u32) {
    let dx = i128::from(target_x) - i128::from(source_x);
    let dz = i128::from(target_z) - i128::from(source_z);
    let length_voxels = integer_sqrt_u128(dx.unsigned_abs().pow(2) + dz.unsigned_abs().pow(2));
    let length_q8 = u64::try_from(length_voxels.saturating_mul(256)).unwrap_or(u64::MAX);
    if length_q8 == 0 {
        return (0, 0);
    }
    let drop_q8 = i64::from(source_surface_q8)
        .saturating_sub(i64::from(target_surface_q8))
        .max(0)
        .cast_unsigned();
    let gradient = u128::from(drop_q8).saturating_mul(1_000_000) / u128::from(length_q8);
    (length_q8, u32::try_from(gradient).unwrap_or(u32::MAX))
}

#[allow(
    clippy::too_many_lines,
    reason = "body construction keeps retained, river, and ocean IDs in one canonical publication pass"
)]
fn build_water_bodies(
    domain: &HydrologicDomainPlanV1,
    segments: &[RiverSegmentV1],
    terminals: &[usize],
    identities: &BTreeMap<usize, BasinIdentity>,
    retained: &BTreeMap<usize, RetainedBodySpec>,
    river_body_by_basin: &BTreeMap<HydrologicBasinIdV1, WaterBodyIdV1>,
) -> WorldgenResult<(Vec<WaterBodyV1>, Vec<Option<WaterBodyIdV1>>)> {
    let mut bodies = Vec::new();
    let mut body_by_cell = vec![None; terminals.len()];
    for (&terminal, spec) in retained {
        let bounds = cell_bounds(domain, &spec.cells)?;
        let surface = match spec.kind {
            WaterBodyKindV1::Wetland => WaterSurfaceModelV1::Wetland {
                level_q8: spec.level_q8,
                depth_q8: u16::try_from(
                    i64::from(spec.level_q8).saturating_sub(i64::from(spec.pit_elevation_q8)),
                )
                .unwrap_or(u16::MAX)
                .min(256),
            },
            WaterBodyKindV1::Lake => WaterSurfaceModelV1::Lake {
                level_q8: spec.level_q8,
            },
            WaterBodyKindV1::Ocean | WaterBodyKindV1::River => {
                return invalid(
                    "topology.retained_body",
                    "retained depression has invalid body kind",
                );
            }
        };
        bodies.push(WaterBodyV1 {
            id: spec.id,
            kind: spec.kind,
            basin: spec.basin,
            outlet: spec.outlet,
            surface,
            min_world_x: bounds.0,
            max_world_x: bounds.1,
            min_world_z: bounds.2,
            max_world_z: bounds.3,
            min_y_q8: spec.pit_elevation_q8,
            max_y_q8: spec.level_q8,
            static_reservoir: true,
            turbidity_per_1024: if spec.kind == WaterBodyKindV1::Wetland {
                640
            } else {
                320
            },
            flow_speed_per_1024: 0,
        });
        for &index in &spec.cells {
            body_by_cell[index as usize] = Some(spec.id);
        }
        if terminals.get(terminal).is_none() {
            return invalid(
                "topology.retained_body",
                "retained terminal lies outside raster",
            );
        }
    }

    let mut segments_by_basin = BTreeMap::<HydrologicBasinIdV1, Vec<&RiverSegmentV1>>::new();
    for segment in segments {
        segments_by_basin
            .entry(segment.basin)
            .or_default()
            .push(segment);
    }
    for (&basin, basin_segments) in &segments_by_basin {
        let identity = identities
            .values()
            .find(|identity| identity.basin == basin)
            .ok_or_else(|| WorldgenError::InvalidHydrologicDomain {
                field: "topology.river_body",
                reason: "river basin has no terminal identity".to_owned(),
            })?;
        let mut profiles = basin_segments
            .iter()
            .map(|segment| (segment.id, segment.centerline.clone()))
            .collect::<Vec<_>>();
        profiles.sort_by_key(|(id, _)| *id);
        let bounds = segment_bounds(basin_segments.as_slice())?;
        bodies.push(WaterBodyV1 {
            id: river_body_by_basin[&basin],
            kind: WaterBodyKindV1::River,
            basin,
            outlet: identity.outlet,
            surface: WaterSurfaceModelV1::River { profiles },
            min_world_x: bounds.0,
            max_world_x: bounds.1,
            min_world_z: bounds.2,
            max_world_z: bounds.3,
            min_y_q8: bounds.4,
            max_y_q8: bounds.5,
            static_reservoir: true,
            turbidity_per_1024: 240,
            flow_speed_per_1024: 512,
        });
    }

    let retained_terminals = retained.keys().copied().collect::<BTreeSet<_>>();
    for (&terminal, identity) in identities {
        if retained_terminals.contains(&terminal) {
            continue;
        }
        let ocean_cells = terminals
            .iter()
            .enumerate()
            .filter_map(|(index, &cell_terminal)| {
                (cell_terminal == terminal
                    && domain.initial_elevation_q8()[index] < domain.base_level_q8())
                .then_some(index)
            })
            .collect::<Vec<_>>();
        if ocean_cells.is_empty() {
            continue;
        }
        let id = WaterBodyIdV1::from_hash(domain_hash(
            b"latticeaxiom.water-body.ocean.id.v1\0",
            &[domain.domain_id().as_bytes(), identity.basin.as_bytes()],
        ));
        let indices = ocean_cells
            .iter()
            .copied()
            .map(index_u32)
            .collect::<WorldgenResult<Vec<_>>>()?;
        let bounds = cell_bounds(domain, &indices)?;
        let min_y = ocean_cells
            .iter()
            .map(|&index| domain.initial_elevation_q8()[index])
            .min()
            .unwrap_or(domain.base_level_q8());
        bodies.push(WaterBodyV1 {
            id,
            kind: WaterBodyKindV1::Ocean,
            basin: identity.basin,
            outlet: identity.outlet,
            surface: WaterSurfaceModelV1::Ocean {
                level_q8: domain.base_level_q8(),
            },
            min_world_x: bounds.0,
            max_world_x: bounds.1,
            min_world_z: bounds.2,
            max_world_z: bounds.3,
            min_y_q8: min_y,
            max_y_q8: domain.base_level_q8(),
            static_reservoir: true,
            turbidity_per_1024: 192,
            flow_speed_per_1024: 64,
        });
        for index in ocean_cells {
            body_by_cell[index] = Some(id);
        }
    }
    Ok((bodies, body_by_cell))
}

fn build_basins(
    domain: &HydrologicDomainPlanV1,
    segments: &[RiverSegmentV1],
    bodies: &[WaterBodyV1],
    terminals: &[usize],
    identities: &BTreeMap<usize, BasinIdentity>,
    hybrid_discharge: &[u64],
) -> WorldgenResult<Vec<BasinRecordV1>> {
    let mut basins = Vec::with_capacity(identities.len());
    for (&terminal, identity) in identities {
        let cell_indices = terminals
            .iter()
            .enumerate()
            .filter_map(|(index, &candidate)| (candidate == terminal).then_some(index))
            .collect::<Vec<_>>();
        let local_runoff = cell_indices.iter().try_fold(0_u64, |sum, &index| {
            sum.checked_add(domain.routed_source_runoff_q16()[index])
                .ok_or(WorldgenError::ArithmeticOverflow {
                    operation: "semantic basin local runoff",
                })
        })?;
        let mut river_segments = segments
            .iter()
            .filter(|segment| segment.basin == identity.basin)
            .map(|segment| segment.id)
            .collect::<Vec<_>>();
        river_segments.sort_unstable();
        let mut water_bodies = bodies
            .iter()
            .filter(|body| body.basin == identity.basin)
            .map(|body| body.id)
            .collect::<Vec<_>>();
        water_bodies.sort_unstable();
        basins.push(BasinRecordV1 {
            id: identity.basin,
            outlet: identity.outlet,
            cell_count: u32::try_from(cell_indices.len()).unwrap_or(u32::MAX),
            local_runoff_q16: local_runoff,
            terminal_discharge_q16: hybrid_discharge[terminal],
            river_segments,
            water_bodies,
        });
    }
    Ok(basins)
}

fn cell_bounds(
    domain: &HydrologicDomainPlanV1,
    indices: &[u32],
) -> WorldgenResult<(i64, i64, i64, i64)> {
    let mut min_x = i64::MAX;
    let mut max_x = i64::MIN;
    let mut min_z = i64::MAX;
    let mut max_z = i64::MIN;
    for &index in indices {
        let coordinate = domain.grid().coordinate_of(index as usize).ok_or_else(|| {
            WorldgenError::InvalidHydrologicDomain {
                field: "topology.water_body.bounds",
                reason: "water-body index lies outside source grid".to_owned(),
            }
        })?;
        let (x, z) = domain.grid().world_coordinate(coordinate)?;
        min_x = min_x.min(x);
        max_x = max_x.max(x);
        min_z = min_z.min(z);
        max_z = max_z.max(z);
    }
    if indices.is_empty() {
        return invalid(
            "topology.water_body.bounds",
            "water body has no member cell",
        );
    }
    Ok((min_x, max_x, min_z, max_z))
}

fn segment_bounds(segments: &[&RiverSegmentV1]) -> WorldgenResult<(i64, i64, i64, i64, i32, i32)> {
    let mut min_x = i64::MAX;
    let mut max_x = i64::MIN;
    let mut min_z = i64::MAX;
    let mut max_z = i64::MIN;
    let mut min_y = i32::MAX;
    let mut max_y = i32::MIN;
    for segment in segments {
        for point in &segment.centerline {
            min_x = min_x.min(point.world_x);
            max_x = max_x.max(point.world_x);
            min_z = min_z.min(point.world_z);
            max_z = max_z.max(point.world_z);
            min_y = min_y.min(point.bed_y_q8);
            max_y = max_y.max(point.surface_y_q8);
        }
    }
    if segments.is_empty() {
        return invalid("topology.river_body.bounds", "river body has no segment");
    }
    Ok((min_x, max_x, min_z, max_z, min_y, max_y))
}

fn stabilize_result_bytes(
    topology: &mut HydrologicTopologyPlanV1,
    max_result_bytes: u64,
) -> WorldgenResult<()> {
    for _ in 0..8 {
        let bytes = u64::try_from(topology.canonical_bytes()?.len()).map_err(|_| {
            WorldgenError::ArithmeticOverflow {
                operation: "hydrologic topology result bytes",
            }
        })?;
        if bytes > max_result_bytes {
            return Err(WorldgenError::BudgetExceeded {
                budget: "hydrologic topology result bytes",
                required: bytes,
                limit: max_result_bytes,
            });
        }
        if topology.accounting.result_bytes == bytes {
            return Ok(());
        }
        topology.accounting.result_bytes = bytes;
    }
    invalid(
        "topology.accounting.result_bytes",
        "canonical result-byte accounting did not reach a fixed point",
    )
}

fn validate_topology(topology: &HydrologicTopologyPlanV1) -> WorldgenResult<()> {
    if topology.channel_segment_by_cell.len() != topology.segment_candidates_by_cell.len()
        || topology.channel_segment_by_cell.len() != topology.water_body_by_cell.len()
        || topology.channel_segment_by_cell.len() != topology.hybrid_discharge_q16.len()
    {
        return invalid(
            "topology.raster_index",
            "topology raster indexes and discharge have inconsistent lengths",
        );
    }
    let segment_ids = topology
        .segments
        .iter()
        .map(|segment| segment.id)
        .collect::<BTreeSet<_>>();
    if segment_ids.len() != topology.segments.len() {
        return invalid("topology.segments", "segment identities are not unique");
    }
    let body_ids = topology
        .water_bodies
        .iter()
        .map(|body| body.id)
        .collect::<BTreeSet<_>>();
    for segment in &topology.segments {
        if !body_ids.contains(&segment.water_body) {
            return invalid(
                "topology.segment.water_body",
                "segment references a missing river body",
            );
        }
        if segment.centerline.len() != 2 {
            return invalid(
                "topology.segment.centerline",
                "revision 1 segments must contain exactly two profile points",
            );
        }
        let source = segment.centerline[0].surface_y_q8;
        let target = segment.centerline[1].surface_y_q8;
        if target > source {
            return invalid(
                "topology.river_profile",
                "river water profile rises downstream",
            );
        }
        let source_point = segment.centerline[0];
        let target_point = segment.centerline[1];
        let expected_metrics = segment_length_and_gradient(
            source_point.world_x,
            source_point.world_z,
            target_point.world_x,
            target_point.world_z,
            source,
            target,
        );
        if (segment.length_q8, segment.gradient_per_million) != expected_metrics {
            return invalid(
                "topology.segment.metrics",
                "stored length or gradient differs from the quantized profile",
            );
        }
        if let Some(downstream) = segment.downstream
            && !segment_ids.contains(&downstream)
        {
            return invalid(
                "topology.segment.downstream",
                "segment references a missing downstream ID",
            );
        }
    }
    let mut spatial_references = 0_u32;
    for references in &topology.segment_candidates_by_cell {
        let mut previous = None;
        for &position in references {
            if position as usize >= topology.segments.len()
                || previous.is_some_and(|p| p >= position)
            {
                return invalid(
                    "topology.spatial_index",
                    "segment references must be in range, strictly ordered, and unique",
                );
            }
            previous = Some(position);
            spatial_references =
                spatial_references
                    .checked_add(1)
                    .ok_or(WorldgenError::ArithmeticOverflow {
                        operation: "topology spatial-reference validation",
                    })?;
        }
    }
    if spatial_references != topology.accounting.spatial_references {
        return invalid(
            "topology.accounting.spatial_references",
            "spatial-reference accounting does not match the index",
        );
    }
    if topology.accounting.source_runoff_q16 != topology.accounting.terminal_runoff_q16 {
        return invalid("topology.runoff", "hybrid routing does not conserve runoff");
    }
    Ok(())
}

fn build_segment_spatial_index(
    domain: &HydrologicDomainPlanV1,
    segments: &[RiverSegmentV1],
    max_references: u32,
) -> WorldgenResult<(Vec<Vec<u32>>, u32)> {
    let grid = domain.grid();
    let spacing = i128::from(grid.spacing_voxels().get());
    let origin_x = i128::from(grid.origin_x());
    let origin_z = i128::from(grid.origin_z());
    let max_x = i128::from(grid.width().get() - 1);
    let max_z = i128::from(grid.height().get() - 1);
    let mut references_by_cell = vec![Vec::new(); grid.sample_count()];
    let mut reference_count = 0_u32;
    for (position, segment) in segments.iter().enumerate() {
        let [source, target] = segment.centerline.as_slice() else {
            return invalid(
                "topology.centerline",
                "v1 river segment must have exactly two points",
            );
        };
        let influence_voxels = i128::from(segment.sdf_influence_radius_q8.div_ceil(256));
        // One extra sample spacing covers the complete nearest-sample Voronoi
        // cell, so any query inside the SDF envelope sees this segment.
        let expansion = influence_voxels + spacing;
        let minimum_x = i128::from(source.world_x.min(target.world_x)) - expansion;
        let maximum_x = i128::from(source.world_x.max(target.world_x)) + expansion;
        let minimum_z = i128::from(source.world_z.min(target.world_z)) - expansion;
        let maximum_z = i128::from(source.world_z.max(target.world_z)) + expansion;
        let first_x = (minimum_x - origin_x).div_euclid(spacing).clamp(0, max_x);
        let last_x = (maximum_x - origin_x).div_euclid(spacing).clamp(0, max_x);
        let first_z = (minimum_z - origin_z).div_euclid(spacing).clamp(0, max_z);
        let last_z = (maximum_z - origin_z).div_euclid(spacing).clamp(0, max_z);
        let position = index_u32(position)?;
        for z in first_z..=last_z {
            for x in first_x..=last_x {
                if reference_count == max_references {
                    return Err(WorldgenError::CollectionLimitExceeded {
                        kind: "topology spatial references",
                        limit: max_references as usize,
                        actual: reference_count as usize + 1,
                    });
                }
                let coordinate = HydrologicGridCoordinateV1::new(
                    u16::try_from(x).map_err(|_| WorldgenError::ArithmeticOverflow {
                        operation: "topology spatial-index X",
                    })?,
                    u16::try_from(z).map_err(|_| WorldgenError::ArithmeticOverflow {
                        operation: "topology spatial-index Z",
                    })?,
                );
                let index = grid.index_of(coordinate).ok_or_else(|| {
                    WorldgenError::InvalidHydrologicDomain {
                        field: "topology.spatial_index",
                        reason: "clamped spatial reference lies outside the domain".to_owned(),
                    }
                })?;
                references_by_cell[index].push(position);
                reference_count += 1;
            }
        }
    }
    Ok((references_by_cell, reference_count))
}

#[derive(Clone, Copy, Debug)]
struct SegmentProjection {
    t_q16: u32,
    distance_q8: i64,
}

#[allow(
    clippy::similar_names,
    reason = "paired X/Z fixed-point projection names make axis symmetry auditable"
)]
fn project_segment_q16(
    world_x: i64,
    world_z: i64,
    source: QuantizedRiverPointV1,
    target: QuantizedRiverPointV1,
) -> WorldgenResult<SegmentProjection> {
    let dx = i128::from(target.world_x) - i128::from(source.world_x);
    let dz = i128::from(target.world_z) - i128::from(source.world_z);
    let px = i128::from(world_x) - i128::from(source.world_x);
    let pz = i128::from(world_z) - i128::from(source.world_z);
    let denominator = dx
        .checked_mul(dx)
        .and_then(|value| dz.checked_mul(dz).and_then(|z| value.checked_add(z)))
        .ok_or(WorldgenError::ArithmeticOverflow {
            operation: "river SDF segment length",
        })?;
    if denominator == 0 {
        let squared = px
            .checked_mul(px)
            .and_then(|value| pz.checked_mul(pz).and_then(|z| value.checked_add(z)))
            .ok_or(WorldgenError::ArithmeticOverflow {
                operation: "river SDF point distance",
            })?;
        return Ok(SegmentProjection {
            t_q16: 0,
            distance_q8: i64::try_from(integer_sqrt_u128(
                u128::try_from(squared).unwrap_or(u128::MAX),
            ))
            .unwrap_or(i64::MAX)
            .saturating_mul(256),
        });
    }
    let dot = px
        .checked_mul(dx)
        .and_then(|value| pz.checked_mul(dz).and_then(|z| value.checked_add(z)))
        .ok_or(WorldgenError::ArithmeticOverflow {
            operation: "river SDF projection dot product",
        })?
        .clamp(0, denominator);
    let t_q16 = u32::try_from(dot.saturating_mul(65_536) / denominator).unwrap_or(u32::MAX);
    let closest_x_q16 = i128::from(source.world_x)
        .saturating_mul(65_536)
        .saturating_add(dx.saturating_mul(i128::from(t_q16)));
    let closest_z_q16 = i128::from(source.world_z)
        .saturating_mul(65_536)
        .saturating_add(dz.saturating_mul(i128::from(t_q16)));
    let delta_x_q16 = i128::from(world_x)
        .saturating_mul(65_536)
        .saturating_sub(closest_x_q16);
    let delta_z_q16 = i128::from(world_z)
        .saturating_mul(65_536)
        .saturating_sub(closest_z_q16);
    let squared_q32 = delta_x_q16
        .checked_mul(delta_x_q16)
        .and_then(|value| {
            delta_z_q16
                .checked_mul(delta_z_q16)
                .and_then(|z| value.checked_add(z))
        })
        .ok_or(WorldgenError::ArithmeticOverflow {
            operation: "river SDF projected distance",
        })?;
    let distance_q16 = integer_sqrt_u128(u128::try_from(squared_q32).unwrap_or(u128::MAX));
    Ok(SegmentProjection {
        t_q16,
        distance_q8: i64::try_from(distance_q16 / 256).unwrap_or(i64::MAX),
    })
}

fn interpolate_i32(source: i32, target: i32, t_q16: u32) -> i32 {
    let delta = i64::from(target) - i64::from(source);
    let value = i64::from(source) + delta.saturating_mul(i64::from(t_q16)) / 65_536;
    i32::try_from(value).unwrap_or(if value.is_negative() {
        i32::MIN
    } else {
        i32::MAX
    })
}

fn integer_sqrt_u128(value: u128) -> u128 {
    if value < 2 {
        return value;
    }
    let mut low = 1_u128;
    let mut high = 1_u128 << value.ilog2().div_ceil(2);
    while low <= high {
        let middle = low + (high - low) / 2;
        if middle <= value / middle {
            low = middle + 1;
        } else {
            high = middle - 1;
        }
    }
    high
}

/// Continuous query result inside a protected river SDF envelope.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RiverSdfSampleV1 {
    segment: RiverSegmentIdV1,
    signed_distance_q8: i64,
    channel_signed_distance_q8: i64,
    bed_y_q8: i32,
    surface_y_q8: i32,
    flow_tangent_q15: [i16; 2],
    waterfall: bool,
}

impl RiverSdfSampleV1 {
    /// Returns the owning segment.
    #[must_use]
    pub const fn segment(self) -> RiverSegmentIdV1 {
        self.segment
    }

    /// Returns signed distance to the protected influence envelope in Q8 voxels.
    #[must_use]
    pub const fn signed_distance_q8(self) -> i64 {
        self.signed_distance_q8
    }

    /// Returns signed distance to the actual water half-width in Q8 voxels.
    #[must_use]
    pub const fn channel_signed_distance_q8(self) -> i64 {
        self.channel_signed_distance_q8
    }

    /// Returns protected river-bed Y in Q24.8 voxels.
    #[must_use]
    pub const fn bed_y_q8(self) -> i32 {
        self.bed_y_q8
    }

    /// Returns authoritative river-surface Y in Q24.8 voxels.
    #[must_use]
    pub const fn surface_y_q8(self) -> i32 {
        self.surface_y_q8
    }
}

/// Queryable static-water column selected from topology and domain surfaces.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StaticWaterColumnV1 {
    water_body: WaterBodyIdV1,
    kind: WaterBodyKindV1,
    bed_y_q8: i32,
    surface_y_q8: i32,
    flow_tangent_q15: [i16; 2],
    waterfall: bool,
}

impl StaticWaterColumnV1 {
    /// Returns the water body.
    #[must_use]
    pub const fn water_body(self) -> WaterBodyIdV1 {
        self.water_body
    }

    /// Returns body kind.
    #[must_use]
    pub const fn kind(self) -> WaterBodyKindV1 {
        self.kind
    }

    /// Returns water surface Y in Q24.8 voxels.
    #[must_use]
    pub const fn surface_y_q8(self) -> i32 {
        self.surface_y_q8
    }

    /// Returns bed Y in Q24.8 voxels.
    #[must_use]
    pub const fn bed_y_q8(self) -> i32 {
        self.bed_y_q8
    }
}

/// V1 cardinal/static flow emitted by reservoir materialization.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum StaticReservoirFlowV1 {
    /// Standing lake/ocean/wetland water.
    Still,
    /// Explicit vertical waterfall sheet.
    Down,
    /// Positive X river presentation flow.
    East,
    /// Negative X river presentation flow.
    West,
    /// Positive Z river presentation flow.
    South,
    /// Negative Z river presentation flow.
    North,
}

/// One sparse static water voxel with accepted `FluidStateV1` level semantics.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct StaticReservoirVoxelV1 {
    x: u16,
    y: u16,
    z: u16,
    fluid: StableId,
    water_body: WaterBodyIdV1,
    level: u8,
    flow: StaticReservoirFlowV1,
}

impl StaticReservoirVoxelV1 {
    /// Returns local `(x, y, z)`.
    #[must_use]
    pub const fn coordinate(&self) -> [u16; 3] {
        [self.x, self.y, self.z]
    }

    /// Returns frozen fluid identity.
    #[must_use]
    pub const fn fluid(&self) -> &StableId {
        &self.fluid
    }

    /// Returns accepted level `0..=7`.
    #[must_use]
    pub const fn level(&self) -> u8 {
        self.level
    }

    /// Returns explicit flow.
    #[must_use]
    pub const fn flow(&self) -> StaticReservoirFlowV1 {
        self.flow
    }
}

/// Sparse static-reservoir candidate for one chunk.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct StaticReservoirChunkV1 {
    schema: &'static str,
    chunk: ChunkCoordinate,
    generation_epoch: GenerationEpochIdV1,
    topology_hash: HydrologicTopologyHashV1,
    reservoir_hash: StaticReservoirHashV1,
    cells: Vec<StaticReservoirVoxelV1>,
    runtime_frontier: Vec<[u16; 3]>,
}

impl StaticReservoirChunkV1 {
    /// Returns sparse cells in canonical `(y, z, x)` order.
    #[must_use]
    pub fn cells(&self) -> &[StaticReservoirVoxelV1] {
        &self.cells
    }

    /// Returns the deliberately empty generated-fluid runtime frontier.
    #[must_use]
    pub fn runtime_frontier(&self) -> &[[u16; 3]] {
        &self.runtime_frontier
    }

    /// Returns canonical candidate hash.
    #[must_use]
    pub const fn reservoir_hash(&self) -> StaticReservoirHashV1 {
        self.reservoir_hash
    }

    /// Returns canonical bytes.
    ///
    /// # Errors
    ///
    /// Returns a canonical-encoding error if serialization fails.
    pub fn canonical_bytes(&self) -> WorldgenResult<Vec<u8>> {
        canonical_json_bytes(self).map_err(|error| WorldgenError::CanonicalEncoding {
            kind: "StaticReservoirChunkV1",
            reason: error.to_string(),
        })
    }
}

#[derive(Clone, Copy, Debug)]
struct HybridReceiver {
    index: usize,
    weight: u32,
}

#[derive(Clone, Copy, Debug)]
struct BasinIdentity {
    basin: HydrologicBasinIdV1,
    outlet: HydrologicOutletIdV1,
}

/// Extracts the conservative single-receiver channel DAG and generated water
/// bodies from one immutable finite-domain plan.
///
/// # Errors
///
/// Returns a hydrologic validation, arithmetic, canonical-encoding, graph, or
/// hard-budget error. No partial topology is returned.
#[allow(
    clippy::too_many_lines,
    reason = "the coordinator keeps all intermediate graphs private and publishes only after every invariant passes"
)]
pub fn build_hydrologic_topology_v1(
    domain: &HydrologicDomainPlanV1,
    config: &HydrologicTopologyConfigV1,
) -> WorldgenResult<HydrologicTopologyPlanV1> {
    config.validate()?;
    let count = domain.routing().len();
    if domain.grid().sample_count() != count
        || domain.routed_source_runoff_q16().len() != count
        || domain.routing_elevation_q8().len() != count
    {
        return invalid(
            "topology.domain",
            "domain raster arrays have inconsistent lengths",
        );
    }
    let domain_hash_value = domain.canonical_hash()?;
    let config_hash = config.canonical_hash()?;
    let primary = domain
        .routing()
        .iter()
        .map(primary_receiver)
        .collect::<Vec<_>>();
    let original_graph = domain
        .routing()
        .iter()
        .map(|cell| {
            cell.receivers()
                .iter()
                .map(|receiver| HybridReceiver {
                    index: receiver.receiver_index() as usize,
                    weight: receiver.weight_per_million(),
                })
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    let original_order = topological_order(&original_graph)?;
    let terminals = terminal_indices(&primary, &original_order)?;
    let terminal_identities = basin_identities(domain, &terminals)?;
    let (mut outlets, retained_body_specs, port_by_terminal) =
        outlet_records(domain, &terminal_identities)?;

    let channel_mask = domain
        .routing()
        .iter()
        .map(|cell| {
            cell.accumulated_runoff_q16() >= config.channel_threshold_q16
                && !cell.receivers().is_empty()
        })
        .collect::<Vec<_>>();
    let channel_count = channel_mask.iter().filter(|&&channel| channel).count();
    check_limit(
        "topology river segments",
        channel_count,
        config.max_segments as usize,
    )?;

    let hybrid_graph = original_graph
        .iter()
        .enumerate()
        .map(|(index, receivers)| {
            if channel_mask[index] {
                primary[index]
                    .map(|receiver| {
                        vec![HybridReceiver {
                            index: receiver,
                            weight: crate::HYDROLOGIC_WEIGHT_SCALE_V1,
                        }]
                    })
                    .unwrap_or_default()
            } else {
                receivers.clone()
            }
        })
        .collect::<Vec<_>>();
    let hybrid_order = topological_order(&hybrid_graph)?;
    let (hybrid_discharge, source_sum, terminal_sum) = conservative_accumulation(
        domain.routed_source_runoff_q16(),
        &hybrid_graph,
        &hybrid_order,
    )?;
    for outlet in &mut outlets {
        let index = domain.grid().index_of(outlet.coordinate).ok_or_else(|| {
            WorldgenError::InvalidHydrologicDomain {
                field: "topology.outlet",
                reason: "outlet coordinate lies outside source grid".to_owned(),
            }
        })?;
        outlet.terminal_discharge_q16 = hybrid_discharge[index];
    }

    let channel_indices = channel_mask
        .iter()
        .enumerate()
        .filter_map(|(index, &channel)| channel.then_some(index))
        .collect::<Vec<_>>();
    let segment_id_by_cell = channel_indices
        .iter()
        .copied()
        .map(|index| {
            Ok((
                index,
                RiverSegmentIdV1::from_hash(domain_hash(
                    b"latticeaxiom.river-segment.id.v1\0",
                    &[
                        domain.domain_id().as_bytes(),
                        &index_u32(index)?.to_be_bytes(),
                    ],
                )),
            ))
        })
        .collect::<WorldgenResult<BTreeMap<_, _>>>()?;
    let downstream_cell = channel_indices
        .iter()
        .copied()
        .map(|index| {
            Ok((
                index,
                trace_next_channel(index, &primary, &channel_mask, count)?,
            ))
        })
        .collect::<WorldgenResult<BTreeMap<_, _>>>()?;
    let mut upstream_by_cell = BTreeMap::<usize, Vec<usize>>::new();
    for (&index, &downstream) in &downstream_cell {
        if let Some(downstream) = downstream {
            upstream_by_cell.entry(downstream).or_default().push(index);
        }
    }
    for upstream in upstream_by_cell.values_mut() {
        upstream.sort_unstable();
    }
    let strahler = strahler_orders(&channel_indices, &downstream_cell, &upstream_by_cell)?;

    let river_body_by_basin = terminal_identities
        .values()
        .map(|identity| {
            (
                identity.basin,
                WaterBodyIdV1::from_hash(domain_hash(
                    b"latticeaxiom.water-body.river.id.v1\0",
                    &[domain.domain_id().as_bytes(), identity.basin.as_bytes()],
                )),
            )
        })
        .collect::<BTreeMap<_, _>>();
    let outlet_level = outlets
        .iter()
        .map(|outlet| (outlet.id, outlet.level_q8))
        .collect::<BTreeMap<_, _>>();
    let mut surface_by_channel = BTreeMap::<usize, i32>::new();
    for &index in hybrid_order
        .iter()
        .rev()
        .filter(|index| channel_mask[**index])
    {
        let terminal = terminals[index];
        let identity = terminal_identities.get(&terminal).ok_or_else(|| {
            WorldgenError::InvalidHydrologicDomain {
                field: "topology.basins",
                reason: "channel terminal has no basin identity".to_owned(),
            }
        })?;
        let downstream_surface = downstream_cell[&index]
            .and_then(|downstream| surface_by_channel.get(&downstream).copied())
            .or_else(|| outlet_level.get(&identity.outlet).copied())
            .unwrap_or(domain.base_level_q8());
        surface_by_channel.insert(
            index,
            domain.routing_elevation_q8()[index].max(downstream_surface),
        );
    }

    let mut segments = Vec::with_capacity(channel_count);
    for &index in &channel_indices {
        let terminal = terminals[index];
        let identity = terminal_identities[&terminal];
        let segment_id = segment_id_by_cell[&index];
        let downstream = downstream_cell[&index];
        let downstream_id = downstream.map(|cell| segment_id_by_cell[&cell]);
        let endpoint = if let Some(id) = downstream_id {
            RiverEndpointV1::Segment(id)
        } else if let Some(port) = port_by_terminal.get(&terminal) {
            RiverEndpointV1::BoundaryPort(*port)
        } else if let Some(body) = retained_body_specs.get(&terminal).map(|spec| spec.id) {
            RiverEndpointV1::WaterBody(body)
        } else {
            RiverEndpointV1::Outlet(identity.outlet)
        };
        let order = strahler[&index];
        let (width_q8, depth_q8) = channel_geometry(config, hybrid_discharge[index], order);
        let downstream_index = downstream.unwrap_or(terminal);
        let source_surface = surface_by_channel[&index];
        let target_surface = downstream
            .and_then(|cell| surface_by_channel.get(&cell).copied())
            .unwrap_or_else(|| outlet_level[&identity.outlet]);
        let source_bed = source_surface.saturating_sub(i32::from(depth_q8));
        let target_bed = target_surface.saturating_sub(i32::from(depth_q8));
        let drop = i64::from(source_surface) - i64::from(target_surface);
        let waterfall = drop > i64::from(config.waterfall_drop_q8);
        let source_coordinate = domain.grid().coordinate_of(index).ok_or_else(|| {
            WorldgenError::InvalidHydrologicDomain {
                field: "topology.channel",
                reason: "channel index lies outside the domain grid".to_owned(),
            }
        })?;
        let target_coordinate = domain
            .grid()
            .coordinate_of(downstream_index)
            .ok_or_else(|| WorldgenError::InvalidHydrologicDomain {
                field: "topology.channel",
                reason: "downstream index lies outside the domain grid".to_owned(),
            })?;
        let (source_x, source_z) = domain.grid().world_coordinate(source_coordinate)?;
        let (target_x, target_z) = domain.grid().world_coordinate(target_coordinate)?;
        let (length_q8, gradient_per_million) = segment_length_and_gradient(
            source_x,
            source_z,
            target_x,
            target_z,
            source_surface,
            target_surface,
        );
        let floodplain_width = u32::from(width_q8)
            .checked_mul(u32::from(config.floodplain_multiplier_per_1024))
            .map(|value| value / 1_024)
            .ok_or(WorldgenError::ArithmeticOverflow {
                operation: "river floodplain width",
            })?;
        let influence = u32::from(width_q8) / 2
            + floodplain_width
            + u32::from(depth_q8)
                .checked_mul(1_024)
                .map(|value| value / u32::from(config.bank_slope_per_1024.max(1)))
                .ok_or(WorldgenError::ArithmeticOverflow {
                    operation: "river bank SDF radius",
                })?;
        let mut upstream = upstream_by_cell
            .get(&index)
            .into_iter()
            .flatten()
            .map(|upstream| segment_id_by_cell[upstream])
            .collect::<Vec<_>>();
        upstream.sort_unstable();
        segments.push(RiverSegmentV1 {
            id: segment_id,
            upstream,
            downstream: downstream_id,
            endpoint,
            basin: identity.basin,
            water_body: river_body_by_basin[&identity.basin],
            centerline: vec![
                QuantizedRiverPointV1 {
                    world_x: source_x,
                    world_z: source_z,
                    surface_y_q8: source_surface,
                    bed_y_q8: source_bed,
                    waterfall_after: waterfall,
                },
                QuantizedRiverPointV1 {
                    world_x: target_x,
                    world_z: target_z,
                    surface_y_q8: target_surface,
                    bed_y_q8: target_bed,
                    waterfall_after: false,
                },
            ],
            length_q8,
            gradient_per_million,
            discharge_q16: hybrid_discharge[index],
            strahler_order: order,
            width_q8,
            bed_depth_q8: depth_q8,
            bank_slope_per_1024: config.bank_slope_per_1024,
            floodplain_width_q8: floodplain_width,
            sdf_influence_radius_q8: influence,
            flow_tangent_q15: tangent_q15(source_x, source_z, target_x, target_z),
        });
    }
    segments.sort_by_key(|segment| segment.id);
    let position_by_id = segments
        .iter()
        .enumerate()
        .map(|(position, segment)| Ok((segment.id, index_u32(position)?)))
        .collect::<WorldgenResult<BTreeMap<_, _>>>()?;
    let mut channel_segment_by_cell = vec![None; count];
    for (&cell, &id) in &segment_id_by_cell {
        channel_segment_by_cell[cell] = Some(position_by_id[&id]);
    }
    let (segment_candidates_by_cell, spatial_references) =
        build_segment_spatial_index(domain, &segments, config.max_spatial_references)?;

    let (mut water_bodies, water_body_by_cell) = build_water_bodies(
        domain,
        &segments,
        &terminals,
        &terminal_identities,
        &retained_body_specs,
        &river_body_by_basin,
    )?;
    water_bodies.sort_by_key(|body| body.id);
    check_limit(
        "topology water bodies",
        water_bodies.len(),
        config.max_water_bodies as usize,
    )?;
    let mut basins = build_basins(
        domain,
        &segments,
        &water_bodies,
        &terminals,
        &terminal_identities,
        &hybrid_discharge,
    )?;
    basins.sort_by_key(|basin| basin.id);
    check_limit("topology basins", basins.len(), config.max_basins as usize)?;
    outlets.sort_by_key(|outlet| outlet.id);
    let profile_points =
        segments
            .len()
            .checked_mul(2)
            .ok_or(WorldgenError::ArithmeticOverflow {
                operation: "river profile point count",
            })?;
    check_limit(
        "topology profile points",
        profile_points,
        config.max_profile_points as usize,
    )?;
    let mut topology = HydrologicTopologyPlanV1 {
        algorithm: TOPOLOGY_ALGORITHM,
        domain_plan_hash: domain_hash_value,
        config_hash,
        generation_epoch: domain.generation_epoch(),
        segments,
        water_bodies,
        basins,
        outlets,
        channel_segment_by_cell,
        segment_candidates_by_cell,
        water_body_by_cell,
        hybrid_discharge_q16: hybrid_discharge,
        accounting: HydrologicTopologyAccountingV1 {
            channel_cells: u32::try_from(channel_count).unwrap_or(u32::MAX),
            segment_count: u32::try_from(channel_count).unwrap_or(u32::MAX),
            basin_count: u32::try_from(terminal_identities.len()).unwrap_or(u32::MAX),
            water_body_count: 0,
            profile_points: u32::try_from(profile_points).unwrap_or(u32::MAX),
            spatial_references,
            source_runoff_q16: source_sum,
            terminal_runoff_q16: terminal_sum,
            result_bytes: 0,
        },
    };
    topology.accounting.water_body_count =
        u32::try_from(topology.water_bodies.len()).unwrap_or(u32::MAX);
    stabilize_result_bytes(&mut topology, config.max_result_bytes)?;
    validate_topology(&topology)?;
    Ok(topology)
}

impl HydrologicTopologyPlanV1 {
    /// Samples the closest river SDF/profile at one world `(x, z)` position.
    ///
    /// # Errors
    ///
    /// Returns a validation or canonical-encoding error when the supplied
    /// domain is not the exact source plan.
    pub fn river_sdf_sample(
        &self,
        domain: &HydrologicDomainPlanV1,
        world_x: i64,
        world_z: i64,
    ) -> WorldgenResult<Option<RiverSdfSampleV1>> {
        self.validate_domain(domain)?;
        self.river_sdf_sample_unchecked(world_x, world_z)
    }

    fn river_sdf_sample_unchecked(
        &self,
        world_x: i64,
        world_z: i64,
    ) -> WorldgenResult<Option<RiverSdfSampleV1>> {
        self.river_sdf_sample_from_positions(0..self.segments.len(), world_x, world_z)
    }

    fn river_sdf_sample_nearby_unchecked(
        &self,
        domain: &HydrologicDomainPlanV1,
        world_x: i64,
        world_z: i64,
    ) -> WorldgenResult<Option<RiverSdfSampleV1>> {
        let Some(coordinate) = domain.grid().nearest_coordinate(world_x, world_z) else {
            return self.river_sdf_sample_unchecked(world_x, world_z);
        };
        let index = domain.grid().index_of(coordinate).ok_or_else(|| {
            WorldgenError::InvalidHydrologicDomain {
                field: "topology.spatial_index",
                reason: "nearest coordinate lies outside the source grid".to_owned(),
            }
        })?;
        self.river_sdf_sample_from_positions(
            self.segment_candidates_by_cell[index]
                .iter()
                .map(|&position| position as usize),
            world_x,
            world_z,
        )
    }

    fn river_sdf_sample_from_positions(
        &self,
        positions: impl IntoIterator<Item = usize>,
        world_x: i64,
        world_z: i64,
    ) -> WorldgenResult<Option<RiverSdfSampleV1>> {
        let mut best = None::<(i64, RiverSegmentIdV1, RiverSdfSampleV1)>;
        for position in positions {
            let segment = self.segments.get(position).ok_or_else(|| {
                WorldgenError::InvalidHydrologicDomain {
                    field: "topology.spatial_index",
                    reason: "spatial index references a missing segment".to_owned(),
                }
            })?;
            let [source, target] = segment.centerline.as_slice() else {
                return invalid(
                    "topology.centerline",
                    "v1 river segment must have exactly two points",
                );
            };
            let projection = project_segment_q16(world_x, world_z, *source, *target)?;
            let distance_q8 = projection.distance_q8;
            let influence_signed = distance_q8 - i64::from(segment.sdf_influence_radius_q8);
            let channel_signed = distance_q8 - i64::from(segment.width_q8 / 2);
            let surface =
                interpolate_i32(source.surface_y_q8, target.surface_y_q8, projection.t_q16);
            let bed = interpolate_i32(source.bed_y_q8, target.bed_y_q8, projection.t_q16);
            let sample = RiverSdfSampleV1 {
                segment: segment.id,
                signed_distance_q8: influence_signed,
                channel_signed_distance_q8: channel_signed,
                bed_y_q8: bed,
                surface_y_q8: surface,
                flow_tangent_q15: segment.flow_tangent_q15,
                waterfall: source.waterfall_after,
            };
            let key = (distance_q8, segment.id, sample);
            if best
                .as_ref()
                .is_none_or(|current| (key.0, key.1) < (current.0, current.1))
            {
                best = Some(key);
            }
        }
        Ok(best.map(|(_, _, sample)| sample))
    }

    /// Samples static generated water at one world column.
    ///
    /// # Errors
    ///
    /// Returns a validation or canonical-encoding error when the domain does
    /// not match this topology.
    pub fn static_water_column(
        &self,
        domain: &HydrologicDomainPlanV1,
        world_x: i64,
        world_z: i64,
    ) -> WorldgenResult<Option<StaticWaterColumnV1>> {
        self.validate_domain(domain)?;
        self.static_water_column_unchecked(domain, world_x, world_z)
    }

    fn static_water_column_unchecked(
        &self,
        domain: &HydrologicDomainPlanV1,
        world_x: i64,
        world_z: i64,
    ) -> WorldgenResult<Option<StaticWaterColumnV1>> {
        let coordinate = domain.grid().nearest_coordinate(world_x, world_z);
        if let Some(coordinate) = coordinate {
            let index = domain.grid().index_of(coordinate).ok_or_else(|| {
                WorldgenError::InvalidHydrologicDomain {
                    field: "topology.query",
                    reason: "nearest coordinate lies outside the source grid".to_owned(),
                }
            })?;
            if let Some(body_id) = self.water_body_by_cell[index] {
                let body_position = self
                    .water_bodies
                    .binary_search_by_key(&body_id, |body| body.id)
                    .map_err(|_| WorldgenError::InvalidHydrologicDomain {
                        field: "topology.water_body_by_cell",
                        reason: "cell references a missing water body".to_owned(),
                    })?;
                let body = &self.water_bodies[body_position];
                let surface = match body.surface {
                    WaterSurfaceModelV1::Ocean { level_q8 }
                    | WaterSurfaceModelV1::Lake { level_q8 }
                    | WaterSurfaceModelV1::Wetland { level_q8, .. } => level_q8,
                    WaterSurfaceModelV1::River { .. } => domain.routing_elevation_q8()[index],
                };
                return Ok(Some(StaticWaterColumnV1 {
                    water_body: body.id,
                    kind: body.kind,
                    bed_y_q8: domain.initial_elevation_q8()[index],
                    surface_y_q8: surface,
                    flow_tangent_q15: [0, 0],
                    waterfall: false,
                }));
            }
        }
        let Some(river) = self.river_sdf_sample_nearby_unchecked(domain, world_x, world_z)? else {
            return Ok(None);
        };
        if river.channel_signed_distance_q8 > 0 {
            return Ok(None);
        }
        let segment_position = self
            .segments
            .binary_search_by_key(&river.segment, |segment| segment.id)
            .map_err(|_| WorldgenError::InvalidHydrologicDomain {
                field: "topology.river_sdf",
                reason: "SDF sample references a missing segment".to_owned(),
            })?;
        let segment = &self.segments[segment_position];
        Ok(Some(StaticWaterColumnV1 {
            water_body: segment.water_body,
            kind: WaterBodyKindV1::River,
            bed_y_q8: river.bed_y_q8,
            surface_y_q8: river.surface_y_q8,
            flow_tangent_q15: river.flow_tangent_q15,
            waterfall: river.waterfall,
        }))
    }

    fn validate_domain(&self, domain: &HydrologicDomainPlanV1) -> WorldgenResult<()> {
        if domain.canonical_hash()? != self.domain_plan_hash {
            return invalid(
                "topology.domain_plan_hash",
                "query domain differs from topology source",
            );
        }
        Ok(())
    }
}

/// Prevalidated view used to sample many chunks from one immutable domain and
/// topology without repeatedly hashing the dense domain artifact.
#[derive(Clone, Copy, Debug)]
pub struct StaticReservoirSamplerV1<'a> {
    domain: &'a HydrologicDomainPlanV1,
    topology: &'a HydrologicTopologyPlanV1,
    config: &'a HydrologicTopologyConfigV1,
    topology_hash: HydrologicTopologyHashV1,
}

impl<'a> StaticReservoirSamplerV1<'a> {
    /// Binds an exact topology/config pair to its source domain.
    ///
    /// # Errors
    ///
    /// Returns a validation or canonical-encoding error when any artifact
    /// differs from the inputs used to build the topology.
    pub fn new(
        domain: &'a HydrologicDomainPlanV1,
        topology: &'a HydrologicTopologyPlanV1,
        config: &'a HydrologicTopologyConfigV1,
    ) -> WorldgenResult<Self> {
        config.validate()?;
        topology.validate_domain(domain)?;
        if config.canonical_hash()? != topology.config_hash {
            return invalid(
                "topology.config_hash",
                "sampling config differs from topology extraction config",
            );
        }
        Ok(Self {
            domain,
            topology,
            config,
            topology_hash: topology.canonical_hash()?,
        })
    }

    /// Materializes one bounded sparse static-reservoir chunk.
    ///
    /// # Errors
    ///
    /// Returns a validation, arithmetic, coordinate, or hard-budget error.
    pub fn materialize(
        self,
        chunk: ChunkCoordinate,
        chunk_edge: NonZeroU16,
        world_floor_y: i32,
        world_ceiling_y: i32,
        water: &StableId,
    ) -> WorldgenResult<StaticReservoirChunkV1> {
        materialize_static_reservoir_chunk_bound_v1(
            self.domain,
            self.topology,
            self.config,
            self.topology_hash,
            chunk,
            chunk_edge,
            world_floor_y,
            world_ceiling_y,
            water,
        )
    }
}

/// Materializes only the static generated water intersecting one chunk.
///
/// Interior cells use full/source level zero. A fractional top is quantized
/// upward to the nearest accepted eighth and encoded as level `0..=7`.
/// Generated occupancy never enters the runtime frontier.
///
/// # Errors
///
/// Returns a validation, arithmetic, canonical-encoding, topology-mismatch,
/// coordinate, or hard cell-budget error.
#[allow(
    clippy::too_many_arguments,
    clippy::too_many_lines,
    reason = "materialization closes chunk geometry, world bounds, identity, topology policy, and atomic hash publication"
)]
pub fn materialize_static_reservoir_chunk_v1(
    domain: &HydrologicDomainPlanV1,
    topology: &HydrologicTopologyPlanV1,
    config: &HydrologicTopologyConfigV1,
    chunk: ChunkCoordinate,
    chunk_edge: NonZeroU16,
    world_floor_y: i32,
    world_ceiling_y: i32,
    water: &StableId,
) -> WorldgenResult<StaticReservoirChunkV1> {
    StaticReservoirSamplerV1::new(domain, topology, config)?.materialize(
        chunk,
        chunk_edge,
        world_floor_y,
        world_ceiling_y,
        water,
    )
}

#[allow(
    clippy::too_many_arguments,
    clippy::too_many_lines,
    reason = "materialization closes chunk geometry, world bounds, identity, topology policy, and atomic hash publication"
)]
fn materialize_static_reservoir_chunk_bound_v1(
    domain: &HydrologicDomainPlanV1,
    topology: &HydrologicTopologyPlanV1,
    config: &HydrologicTopologyConfigV1,
    topology_hash: HydrologicTopologyHashV1,
    chunk: ChunkCoordinate,
    chunk_edge: NonZeroU16,
    world_floor_y: i32,
    world_ceiling_y: i32,
    water: &StableId,
) -> WorldgenResult<StaticReservoirChunkV1> {
    if water.kind() != "fluid" || water.major().is_some() {
        return invalid(
            "static_reservoir.water",
            "water identity must be an unversioned fluid registration",
        );
    }
    if world_floor_y > world_ceiling_y {
        return invalid(
            "static_reservoir.world_bounds",
            "world floor exceeds ceiling",
        );
    }
    if chunk_edge.get() > MAX_STATIC_RESERVOIR_CHUNK_EDGE {
        return Err(WorldgenError::BudgetExceeded {
            budget: "static reservoir chunk edge",
            required: u64::from(chunk_edge.get()),
            limit: u64::from(MAX_STATIC_RESERVOIR_CHUNK_EDGE),
        });
    }
    let edge = i64::from(chunk_edge.get());
    let origin_x =
        i64::from(chunk.x)
            .checked_mul(edge)
            .ok_or(WorldgenError::ArithmeticOverflow {
                operation: "static reservoir chunk origin X",
            })?;
    let origin_y =
        i64::from(chunk.y)
            .checked_mul(edge)
            .ok_or(WorldgenError::ArithmeticOverflow {
                operation: "static reservoir chunk origin Y",
            })?;
    let origin_z =
        i64::from(chunk.z)
            .checked_mul(edge)
            .ok_or(WorldgenError::ArithmeticOverflow {
                operation: "static reservoir chunk origin Z",
            })?;
    let chunk_max_y = origin_y
        .checked_add(edge - 1)
        .ok_or(WorldgenError::ArithmeticOverflow {
            operation: "static reservoir chunk maximum Y",
        })?;
    let bounded_min_y = origin_y.max(i64::from(world_floor_y));
    let bounded_max_y = chunk_max_y.min(i64::from(world_ceiling_y));
    let mut cells = Vec::new();
    if bounded_min_y <= bounded_max_y {
        for local_z in 0..chunk_edge.get() {
            for local_x in 0..chunk_edge.get() {
                let world_x = origin_x + i64::from(local_x);
                let world_z = origin_z + i64::from(local_z);
                let Some(column) =
                    topology.static_water_column_unchecked(domain, world_x, world_z)?
                else {
                    continue;
                };
                let first_water_y = i64::from(column.bed_y_q8).div_euclid(256);
                let last_water_y = i64::from(column.surface_y_q8.saturating_sub(1)).div_euclid(256);
                let first = first_water_y.max(bounded_min_y);
                let last = last_water_y.min(bounded_max_y);
                if first > last {
                    continue;
                }
                for world_y in first..=last {
                    let local_y = u16::try_from(world_y - origin_y).map_err(|_| {
                        WorldgenError::ArithmeticOverflow {
                            operation: "static reservoir local Y",
                        }
                    })?;
                    let level = if world_y == last_water_y {
                        surface_level_v1(column.surface_y_q8, world_y)?
                    } else {
                        0
                    };
                    cells.push(StaticReservoirVoxelV1 {
                        x: local_x,
                        y: local_y,
                        z: local_z,
                        fluid: water.clone(),
                        water_body: column.water_body,
                        level,
                        flow: static_flow(column),
                    });
                    check_limit(
                        "static reservoir cells per chunk",
                        cells.len(),
                        config.max_static_cells_per_chunk as usize,
                    )?;
                }
            }
        }
    }
    cells.sort_by_key(|cell| (cell.y, cell.z, cell.x, cell.water_body));
    let hash_input = StaticReservoirHashInput {
        schema: STATIC_RESERVOIR_SCHEMA,
        chunk,
        generation_epoch: domain.generation_epoch(),
        topology_hash,
        cells: &cells,
        runtime_frontier: &[],
    };
    let bytes =
        canonical_json_bytes(&hash_input).map_err(|error| WorldgenError::CanonicalEncoding {
            kind: "StaticReservoirHashInput",
            reason: error.to_string(),
        })?;
    let reservoir_hash = StaticReservoirHashV1::from_hash(domain_hash(
        b"latticeaxiom.static-reservoir.chunk.v1\0",
        &[&bytes],
    ));
    Ok(StaticReservoirChunkV1 {
        schema: STATIC_RESERVOIR_SCHEMA,
        chunk,
        generation_epoch: domain.generation_epoch(),
        topology_hash,
        reservoir_hash,
        cells,
        runtime_frontier: Vec::new(),
    })
}

#[derive(Serialize)]
struct StaticReservoirHashInput<'a> {
    schema: &'static str,
    chunk: ChunkCoordinate,
    generation_epoch: GenerationEpochIdV1,
    topology_hash: HydrologicTopologyHashV1,
    cells: &'a [StaticReservoirVoxelV1],
    runtime_frontier: &'a [[u16; 3]],
}

fn surface_level_v1(surface_y_q8: i32, world_y: i64) -> WorldgenResult<u8> {
    let cell_base_q8 = world_y
        .checked_mul(256)
        .ok_or(WorldgenError::ArithmeticOverflow {
            operation: "static reservoir surface cell base",
        })?;
    let fill_q8 = i64::from(surface_y_q8)
        .checked_sub(cell_base_q8)
        .ok_or(WorldgenError::ArithmeticOverflow {
            operation: "static reservoir top fill",
        })?
        .clamp(1, 256);
    let eighths =
        u8::try_from((fill_q8 + 31) / 32).map_err(|_| WorldgenError::ArithmeticOverflow {
            operation: "static reservoir top eighths",
        })?;
    Ok(8_u8.saturating_sub(eighths).min(7))
}

fn static_flow(column: StaticWaterColumnV1) -> StaticReservoirFlowV1 {
    if column.waterfall {
        return StaticReservoirFlowV1::Down;
    }
    if column.kind != WaterBodyKindV1::River {
        return StaticReservoirFlowV1::Still;
    }
    let [x, z] = column.flow_tangent_q15;
    if x.unsigned_abs() >= z.unsigned_abs() {
        if x >= 0 {
            StaticReservoirFlowV1::East
        } else {
            StaticReservoirFlowV1::West
        }
    } else if z >= 0 {
        StaticReservoirFlowV1::South
    } else {
        StaticReservoirFlowV1::North
    }
}

fn check_limit(kind: &'static str, actual: usize, limit: usize) -> WorldgenResult<()> {
    if actual > limit {
        return Err(WorldgenError::CollectionLimitExceeded {
            kind,
            actual,
            limit,
        });
    }
    Ok(())
}

fn index_u32(index: usize) -> WorldgenResult<u32> {
    u32::try_from(index).map_err(|_| WorldgenError::ArithmeticOverflow {
        operation: "hydrologic topology row-major index",
    })
}

fn invalid<T>(field: &'static str, reason: impl Into<String>) -> WorldgenResult<T> {
    Err(WorldgenError::InvalidHydrologicDomain {
        field,
        reason: reason.into(),
    })
}

const fn water_body_kind_tag(kind: WaterBodyKindV1) -> u8 {
    match kind {
        WaterBodyKindV1::Ocean => 0,
        WaterBodyKindV1::Lake => 1,
        WaterBodyKindV1::River => 2,
        WaterBodyKindV1::Wetland => 3,
    }
}

#[cfg(test)]
mod tests {
    use std::{num::NonZeroU32, str::FromStr};

    use latticeaxiom_core::CanonicalHash;
    use latticeaxiom_storage::DimensionId;

    use super::*;
    use crate::{
        GenerationEpochIdV1, HydrologicDomainConfigV1, HydrologicDomainGridV1,
        HydrologicDomainInputV1, plan_hydrologic_domain_v1,
    };

    fn domain_from_dem(
        edge: u16,
        elevations: Vec<i32>,
        base_level_q8: i32,
    ) -> HydrologicDomainPlanV1 {
        let input = HydrologicDomainInputV1::new(
            DimensionId::from_str("latticeaxiom:dimension/terrenia")
                .expect("fixture dimension identity is valid"),
            GenerationEpochIdV1::from_hash(CanonicalHash::digest("topology-epoch")),
            0,
            0,
            None,
            CanonicalHash::digest("topology-provenance"),
            HydrologicDomainGridV1::new(
                0,
                0,
                NonZeroU32::MIN,
                NonZeroU16::new(edge).expect("fixture edge is nonzero"),
                NonZeroU16::new(edge).expect("fixture edge is nonzero"),
                0,
            ),
            base_level_q8,
            elevations,
            vec![65_536; usize::from(edge) * usize::from(edge)],
            Vec::new(),
            HydrologicDomainConfigV1::default(),
        )
        .expect("fixture domain input is valid");
        plan_hydrologic_domain_v1(&input).expect("fixture domain plans")
    }

    fn plane(edge: u16) -> Vec<i32> {
        (0..edge)
            .flat_map(|z| (0..edge).map(move |x| 4_096 - i32::from(x) * 64 - i32::from(z) * 32))
            .collect()
    }

    fn bowl(edge: u16) -> Vec<i32> {
        let center = i32::from(edge / 2);
        (0..edge)
            .flat_map(|z| {
                (0..edge).map(move |x| {
                    let radius = (i32::from(x) - center)
                        .abs()
                        .max((i32::from(z) - center).abs());
                    4_096 - (center - radius) * 256
                })
            })
            .collect()
    }

    fn channel_config() -> HydrologicTopologyConfigV1 {
        HydrologicTopologyConfigV1 {
            channel_threshold_q16: 4 * 65_536,
            medium_channel_threshold_q16: 16 * 65_536,
            large_channel_threshold_q16: 64 * 65_536,
            ..HydrologicTopologyConfigV1::default()
        }
    }

    #[test]
    fn plane_builds_connected_acyclic_channels_and_conserves_runoff() {
        let domain = domain_from_dem(17, plane(17), i32::MIN);
        let topology =
            build_hydrologic_topology_v1(&domain, &channel_config()).expect("topology builds");
        assert!(!topology.segments.is_empty());
        assert_eq!(
            topology.accounting.source_runoff_q16,
            topology.accounting.terminal_runoff_q16
        );
        let ids = topology
            .segments
            .iter()
            .map(|segment| segment.id)
            .collect::<BTreeSet<_>>();
        for segment in &topology.segments {
            assert!(segment.centerline[1].surface_y_q8 <= segment.centerline[0].surface_y_q8);
            if let Some(downstream) = segment.downstream {
                assert!(ids.contains(&downstream));
            }
            assert!(segment.strahler_order >= 1);
            if segment.length_q8 > 0 {
                assert!(segment.gradient_per_million > 0);
            }
        }
        assert!(
            topology
                .segments
                .iter()
                .any(|segment| segment.strahler_order > 1)
        );
    }

    #[test]
    fn retained_bowl_publishes_one_constant_level_lake() {
        let domain = domain_from_dem(9, bowl(9), i32::MIN);
        let topology =
            build_hydrologic_topology_v1(&domain, &channel_config()).expect("topology builds");
        let lakes = topology
            .water_bodies
            .iter()
            .filter(|body| body.kind == WaterBodyKindV1::Lake)
            .collect::<Vec<_>>();
        assert_eq!(lakes.len(), 1);
        let WaterSurfaceModelV1::Lake { level_q8 } = lakes[0].surface else {
            panic!("retained bowl must publish a lake surface");
        };
        for &cell in domain.depressions().records()[0].cell_indices() {
            let coordinate = domain
                .grid()
                .coordinate_of(cell as usize)
                .expect("depression cell is in bounds");
            let (x, z) = domain
                .grid()
                .world_coordinate(coordinate)
                .expect("fixture world coordinate fits");
            let sample = topology
                .static_water_column(&domain, x, z)
                .expect("lake query succeeds")
                .expect("lake cell contains static water");
            assert_eq!(sample.surface_y_q8, level_q8);
        }
    }

    #[test]
    fn river_sdf_and_profile_are_continuous_at_segment_joins() {
        let domain = domain_from_dem(17, plane(17), i32::MIN);
        let topology =
            build_hydrologic_topology_v1(&domain, &channel_config()).expect("topology builds");
        for segment in &topology.segments {
            let Some(downstream_id) = segment.downstream else {
                continue;
            };
            let downstream = topology
                .segments
                .iter()
                .find(|candidate| candidate.id == downstream_id)
                .expect("downstream segment exists");
            assert_eq!(
                segment.centerline[1].surface_y_q8,
                downstream.centerline[0].surface_y_q8
            );
            let join = segment.centerline[1];
            let sample = topology
                .river_sdf_sample(&domain, join.world_x, join.world_z)
                .expect("SDF query succeeds")
                .expect("channel corpus has a closest segment");
            assert!(sample.channel_signed_distance_q8 <= 0);
        }
        for index in 0..domain.grid().sample_count() {
            let coordinate = domain
                .grid()
                .coordinate_of(index)
                .expect("fixture index is in bounds");
            let (world_x, world_z) = domain
                .grid()
                .world_coordinate(coordinate)
                .expect("fixture world coordinate fits");
            let global = topology
                .river_sdf_sample_unchecked(world_x, world_z)
                .expect("global SDF query succeeds");
            if global.is_some_and(|sample| sample.channel_signed_distance_q8 <= 0) {
                assert_eq!(
                    topology
                        .river_sdf_sample_nearby_unchecked(&domain, world_x, world_z)
                        .expect("indexed SDF query succeeds"),
                    global
                );
            }
        }
    }

    #[test]
    fn static_materialization_has_levels_flow_and_no_runtime_frontier() {
        let domain = domain_from_dem(17, plane(17), i32::MIN);
        let config = channel_config();
        let topology = build_hydrologic_topology_v1(&domain, &config).expect("topology builds");
        let water = StableId::from_str("latticeaxiom:fluid/water")
            .expect("fixture water identity is valid");
        let candidate = materialize_static_reservoir_chunk_v1(
            &domain,
            &topology,
            &config,
            ChunkCoordinate::new(0, 0, 0),
            NonZeroU16::new(16).expect("fixture chunk edge is nonzero"),
            0,
            31,
            &water,
        )
        .expect("static reservoir materializes");
        assert!(!candidate.cells.is_empty());
        assert!(candidate.runtime_frontier.is_empty());
        assert!(candidate.cells.iter().all(|cell| cell.level <= 7));
        assert!(
            candidate
                .cells
                .iter()
                .any(|cell| cell.flow != StaticReservoirFlowV1::Still)
        );
        assert_eq!(
            candidate.canonical_bytes().ok(),
            materialize_static_reservoir_chunk_v1(
                &domain,
                &topology,
                &config,
                ChunkCoordinate::new(0, 0, 0),
                NonZeroU16::new(16).expect("fixture chunk edge is nonzero"),
                0,
                31,
                &StableId::from_str("latticeaxiom:fluid/water")
                    .expect("fixture water identity is valid"),
            )
            .and_then(|value| value.canonical_bytes())
            .ok()
        );
    }

    #[test]
    fn topology_query_order_and_canonical_hash_are_stable() {
        let domain = domain_from_dem(17, plane(17), i32::MIN);
        let topology =
            build_hydrologic_topology_v1(&domain, &channel_config()).expect("topology builds");
        let before = topology.canonical_bytes().expect("topology canonicalizes");
        for z in (0..17_i64).rev() {
            for x in (0..17_i64).rev() {
                let _ = topology
                    .static_water_column(&domain, x, z)
                    .expect("query succeeds");
            }
        }
        assert_eq!(
            before,
            topology.canonical_bytes().expect("topology canonicalizes")
        );
        assert_eq!(
            topology
                .canonical_hash()
                .expect("topology hashes")
                .to_string(),
            "5bcc123eefa54f6c43b460f617531a278704563f16ba8938c40cf9e860740d1e"
        );
    }

    #[test]
    fn topology_limits_reject_boundary_plus_one() {
        let domain = domain_from_dem(17, plane(17), i32::MIN);
        let config = HydrologicTopologyConfigV1 {
            max_segments: 1,
            ..channel_config()
        };
        assert!(matches!(
            build_hydrologic_topology_v1(&domain, &config),
            Err(WorldgenError::CollectionLimitExceeded {
                kind: "topology river segments",
                ..
            })
        ));

        let spatial_config = HydrologicTopologyConfigV1 {
            max_spatial_references: 1,
            ..channel_config()
        };
        assert!(matches!(
            build_hydrologic_topology_v1(&domain, &spatial_config),
            Err(WorldgenError::CollectionLimitExceeded {
                kind: "topology spatial references",
                ..
            })
        ));

        let topology = build_hydrologic_topology_v1(&domain, &channel_config())
            .expect("topology builds for the materialization-limit fixture");
        let water = StableId::from_str("latticeaxiom:fluid/water")
            .expect("fixture water identity is valid");
        assert!(matches!(
            materialize_static_reservoir_chunk_v1(
                &domain,
                &topology,
                &channel_config(),
                ChunkCoordinate::new(0, 0, 0),
                NonZeroU16::new(MAX_STATIC_RESERVOIR_CHUNK_EDGE + 1)
                    .expect("boundary-plus-one edge remains nonzero"),
                0,
                31,
                &water,
            ),
            Err(WorldgenError::BudgetExceeded {
                budget: "static reservoir chunk edge",
                ..
            })
        ));
    }
}
