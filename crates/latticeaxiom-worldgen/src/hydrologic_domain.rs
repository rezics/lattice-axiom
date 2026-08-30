//! Bounded, deterministic hydrologic-domain planning.
//!
//! The authoritative path uses integer elevations, runoff, routing weights,
//! and accumulation. Floating point appears only in the explicitly named
//! development comparator and never enters a cache key or persisted plan.

use std::{
    cmp::{Ordering, Reverse},
    collections::{BTreeMap, BTreeSet, BinaryHeap, VecDeque},
    mem::size_of,
    num::{NonZeroU16, NonZeroU32},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering as AtomicOrdering},
    },
};

use bevy::tasks::TaskPool;
use latticeaxiom_core::{CanonicalHash, canonical_json_bytes};
use latticeaxiom_storage::DimensionId;
use serde::{Deserialize, Serialize};

use crate::{
    GenerationEpochIdV1, HydrologicBoundarySignatureV1, HydrologicDomainConfigHashV1,
    HydrologicDomainIdV1, HydrologicDomainInputHashV1, HydrologicDomainPlanHashV1,
    HydrologicPortIdV1, WorldgenError, WorldgenResult, hashes::domain_hash,
};

/// Denominator shared by every authoritative MFD receiver weight.
pub const HYDROLOGIC_WEIGHT_SCALE_V1: u32 = 1_000_000;

const CARDINAL_SLOPE_SCALE: u64 = 1_024;
const DIAGONAL_SLOPE_SCALE: u64 = 724;
const CANCELLATION_INTERVAL: u64 = 1_024;
const ALGORITHM_REVISION: &[u8] = b"latticeaxiom:hydrologic-domain/priority-flood-mfd-p1@1";

/// Frozen hydrologic-domain algorithm identity.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum HydrologicDomainAlgorithmV1 {
    /// Stable Priority-Flood correction with Freeman MFD using `p = 1`.
    PriorityFloodMfdP1Revision1,
}

impl HydrologicDomainAlgorithmV1 {
    /// Returns the stable identity included in plan provenance and cache keys.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::PriorityFloodMfdP1Revision1 => {
                "latticeaxiom:hydrologic-domain/priority-flood-mfd-p1@1"
            }
        }
    }
}

/// Integer coordinate in a domain raster, ordered `(x, z)`.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HydrologicGridCoordinateV1 {
    x: u16,
    z: u16,
}

impl HydrologicGridCoordinateV1 {
    /// Creates a grid coordinate. Bounds are validated by its containing grid.
    #[must_use]
    pub const fn new(x: u16, z: u16) -> Self {
        Self { x, z }
    }

    /// Returns the X sample index.
    #[must_use]
    pub const fn x(self) -> u16 {
        self.x
    }

    /// Returns the Z sample index.
    #[must_use]
    pub const fn z(self) -> u16 {
        self.z
    }
}

/// Bounded world-space raster geometry for one hydrologic domain.
#[derive(Clone, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HydrologicDomainGridV1 {
    origin_x: i64,
    origin_z: i64,
    spacing_voxels: NonZeroU32,
    width: NonZeroU16,
    height: NonZeroU16,
    halo_samples: u16,
}

impl HydrologicDomainGridV1 {
    /// Creates a finite domain grid.
    #[must_use]
    pub const fn new(
        origin_x: i64,
        origin_z: i64,
        spacing_voxels: NonZeroU32,
        width: NonZeroU16,
        height: NonZeroU16,
        halo_samples: u16,
    ) -> Self {
        Self {
            origin_x,
            origin_z,
            spacing_voxels,
            width,
            height,
            halo_samples,
        }
    }

    /// Returns the world X coordinate of sample zero.
    #[must_use]
    pub const fn origin_x(&self) -> i64 {
        self.origin_x
    }

    /// Returns the world Z coordinate of sample zero.
    #[must_use]
    pub const fn origin_z(&self) -> i64 {
        self.origin_z
    }

    /// Returns the fixed sample spacing in voxels.
    #[must_use]
    pub const fn spacing_voxels(&self) -> NonZeroU32 {
        self.spacing_voxels
    }

    /// Returns the sample width.
    #[must_use]
    pub const fn width(&self) -> NonZeroU16 {
        self.width
    }

    /// Returns the sample height.
    #[must_use]
    pub const fn height(&self) -> NonZeroU16 {
        self.height
    }

    /// Returns the halo width retained around the domain core.
    #[must_use]
    pub const fn halo_samples(&self) -> u16 {
        self.halo_samples
    }

    /// Returns the exact number of raster samples.
    #[must_use]
    pub const fn sample_count(&self) -> usize {
        self.width.get() as usize * self.height.get() as usize
    }

    fn index(&self, coordinate: HydrologicGridCoordinateV1) -> Option<usize> {
        if coordinate.x >= self.width.get() || coordinate.z >= self.height.get() {
            return None;
        }
        Some(usize::from(coordinate.x) + usize::from(coordinate.z) * usize::from(self.width.get()))
    }

    fn coordinate(&self, index: usize) -> HydrologicGridCoordinateV1 {
        let width = usize::from(self.width.get());
        HydrologicGridCoordinateV1::new(
            u16::try_from(index % width).unwrap_or(u16::MAX),
            u16::try_from(index / width).unwrap_or(u16::MAX),
        )
    }
}

/// Canonical edge of a finite hydrologic domain.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum HydrologicBoundaryEdgeV1 {
    /// Minimum Z edge.
    North,
    /// Maximum X edge.
    East,
    /// Maximum Z edge.
    South,
    /// Minimum X edge.
    West,
}

/// Semantic direction of a parent/domain boundary port.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum HydrologicPortKindV1 {
    /// Runoff supplied by a parent or adjacent domain.
    Inflow,
    /// Required continuation into an adjacent domain.
    Outflow,
    /// Terminal connection to the dimension base level.
    OceanOutlet,
}

/// Direction-independent parent/domain boundary constraint.
#[derive(Clone, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HydrologicBoundaryPortV1 {
    id: HydrologicPortIdV1,
    signature: HydrologicBoundarySignatureV1,
    edge: HydrologicBoundaryEdgeV1,
    offset: u16,
    kind: HydrologicPortKindV1,
    level_q8: i32,
    discharge_q16: u64,
}

impl HydrologicBoundaryPortV1 {
    /// Constructs a port shared by two canonical domain identities.
    ///
    /// Domain order does not affect the returned ID or signature. Each side is
    /// allowed to encode its local edge independently; shared world position
    /// and semantic values define the direction-independent receipt.
    #[allow(
        clippy::too_many_arguments,
        reason = "the constructor closes one complete boundary receipt"
    )]
    #[must_use]
    pub fn between(
        domain_a: HydrologicDomainIdV1,
        domain_b: HydrologicDomainIdV1,
        edge: HydrologicBoundaryEdgeV1,
        offset: u16,
        world_x: i64,
        world_z: i64,
        kind: HydrologicPortKindV1,
        level_q8: i32,
        discharge_q16: u64,
    ) -> Self {
        let (first, second) = if domain_a <= domain_b {
            (domain_a, domain_b)
        } else {
            (domain_b, domain_a)
        };
        let x = world_x.to_be_bytes();
        let z = world_z.to_be_bytes();
        let level = level_q8.to_be_bytes();
        let discharge = discharge_q16.to_be_bytes();
        let id = HydrologicPortIdV1::from_hash(domain_hash(
            b"latticeaxiom.hydrologic-port.id.v1\0",
            &[first.as_bytes(), second.as_bytes(), &x, &z],
        ));
        let signature = HydrologicBoundarySignatureV1::from_hash(domain_hash(
            b"latticeaxiom.hydrologic-port.signature.v1\0",
            &[
                first.as_bytes(),
                second.as_bytes(),
                &x,
                &z,
                &[port_kind_tag(kind)],
                &level,
                &discharge,
            ],
        ));
        Self {
            id,
            signature,
            edge,
            offset,
            kind,
            level_q8,
            discharge_q16,
        }
    }

    /// Returns the stable port identity.
    #[must_use]
    pub const fn id(&self) -> HydrologicPortIdV1 {
        self.id
    }

    /// Returns the direction-independent boundary signature.
    #[must_use]
    pub const fn signature(&self) -> HydrologicBoundarySignatureV1 {
        self.signature
    }

    /// Returns the local domain edge.
    #[must_use]
    pub const fn edge(&self) -> HydrologicBoundaryEdgeV1 {
        self.edge
    }

    /// Returns the local sample offset along the edge.
    #[must_use]
    pub const fn offset(&self) -> u16 {
        self.offset
    }

    /// Returns the port semantics.
    #[must_use]
    pub const fn kind(&self) -> HydrologicPortKindV1 {
        self.kind
    }

    /// Returns the Q24.8 boundary water or base level.
    #[must_use]
    pub const fn level_q8(&self) -> i32 {
        self.level_q8
    }

    /// Returns Q48.16 incoming or expected discharge.
    #[must_use]
    pub const fn discharge_q16(&self) -> u64 {
        self.discharge_q16
    }
}

/// Closed depression classification policy result.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum DepressionClassV1 {
    /// Permanent level water body at its declared spill elevation.
    Lake,
    /// Internally draining permanent or seasonal terminal basin.
    Endorheic,
    /// Shallow saturated terrain rather than open deep water.
    Wetland,
    /// Deterministically cut through an allowed saddle.
    Breach,
    /// Small artifact pit raised to its spill surface.
    Fill,
}

/// Hard limits and closed policy for one domain plan.
#[derive(Clone, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HydrologicDomainConfigV1 {
    max_grid_points: u32,
    max_ports: u16,
    max_graph_edges: u32,
    max_depressions: u32,
    max_queue_entries: u32,
    max_work_units: u64,
    max_temporary_bytes: u64,
    max_result_bytes: u64,
    min_lake_area_cells: u32,
    min_lake_depth_q8: u32,
    min_endorheic_volume_q8: u64,
    max_wetland_depth_q8: u32,
    max_fill_depth_q8: u32,
    allow_endorheic: bool,
    allow_breach: bool,
}

impl Default for HydrologicDomainConfigV1 {
    fn default() -> Self {
        Self {
            max_grid_points: 1_048_576,
            max_ports: 256,
            max_graph_edges: 8_388_608,
            max_depressions: 262_144,
            max_queue_entries: 1_048_576,
            max_work_units: 100_000_000,
            max_temporary_bytes: 512 * 1024 * 1024,
            max_result_bytes: 512 * 1024 * 1024,
            min_lake_area_cells: 16,
            min_lake_depth_q8: 2 * 256,
            min_endorheic_volume_q8: 16_384,
            max_wetland_depth_q8: 128,
            max_fill_depth_q8: 64,
            allow_endorheic: true,
            allow_breach: true,
        }
    }
}

impl HydrologicDomainConfigV1 {
    /// Creates a closed config from hard resource limits and depression policy.
    #[allow(
        clippy::too_many_arguments,
        reason = "the constructor closes every persisted policy field"
    )]
    #[must_use]
    pub const fn new(
        max_grid_points: u32,
        max_ports: u16,
        max_graph_edges: u32,
        max_depressions: u32,
        max_queue_entries: u32,
        max_work_units: u64,
        max_temporary_bytes: u64,
        max_result_bytes: u64,
        min_lake_area_cells: u32,
        min_lake_depth_q8: u32,
        min_endorheic_volume_q8: u64,
        max_wetland_depth_q8: u32,
        max_fill_depth_q8: u32,
        allow_endorheic: bool,
        allow_breach: bool,
    ) -> Self {
        Self {
            max_grid_points,
            max_ports,
            max_graph_edges,
            max_depressions,
            max_queue_entries,
            max_work_units,
            max_temporary_bytes,
            max_result_bytes,
            min_lake_area_cells,
            min_lake_depth_q8,
            min_endorheic_volume_q8,
            max_wetland_depth_q8,
            max_fill_depth_q8,
            allow_endorheic,
            allow_breach,
        }
    }

    /// Computes the canonical config hash used by cache keys.
    ///
    /// # Errors
    ///
    /// Returns a canonical-encoding error if serialization fails.
    pub fn canonical_hash(&self) -> WorldgenResult<HydrologicDomainConfigHashV1> {
        let bytes =
            canonical_json_bytes(self).map_err(|error| WorldgenError::CanonicalEncoding {
                kind: "HydrologicDomainConfigV1",
                reason: error.to_string(),
            })?;
        Ok(HydrologicDomainConfigHashV1::from_hash(domain_hash(
            b"latticeaxiom.hydrologic-domain.config.v1\0",
            &[&bytes],
        )))
    }
}

/// Complete bounded input for one finite domain solve.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HydrologicDomainInputV1 {
    dimension: DimensionId,
    generation_epoch: GenerationEpochIdV1,
    domain_x: i64,
    domain_z: i64,
    parent_domain: Option<HydrologicDomainIdV1>,
    algorithm: HydrologicDomainAlgorithmV1,
    provenance: CanonicalHash,
    grid: HydrologicDomainGridV1,
    base_level_q8: i32,
    initial_elevation_q8: Vec<i32>,
    effective_runoff_q16: Vec<u32>,
    ports: Vec<HydrologicBoundaryPortV1>,
    config: HydrologicDomainConfigV1,
}

impl HydrologicDomainInputV1 {
    /// Constructs and validates a canonical domain input.
    ///
    /// Ports are sorted by stable identity before storage so registration
    /// order cannot influence input bytes.
    ///
    /// # Errors
    ///
    /// Returns a hydrologic-domain validation error when raster sizes, port
    /// coordinates, world bounds, or hard limits are invalid.
    #[allow(
        clippy::too_many_arguments,
        reason = "the constructor closes the complete domain input"
    )]
    pub fn new(
        dimension: DimensionId,
        generation_epoch: GenerationEpochIdV1,
        domain_x: i64,
        domain_z: i64,
        parent_domain: Option<HydrologicDomainIdV1>,
        provenance: CanonicalHash,
        grid: HydrologicDomainGridV1,
        base_level_q8: i32,
        initial_elevation_q8: Vec<i32>,
        effective_runoff_q16: Vec<u32>,
        mut ports: Vec<HydrologicBoundaryPortV1>,
        config: HydrologicDomainConfigV1,
    ) -> WorldgenResult<Self> {
        ports.sort_by_key(HydrologicBoundaryPortV1::id);
        let input = Self {
            dimension,
            generation_epoch,
            domain_x,
            domain_z,
            parent_domain,
            algorithm: HydrologicDomainAlgorithmV1::PriorityFloodMfdP1Revision1,
            provenance,
            grid,
            base_level_q8,
            initial_elevation_q8,
            effective_runoff_q16,
            ports,
            config,
        };
        input.validate()?;
        Ok(input)
    }

    /// Returns the stable domain identity derived by bounded coordinate lookup.
    #[must_use]
    pub fn domain_id(&self) -> HydrologicDomainIdV1 {
        let x = self.domain_x.to_be_bytes();
        let z = self.domain_z.to_be_bytes();
        HydrologicDomainIdV1::from_hash(domain_hash(
            b"latticeaxiom.hydrologic-domain.id.v1\0",
            &[
                self.dimension.as_str().as_bytes(),
                self.generation_epoch.as_bytes(),
                ALGORITHM_REVISION,
                &x,
                &z,
            ],
        ))
    }

    /// Returns the dimension identity.
    #[must_use]
    pub const fn dimension(&self) -> &DimensionId {
        &self.dimension
    }

    /// Returns the frozen generation epoch.
    #[must_use]
    pub const fn generation_epoch(&self) -> GenerationEpochIdV1 {
        self.generation_epoch
    }

    /// Returns the domain raster geometry.
    #[must_use]
    pub const fn grid(&self) -> &HydrologicDomainGridV1 {
        &self.grid
    }

    /// Returns original Q24.8 DEM samples in canonical row-major order.
    #[must_use]
    pub fn initial_elevation_q8(&self) -> &[i32] {
        &self.initial_elevation_q8
    }

    /// Returns Q16 effective runoff samples in canonical row-major order.
    #[must_use]
    pub fn effective_runoff_q16(&self) -> &[u32] {
        &self.effective_runoff_q16
    }

    /// Returns canonically ordered boundary ports.
    #[must_use]
    pub fn ports(&self) -> &[HydrologicBoundaryPortV1] {
        &self.ports
    }

    /// Returns the closed domain configuration.
    #[must_use]
    pub const fn config(&self) -> &HydrologicDomainConfigV1 {
        &self.config
    }

    /// Returns canonical input bytes.
    ///
    /// # Errors
    ///
    /// Returns a canonical-encoding error if serialization fails.
    pub fn canonical_bytes(&self) -> WorldgenResult<Vec<u8>> {
        canonical_json_bytes(self).map_err(|error| WorldgenError::CanonicalEncoding {
            kind: "HydrologicDomainInputV1",
            reason: error.to_string(),
        })
    }

    /// Returns the canonical output-affecting input hash.
    ///
    /// # Errors
    ///
    /// Returns a canonical-encoding error if serialization fails.
    pub fn input_hash(&self) -> WorldgenResult<HydrologicDomainInputHashV1> {
        let bytes = self.canonical_bytes()?;
        Ok(HydrologicDomainInputHashV1::from_hash(domain_hash(
            b"latticeaxiom.hydrologic-domain.input.v1\0",
            &[&bytes],
        )))
    }

    /// Returns the exact cache key for this input.
    ///
    /// # Errors
    ///
    /// Returns a canonical-encoding error if input or config serialization
    /// fails.
    pub fn cache_key(&self) -> WorldgenResult<HydrologicDomainCacheKeyV1> {
        Ok(HydrologicDomainCacheKeyV1 {
            dimension: self.dimension.clone(),
            domain_id: self.domain_id(),
            generation_epoch: self.generation_epoch,
            input_hash: self.input_hash()?,
            algorithm: self.algorithm,
            config_hash: self.config.canonical_hash()?,
        })
    }

    fn validate(&self) -> WorldgenResult<()> {
        validate_config(&self.config)?;
        let samples = self.grid.sample_count();
        if samples < 4 || self.grid.width.get() < 2 || self.grid.height.get() < 2 {
            return invalid("grid", "width and height must each be at least two samples");
        }
        check_count(
            "hydrologic grid points",
            samples,
            self.config.max_grid_points as usize,
        )?;
        if self.initial_elevation_q8.len() != samples {
            return invalid(
                "initial_elevation_q8",
                "sample count must equal grid width times height",
            );
        }
        if self.effective_runoff_q16.len() != samples {
            return invalid(
                "effective_runoff_q16",
                "sample count must equal grid width times height",
            );
        }
        check_count(
            "hydrologic ports",
            self.ports.len(),
            usize::from(self.config.max_ports),
        )?;
        if self.ports.windows(2).any(|pair| pair[0].id == pair[1].id) {
            return invalid("ports", "port identities must be unique");
        }
        for port in &self.ports {
            let edge_length = match port.edge {
                HydrologicBoundaryEdgeV1::North | HydrologicBoundaryEdgeV1::South => {
                    self.grid.width.get()
                }
                HydrologicBoundaryEdgeV1::East | HydrologicBoundaryEdgeV1::West => {
                    self.grid.height.get()
                }
            };
            if port.offset >= edge_length {
                return invalid("ports.offset", "port offset lies outside its declared edge");
            }
        }
        let width_span = i64::from(self.grid.width.get() - 1)
            .checked_mul(i64::from(self.grid.spacing_voxels.get()))
            .and_then(|span| self.grid.origin_x.checked_add(span));
        let height_span = i64::from(self.grid.height.get() - 1)
            .checked_mul(i64::from(self.grid.spacing_voxels.get()))
            .and_then(|span| self.grid.origin_z.checked_add(span));
        if width_span.is_none() || height_span.is_none() {
            return Err(WorldgenError::ArithmeticOverflow {
                operation: "hydrologic domain world bounds",
            });
        }
        Ok(())
    }
}

/// Exact key for a bounded domain-plan cache entry.
#[derive(Clone, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HydrologicDomainCacheKeyV1 {
    dimension: DimensionId,
    domain_id: HydrologicDomainIdV1,
    generation_epoch: GenerationEpochIdV1,
    input_hash: HydrologicDomainInputHashV1,
    algorithm: HydrologicDomainAlgorithmV1,
    config_hash: HydrologicDomainConfigHashV1,
}

impl HydrologicDomainCacheKeyV1 {
    /// Returns the finite domain identity.
    #[must_use]
    pub const fn domain_id(&self) -> HydrologicDomainIdV1 {
        self.domain_id
    }

    /// Returns the exact output-affecting input hash.
    #[must_use]
    pub const fn input_hash(&self) -> HydrologicDomainInputHashV1 {
        self.input_hash
    }
}

/// One retained or corrected depression in the original DEM.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DepressionRecordV1 {
    id: CanonicalHash,
    parent: Option<CanonicalHash>,
    pit: HydrologicGridCoordinateV1,
    spill: HydrologicGridCoordinateV1,
    outlet: HydrologicGridCoordinateV1,
    class: DepressionClassV1,
    cell_indices: Vec<u32>,
    pit_elevation_q8: i32,
    spill_elevation_q8: i32,
    volume_q8: u64,
}

impl DepressionRecordV1 {
    /// Returns the stable depression ID.
    #[must_use]
    pub const fn id(&self) -> &CanonicalHash {
        &self.id
    }

    /// Returns the optional containing depression ID.
    #[must_use]
    pub const fn parent(&self) -> Option<&CanonicalHash> {
        self.parent.as_ref()
    }

    /// Returns the deterministic depression classification.
    #[must_use]
    pub const fn class(&self) -> DepressionClassV1 {
        self.class
    }

    /// Returns the lowest original DEM sample.
    #[must_use]
    pub const fn pit(&self) -> HydrologicGridCoordinateV1 {
        self.pit
    }

    /// Returns the spill saddle sample.
    #[must_use]
    pub const fn spill(&self) -> HydrologicGridCoordinateV1 {
        self.spill
    }

    /// Returns sorted row-major member indices.
    #[must_use]
    pub fn cell_indices(&self) -> &[u32] {
        &self.cell_indices
    }

    /// Returns retained or filled volume in elevation-Q8 cell units.
    #[must_use]
    pub const fn volume_q8(&self) -> u64 {
        self.volume_q8
    }
}

/// Canonically ordered depression forest derived before correction policy.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DepressionHierarchyV1 {
    records: Vec<DepressionRecordV1>,
}

impl DepressionHierarchyV1 {
    /// Returns records ordered by stable depression ID.
    #[must_use]
    pub fn records(&self) -> &[DepressionRecordV1] {
        &self.records
    }
}

/// One exact fixed-point MFD receiver.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct MfdReceiverV1 {
    receiver_index: u32,
    weight_per_million: u32,
    slope_numerator: u64,
}

impl MfdReceiverV1 {
    /// Returns the row-major receiver index.
    #[must_use]
    pub const fn receiver_index(self) -> u32 {
        self.receiver_index
    }

    /// Returns the exact weight over [`HYDROLOGIC_WEIGHT_SCALE_V1`].
    #[must_use]
    pub const fn weight_per_million(self) -> u32 {
        self.weight_per_million
    }

    /// Returns the fixed slope numerator used before normalization.
    #[must_use]
    pub const fn slope_numerator(self) -> u64 {
        self.slope_numerator
    }
}

/// MFD routing and conservative accumulated runoff for one raster cell.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct MfdRoutingCellV1 {
    receivers: Vec<MfdReceiverV1>,
    accumulated_runoff_q16: u64,
    retained_terminal: bool,
    boundary_terminal: bool,
}

impl MfdRoutingCellV1 {
    /// Returns receivers in canonical direction/index order.
    #[must_use]
    pub fn receivers(&self) -> &[MfdReceiverV1] {
        &self.receivers
    }

    /// Returns exactly accumulated Q16 runoff at this cell.
    #[must_use]
    pub const fn accumulated_runoff_q16(&self) -> u64 {
        self.accumulated_runoff_q16
    }

    /// Returns whether this cell terminates in a retained depression.
    #[must_use]
    pub const fn retained_terminal(&self) -> bool {
        self.retained_terminal
    }

    /// Returns whether this cell terminates at a declared/generated boundary outlet.
    #[must_use]
    pub const fn boundary_terminal(&self) -> bool {
        self.boundary_terminal
    }
}

/// Deterministic work and conservation diagnostics for one solve.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HydrologicDomainAccountingV1 {
    examined_cells: u64,
    work_units: u64,
    priority_queue_peak: u32,
    graph_edges: u32,
    depression_count: u32,
    temporary_bytes_peak: u64,
    result_bytes: u64,
    input_runoff_q16: u64,
    terminal_runoff_q16: u64,
}

impl HydrologicDomainAccountingV1 {
    /// Returns the exact deterministic work-unit count.
    #[must_use]
    pub const fn work_units(self) -> u64 {
        self.work_units
    }

    /// Returns the observed priority queue high-water mark.
    #[must_use]
    pub const fn priority_queue_peak(self) -> u32 {
        self.priority_queue_peak
    }

    /// Returns the final receiver edge count.
    #[must_use]
    pub const fn graph_edges(self) -> u32 {
        self.graph_edges
    }

    /// Returns peak estimated owned temporary bytes.
    #[must_use]
    pub const fn temporary_bytes_peak(self) -> u64 {
        self.temporary_bytes_peak
    }

    /// Returns canonical result bytes.
    #[must_use]
    pub const fn result_bytes(self) -> u64 {
        self.result_bytes
    }

    /// Returns summed local plus boundary-input runoff.
    #[must_use]
    pub const fn input_runoff_q16(self) -> u64 {
        self.input_runoff_q16
    }

    /// Returns runoff terminating at retained basins or boundary outlets.
    #[must_use]
    pub const fn terminal_runoff_q16(self) -> u64 {
        self.terminal_runoff_q16
    }
}

/// Immutable result of one finite hydrologic-domain solve.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HydrologicDomainPlanV1 {
    dimension: DimensionId,
    domain_id: HydrologicDomainIdV1,
    parent_domain: Option<HydrologicDomainIdV1>,
    generation_epoch: GenerationEpochIdV1,
    input_hash: HydrologicDomainInputHashV1,
    config_hash: HydrologicDomainConfigHashV1,
    algorithm: HydrologicDomainAlgorithmV1,
    provenance: CanonicalHash,
    grid: HydrologicDomainGridV1,
    base_level_q8: i32,
    initial_elevation_q8: Vec<i32>,
    routing_elevation_q8: Vec<i32>,
    flood_predecessor: Vec<Option<u32>>,
    flood_rank: Vec<u32>,
    depressions: DepressionHierarchyV1,
    routing: Vec<MfdRoutingCellV1>,
    ports: Vec<HydrologicBoundaryPortV1>,
    boundary_outlets: Vec<HydrologicGridCoordinateV1>,
    accounting: HydrologicDomainAccountingV1,
}

impl HydrologicDomainPlanV1 {
    /// Returns the dimension identity.
    #[must_use]
    pub const fn dimension(&self) -> &DimensionId {
        &self.dimension
    }

    /// Returns this plan's stable domain identity.
    #[must_use]
    pub const fn domain_id(&self) -> HydrologicDomainIdV1 {
        self.domain_id
    }

    /// Returns the canonical input hash.
    #[must_use]
    pub const fn input_hash(&self) -> HydrologicDomainInputHashV1 {
        self.input_hash
    }

    /// Returns the exact closed-config hash.
    #[must_use]
    pub const fn config_hash(&self) -> HydrologicDomainConfigHashV1 {
        self.config_hash
    }

    /// Returns the finite grid geometry.
    #[must_use]
    pub const fn grid(&self) -> &HydrologicDomainGridV1 {
        &self.grid
    }

    /// Returns the untouched initial Q24.8 DEM.
    #[must_use]
    pub fn initial_elevation_q8(&self) -> &[i32] {
        &self.initial_elevation_q8
    }

    /// Returns the depression-corrected routing surface.
    #[must_use]
    pub fn routing_elevation_q8(&self) -> &[i32] {
        &self.routing_elevation_q8
    }

    /// Returns the original-DEM depression hierarchy.
    #[must_use]
    pub const fn depressions(&self) -> &DepressionHierarchyV1 {
        &self.depressions
    }

    /// Returns row-major exact MFD routing records.
    #[must_use]
    pub fn routing(&self) -> &[MfdRoutingCellV1] {
        &self.routing
    }

    /// Returns declared and generated boundary outlet coordinates.
    #[must_use]
    pub fn boundary_outlets(&self) -> &[HydrologicGridCoordinateV1] {
        &self.boundary_outlets
    }

    /// Returns deterministic work and conservation accounting.
    #[must_use]
    pub const fn accounting(&self) -> HydrologicDomainAccountingV1 {
        self.accounting
    }

    /// Returns canonical persisted plan bytes.
    ///
    /// # Errors
    ///
    /// Returns a canonical-encoding error if serialization fails.
    pub fn canonical_bytes(&self) -> WorldgenResult<Vec<u8>> {
        canonical_json_bytes(self).map_err(|error| WorldgenError::CanonicalEncoding {
            kind: "HydrologicDomainPlanV1",
            reason: error.to_string(),
        })
    }

    /// Returns the canonical plan hash.
    ///
    /// # Errors
    ///
    /// Returns a canonical-encoding error if serialization fails.
    pub fn canonical_hash(&self) -> WorldgenResult<HydrologicDomainPlanHashV1> {
        let bytes = self.canonical_bytes()?;
        Ok(HydrologicDomainPlanHashV1::from_hash(domain_hash(
            b"latticeaxiom.hydrologic-domain.plan.v1\0",
            &[&bytes],
        )))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct FloodNode {
    elevation_q8: i32,
    index: usize,
}

impl Ord for FloodNode {
    fn cmp(&self, other: &Self) -> Ordering {
        (self.elevation_q8, self.index).cmp(&(other.elevation_q8, other.index))
    }
}

impl PartialOrd for FloodNode {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

#[derive(Debug)]
struct PlanningState<'a> {
    config: &'a HydrologicDomainConfigV1,
    cancellation: Option<&'a AtomicBool>,
    accounting: HydrologicDomainAccountingV1,
}

impl<'a> PlanningState<'a> {
    fn new(
        config: &'a HydrologicDomainConfigV1,
        cancellation: Option<&'a AtomicBool>,
        sample_count: usize,
    ) -> WorldgenResult<Self> {
        if cancellation.is_some_and(|flag| flag.load(AtomicOrdering::Relaxed)) {
            return Err(WorldgenError::HydrologicPlanningCancelled {
                completed_work_units: 0,
            });
        }
        let per_sample = size_of::<i32>() * 2
            + size_of::<Option<u32>>()
            + size_of::<u32>()
            + size_of::<bool>() * 3
            + size_of::<MfdRoutingCellV1>();
        let estimated = sample_count
            .checked_mul(per_sample)
            .and_then(|bytes| u64::try_from(bytes).ok())
            .ok_or(WorldgenError::ArithmeticOverflow {
                operation: "hydrologic temporary byte preflight",
            })?;
        if estimated > config.max_temporary_bytes {
            return Err(WorldgenError::BudgetExceeded {
                budget: "hydrologic temporary bytes",
                required: estimated,
                limit: config.max_temporary_bytes,
            });
        }
        Ok(Self {
            config,
            cancellation,
            accounting: HydrologicDomainAccountingV1 {
                temporary_bytes_peak: estimated,
                ..HydrologicDomainAccountingV1::default()
            },
        })
    }

    fn work(&mut self, units: u64) -> WorldgenResult<()> {
        self.accounting.work_units = self.accounting.work_units.checked_add(units).ok_or(
            WorldgenError::ArithmeticOverflow {
                operation: "hydrologic work accounting",
            },
        )?;
        if self.accounting.work_units > self.config.max_work_units {
            return Err(WorldgenError::BudgetExceeded {
                budget: "hydrologic work units",
                required: self.accounting.work_units,
                limit: self.config.max_work_units,
            });
        }
        if self.accounting.work_units % CANCELLATION_INTERVAL < units
            && self
                .cancellation
                .is_some_and(|flag| flag.load(AtomicOrdering::Relaxed))
        {
            return Err(WorldgenError::HydrologicPlanningCancelled {
                completed_work_units: self.accounting.work_units,
            });
        }
        Ok(())
    }

    fn examine(&mut self) -> WorldgenResult<()> {
        self.accounting.examined_cells = self.accounting.examined_cells.checked_add(1).ok_or(
            WorldgenError::ArithmeticOverflow {
                operation: "hydrologic examined-cell accounting",
            },
        )?;
        self.work(1)
    }

    fn observe_queue(&mut self, length: usize) -> WorldgenResult<()> {
        check_count(
            "hydrologic priority queue",
            length,
            self.config.max_queue_entries as usize,
        )?;
        let length = u32::try_from(length).map_err(|_| WorldgenError::ArithmeticOverflow {
            operation: "hydrologic queue accounting",
        })?;
        self.accounting.priority_queue_peak = self.accounting.priority_queue_peak.max(length);
        Ok(())
    }
}

#[derive(Debug)]
struct FloodResult {
    elevation_q8: Vec<i32>,
    predecessor: Vec<Option<u32>>,
    rank: Vec<u32>,
    outlets: Vec<usize>,
}

/// Plans one finite domain using the authoritative v1 algorithm.
///
/// # Errors
///
/// Returns a validation, arithmetic, canonical-encoding, cancellation, or hard
/// budget error. No partial plan is returned.
pub fn plan_hydrologic_domain_v1(
    input: &HydrologicDomainInputV1,
) -> WorldgenResult<HydrologicDomainPlanV1> {
    plan_hydrologic_domain_with_cancellation_v1(input, None)
}

/// Plans one finite domain while observing a caller-owned cancellation flag.
///
/// Cancellation is checked at deterministic work intervals. It is operational
/// state only and never becomes part of authoritative plan bytes.
///
/// # Errors
///
/// Returns a validation, arithmetic, canonical-encoding, cancellation, or hard
/// budget error. No partial plan is returned.
pub fn plan_hydrologic_domain_with_cancellation_v1(
    input: &HydrologicDomainInputV1,
    cancellation: Option<&AtomicBool>,
) -> WorldgenResult<HydrologicDomainPlanV1> {
    input.validate()?;
    let sample_count = input.grid.sample_count();
    let mut state = PlanningState::new(&input.config, cancellation, sample_count)?;
    let mut flood = priority_flood(input, &mut state)?;
    let mut hierarchy = depression_hierarchy(input, &flood, &mut state)?;
    apply_depression_policy(input, &mut flood, &mut hierarchy, &mut state)?;
    let (mut routing, terminal_runoff) = route_mfd(input, &flood, &hierarchy, &mut state)?;
    state.accounting.terminal_runoff_q16 = terminal_runoff;
    state.accounting.depression_count =
        u32::try_from(hierarchy.records.len()).map_err(|_| WorldgenError::ArithmeticOverflow {
            operation: "hydrologic depression accounting",
        })?;
    let input_hash = input.input_hash()?;
    let config_hash = input.config.canonical_hash()?;
    let mut plan = HydrologicDomainPlanV1 {
        dimension: input.dimension.clone(),
        domain_id: input.domain_id(),
        parent_domain: input.parent_domain,
        generation_epoch: input.generation_epoch,
        input_hash,
        config_hash,
        algorithm: input.algorithm,
        provenance: input.provenance,
        grid: input.grid.clone(),
        base_level_q8: input.base_level_q8,
        initial_elevation_q8: input.initial_elevation_q8.clone(),
        routing_elevation_q8: flood.elevation_q8,
        flood_predecessor: flood.predecessor,
        flood_rank: flood.rank,
        depressions: hierarchy,
        routing: std::mem::take(&mut routing),
        ports: input.ports.clone(),
        boundary_outlets: flood
            .outlets
            .into_iter()
            .map(|index| input.grid.coordinate(index))
            .collect(),
        accounting: state.accounting,
    };
    let mut stable = false;
    for _ in 0..8 {
        let result_bytes = u64::try_from(plan.canonical_bytes()?.len()).map_err(|_| {
            WorldgenError::ArithmeticOverflow {
                operation: "hydrologic result byte accounting",
            }
        })?;
        if result_bytes > input.config.max_result_bytes {
            return Err(WorldgenError::BudgetExceeded {
                budget: "hydrologic result bytes",
                required: result_bytes,
                limit: input.config.max_result_bytes,
            });
        }
        if result_bytes == plan.accounting.result_bytes {
            stable = true;
            break;
        }
        plan.accounting.result_bytes = result_bytes;
    }
    if !stable {
        return invalid(
            "accounting.result_bytes",
            "canonical result-byte accounting did not reach a fixed point",
        );
    }
    Ok(plan)
}

/// Plans independent domains on a Bevy task pool and publishes a stable merge.
///
/// Inputs are sorted by domain identity before spawning, and results are sorted
/// again after completion. Task count and completion order therefore cannot
/// affect the returned sequence or any plan bytes.
pub fn plan_hydrologic_domains_parallel_v1(
    pool: &TaskPool,
    mut inputs: Vec<HydrologicDomainInputV1>,
) -> Vec<(HydrologicDomainIdV1, WorldgenResult<HydrologicDomainPlanV1>)> {
    inputs.sort_by_key(HydrologicDomainInputV1::domain_id);
    let mut results = pool.scope_with_executor(false, None, |scope| {
        for input in inputs {
            scope.spawn(async move {
                let id = input.domain_id();
                (id, plan_hydrologic_domain_v1(&input))
            });
        }
    });
    results.sort_by_key(|(id, _)| *id);
    results
}

fn priority_flood(
    input: &HydrologicDomainInputV1,
    state: &mut PlanningState<'_>,
) -> WorldgenResult<FloodResult> {
    let count = input.grid.sample_count();
    let mut seeds = boundary_seed_indices(input);
    if seeds.is_empty() {
        let fallback = boundary_indices(&input.grid)
            .into_iter()
            .min_by_key(|&index| (input.initial_elevation_q8[index], index))
            .ok_or_else(|| WorldgenError::InvalidHydrologicDomain {
                field: "grid",
                reason: "finite grid has no boundary sample".to_owned(),
            })?;
        seeds.push(fallback);
    }
    seeds.sort_unstable();
    seeds.dedup();

    let mut flooded = input.initial_elevation_q8.clone();
    let mut predecessor = vec![None; count];
    let mut rank = vec![u32::MAX; count];
    let mut visited = vec![false; count];
    let mut heap = BinaryHeap::new();
    for &index in &seeds {
        visited[index] = true;
        rank[index] = 0;
        if let Some(level) = port_level_at(input, index) {
            flooded[index] = flooded[index].min(level);
        }
        heap.push(Reverse(FloodNode {
            elevation_q8: flooded[index],
            index,
        }));
    }
    state.observe_queue(heap.len())?;
    while let Some(Reverse(node)) = heap.pop() {
        state.examine()?;
        for neighbor in neighbors(&input.grid, node.index) {
            state.work(1)?;
            if visited[neighbor.index] {
                continue;
            }
            visited[neighbor.index] = true;
            flooded[neighbor.index] = flooded[neighbor.index].max(node.elevation_q8);
            predecessor[neighbor.index] = Some(index_u32(node.index)?);
            rank[neighbor.index] =
                rank[node.index]
                    .checked_add(1)
                    .ok_or(WorldgenError::ArithmeticOverflow {
                        operation: "Priority-Flood outlet distance rank",
                    })?;
            heap.push(Reverse(FloodNode {
                elevation_q8: flooded[neighbor.index],
                index: neighbor.index,
            }));
        }
        state.observe_queue(heap.len())?;
    }
    if visited.iter().any(|seen| !seen) {
        return invalid("priority_flood", "not every raster sample was reached");
    }
    Ok(FloodResult {
        elevation_q8: flooded,
        predecessor,
        rank,
        outlets: seeds,
    })
}

#[allow(
    clippy::too_many_lines,
    reason = "the single pass keeps component discovery and its closed depression record together"
)]
fn depression_hierarchy(
    input: &HydrologicDomainInputV1,
    flood: &FloodResult,
    state: &mut PlanningState<'_>,
) -> WorldgenResult<DepressionHierarchyV1> {
    let count = input.grid.sample_count();
    let mut depressed = vec![false; count];
    for (index, (&original, &corrected)) in input
        .initial_elevation_q8
        .iter()
        .zip(&flood.elevation_q8)
        .enumerate()
    {
        depressed[index] = corrected > original;
    }
    let mut assigned = vec![false; count];
    let mut records = Vec::new();
    for start in 0..count {
        state.examine()?;
        if !depressed[start] || assigned[start] {
            continue;
        }
        let mut queue = VecDeque::from([start]);
        let mut cells = Vec::new();
        assigned[start] = true;
        while let Some(index) = queue.pop_front() {
            state.work(1)?;
            cells.push(index);
            for neighbor in neighbors(&input.grid, index) {
                if depressed[neighbor.index] && !assigned[neighbor.index] {
                    assigned[neighbor.index] = true;
                    queue.push_back(neighbor.index);
                    state.observe_queue(queue.len())?;
                }
            }
        }
        cells.sort_unstable();
        let pit = cells
            .iter()
            .copied()
            .min_by_key(|&index| (input.initial_elevation_q8[index], index))
            .ok_or_else(|| WorldgenError::InvalidHydrologicDomain {
                field: "depression_hierarchy",
                reason: "empty depression component".to_owned(),
            })?;
        let spill = cells
            .iter()
            .copied()
            .min_by_key(|&index| (flood.elevation_q8[index], flood.rank[index], index))
            .ok_or_else(|| WorldgenError::InvalidHydrologicDomain {
                field: "depression_hierarchy",
                reason: "depression has no spill sample".to_owned(),
            })?;
        let outlet = ultimate_predecessor(spill, &flood.predecessor);
        let volume_q8 = cells.iter().try_fold(0_u64, |sum, &index| {
            let difference = i64::from(flood.elevation_q8[index])
                .checked_sub(i64::from(input.initial_elevation_q8[index]))
                .ok_or(WorldgenError::ArithmeticOverflow {
                    operation: "depression volume difference",
                })?;
            let difference =
                u64::try_from(difference).map_err(|_| WorldgenError::ArithmeticOverflow {
                    operation: "nonnegative depression volume",
                })?;
            sum.checked_add(difference)
                .ok_or(WorldgenError::ArithmeticOverflow {
                    operation: "depression volume sum",
                })
        })?;
        let depth_q8 = u32::try_from(
            i64::from(flood.elevation_q8[spill])
                .saturating_sub(i64::from(input.initial_elevation_q8[pit])),
        )
        .unwrap_or(u32::MAX);
        let class = classify_depression(input, cells.len(), depth_q8, volume_q8);
        let first = u32::try_from(cells[0]).map_err(|_| WorldgenError::ArithmeticOverflow {
            operation: "depression identity index",
        })?;
        let id = domain_hash(
            b"latticeaxiom.hydrologic-depression.id.v1\0",
            &[input.domain_id().as_bytes(), &first.to_be_bytes()],
        );
        records.push(DepressionRecordV1 {
            id,
            parent: None,
            pit: input.grid.coordinate(pit),
            spill: input.grid.coordinate(spill),
            outlet: input.grid.coordinate(outlet),
            class,
            cell_indices: cells
                .into_iter()
                .map(index_u32)
                .collect::<WorldgenResult<Vec<_>>>()?,
            pit_elevation_q8: input.initial_elevation_q8[pit],
            spill_elevation_q8: flood.elevation_q8[spill],
            volume_q8,
        });
        check_count(
            "hydrologic depressions",
            records.len(),
            input.config.max_depressions as usize,
        )?;
    }
    records.sort_by_key(|record| record.id);
    assign_depression_parents(input, &mut records)?;
    Ok(DepressionHierarchyV1 { records })
}

fn apply_depression_policy(
    input: &HydrologicDomainInputV1,
    flood: &mut FloodResult,
    hierarchy: &mut DepressionHierarchyV1,
    state: &mut PlanningState<'_>,
) -> WorldgenResult<()> {
    for record in &hierarchy.records {
        match record.class {
            DepressionClassV1::Lake | DepressionClassV1::Endorheic | DepressionClassV1::Wetland => {
                for &index in &record.cell_indices {
                    state.work(1)?;
                    let index = index as usize;
                    flood.elevation_q8[index] = input.initial_elevation_q8[index];
                }
                rank_retained_component(input, flood, record)?;
            }
            DepressionClassV1::Breach => carve_breach(input, flood, record, state)?,
            DepressionClassV1::Fill => {}
        }
    }
    Ok(())
}

#[allow(
    clippy::too_many_lines,
    reason = "routing construction and the immediately following conservative accumulation share invariants"
)]
fn route_mfd(
    input: &HydrologicDomainInputV1,
    flood: &FloodResult,
    hierarchy: &DepressionHierarchyV1,
    state: &mut PlanningState<'_>,
) -> WorldgenResult<(Vec<MfdRoutingCellV1>, u64)> {
    let count = input.grid.sample_count();
    let mut retained_pits = vec![false; count];
    let mut retained_owner = vec![None; count];
    for record in &hierarchy.records {
        if matches!(
            record.class,
            DepressionClassV1::Lake | DepressionClassV1::Endorheic | DepressionClassV1::Wetland
        ) {
            let pit = input.grid.index(record.pit).ok_or_else(|| {
                WorldgenError::InvalidHydrologicDomain {
                    field: "depression.pit",
                    reason: "pit lies outside its domain".to_owned(),
                }
            })?;
            retained_pits[pit] = true;
            for &index in &record.cell_indices {
                retained_owner[index as usize] = Some(pit);
            }
        }
    }
    let boundary_terminals = flood.outlets.iter().copied().collect::<BTreeSet<_>>();
    let mut routing = Vec::with_capacity(count);
    for (index, &retained_terminal) in retained_pits.iter().enumerate() {
        state.examine()?;
        let boundary_terminal = boundary_terminals.contains(&index);
        let receivers = if retained_terminal || boundary_terminal {
            Vec::new()
        } else {
            mfd_receivers(
                &input.grid,
                &flood.elevation_q8,
                &flood.rank,
                &retained_owner,
                index,
            )?
        };
        if receivers.is_empty() && !retained_terminal && !boundary_terminal {
            let neighborhood = neighbors(&input.grid, index)
                .into_iter()
                .map(|neighbor| {
                    (
                        neighbor.index,
                        flood.elevation_q8[neighbor.index],
                        flood.rank[neighbor.index],
                        retained_owner[neighbor.index],
                    )
                })
                .collect::<Vec<_>>();
            return invalid(
                "mfd_routing",
                format!(
                    "cell {index} at elevation {} rank {} owner {:?} has no receiver and is not a declared terminal; neighbors {neighborhood:?}",
                    flood.elevation_q8[index], flood.rank[index], retained_owner[index]
                ),
            );
        }
        let next_edges = state
            .accounting
            .graph_edges
            .checked_add(u32::try_from(receivers.len()).unwrap_or(u32::MAX))
            .ok_or(WorldgenError::ArithmeticOverflow {
                operation: "MFD edge accounting",
            })?;
        if next_edges > input.config.max_graph_edges {
            return Err(WorldgenError::BudgetExceeded {
                budget: "hydrologic graph edges",
                required: u64::from(next_edges),
                limit: u64::from(input.config.max_graph_edges),
            });
        }
        state.accounting.graph_edges = next_edges;
        routing.push(MfdRoutingCellV1 {
            receivers,
            accumulated_runoff_q16: u64::from(input.effective_runoff_q16[index]),
            retained_terminal,
            boundary_terminal,
        });
    }
    for port in &input.ports {
        if port.kind != HydrologicPortKindV1::Inflow {
            continue;
        }
        let index = port_index(&input.grid, port)?;
        routing[index].accumulated_runoff_q16 = routing[index]
            .accumulated_runoff_q16
            .checked_add(port.discharge_q16)
            .ok_or(WorldgenError::ArithmeticOverflow {
                operation: "boundary inflow accumulation",
            })?;
    }
    let input_runoff = routing.iter().try_fold(0_u64, |sum, cell| {
        sum.checked_add(cell.accumulated_runoff_q16)
            .ok_or(WorldgenError::ArithmeticOverflow {
                operation: "domain input runoff sum",
            })
    })?;
    state.accounting.input_runoff_q16 = input_runoff;

    let order = routing_topological_order(&routing)?;
    for index in order {
        state.work(1)?;
        let amount = routing[index].accumulated_runoff_q16;
        let receivers = routing[index].receivers.clone();
        let allocations = conservative_allocations(amount, &receivers)?;
        for (receiver, allocation) in receivers.iter().zip(allocations) {
            let target = receiver.receiver_index as usize;
            routing[target].accumulated_runoff_q16 = routing[target]
                .accumulated_runoff_q16
                .checked_add(allocation)
                .ok_or(WorldgenError::ArithmeticOverflow {
                    operation: "MFD accumulated runoff",
                })?;
        }
    }
    let terminal = routing.iter().try_fold(0_u64, |sum, cell| {
        if cell.receivers.is_empty() {
            sum.checked_add(cell.accumulated_runoff_q16)
                .ok_or(WorldgenError::ArithmeticOverflow {
                    operation: "terminal runoff sum",
                })
        } else {
            Ok(sum)
        }
    })?;
    if terminal != input_runoff {
        return invalid(
            "runoff_conservation",
            format!("input {input_runoff} differs from terminal {terminal}"),
        );
    }
    Ok((routing, terminal))
}

#[derive(Clone, Copy, Debug)]
struct Neighbor {
    index: usize,
    diagonal: bool,
    direction: u8,
}

fn neighbors(grid: &HydrologicDomainGridV1, index: usize) -> Vec<Neighbor> {
    const OFFSETS: [(i32, i32); 8] = [
        (0, -1),
        (1, -1),
        (1, 0),
        (1, 1),
        (0, 1),
        (-1, 1),
        (-1, 0),
        (-1, -1),
    ];
    let coordinate = grid.coordinate(index);
    let x = i32::from(coordinate.x);
    let z = i32::from(coordinate.z);
    let width = i32::from(grid.width.get());
    let height = i32::from(grid.height.get());
    OFFSETS
        .into_iter()
        .enumerate()
        .filter_map(|(direction, (dx, dz))| {
            let neighbor_x = x + dx;
            let neighbor_z = z + dz;
            if neighbor_x < 0 || neighbor_x >= width || neighbor_z < 0 || neighbor_z >= height {
                return None;
            }
            let neighbor_x = usize::try_from(neighbor_x).ok()?;
            let neighbor_z = usize::try_from(neighbor_z).ok()?;
            Some(Neighbor {
                index: neighbor_x + neighbor_z * usize::from(grid.width.get()),
                diagonal: dx != 0 && dz != 0,
                direction: u8::try_from(direction).ok()?,
            })
        })
        .collect()
}

fn boundary_indices(grid: &HydrologicDomainGridV1) -> Vec<usize> {
    let width = usize::from(grid.width.get());
    let height = usize::from(grid.height.get());
    let mut indices = Vec::with_capacity(width.saturating_mul(2) + height.saturating_mul(2));
    for x in 0..width {
        indices.push(x);
        indices.push(x + (height - 1) * width);
    }
    for z in 1..height.saturating_sub(1) {
        indices.push(z * width);
        indices.push(width - 1 + z * width);
    }
    indices.sort_unstable();
    indices.dedup();
    indices
}

fn boundary_seed_indices(input: &HydrologicDomainInputV1) -> Vec<usize> {
    let mut seeds = boundary_indices(&input.grid)
        .into_iter()
        .filter(|&index| input.initial_elevation_q8[index] <= input.base_level_q8)
        .collect::<Vec<_>>();
    seeds.extend(
        input
            .ports
            .iter()
            .filter(|port| port.kind != HydrologicPortKindV1::Inflow)
            .filter_map(|port| port_index(&input.grid, port).ok()),
    );
    seeds
}

fn port_index(
    grid: &HydrologicDomainGridV1,
    port: &HydrologicBoundaryPortV1,
) -> WorldgenResult<usize> {
    let coordinate = match port.edge {
        HydrologicBoundaryEdgeV1::North => HydrologicGridCoordinateV1::new(port.offset, 0),
        HydrologicBoundaryEdgeV1::East => {
            HydrologicGridCoordinateV1::new(grid.width.get() - 1, port.offset)
        }
        HydrologicBoundaryEdgeV1::South => {
            HydrologicGridCoordinateV1::new(port.offset, grid.height.get() - 1)
        }
        HydrologicBoundaryEdgeV1::West => HydrologicGridCoordinateV1::new(0, port.offset),
    };
    grid.index(coordinate)
        .ok_or_else(|| WorldgenError::InvalidHydrologicDomain {
            field: "ports.offset",
            reason: "port lies outside its declared edge".to_owned(),
        })
}

fn port_level_at(input: &HydrologicDomainInputV1, index: usize) -> Option<i32> {
    input
        .ports
        .iter()
        .filter(|port| port.kind != HydrologicPortKindV1::Inflow)
        .filter_map(|port| {
            (port_index(&input.grid, port).ok() == Some(index)).then_some(port.level_q8)
        })
        .min()
}

fn ultimate_predecessor(start: usize, predecessor: &[Option<u32>]) -> usize {
    let mut current = start;
    for _ in 0..predecessor.len() {
        let Some(next) = predecessor.get(current).copied().flatten() else {
            return current;
        };
        let next = next as usize;
        current = next;
    }
    current
}

fn classify_depression(
    input: &HydrologicDomainInputV1,
    area: usize,
    depth_q8: u32,
    volume_q8: u64,
) -> DepressionClassV1 {
    let has_declared_outlet = input
        .ports
        .iter()
        .any(|port| port.kind != HydrologicPortKindV1::Inflow);
    if input.config.allow_endorheic
        && !has_declared_outlet
        && volume_q8 >= input.config.min_endorheic_volume_q8
    {
        return DepressionClassV1::Endorheic;
    }
    if area >= input.config.min_lake_area_cells as usize
        && depth_q8 >= input.config.min_lake_depth_q8
    {
        return DepressionClassV1::Lake;
    }
    if depth_q8 <= input.config.max_wetland_depth_q8
        && area >= input.config.min_lake_area_cells as usize
    {
        return DepressionClassV1::Wetland;
    }
    if depth_q8 <= input.config.max_fill_depth_q8 {
        return DepressionClassV1::Fill;
    }
    if input.config.allow_breach {
        DepressionClassV1::Breach
    } else {
        DepressionClassV1::Fill
    }
}

fn assign_depression_parents(
    input: &HydrologicDomainInputV1,
    records: &mut [DepressionRecordV1],
) -> WorldgenResult<()> {
    let mut owner = BTreeMap::new();
    for record in records.iter() {
        for &index in &record.cell_indices {
            owner.insert(index, record.id);
        }
    }
    for record in records {
        let outlet_index = input.grid.index(record.outlet).ok_or_else(|| {
            WorldgenError::InvalidHydrologicDomain {
                field: "depression.outlet",
                reason: "outlet lies outside its domain".to_owned(),
            }
        })?;
        let outlet_index = index_u32(outlet_index)?;
        record.parent = owner
            .get(&outlet_index)
            .copied()
            .filter(|parent| *parent != record.id);
    }
    Ok(())
}

fn rank_retained_component(
    input: &HydrologicDomainInputV1,
    flood: &mut FloodResult,
    record: &DepressionRecordV1,
) -> WorldgenResult<()> {
    let pit =
        input
            .grid
            .index(record.pit)
            .ok_or_else(|| WorldgenError::InvalidHydrologicDomain {
                field: "depression.pit",
                reason: "pit lies outside its domain".to_owned(),
            })?;
    let component = record
        .cell_indices
        .iter()
        .copied()
        .map(|index| index as usize)
        .collect::<BTreeSet<_>>();
    let mut queue = VecDeque::from([(pit, 0_u32)]);
    let mut seen = BTreeSet::from([pit]);
    while let Some((index, distance)) = queue.pop_front() {
        flood.rank[index] = distance;
        for neighbor in neighbors(&input.grid, index) {
            if component.contains(&neighbor.index) && !seen.contains(&neighbor.index) {
                seen.insert(neighbor.index);
                queue.push_back((
                    neighbor.index,
                    distance
                        .checked_add(1)
                        .ok_or(WorldgenError::ArithmeticOverflow {
                            operation: "retained depression routing rank",
                        })?,
                ));
            }
        }
    }
    if seen.len() != component.len() {
        return invalid(
            "depression_hierarchy",
            "retained depression component is disconnected",
        );
    }
    Ok(())
}

fn carve_breach(
    input: &HydrologicDomainInputV1,
    flood: &mut FloodResult,
    record: &DepressionRecordV1,
    state: &mut PlanningState<'_>,
) -> WorldgenResult<()> {
    let mut current =
        input
            .grid
            .index(record.pit)
            .ok_or_else(|| WorldgenError::InvalidHydrologicDomain {
                field: "depression.pit",
                reason: "pit lies outside its domain".to_owned(),
            })?;
    let mut target = input.initial_elevation_q8[current];
    let outlets = flood.outlets.iter().copied().collect::<BTreeSet<_>>();
    for _ in 0..input.grid.sample_count() {
        state.work(1)?;
        flood.elevation_q8[current] = flood.elevation_q8[current].min(target);
        let Some(next) = flood.predecessor[current] else {
            break;
        };
        let next = next as usize;
        target = target.saturating_sub(1);
        if flood.elevation_q8[next] < target {
            break;
        }
        flood.elevation_q8[next] = flood.elevation_q8[next].min(target);
        current = next;
        if outlets.contains(&current) {
            break;
        }
    }
    Ok(())
}

#[derive(Clone, Copy, Debug)]
struct WeightCandidate {
    receiver_index: usize,
    slope: u64,
    direction: u8,
    weight: u32,
    remainder: u128,
}

fn mfd_receivers(
    grid: &HydrologicDomainGridV1,
    elevation_q8: &[i32],
    rank: &[u32],
    retained_owner: &[Option<usize>],
    index: usize,
) -> WorldgenResult<Vec<MfdReceiverV1>> {
    let source_owner = retained_owner[index];
    let mut candidates = Vec::new();
    for neighbor in neighbors(grid, index) {
        if source_owner.is_some() && retained_owner[neighbor.index] != source_owner {
            continue;
        }
        let delta = i64::from(elevation_q8[index]) - i64::from(elevation_q8[neighbor.index]);
        let follows_retained_rank = source_owner.is_some() && rank[neighbor.index] < rank[index];
        if source_owner.is_some() && !follows_retained_rank {
            continue;
        }
        let slope = if follows_retained_rank && delta <= 0 {
            u64::from(rank[index] - rank[neighbor.index])
        } else if delta > 0 {
            let delta = u64::try_from(delta).map_err(|_| WorldgenError::ArithmeticOverflow {
                operation: "positive MFD slope",
            })?;
            delta
                .checked_mul(if neighbor.diagonal {
                    DIAGONAL_SLOPE_SCALE
                } else {
                    CARDINAL_SLOPE_SCALE
                })
                .ok_or(WorldgenError::ArithmeticOverflow {
                    operation: "MFD slope numerator",
                })?
        } else if delta == 0 && rank[neighbor.index] < rank[index] {
            1
        } else {
            continue;
        };
        candidates.push(WeightCandidate {
            receiver_index: neighbor.index,
            slope,
            direction: neighbor.direction,
            weight: 0,
            remainder: 0,
        });
    }
    if candidates.is_empty() {
        return Ok(Vec::new());
    }
    let slope_sum = candidates
        .iter()
        .map(|candidate| u128::from(candidate.slope))
        .sum::<u128>();
    let mut assigned = 0_u32;
    for candidate in &mut candidates {
        let numerator = u128::from(candidate.slope) * u128::from(HYDROLOGIC_WEIGHT_SCALE_V1);
        candidate.weight = u32::try_from(numerator / slope_sum).map_err(|_| {
            WorldgenError::ArithmeticOverflow {
                operation: "MFD fixed-point weight",
            }
        })?;
        candidate.remainder = numerator % slope_sum;
        assigned =
            assigned
                .checked_add(candidate.weight)
                .ok_or(WorldgenError::ArithmeticOverflow {
                    operation: "MFD assigned weight",
                })?;
    }
    let residual = HYDROLOGIC_WEIGHT_SCALE_V1.checked_sub(assigned).ok_or(
        WorldgenError::ArithmeticOverflow {
            operation: "MFD weight residual",
        },
    )?;
    let mut residual_order = (0..candidates.len()).collect::<Vec<_>>();
    residual_order.sort_by_key(|&position| {
        (
            Reverse(candidates[position].remainder),
            candidates[position].receiver_index,
            candidates[position].direction,
        )
    });
    for &position in residual_order.iter().take(residual as usize) {
        candidates[position].weight = candidates[position].weight.checked_add(1).ok_or(
            WorldgenError::ArithmeticOverflow {
                operation: "MFD residual assignment",
            },
        )?;
    }
    candidates.sort_by_key(|candidate| (candidate.direction, candidate.receiver_index));
    Ok(candidates
        .into_iter()
        .map(|candidate| MfdReceiverV1 {
            receiver_index: u32::try_from(candidate.receiver_index).unwrap_or(u32::MAX),
            weight_per_million: candidate.weight,
            slope_numerator: candidate.slope,
        })
        .collect())
}

fn routing_topological_order(routing: &[MfdRoutingCellV1]) -> WorldgenResult<Vec<usize>> {
    let mut indegree = vec![0_u8; routing.len()];
    for cell in routing {
        for receiver in &cell.receivers {
            let target = receiver.receiver_index as usize;
            let Some(value) = indegree.get_mut(target) else {
                return invalid("mfd_routing", "receiver index lies outside the raster");
            };
            *value = value
                .checked_add(1)
                .ok_or(WorldgenError::ArithmeticOverflow {
                    operation: "MFD topological indegree",
                })?;
        }
    }
    let mut ready = BinaryHeap::new();
    for (index, &degree) in indegree.iter().enumerate() {
        if degree == 0 {
            ready.push(Reverse(index));
        }
    }
    let mut order = Vec::with_capacity(routing.len());
    while let Some(Reverse(index)) = ready.pop() {
        order.push(index);
        for receiver in &routing[index].receivers {
            let target = receiver.receiver_index as usize;
            indegree[target] = indegree[target].checked_sub(1).ok_or_else(|| {
                WorldgenError::InvalidHydrologicDomain {
                    field: "mfd_routing",
                    reason: "topological indegree underflow".to_owned(),
                }
            })?;
            if indegree[target] == 0 {
                ready.push(Reverse(target));
            }
        }
    }
    if order.len() != routing.len() {
        return invalid("mfd_routing", "receiver graph contains a cycle");
    }
    Ok(order)
}

fn conservative_allocations(amount: u64, receivers: &[MfdReceiverV1]) -> WorldgenResult<Vec<u64>> {
    if receivers.is_empty() {
        return Ok(Vec::new());
    }
    let denominator = u128::from(HYDROLOGIC_WEIGHT_SCALE_V1);
    let mut allocations = Vec::with_capacity(receivers.len());
    let mut remainders = Vec::with_capacity(receivers.len());
    let mut assigned = 0_u64;
    for (position, receiver) in receivers.iter().enumerate() {
        let numerator = u128::from(amount) * u128::from(receiver.weight_per_million);
        let allocation = u64::try_from(numerator / denominator).map_err(|_| {
            WorldgenError::ArithmeticOverflow {
                operation: "MFD runoff allocation",
            }
        })?;
        assigned = assigned
            .checked_add(allocation)
            .ok_or(WorldgenError::ArithmeticOverflow {
                operation: "MFD runoff assigned sum",
            })?;
        allocations.push(allocation);
        remainders.push((numerator % denominator, receiver.receiver_index, position));
    }
    let residual = amount
        .checked_sub(assigned)
        .ok_or(WorldgenError::ArithmeticOverflow {
            operation: "MFD runoff residual",
        })?;
    remainders.sort_by_key(|&(remainder, receiver, _)| (Reverse(remainder), receiver));
    let residual_count =
        usize::try_from(residual).map_err(|_| WorldgenError::ArithmeticOverflow {
            operation: "MFD runoff residual count",
        })?;
    for &(_, _, position) in remainders.iter().take(residual_count) {
        allocations[position] =
            allocations[position]
                .checked_add(1)
                .ok_or(WorldgenError::ArithmeticOverflow {
                    operation: "MFD runoff residual assignment",
                })?;
    }
    Ok(allocations)
}

fn validate_config(config: &HydrologicDomainConfigV1) -> WorldgenResult<()> {
    if config.max_grid_points < 4 {
        return invalid(
            "config.max_grid_points",
            "must permit at least a 2 by 2 grid",
        );
    }
    if config.max_queue_entries < config.max_grid_points {
        return invalid(
            "config.max_queue_entries",
            "must permit one entry per grid point",
        );
    }
    if config.max_graph_edges < config.max_grid_points.saturating_sub(1) {
        return invalid(
            "config.max_graph_edges",
            "must permit at least a spanning receiver graph",
        );
    }
    if config.max_work_units == 0 || config.max_temporary_bytes == 0 || config.max_result_bytes == 0
    {
        return invalid(
            "config",
            "work, temporary-byte, and result-byte limits must be nonzero",
        );
    }
    if config.max_fill_depth_q8 > config.max_wetland_depth_q8 {
        return invalid(
            "config.max_fill_depth_q8",
            "fill depth cannot exceed the wetland depth threshold",
        );
    }
    Ok(())
}

fn check_count(kind: &'static str, actual: usize, limit: usize) -> WorldgenResult<()> {
    if actual > limit {
        return Err(WorldgenError::CollectionLimitExceeded {
            kind,
            actual,
            limit,
        });
    }
    Ok(())
}

fn invalid<T>(field: &'static str, reason: impl Into<String>) -> WorldgenResult<T> {
    Err(WorldgenError::InvalidHydrologicDomain {
        field,
        reason: reason.into(),
    })
}

fn index_u32(index: usize) -> WorldgenResult<u32> {
    u32::try_from(index).map_err(|_| WorldgenError::ArithmeticOverflow {
        operation: "hydrologic row-major index",
    })
}

const fn port_kind_tag(kind: HydrologicPortKindV1) -> u8 {
    match kind {
        HydrologicPortKindV1::Inflow | HydrologicPortKindV1::Outflow => 0,
        HydrologicPortKindV1::OceanOutlet => 1,
    }
}

#[derive(Debug)]
struct CachedDomainPlan {
    plan: Arc<HydrologicDomainPlanV1>,
    bytes: u64,
}

/// Explicitly bounded in-memory cache for immutable domain plans.
///
/// The cache never performs unbounded eviction work. Insertion fails closed
/// when either the entry or byte cap would be exceeded; callers may create a
/// new cache generation at a safe scheduling boundary.
#[derive(Debug)]
pub struct HydrologicDomainCacheV1 {
    max_entries: usize,
    max_bytes: u64,
    used_bytes: u64,
    entries: BTreeMap<HydrologicDomainCacheKeyV1, CachedDomainPlan>,
}

impl HydrologicDomainCacheV1 {
    /// Creates an empty cache with hard entry and canonical-byte limits.
    ///
    /// # Errors
    ///
    /// Returns a hydrologic-domain validation error when either cap is zero.
    pub fn new(max_entries: usize, max_bytes: u64) -> WorldgenResult<Self> {
        if max_entries == 0 || max_bytes == 0 {
            return invalid("domain_cache", "entry and byte limits must be nonzero");
        }
        Ok(Self {
            max_entries,
            max_bytes,
            used_bytes: 0,
            entries: BTreeMap::new(),
        })
    }

    /// Returns a shared immutable plan for an exact cache-key match.
    #[must_use]
    pub fn get(&self, key: &HydrologicDomainCacheKeyV1) -> Option<Arc<HydrologicDomainPlanV1>> {
        self.entries.get(key).map(|entry| Arc::clone(&entry.plan))
    }

    /// Atomically inserts one plan after validating every key component.
    ///
    /// Existing exact keys are idempotent only when their canonical plan bytes
    /// match. A rejected insertion leaves the cache unchanged.
    ///
    /// # Errors
    ///
    /// Returns a validation, canonical-encoding, or hard cache-budget error.
    pub fn insert(
        &mut self,
        key: HydrologicDomainCacheKeyV1,
        plan: Arc<HydrologicDomainPlanV1>,
    ) -> WorldgenResult<()> {
        if key.dimension != plan.dimension
            || key.domain_id != plan.domain_id
            || key.generation_epoch != plan.generation_epoch
            || key.input_hash != plan.input_hash
            || key.algorithm != plan.algorithm
            || key.config_hash != plan.config_hash
        {
            return invalid(
                "domain_cache.key",
                "cache key does not describe the supplied plan",
            );
        }
        let canonical = plan.canonical_bytes()?;
        let bytes =
            u64::try_from(canonical.len()).map_err(|_| WorldgenError::ArithmeticOverflow {
                operation: "hydrologic cache entry bytes",
            })?;
        if let Some(existing) = self.entries.get(&key) {
            if existing.plan.canonical_bytes()? != canonical {
                return invalid(
                    "domain_cache.entry",
                    "one exact cache key resolved to different plan bytes",
                );
            }
            return Ok(());
        }
        if self.entries.len() == self.max_entries {
            return Err(WorldgenError::BudgetExceeded {
                budget: "hydrologic cache entries",
                required: u64::try_from(self.entries.len().saturating_add(1)).unwrap_or(u64::MAX),
                limit: u64::try_from(self.max_entries).unwrap_or(u64::MAX),
            });
        }
        let required =
            self.used_bytes
                .checked_add(bytes)
                .ok_or(WorldgenError::ArithmeticOverflow {
                    operation: "hydrologic cache used bytes",
                })?;
        if required > self.max_bytes {
            return Err(WorldgenError::BudgetExceeded {
                budget: "hydrologic cache bytes",
                required,
                limit: self.max_bytes,
            });
        }
        self.entries.insert(key, CachedDomainPlan { plan, bytes });
        self.used_bytes = required;
        Ok(())
    }

    /// Removes one exact entry and returns its shared plan.
    pub fn remove(
        &mut self,
        key: &HydrologicDomainCacheKeyV1,
    ) -> Option<Arc<HydrologicDomainPlanV1>> {
        let removed = self.entries.remove(key)?;
        self.used_bytes = self.used_bytes.saturating_sub(removed.bytes);
        Some(removed.plan)
    }

    /// Returns the current entry count.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Returns whether the cache is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Returns canonical plan bytes retained by the cache.
    #[must_use]
    pub const fn used_bytes(&self) -> u64 {
        self.used_bytes
    }
}

/// Non-authoritative orientation diagnostics for routing-candidate comparison.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DevelopmentRoutingComparisonV1 {
    compared_cells: u32,
    mfd_p1_axis_bias_ppm: u32,
    mfd_p11_axis_bias_ppm: u32,
    d_infinity_axis_bias_ppm: u32,
}

impl DevelopmentRoutingComparisonV1 {
    /// Returns the number of interior downslope cells compared.
    #[must_use]
    pub const fn compared_cells(self) -> u32 {
        self.compared_cells
    }

    /// Returns cardinal-versus-diagonal weight imbalance for MFD `p = 1`.
    #[must_use]
    pub const fn mfd_p1_axis_bias_ppm(self) -> u32 {
        self.mfd_p1_axis_bias_ppm
    }

    /// Returns the same diagnostic for development-only MFD `p = 1.1`.
    #[must_use]
    pub const fn mfd_p11_axis_bias_ppm(self) -> u32 {
        self.mfd_p11_axis_bias_ppm
    }

    /// Returns the same diagnostic for the D-infinity angle comparator.
    #[must_use]
    pub const fn d_infinity_axis_bias_ppm(self) -> u32 {
        self.d_infinity_axis_bias_ppm
    }
}

/// Compares authoritative MFD `p = 1` with floating MFD `p = 1.1` and a
/// D-infinity-style continuous-angle routing oracle.
///
/// This tool is intentionally diagnostic: it returns aggregate integer
/// metrics and cannot create or mutate an authoritative domain plan.
///
/// # Errors
///
/// Returns a hydrologic-domain validation error for malformed or too-small
/// input.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::similar_names,
    reason = "the development comparator clamps finite aggregate metrics before conversion"
)]
pub fn d_infinity_development_comparison_v1(
    input: &HydrologicDomainInputV1,
) -> WorldgenResult<DevelopmentRoutingComparisonV1> {
    input.validate()?;
    if input.grid.width.get() < 3 || input.grid.height.get() < 3 {
        return invalid(
            "comparison.grid",
            "routing comparison requires at least 3 by 3 samples",
        );
    }
    let width = usize::from(input.grid.width.get());
    let height = usize::from(input.grid.height.get());
    let mut p1_axis = 0_f64;
    let mut p1_diagonal = 0_f64;
    let mut p11_axis = 0_f64;
    let mut p11_diagonal = 0_f64;
    let mut dinf_axis = 0_f64;
    let mut dinf_diagonal = 0_f64;
    let mut compared = 0_u32;
    for z in 1..height - 1 {
        for x in 1..width - 1 {
            let index = x + z * width;
            let mut slopes = Vec::new();
            for neighbor in neighbors(&input.grid, index) {
                let delta = f64::from(input.initial_elevation_q8[index])
                    - f64::from(input.initial_elevation_q8[neighbor.index]);
                if delta > 0.0 {
                    let distance = if neighbor.diagonal {
                        std::f64::consts::SQRT_2
                    } else {
                        1.0
                    };
                    slopes.push((neighbor.direction, delta / distance));
                }
            }
            if slopes.is_empty() {
                continue;
            }
            compared = compared.saturating_add(1);
            let p1_total = slopes.iter().map(|(_, slope)| slope).sum::<f64>();
            let p11_total = slopes.iter().map(|(_, slope)| slope.powf(1.1)).sum::<f64>();
            for &(direction, slope) in &slopes {
                let is_axis = direction % 2 == 0;
                if is_axis {
                    p1_axis += slope / p1_total;
                    p11_axis += slope.powf(1.1) / p11_total;
                } else {
                    p1_diagonal += slope / p1_total;
                    p11_diagonal += slope.powf(1.1) / p11_total;
                }
            }

            let west = f64::from(input.initial_elevation_q8[index - 1]);
            let east = f64::from(input.initial_elevation_q8[index + 1]);
            let north = f64::from(input.initial_elevation_q8[index - width]);
            let south = f64::from(input.initial_elevation_q8[index + width]);
            let downhill_x = west - east;
            let downhill_z = north - south;
            let mut sector = downhill_x.atan2(-downhill_z) / (std::f64::consts::FRAC_PI_4);
            if sector < 0.0 {
                sector += 8.0;
            }
            let lower = sector.floor() as u8 % 8;
            let upper = (lower + 1) % 8;
            let upper_weight = sector - sector.floor();
            let lower_weight = 1.0 - upper_weight;
            if lower.is_multiple_of(2) {
                dinf_axis += lower_weight;
            } else {
                dinf_diagonal += lower_weight;
            }
            if upper.is_multiple_of(2) {
                dinf_axis += upper_weight;
            } else {
                dinf_diagonal += upper_weight;
            }
        }
    }
    if compared == 0 {
        return invalid("comparison.dem", "DEM has no interior downslope sample");
    }
    Ok(DevelopmentRoutingComparisonV1 {
        compared_cells: compared,
        mfd_p1_axis_bias_ppm: axis_bias_ppm(p1_axis, p1_diagonal),
        mfd_p11_axis_bias_ppm: axis_bias_ppm(p11_axis, p11_diagonal),
        d_infinity_axis_bias_ppm: axis_bias_ppm(dinf_axis, dinf_diagonal),
    })
}

#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "the finite diagnostic is explicitly clamped to the u32 ppm envelope"
)]
fn axis_bias_ppm(axis: f64, diagonal: f64) -> u32 {
    let total = axis + diagonal;
    if total <= f64::EPSILON {
        return 0;
    }
    (((axis - diagonal).abs() / total) * f64::from(HYDROLOGIC_WEIGHT_SCALE_V1))
        .round()
        .clamp(0.0, f64::from(HYDROLOGIC_WEIGHT_SCALE_V1)) as u32
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeSet, str::FromStr};

    use bevy::tasks::TaskPoolBuilder;
    use latticeaxiom_core::CanonicalHash;

    use super::*;

    fn fixture_input(
        width: u16,
        height: u16,
        elevations: Vec<i32>,
        config: HydrologicDomainConfigV1,
        domain_x: i64,
    ) -> HydrologicDomainInputV1 {
        HydrologicDomainInputV1::new(
            DimensionId::from_str("latticeaxiom:dimension/terrenia")
                .expect("fixture dimension identity is valid"),
            GenerationEpochIdV1::from_hash(CanonicalHash::digest("hydrology-epoch")),
            domain_x,
            -7,
            None,
            CanonicalHash::digest("fixture-provider-provenance"),
            HydrologicDomainGridV1::new(
                -1_024,
                2_048,
                NonZeroU32::new(32).expect("fixture spacing is nonzero"),
                NonZeroU16::new(width).expect("fixture width is nonzero"),
                NonZeroU16::new(height).expect("fixture height is nonzero"),
                1,
            ),
            i32::MIN,
            elevations,
            vec![65_536; usize::from(width) * usize::from(height)],
            Vec::new(),
            config,
        )
        .expect("fixture domain input is valid")
    }

    fn plane(width: u16, height: u16) -> Vec<i32> {
        let mut values = Vec::with_capacity(usize::from(width) * usize::from(height));
        for z in 0..height {
            for x in 0..width {
                values.push(20_000 - i32::from(x) * 257 - i32::from(z) * 131);
            }
        }
        values
    }

    fn bowl(edge: u16) -> Vec<i32> {
        let center = i32::from(edge / 2);
        let mut values = Vec::with_capacity(usize::from(edge) * usize::from(edge));
        for z in 0..edge {
            for x in 0..edge {
                let distance = (i32::from(x) - center)
                    .abs()
                    .max((i32::from(z) - center).abs());
                values.push(1_000 - (center - distance) * 180);
            }
        }
        values
    }

    fn assert_all_paths_terminate(plan: &HydrologicDomainPlanV1) {
        fn visit(
            index: usize,
            routing: &[MfdRoutingCellV1],
            active: &mut BTreeSet<usize>,
            complete: &mut BTreeSet<usize>,
        ) {
            if complete.contains(&index) {
                return;
            }
            assert!(
                active.insert(index),
                "receiver graph contains a cycle at {index}"
            );
            let cell = &routing[index];
            if cell.receivers.is_empty() {
                assert!(cell.retained_terminal || cell.boundary_terminal);
            } else {
                for receiver in &cell.receivers {
                    visit(receiver.receiver_index as usize, routing, active, complete);
                }
            }
            active.remove(&index);
            complete.insert(index);
        }

        let mut complete = BTreeSet::new();
        for start in 0..plan.routing.len() {
            visit(start, &plan.routing, &mut BTreeSet::new(), &mut complete);
        }
    }

    #[test]
    fn analytic_plane_routes_and_conserves_every_runoff_unit() {
        let input = fixture_input(
            17,
            13,
            plane(17, 13),
            HydrologicDomainConfigV1::default(),
            0,
        );
        let plan = plan_hydrologic_domain_v1(&input).expect("plane domain plans");
        assert!(plan.depressions.records.is_empty());
        assert_eq!(plan.accounting.input_runoff_q16, 17_u64 * 13 * 65_536);
        assert_eq!(
            plan.accounting.input_runoff_q16,
            plan.accounting.terminal_runoff_q16
        );
        assert_all_paths_terminate(&plan);
        for cell in &plan.routing {
            if !cell.receivers.is_empty() {
                assert_eq!(
                    cell.receivers
                        .iter()
                        .map(|receiver| receiver.weight_per_million)
                        .sum::<u32>(),
                    HYDROLOGIC_WEIGHT_SCALE_V1
                );
            }
        }
    }

    #[test]
    fn priority_flood_retains_one_level_bowl_with_original_dem_intact() {
        let config = HydrologicDomainConfigV1 {
            allow_endorheic: false,
            min_lake_area_cells: 4,
            min_lake_depth_q8: 100,
            ..HydrologicDomainConfigV1::default()
        };
        let elevations = bowl(9);
        let input = fixture_input(9, 9, elevations.clone(), config, 0);
        let plan = plan_hydrologic_domain_v1(&input).expect("bowl domain plans");
        assert_eq!(plan.initial_elevation_q8, elevations);
        assert_eq!(plan.depressions.records.len(), 1);
        assert_eq!(plan.depressions.records[0].class, DepressionClassV1::Lake);
        assert!(plan.depressions.records[0].volume_q8 > 0);
        assert_all_paths_terminate(&plan);
        assert_eq!(
            plan.accounting.input_runoff_q16,
            plan.accounting.terminal_runoff_q16
        );
    }

    #[test]
    fn direction_independent_ports_match_from_opposite_sides() {
        let first = HydrologicDomainIdV1::from_hash(CanonicalHash::digest("first-domain"));
        let second = HydrologicDomainIdV1::from_hash(CanonicalHash::digest("second-domain"));
        let outgoing = HydrologicBoundaryPortV1::between(
            first,
            second,
            HydrologicBoundaryEdgeV1::East,
            3,
            128,
            -64,
            HydrologicPortKindV1::Outflow,
            12_345,
            98_765,
        );
        let incoming = HydrologicBoundaryPortV1::between(
            second,
            first,
            HydrologicBoundaryEdgeV1::West,
            3,
            128,
            -64,
            HydrologicPortKindV1::Inflow,
            12_345,
            98_765,
        );
        assert_eq!(outgoing.id, incoming.id);
        assert_eq!(outgoing.signature, incoming.signature);
    }

    #[test]
    fn input_port_registration_order_does_not_change_bytes() {
        let mut input = fixture_input(5, 5, plane(5, 5), HydrologicDomainConfigV1::default(), 0);
        let local = input.domain_id();
        let adjacent_a = HydrologicDomainIdV1::from_hash(CanonicalHash::digest("adjacent-a"));
        let adjacent_b = HydrologicDomainIdV1::from_hash(CanonicalHash::digest("adjacent-b"));
        let a = HydrologicBoundaryPortV1::between(
            local,
            adjacent_a,
            HydrologicBoundaryEdgeV1::North,
            1,
            -992,
            2_048,
            HydrologicPortKindV1::Outflow,
            18_000,
            0,
        );
        let b = HydrologicBoundaryPortV1::between(
            local,
            adjacent_b,
            HydrologicBoundaryEdgeV1::South,
            2,
            -960,
            2_176,
            HydrologicPortKindV1::Outflow,
            17_000,
            0,
        );
        let mut reversed = input.clone();
        input.ports = vec![a.clone(), b.clone()];
        input.ports.sort_by_key(HydrologicBoundaryPortV1::id);
        reversed.ports = vec![b, a];
        reversed.ports.sort_by_key(HydrologicBoundaryPortV1::id);
        assert_eq!(
            input.canonical_bytes().ok(),
            reversed.canonical_bytes().ok()
        );
        assert_eq!(
            plan_hydrologic_domain_v1(&input)
                .and_then(|plan| plan.canonical_bytes())
                .ok(),
            plan_hydrologic_domain_v1(&reversed)
                .and_then(|plan| plan.canonical_bytes())
                .ok()
        );
    }

    #[test]
    fn bevy_task_count_and_completion_order_do_not_change_plan_bytes() {
        let inputs = (0..6_i64)
            .map(|domain_x| {
                fixture_input(
                    17,
                    13,
                    plane(17, 13),
                    HydrologicDomainConfigV1::default(),
                    domain_x,
                )
            })
            .collect::<Vec<_>>();
        let single = TaskPoolBuilder::new().num_threads(1).build();
        let parallel = TaskPoolBuilder::new().num_threads(4).build();
        let first = plan_hydrologic_domains_parallel_v1(&single, inputs.clone());
        let mut reversed = inputs;
        reversed.reverse();
        let second = plan_hydrologic_domains_parallel_v1(&parallel, reversed);
        assert_eq!(first.len(), second.len());
        for ((first_id, first), (second_id, second)) in first.into_iter().zip(second) {
            assert_eq!(first_id, second_id);
            assert_eq!(
                first.and_then(|plan| plan.canonical_bytes()).ok(),
                second.and_then(|plan| plan.canonical_bytes()).ok()
            );
        }
    }

    #[test]
    fn hard_grid_cache_and_cancellation_limits_fail_closed() {
        let bounded = HydrologicDomainConfigV1 {
            max_grid_points: 24,
            ..HydrologicDomainConfigV1::default()
        };
        assert!(
            HydrologicDomainInputV1::new(
                DimensionId::from_str("latticeaxiom:dimension/terrenia")
                    .expect("fixture dimension identity is valid"),
                GenerationEpochIdV1::from_hash(CanonicalHash::digest("epoch")),
                0,
                0,
                None,
                CanonicalHash::digest("provenance"),
                HydrologicDomainGridV1::new(
                    0,
                    0,
                    NonZeroU32::MIN,
                    NonZeroU16::new(5).expect("five is nonzero"),
                    NonZeroU16::new(5).expect("five is nonzero"),
                    0,
                ),
                0,
                plane(5, 5),
                vec![1; 25],
                Vec::new(),
                bounded,
            )
            .is_err()
        );

        let input = fixture_input(5, 5, plane(5, 5), HydrologicDomainConfigV1::default(), 0);
        let cancellation = AtomicBool::new(true);
        assert!(matches!(
            plan_hydrologic_domain_with_cancellation_v1(&input, Some(&cancellation)),
            Err(WorldgenError::HydrologicPlanningCancelled {
                completed_work_units: 0
            })
        ));

        let key = input.cache_key().expect("fixture cache key canonicalizes");
        let plan = Arc::new(plan_hydrologic_domain_v1(&input).expect("fixture plans"));
        let plan_bytes = u64::try_from(plan.canonical_bytes().expect("plan canonicalizes").len())
            .expect("fixture byte count fits u64");
        let mut cache =
            HydrologicDomainCacheV1::new(1, plan_bytes).expect("positive cache bounds are valid");
        cache
            .insert(key.clone(), Arc::clone(&plan))
            .expect("first exact entry fits");
        cache
            .insert(key.clone(), Arc::clone(&plan))
            .expect("idempotent exact entry fits");
        assert_eq!(
            cache.get(&key).map(|cached| cached.domain_id),
            Some(plan.domain_id)
        );
        assert_eq!(cache.len(), 1);
        assert_eq!(cache.used_bytes(), plan_bytes);
        assert!(cache.remove(&key).is_some());
        assert!(cache.is_empty());
    }

    #[test]
    fn development_comparator_covers_axis_and_diagonal_planes() {
        let axis = fixture_input(
            17,
            17,
            plane(17, 17),
            HydrologicDomainConfigV1::default(),
            0,
        );
        let axis_comparison =
            d_infinity_development_comparison_v1(&axis).expect("axis plane compares");
        assert_eq!(axis_comparison.compared_cells, 225);
        assert!(axis_comparison.mfd_p1_axis_bias_ppm <= HYDROLOGIC_WEIGHT_SCALE_V1);
        assert!(axis_comparison.mfd_p11_axis_bias_ppm <= HYDROLOGIC_WEIGHT_SCALE_V1);
        assert!(axis_comparison.d_infinity_axis_bias_ppm <= HYDROLOGIC_WEIGHT_SCALE_V1);

        let diagonal_values = (0..17_u16)
            .flat_map(|z| (0..17_u16).map(move |x| 20_000 - (i32::from(x) + i32::from(z)) * 200))
            .collect();
        let diagonal = fixture_input(
            17,
            17,
            diagonal_values,
            HydrologicDomainConfigV1::default(),
            1,
        );
        let diagonal_comparison =
            d_infinity_development_comparison_v1(&diagonal).expect("diagonal plane compares");
        assert_eq!(diagonal_comparison.compared_cells, 225);
    }

    #[test]
    fn canonical_plan_has_stable_known_answer_hash() {
        let input = fixture_input(5, 5, plane(5, 5), HydrologicDomainConfigV1::default(), 3);
        let plan = plan_hydrologic_domain_v1(&input).expect("golden fixture plans");
        assert_eq!(
            plan.canonical_hash()
                .expect("golden plan canonicalizes")
                .to_string(),
            "1d254ead7d586bfad7cec2cf9f0b3a82a22b4f972a023449731f203c74b8bff8"
        );
    }

    #[test]
    fn rough_benchmark_dem_has_no_orphan() {
        let edge = 64_u16;
        let center = i32::from(edge / 2);
        let mut elevations = Vec::with_capacity(usize::from(edge) * usize::from(edge));
        for z in 0..edge {
            for x in 0..edge {
                let dx = i32::from(x) - center;
                let dz = i32::from(z) - center;
                elevations.push(
                    40_000 - i32::from(x) * 29 - i32::from(z) * 17
                        + (dx * dx + dz * dz).rem_euclid(257),
                );
            }
        }
        let input = fixture_input(
            edge,
            edge,
            elevations,
            HydrologicDomainConfigV1::default(),
            0,
        );
        let result = plan_hydrologic_domain_v1(&input);
        assert!(result.is_ok(), "{:?}", result.err());
    }
}
