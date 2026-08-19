//! Bounded renderer-handle pool for streamed chunk meshes.

use std::collections::BTreeMap;

use anyhow::{Context as _, Result};
use glam::{Mat4, Vec3};
use latticeaxiom_core::ChunkPos;
use latticeaxiom_render::{MaterialData, MaterialId, MeshData, MeshId, MeshInstance, Renderer};

use crate::chunk_mesh;
use crate::sandbox_world::SandboxWorld;

/// Render resources owned by the currently resident chunk set.
#[derive(Debug)]
pub struct ChunkMeshes {
    material: MaterialId,
    active: BTreeMap<ChunkPos, MeshId>,
    free: Vec<MeshId>,
}

impl ChunkMeshes {
    /// Uploads the shared white tint used with per-vertex block colors.
    pub fn new(renderer: &mut impl Renderer) -> Result<Self> {
        let material = renderer
            .upload_material(&MaterialData {
                base_color: [1.0; 4],
            })
            .context("failed to upload chunk material")?;
        Ok(Self {
            material,
            active: BTreeMap::new(),
            free: Vec::new(),
        })
    }

    /// Installs, replaces, or removes one chunk mesh.
    pub fn update(
        &mut self,
        renderer: &mut impl Renderer,
        position: ChunkPos,
        mesh: Option<&MeshData>,
    ) -> Result<()> {
        match (self.active.get(&position).copied(), mesh) {
            (Some(id), Some(mesh)) => renderer
                .replace_mesh(id, mesh)
                .context("failed to replace chunk mesh")?,
            (Some(id), None) => {
                self.active.remove(&position);
                self.free.push(id);
            }
            (None, Some(mesh)) => {
                let id = if let Some(id) = self.free.pop() {
                    renderer
                        .replace_mesh(id, mesh)
                        .context("failed to reuse chunk mesh slot")?;
                    id
                } else {
                    renderer
                        .upload_mesh(mesh)
                        .context("failed to upload chunk mesh")?
                };
                self.active.insert(position, id);
            }
            (None, None) => {}
        }
        Ok(())
    }

    /// Returns an unloaded chunk's handle to the reuse pool.
    pub fn unload(&mut self, position: ChunkPos) {
        if let Some(id) = self.active.remove(&position) {
            self.free.push(id);
        }
    }

    /// Extracts stable render instances for all non-empty resident chunks.
    pub fn instances(&self) -> Result<Vec<MeshInstance>> {
        self.active
            .iter()
            .map(|(position, &mesh)| {
                let origin = position
                    .origin()
                    .context("rendered chunk origin exceeds i32 world range")?;
                #[allow(
                    clippy::cast_precision_loss,
                    reason = "playable chunk coordinates remain far inside f32 exact integer range"
                )]
                let translation = Vec3::new(origin.x as f32, origin.y as f32, origin.z as f32);
                Ok(MeshInstance {
                    mesh,
                    material: self.material,
                    transform: Mat4::from_translation(translation),
                })
            })
            .collect()
    }

    /// Number of visible, non-empty chunk meshes.
    #[must_use]
    pub fn active_len(&self) -> usize {
        self.active.len()
    }

    /// Total mesh handles ever allocated; bounded by the residency high-water
    /// mark because unloaded handles are reused.
    #[must_use]
    pub fn allocated_slots(&self) -> usize {
        self.active.len() + self.free.len()
    }
}

/// Rebuilds a bounded batch of dirty authoritative chunks into render slots.
///
/// # Errors
///
/// Fails when world-coordinate conversion, meshing, or renderer upload fails.
pub fn rebuild_pending(
    renderer: &mut impl Renderer,
    meshes: &mut ChunkMeshes,
    world: &mut SandboxWorld,
    budget: usize,
) -> Result<usize> {
    let positions = world.take_dirty_meshes(budget);
    for position in &positions {
        let mesh = chunk_mesh::compile(
            *position,
            |block| world.block_at(block),
            |block| world.catalog().is_solid(block),
            |block| world.catalog().color(block),
        )?;
        meshes.update(renderer, *position, mesh.as_ref())?;
    }
    Ok(positions.len())
}

#[cfg(test)]
mod tests {
    use latticeaxiom_render::conformance::test_triangle;
    use latticeaxiom_render_headless::HeadlessRenderer;

    use super::*;

    #[test]
    fn unloaded_slots_are_reused() {
        let mut renderer = HeadlessRenderer::new(64, 64);
        let mut chunks = ChunkMeshes::new(&mut renderer).expect("material upload must succeed");
        chunks
            .update(
                &mut renderer,
                ChunkPos::new(0, 0, 0),
                Some(&test_triangle()),
            )
            .expect("first mesh upload must succeed");
        chunks.unload(ChunkPos::new(0, 0, 0));
        chunks
            .update(
                &mut renderer,
                ChunkPos::new(1, 0, 0),
                Some(&test_triangle()),
            )
            .expect("slot reuse must succeed");

        assert_eq!(chunks.active_len(), 1);
        assert_eq!(chunks.allocated_slots(), 1);
    }
}
