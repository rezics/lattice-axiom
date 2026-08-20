use latticeaxiom_core::CanonicalHash;
use latticeaxiom_storage::ChunkCoordinate;
use serde::{Deserialize, Serialize};

use crate::{
    GenerationInputHashV1, ProviderGenerationIdentityV1, SharedFaceHashV1, WorldSeedV1,
    WorldgenConfigV1, WorldgenError, WorldgenResult,
    hashes::{domain_hash, hash_u64},
};

const SHARED_FACE_DOMAIN: &[u8] = b"latticeaxiom.cave-shared-face.v1\0";
const CAVE_VOID_DOMAIN: &[u8] = b"latticeaxiom.d4-cave-void.v2\0";

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
}

impl CaveSamplerV1 {
    pub(crate) const fn new(
        seed: WorldSeedV1,
        input_hash: GenerationInputHashV1,
        config: WorldgenConfigV1,
        provider: ProviderGenerationIdentityV1,
    ) -> Self {
        Self {
            seed,
            input_hash,
            config,
            provider,
        }
    }

    pub(crate) fn is_void(&self, x: i64, y: i64, z: i64, surface_y: i32) -> bool {
        let minimum_cover = i64::from(self.config.cave_minimum_cover);
        if y > i64::from(surface_y).saturating_sub(minimum_cover)
            || y <= i64::from(self.config.world_floor_y)
        {
            return false;
        }
        self.signed_distance_fixed(x, y, z) <= 0
    }

    /// Returns the bounded integer cave field; non-positive samples are void.
    pub(crate) fn signed_distance_fixed(&self, x: i64, y: i64, z: i64) -> i32 {
        let base = self.coarse_cell_distance(x, y, z);
        let Some(chunk) = self.chunk_at_world(x, y, z) else {
            return base;
        };
        let mut distance = base;
        for face in ChunkFaceV1::ALL {
            let Ok(key) = self.shared_face_key(chunk, face) else {
                continue;
            };
            let portal = self.portal_contract(key);
            if portal.requested {
                distance = distance.min(self.portal_distance(x, y, z, chunk, face, portal));
            }
        }
        distance
    }

    pub(crate) fn face_requests(
        &self,
        coordinate: ChunkCoordinate,
    ) -> WorldgenResult<Vec<CaveFaceFieldRequestV1>> {
        ChunkFaceV1::ALL
            .into_iter()
            .map(|face| {
                self.shared_face_key(coordinate, face).map(|key| {
                    let portal = self.portal_contract(key);
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
        let roll = hash_u64(
            CAVE_VOID_DOMAIN,
            &[
                self.seed.as_bytes(),
                self.input_hash.as_bytes(),
                self.provider.provider_stable_id().as_str().as_bytes(),
                self.provider.implementation_fingerprint().as_bytes(),
                &cell_x.to_be_bytes(),
                &cell_y.to_be_bytes(),
                &cell_z.to_be_bytes(),
            ],
        ) % 1_024;
        let edge_i32 = i32::from(self.config.cave_cell_edge_voxels);
        if self.config.cave_threshold_per_1024 == 1_024 {
            return edge_i32.saturating_mul(-2);
        }
        if roll >= u64::from(self.config.cave_threshold_per_1024) {
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

    fn chunk_at_world(&self, x: i64, y: i64, z: i64) -> Option<ChunkCoordinate> {
        let edge = i64::from(self.config.chunk_edge_voxels);
        Some(ChunkCoordinate::new(
            i32::try_from(x.div_euclid(edge)).ok()?,
            i32::try_from(y.div_euclid(edge)).ok()?,
            i32::try_from(z.div_euclid(edge)).ok()?,
        ))
    }

    fn portal_contract(&self, key: SharedFaceKeyV1) -> PortalContractV1 {
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
