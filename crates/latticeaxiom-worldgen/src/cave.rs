use std::collections::BTreeMap;

use latticeaxiom_core::{CanonicalHash, StableId};
use latticeaxiom_storage::ChunkCoordinate;
use serde::{Deserialize, Serialize};

use crate::{
    CaveTopologyAlgorithmV1, CaveVoxelPassabilityReceiptV1, GenerationInputHashV1,
    ProviderGenerationIdentityV1, SharedFaceHashV1, WorldSeedV1, WorldgenConfigV1, WorldgenError,
    WorldgenResult,
    cave_topology::{CaveTopologyLayerInputV1, TopologyFieldV1},
    hashes::{domain_hash, hash_u64, sample_hash_3d},
};

const SHARED_FACE_DOMAIN: &[u8] = b"latticeaxiom.cave-shared-face.v1\0";
const CAVE_VOID_DOMAIN: &[u8] = b"latticeaxiom.d4-cave-void.v2\0";
const CAVE_BRANCH_DOMAIN: &[u8] = b"latticeaxiom.d4-cave-branch.v1\0";

/// Canonical outward face of a cubic chunk in Y-up coordinates.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ChunkFaceV1 {
    /// Face toward decreasing X.
    NegativeX,
    /// Face toward increasing X.
    PositiveX,
    /// Face toward decreasing Y.
    NegativeY,
    /// Face toward increasing Y.
    PositiveY,
    /// Face toward decreasing Z (conventional forward).
    NegativeZ,
    /// Face toward increasing Z.
    PositiveZ,
}

impl ChunkFaceV1 {
    /// Canonical face order used in generation receipts.
    pub const ALL: [Self; 6] = [
        Self::NegativeX,
        Self::PositiveX,
        Self::NegativeY,
        Self::PositiveY,
        Self::NegativeZ,
        Self::PositiveZ,
    ];

    /// Returns the face on the other side of the same shared boundary.
    #[must_use]
    pub const fn opposite(self) -> Self {
        match self {
            Self::NegativeX => Self::PositiveX,
            Self::PositiveX => Self::NegativeX,
            Self::NegativeY => Self::PositiveY,
            Self::PositiveY => Self::NegativeY,
            Self::NegativeZ => Self::PositiveZ,
            Self::PositiveZ => Self::NegativeZ,
        }
    }

    const fn axis(self) -> u8 {
        match self {
            Self::NegativeX | Self::PositiveX => 0,
            Self::NegativeY | Self::PositiveY => 1,
            Self::NegativeZ | Self::PositiveZ => 2,
        }
    }
}

/// Direction-independent shared-face key for adjacent chunk cave planning.
#[repr(transparent)]
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct SharedFaceKeyV1(SharedFaceHashV1);

impl SharedFaceKeyV1 {
    /// Returns the typed canonical shared-face hash.
    #[must_use]
    pub const fn hash(self) -> SharedFaceHashV1 {
        self.0
    }
}

/// Direction-independent request to union a portal into the raw cave field.
///
/// This request does not assert final voxel occupancy. Minimum-cover and world
/// bounds are applied later, and [`CaveFaceOccupancyValidationV1`] records the
/// resulting materialized aperture.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CaveFaceFieldRequestV1 {
    face: ChunkFaceV1,
    key: SharedFaceKeyV1,
    portal_requested: bool,
    portal_u_voxel: u16,
    portal_v_voxel: u16,
    clearance_radius_voxels: u16,
}

impl CaveFaceFieldRequestV1 {
    /// Returns the local outward face.
    #[must_use]
    pub const fn face(self) -> ChunkFaceV1 {
        self.face
    }

    /// Returns the direction-independent shared-face key.
    #[must_use]
    pub const fn key(self) -> SharedFaceKeyV1 {
        self.key
    }

    /// Returns whether the shared-face contract requests a raw-field portal.
    ///
    /// A request is not proof that final occupancy is empty.
    #[must_use]
    pub const fn portal_requested(self) -> bool {
        self.portal_requested
    }

    /// Returns the first local tangential portal coordinate.
    #[must_use]
    pub const fn portal_u_voxel(self) -> u16 {
        self.portal_u_voxel
    }

    /// Returns the second local tangential portal coordinate.
    #[must_use]
    pub const fn portal_v_voxel(self) -> u16 {
        self.portal_v_voxel
    }

    /// Returns the requested raw-field L-infinity clearance radius.
    #[must_use]
    pub const fn clearance_radius_voxels(self) -> u16 {
        self.clearance_radius_voxels
    }

    /// Returns machine-readable raw-field portal evidence when a portal is requested.
    ///
    /// Position is the tangential `(u, v)` aperture. The face is the portal-plane
    /// normal; `u`/`v` are the tangent coordinates in that plane. Clearance is the
    /// L-infinity radius. Fluid compatibility is a territory hydrology constraint
    /// and is not owned by the cave field.
    #[must_use]
    pub const fn assertion(self) -> Option<CaveFieldPortalAssertionV1> {
        if !self.portal_requested {
            return None;
        }
        Some(CaveFieldPortalAssertionV1 {
            key: self.key,
            face: self.face,
            position_u_voxel: self.portal_u_voxel,
            position_v_voxel: self.portal_v_voxel,
            clearance_radius_voxels: self.clearance_radius_voxels,
        })
    }
}

/// Machine-readable raw-field portal assertion for one shared chunk face.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CaveFieldPortalAssertionV1 {
    key: SharedFaceKeyV1,
    face: ChunkFaceV1,
    position_u_voxel: u16,
    position_v_voxel: u16,
    clearance_radius_voxels: u16,
}

impl CaveFieldPortalAssertionV1 {
    /// Returns the direction-independent shared-face key.
    #[must_use]
    pub const fn key(self) -> SharedFaceKeyV1 {
        self.key
    }

    /// Returns the local outward face, which is the portal-plane normal.
    #[must_use]
    pub const fn tangent_face(self) -> ChunkFaceV1 {
        self.face
    }

    /// Returns the first tangential portal coordinate.
    #[must_use]
    pub const fn position_u_voxel(self) -> u16 {
        self.position_u_voxel
    }

    /// Returns the second tangential portal coordinate.
    #[must_use]
    pub const fn position_v_voxel(self) -> u16 {
        self.position_v_voxel
    }

    /// Returns the requested raw-field L-infinity clearance radius.
    #[must_use]
    pub const fn clearance_radius_voxels(self) -> u16 {
        self.clearance_radius_voxels
    }
}

/// Canonical raw-field portal requests collected independently of chunk order.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CaveFieldPortalPlanV1 {
    requests: Vec<CaveFaceFieldRequestV1>,
}

impl CaveFieldPortalPlanV1 {
    /// Keeps unique requested portals, sorted by shared-face key.
    ///
    /// Opposite faces of the same boundary collapse to one request. The stored
    /// face is the lexicographically smaller local face so shuffled chunk order
    /// cannot change the plan.
    #[must_use]
    pub fn from_face_requests(requests: impl IntoIterator<Item = CaveFaceFieldRequestV1>) -> Self {
        let mut by_key = BTreeMap::<SharedFaceKeyV1, CaveFaceFieldRequestV1>::new();
        for request in requests {
            if !request.portal_requested {
                continue;
            }
            match by_key.get(&request.key) {
                Some(existing) if existing.face <= request.face => {}
                _ => {
                    by_key.insert(request.key, request);
                }
            }
        }
        Self {
            requests: by_key.into_values().collect(),
        }
    }

    /// Returns requested portals in shared-face-key order.
    #[must_use]
    pub fn requests(&self) -> &[CaveFaceFieldRequestV1] {
        &self.requests
    }

    /// Returns machine-readable position, tangent, and clearance assertions.
    #[must_use]
    pub fn assertions(&self) -> Vec<CaveFieldPortalAssertionV1> {
        self.requests
            .iter()
            .copied()
            .filter_map(CaveFaceFieldRequestV1::assertion)
            .collect()
    }
}

/// Validation of a raw field request against the materialized face aperture.
///
/// The raw field and final occupancy counts are kept separate because minimum
/// cover, the world floor, and above-surface placement can legitimately prevent
/// a field request from becoming an open final voxel.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CaveFaceOccupancyValidationV1 {
    request: CaveFaceFieldRequestV1,
    aperture_samples: u32,
    field_void_samples: u32,
    final_empty_samples: u32,
}

impl CaveFaceOccupancyValidationV1 {
    pub(crate) const fn new(
        request: CaveFaceFieldRequestV1,
        aperture_samples: u32,
        field_void_samples: u32,
        final_empty_samples: u32,
    ) -> Self {
        Self {
            request,
            aperture_samples,
            field_void_samples,
            final_empty_samples,
        }
    }

    /// Returns the field request being validated.
    #[must_use]
    pub const fn request(self) -> CaveFaceFieldRequestV1 {
        self.request
    }

    /// Returns the number of bounded aperture voxels inspected.
    #[must_use]
    pub const fn aperture_samples(self) -> u32 {
        self.aperture_samples
    }

    /// Returns inspected voxels that are void in the raw cave field.
    #[must_use]
    pub const fn field_void_samples(self) -> u32 {
        self.field_void_samples
    }

    /// Returns inspected voxels whose final concrete block is the Empty Role target.
    #[must_use]
    pub const fn final_empty_samples(self) -> u32 {
        self.final_empty_samples
    }

    /// Returns whether every requested aperture voxel is empty after materialization.
    #[must_use]
    pub const fn is_finally_open(self) -> bool {
        self.request.portal_requested
            && self.aperture_samples > 0
            && self.field_void_samples == self.aperture_samples
            && self.final_empty_samples == self.aperture_samples
    }

    /// Returns whether the raw field still contains the required portal aperture.
    ///
    /// A bounded branch contributor may add extra face holes, but it cannot fill
    /// a requested clearance. Minimum cover may still suppress final occupancy.
    #[must_use]
    pub const fn portal_clearance_intact(self) -> bool {
        !self.request.portal_requested
            || (self.aperture_samples > 0 && self.field_void_samples == self.aperture_samples)
    }
}

/// Local, branch, and portal field samples plus the final occupancy decision.
///
/// Contributors union voids with a minimum signed-distance compositor. Branch
/// passages therefore cannot fill required portal clearance. Minimum cover and
/// the world floor are applied after that union; material, ore, and fluid
/// placement must use [`Self::is_finally_void`] rather than the raw field.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CaveOccupancyArbitrationV1 {
    local_signed_distance: i32,
    branch_signed_distance: i32,
    portal_signed_distance: i32,
    raw_signed_distance: i32,
    finally_void: bool,
}

impl CaveOccupancyArbitrationV1 {
    /// Returns the primary local cave-cell field.
    #[must_use]
    pub const fn local_signed_distance(self) -> i32 {
        self.local_signed_distance
    }

    /// Returns the bounded branch-contributor field.
    #[must_use]
    pub const fn branch_signed_distance(self) -> i32 {
        self.branch_signed_distance
    }

    /// Returns the tightest requested portal field, or `i32::MAX` when none.
    #[must_use]
    pub const fn portal_signed_distance(self) -> i32 {
        self.portal_signed_distance
    }

    /// Returns the unioned raw cave field; non-positive samples are void.
    #[must_use]
    pub const fn raw_signed_distance(self) -> i32 {
        self.raw_signed_distance
    }

    /// Returns whether the raw union is a cave void before cover and floor.
    #[must_use]
    pub const fn is_raw_void(self) -> bool {
        self.raw_signed_distance <= 0
    }

    /// Returns whether final occupancy is a cave void after cover and floor.
    #[must_use]
    pub const fn is_finally_void(self) -> bool {
        self.finally_void
    }

    /// Returns whether a branch spur opened a voxel the local field left solid.
    #[must_use]
    pub const fn branch_added_side_passage(self) -> bool {
        self.branch_signed_distance <= 0 && self.local_signed_distance > 0
    }

    /// Returns whether solid material or ore may occupy this cell.
    #[must_use]
    pub const fn allows_solid_placement(self) -> bool {
        !self.finally_void
    }

    /// Returns whether hydrology/fluid occupancy may fill this final cave void.
    ///
    /// Fluid cannot carve a new void or own cave topology.
    #[must_use]
    pub const fn allows_fluid_occupancy(self) -> bool {
        self.finally_void
    }
}

#[derive(Clone, Copy, Debug)]
struct PortalContractV1 {
    requested: bool,
    u: u16,
    v: u16,
    radius: u16,
}

#[derive(Clone, Debug)]
pub(crate) struct CaveSamplerV1 {
    seed: WorldSeedV1,
    input_hash: GenerationInputHashV1,
    config: WorldgenConfigV1,
    provider: ProviderGenerationIdentityV1,
    void_seed: u64,
    branch_seed: u64,
    topology: Option<TopologyFieldV1>,
}

impl CaveSamplerV1 {
    pub(crate) fn new(
        seed: WorldSeedV1,
        input_hash: GenerationInputHashV1,
        config: WorldgenConfigV1,
        provider: ProviderGenerationIdentityV1,
    ) -> Self {
        let sample_seed = |domain| {
            hash_u64(
                domain,
                &[
                    seed.as_bytes(),
                    input_hash.as_bytes(),
                    provider.provider_stable_id().as_str().as_bytes(),
                    provider.implementation_fingerprint().as_bytes(),
                ],
            )
        };
        let void_seed = sample_seed(CAVE_VOID_DOMAIN);
        let branch_seed = sample_seed(CAVE_BRANCH_DOMAIN);
        Self {
            seed,
            input_hash,
            config,
            provider,
            void_seed,
            branch_seed,
            topology: None,
        }
    }

    pub(crate) fn with_topology(mut self, layer: CaveTopologyLayerInputV1) -> Self {
        self.topology = Some(TopologyFieldV1::compile(self.config.clone(), layer));
        self
    }

    pub(crate) const fn has_topology(&self) -> bool {
        self.topology.is_some()
    }

    pub(crate) const fn topology(&self) -> Option<&TopologyFieldV1> {
        self.topology.as_ref()
    }

    pub(crate) fn topology_algorithm(
        &self,
        x: i64,
        y: i64,
        z: i64,
    ) -> Option<CaveTopologyAlgorithmV1> {
        self.topology
            .as_ref()
            .map(|topology| topology.algorithm_at(x, y, z))
    }

    pub(crate) fn topology_domain(&self, x: i64, y: i64, z: i64) -> Option<&StableId> {
        self.topology
            .as_ref()
            .map(|topology| topology.domain_id_at(x, y, z))
    }

    pub(crate) fn in_declared_influence(&self, x: i64, y: i64, z: i64) -> bool {
        self.topology
            .as_ref()
            .is_some_and(|topology| topology.in_declared_influence(x, y, z))
    }

    pub(crate) fn passability_receipts(
        &self,
        surface_y_at: impl Fn(i64, i64) -> i32,
    ) -> Vec<CaveVoxelPassabilityReceiptV1> {
        let Some(topology) = &self.topology else {
            return Vec::new();
        };
        topology.passability_receipts(|x, y, z| {
            self.occupancy(x, y, z, surface_y_at(x, z))
                .is_finally_void()
        })
    }

    pub(crate) fn is_void(&self, x: i64, y: i64, z: i64, surface_y: i32) -> bool {
        self.occupancy(x, y, z, surface_y).is_finally_void()
    }

    /// Returns the bounded integer cave field; non-positive samples are void.
    pub(crate) fn signed_distance_fixed(&self, x: i64, y: i64, z: i64) -> i32 {
        self.occupancy_uncapped(x, y, z).raw_signed_distance
    }

    /// Unions the local field, optional branch, and required portal clearance.
    ///
    /// The resolved cave-topology provider remains the only cave owner. Branch
    /// and portal samples are bounded contributors, not a second topology.
    /// Hydrology is not a field contributor.
    pub(crate) fn occupancy(
        &self,
        x: i64,
        y: i64,
        z: i64,
        surface_y: i32,
    ) -> CaveOccupancyArbitrationV1 {
        let mut decision = self.occupancy_uncapped(x, y, z);
        let minimum_cover = i64::from(self.config.cave_minimum_cover);
        let protected = y > i64::from(surface_y).saturating_sub(minimum_cover)
            || y <= i64::from(self.config.world_floor_y);
        decision.finally_void = !protected && decision.raw_signed_distance <= 0;
        decision
    }

    fn occupancy_uncapped(&self, x: i64, y: i64, z: i64) -> CaveOccupancyArbitrationV1 {
        let (local_signed_distance, branch_signed_distance, portal_signed_distance) =
            if let Some(topology) = &self.topology {
                let sample = topology.occupancy_sample(x, y, z);
                let branch_signed_distance = if sample.branch_contains() {
                    self.branch_signed_distance(x, y, z)
                } else {
                    i32::from(self.config.cave_cell_edge_voxels).saturating_mul(2)
                };
                (
                    sample.local_signed_distance(),
                    branch_signed_distance,
                    sample.portal_signed_distance(),
                )
            } else {
                (
                    self.coarse_cell_distance(x, y, z),
                    self.branch_signed_distance(x, y, z),
                    self.portal_signed_distance(x, y, z),
                )
            };
        let raw_signed_distance = local_signed_distance
            .min(branch_signed_distance)
            .min(portal_signed_distance);
        CaveOccupancyArbitrationV1 {
            local_signed_distance,
            branch_signed_distance,
            portal_signed_distance,
            raw_signed_distance,
            finally_void: raw_signed_distance <= 0,
        }
    }

    pub(crate) fn field_portal_plan(
        &self,
        chunks: impl IntoIterator<Item = ChunkCoordinate>,
    ) -> WorldgenResult<CaveFieldPortalPlanV1> {
        let mut requests = Vec::new();
        for chunk in chunks {
            requests.extend(self.face_requests(chunk)?);
        }
        Ok(CaveFieldPortalPlanV1::from_face_requests(requests))
    }

    pub(crate) fn face_requests(
        &self,
        coordinate: ChunkCoordinate,
    ) -> WorldgenResult<Vec<CaveFaceFieldRequestV1>> {
        ChunkFaceV1::ALL
            .into_iter()
            .map(|face| {
                self.shared_face_key(coordinate, face).map(|key| {
                    let portal = self.portal_contract(coordinate, face, key);
                    CaveFaceFieldRequestV1 {
                        face,
                        key,
                        portal_requested: portal.requested,
                        portal_u_voxel: portal.u,
                        portal_v_voxel: portal.v,
                        clearance_radius_voxels: portal.radius,
                    }
                })
            })
            .collect()
    }

    pub(crate) fn shared_face_key(
        &self,
        coordinate: ChunkCoordinate,
        face: ChunkFaceV1,
    ) -> WorldgenResult<SharedFaceKeyV1> {
        let neighbor = adjacent_coordinate(coordinate, face)?;
        let (first, second) = if coordinate < neighbor {
            (coordinate, neighbor)
        } else {
            (neighbor, coordinate)
        };
        let first_bytes = coordinate_bytes(first);
        let second_bytes = coordinate_bytes(second);
        let axis = [face.axis()];
        Ok(SharedFaceKeyV1(SharedFaceHashV1::from_hash(domain_hash(
            SHARED_FACE_DOMAIN,
            &[
                self.seed.as_bytes(),
                self.input_hash.as_bytes(),
                self.provider.provider_stable_id().as_str().as_bytes(),
                self.provider.implementation_fingerprint().as_bytes(),
                &axis,
                &first_bytes,
                &second_bytes,
            ],
        ))))
    }

    fn coarse_cell_distance(&self, x: i64, y: i64, z: i64) -> i32 {
        let edge = i64::from(self.config.cave_cell_edge_voxels);
        let cell_x = x.div_euclid(edge);
        let cell_y = y.div_euclid(edge);
        let cell_z = z.div_euclid(edge);
        let edge_i32 = i32::from(self.config.cave_cell_edge_voxels);
        if self.config.cave_threshold_per_1024 == 1_024 {
            return edge_i32.saturating_mul(-2);
        }
        if !self.coarse_cell_accepted(cell_x, cell_y, cell_z) {
            return edge_i32.saturating_mul(2);
        }
        let center = edge.saturating_sub(1);
        let local_x = x.rem_euclid(edge).saturating_mul(2);
        let local_y = y.rem_euclid(edge).saturating_mul(2);
        let local_z = z.rem_euclid(edge).saturating_mul(2);
        let maximum_axis = local_x
            .saturating_sub(center)
            .abs()
            .max(local_y.saturating_sub(center).abs())
            .max(local_z.saturating_sub(center).abs());
        let radius = edge.saturating_sub(2).max(1);
        i32::try_from(maximum_axis.saturating_sub(radius)).unwrap_or(i32::MAX)
    }

    fn branch_signed_distance(&self, x: i64, y: i64, z: i64) -> i32 {
        let edge = i64::from(self.config.cave_cell_edge_voxels);
        let edge_i32 = i32::from(self.config.cave_cell_edge_voxels);
        let inactive = edge_i32.saturating_mul(2);
        let cell_x = x.div_euclid(edge);
        let cell_y = y.div_euclid(edge);
        let cell_z = z.div_euclid(edge);
        if self.config.cave_threshold_per_1024 == 0 {
            return inactive;
        }
        if self.config.cave_threshold_per_1024 != 1_024
            && !self.coarse_cell_accepted(cell_x, cell_y, cell_z)
        {
            return inactive;
        }
        let branch_bits = sample_hash_3d(self.branch_seed, cell_x, cell_y, cell_z);
        if self.config.cave_threshold_per_1024 != 1_024
            && branch_bits % 1_024 >= u64::from(self.config.cave_threshold_per_1024)
        {
            return inactive;
        }
        let axis = (branch_bits >> 10) & 0b11;
        let local_x = x.rem_euclid(edge);
        let local_y = y.rem_euclid(edge);
        let local_z = z.rem_euclid(edge);
        let center = edge.saturating_sub(1).saturating_div(2);
        let (cross_a, cross_b) = match axis {
            0 => (local_y, local_z),
            1 => (local_x, local_z),
            _ => (local_x, local_y),
        };
        let maximum = cross_a
            .saturating_sub(center)
            .abs()
            .max(cross_b.saturating_sub(center).abs());
        i32::try_from(maximum.saturating_sub(1)).unwrap_or(i32::MAX)
    }

    fn portal_signed_distance(&self, x: i64, y: i64, z: i64) -> i32 {
        let Some(chunk) = self.chunk_at_world(x, y, z) else {
            return i32::MAX;
        };
        let mut distance = i32::MAX;
        for face in ChunkFaceV1::ALL {
            let Ok(key) = self.shared_face_key(chunk, face) else {
                continue;
            };
            let portal = self.portal_contract(chunk, face, key);
            if portal.requested {
                distance = distance.min(self.portal_distance(x, y, z, chunk, face, portal));
            }
        }
        distance
    }

    fn coarse_cell_accepted(&self, cell_x: i64, cell_y: i64, cell_z: i64) -> bool {
        if self.config.cave_threshold_per_1024 == 1_024 {
            return true;
        }
        if self.config.cave_threshold_per_1024 == 0 {
            return false;
        }
        let roll = sample_hash_3d(self.void_seed, cell_x, cell_y, cell_z) % 1_024;
        roll < u64::from(self.config.cave_threshold_per_1024)
    }

    fn chunk_at_world(&self, x: i64, y: i64, z: i64) -> Option<ChunkCoordinate> {
        let edge = i64::from(self.config.chunk_edge_voxels);
        Some(ChunkCoordinate::new(
            i32::try_from(x.div_euclid(edge)).ok()?,
            i32::try_from(y.div_euclid(edge)).ok()?,
            i32::try_from(z.div_euclid(edge)).ok()?,
        ))
    }

    fn portal_contract(
        &self,
        coordinate: ChunkCoordinate,
        face: ChunkFaceV1,
        key: SharedFaceKeyV1,
    ) -> PortalContractV1 {
        if let Some(topology) = &self.topology {
            return match topology.face_portal(coordinate, face) {
                Some((u, v, radius)) => PortalContractV1 {
                    requested: true,
                    u,
                    v,
                    radius,
                },
                None => PortalContractV1 {
                    requested: false,
                    u: 0,
                    v: 0,
                    radius: 1,
                },
            };
        }
        let hash = key.hash();
        let bytes = hash.as_bytes();
        let roll = u16::from_be_bytes([bytes[0], bytes[1]]) % 1_024;
        let radius = (self.config.chunk_edge_voxels / 8).clamp(2, 4);
        let margin = radius.saturating_add(1);
        let span = self
            .config
            .chunk_edge_voxels
            .saturating_sub(margin.saturating_mul(2))
            .max(1);
        let u = margin.saturating_add(u16::from_be_bytes([bytes[2], bytes[3]]) % span);
        let v = margin.saturating_add(u16::from_be_bytes([bytes[4], bytes[5]]) % span);
        PortalContractV1 {
            requested: roll < self.config.cave_threshold_per_1024.saturating_div(4),
            u,
            v,
            radius,
        }
    }

    #[allow(
        clippy::many_single_char_names,
        clippy::similar_names,
        clippy::too_many_arguments,
        reason = "X/Y/Z axes and portal U/V are the explicit fixed-point domain terms"
    )]
    fn portal_distance(
        &self,
        x: i64,
        y: i64,
        z: i64,
        chunk: ChunkCoordinate,
        face: ChunkFaceV1,
        portal: PortalContractV1,
    ) -> i32 {
        let edge = i64::from(self.config.chunk_edge_voxels);
        let origin_x = i64::from(chunk.x).saturating_mul(edge);
        let origin_y = i64::from(chunk.y).saturating_mul(edge);
        let origin_z = i64::from(chunk.z).saturating_mul(edge);
        let plane_x_negative = origin_x.saturating_mul(2).saturating_sub(1);
        let plane_x_positive = origin_x
            .saturating_add(edge)
            .saturating_mul(2)
            .saturating_sub(1);
        let plane_y_negative = origin_y.saturating_mul(2).saturating_sub(1);
        let plane_y_positive = origin_y
            .saturating_add(edge)
            .saturating_mul(2)
            .saturating_sub(1);
        let plane_z_negative = origin_z.saturating_mul(2).saturating_sub(1);
        let plane_z_positive = origin_z
            .saturating_add(edge)
            .saturating_mul(2)
            .saturating_sub(1);
        let u = i64::from(portal.u).saturating_mul(2);
        let v = i64::from(portal.v).saturating_mul(2);
        let (center_x, center_y, center_z) = match face {
            ChunkFaceV1::NegativeX => (
                plane_x_negative,
                origin_y.saturating_mul(2).saturating_add(u),
                origin_z.saturating_mul(2).saturating_add(v),
            ),
            ChunkFaceV1::PositiveX => (
                plane_x_positive,
                origin_y.saturating_mul(2).saturating_add(u),
                origin_z.saturating_mul(2).saturating_add(v),
            ),
            ChunkFaceV1::NegativeY => (
                origin_x.saturating_mul(2).saturating_add(u),
                plane_y_negative,
                origin_z.saturating_mul(2).saturating_add(v),
            ),
            ChunkFaceV1::PositiveY => (
                origin_x.saturating_mul(2).saturating_add(u),
                plane_y_positive,
                origin_z.saturating_mul(2).saturating_add(v),
            ),
            ChunkFaceV1::NegativeZ => (
                origin_x.saturating_mul(2).saturating_add(u),
                origin_y.saturating_mul(2).saturating_add(v),
                plane_z_negative,
            ),
            ChunkFaceV1::PositiveZ => (
                origin_x.saturating_mul(2).saturating_add(u),
                origin_y.saturating_mul(2).saturating_add(v),
                plane_z_positive,
            ),
        };
        let point_x = x.saturating_mul(2);
        let point_y = y.saturating_mul(2);
        let point_z = z.saturating_mul(2);
        let maximum_axis = point_x
            .saturating_sub(center_x)
            .abs()
            .max(point_y.saturating_sub(center_y).abs())
            .max(point_z.saturating_sub(center_z).abs());
        let radius = i64::from(portal.radius).saturating_mul(2);
        i32::try_from(maximum_axis.saturating_sub(radius)).unwrap_or(i32::MAX)
    }
}

fn adjacent_coordinate(
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
            operation: "shared-face neighbor X",
        })?,
        y.ok_or(WorldgenError::ArithmeticOverflow {
            operation: "shared-face neighbor Y",
        })?,
        z.ok_or(WorldgenError::ArithmeticOverflow {
            operation: "shared-face neighbor Z",
        })?,
    ))
}

fn coordinate_bytes(coordinate: ChunkCoordinate) -> [u8; 12] {
    let mut bytes = [0_u8; 12];
    bytes[..4].copy_from_slice(&coordinate.x.to_be_bytes());
    bytes[4..8].copy_from_slice(&coordinate.y.to_be_bytes());
    bytes[8..].copy_from_slice(&coordinate.z.to_be_bytes());
    bytes
}

pub(crate) fn snapshot_checksum(bytes: &[u8]) -> crate::SnapshotChecksumV1 {
    crate::SnapshotChecksumV1::from_hash(CanonicalHash::digest(bytes))
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU32;

    use latticeaxiom_core::{CanonicalHash, StableId};
    use latticeaxiom_storage::ChunkCoordinate;

    use super::{CaveFieldPortalPlanV1, CaveSamplerV1, ChunkFaceV1};
    use crate::{
        GenerationInputHashV1, ProviderGenerationIdentityV1, WorldSeedV1, WorldgenConfigV1,
        WorldgenError,
    };

    #[test]
    fn shared_face_requests_are_direction_independent_from_both_sides() {
        let sampler = test_sampler(132);
        for face in ChunkFaceV1::ALL {
            let chunk = ChunkCoordinate::new(3, -2, 4);
            let neighbor = match face {
                ChunkFaceV1::NegativeX => ChunkCoordinate::new(2, -2, 4),
                ChunkFaceV1::PositiveX => ChunkCoordinate::new(4, -2, 4),
                ChunkFaceV1::NegativeY => ChunkCoordinate::new(3, -3, 4),
                ChunkFaceV1::PositiveY => ChunkCoordinate::new(3, -1, 4),
                ChunkFaceV1::NegativeZ => ChunkCoordinate::new(3, -2, 3),
                ChunkFaceV1::PositiveZ => ChunkCoordinate::new(3, -2, 5),
            };
            let key = sampler
                .shared_face_key(chunk, face)
                .expect("interior shared face is representable");
            let opposite_key = sampler
                .shared_face_key(neighbor, face.opposite())
                .expect("neighbor shared face is representable");
            assert_eq!(key, opposite_key);

            let request = sampler
                .face_requests(chunk)
                .expect("interior face requests are representable")
                .into_iter()
                .find(|candidate| candidate.face() == face)
                .expect("all six face requests exist");
            let opposite = sampler
                .face_requests(neighbor)
                .expect("neighbor face requests are representable")
                .into_iter()
                .find(|candidate| candidate.face() == face.opposite())
                .expect("neighbor has all six face requests");
            assert_eq!(request.key(), opposite.key());
            assert_eq!(request.portal_requested(), opposite.portal_requested());
            assert_eq!(request.portal_u_voxel(), opposite.portal_u_voxel());
            assert_eq!(request.portal_v_voxel(), opposite.portal_v_voxel());
            assert_eq!(
                request.clearance_radius_voxels(),
                opposite.clearance_radius_voxels()
            );
        }
        assert!(matches!(
            sampler.shared_face_key(ChunkCoordinate::new(i32::MAX, 0, 0), ChunkFaceV1::PositiveX),
            Err(WorldgenError::ArithmeticOverflow { .. })
        ));
    }

    #[test]
    fn branch_union_adds_side_passages_without_filling_required_portal_clearance() {
        let sampler = test_sampler(512);
        let mut found_side_passage = false;
        let mut found_portal_clearance = false;
        for world_y in [-32, -24, -16, -8] {
            for world_x in -48..=48 {
                for world_z in -48..=48 {
                    let occupancy = sampler.occupancy(world_x, world_y, world_z, 24);
                    assert!(
                        occupancy.raw_signed_distance()
                            <= occupancy
                                .local_signed_distance()
                                .min(occupancy.portal_signed_distance()),
                        "branch union may add voids but cannot fill local or portal voids"
                    );
                    if occupancy.portal_signed_distance() <= 0 {
                        found_portal_clearance = true;
                        assert!(occupancy.is_raw_void());
                        assert!(
                            occupancy.raw_signed_distance() <= occupancy.portal_signed_distance()
                        );
                    }
                    if occupancy.branch_added_side_passage() && occupancy.is_finally_void() {
                        found_side_passage = true;
                        assert!(occupancy.is_raw_void());
                        assert!(occupancy.allows_fluid_occupancy());
                        assert!(!occupancy.allows_solid_placement());
                        assert!(occupancy.local_signed_distance() > 0);
                    }
                    if found_side_passage && found_portal_clearance {
                        return;
                    }
                }
            }
        }
        assert!(found_side_passage, "bounded branch opens a side passage");
        assert!(
            found_portal_clearance,
            "required portal clearance stays void"
        );
    }

    #[test]
    fn field_portal_plan_is_identical_under_shuffled_chunk_order() {
        let sampler = test_sampler(132);
        let mut chunks = Vec::new();
        for z in -4..=4 {
            for y in -2..=0 {
                for x in -4..=4 {
                    chunks.push(ChunkCoordinate::new(x, y, z));
                }
            }
        }
        let forward = sampler
            .field_portal_plan(chunks.clone())
            .expect("forward field portal plan is representable");
        let reversed = sampler
            .field_portal_plan(chunks.iter().copied().rev())
            .expect("reversed field portal plan is representable");
        let mut rotated = chunks;
        let rotate_by = rotated.len() / 3;
        rotated.rotate_left(rotate_by);
        let rotated = sampler
            .field_portal_plan(rotated)
            .expect("rotated field portal plan is representable");
        assert_eq!(forward, reversed);
        assert_eq!(forward, rotated);
        assert!(!forward.requests().is_empty());
        let collapsed = CaveFieldPortalPlanV1::from_face_requests(
            forward
                .requests()
                .iter()
                .copied()
                .chain(forward.requests().iter().copied()),
        );
        assert_eq!(forward, collapsed);
    }

    #[test]
    fn field_portal_assertions_expose_position_tangent_and_clearance() {
        let sampler = test_sampler(132);
        let chunks = (-4..=4)
            .flat_map(|z| {
                (-2..=0).flat_map(move |y| (-4..=4).map(move |x| ChunkCoordinate::new(x, y, z)))
            })
            .collect::<Vec<_>>();
        let plan = sampler
            .field_portal_plan(chunks)
            .expect("field portal plan is representable");
        let assertions = plan.assertions();
        assert_eq!(assertions.len(), plan.requests().len());
        assert!(!assertions.is_empty());
        for (request, assertion) in plan.requests().iter().copied().zip(assertions) {
            assert!(request.portal_requested());
            assert_eq!(assertion.key(), request.key());
            assert_eq!(assertion.tangent_face(), request.face());
            assert_eq!(assertion.position_u_voxel(), request.portal_u_voxel());
            assert_eq!(assertion.position_v_voxel(), request.portal_v_voxel());
            assert_eq!(
                assertion.clearance_radius_voxels(),
                request.clearance_radius_voxels()
            );
            assert!(assertion.clearance_radius_voxels() > 0);
        }
    }

    #[test]
    fn occupancy_arbitration_keeps_cover_and_floor_solid_for_placement() {
        let sampler = test_sampler(1_024);
        let surface_y = 24;
        let protected = sampler.occupancy(3, 20, -5, surface_y);
        assert!(protected.is_raw_void());
        assert!(!protected.is_finally_void());
        assert!(protected.allows_solid_placement());
        assert!(!protected.allows_fluid_occupancy());

        let carvable = sampler.occupancy(3, 18, -5, surface_y);
        assert!(carvable.is_raw_void());
        assert!(carvable.is_finally_void());
        assert!(!carvable.allows_solid_placement());
        assert!(carvable.allows_fluid_occupancy());

        let floor = sampler.occupancy(3, -64, -5, surface_y);
        assert!(floor.is_raw_void());
        assert!(!floor.is_finally_void());
        assert!(floor.allows_solid_placement());
        assert!(!floor.allows_fluid_occupancy());
    }

    fn test_sampler(threshold: u16) -> CaveSamplerV1 {
        let config = WorldgenConfigV1 {
            chunk_edge_voxels: 8,
            planning_cell_edge_chunks: 8,
            transition_width_voxels: 8,
            cave_cell_edge_voxels: 8,
            cave_threshold_per_1024: threshold,
            cave_minimum_cover: 6,
            world_floor_y: -64,
            world_ceiling_y: 127,
            ..WorldgenConfigV1::default()
        };
        CaveSamplerV1::new(
            WorldSeedV1::from_integer(42),
            GenerationInputHashV1::from_hash(CanonicalHash::digest(b"cave-occupancy-test")),
            config,
            ProviderGenerationIdentityV1::new(
                "fixture:worldgen-provider/cave@1"
                    .parse::<StableId>()
                    .expect("fixture cave provider identity is valid"),
                NonZeroU32::MIN,
                8,
                CanonicalHash::digest(b"cave-implementation-v8"),
            ),
        )
    }
}
