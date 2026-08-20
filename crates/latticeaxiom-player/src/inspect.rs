use bevy::prelude::Message;
use latticeaxiom_gameplay::{BlockId, PlayerId};
use thiserror::Error;

use crate::{ClientTargetObservationV1, TargetEyePoseV1};

/// Headless inspect DTO produced after authoritative Y-up voxel DDA.
///
/// The observation copies the DDA hit for presentation diagnostics. Authority
/// never treats [`ClientTargetObservationV1`] as the selected target.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HeadlessTargetInspectV1 {
    /// Voxel, face, revision, and quantized distance of the DDA hit.
    pub observation: ClientTargetObservationV1,
    /// Stable content of the targeted voxel.
    pub block_id: BlockId,
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
    use latticeaxiom_gameplay::{BlockPosition, ChunkRevision};

    use super::*;
    use crate::BlockFaceV1;

    #[test]
    fn headless_inspect_dto_carries_the_dda_observation_and_block_id() {
        let block_id = BlockId::parse("terrenia:block/stone").expect("fixture block id");
        let inspect = HeadlessTargetInspectV1 {
            observation: ClientTargetObservationV1 {
                position: BlockPosition {
                    x: -1,
                    y: 30,
                    z: -1,
                },
                face: BlockFaceV1::NegativeY,
                chunk_revision: ChunkRevision::new(1),
                distance_mm: 1_250,
            },
            block_id: block_id.clone(),
        };

        assert_eq!(inspect.block_id, block_id);
        assert_eq!(inspect.observation.position.x, -1);
        assert_eq!(inspect.observation.distance_mm, 1_250);
    }
}
