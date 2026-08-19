//! Pure validation and edit planning for breaking and placing blocks.

use thiserror::Error;

use crate::{Aabb, BlockId, BlockPos};

/// One validated atomic replacement in world block space.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BlockEdit {
    /// World cell to replace.
    pub position: BlockPos,
    /// Identity the caller observed before validation.
    pub previous: BlockId,
    /// Identity to store atomically.
    pub replacement: BlockId,
}

/// A block break or placement request rejected by domain rules.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Error)]
pub enum BlockEditError {
    /// Empty space cannot be broken.
    #[error("cannot break air at {position:?}")]
    CannotBreakAir {
        /// Requested target cell.
        position: BlockPos,
    },
    /// Air is not a placeable block kind.
    #[error("cannot place BlockId::AIR")]
    CannotPlaceAir,
    /// Placement target is already occupied.
    #[error("cannot place at occupied cell {position:?} containing block {existing}")]
    TargetOccupied {
        /// Requested target cell.
        position: BlockPos,
        /// Existing non-air block.
        existing: BlockId,
    },
    /// Placement would overlap the player's collision bounds.
    #[error("cannot place at {position:?} because it intersects the player")]
    IntersectsPlayer {
        /// Requested target cell.
        position: BlockPos,
    },
}

/// Validates breaking the currently observed block and returns its edit.
///
/// # Errors
///
/// Returns [`BlockEditError::CannotBreakAir`] when `current` is air.
pub fn break_block(position: BlockPos, current: BlockId) -> Result<BlockEdit, BlockEditError> {
    if current.is_air() {
        return Err(BlockEditError::CannotBreakAir { position });
    }
    Ok(BlockEdit {
        position,
        previous: current,
        replacement: BlockId::AIR,
    })
}

/// Validates placement into an observed cell without trapping the player.
///
/// Callers normally pass [`RaycastHit::placement`](crate::RaycastHit::placement)
/// as `position`. Applying the returned edit must still use compare-and-swap
/// semantics against [`BlockEdit::previous`] if another simulation actor can
/// mutate the world concurrently.
///
/// # Errors
///
/// Returns [`BlockEditError`] when the placed identity is air, the target is
/// occupied, or its unit cell overlaps `player_bounds`.
pub fn place_block(
    position: BlockPos,
    current: BlockId,
    placed: BlockId,
    player_bounds: Aabb,
) -> Result<BlockEdit, BlockEditError> {
    if placed.is_air() {
        return Err(BlockEditError::CannotPlaceAir);
    }
    if !current.is_air() {
        return Err(BlockEditError::TargetOccupied {
            position,
            existing: current,
        });
    }
    if player_bounds.intersects_block(position) {
        return Err(BlockEditError::IntersectsPlayer { position });
    }
    Ok(BlockEdit {
        position,
        previous: current,
        replacement: placed,
    })
}

#[cfg(test)]
mod tests {
    use glam::Vec3;

    use super::*;

    fn player() -> Aabb {
        Aabb::new(Vec3::new(-0.3, -0.3, 1.0), Vec3::new(0.3, 0.3, 2.8))
            .expect("test player bounds are valid")
    }

    #[test]
    fn breaking_solid_produces_air_replacement() {
        let stone = BlockId::from_raw(7);
        let position = BlockPos::new(1, 2, 3);
        assert_eq!(
            break_block(position, stone),
            Ok(BlockEdit {
                position,
                previous: stone,
                replacement: BlockId::AIR,
            })
        );
    }

    #[test]
    fn placement_refuses_player_overlap() {
        assert_eq!(
            place_block(
                BlockPos::new(-1, -1, 1),
                BlockId::AIR,
                BlockId::from_raw(4),
                player(),
            ),
            Err(BlockEditError::IntersectsPlayer {
                position: BlockPos::new(-1, -1, 1),
            })
        );
    }

    #[test]
    fn placement_allows_face_touching_player() {
        let position = BlockPos::new(0, 0, 0);
        assert!(place_block(position, BlockId::AIR, BlockId::from_raw(4), player(),).is_ok());
    }
}
