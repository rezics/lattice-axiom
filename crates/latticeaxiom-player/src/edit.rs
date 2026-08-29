use std::fmt;

use bevy::prelude::{Component, Message, Resource};
use latticeaxiom_gameplay::{
    BlockId, BlockPosition, ChunkRevision, MiningStepCountV1, PlayerId, ToolClassId,
};
use thiserror::Error;

/// Maximum authoritative break/place reach in meters.
pub const MAX_BLOCK_EDIT_REACH_M: f32 = 5.0;
/// Canonical mining cadence, independent from the configurable simulation rate.
pub const CANONICAL_MINING_STEPS_PER_SECOND: u16 = 60;

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
    /// Canonical 60 Hz work steps represented by this request.
    pub mining_steps: MiningStepCountV1,
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

/// Request to clear one player's transient mining target on button release.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AuthoritativeMiningCancelRequestV1 {
    /// Stable player releasing the mining action.
    pub player: PlayerId,
    /// Authoritative fixed tick of the release observation.
    pub fixed_tick: u64,
    /// Input generation that observed the release.
    pub input_generation: u64,
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

/// Stable result of cancelling one transient mining target.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct MiningCancelSuccessV1 {
    /// Whether accumulated progress existed and was cleared.
    pub had_progress: bool,
}

/// Receipt emitted when a held mining action is released.
#[derive(Clone, Debug, Message, PartialEq)]
pub struct MiningCancelReceiptV1 {
    /// Stable player that released mining.
    pub player: PlayerId,
    /// Fixed tick on which authority evaluated cancellation.
    pub fixed_tick: u64,
    /// Input generation that observed the release.
    pub input_generation: u64,
    /// Typed authoritative result.
    pub result: Result<MiningCancelSuccessV1, BlockEditRejectV1>,
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

    /// Clears transient mining progress after the break action is released.
    ///
    /// # Errors
    ///
    /// Returns a stable rejection when authoritative cancellation cannot be
    /// completed. Stateless authorities may keep the default no-op behavior.
    fn cancel_mining(
        &mut self,
        _request: AuthoritativeMiningCancelRequestV1,
    ) -> Result<MiningCancelSuccessV1, BlockEditRejectV1> {
        Ok(MiningCancelSuccessV1::default())
    }
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

    pub(crate) fn cancel_mining(
        &mut self,
        request: AuthoritativeMiningCancelRequestV1,
    ) -> Result<MiningCancelSuccessV1, BlockEditRejectV1> {
        self.0.cancel_mining(request)
    }
}

impl fmt::Debug for BlockEditAuthorityResource {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("BlockEditAuthorityResource")
            .finish_non_exhaustive()
    }
}

/// Per-player held-mining cadence state.
#[derive(Clone, Copy, Component, Debug, Default, Eq, PartialEq)]
pub struct BlockEditInputStateV1 {
    break_was_active: bool,
    mining_phase: u32,
    cadence_rate_hz: u16,
}

impl BlockEditInputStateV1 {
    pub(crate) fn sample_break(
        &mut self,
        active: bool,
        simulation_rate_hz: u16,
    ) -> BreakCadenceSample {
        let simulation_rate_hz = simulation_rate_hz.max(1);
        if !active {
            let released = self.break_was_active;
            self.break_was_active = false;
            self.mining_phase = 0;
            self.cadence_rate_hz = simulation_rate_hz;
            return BreakCadenceSample {
                steps: None,
                released,
            };
        }
        if !self.break_was_active {
            self.break_was_active = true;
            let canonical_rate = u32::from(CANONICAL_MINING_STEPS_PER_SECOND);
            let simulation_rate = u32::from(simulation_rate_hz);
            let steps = (canonical_rate / simulation_rate).max(1);
            self.mining_phase = if canonical_rate >= simulation_rate {
                canonical_rate % simulation_rate
            } else {
                0
            };
            self.cadence_rate_hz = simulation_rate_hz;
            return BreakCadenceSample {
                steps: u16::try_from(steps).ok().and_then(MiningStepCountV1::new),
                released: false,
            };
        }
        if self.cadence_rate_hz != simulation_rate_hz {
            self.mining_phase = self
                .mining_phase
                .saturating_mul(u32::from(simulation_rate_hz))
                / u32::from(self.cadence_rate_hz.max(1));
            self.cadence_rate_hz = simulation_rate_hz;
        }
        self.mining_phase = self
            .mining_phase
            .saturating_add(u32::from(CANONICAL_MINING_STEPS_PER_SECOND));
        let steps = self.mining_phase / u32::from(simulation_rate_hz);
        self.mining_phase %= u32::from(simulation_rate_hz);
        BreakCadenceSample {
            steps: u16::try_from(steps).ok().and_then(MiningStepCountV1::new),
            released: false,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct BreakCadenceSample {
    pub(crate) steps: Option<MiningStepCountV1>,
    pub(crate) released: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mining_cadence_is_sixty_steps_per_second_at_any_tick_rate() {
        for rate in [30, 60, 240, 10_000] {
            let mut cadence = BlockEditInputStateV1::default();
            let mut steps = 0_u32;
            for _ in 0..rate {
                steps += cadence
                    .sample_break(true, rate)
                    .steps
                    .map_or(0, |batch| u32::from(batch.get()));
            }
            assert_eq!(steps, 60, "unexpected mining work at {rate} Hz");
        }
    }

    #[test]
    fn mining_release_is_reported_once_and_resets_phase() {
        let mut cadence = BlockEditInputStateV1::default();
        assert_eq!(
            cadence.sample_break(true, 240).steps,
            Some(MiningStepCountV1::ONE)
        );
        assert!(cadence.sample_break(false, 240).released);
        assert!(!cadence.sample_break(false, 240).released);
        assert_eq!(
            cadence.sample_break(true, 240).steps,
            Some(MiningStepCountV1::ONE)
        );
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
