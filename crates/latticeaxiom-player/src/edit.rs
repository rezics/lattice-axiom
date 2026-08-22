use std::fmt;

use bevy::prelude::{Component, Message, Resource};
use latticeaxiom_gameplay::{BlockId, BlockPosition, ChunkRevision, PlayerId, ToolClassId};
use thiserror::Error;

/// Maximum authoritative break/place reach in meters.
pub const MAX_BLOCK_EDIT_REACH_M: f32 = 5.0;
/// Shared successful break/place cooldown at 60 Hz.
pub const SUCCESSFUL_EDIT_COOLDOWN_TICKS: u64 = 12;

/// Version-one authoritative block edit action.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[repr(u8)]
pub enum BlockEditActionV1 {
    /// Remove the first selectable voxel hit by authority.
    Break = 1,
    /// Place beside the first selectable voxel hit by authority.
    Place = 2,
}

/// Axis-aligned voxel face in Bevy's right-handed Y-up coordinates.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[repr(u8)]
pub enum BlockFaceV1 {
    /// Face whose outward normal is `+X`.
    PositiveX = 1,
    /// Face whose outward normal is `-X`.
    NegativeX = 2,
    /// Face whose outward normal is `+Y`.
    PositiveY = 3,
    /// Face whose outward normal is `-Y`.
    NegativeY = 4,
    /// Face whose outward normal is `+Z`.
    PositiveZ = 5,
    /// Face whose outward normal is `-Z`.
    NegativeZ = 6,
}

impl BlockFaceV1 {
    /// Returns the adjacent voxel, or `None` at the bounded coordinate limit.
    #[must_use]
    pub fn adjacent(self, position: BlockPosition) -> Option<BlockPosition> {
        let mut adjacent = position;
        match self {
            Self::PositiveX => adjacent.x = adjacent.x.checked_add(1)?,
            Self::NegativeX => adjacent.x = adjacent.x.checked_sub(1)?,
            Self::PositiveY => adjacent.y = adjacent.y.checked_add(1)?,
            Self::NegativeY => adjacent.y = adjacent.y.checked_sub(1)?,
            Self::PositiveZ => adjacent.z = adjacent.z.checked_add(1)?,
            Self::NegativeZ => adjacent.z = adjacent.z.checked_sub(1)?,
        }
        Some(adjacent)
    }
}

/// Non-authoritative target observation supplied by the client presentation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ClientTargetObservationV1 {
    /// Voxel the client believed it hit.
    pub position: BlockPosition,
    /// Face the client believed it hit.
    pub face: BlockFaceV1,
    /// Chunk revision observed by the client.
    pub chunk_revision: ChunkRevision,
    /// Quantized client hit distance in millimeters.
    pub distance_mm: u16,
}

/// Version-one input intent before authoritative DDA and world validation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BlockEditIntentV1 {
    /// Requested edit operation.
    pub action: BlockEditActionV1,
    /// Input generation that produced the edge.
    pub input_generation: u64,
    /// Optional stable content selected by a placement palette.
    pub placement_content: Option<BlockId>,
    /// Optional presentation observation, used only for diagnostics/prediction.
    pub client_observation: Option<ClientTargetObservationV1>,
}

/// Fixed-tick player eye pose passed to the authoritative target capability.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TargetEyePoseV1 {
    /// Local working-set origin in meters, ordered `(x, y, z)`.
    pub origin_m: [f32; 3],
    /// Unit forward direction in Bevy world coordinates.
    pub forward: [f32; 3],
}

/// Request passed across the Lattice-owned authoritative edit capability.
///
/// Implementations must rerun deterministic Y-up voxel DDA from `eye_pose`,
/// use [`Self::maximum_reach_m`], and validate revision, permission, placement
/// rules, support, occupancy, and player-capsule overlap before committing.
#[derive(Clone, Debug, PartialEq)]
pub struct AuthoritativeBlockEditRequestV1 {
    /// Stable player performing the request.
    pub player: PlayerId,
    /// Authoritative fixed tick of the eye pose.
    pub fixed_tick: u64,
    /// Fixed-tick eye origin and direction.
    pub eye_pose: TargetEyePoseV1,
    /// Stable edit intent.
    pub intent: BlockEditIntentV1,
}

impl AuthoritativeBlockEditRequestV1 {
    /// Returns the immutable V1 reach limit.
    #[must_use]
    pub const fn maximum_reach_m(&self) -> f32 {
        MAX_BLOCK_EDIT_REACH_M
    }
}

/// Stable authoritative block-edit rejection.
#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum BlockEditRejectV1 {
    /// The authoritative DDA found no selectable voxel.
    #[error("no selectable block target")]
    NoTarget,
    /// The candidate lies beyond the V1 reach limit.
    #[error("block target is outside the {maximum_mm} mm reach limit")]
    OutOfReach {
        /// Quantized reach limit used for stable diagnostics.
        maximum_mm: u16,
    },
    /// A nearer cell occludes the observed target.
    #[error("block target is occluded")]
    Occluded,
    /// A prior successful break/place still owns the shared cooldown.
    #[error("block edit cooldown has {remaining_ticks} ticks remaining")]
    Cooldown {
        /// Number of fixed ticks until another success is eligible.
        remaining_ticks: u64,
    },
    /// The client observed an older chunk revision.
    #[error("stale chunk revision: expected {expected}, actual {actual}")]
    StaleRevision {
        /// Client-observed revision.
        expected: u64,
        /// Current authoritative revision.
        actual: u64,
    },
    /// The selected voxel is not breakable.
    #[error("selected block is not breakable")]
    NotBreakable,
    /// The placement cell is not replaceable.
    #[error("placement cell is not replaceable")]
    NotReplaceable,
    /// Placement would intersect the authoritative player capsule.
    #[error("placement would intersect an actor")]
    WouldIntersectActor,
    /// The selected voxel requires a qualified tool.
    #[error("block edit requires tool class {required:?}")]
    RequiresTool {
        /// Required registered tool class.
        required: ToolClassId,
    },
    /// The selected tool has exhausted its durability.
    #[error("selected tool is broken")]
    ToolBroken,
    /// The action contributes progress but has not completed the break.
    #[error("block edit requires {remaining_work} more work units")]
    RequiresProgress {
        /// Remaining deterministic work units.
        remaining_work: u32,
    },
    /// Place was requested without stable content.
    #[error("no placement content was supplied")]
    NoPlacementContent,
    /// The caller lacks permission to mutate this world.
    #[error("block edit permission denied")]
    PermissionDenied,
    /// A required registered content definition is unavailable.
    #[error("block edit content is unavailable")]
    ContentUnavailable,
    /// The atomic world storage boundary could not commit.
    #[error("block edit storage is unavailable")]
    StorageUnavailable,
}

/// Stable result of one committed block mutation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BlockEditSuccessV1 {
    /// Coordinate committed by authority.
    pub position: BlockPosition,
    /// Stable previous content; `None` represents an empty voxel.
    pub old_content: Option<BlockId>,
    /// Stable new content; `None` represents an empty voxel.
    pub new_content: Option<BlockId>,
    /// Chunk revision after the atomic commit.
    pub committed_chunk_revision: ChunkRevision,
}

/// Receipt emitted for every bounded break/place edge.
#[derive(Clone, Debug, Message, PartialEq)]
pub struct BlockEditReceiptV1 {
    /// Stable player that produced the action.
    pub player: PlayerId,
    /// Fixed tick on which authority evaluated the action.
    pub fixed_tick: u64,
    /// Stable source action.
    pub action: BlockEditActionV1,
    /// Input generation that produced the edge.
    pub input_generation: u64,
    /// Typed authoritative result.
    pub result: Result<BlockEditSuccessV1, BlockEditRejectV1>,
}

/// Lattice-owned capability implemented by the authoritative voxel host.
pub trait BlockEditAuthority: Send + Sync + 'static {
    /// Reruns targeting, validates, and atomically commits one intent.
    ///
    /// # Errors
    ///
    /// Returns a stable rejection when targeting, validation, or commit fails.
    fn apply(
        &mut self,
        request: AuthoritativeBlockEditRequestV1,
    ) -> Result<BlockEditSuccessV1, BlockEditRejectV1>;
}

/// Bevy resource containing the active authoritative block-edit capability.
#[derive(Resource)]
pub struct BlockEditAuthorityResource(Box<dyn BlockEditAuthority>);

impl BlockEditAuthorityResource {
    /// Wraps an authoritative implementation for installation in a Bevy app.
    #[must_use]
    pub fn new(authority: impl BlockEditAuthority) -> Self {
        Self(Box::new(authority))
    }

    pub(crate) fn apply(
        &mut self,
        request: AuthoritativeBlockEditRequestV1,
    ) -> Result<BlockEditSuccessV1, BlockEditRejectV1> {
        self.0.apply(request)
    }
}

impl fmt::Debug for BlockEditAuthorityResource {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("BlockEditAuthorityResource")
            .finish_non_exhaustive()
    }
}

/// Per-player shared successful break/place cooldown.
#[derive(Clone, Copy, Component, Debug, Default, Eq, PartialEq)]
pub struct SuccessfulEditCooldownV1 {
    next_success_tick: u64,
}

impl SuccessfulEditCooldownV1 {
    /// Returns the remaining ticks, or zero when an attempt is eligible.
    #[must_use]
    pub const fn remaining_ticks(self, fixed_tick: u64) -> u64 {
        self.next_success_tick.saturating_sub(fixed_tick)
    }

    /// Records a successful mutation. Rejections must never call this method.
    pub fn record_success(&mut self, fixed_tick: u64) {
        self.next_success_tick = fixed_tick.saturating_add(SUCCESSFUL_EDIT_COOLDOWN_TICKS);
    }

    /// Returns the next tick on which a successful mutation is eligible.
    #[must_use]
    pub const fn next_success_tick(self) -> u64 {
        self.next_success_tick
    }
}

/// Descriptive alias for the shared break/place limiter component.
pub type PlayerEditLimiter = SuccessfulEditCooldownV1;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn break_and_place_share_the_twelve_tick_success_window() {
        let mut limiter = SuccessfulEditCooldownV1::default();
        limiter.record_success(20);

        assert_eq!(limiter.remaining_ticks(31), 1);
        assert_eq!(limiter.remaining_ticks(32), 0);
    }

    #[test]
    fn face_adjacency_uses_y_up_xyz_coordinates() {
        let origin = BlockPosition { x: 3, y: 5, z: 7 };
        assert_eq!(
            BlockFaceV1::PositiveY.adjacent(origin),
            Some(BlockPosition { x: 3, y: 6, z: 7 })
        );
        assert_eq!(
            BlockFaceV1::NegativeZ.adjacent(origin),
            Some(BlockPosition { x: 3, y: 5, z: 6 })
        );
    }

    #[test]
    fn face_adjacency_rejects_coordinate_overflow() {
        let maximum = BlockPosition {
            x: i32::MAX,
            y: 0,
            z: 0,
        };
        assert_eq!(BlockFaceV1::PositiveX.adjacent(maximum), None);
    }
}
