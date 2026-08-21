//! GPU chunk meshes built from occupied production cells.

use std::collections::BTreeSet;

use bevy::{
    asset::{Assets, Handle, RenderAssetUsages},
    mesh::{Indices, Mesh, PrimitiveTopology},
    prelude::{Commands, Component, Entity, Mesh3d, MeshMaterial3d, Resource, StandardMaterial},
};
use latticeaxiom_gameplay::BlockId;

use super::spine::OccupiedCell;

/// Shared PBR material multiplied by per-vertex block colors.
#[derive(Clone, Debug, Resource)]
pub(super) struct ProductionTerrainMaterial(pub(super) Handle<StandardMaterial>);

/// Palette-index to linear RGBA table for one production session.
#[derive(Clone, Debug, Resource)]
pub(super) struct ProductionTerrainPalette {
    colors: Vec<[f32; 4]>,
}

impl ProductionTerrainPalette {
    pub(super) fn from_ids(ids: &[BlockId]) -> Self {
        Self {
            colors: ids.iter().map(|id| block_color(id.as_str())).collect(),
        }
    }

    fn color(&self, palette_index: u16) -> [f32; 4] {
        self.colors
            .get(usize::from(palette_index))
            .copied()
            .unwrap_or([0.38, 0.41, 0.43, 1.0])
    }
}

/// Handle to the GPU mesh currently attached to a chunk entity.
#[derive(Clone, Component, Debug)]
pub(super) struct ChunkGpuMesh(Handle<Mesh>);

/// Builds or replaces the visible mesh on a presented chunk entity.
pub(super) fn apply_chunk_mesh(
    commands: &mut Commands<'_, '_>,
    meshes: &mut Assets<Mesh>,
    material: &ProductionTerrainMaterial,
    palette: &ProductionTerrainPalette,
    entity: Entity,
    occupied: &[OccupiedCell],
    existing: Option<&ChunkGpuMesh>,
) {
    let Some(mesh) = mesh_from_occupied(occupied, palette) else {
        if existing.is_some() {
            commands
                .entity(entity)
                .remove::<(Mesh3d, MeshMaterial3d<StandardMaterial>, ChunkGpuMesh)>();
        }
        return;
    };
    if let Some(existing) = existing
        && let Some(mut stored) = meshes.get_mut(&existing.0)
    {
        *stored = mesh;
        return;
    }
    let handle = meshes.add(mesh);
    commands.entity(entity).insert((
        Mesh3d(handle.clone()),
        MeshMaterial3d(material.0.clone()),
        ChunkGpuMesh(handle),
    ));
}

#[allow(clippy::cast_precision_loss)] // Interior cell coordinates stay inside f32's exact integer range.
fn mesh_from_occupied(
    occupied: &[OccupiedCell],
    palette: &ProductionTerrainPalette,
) -> Option<Mesh> {
    if occupied.is_empty() {
        return None;
    }
    let occupancy: BTreeSet<[u16; 3]> = occupied.iter().map(|cell| cell.local).collect();
    let mut positions = Vec::new();
    let mut normals = Vec::new();
    let mut colors = Vec::new();
    let mut indices = Vec::new();

    for cell in occupied {
        let color = palette.color(cell.palette_index);
        let [x, y, z] = cell.local;
        for face in FACES {
            if neighbor_occupied(&occupancy, [x, y, z], face.neighbor) {
                continue;
            }
            let Ok(base) = u32::try_from(positions.len()) else {
                break;
            };
            for corner in face.corners {
                positions.push([
                    f32::from(x) + corner[0],
                    f32::from(y) + corner[1],
                    f32::from(z) + corner[2],
                ]);
                normals.push(face.normal);
                colors.push(color);
            }
            indices.extend([base, base + 1, base + 2, base, base + 2, base + 3]);
        }
    }

    if indices.is_empty() {
        return None;
    }

    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD,
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
    mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, colors);
    mesh.insert_indices(Indices::U32(indices));
    Some(mesh)
}

fn neighbor_occupied(occupancy: &BTreeSet<[u16; 3]>, local: [u16; 3], offset: [i32; 3]) -> bool {
    let Some(x) = i32::from(local[0]).checked_add(offset[0]) else {
        return false;
    };
    let Some(y) = i32::from(local[1]).checked_add(offset[1]) else {
        return false;
    };
    let Some(z) = i32::from(local[2]).checked_add(offset[2]) else {
        return false;
    };
    let Ok(x) = u16::try_from(x) else {
        return false;
    };
    let Ok(y) = u16::try_from(y) else {
        return false;
    };
    let Ok(z) = u16::try_from(z) else {
        return false;
    };
    occupancy.contains(&[x, y, z])
}

struct FaceSpec {
    neighbor: [i32; 3],
    normal: [f32; 3],
    corners: [[f32; 3]; 4],
}

const FACES: [FaceSpec; 6] = [
    FaceSpec {
        neighbor: [1, 0, 0],
        normal: [1.0, 0.0, 0.0],
        corners: [
            [1.0, 0.0, 0.0],
            [1.0, 0.0, 1.0],
            [1.0, 1.0, 1.0],
            [1.0, 1.0, 0.0],
        ],
    },
    FaceSpec {
        neighbor: [-1, 0, 0],
        normal: [-1.0, 0.0, 0.0],
        corners: [
            [0.0, 0.0, 1.0],
            [0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            [0.0, 1.0, 1.0],
        ],
    },
    FaceSpec {
        neighbor: [0, 1, 0],
        normal: [0.0, 1.0, 0.0],
        corners: [
            [0.0, 1.0, 0.0],
            [1.0, 1.0, 0.0],
            [1.0, 1.0, 1.0],
            [0.0, 1.0, 1.0],
        ],
    },
    FaceSpec {
        neighbor: [0, -1, 0],
        normal: [0.0, -1.0, 0.0],
        corners: [
            [0.0, 0.0, 1.0],
            [1.0, 0.0, 1.0],
            [1.0, 0.0, 0.0],
            [0.0, 0.0, 0.0],
        ],
    },
    FaceSpec {
        neighbor: [0, 0, 1],
        normal: [0.0, 0.0, 1.0],
        corners: [
            [1.0, 0.0, 1.0],
            [0.0, 0.0, 1.0],
            [0.0, 1.0, 1.0],
            [1.0, 1.0, 1.0],
        ],
    },
    FaceSpec {
        neighbor: [0, 0, -1],
        normal: [0.0, 0.0, -1.0],
        corners: [
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [1.0, 1.0, 0.0],
            [0.0, 1.0, 0.0],
        ],
    },
];

fn block_color(block_id: &str) -> [f32; 4] {
    match block_id {
        "terrenia:block/grass" | "terrenia:block/tall-grass" | "terrenia:block/moss" => {
            [0.24, 0.56, 0.18, 1.0]
        }
        "terrenia:block/dirt"
        | "terrenia:block/coarse-dirt"
        | "terrenia:block/rooted-dirt"
        | "terrenia:block/peat"
        | "terrenia:block/mud" => [0.39, 0.24, 0.12, 1.0],
        "terrenia:block/sand" | "terrenia:block/sandstone" | "terrenia:block/silt" => {
            [0.76, 0.67, 0.42, 1.0]
        }
        "terrenia:block/red-sand" | "terrenia:block/red-sandstone" => [0.72, 0.38, 0.22, 1.0],
        "terrenia:block/stone"
        | "terrenia:block/cobblestone"
        | "terrenia:block/stone-bricks"
        | "terrenia:block/polished-stone" => [0.38, 0.41, 0.43, 1.0],
        "terrenia:block/water" => [0.18, 0.42, 0.72, 1.0],
        "terrenia:block/lava" => [0.86, 0.28, 0.08, 1.0],
        "terrenia:block/oak-log" | "terrenia:block/pine-log" => [0.42, 0.28, 0.14, 1.0],
        "terrenia:block/oak-leaves" | "terrenia:block/pine-leaves" => [0.18, 0.42, 0.16, 1.0],
        "terrenia:block/snow" | "terrenia:block/ice" => [0.86, 0.91, 0.95, 1.0],
        "terrenia:block/coal-ore" | "terrenia:block/coal-block" => [0.16, 0.16, 0.18, 1.0],
        _ => hashed_color(block_id),
    }
}

fn hashed_color(block_id: &str) -> [f32; 4] {
    let mut hash = 2_166_136_261_u32;
    for byte in block_id.as_bytes() {
        hash ^= u32::from(*byte);
        hash = hash.wrapping_mul(16_777_619);
    }
    [
        0.22 + f32::from(channel(hash >> 16)) / 255.0 * 0.55,
        0.22 + f32::from(channel(hash >> 8)) / 255.0 * 0.55,
        0.22 + f32::from(channel(hash)) / 255.0 * 0.55,
        1.0,
    ]
}

fn channel(value: u32) -> u8 {
    u8::try_from(value & 0xff).unwrap_or(0)
}
