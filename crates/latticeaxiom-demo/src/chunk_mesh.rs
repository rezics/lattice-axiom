//! Conversion from authoritative chunk samples to renderer mesh data.

use anyhow::{Context as _, Result};
use latticeaxiom_core::{BlockId, BlockPos, CHUNK_EDGE, ChunkPos};
use latticeaxiom_render::MeshData;
use latticeaxiom_voxel_mesh::{Face, MergeVoxel, PaddedDims, Voxel, greedy_quads};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct MeshVoxel {
    id: BlockId,
    opaque: bool,
}

impl Voxel for MeshVoxel {
    fn is_opaque(&self) -> bool {
        self.opaque
    }
}

impl MergeVoxel for MeshVoxel {
    type MergeValue = BlockId;

    fn merge_value(&self) -> Self::MergeValue {
        self.id
    }
}

/// Compiles one chunk with a one-block neighbor apron.
///
/// `block_at` must expose adjacent loaded/generated samples so faces at chunk
/// seams are culled consistently. `is_opaque` and `color_for` resolve
/// already-registered numeric IDs; no package strings enter the meshing loop.
pub fn compile(
    position: ChunkPos,
    mut block_at: impl FnMut(BlockPos) -> BlockId,
    mut is_opaque: impl FnMut(BlockId) -> bool,
    mut color_for: impl FnMut(BlockId) -> [f32; 4],
) -> Result<Option<MeshData>> {
    let origin = position
        .origin()
        .context("chunk origin exceeds the supported i32 world range")?;
    let padded_edge = usize::try_from(CHUNK_EDGE)
        .context("chunk edge must fit usize")?
        .saturating_add(2);
    let dims = PaddedDims::new([padded_edge; 3]);
    let mut samples = vec![MeshVoxel::default(); dims.volume_len()];

    for z in 0..padded_edge {
        for y in 0..padded_edge {
            for x in 0..padded_edge {
                let world = BlockPos::new(
                    origin
                        .x
                        .checked_add(padded_offset(x))
                        .context("padded chunk x exceeds i32 world range")?,
                    origin
                        .y
                        .checked_add(padded_offset(y))
                        .context("padded chunk y exceeds i32 world range")?,
                    origin
                        .z
                        .checked_add(padded_offset(z))
                        .context("padded chunk z exceeds i32 world range")?,
                );
                let id = block_at(world);
                samples[dims.linearize([x, y, z])] = MeshVoxel {
                    id,
                    opaque: is_opaque(id),
                };
            }
        }
    }

    let quads = greedy_quads(&samples, dims);
    if quads.num_quads() == 0 {
        return Ok(None);
    }

    let mut mesh = MeshData {
        positions: Vec::with_capacity(quads.num_quads() * 4),
        normals: Vec::with_capacity(quads.num_quads() * 4),
        colors: Vec::with_capacity(quads.num_quads() * 4),
        indices: Vec::with_capacity(quads.num_quads() * 6),
    };

    for (face, quad) in quads.iter() {
        let sample = [
            usize::try_from(quad.minimum[0]).context("quad x must fit usize")? + 1,
            usize::try_from(quad.minimum[1]).context("quad y must fit usize")? + 1,
            usize::try_from(quad.minimum[2]).context("quad z must fit usize")? + 1,
        ];
        let block = samples[dims.linearize(sample)].id;
        let color = color_for(block);
        let base =
            u32::try_from(mesh.positions.len()).context("chunk mesh exceeds u32 vertices")?;
        mesh.positions.extend(face.quad_positions(quad));
        mesh.normals.extend(face.quad_normals());
        mesh.colors.extend([color; 4]);
        mesh.indices.extend(Face::quad_indices(base));
    }

    Ok(Some(mesh))
}

#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    reason = "padded chunk coordinates are bounded to CHUNK_EDGE + 2"
)]
const fn padded_offset(value: usize) -> i32 {
    value as i32 - 1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_block_emits_colored_cube() {
        let stone = BlockId::from_raw(1);
        let mesh = compile(
            ChunkPos::default(),
            |position| {
                if position == BlockPos::new(0, 0, 0) {
                    stone
                } else {
                    BlockId::AIR
                }
            },
            |block| !block.is_air(),
            |_| [0.25, 0.5, 0.75, 1.0],
        )
        .expect("bounded test chunk must compile")
        .expect("one solid block must produce a mesh");

        assert_eq!(mesh.positions.len(), 24);
        assert_eq!(mesh.indices.len(), 36);
        assert!(mesh.colors.iter().all(|color| {
            color
                .iter()
                .zip([0.25, 0.5, 0.75, 1.0])
                .all(|(actual, expected)| (*actual - expected).abs() <= f32::EPSILON)
        }));
        mesh.validate()
            .expect("compiled mesh must satisfy the facade");
    }

    #[test]
    fn solid_neighbor_apron_culls_chunk_seam() {
        let stone = BlockId::from_raw(1);
        let mesh = compile(ChunkPos::default(), |_| stone, |_| true, |_| [1.0; 4])
            .expect("bounded test chunk must compile");
        assert!(mesh.is_none(), "a solid apron hides every interior face");
    }

    #[test]
    fn non_opaque_non_air_registration_does_not_emit_geometry() {
        let decorative = BlockId::from_raw(9);
        let mesh = compile(
            ChunkPos::default(),
            |position| {
                if position == BlockPos::new(0, 0, 0) {
                    decorative
                } else {
                    BlockId::AIR
                }
            },
            |_| false,
            |_| [1.0; 4],
        )
        .expect("bounded test chunk must compile");
        assert!(mesh.is_none());
    }
}
