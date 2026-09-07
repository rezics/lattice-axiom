//! Package-owned V6 hydrology occupancy on the existing V4 coordinator.
//!
//! Surface water, underground drainage, aquifer tables, and initial water/lava
//! occupancy are coordinate queries. The module emits versioned candidates; it
//! never opens a writer, never runs D9 dynamic flow, and never edits cave
//! topology. Snapshot bytes stay owned by the D4 materializer.

use latticeaxiom_core::{CanonicalHash, StableId, canonical_json_bytes};
use latticeaxiom_storage::{ChunkCoordinate, DimensionId};
use serde::{Deserialize, Serialize};

use crate::{
    AquiferBasinIdV1, ChunkFaceV1, DrainageLinkIdV1, GenerationEpochIdV1, GenerationInputHashV1,
    HydrologyOccupancyHashV1, NaturalSamplerV1, RiverSampleV1, TerrainConfigV2, TerrainStyleV1,
    WorldgenConfigV1, WorldgenError, WorldgenResult, WorldgenSeedRootV2,
    hashes::{domain_hash, hash_u64},
};

const OCCUPANCY_DOMAIN: &[u8] = b"latticeaxiom.hydrology-occupancy.v1\0";
const AQUIFER_DOMAIN: &[u8] = b"latticeaxiom.hydrology-aquifer.v1\0";
const DRAINAGE_DOMAIN: &[u8] = b"latticeaxiom.hydrology-drainage.v1\0";
const FACE_DOMAIN: &[u8] = b"latticeaxiom.hydrology-shared-face.v1\0";
const LAVA_DOMAIN: &[u8] = b"latticeaxiom.hydrology-lava.v1\0";
const CANDIDATE_SCHEMA: &str = "latticeaxiom:hydrology-occupancy-candidate@1";

/// Closed integer configuration for V6 hydrology occupancy.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct HydrologyOccupancyConfigV1 {
    /// Constrain surface channels to sampled final banks. Omitted in legacy
    /// canonical records so their generation identities remain unchanged.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub bank_constrained_channels: bool,
    /// Inclusive global surface-water level, or `None` for no ocean fill.
    pub sea_level_y: Option<i32>,
    /// Coarse aquifer basin cell edge in voxels.
    pub aquifer_cell_edge_voxels: u16,
    /// Depth below the river-adjusted surface of the water table.
    pub aquifer_depth_voxels: u16,
    /// Aquifer presence threshold in `0..=1024` hash units.
    pub aquifer_threshold_per_1024: u16,
    /// Inclusive lava column height above the world floor.
    pub lava_column_height_voxels: u16,
    /// Deep lava occupancy threshold in `0..=1024` hash units.
    pub lava_threshold_per_1024: u16,
    /// Hard occupancy cell bound for one chunk candidate.
    pub max_cells_per_chunk: u32,
    /// Hard drainage-frontier queue bound for one chunk candidate.
    pub max_queue_depth: u32,
    /// Hard canonical-candidate byte bound.
    pub max_in_flight_bytes: u32,
}

impl Default for HydrologyOccupancyConfigV1 {
    fn default() -> Self {
        Self {
            bank_constrained_channels: false,
            sea_level_y: None,
            aquifer_cell_edge_voxels: 32,
            aquifer_depth_voxels: 12,
            aquifer_threshold_per_1024: 640,
            lava_column_height_voxels: 8,
            lava_threshold_per_1024: 96,
            max_cells_per_chunk: 4_096,
            max_queue_depth: 8_192,
            max_in_flight_bytes: 524_288,
        }
    }
}

impl HydrologyOccupancyConfigV1 {
    /// Decodes a closed JSON record and validates occupancy bounds.
    ///
    /// # Errors
    ///
    /// Returns a config encoding or bounds error.
    pub fn from_json(bytes: &[u8]) -> WorldgenResult<Self> {
        let value: Self = serde_json::from_slice(bytes).map_err(|error| {
            WorldgenError::InvalidConfigEncoding {
                reason: error.to_string(),
            }
        })?;
        value.validate()?;
        Ok(value)
    }

    /// Validates closed occupancy numeric bounds.
    ///
    /// # Errors
    ///
    /// Returns [`WorldgenError::InvalidHydrologyOccupancy`] when a field is
    /// outside the closed domain.
    pub fn validate(&self) -> WorldgenResult<()> {
        bounded_u16(
            "aquifer_cell_edge_voxels",
            self.aquifer_cell_edge_voxels,
            8,
            256,
        )?;
        bounded_u16("aquifer_depth_voxels", self.aquifer_depth_voxels, 1, 64)?;
        bounded_u16(
            "aquifer_threshold_per_1024",
            self.aquifer_threshold_per_1024,
            0,
            1_024,
        )?;
        bounded_u16(
            "lava_column_height_voxels",
            self.lava_column_height_voxels,
            1,
            32,
        )?;
        bounded_u16(
            "lava_threshold_per_1024",
            self.lava_threshold_per_1024,
            0,
            1_024,
        )?;
        bounded_u32(
            "max_cells_per_chunk",
            self.max_cells_per_chunk,
            1,
            1_048_576,
        )?;
        bounded_u32("max_queue_depth", self.max_queue_depth, 1, 4_194_304)?;
        bounded_u32(
            "max_in_flight_bytes",
            self.max_in_flight_bytes,
            1,
            67_108_864,
        )?;
        Ok(())
    }

    /// Validates surface-water bounds against one closed world column.
    ///
    /// # Errors
    ///
    /// Returns [`WorldgenError::InvalidHydrologyOccupancy`] when an enabled
    /// sea level is outside the inclusive world floor and ceiling.
    pub fn validate_for_world(&self, world: &WorldgenConfigV1) -> WorldgenResult<()> {
        self.validate()?;
        match self.sea_level_y {
            Some(sea_level_y)
                if sea_level_y < world.world_floor_y || sea_level_y > world.world_ceiling_y =>
            {
                Err(WorldgenError::InvalidHydrologyOccupancy {
                    field: "sea_level_y",
                    reason: format!(
                        "must stay inside world column {}..={}, got {sea_level_y}",
                        world.world_floor_y, world.world_ceiling_y
                    ),
                })
            }
            Some(_) | None => Ok(()),
        }
    }

    /// Returns canonical compact JSON with defaults materialized.
    ///
    /// # Errors
    ///
    /// Returns a canonical encoding error if serialization fails.
    pub fn canonical_bytes(&self) -> WorldgenResult<Vec<u8>> {
        canonical_json_bytes(self).map_err(|error| WorldgenError::CanonicalEncoding {
            kind: "HydrologyOccupancyConfigV1",
            reason: error.to_string(),
        })
    }
}

/// Frozen water/lava identities and placement predicates.
///
/// Concrete IDs come from the reopened lock and frozen Role/Predicate outputs.
/// This crate never defaults them to Terrenia identities.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HydrologyFluidBindingsV1 {
    water: StableId,
    lava: StableId,
    water_predicate: StableId,
    lava_predicate: StableId,
}

impl HydrologyFluidBindingsV1 {
    /// Validates frozen water and lava occupancy identities.
    ///
    /// # Errors
    ///
    /// Returns an error for the wrong registration kind, a missing predicate
    /// major, identical water and lava IDs, or a fluid identity that carries a
    /// contract major.
    pub fn new(
        water: StableId,
        lava: StableId,
        water_predicate: StableId,
        lava_predicate: StableId,
    ) -> WorldgenResult<Self> {
        validate_fluid("water", &water)?;
        validate_fluid("lava", &lava)?;
        validate_predicate("water", &water_predicate)?;
        validate_predicate("lava", &lava_predicate)?;
        if water == lava {
            return Err(WorldgenError::InvalidHydrologyOccupancy {
                field: "fluids",
                reason: "water and lava must resolve to distinct identities".to_owned(),
            });
        }
        Ok(Self {
            water,
            lava,
            water_predicate,
            lava_predicate,
        })
    }

    /// Returns the frozen water fluid identity.
    #[must_use]
    pub const fn water(&self) -> &StableId {
        &self.water
    }

    /// Returns the frozen lava fluid identity.
    #[must_use]
    pub const fn lava(&self) -> &StableId {
        &self.lava
    }

    /// Returns the frozen water placement predicate.
    #[must_use]
    pub const fn water_predicate(&self) -> &StableId {
        &self.water_predicate
    }

    /// Returns the frozen lava placement predicate.
    #[must_use]
    pub const fn lava_predicate(&self) -> &StableId {
        &self.lava_predicate
    }
}

/// Immutable inputs that enable V6 hydrology occupancy on a V5 natural plan.
#[derive(Clone, Debug)]
pub struct HydrologyOccupancyInputV1 {
    config: HydrologyOccupancyConfigV1,
    fluids: HydrologyFluidBindingsV1,
}

impl HydrologyOccupancyInputV1 {
    /// Creates a complete hydrology occupancy compilation input.
    #[must_use]
    pub const fn new(config: HydrologyOccupancyConfigV1, fluids: HydrologyFluidBindingsV1) -> Self {
        Self { config, fluids }
    }

    /// Returns the closed occupancy configuration.
    #[must_use]
    pub const fn config(&self) -> &HydrologyOccupancyConfigV1 {
        &self.config
    }

    /// Returns the frozen water/lava bindings.
    #[must_use]
    pub const fn fluids(&self) -> &HydrologyFluidBindingsV1 {
        &self.fluids
    }
}

/// Queryable aquifer table at one world column.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AquiferSampleV1 {
    basin: AquiferBasinIdV1,
    water_table_y: i32,
    lava_table_y: i32,
    present: bool,
}

impl AquiferSampleV1 {
    /// Returns the stable aquifer basin identity covering this column.
    #[must_use]
    pub const fn basin(self) -> AquiferBasinIdV1 {
        self.basin
    }

    /// Returns the inclusive water-table voxel Y.
    #[must_use]
    pub const fn water_table_y(self) -> i32 {
        self.water_table_y
    }

    /// Returns the inclusive lava-table voxel Y.
    #[must_use]
    pub const fn lava_table_y(self) -> i32 {
        self.lava_table_y
    }

    /// Returns whether this coarse cell hosts an aquifer body.
    #[must_use]
    pub const fn is_present(self) -> bool {
        self.present
    }
}

/// Queryable vertical drainage decision at one world column.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DrainageSampleV1 {
    link: DrainageLinkIdV1,
    connected: bool,
}

impl DrainageSampleV1 {
    /// Returns the stable drainage-link identity for this column.
    #[must_use]
    pub const fn link(self) -> DrainageLinkIdV1 {
        self.link
    }

    /// Returns whether the column drains a surface river into underground voids.
    #[must_use]
    pub const fn is_connected(self) -> bool {
        self.connected
    }
}

/// Closed initial-occupancy kinds. These are not fluid identities.
#[repr(u8)]
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum HydrologyOccupancyKindV1 {
    /// No fluid occupancy at this cell.
    Empty,
    /// Surface river channel above the incised bed.
    SurfaceChannel,
    /// Standing ocean water above terrain and at or below sea level.
    SurfaceWater,
    /// Underground drainage shaft beneath a river column.
    Drainage,
    /// Aquifer fill of a final cave void.
    Aquifer,
    /// Deep lava occupancy of a final cave void.
    LavaPool,
}

impl HydrologyOccupancyKindV1 {
    /// Returns the stable diagnostic token.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Empty => "empty",
            Self::SurfaceChannel => "surface-channel",
            Self::SurfaceWater => "surface-water",
            Self::Drainage => "drainage",
            Self::Aquifer => "aquifer",
            Self::LavaPool => "lava-pool",
        }
    }
}

/// Finite initial flow used by occupancy candidates. Dynamic flow is not run.
#[repr(u8)]
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum HydrologyFlowV1 {
    /// Standing source occupancy.
    Still,
    /// Negative Y occupancy into a same-fluid cell below.
    Down,
    /// Positive X. Reserved for later bounded flow; unused by initial occupancy.
    East,
    /// Negative X. Reserved for later bounded flow; unused by initial occupancy.
    West,
    /// Positive Z. Reserved for later bounded flow; unused by initial occupancy.
    South,
    /// Negative Z. Reserved for later bounded flow; unused by initial occupancy.
    North,
}

/// Locally queryable hydrology occupancy sample at one world voxel.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HydrologyOccupancySampleV1 {
    kind: HydrologyOccupancyKindV1,
    fluid: Option<StableId>,
    level: u8,
    flow: HydrologyFlowV1,
}

impl HydrologyOccupancySampleV1 {
    /// Returns the occupancy classification.
    #[must_use]
    pub const fn kind(&self) -> HydrologyOccupancyKindV1 {
        self.kind
    }

    /// Returns the frozen fluid identity when the cell is occupied.
    #[must_use]
    pub const fn fluid(&self) -> Option<&StableId> {
        self.fluid.as_ref()
    }

    /// Returns the initial source level. Always zero when occupied.
    #[must_use]
    pub const fn level(&self) -> u8 {
        self.level
    }

    /// Returns the explicit initial flow. Presentation must not infer it.
    #[must_use]
    pub const fn flow(&self) -> HydrologyFlowV1 {
        self.flow
    }

    /// Returns whether this sample occupies a fluid layer.
    #[must_use]
    pub const fn is_occupied(&self) -> bool {
        self.fluid.is_some()
    }
}

/// Sparse occupied cell stored on a hydrology occupancy candidate.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HydrologyOccupancyCellV1 {
    x: u16,
    y: u16,
    z: u16,
    kind: HydrologyOccupancyKindV1,
    fluid: StableId,
    level: u8,
    flow: HydrologyFlowV1,
}

impl HydrologyOccupancyCellV1 {
    /// Returns local X.
    #[must_use]
    pub const fn x(&self) -> u16 {
        self.x
    }

    /// Returns local Y.
    #[must_use]
    pub const fn y(&self) -> u16 {
        self.y
    }

    /// Returns local Z.
    #[must_use]
    pub const fn z(&self) -> u16 {
        self.z
    }

    /// Returns the occupancy classification.
    #[must_use]
    pub const fn kind(&self) -> HydrologyOccupancyKindV1 {
        self.kind
    }

    /// Returns the frozen fluid identity.
    #[must_use]
    pub const fn fluid(&self) -> &StableId {
        &self.fluid
    }

    /// Returns the initial source level.
    #[must_use]
    pub const fn level(&self) -> u8 {
        self.level
    }

    /// Returns the explicit initial flow.
    #[must_use]
    pub const fn flow(&self) -> HydrologyFlowV1 {
        self.flow
    }
}

/// Hard occupancy cell, queue, and byte counters for one candidate.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HydrologyAccountingV1 {
    cells_examined: u64,
    cells_occupied: u64,
    queue_depth: u64,
    in_flight_bytes: u64,
    max_cells: u64,
    max_queue_depth: u64,
    max_in_flight_bytes: u64,
}

impl HydrologyAccountingV1 {
    /// Returns examined voxel count.
    #[must_use]
    pub const fn cells_examined(self) -> u64 {
        self.cells_examined
    }

    /// Returns occupied fluid-cell count.
    #[must_use]
    pub const fn cells_occupied(self) -> u64 {
        self.cells_occupied
    }

    /// Returns drainage-frontier high water.
    #[must_use]
    pub const fn queue_depth(self) -> u64 {
        self.queue_depth
    }

    /// Returns accounted candidate bytes.
    #[must_use]
    pub const fn in_flight_bytes(self) -> u64 {
        self.in_flight_bytes
    }
}

/// Direction-independent occupancy continuity receipt for one shared chunk face.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HydrologyFaceContinuityV1 {
    first: ChunkCoordinate,
    second: ChunkCoordinate,
    axis: u8,
    occupancy_hash: CanonicalHash,
    occupied_cells: u32,
}

impl HydrologyFaceContinuityV1 {
    /// Returns the canonically lesser chunk of the shared boundary.
    #[must_use]
    pub const fn first(self) -> ChunkCoordinate {
        self.first
    }

    /// Returns the canonically greater chunk of the shared boundary.
    #[must_use]
    pub const fn second(self) -> ChunkCoordinate {
        self.second
    }

    /// Returns occupied cells on the shared face.
    #[must_use]
    pub const fn occupied_cells(self) -> u32 {
        self.occupied_cells
    }

    /// Returns the direction-independent occupancy hash.
    #[must_use]
    pub const fn occupancy_hash(self) -> CanonicalHash {
        self.occupancy_hash
    }

    /// Returns the shared-face axis: `0` for X, `1` for Y, `2` for Z.
    #[must_use]
    pub const fn axis(self) -> u8 {
        self.axis
    }
}

/// Versioned hydrology occupancy candidate. Not a storage snapshot envelope.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HydrologyOccupancyCandidateV1 {
    schema: &'static str,
    dimension: DimensionId,
    chunk: ChunkCoordinate,
    generation_epoch: GenerationEpochIdV1,
    generation_input_hash: GenerationInputHashV1,
    occupancy_hash: HydrologyOccupancyHashV1,
    water: StableId,
    lava: StableId,
    cells: Vec<HydrologyOccupancyCellV1>,
    accounting: HydrologyAccountingV1,
}

impl HydrologyOccupancyCandidateV1 {
    /// Returns the frozen candidate schema identity.
    #[must_use]
    pub const fn schema(&self) -> &'static str {
        self.schema
    }

    /// Returns the dimension of the candidate.
    #[must_use]
    pub const fn dimension(&self) -> &DimensionId {
        &self.dimension
    }

    /// Returns the chunk coordinate.
    #[must_use]
    pub const fn chunk(&self) -> ChunkCoordinate {
        self.chunk
    }

    /// Returns occupied cells in `(y, z, x)` order.
    #[must_use]
    pub fn cells(&self) -> &[HydrologyOccupancyCellV1] {
        &self.cells
    }

    /// Returns occupancy accounting captured for this candidate.
    #[must_use]
    pub const fn accounting(&self) -> HydrologyAccountingV1 {
        self.accounting
    }

    /// Returns the generation epoch captured when the candidate was built.
    #[must_use]
    pub const fn generation_epoch(&self) -> GenerationEpochIdV1 {
        self.generation_epoch
    }

    /// Returns the generation input hash captured when the candidate was built.
    #[must_use]
    pub const fn generation_input_hash(&self) -> GenerationInputHashV1 {
        self.generation_input_hash
    }

    /// Returns the occupancy-layer hash independent of snapshot bytes.
    #[must_use]
    pub const fn occupancy_hash(&self) -> HydrologyOccupancyHashV1 {
        self.occupancy_hash
    }

    /// Returns canonical compact JSON for the occupancy candidate.
    ///
    /// # Errors
    ///
    /// Returns a canonical encoding error if serialization fails.
    pub fn canonical_bytes(&self) -> WorldgenResult<Vec<u8>> {
        canonical_json_bytes(self).map_err(|error| WorldgenError::CanonicalEncoding {
            kind: "HydrologyOccupancyCandidateV1",
            reason: error.to_string(),
        })
    }
}

/// Compiled, allocation-light V6 occupancy sampler.
#[derive(Clone, Debug)]
pub(crate) struct HydrologySamplerV1 {
    seed_root: WorldgenSeedRootV2,
    occupancy_hash: HydrologyOccupancyHashV1,
    config: HydrologyOccupancyConfigV1,
    fluids: HydrologyFluidBindingsV1,
    world_floor_y: i32,
    world_ceiling_y: i32,
    sea_level_y: Option<i32>,
    river_incision_voxels: u16,
    underground_rivers: bool,
}

/// Hydrology facts that are invariant along one world `(x, z)` column.
#[derive(Clone, Copy, Debug)]
pub(crate) struct HydrologyColumnV1 {
    surface_y: i32,
    style: TerrainStyleV1,
    aquifer: AquiferSampleV1,
    drainage: DrainageSampleV1,
    river_channel: bool,
    standing_water_y: Option<i32>,
}

impl HydrologyColumnV1 {
    pub(crate) const fn surface_y(self) -> i32 {
        self.surface_y
    }
}

impl HydrologySamplerV1 {
    pub(crate) const fn bank_constrained_channels(&self) -> bool {
        self.config.bank_constrained_channels
    }

    pub(crate) const fn sea_level_y(&self) -> Option<i32> {
        self.sea_level_y
    }
    pub(crate) fn compile(
        seed_root: WorldgenSeedRootV2,
        spine: &WorldgenConfigV1,
        terrain: &TerrainConfigV2,
        natural: &NaturalSamplerV1,
        layer: HydrologyOccupancyInputV1,
    ) -> WorldgenResult<Self> {
        layer.config.validate_for_world(spine)?;
        let occupancy_hash = HydrologyOccupancyHashV1::from_hash(domain_hash(
            OCCUPANCY_DOMAIN,
            &[
                layer.config.canonical_bytes()?.as_slice(),
                terrain.canonical_bytes()?.as_slice(),
                layer.fluids.water.as_str().as_bytes(),
                layer.fluids.lava.as_str().as_bytes(),
                layer.fluids.water_predicate.as_str().as_bytes(),
                layer.fluids.lava_predicate.as_str().as_bytes(),
                natural
                    .hydrology_identity()
                    .provider_stable_id()
                    .as_str()
                    .as_bytes(),
                natural
                    .hydrology_identity()
                    .implementation_fingerprint()
                    .as_bytes(),
            ],
        ));
        Ok(Self {
            seed_root,
            occupancy_hash,
            config: layer.config,
            fluids: layer.fluids,
            world_floor_y: spine.world_floor_y,
            world_ceiling_y: spine.world_ceiling_y,
            sea_level_y: layer.config.sea_level_y,
            river_incision_voxels: natural.config().river_incision_voxels,
            underground_rivers: terrain.water.underground_rivers,
        })
    }

    pub(crate) const fn occupancy_hash(&self) -> HydrologyOccupancyHashV1 {
        self.occupancy_hash
    }

    pub(crate) const fn fluids(&self) -> &HydrologyFluidBindingsV1 {
        &self.fluids
    }

    pub(crate) fn aquifer_sample(&self, x: i64, z: i64, surface_y: i32) -> AquiferSampleV1 {
        let edge = i64::from(self.config.aquifer_cell_edge_voxels.max(1));
        let cell_x = x.div_euclid(edge);
        let cell_z = z.div_euclid(edge);
        let basin = AquiferBasinIdV1::from_hash(domain_hash(
            AQUIFER_DOMAIN,
            &[
                self.seed_root.as_bytes(),
                &cell_x.to_be_bytes(),
                &cell_z.to_be_bytes(),
            ],
        ));
        let lava_table_y = self
            .world_floor_y
            .saturating_add(i32::from(self.config.lava_column_height_voxels).saturating_sub(1));
        let water_table_y = surface_y
            .saturating_sub(i32::from(self.config.aquifer_depth_voxels))
            .max(lava_table_y.saturating_add(1))
            .min(self.world_ceiling_y);
        let rank = hash_u64(
            AQUIFER_DOMAIN,
            &[
                self.seed_root.as_bytes(),
                &cell_x.to_be_bytes(),
                &cell_z.to_be_bytes(),
            ],
        );
        AquiferSampleV1 {
            basin,
            water_table_y,
            lava_table_y,
            present: rank % 1_024 < u64::from(self.config.aquifer_threshold_per_1024),
        }
    }

    pub(crate) fn drainage_sample(
        &self,
        x: i64,
        z: i64,
        river: Option<RiverSampleV1>,
    ) -> DrainageSampleV1 {
        let connected = self.underground_rivers && river.is_some_and(RiverSampleV1::in_channel);
        let basin_bytes = river.map_or([0_u8; 32], |sample| *sample.basin().as_bytes());
        let link = DrainageLinkIdV1::from_hash(domain_hash(
            DRAINAGE_DOMAIN,
            &[
                self.seed_root.as_bytes(),
                &x.to_be_bytes(),
                &z.to_be_bytes(),
                &basin_bytes,
            ],
        ));
        DrainageSampleV1 { link, connected }
    }

    #[allow(
        clippy::too_many_arguments,
        reason = "occupancy stays a pure coordinate query of height, cave, river, and style"
    )]
    pub(crate) fn occupy(
        &self,
        x: i64,
        y: i64,
        z: i64,
        surface_y: i32,
        cave_allows_fluid: bool,
        river: Option<RiverSampleV1>,
        style: TerrainStyleV1,
        surface_water_y: Option<i32>,
    ) -> HydrologyOccupancySampleV1 {
        let column = self.column(x, z, surface_y, river, style, surface_water_y);
        self.occupy_column(x, y, z, cave_allows_fluid, column)
    }

    pub(crate) fn column(
        &self,
        x: i64,
        z: i64,
        surface_y: i32,
        river: Option<RiverSampleV1>,
        style: TerrainStyleV1,
        surface_water_y: Option<i32>,
    ) -> HydrologyColumnV1 {
        let river_channel = river.is_some_and(RiverSampleV1::in_channel);
        HydrologyColumnV1 {
            surface_y,
            style,
            aquifer: self.aquifer_sample(x, z, surface_y),
            drainage: self.drainage_sample(x, z, river),
            river_channel,
            standing_water_y: maximum_optional(self.sea_level_y, surface_water_y),
        }
    }

    pub(crate) fn occupy_column(
        &self,
        x: i64,
        y: i64,
        z: i64,
        cave_allows_fluid: bool,
        column: HydrologyColumnV1,
    ) -> HydrologyOccupancySampleV1 {
        if y < i64::from(self.world_floor_y) || y > i64::from(self.world_ceiling_y) {
            return empty_sample();
        }
        let kind = occupancy_kind(
            y,
            self.river_incision_voxels,
            cave_allows_fluid,
            column,
            self.lava_occupies(x, y, z, column.style, column.aquifer.lava_table_y),
        );
        match kind {
            HydrologyOccupancyKindV1::Empty => empty_sample(),
            HydrologyOccupancyKindV1::LavaPool => HydrologyOccupancySampleV1 {
                kind,
                fluid: Some(self.fluids.lava.clone()),
                level: 0,
                flow: self.initial_flow(x, y, z, cave_allows_fluid, column, kind),
            },
            HydrologyOccupancyKindV1::SurfaceChannel
            | HydrologyOccupancyKindV1::SurfaceWater
            | HydrologyOccupancyKindV1::Drainage
            | HydrologyOccupancyKindV1::Aquifer => HydrologyOccupancySampleV1 {
                kind,
                fluid: Some(self.fluids.water.clone()),
                level: 0,
                flow: self.initial_flow(x, y, z, cave_allows_fluid, column, kind),
            },
        }
    }

    fn initial_flow(
        &self,
        x: i64,
        y: i64,
        z: i64,
        cave_allows_fluid: bool,
        column: HydrologyColumnV1,
        kind: HydrologyOccupancyKindV1,
    ) -> HydrologyFlowV1 {
        if matches!(
            kind,
            HydrologyOccupancyKindV1::SurfaceChannel | HydrologyOccupancyKindV1::SurfaceWater
        ) {
            return HydrologyFlowV1::Still;
        }
        let below = y.saturating_sub(1);
        if below < i64::from(self.world_floor_y) {
            return HydrologyFlowV1::Still;
        }
        let below_kind = occupancy_kind(
            below,
            self.river_incision_voxels,
            cave_allows_fluid,
            column,
            self.lava_occupies(x, below, z, column.style, column.aquifer.lava_table_y),
        );
        if fluid_family(kind) == fluid_family(below_kind)
            && below_kind != HydrologyOccupancyKindV1::Empty
        {
            HydrologyFlowV1::Down
        } else {
            HydrologyFlowV1::Still
        }
    }

    fn lava_occupies(
        &self,
        x: i64,
        y: i64,
        z: i64,
        style: TerrainStyleV1,
        lava_table_y: i32,
    ) -> bool {
        if y > i64::from(lava_table_y) {
            return false;
        }
        if style == TerrainStyleV1::AridBadlands {
            return true;
        }
        let rank = hash_u64(
            LAVA_DOMAIN,
            &[
                self.seed_root.as_bytes(),
                &x.to_be_bytes(),
                &y.to_be_bytes(),
                &z.to_be_bytes(),
            ],
        );
        rank % 1_024 < u64::from(self.config.lava_threshold_per_1024)
    }

    pub(crate) fn candidate(
        &self,
        dimension: DimensionId,
        chunk: ChunkCoordinate,
        generation_epoch: GenerationEpochIdV1,
        generation_input_hash: GenerationInputHashV1,
        mut cells: Vec<HydrologyOccupancyCellV1>,
        mut accounting: HydrologyAccountingV1,
    ) -> WorldgenResult<HydrologyOccupancyCandidateV1> {
        cells.sort_by(|left, right| {
            left.y
                .cmp(&right.y)
                .then(left.z.cmp(&right.z))
                .then(left.x.cmp(&right.x))
        });
        for pair in cells.windows(2) {
            if pair[0].x == pair[1].x && pair[0].y == pair[1].y && pair[0].z == pair[1].z {
                return Err(WorldgenError::InvalidHydrologyOccupancy {
                    field: "cells",
                    reason: "duplicate occupancy cell coordinate".to_owned(),
                });
            }
        }
        let mut candidate = HydrologyOccupancyCandidateV1 {
            schema: CANDIDATE_SCHEMA,
            dimension,
            chunk,
            generation_epoch,
            generation_input_hash,
            occupancy_hash: self.occupancy_hash,
            water: self.fluids.water.clone(),
            lava: self.fluids.lava.clone(),
            cells,
            accounting,
        };
        let bytes = candidate.canonical_bytes()?;
        let byte_len =
            u64::try_from(bytes.len()).map_err(|_| WorldgenError::ArithmeticOverflow {
                operation: "hydrology occupancy candidate bytes",
            })?;
        accounting.in_flight_bytes = byte_len;
        if byte_len > accounting.max_in_flight_bytes {
            return Err(WorldgenError::BudgetExceeded {
                budget: "hydrology occupancy in-flight bytes",
                required: byte_len,
                limit: accounting.max_in_flight_bytes,
            });
        }
        candidate.accounting = accounting;
        Ok(candidate)
    }

    pub(crate) fn face_continuity(
        coordinate: ChunkCoordinate,
        neighbor: ChunkCoordinate,
        axis: u8,
        occupancy_hash: CanonicalHash,
        occupied_cells: u32,
    ) -> HydrologyFaceContinuityV1 {
        let (first, second) = if coordinate < neighbor {
            (coordinate, neighbor)
        } else {
            (neighbor, coordinate)
        };
        HydrologyFaceContinuityV1 {
            first,
            second,
            axis,
            occupancy_hash,
            occupied_cells,
        }
    }

    pub(crate) fn start_accounting(&self) -> HydrologyAccountingV1 {
        HydrologyAccountingV1 {
            cells_examined: 0,
            cells_occupied: 0,
            queue_depth: 0,
            in_flight_bytes: 0,
            max_cells: u64::from(self.config.max_cells_per_chunk),
            max_queue_depth: u64::from(self.config.max_queue_depth),
            max_in_flight_bytes: u64::from(self.config.max_in_flight_bytes),
        }
    }

    pub(crate) fn examine(accounting: &mut HydrologyAccountingV1, count: u64) {
        accounting.cells_examined = accounting.cells_examined.saturating_add(count);
    }

    pub(crate) fn occupy_cell(
        accounting: &mut HydrologyAccountingV1,
        drainage_frontier: bool,
    ) -> WorldgenResult<()> {
        accounting.cells_occupied = accounting.cells_occupied.saturating_add(1);
        if accounting.cells_occupied > accounting.max_cells {
            return Err(WorldgenError::BudgetExceeded {
                budget: "hydrology occupancy cells",
                required: accounting.cells_occupied,
                limit: accounting.max_cells,
            });
        }
        if drainage_frontier {
            accounting.queue_depth = accounting.queue_depth.saturating_add(1);
            if accounting.queue_depth > accounting.max_queue_depth {
                return Err(WorldgenError::BudgetExceeded {
                    budget: "hydrology occupancy queue depth",
                    required: accounting.queue_depth,
                    limit: accounting.max_queue_depth,
                });
            }
        }
        Ok(())
    }

    pub(crate) fn occupancy_cell(
        local_x: u16,
        local_y: u16,
        local_z: u16,
        sample: &HydrologyOccupancySampleV1,
    ) -> Option<HydrologyOccupancyCellV1> {
        let fluid = sample.fluid.clone()?;
        Some(HydrologyOccupancyCellV1 {
            x: local_x,
            y: local_y,
            z: local_z,
            kind: sample.kind,
            fluid,
            level: sample.level,
            flow: sample.flow,
        })
    }
}

pub(crate) fn hydrology_face_axis(face: ChunkFaceV1) -> u8 {
    match face {
        ChunkFaceV1::NegativeX | ChunkFaceV1::PositiveX => 0,
        ChunkFaceV1::NegativeY | ChunkFaceV1::PositiveY => 1,
        ChunkFaceV1::NegativeZ | ChunkFaceV1::PositiveZ => 2,
    }
}

pub(crate) fn hydrology_adjacent_chunk(
    coordinate: ChunkCoordinate,
    face: ChunkFaceV1,
) -> WorldgenResult<ChunkCoordinate> {
    let (x, y, z) = match face {
        ChunkFaceV1::NegativeX => (
            coordinate.x.checked_sub(1),
            Some(coordinate.y),
            Some(coordinate.z),
        ),
        ChunkFaceV1::PositiveX => (
            coordinate.x.checked_add(1),
            Some(coordinate.y),
            Some(coordinate.z),
        ),
        ChunkFaceV1::NegativeY => (
            Some(coordinate.x),
            coordinate.y.checked_sub(1),
            Some(coordinate.z),
        ),
        ChunkFaceV1::PositiveY => (
            Some(coordinate.x),
            coordinate.y.checked_add(1),
            Some(coordinate.z),
        ),
        ChunkFaceV1::NegativeZ => (
            Some(coordinate.x),
            Some(coordinate.y),
            coordinate.z.checked_sub(1),
        ),
        ChunkFaceV1::PositiveZ => (
            Some(coordinate.x),
            Some(coordinate.y),
            coordinate.z.checked_add(1),
        ),
    };
    Ok(ChunkCoordinate::new(
        x.ok_or(WorldgenError::ArithmeticOverflow {
            operation: "hydrology shared-face neighbor X",
        })?,
        y.ok_or(WorldgenError::ArithmeticOverflow {
            operation: "hydrology shared-face neighbor Y",
        })?,
        z.ok_or(WorldgenError::ArithmeticOverflow {
            operation: "hydrology shared-face neighbor Z",
        })?,
    ))
}

pub(crate) fn hydrology_face_hash(
    samples: &[(u16, u16, HydrologyOccupancyKindV1, u8, u8)],
) -> CanonicalHash {
    let mut parts = Vec::with_capacity(samples.len().saturating_mul(5));
    let mut packed = Vec::with_capacity(samples.len().saturating_mul(6));
    for (u, v, kind, level, flow) in samples {
        packed.extend_from_slice(&u.to_be_bytes());
        packed.extend_from_slice(&v.to_be_bytes());
        packed.push(*kind as u8);
        packed.push(*level);
        packed.push(*flow);
    }
    parts.push(packed.as_slice());
    domain_hash(FACE_DOMAIN, &parts)
}

fn occupancy_kind(
    y: i64,
    incision: u16,
    cave_allows_fluid: bool,
    column: HydrologyColumnV1,
    lava: bool,
) -> HydrologyOccupancyKindV1 {
    let surface = i64::from(column.surface_y);
    let channel_top = surface.saturating_add(i64::from(incision));
    if lava && cave_allows_fluid && y <= i64::from(column.aquifer.lava_table_y) {
        return HydrologyOccupancyKindV1::LavaPool;
    }
    if y > surface && y <= channel_top && column.river_channel {
        return HydrologyOccupancyKindV1::SurfaceChannel;
    }
    if column
        .standing_water_y
        .is_some_and(|water_y| y > surface && y <= i64::from(water_y))
    {
        return HydrologyOccupancyKindV1::SurfaceWater;
    }
    if cave_allows_fluid && y <= surface && y > i64::from(column.aquifer.lava_table_y) {
        if column.drainage.is_connected() {
            return HydrologyOccupancyKindV1::Drainage;
        }
        if column.aquifer.present && y <= i64::from(column.aquifer.water_table_y) {
            return HydrologyOccupancyKindV1::Aquifer;
        }
    }
    HydrologyOccupancyKindV1::Empty
}

const fn maximum_optional(left: Option<i32>, right: Option<i32>) -> Option<i32> {
    match (left, right) {
        (Some(left), Some(right)) => Some(if left > right { left } else { right }),
        (Some(value), None) | (None, Some(value)) => Some(value),
        (None, None) => None,
    }
}

fn fluid_family(kind: HydrologyOccupancyKindV1) -> u8 {
    match kind {
        HydrologyOccupancyKindV1::Empty => 0,
        HydrologyOccupancyKindV1::SurfaceChannel
        | HydrologyOccupancyKindV1::SurfaceWater
        | HydrologyOccupancyKindV1::Drainage
        | HydrologyOccupancyKindV1::Aquifer => 1,
        HydrologyOccupancyKindV1::LavaPool => 2,
    }
}

fn empty_sample() -> HydrologyOccupancySampleV1 {
    HydrologyOccupancySampleV1 {
        kind: HydrologyOccupancyKindV1::Empty,
        fluid: None,
        level: 0,
        flow: HydrologyFlowV1::Still,
    }
}

fn validate_fluid(purpose: &'static str, id: &StableId) -> WorldgenResult<()> {
    if id.kind() != "fluid" {
        return Err(WorldgenError::InvalidHydrologyFluidIdentity {
            purpose,
            id: id.clone(),
            expected: "fluid",
        });
    }
    if id.major().is_some() {
        return Err(WorldgenError::InvalidHydrologyOccupancy {
            field: purpose,
            reason: format!("fluid identity `{id}` must not carry a major suffix"),
        });
    }
    Ok(())
}

fn validate_predicate(purpose: &'static str, id: &StableId) -> WorldgenResult<()> {
    if id.kind() != "predicate" {
        return Err(WorldgenError::InvalidHydrologyFluidIdentity {
            purpose,
            id: id.clone(),
            expected: "predicate",
        });
    }
    if id.major().is_none() {
        return Err(WorldgenError::InvalidHydrologyOccupancy {
            field: purpose,
            reason: format!("predicate `{id}` requires a major suffix"),
        });
    }
    Ok(())
}

fn bounded_u16(field: &'static str, value: u16, minimum: u16, maximum: u16) -> WorldgenResult<()> {
    if (minimum..=maximum).contains(&value) {
        Ok(())
    } else {
        Err(WorldgenError::InvalidHydrologyOccupancy {
            field,
            reason: format!("must be in {minimum}..={maximum}, got {value}"),
        })
    }
}

fn bounded_u32(field: &'static str, value: u32, minimum: u32, maximum: u32) -> WorldgenResult<()> {
    if (minimum..=maximum).contains(&value) {
        Ok(())
    } else {
        Err(WorldgenError::InvalidHydrologyOccupancy {
            field,
            reason: format!("must be in {minimum}..={maximum}, got {value}"),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn occupancy_config_rejects_zero_cell_budget() {
        let config = HydrologyOccupancyConfigV1 {
            max_cells_per_chunk: 0,
            ..HydrologyOccupancyConfigV1::default()
        };
        assert!(matches!(
            config.validate(),
            Err(WorldgenError::InvalidHydrologyOccupancy {
                field: "max_cells_per_chunk",
                ..
            })
        ));
    }

    #[test]
    fn fluid_bindings_reject_identical_water_and_lava() {
        let water: StableId = "fixture:fluid/water".parse().expect("water id");
        let predicate: StableId = "fixture:predicate/place-water@1"
            .parse()
            .expect("predicate id");
        let error =
            HydrologyFluidBindingsV1::new(water.clone(), water, predicate.clone(), predicate)
                .expect_err("identical fluids must fail closed");
        assert!(matches!(
            error,
            WorldgenError::InvalidHydrologyOccupancy {
                field: "fluids",
                ..
            }
        ));
    }
}
