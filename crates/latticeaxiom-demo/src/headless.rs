//! GPU-free acceptance smoke for the M1-M3 vertical slice.

use std::sync::Arc;

use anyhow::{Context as _, Result};
use glam::Vec3;
use latticeaxiom_core::{BlockPos, break_block, place_block, raycast_voxels};
use latticeaxiom_render::{RenderWorld, Renderer as _};
use latticeaxiom_render_headless::HeadlessRenderer;
use latticeaxiom_storage::MemoryWorldStorage;

use crate::chunk_render::{ChunkMeshes, rebuild_pending};
use crate::player_controller::PlayerController;
use crate::runtime_config::{RuntimeConfig, project_root};
use crate::sandbox_world::SandboxWorld;

/// Runs deterministic composition, world, gameplay, and render acceptance.
///
/// The smoke materializes the initial world, submits the requested fixed-step
/// frames, breaks and places blocks through the core interaction rules, then
/// recreates the runtime over the same store and verifies both edits survived.
///
/// # Errors
///
/// Fails when any layer from Nickel composition through persistence, meshing,
/// physics, interaction, or headless render validation rejects the slice.
pub fn run(frames: u32) -> Result<()> {
    let runtime = RuntimeConfig::load(&project_root())?;
    let storage = Arc::new(MemoryWorldStorage::new());
    let mut world = SandboxWorld::new(
        storage.clone(),
        runtime.catalog.clone(),
        runtime.seed,
        runtime.producer_hash,
    )?;
    let surface_height = world.heightmap().height_at(0, 0);
    #[allow(
        clippy::cast_precision_loss,
        reason = "the demo heightmap stays far inside exact f32 integer precision"
    )]
    let spawn = Vec3::new(0.5, 0.5, surface_height as f32 + 1.9);
    let mut player = PlayerController::spawn(spawn, 0.0, -0.2)
        .context("fixed headless spawn must form a valid player body")?;
    let _ = world.center_on(player.center_block());
    world.stream_all()?;

    let mut renderer = HeadlessRenderer::new(1280, 720);
    let mut chunks = ChunkMeshes::new(&mut renderer)?;
    rebuild_pending(&mut renderer, &mut chunks, &mut world, usize::MAX)?;

    for _ in 0..frames {
        player.advance(1.0 / 60.0, |block| world.is_collision_solid_at(block))?;
        let residency = world.center_on(player.center_block());
        for position in residency.unloaded {
            chunks.unload(position);
        }
        world.stream(4)?;
        rebuild_pending(&mut renderer, &mut chunks, &mut world, 6)?;
        renderer.submit(&RenderWorld {
            camera: player.camera(),
            instances: chunks.instances()?,
        })?;
    }

    let instances_drawn =
        exercise_residency_boundaries(&mut world, &mut chunks, &mut renderer, &player)?;

    let target_x = 4;
    let target_y = 0;
    let target_height = world.heightmap().height_at(target_x, target_y);
    #[allow(
        clippy::cast_precision_loss,
        reason = "the demo heightmap stays far inside exact f32 integer precision"
    )]
    let ray_origin = Vec3::new(
        target_x as f32 + 0.5,
        target_y as f32 + 0.5,
        target_height as f32 + 4.0,
    );
    let hit = raycast_voxels(ray_origin, -Vec3::Z, 8.0, |block| world.is_solid_at(block))
        .context("headless interaction ray must reach generated terrain")?;
    let break_edit = break_block(hit.block, world.block_at(hit.block))?;
    world.set_block_durable(break_edit.position, break_edit.replacement)?;

    let placed = world.catalog().selected();
    let place_edit = place_block(
        hit.placement,
        world.block_at(hit.placement),
        placed,
        player.body().bounds(),
    )?;
    world.set_block_durable(place_edit.position, place_edit.replacement)?;

    drop(chunks);
    drop(renderer);
    drop(world);

    let mut reopened = SandboxWorld::new(
        storage,
        runtime.catalog,
        runtime.seed,
        runtime.producer_hash,
    )?;
    let _ = reopened.center_on(BlockPos::new(target_x, target_y, target_height + 2));
    reopened.stream_all()?;
    anyhow::ensure!(
        reopened.block_at(break_edit.position).is_air(),
        "durable headless break did not survive runtime recreation"
    );
    anyhow::ensure!(
        reopened.block_at(place_edit.position) == placed,
        "durable headless placement did not survive runtime recreation"
    );

    tracing::info!(
        frames,
        instances_per_frame = instances_drawn,
        resident_chunks = reopened.loaded_chunks(),
        "M1-M3 headless acceptance completed"
    );
    Ok(())
}

fn exercise_residency_boundaries(
    world: &mut SandboxWorld,
    chunks: &mut ChunkMeshes,
    renderer: &mut HeadlessRenderer,
    player: &PlayerController,
) -> Result<usize> {
    // Cross both signs and several chunk boundaries without depending on wall
    // clock duration. This exercises unload/remesh, negative-coordinate keys,
    // and renderer slot reuse in every short CI smoke.
    let residency_capacity = world.loaded_chunks();
    let mut instances_drawn = 0;
    for center_x in [33, -33, 0] {
        let center_height = world.heightmap().height_at(center_x, 0);
        let residency = world.center_on(BlockPos::new(center_x, 0, center_height + 2));
        for position in residency.unloaded {
            chunks.unload(position);
        }
        world.stream_all()?;
        rebuild_pending(renderer, chunks, world, usize::MAX)?;
        anyhow::ensure!(
            world.loaded_chunks() == residency_capacity,
            "streaming changed the bounded residency capacity"
        );
        anyhow::ensure!(
            chunks.allocated_slots() <= residency_capacity,
            "chunk renderer handles grew beyond the residency high-water mark"
        );
        instances_drawn = renderer
            .submit(&RenderWorld {
                camera: player.camera(),
                instances: chunks.instances()?,
            })?
            .instances_drawn;
    }
    Ok(instances_drawn)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn headless_acceptance_completes() {
        run(3).expect("checked-in M1-M3 vertical slice must pass headless acceptance");
    }
}
