//! Non-durable block authority for the single-session playable fixture.
//!
//! Cells use voxel minimum-corner coordinates: a cell at `(x, y, z)` occupies
//! `[x, x + 1) x [y, y + 1) x [z, z + 1)` in Bevy world space. This module is
//! intentionally not a persistence or package-authority implementation.

use std::collections::{BTreeMap, BTreeSet};

use bevy::prelude::Resource;
use latticeaxiom_gameplay::{
    BlockId, BlockPosition, ChunkCoordinate, ChunkRevision, GameplayIdError,
};
use latticeaxiom_player::{
    AuthoritativeBlockEditRequestV1, BlockEditActionV1, BlockEditAuthority, BlockEditRejectV1,
    BlockEditSuccessV1, BlockFaceV1, ClientTargetObservationV1, MAX_BLOCK_EDIT_REACH_M,
    TargetEyePoseV1,
};

const WORLD_HORIZONTAL_MIN: i32 = -8;
const WORLD_HORIZONTAL_MAX: i32 = 8;
const WORLD_VERTICAL_MIN: i32 = 0;
const WORLD_VERTICAL_MAX: i32 = 8;
const MAX_DDA_STEPS: usize = 32;
const MAX_BLOCK_EDIT_REACH_MM: u16 = 5_000;

/// Immutable initial cell snapshot for the non-durable playable scene.
///
/// Runtime mutations are reported by block-edit receipts rather than written
/// back into this seed resource.
#[derive(Clone, Debug, Resource)]
pub(super) struct PlayableSeedCells {
    cells: BTreeMap<BlockPosition, BlockId>,
}

impl PlayableSeedCells {
    /// Iterates initial cells in stable coordinate order.
    pub(super) fn iter(&self) -> impl ExactSizeIterator<Item = (BlockPosition, &BlockId)> + '_ {
        self.cells
            .iter()
            .map(|(position, block)| (*position, block))
    }
}

/// Authoritative, in-memory cells for one non-durable development session.
///
/// The authority owns a stable finite terrain, performs first-hit voxel DDA,
/// and advances only the revision of the chunk containing a committed edit.
#[derive(Debug)]
pub(super) struct PlayableBlockAuthority {
    cells: BTreeMap<BlockPosition, BlockId>,
    chunk_revisions: BTreeMap<ChunkCoordinate, ChunkRevision>,
    registered_blocks: BTreeSet<BlockId>,
}

/// Creates the deterministic Terrenia development world and default placement.
///
/// This fixture is deliberately single-session and non-durable. Its returned
/// seed cells are an immutable presentation snapshot; successful edit receipts
/// carry every later scene mutation.
///
/// # Errors
///
/// Returns [`GameplayIdError`] if a built-in Terrenia block identifier ceases
/// to satisfy the canonical gameplay-ID contract.
pub(super) fn seeded_playable_world()
-> Result<(PlayableBlockAuthority, PlayableSeedCells, BlockId), GameplayIdError> {
    let grass = BlockId::parse("terrenia:block/grass")?;
    let dirt = BlockId::parse("terrenia:block/dirt")?;
    let stone = BlockId::parse("terrenia:block/stone")?;
    let sand = BlockId::parse("terrenia:block/sand")?;

    let mut cells = BTreeMap::new();
    for x in WORLD_HORIZONTAL_MIN..=WORLD_HORIZONTAL_MAX {
        for z in WORLD_HORIZONTAL_MIN..=WORLD_HORIZONTAL_MAX {
            let surface_y = stable_surface_height(x, z);
            cells.insert(BlockPosition { x, y: 0, z }, stone.clone());
            if surface_y == 2 {
                cells.insert(BlockPosition { x, y: 1, z }, dirt.clone());
            }
            let surface = if is_sand_patch(x, z) {
                sand.clone()
            } else {
                grass.clone()
            };
            cells.insert(BlockPosition { x, y: surface_y, z }, surface);
        }
    }

    let registered_blocks = [grass, dirt.clone(), stone, sand].into_iter().collect();
    let seed = PlayableSeedCells {
        cells: cells.clone(),
    };
    let authority = PlayableBlockAuthority {
        cells,
        chunk_revisions: BTreeMap::new(),
        registered_blocks,
    };
    Ok((authority, seed, dirt))
}

impl BlockEditAuthority for PlayableBlockAuthority {
    fn apply(
        &mut self,
        request: AuthoritativeBlockEditRequestV1,
    ) -> Result<BlockEditSuccessV1, BlockEditRejectV1> {
        let hit = self
            .first_hit(request.eye_pose, request.maximum_reach_m())
            .ok_or(BlockEditRejectV1::NoTarget)?;
        let hit_revision = self.chunk_revision(hit.position.chunk());
        validate_client_observation(
            request.intent.client_observation.as_ref(),
            hit,
            hit_revision,
        )?;

        match request.intent.action {
            BlockEditActionV1::Break => self.commit_break(hit),
            BlockEditActionV1::Place => {
                let placement = request
                    .intent
                    .placement_content
                    .ok_or(BlockEditRejectV1::NoPlacementContent)?;
                self.commit_place(hit, placement, request.eye_pose)
            }
        }
    }
}

impl PlayableBlockAuthority {
    fn first_hit(&self, eye_pose: TargetEyePoseV1, maximum_reach_m: f32) -> Option<RayHit> {
        let [origin_x, origin_y, origin_z] = eye_pose.origin_m;
        let [forward_x, forward_y, forward_z] = eye_pose.forward;
        if ![
            origin_x, origin_y, origin_z, forward_x, forward_y, forward_z,
        ]
        .into_iter()
        .all(f32::is_finite)
        {
            return None;
        }

        let length_squared = forward_x.mul_add(
            forward_x,
            forward_y.mul_add(forward_y, forward_z * forward_z),
        );
        if length_squared <= f32::EPSILON {
            return None;
        }
        let inverse_length = length_squared.sqrt().recip();
        let direction = [
            forward_x * inverse_length,
            forward_y * inverse_length,
            forward_z * inverse_length,
        ];
        let mut cell = BlockPosition {
            x: bounded_floor(origin_x)?,
            y: bounded_floor(origin_y)?,
            z: bounded_floor(origin_z)?,
        };
        let steps = direction.map(axis_step);
        let mut crossing_distance = [
            first_boundary_distance(origin_x, cell.x, direction[0], steps[0]),
            first_boundary_distance(origin_y, cell.y, direction[1], steps[1]),
            first_boundary_distance(origin_z, cell.z, direction[2], steps[2]),
        ];
        let boundary_stride = direction.map(|axis| {
            if axis == 0.0 {
                f32::INFINITY
            } else {
                axis.abs().recip()
            }
        });
        let mut distance_m = 0.0;
        let mut entry_face = face_opposite_major_axis(direction);

        for _ in 0..MAX_DDA_STEPS {
            if self.cells.contains_key(&cell) {
                return Some(RayHit {
                    position: cell,
                    face: entry_face,
                    distance_m,
                });
            }

            let axis = least_crossing_axis(crossing_distance);
            let next_distance = crossing_distance[axis];
            if !next_distance.is_finite() || next_distance > maximum_reach_m {
                return None;
            }
            entry_face = entry_face_for_step(axis, steps[axis]);
            advance_cell_axis(&mut cell, axis, steps[axis])?;
            crossing_distance[axis] += boundary_stride[axis];
            distance_m = next_distance.max(0.0);
        }
        None
    }

    fn commit_break(&mut self, hit: RayHit) -> Result<BlockEditSuccessV1, BlockEditRejectV1> {
        let Some(old_content) = self.cells.get(&hit.position).cloned() else {
            return Err(BlockEditRejectV1::NoTarget);
        };
        if !self.registered_blocks.contains(&old_content) {
            return Err(BlockEditRejectV1::NotBreakable);
        }
        let chunk = hit.position.chunk();
        let next_revision = self.next_chunk_revision(chunk)?;
        if self.cells.remove(&hit.position).is_none() {
            return Err(BlockEditRejectV1::StorageUnavailable);
        }
        self.chunk_revisions.insert(chunk, next_revision);
        Ok(BlockEditSuccessV1 {
            position: hit.position,
            old_content: Some(old_content),
            new_content: None,
            committed_chunk_revision: next_revision,
        })
    }

    fn commit_place(
        &mut self,
        hit: RayHit,
        placement: BlockId,
        eye_pose: TargetEyePoseV1,
    ) -> Result<BlockEditSuccessV1, BlockEditRejectV1> {
        if !self.registered_blocks.contains(&placement) {
            return Err(BlockEditRejectV1::ContentUnavailable);
        }
        let position = hit
            .face
            .adjacent(hit.position)
            .ok_or(BlockEditRejectV1::PermissionDenied)?;
        if !inside_edit_bounds(position) {
            return Err(BlockEditRejectV1::PermissionDenied);
        }
        if self.cells.contains_key(&position) {
            return Err(BlockEditRejectV1::NotReplaceable);
        }
        if cell_intersects_player(position, eye_pose.origin_m) {
            return Err(BlockEditRejectV1::WouldIntersectActor);
        }

        let chunk = position.chunk();
        let next_revision = self.next_chunk_revision(chunk)?;
        self.cells.insert(position, placement.clone());
        self.chunk_revisions.insert(chunk, next_revision);
        Ok(BlockEditSuccessV1 {
            position,
            old_content: None,
            new_content: Some(placement),
            committed_chunk_revision: next_revision,
        })
    }

    fn chunk_revision(&self, chunk: ChunkCoordinate) -> ChunkRevision {
        self.chunk_revisions
            .get(&chunk)
            .copied()
            .unwrap_or(ChunkRevision::ZERO)
    }

    fn next_chunk_revision(
        &self,
        chunk: ChunkCoordinate,
    ) -> Result<ChunkRevision, BlockEditRejectV1> {
        self.chunk_revision(chunk)
            .get()
            .checked_add(1)
            .map(ChunkRevision::new)
            .ok_or(BlockEditRejectV1::StorageUnavailable)
    }
}

#[derive(Clone, Copy)]
struct RayHit {
    position: BlockPosition,
    face: BlockFaceV1,
    distance_m: f32,
}

fn validate_client_observation(
    observation: Option<&ClientTargetObservationV1>,
    hit: RayHit,
    actual_revision: ChunkRevision,
) -> Result<(), BlockEditRejectV1> {
    let Some(observation) = observation else {
        return Ok(());
    };
    if observation.distance_mm > MAX_BLOCK_EDIT_REACH_MM || hit.distance_m > MAX_BLOCK_EDIT_REACH_M
    {
        return Err(BlockEditRejectV1::OutOfReach {
            maximum_mm: MAX_BLOCK_EDIT_REACH_MM,
        });
    }
    if observation.position != hit.position || observation.face != hit.face {
        return Err(BlockEditRejectV1::Occluded);
    }
    if observation.chunk_revision != actual_revision {
        return Err(BlockEditRejectV1::StaleRevision {
            expected: observation.chunk_revision.get(),
            actual: actual_revision.get(),
        });
    }
    Ok(())
}

const fn stable_surface_height(x: i32, z: i32) -> i32 {
    let variation = (x * 31 + z * 17 + x * z * 7).rem_euclid(13);
    if variation <= 2 { 2 } else { 1 }
}

const fn is_sand_patch(x: i32, z: i32) -> bool {
    let offset_x = x - 5;
    let offset_z = z + 5;
    offset_x * offset_x + offset_z * offset_z <= 10
}

const fn inside_edit_bounds(position: BlockPosition) -> bool {
    position.x >= WORLD_HORIZONTAL_MIN
        && position.x <= WORLD_HORIZONTAL_MAX
        && position.y >= WORLD_VERTICAL_MIN
        && position.y <= WORLD_VERTICAL_MAX
        && position.z >= WORLD_HORIZONTAL_MIN
        && position.z <= WORLD_HORIZONTAL_MAX
}

fn axis_step(axis: f32) -> i32 {
    if axis > 0.0 {
        1
    } else if axis < 0.0 {
        -1
    } else {
        0
    }
}

#[allow(clippy::cast_possible_truncation)]
fn bounded_floor(value: f32) -> Option<i32> {
    const MIN_RAY_COORDINATE: f32 = -32.0;
    const MAX_RAY_COORDINATE: f32 = 32.0;
    if !(MIN_RAY_COORDINATE..=MAX_RAY_COORDINATE).contains(&value) {
        return None;
    }
    Some(value.floor() as i32)
}

#[allow(clippy::cast_precision_loss)]
fn first_boundary_distance(origin: f32, cell: i32, direction: f32, step: i32) -> f32 {
    match step {
        1 => ((cell + 1) as f32 - origin) / direction,
        -1 => (origin - cell as f32) / -direction,
        _ => f32::INFINITY,
    }
}

const fn least_crossing_axis(distances: [f32; 3]) -> usize {
    if distances[0] <= distances[1] && distances[0] <= distances[2] {
        0
    } else if distances[1] <= distances[2] {
        1
    } else {
        2
    }
}

fn advance_cell_axis(cell: &mut BlockPosition, axis: usize, step: i32) -> Option<()> {
    match axis {
        0 => cell.x = cell.x.checked_add(step)?,
        1 => cell.y = cell.y.checked_add(step)?,
        2 => cell.z = cell.z.checked_add(step)?,
        _ => return None,
    }
    Some(())
}

fn entry_face_for_step(axis: usize, step: i32) -> BlockFaceV1 {
    match (axis, step) {
        (0, 1) => BlockFaceV1::NegativeX,
        (0, _) => BlockFaceV1::PositiveX,
        (1, 1) => BlockFaceV1::NegativeY,
        (1, _) => BlockFaceV1::PositiveY,
        (2, 1) => BlockFaceV1::NegativeZ,
        _ => BlockFaceV1::PositiveZ,
    }
}

fn face_opposite_major_axis(direction: [f32; 3]) -> BlockFaceV1 {
    let absolute = direction.map(f32::abs);
    let axis = if absolute[0] >= absolute[1] && absolute[0] >= absolute[2] {
        0
    } else if absolute[1] >= absolute[2] {
        1
    } else {
        2
    };
    entry_face_for_step(axis, axis_step(direction[axis]))
}

#[allow(clippy::cast_precision_loss)]
fn cell_intersects_player(position: BlockPosition, eye_origin: [f32; 3]) -> bool {
    const PLAYER_RADIUS_M: f32 = 0.42;
    const PLAYER_HEIGHT_BELOW_EYE_M: f32 = 1.7;
    const PLAYER_HEIGHT_ABOVE_EYE_M: f32 = 0.15;

    let cell_min_x = position.x as f32;
    let cell_min_y = position.y as f32;
    let cell_min_z = position.z as f32;
    let cell_max_x = cell_min_x + 1.0;
    let cell_max_y = cell_min_y + 1.0;
    let cell_max_z = cell_min_z + 1.0;
    let actor_min_y = eye_origin[1] - PLAYER_HEIGHT_BELOW_EYE_M;
    let actor_max_y = eye_origin[1] + PLAYER_HEIGHT_ABOVE_EYE_M;
    let overlaps_vertically = cell_max_y > actor_min_y && cell_min_y < actor_max_y;
    if !overlaps_vertically {
        return false;
    }

    let nearest_x = eye_origin[0].clamp(cell_min_x, cell_max_x);
    let nearest_z = eye_origin[2].clamp(cell_min_z, cell_max_z);
    let offset_x = eye_origin[0] - nearest_x;
    let offset_z = eye_origin[2] - nearest_z;
    offset_x.mul_add(offset_x, offset_z * offset_z) < PLAYER_RADIUS_M * PLAYER_RADIUS_M
}
