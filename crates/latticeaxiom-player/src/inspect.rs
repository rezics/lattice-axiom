use bevy::prelude::Message;
use latticeaxiom_gameplay::{BlockId, ChunkCoordinate, PlayerId};
use thiserror::Error;

use crate::{ClientTargetObservationV1, TargetEyePoseV1};

/// Headless inspect DTO produced after authoritative Y-up voxel DDA.
///
/// The observation copies the DDA hit for presentation diagnostics. Authority
/// never treats [`ClientTargetObservationV1`] as the selected target.
/// Occupancy fields are working-set counts, not frame-time budgets.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HeadlessTargetInspectV1 {
    /// Voxel, face, revision, and quantized distance of the DDA hit.
    pub observation: ClientTargetObservationV1,
    /// Stable content of the targeted voxel.
    pub block_id: BlockId,
    /// Presentation label derived from the targeted block path.
    pub block_display_name: String,
    /// Deterministic icon identity used when presentation assets are omitted.
    pub block_display_icon: String,
    /// Cubic chunk containing the targeted voxel.
    pub chunk: ChunkCoordinate,
    /// Resident committed projections at inspect time.
    pub resident: u32,
    /// Projections with both mesh and collider last-applied keys.
    pub active: u32,
    /// Combined mesh and collider jobs currently in flight.
    pub in_flight: u32,
    /// Resident projections pinned because they were edited.
    pub dirty: u32,
}

impl HeadlessTargetInspectV1 {
    /// Builds an inspect DTO from a DDA hit, chunk, and working-set occupancy.
    #[must_use]
    pub fn new(
        observation: ClientTargetObservationV1,
        block_id: BlockId,
        chunk: ChunkCoordinate,
        resident: u32,
        active: u32,
        in_flight: u32,
        dirty: u32,
    ) -> Self {
        Self {
            block_display_name: missing_presentation_display_name(block_id.as_str()),
            block_display_icon: missing_presentation_icon(block_id.as_str()),
            observation,
            block_id,
            chunk,
            resident,
            active,
            in_flight,
            dirty,
        }
    }

    /// One-line occupancy fragment used by the F3 inspect overlay.
    #[must_use]
    pub fn occupancy_line(&self) -> String {
        occupancy_line(self.resident, self.active, self.in_flight, self.dirty)
    }

    /// One-line chunk coordinate used by the F3 inspect overlay.
    #[must_use]
    pub fn chunk_line(&self) -> String {
        chunk_line(self.chunk)
    }
}

/// Formats working-set occupancy for the F3 inspect overlay.
#[must_use]
pub fn occupancy_line(resident: u32, active: u32, in_flight: u32, dirty: u32) -> String {
    format!("r{resident} a{active} i{in_flight} d{dirty}")
}

/// Formats a cubic chunk coordinate for the F3 inspect overlay.
#[must_use]
pub fn chunk_line(chunk: ChunkCoordinate) -> String {
    format!("chunk {},{},{}", chunk.x, chunk.y, chunk.z)
}

fn missing_presentation_display_name(content_id: &str) -> String {
    let path = content_id
        .rsplit_once('/')
        .map_or(content_id, |(_, path)| path);
    let mut display = String::new();
    for segment in path.split('-').filter(|part| !part.is_empty()) {
        if !display.is_empty() {
            display.push(' ');
        }
        let mut chars = segment.chars();
        if let Some(first) = chars.next() {
            display.extend(first.to_uppercase());
            display.push_str(chars.as_str());
        }
    }
    if display.is_empty() {
        content_id.to_owned()
    } else {
        display
    }
}

fn missing_presentation_icon(content_id: &str) -> String {
    let Some((namespace, rest)) = content_id.split_once(':') else {
        return content_id.to_owned();
    };
    let Some((kind, path)) = rest.split_once('/') else {
        return content_id.to_owned();
    };
    format!("{namespace}:asset/icon-{kind}-{path}")
}

/// Request passed across the Lattice-owned authoritative inspect capability.
///
/// Implementations must rerun deterministic Y-up voxel DDA from `eye_pose` and
/// must not trust [`Self::client_observation`] as a hit.
#[derive(Clone, Debug, PartialEq)]
pub struct AuthoritativeTargetInspectRequestV1 {
    /// Stable player performing the request.
    pub player: PlayerId,
    /// Authoritative fixed tick of the eye pose.
    pub fixed_tick: u64,
    /// Fixed-tick eye origin and direction.
    pub eye_pose: TargetEyePoseV1,
    /// Input generation that produced the inspect edge.
    pub input_generation: u64,
    /// Optional presentation observation, used only for diagnostics.
    pub client_observation: Option<ClientTargetObservationV1>,
}

/// Stable authoritative inspect rejection.
#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum TargetInspectRejectV1 {
    /// The authoritative DDA found no selectable voxel.
    #[error("no selectable inspect target")]
    NoTarget,
    /// The candidate lies beyond the V1 reach limit.
    #[error("inspect target is outside the {maximum_mm} mm reach limit")]
    OutOfReach {
        /// Quantized reach limit used for stable diagnostics.
        maximum_mm: u16,
    },
    /// A nearer cell occludes the observed target.
    #[error("inspect target is occluded")]
    Occluded,
    /// A required registered content definition is unavailable.
    #[error("inspect content is unavailable")]
    ContentUnavailable,
    /// The committed projection could not be queried.
    #[error("inspect storage is unavailable")]
    StorageUnavailable,
}

/// Receipt emitted for every bounded inspect edge.
#[derive(Clone, Debug, Message, PartialEq)]
pub struct TargetInspectReceiptV1 {
    /// Stable player that produced the action.
    pub player: PlayerId,
    /// Fixed tick on which authority evaluated the action.
    pub fixed_tick: u64,
    /// Input generation that produced the edge.
    pub input_generation: u64,
    /// Typed authoritative result.
    pub result: Result<HeadlessTargetInspectV1, TargetInspectRejectV1>,
}

#[cfg(test)]
mod tests {
    use latticeaxiom_gameplay::{BlockPosition, ChunkCoordinate, ChunkRevision};

    use super::*;
    use crate::BlockFaceV1;

    #[test]
    fn headless_inspect_dto_carries_display_name_chunk_and_occupancy() {
        let block_id = BlockId::parse("terrenia:block/oak-log").expect("fixture block id");
        let inspect = HeadlessTargetInspectV1::new(
            ClientTargetObservationV1 {
                position: BlockPosition {
                    x: -1,
                    y: 30,
                    z: -1,
                },
                face: BlockFaceV1::NegativeY,
                chunk_revision: ChunkRevision::new(1),
                distance_mm: 1_250,
            },
            block_id.clone(),
            ChunkCoordinate::new(-1, 3, -1),
            12,
            8,
            1,
            2,
        );

        assert_eq!(inspect.block_id, block_id);
        assert_eq!(inspect.block_display_name, "Oak Log");
        assert_eq!(
            inspect.block_display_icon,
            "terrenia:asset/icon-block-oak-log"
        );
        assert_eq!(inspect.observation.position.x, -1);
        assert_eq!(inspect.observation.distance_mm, 1_250);
        assert_eq!(inspect.chunk, ChunkCoordinate::new(-1, 3, -1));
        assert_eq!(inspect.resident, 12);
        assert_eq!(inspect.active, 8);
        assert_eq!(inspect.in_flight, 1);
        assert_eq!(inspect.dirty, 2);
        assert_eq!(inspect.chunk_line(), "chunk -1,3,-1");
        assert_eq!(inspect.occupancy_line(), "r12 a8 i1 d2");
    }
}
