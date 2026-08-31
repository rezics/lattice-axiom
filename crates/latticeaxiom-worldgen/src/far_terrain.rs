use std::{collections::BTreeSet, mem::size_of};

use latticeaxiom_core::{CanonicalHash, canonical_json_bytes};
use latticeaxiom_storage::{ChunkRevision, DimensionId};
use serde::{Deserialize, Serialize};

use crate::{
    D4MaterialRoleV1, GenerationEpochIdV1, GenerationInputHashV1, GenerationPlanV1,
    GenerationProvenanceHashV1, GeneratorFingerprintV1, PlanActivationIdV1, SnapshotChecksumV1,
    WorldSeedV1, WorldgenError, WorldgenResult, hashes::concatenated_hash,
};

const FAR_TERRAIN_CACHE_KEY_DOMAIN_V1: &[u8] = b"latticeaxiom.far-terrain.cache-key.v1\0";

/// Current deterministic far-terrain sampling and shell algorithm revision.
pub const FAR_TERRAIN_MESH_ALGORITHM_REVISION_V1: u32 = 1;

/// Highest LOD accepted by the bounded version-one tile contract.
pub const MAX_FAR_TERRAIN_LOD_LEVEL_V1: u8 = 3;

/// Largest base tile edge accepted by the bounded version-one tile contract.
/// At the maximum LOD this caps one build at `513 * 513` dense samples.
pub const MAX_FAR_TERRAIN_BASE_TILE_EDGE_V1: u16 = 64;

/// Horizontal coordinate of one far-terrain tile within its LOD level.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FarTerrainTileCoordinateV1 {
    x: i32,
    z: i32,
}

impl FarTerrainTileCoordinateV1 {
    /// Creates one signed horizontal tile coordinate.
    #[must_use]
    pub const fn new(x: i32, z: i32) -> Self {
        Self { x, z }
    }

    /// Returns the tile X coordinate.
    #[must_use]
    pub const fn x(self) -> i32 {
        self.x
    }

    /// Returns the tile Z coordinate.
    #[must_use]
    pub const fn z(self) -> i32 {
        self.z
    }
}

/// Bounded far-terrain LOD level. Level zero samples every voxel column;
/// each subsequent level doubles both sample spacing and tile span.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct FarTerrainLodLevelV1(u8);

impl FarTerrainLodLevelV1 {
    /// Creates a supported far-terrain LOD level.
    #[must_use]
    pub const fn new(level: u8) -> Option<Self> {
        if level <= MAX_FAR_TERRAIN_LOD_LEVEL_V1 {
            Some(Self(level))
        } else {
            None
        }
    }

    /// Returns the zero-based LOD level.
    #[must_use]
    pub const fn get(self) -> u8 {
        self.0
    }

    /// Returns the voxel-column spacing between retained surface vertices.
    #[must_use]
    pub const fn sample_step_voxels(self) -> u16 {
        1_u16 << self.0
    }
}

impl<'de> Deserialize<'de> for FarTerrainLodLevelV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let level = u8::deserialize(deserializer)?;
        Self::new(level).ok_or_else(|| {
            serde::de::Error::custom(format_args!(
                "far-terrain LOD {level} exceeds maximum {MAX_FAR_TERRAIN_LOD_LEVEL_V1}"
            ))
        })
    }
}

/// Coordinate plus LOD identity of one hierarchical far-terrain tile.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FarTerrainTileAddressV1 {
    coordinate: FarTerrainTileCoordinateV1,
    lod: FarTerrainLodLevelV1,
}

impl FarTerrainTileAddressV1 {
    /// Creates one hierarchical tile address.
    #[must_use]
    pub const fn new(coordinate: FarTerrainTileCoordinateV1, lod: FarTerrainLodLevelV1) -> Self {
        Self { coordinate, lod }
    }

    /// Returns the signed tile coordinate.
    #[must_use]
    pub const fn coordinate(self) -> FarTerrainTileCoordinateV1 {
        self.coordinate
    }

    /// Returns the tile LOD.
    #[must_use]
    pub const fn lod(self) -> FarTerrainLodLevelV1 {
        self.lod
    }

    fn span_voxels(self, base_tile_edge_voxels: u16) -> WorldgenResult<u16> {
        base_tile_edge_voxels
            .checked_mul(self.lod.sample_step_voxels())
            .ok_or(WorldgenError::ArithmeticOverflow {
                operation: "far-terrain tile span",
            })
    }

    fn world_origin(self, base_tile_edge_voxels: u16) -> WorldgenResult<(i64, i64)> {
        let span = i64::from(self.span_voxels(base_tile_edge_voxels)?);
        let x = i64::from(self.coordinate.x).checked_mul(span).ok_or(
            WorldgenError::ArithmeticOverflow {
                operation: "far-terrain tile origin X",
            },
        )?;
        let z = i64::from(self.coordinate.z).checked_mul(span).ok_or(
            WorldgenError::ArithmeticOverflow {
                operation: "far-terrain tile origin Z",
            },
        )?;
        Ok((x, z))
    }
}

/// Procedural source identity for a far tile derived without materializing
/// authoritative chunks.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FarTerrainProceduralProvenanceV1 {
    dimension: DimensionId,
    world_seed: WorldSeedV1,
    generation_epoch: GenerationEpochIdV1,
    plan_activation_id: PlanActivationIdV1,
    generator_fingerprint: GeneratorFingerprintV1,
    generation_input_hash: GenerationInputHashV1,
    generation_provenance_hash: GenerationProvenanceHashV1,
}

impl FarTerrainProceduralProvenanceV1 {
    /// Captures every procedural identity required to reject stale or foreign
    /// tile results.
    #[must_use]
    pub fn from_plan(plan: &GenerationPlanV1) -> Self {
        Self {
            dimension: plan.dimension().clone(),
            world_seed: plan.world_seed(),
            generation_epoch: plan.generation_epoch(),
            plan_activation_id: plan.plan_activation_id(),
            generator_fingerprint: plan.generator_fingerprint(),
            generation_input_hash: plan.generation_input_hash(),
            generation_provenance_hash: plan.generation_provenance_hash(),
        }
    }
}

/// Committed source identity supplied by the authoritative snapshot adapter.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FarTerrainCommittedProvenanceV1 {
    dimension: DimensionId,
    revision: ChunkRevision,
    checksum: SnapshotChecksumV1,
}

impl FarTerrainCommittedProvenanceV1 {
    /// Creates committed provenance from an authority-verified revision and
    /// exact source checksum.
    #[must_use]
    pub const fn new(
        dimension: DimensionId,
        revision: ChunkRevision,
        checksum: SnapshotChecksumV1,
    ) -> Self {
        Self {
            dimension,
            revision,
            checksum,
        }
    }
}

/// Source authority used to derive a presentation-only far tile.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum FarTerrainSourceProvenanceV1 {
    /// The exact generation plan can answer the surface query directly.
    Procedural(FarTerrainProceduralProvenanceV1),
    /// An authoritative committed surface snapshot supplied the samples.
    Committed(FarTerrainCommittedProvenanceV1),
}

/// Complete cache identity for one far tile.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FarTerrainCacheKeyV1 {
    source: FarTerrainSourceProvenanceV1,
    address: FarTerrainTileAddressV1,
    mesh_algorithm_revision: u32,
}

impl FarTerrainCacheKeyV1 {
    /// Creates a cache key using the current shell algorithm revision.
    #[must_use]
    pub const fn new(
        source: FarTerrainSourceProvenanceV1,
        address: FarTerrainTileAddressV1,
    ) -> Self {
        Self {
            source,
            address,
            mesh_algorithm_revision: FAR_TERRAIN_MESH_ALGORITHM_REVISION_V1,
        }
    }

    /// Returns the hierarchical tile address.
    #[must_use]
    pub const fn address(&self) -> FarTerrainTileAddressV1 {
        self.address
    }

    /// Returns the exact procedural or committed source identity.
    #[must_use]
    pub const fn source(&self) -> &FarTerrainSourceProvenanceV1 {
        &self.source
    }

    /// Returns the output-affecting shell algorithm revision.
    #[must_use]
    pub const fn mesh_algorithm_revision(&self) -> u32 {
        self.mesh_algorithm_revision
    }

    /// Returns a domain-separated hash of the exact canonical cache key.
    ///
    /// # Errors
    ///
    /// Returns an encoding error if the validated key cannot be serialized.
    pub fn canonical_hash(&self) -> WorldgenResult<CanonicalHash> {
        let bytes =
            canonical_json_bytes(self).map_err(|error| WorldgenError::CanonicalEncoding {
                kind: "far-terrain cache key",
                reason: error.to_string(),
            })?;
        Ok(concatenated_hash(
            FAR_TERRAIN_CACHE_KEY_DOMAIN_V1,
            &[bytes.as_slice()],
        ))
    }
}

/// Final authoritative surface facts for one horizontal voxel column.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FarTerrainSurfaceSampleV1 {
    solid_y: i32,
    material: D4MaterialRoleV1,
    water_y: Option<i32>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FarTerrainSurfaceSampleWireV1 {
    solid_y: i32,
    material: D4MaterialRoleV1,
    water_y: Option<i32>,
}

impl<'de> Deserialize<'de> for FarTerrainSurfaceSampleV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let wire = FarTerrainSurfaceSampleWireV1::deserialize(deserializer)?;
        if wire.water_y.is_some_and(|water_y| water_y <= wire.solid_y) {
            return Err(serde::de::Error::custom(
                "far-terrain water surface must be above the solid surface",
            ));
        }
        Ok(Self::new(wire.solid_y, wire.material, wire.water_y))
    }
}

impl FarTerrainSurfaceSampleV1 {
    /// Creates a final surface sample. Water at or below solid is discarded.
    #[must_use]
    pub const fn new(solid_y: i32, material: D4MaterialRoleV1, water_y: Option<i32>) -> Self {
        let water_y = match water_y {
            Some(level) if level > solid_y => Some(level),
            _ => None,
        };
        Self {
            solid_y,
            material,
            water_y,
        }
    }

    /// Returns the final solid surface height.
    #[must_use]
    pub const fn solid_y(self) -> i32 {
        self.solid_y
    }

    /// Returns the final surface material role.
    #[must_use]
    pub const fn material(self) -> D4MaterialRoleV1 {
        self.material
    }

    /// Returns standing water above the solid surface.
    #[must_use]
    pub const fn water_y(self) -> Option<i32> {
        self.water_y
    }
}

/// Pure, order-independent final-surface query used by the far-tile builder.
pub trait FarTerrainSurfaceSourceV1 {
    /// Samples one world-space horizontal voxel column.
    ///
    /// # Errors
    ///
    /// Returns a typed source error when the final surface is unavailable.
    fn sample_far_terrain_surface(
        &self,
        world_x: i64,
        world_z: i64,
    ) -> WorldgenResult<FarTerrainSurfaceSampleV1>;
}

impl FarTerrainSurfaceSourceV1 for GenerationPlanV1 {
    fn sample_far_terrain_surface(
        &self,
        world_x: i64,
        world_z: i64,
    ) -> WorldgenResult<FarTerrainSurfaceSampleV1> {
        self.far_terrain_surface_sample(world_x, world_z)
    }
}

/// One retained height/material/water vertex in the far shell.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FarTerrainVertexV1 {
    /// Tile-local X in voxel columns.
    pub local_x: u16,
    /// Final solid surface height.
    pub solid_y: i32,
    /// Tile-local Z in voxel columns.
    pub local_z: u16,
    /// Final surface material role.
    pub material: D4MaterialRoleV1,
}

/// One retained sample in the separate water-surface lane.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FarTerrainWaterVertexV1 {
    /// Tile-local X in voxel columns.
    pub local_x: u16,
    /// Tile-local Z in voxel columns.
    pub local_z: u16,
    /// Standing water above the solid surface, or no water at this sample.
    pub surface_y: Option<i32>,
}

/// One stable top-surface triangle using indices into the vertex grid.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(transparent)]
pub struct FarTerrainTriangleV1(pub [u32; 3]);

/// Exact dense-source bounds and maximum bilinear simplification error for one
/// retained cell.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FarTerrainCellEnvelopeV1 {
    /// Minimum dense final-surface height in the cell.
    pub min_solid_y: i32,
    /// Maximum dense final-surface height in the cell.
    pub max_solid_y: i32,
    /// Maximum ceil-rounded absolute deviation from the retained bilinear patch.
    pub max_error_voxels: u16,
    /// Minimum standing-water level in the cell, if any.
    pub min_water_y: Option<i32>,
    /// Maximum standing-water level in the cell, if any.
    pub max_water_y: Option<i32>,
}

/// Orientation of one retained vertical cliff wall.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum FarTerrainWallAxisV1 {
    /// Wall lies on a constant-X boundary.
    X,
    /// Wall lies on a constant-Z boundary.
    Z,
}

/// One deterministic representative vertical wall that prevents a cell's
/// strongest cliff or terrace from collapsing into a smooth ramp.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FarTerrainWallV1 {
    /// Coarse cell X within the tile.
    pub cell_x: u16,
    /// Coarse cell Z within the tile.
    pub cell_z: u16,
    /// Wall orientation.
    pub axis: FarTerrainWallAxisV1,
    /// Exact tile-local coordinate of the wall plane.
    pub plane_offset: u16,
    /// Exact tile-local coordinate of the retained one-voxel wall segment.
    pub segment_offset: u16,
    /// Lower solid surface height.
    pub bottom_y: i32,
    /// Upper solid surface height.
    pub top_y: i32,
    /// Material of the higher side.
    pub material: D4MaterialRoleV1,
}

/// Outer edge of a far tile.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum FarTerrainBorderSideV1 {
    /// Negative X edge.
    West,
    /// Positive X edge.
    East,
    /// Negative Z edge.
    North,
    /// Positive Z edge.
    South,
}

/// Downward border closure used to hide cross-LOD transition gaps.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FarTerrainBorderSkirtV1 {
    /// Tile border.
    pub side: FarTerrainBorderSideV1,
    /// Coarse segment index along that border.
    pub segment: u16,
    /// First retained edge height.
    pub first_top_y: i32,
    /// Second retained edge height.
    pub second_top_y: i32,
    /// Shared downward closure height.
    pub bottom_y: i32,
}

/// Complete pure-data shell. Solid and water facts remain separate so a later
/// renderer can assign exactly one water owner through near/far overlap.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FarTerrainShellV1 {
    vertices: Vec<FarTerrainVertexV1>,
    water_vertices: Vec<FarTerrainWaterVertexV1>,
    top_triangles: Vec<FarTerrainTriangleV1>,
    cell_envelopes: Vec<FarTerrainCellEnvelopeV1>,
    cliff_walls: Vec<FarTerrainWallV1>,
    border_skirts: Vec<FarTerrainBorderSkirtV1>,
}

impl FarTerrainShellV1 {
    /// Returns the retained `(edge + 1)^2` vertex grid in row-major Z/X order.
    #[must_use]
    pub fn vertices(&self) -> &[FarTerrainVertexV1] {
        &self.vertices
    }

    /// Returns the separate water sample grid in the same row-major layout.
    #[must_use]
    pub fn water_vertices(&self) -> &[FarTerrainWaterVertexV1] {
        &self.water_vertices
    }

    /// Returns stable top-surface triangles.
    #[must_use]
    pub fn top_triangles(&self) -> &[FarTerrainTriangleV1] {
        &self.top_triangles
    }

    /// Returns dense-source envelopes in row-major cell order.
    #[must_use]
    pub fn cell_envelopes(&self) -> &[FarTerrainCellEnvelopeV1] {
        &self.cell_envelopes
    }

    /// Returns strongest retained cliff walls, sorted by cell then axis.
    #[must_use]
    pub fn cliff_walls(&self) -> &[FarTerrainWallV1] {
        &self.cliff_walls
    }

    /// Returns stable outer closure segments.
    #[must_use]
    pub fn border_skirts(&self) -> &[FarTerrainBorderSkirtV1] {
        &self.border_skirts
    }
}

/// One provenance-keyed deterministic far-terrain tile.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FarTerrainTileV1 {
    cache_key: FarTerrainCacheKeyV1,
    base_tile_edge_voxels: u16,
    sample_step_voxels: u16,
    world_origin_x: i64,
    world_origin_z: i64,
    shell: FarTerrainShellV1,
}

impl FarTerrainTileV1 {
    /// Returns the exact cache identity.
    #[must_use]
    pub const fn cache_key(&self) -> &FarTerrainCacheKeyV1 {
        &self.cache_key
    }

    /// Returns the tile-local shell.
    #[must_use]
    pub const fn shell(&self) -> &FarTerrainShellV1 {
        &self.shell
    }

    /// Returns the inclusive world-space X/Z origin.
    #[must_use]
    pub const fn world_origin_xz(&self) -> [i64; 2] {
        [self.world_origin_x, self.world_origin_z]
    }

    /// Returns the retained vertex-cell edge before LOD spacing is applied.
    #[must_use]
    pub const fn base_tile_edge_voxels(&self) -> u16 {
        self.base_tile_edge_voxels
    }

    /// Returns the world-column spacing between retained vertices.
    #[must_use]
    pub const fn sample_step_voxels(&self) -> u16 {
        self.sample_step_voxels
    }

    /// Returns canonical stable bytes for golden and cache-conformance checks.
    ///
    /// # Errors
    ///
    /// Returns an encoding error if the validated tile cannot be serialized.
    pub fn canonical_bytes(&self) -> WorldgenResult<Vec<u8>> {
        canonical_json_bytes(self).map_err(|error| WorldgenError::CanonicalEncoding {
            kind: "far-terrain tile",
            reason: error.to_string(),
        })
    }

    /// Returns a conservative heap accounting value for the retained vectors.
    #[must_use]
    pub fn retained_vector_bytes(&self) -> usize {
        self.shell
            .vertices
            .capacity()
            .saturating_mul(size_of::<FarTerrainVertexV1>())
            .saturating_add(
                self.shell
                    .water_vertices
                    .capacity()
                    .saturating_mul(size_of::<FarTerrainWaterVertexV1>()),
            )
            .saturating_add(
                self.shell
                    .top_triangles
                    .capacity()
                    .saturating_mul(size_of::<FarTerrainTriangleV1>()),
            )
            .saturating_add(
                self.shell
                    .cell_envelopes
                    .capacity()
                    .saturating_mul(size_of::<FarTerrainCellEnvelopeV1>()),
            )
            .saturating_add(
                self.shell
                    .cliff_walls
                    .capacity()
                    .saturating_mul(size_of::<FarTerrainWallV1>()),
            )
            .saturating_add(
                self.shell
                    .border_skirts
                    .capacity()
                    .saturating_mul(size_of::<FarTerrainBorderSkirtV1>()),
            )
    }

    /// Returns the horizontal voxel-column count covered by this tile.
    #[must_use]
    pub fn covered_columns(&self) -> u64 {
        let span = u64::from(self.base_tile_edge_voxels)
            .saturating_mul(u64::from(self.sample_step_voxels));
        span.saturating_mul(span)
    }
}

/// Builds one deterministic far-terrain tile from a pure final-surface source.
///
/// The builder densely inspects every source column inside the tile's LOD span,
/// then retains a fixed `(base edge + 1)^2` surface grid plus exact per-cell
/// bounds, error envelopes, strongest cliff walls, and border skirts.
///
/// # Errors
///
/// Returns an invalid-request, arithmetic, source, or canonical-key error.
pub fn build_far_terrain_tile_v1<S: FarTerrainSurfaceSourceV1 + ?Sized>(
    source: &S,
    provenance: FarTerrainSourceProvenanceV1,
    address: FarTerrainTileAddressV1,
    base_tile_edge_voxels: u16,
) -> WorldgenResult<FarTerrainTileV1> {
    validate_base_tile_edge_v1(base_tile_edge_voxels)?;
    let step = address.lod.sample_step_voxels();
    let span = address.span_voxels(base_tile_edge_voxels)?;
    let dense_edge = usize::from(span).saturating_add(1);
    let dense_len =
        dense_edge
            .checked_mul(dense_edge)
            .ok_or(WorldgenError::ArithmeticOverflow {
                operation: "far-terrain dense sample allocation",
            })?;
    let (world_origin_x, world_origin_z) = address.world_origin(base_tile_edge_voxels)?;
    let mut dense = Vec::with_capacity(dense_len);
    for local_z in 0..=span {
        for local_x in 0..=span {
            dense.push(source.sample_far_terrain_surface(
                world_origin_x.saturating_add(i64::from(local_x)),
                world_origin_z.saturating_add(i64::from(local_z)),
            )?);
        }
    }

    let coarse_edge = usize::from(base_tile_edge_voxels).saturating_add(1);
    let mut vertices = Vec::with_capacity(coarse_edge.saturating_mul(coarse_edge));
    let mut water_vertices = Vec::with_capacity(coarse_edge.saturating_mul(coarse_edge));
    for coarse_z in 0..=base_tile_edge_voxels {
        for coarse_x in 0..=base_tile_edge_voxels {
            let local_x = coarse_x.saturating_mul(step);
            let local_z = coarse_z.saturating_mul(step);
            let sample = dense[dense_index(local_x, local_z, dense_edge)];
            vertices.push(FarTerrainVertexV1 {
                local_x,
                solid_y: sample.solid_y,
                local_z,
                material: sample.material,
            });
            water_vertices.push(FarTerrainWaterVertexV1 {
                local_x,
                local_z,
                surface_y: sample.water_y,
            });
        }
    }

    let cell_count = usize::from(base_tile_edge_voxels)
        .checked_mul(usize::from(base_tile_edge_voxels))
        .ok_or(WorldgenError::ArithmeticOverflow {
            operation: "far-terrain cell count",
        })?;
    let mut top_triangles = Vec::with_capacity(cell_count.saturating_mul(2));
    let mut cell_envelopes = Vec::with_capacity(cell_count);
    let mut cliff_walls = Vec::with_capacity(cell_count);
    for cell_z in 0..base_tile_edge_voxels {
        for cell_x in 0..base_tile_edge_voxels {
            append_top_triangles(
                &mut top_triangles,
                cell_x,
                cell_z,
                base_tile_edge_voxels,
                address,
            );
            let origin_x = cell_x.saturating_mul(step);
            let origin_z = cell_z.saturating_mul(step);
            let envelope = cell_envelope(&dense, dense_edge, origin_x, origin_z, step);
            cell_envelopes.push(envelope);
            append_strongest_cliff_walls(
                &mut cliff_walls,
                &dense,
                dense_edge,
                cell_x,
                cell_z,
                origin_x,
                origin_z,
                step,
            );
        }
    }
    let border_skirts = border_skirts(&vertices, &cell_envelopes, base_tile_edge_voxels);
    let cache_key = FarTerrainCacheKeyV1::new(provenance, address);
    let _ = cache_key.canonical_hash()?;
    Ok(FarTerrainTileV1 {
        cache_key,
        base_tile_edge_voxels,
        sample_step_voxels: step,
        world_origin_x,
        world_origin_z,
        shell: FarTerrainShellV1 {
            vertices,
            water_vertices,
            top_triangles,
            cell_envelopes,
            cliff_walls,
            border_skirts,
        },
    })
}

/// Returns every base tile, shared-border neighbor, and coarser ancestor whose
/// samples can be affected by editing one world-space horizontal column.
///
/// # Errors
///
/// Returns an invalid edge or arithmetic error.
pub fn far_terrain_edit_invalidation_v1(
    world_x: i64,
    world_z: i64,
    base_tile_edge_voxels: u16,
    maximum_lod: FarTerrainLodLevelV1,
) -> WorldgenResult<BTreeSet<FarTerrainTileAddressV1>> {
    validate_base_tile_edge_v1(base_tile_edge_voxels)?;
    let mut affected = BTreeSet::new();
    for level in 0..=maximum_lod.get() {
        let lod = FarTerrainLodLevelV1(level);
        let span = i64::from(base_tile_edge_voxels)
            .checked_mul(i64::from(lod.sample_step_voxels()))
            .ok_or(WorldgenError::ArithmeticOverflow {
                operation: "far-terrain invalidation span",
            })?;
        let tile_x = world_x.div_euclid(span);
        let tile_z = world_z.div_euclid(span);
        let mut xs = vec![tile_x];
        let mut zs = vec![tile_z];
        if world_x.rem_euclid(span) == 0 {
            xs.push(tile_x.saturating_sub(1));
        }
        if world_z.rem_euclid(span) == 0 {
            zs.push(tile_z.saturating_sub(1));
        }
        for x in &xs {
            for z in &zs {
                let x = i32::try_from(*x).map_err(|_| WorldgenError::ArithmeticOverflow {
                    operation: "far-terrain invalidation tile X",
                })?;
                let z = i32::try_from(*z).map_err(|_| WorldgenError::ArithmeticOverflow {
                    operation: "far-terrain invalidation tile Z",
                })?;
                affected.insert(FarTerrainTileAddressV1::new(
                    FarTerrainTileCoordinateV1::new(x, z),
                    lod,
                ));
            }
        }
    }
    Ok(affected)
}

fn validate_base_tile_edge_v1(base_tile_edge_voxels: u16) -> WorldgenResult<()> {
    if base_tile_edge_voxels == 0 || !base_tile_edge_voxels.is_power_of_two() {
        return Err(WorldgenError::InvalidFarTerrainTile {
            field: "base_tile_edge_voxels",
            reason: "tile edge must be a non-zero power of two".to_owned(),
        });
    }
    if base_tile_edge_voxels > MAX_FAR_TERRAIN_BASE_TILE_EDGE_V1 {
        return Err(WorldgenError::InvalidFarTerrainTile {
            field: "base_tile_edge_voxels",
            reason: format!(
                "tile edge {base_tile_edge_voxels} exceeds maximum {MAX_FAR_TERRAIN_BASE_TILE_EDGE_V1}"
            ),
        });
    }
    Ok(())
}

fn dense_index(local_x: u16, local_z: u16, dense_edge: usize) -> usize {
    usize::from(local_z)
        .saturating_mul(dense_edge)
        .saturating_add(usize::from(local_x))
}

fn append_top_triangles(
    triangles: &mut Vec<FarTerrainTriangleV1>,
    cell_x: u16,
    cell_z: u16,
    edge: u16,
    address: FarTerrainTileAddressV1,
) {
    let vertex_edge = u32::from(edge).saturating_add(1);
    let north_west = u32::from(cell_z)
        .saturating_mul(vertex_edge)
        .saturating_add(u32::from(cell_x));
    let north_east = north_west.saturating_add(1);
    let south_west = north_west.saturating_add(vertex_edge);
    let south_east = south_west.saturating_add(1);
    let world_parity = i64::from(address.coordinate.x)
        .saturating_mul(i64::from(edge))
        .saturating_add(i64::from(cell_x))
        .saturating_add(
            i64::from(address.coordinate.z)
                .saturating_mul(i64::from(edge))
                .saturating_add(i64::from(cell_z)),
        )
        & 1;
    if world_parity == 0 {
        triangles.push(FarTerrainTriangleV1([north_west, south_west, south_east]));
        triangles.push(FarTerrainTriangleV1([north_west, south_east, north_east]));
    } else {
        triangles.push(FarTerrainTriangleV1([north_west, south_west, north_east]));
        triangles.push(FarTerrainTriangleV1([north_east, south_west, south_east]));
    }
}

fn cell_envelope(
    dense: &[FarTerrainSurfaceSampleV1],
    dense_edge: usize,
    origin_x: u16,
    origin_z: u16,
    step: u16,
) -> FarTerrainCellEnvelopeV1 {
    let h00 = dense[dense_index(origin_x, origin_z, dense_edge)].solid_y;
    let h10 = dense[dense_index(origin_x.saturating_add(step), origin_z, dense_edge)].solid_y;
    let h01 = dense[dense_index(origin_x, origin_z.saturating_add(step), dense_edge)].solid_y;
    let h11 = dense[dense_index(
        origin_x.saturating_add(step),
        origin_z.saturating_add(step),
        dense_edge,
    )]
    .solid_y;
    let denominator = i128::from(step).saturating_mul(i128::from(step));
    let mut min_solid_y = i32::MAX;
    let mut max_solid_y = i32::MIN;
    let mut max_error = 0_u16;
    let mut min_water_y = None;
    let mut max_water_y = None;
    for dz in 0..=step {
        for dx in 0..=step {
            let sample = dense[dense_index(
                origin_x.saturating_add(dx),
                origin_z.saturating_add(dz),
                dense_edge,
            )];
            min_solid_y = min_solid_y.min(sample.solid_y);
            max_solid_y = max_solid_y.max(sample.solid_y);
            if let Some(water_y) = sample.water_y {
                min_water_y = Some(min_water_y.map_or(water_y, |level: i32| level.min(water_y)));
                max_water_y = Some(max_water_y.map_or(water_y, |level: i32| level.max(water_y)));
            }
            let inverse_x = i128::from(step.saturating_sub(dx));
            let inverse_z = i128::from(step.saturating_sub(dz));
            let dx = i128::from(dx);
            let dz = i128::from(dz);
            let predicted_numerator = i128::from(h00)
                .saturating_mul(inverse_x)
                .saturating_mul(inverse_z)
                .saturating_add(i128::from(h10).saturating_mul(dx).saturating_mul(inverse_z))
                .saturating_add(i128::from(h01).saturating_mul(inverse_x).saturating_mul(dz))
                .saturating_add(i128::from(h11).saturating_mul(dx).saturating_mul(dz));
            let actual_numerator = i128::from(sample.solid_y).saturating_mul(denominator);
            let absolute = actual_numerator.saturating_sub(predicted_numerator).abs();
            let ceil_error = absolute
                .saturating_add(denominator.saturating_sub(1))
                .checked_div(denominator)
                .unwrap_or(i128::from(u16::MAX));
            max_error = max_error.max(u16::try_from(ceil_error).unwrap_or(u16::MAX));
        }
    }
    FarTerrainCellEnvelopeV1 {
        min_solid_y,
        max_solid_y,
        max_error_voxels: max_error,
        min_water_y,
        max_water_y,
    }
}

#[allow(clippy::too_many_arguments)]
fn append_strongest_cliff_walls(
    walls: &mut Vec<FarTerrainWallV1>,
    dense: &[FarTerrainSurfaceSampleV1],
    dense_edge: usize,
    cell_x: u16,
    cell_z: u16,
    origin_x: u16,
    origin_z: u16,
    step: u16,
) {
    let mut strongest_x: Option<(
        u16,
        u16,
        FarTerrainSurfaceSampleV1,
        FarTerrainSurfaceSampleV1,
    )> = None;
    let mut strongest_z: Option<(
        u16,
        u16,
        FarTerrainSurfaceSampleV1,
        FarTerrainSurfaceSampleV1,
    )> = None;
    for dz in 0..=step {
        for dx in 0..step {
            let left = dense[dense_index(
                origin_x.saturating_add(dx),
                origin_z.saturating_add(dz),
                dense_edge,
            )];
            let right = dense[dense_index(
                origin_x.saturating_add(dx).saturating_add(1),
                origin_z.saturating_add(dz),
                dense_edge,
            )];
            if height_delta(left, right) >= 2
                && strongest_x.as_ref().is_none_or(|(_, _, low, high)| {
                    height_delta(left, right) > height_delta(*low, *high)
                })
            {
                strongest_x = Some((
                    origin_x.saturating_add(dx).saturating_add(1),
                    origin_z.saturating_add(dz),
                    left,
                    right,
                ));
            }
        }
    }
    for dz in 0..step {
        for dx in 0..=step {
            let north = dense[dense_index(
                origin_x.saturating_add(dx),
                origin_z.saturating_add(dz),
                dense_edge,
            )];
            let south = dense[dense_index(
                origin_x.saturating_add(dx),
                origin_z.saturating_add(dz).saturating_add(1),
                dense_edge,
            )];
            if height_delta(north, south) >= 2
                && strongest_z.as_ref().is_none_or(|(_, _, low, high)| {
                    height_delta(north, south) > height_delta(*low, *high)
                })
            {
                strongest_z = Some((
                    origin_z.saturating_add(dz).saturating_add(1),
                    origin_x.saturating_add(dx),
                    north,
                    south,
                ));
            }
        }
    }
    if let Some((plane_offset, segment_offset, first, second)) = strongest_x {
        walls.push(wall(
            cell_x,
            cell_z,
            FarTerrainWallAxisV1::X,
            plane_offset,
            segment_offset,
            first,
            second,
        ));
    }
    if let Some((plane_offset, segment_offset, first, second)) = strongest_z {
        walls.push(wall(
            cell_x,
            cell_z,
            FarTerrainWallAxisV1::Z,
            plane_offset,
            segment_offset,
            first,
            second,
        ));
    }
}

fn height_delta(first: FarTerrainSurfaceSampleV1, second: FarTerrainSurfaceSampleV1) -> u32 {
    first.solid_y.abs_diff(second.solid_y)
}

fn wall(
    cell_x: u16,
    cell_z: u16,
    axis: FarTerrainWallAxisV1,
    plane_offset: u16,
    segment_offset: u16,
    first: FarTerrainSurfaceSampleV1,
    second: FarTerrainSurfaceSampleV1,
) -> FarTerrainWallV1 {
    let (lower, higher) = if first.solid_y <= second.solid_y {
        (first, second)
    } else {
        (second, first)
    };
    FarTerrainWallV1 {
        cell_x,
        cell_z,
        axis,
        plane_offset,
        segment_offset,
        bottom_y: lower.solid_y,
        top_y: higher.solid_y,
        material: higher.material,
    }
}

fn border_skirts(
    vertices: &[FarTerrainVertexV1],
    envelopes: &[FarTerrainCellEnvelopeV1],
    edge: u16,
) -> Vec<FarTerrainBorderSkirtV1> {
    let mut skirts = Vec::with_capacity(usize::from(edge).saturating_mul(4));
    let vertex_edge = usize::from(edge).saturating_add(1);
    let cell_edge = usize::from(edge);
    for segment in 0..edge {
        let segment_usize = usize::from(segment);
        append_skirt(
            &mut skirts,
            FarTerrainBorderSideV1::North,
            segment,
            vertices[segment_usize],
            vertices[segment_usize.saturating_add(1)],
            envelopes[segment_usize],
        );
        let south_vertex = usize::from(edge)
            .saturating_mul(vertex_edge)
            .saturating_add(segment_usize);
        let south_cell = usize::from(edge.saturating_sub(1))
            .saturating_mul(cell_edge)
            .saturating_add(segment_usize);
        append_skirt(
            &mut skirts,
            FarTerrainBorderSideV1::South,
            segment,
            vertices[south_vertex],
            vertices[south_vertex.saturating_add(1)],
            envelopes[south_cell],
        );
        let west_vertex = segment_usize.saturating_mul(vertex_edge);
        let west_cell = segment_usize.saturating_mul(cell_edge);
        append_skirt(
            &mut skirts,
            FarTerrainBorderSideV1::West,
            segment,
            vertices[west_vertex],
            vertices[west_vertex.saturating_add(vertex_edge)],
            envelopes[west_cell],
        );
        let east_vertex = segment_usize
            .saturating_mul(vertex_edge)
            .saturating_add(usize::from(edge));
        let east_cell = segment_usize
            .saturating_mul(cell_edge)
            .saturating_add(usize::from(edge.saturating_sub(1)));
        append_skirt(
            &mut skirts,
            FarTerrainBorderSideV1::East,
            segment,
            vertices[east_vertex],
            vertices[east_vertex.saturating_add(vertex_edge)],
            envelopes[east_cell],
        );
    }
    skirts
}

fn append_skirt(
    skirts: &mut Vec<FarTerrainBorderSkirtV1>,
    side: FarTerrainBorderSideV1,
    segment: u16,
    first: FarTerrainVertexV1,
    second: FarTerrainVertexV1,
    envelope: FarTerrainCellEnvelopeV1,
) {
    let depth = i32::from(envelope.max_error_voxels.max(1));
    skirts.push(FarTerrainBorderSkirtV1 {
        side,
        segment,
        first_top_y: first.solid_y,
        second_top_y: second.solid_y,
        bottom_y: envelope.min_solid_y.saturating_sub(depth),
    });
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use latticeaxiom_core::CanonicalHash;
    use latticeaxiom_storage::{ChunkRevision, DimensionId};
    use proptest::prelude::*;

    use super::*;

    #[derive(Clone, Copy)]
    struct SyntheticSurface;

    impl FarTerrainSurfaceSourceV1 for SyntheticSurface {
        fn sample_far_terrain_surface(
            &self,
            world_x: i64,
            world_z: i64,
        ) -> WorldgenResult<FarTerrainSurfaceSampleV1> {
            let rolling = world_x.div_euclid(7).saturating_add(world_z.div_euclid(11));
            let terrace = if world_x.rem_euclid(16) >= 8 { 12 } else { 0 };
            let solid_y = i32::try_from(rolling.saturating_add(terrace)).unwrap_or_else(|_| {
                if rolling.is_negative() {
                    i32::MIN
                } else {
                    i32::MAX
                }
            });
            let material = if terrace == 0 {
                D4MaterialRoleV1::TemperateSurface
            } else {
                D4MaterialRoleV1::TemperateBaseRock
            };
            let water_y = (solid_y < 2).then_some(3);
            Ok(FarTerrainSurfaceSampleV1::new(solid_y, material, water_y))
        }
    }

    fn provenance(revision: u64) -> FarTerrainSourceProvenanceV1 {
        FarTerrainSourceProvenanceV1::Committed(FarTerrainCommittedProvenanceV1::new(
            "terrenia:dimension/terrenia"
                .parse::<DimensionId>()
                .expect("fixture dimension is valid"),
            ChunkRevision::new(revision),
            SnapshotChecksumV1::from_hash(CanonicalHash::digest(format!(
                "surface-revision-{revision}"
            ))),
        ))
    }

    fn address(x: i32, z: i32, lod: u8) -> FarTerrainTileAddressV1 {
        FarTerrainTileAddressV1::new(
            FarTerrainTileCoordinateV1::new(x, z),
            FarTerrainLodLevelV1::new(lod).expect("fixture LOD is bounded"),
        )
    }

    #[test]
    fn deserialization_cannot_bypass_lod_or_surface_invariants() {
        assert!(serde_json::from_str::<FarTerrainLodLevelV1>("3").is_ok());
        assert!(serde_json::from_str::<FarTerrainLodLevelV1>("4").is_err());

        let mut invalid_surface = serde_json::to_value(FarTerrainSurfaceSampleV1::new(
            4,
            D4MaterialRoleV1::TemperateSurface,
            Some(7),
        ))
        .expect("valid surface serializes");
        invalid_surface["water_y"] = serde_json::json!(4);
        assert!(
            serde_json::from_value::<FarTerrainSurfaceSampleV1>(invalid_surface).is_err(),
            "wire data cannot place water at or below the solid surface"
        );
    }

    #[test]
    fn tile_edge_limit_rejects_unbounded_dense_sampling() {
        let maximum_lod = FarTerrainLodLevelV1::new(MAX_FAR_TERRAIN_LOD_LEVEL_V1)
            .expect("published maximum LOD is valid");
        assert!(
            far_terrain_edit_invalidation_v1(0, 0, MAX_FAR_TERRAIN_BASE_TILE_EDGE_V1, maximum_lod,)
                .is_ok()
        );
        assert!(
            far_terrain_edit_invalidation_v1(
                0,
                0,
                MAX_FAR_TERRAIN_BASE_TILE_EDGE_V1.saturating_mul(2),
                maximum_lod,
            )
            .is_err()
        );
    }

    #[test]
    fn lod_shell_retains_error_cliffs_water_and_bounded_memory() {
        let tile =
            build_far_terrain_tile_v1(&SyntheticSurface, provenance(1), address(0, -1, 1), 8)
                .expect("synthetic tile builds");

        assert_eq!(tile.shell.vertices.len(), 81);
        assert_eq!(tile.shell.top_triangles.len(), 128);
        assert_eq!(tile.shell.cell_envelopes.len(), 64);
        assert_eq!(tile.shell.border_skirts.len(), 32);
        assert!(tile.shell.cliff_walls.iter().any(|wall| {
            wall.top_y.saturating_sub(wall.bottom_y) >= 12
                && wall.material == D4MaterialRoleV1::TemperateBaseRock
        }));
        assert!(
            tile.shell
                .water_vertices
                .iter()
                .any(|vertex| vertex.surface_y.is_some())
        );
        assert!(
            tile.shell
                .cell_envelopes
                .iter()
                .any(|cell| cell.max_error_voxels > 0)
        );
        let equivalent_full_chunk_bytes = tile
            .covered_columns()
            .saturating_mul(7)
            .saturating_mul(8)
            .saturating_mul(2);
        assert!(
            u64::try_from(tile.retained_vector_bytes()).unwrap_or(u64::MAX)
                < equivalent_full_chunk_bytes
        );
    }

    #[test]
    fn adjacent_positive_and_negative_tiles_share_exact_border_samples() {
        for lod in 0..=MAX_FAR_TERRAIN_LOD_LEVEL_V1 {
            let left = build_far_terrain_tile_v1(
                &SyntheticSurface,
                provenance(1),
                address(-1, -2, lod),
                8,
            )
            .expect("left tile builds");
            let right =
                build_far_terrain_tile_v1(&SyntheticSurface, provenance(1), address(0, -2, lod), 8)
                    .expect("right tile builds");
            assert_shared_x_border(&left, &right, 8);
        }
    }

    #[test]
    fn cache_key_changes_with_revision_lod_coordinate_and_algorithm_inputs() {
        let first = FarTerrainCacheKeyV1::new(provenance(1), address(-1, 2, 0));
        let revision = FarTerrainCacheKeyV1::new(provenance(2), address(-1, 2, 0));
        let lod = FarTerrainCacheKeyV1::new(provenance(1), address(-1, 2, 1));
        let coordinate = FarTerrainCacheKeyV1::new(provenance(1), address(0, 2, 0));
        let hashes = [first, revision, lod, coordinate]
            .iter()
            .map(FarTerrainCacheKeyV1::canonical_hash)
            .collect::<WorldgenResult<BTreeSet<_>>>()
            .expect("cache keys encode");
        assert_eq!(hashes.len(), 4);
    }

    #[test]
    fn boundary_edit_invalidates_neighbors_and_every_coarser_ancestor() {
        let affected = far_terrain_edit_invalidation_v1(
            0,
            0,
            8,
            FarTerrainLodLevelV1::new(3).expect("LOD is bounded"),
        )
        .expect("boundary invalidation is representable");
        assert_eq!(affected.len(), 16);
        for lod in 0..=3 {
            assert!(affected.contains(&address(0, 0, lod)));
            assert!(affected.contains(&address(-1, 0, lod)));
            assert!(affected.contains(&address(0, -1, lod)));
            assert!(affected.contains(&address(-1, -1, lod)));
        }
        let interior = far_terrain_edit_invalidation_v1(
            3,
            -5,
            8,
            FarTerrainLodLevelV1::new(3).expect("LOD is bounded"),
        )
        .expect("interior invalidation is representable");
        assert_eq!(interior.len(), 4);
    }

    #[test]
    fn generation_order_cannot_change_tile_bytes() {
        let addresses = [address(-2, 1, 0), address(0, -1, 2), address(3, 4, 1)];
        let build = |ordered: &[FarTerrainTileAddressV1]| {
            ordered
                .iter()
                .map(|&address| {
                    let bytes =
                        build_far_terrain_tile_v1(&SyntheticSurface, provenance(7), address, 8)
                            .and_then(|tile| tile.canonical_bytes())?;
                    Ok((address, bytes))
                })
                .collect::<WorldgenResult<BTreeMap<_, _>>>()
        };
        let forward = build(&addresses).expect("forward tiles build");
        let reverse =
            build(&addresses.into_iter().rev().collect::<Vec<_>>()).expect("reverse tiles build");
        assert_eq!(forward, reverse);
    }

    #[test]
    fn canonical_tile_bytes_match_golden_hash() {
        let bytes =
            build_far_terrain_tile_v1(&SyntheticSurface, provenance(11), address(-2, 3, 1), 8)
                .and_then(|tile| tile.canonical_bytes())
                .expect("golden tile encodes");
        assert_eq!(
            CanonicalHash::digest(bytes).to_string(),
            "8a9db7eb39f80ff35ef94f69e67c8252e50a46c37938b41794807046293a0b32"
        );
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(64))]

        #[test]
        fn adjacent_tile_seams_are_exact_for_signed_coordinates(
            tile_x in -16_i32..16,
            tile_z in -16_i32..16,
            lod in 0_u8..=MAX_FAR_TERRAIN_LOD_LEVEL_V1,
        ) {
            let left = build_far_terrain_tile_v1(
                &SyntheticSurface,
                provenance(1),
                address(tile_x, tile_z, lod),
                8,
            ).expect("left property tile builds");
            let right = build_far_terrain_tile_v1(
                &SyntheticSurface,
                provenance(1),
                address(tile_x.saturating_add(1), tile_z, lod),
                8,
            ).expect("right property tile builds");
            prop_assert!(shared_x_border_matches(&left, &right, 8));
        }
    }

    fn assert_shared_x_border(left: &FarTerrainTileV1, right: &FarTerrainTileV1, edge: u16) {
        assert!(shared_x_border_matches(left, right, edge));
    }

    fn shared_x_border_matches(
        left: &FarTerrainTileV1,
        right: &FarTerrainTileV1,
        edge: u16,
    ) -> bool {
        let vertex_edge = usize::from(edge).saturating_add(1);
        (0..=usize::from(edge)).all(|row| {
            let left_vertex = left.shell.vertices[row
                .saturating_mul(vertex_edge)
                .saturating_add(usize::from(edge))];
            let right_vertex = right.shell.vertices[row.saturating_mul(vertex_edge)];
            let left_water = left.shell.water_vertices[row
                .saturating_mul(vertex_edge)
                .saturating_add(usize::from(edge))];
            let right_water = right.shell.water_vertices[row.saturating_mul(vertex_edge)];
            left_vertex.solid_y == right_vertex.solid_y
                && left_vertex.material == right_vertex.material
                && left_water.surface_y == right_water.surface_y
                && left
                    .world_origin_x
                    .saturating_add(i64::from(left_vertex.local_x))
                    == right
                        .world_origin_x
                        .saturating_add(i64::from(right_vertex.local_x))
                && left
                    .world_origin_z
                    .saturating_add(i64::from(left_vertex.local_z))
                    == right
                        .world_origin_z
                        .saturating_add(i64::from(right_vertex.local_z))
        })
    }
}
