//! GPU chunk meshes built from halo-aware derived geometry.

use bevy::{
    asset::{Assets, Handle, RenderAssetUsages},
    camera::primitives::Aabb as GpuAabb,
    mesh::{Indices, Mesh, PrimitiveTopology},
    prelude::{
        Commands, Component, Entity, Mesh3d, MeshMaterial3d, Resource, StandardMaterial, Vec3,
    },
};
use latticeaxiom_gameplay::BlockId;
use latticeaxiom_voxel_mesh::{Aabb, Face, MeshBuffer, MeshGroup};

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

/// Marker that a chunk entity currently presents a complete GPU mesh.
#[derive(Clone, Component, Debug)]
pub(super) struct ChunkGpuMesh;

/// Builds or replaces the visible mesh on a presented chunk entity.
///
/// A complete Bevy mesh asset is inserted before the previous handle is
/// dropped. Empty accepted geometry removes the GPU mesh; missing geometry
/// leaves the previous presentation in place.
#[allow(clippy::too_many_arguments)] // Palette, bounds, and previous GPU mesh are presentation inputs.
pub(super) fn apply_chunk_mesh(
    commands: &mut Commands<'_, '_>,
    meshes: &mut Assets<Mesh>,
    material: &ProductionTerrainMaterial,
    palette: &ProductionTerrainPalette,
    entity: Entity,
    geometry: &MeshBuffer<u16>,
    bounds: Option<Aabb>,
    existing: Option<&ChunkGpuMesh>,
) {
    let Some(mesh) = mesh_from_buffer(geometry, palette) else {
        if existing.is_some() {
            commands.entity(entity).remove::<(
                Mesh3d,
                MeshMaterial3d<StandardMaterial>,
                ChunkGpuMesh,
                GpuAabb,
            )>();
        }
        return;
    };
    let handle = meshes.add(mesh);
    let mut entity_commands = commands.entity(entity);
    entity_commands.insert((
        Mesh3d(handle),
        MeshMaterial3d(material.0.clone()),
        ChunkGpuMesh,
    ));
    if let Some(bounds) = bounds.or_else(|| geometry.bounds()) {
        entity_commands.insert(gpu_aabb(bounds));
    }
}

fn mesh_from_buffer(
    geometry: &MeshBuffer<u16>,
    palette: &ProductionTerrainPalette,
) -> Option<Mesh> {
    let cpu = adapter_cpu_mesh_from_buffer(geometry, |key| palette.color(*key))?;
    if cpu.group_count == 0 {
        return None;
    }
    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD,
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, cpu.positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, cpu.normals);
    mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, cpu.colors);
    mesh.insert_indices(Indices::U32(cpu.indices));
    Some(mesh)
}

fn gpu_aabb(bounds: Aabb) -> GpuAabb {
    GpuAabb::from_min_max(Vec3::from(bounds.minimum), Vec3::from(bounds.maximum))
}

/// CPU triangle list emitted by the production renderer adapter.
#[derive(Clone, Debug)]
struct AdapterCpuMesh {
    positions: Vec<[f32; 3]>,
    normals: Vec<[f32; 3]>,
    colors: Vec<[f32; 4]>,
    indices: Vec<u32>,
    group_count: usize,
}

fn adapter_cpu_mesh_from_buffer<K>(
    geometry: &MeshBuffer<K>,
    mut color_of: impl FnMut(&K) -> [f32; 4],
) -> Option<AdapterCpuMesh> {
    if geometry.is_empty() {
        return None;
    }
    let mut positions = Vec::with_capacity(geometry.vertex_count());
    let mut normals = Vec::with_capacity(geometry.vertex_count());
    let mut colors = Vec::with_capacity(geometry.vertex_count());
    let mut indices = Vec::with_capacity(geometry.index_count());
    let mut group_count = 0_usize;

    for group in MeshGroup::ALL {
        let mut group_has_quads = false;
        for face in Face::ALL {
            for quad in geometry.group(group, face) {
                let Ok(base) = u32::try_from(positions.len()) else {
                    break;
                };
                let Some(quad_indices) = Face::quad_indices(base) else {
                    break;
                };
                let color = color_of(quad.merge_key());
                for (position, normal) in quad.positions(face).into_iter().zip(face.quad_normals())
                {
                    positions.push(position);
                    normals.push(normal);
                    colors.push(color);
                }
                indices.extend(quad_indices);
                group_has_quads = true;
            }
        }
        if group_has_quads {
            group_count = group_count.saturating_add(1);
        }
    }

    if indices.is_empty() {
        return None;
    }
    Some(AdapterCpuMesh {
        positions,
        normals,
        colors,
        indices,
        group_count,
    })
}

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

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use latticeaxiom_voxel_mesh::{
        Aabb, ChunkCoordinate, Face, FaceDescriptor, FaceOcclusion, MeshBuffer, MeshGroup,
        MeshSource, PaddedChunk, SourceEpoch, SourceFingerprint, SourceRevision, Voxel,
        visible_faces,
    };

    use super::{AdapterCpuMesh, ProductionTerrainPalette, adapter_cpu_mesh_from_buffer};

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    struct TestVoxel {
        material: Option<u8>,
        group: MeshGroup,
        occlusion: FaceOcclusion,
    }

    impl TestVoxel {
        const AIR: Self = Self {
            material: None,
            group: MeshGroup::Opaque,
            occlusion: FaceOcclusion::None,
        };

        const fn opaque(material: u8) -> Self {
            Self {
                material: Some(material),
                group: MeshGroup::Opaque,
                occlusion: FaceOcclusion::Full,
            }
        }

        const fn grouped(material: u8, group: MeshGroup) -> Self {
            Self {
                material: Some(material),
                group,
                occlusion: FaceOcclusion::Matching,
            }
        }
    }

    impl Voxel for TestVoxel {
        type MergeKey = u8;

        fn face(&self, _face: Face) -> Option<FaceDescriptor<Self::MergeKey>> {
            self.material
                .map(|material| FaceDescriptor::new(self.group, material, self.occlusion))
        }
    }

    fn source() -> MeshSource {
        MeshSource::new(
            ChunkCoordinate::new(0, 0, 0),
            SourceEpoch::new(1),
            SourceRevision::new(1),
            SourceFingerprint::new([0; 32]),
        )
    }

    fn volume(
        interior: [usize; 3],
        voxel_at: impl Fn([usize; 3]) -> TestVoxel,
    ) -> (Vec<TestVoxel>, PaddedChunk) {
        let dimensions = PaddedChunk::new(interior).expect("valid test dimensions");
        let mut voxels = vec![TestVoxel::AIR; dimensions.volume_len()];
        for z in 0..interior[2] {
            for y in 0..interior[1] {
                for x in 0..interior[0] {
                    let padded = dimensions
                        .pad_interior([x, y, z])
                        .expect("loop coordinate is in the interior");
                    let index = dimensions
                        .linearize(padded)
                        .expect("padded interior coordinate is in bounds");
                    voxels[index] = voxel_at([x, y, z]);
                }
            }
        }
        (voxels, dimensions)
    }

    fn palette() -> ProductionTerrainPalette {
        ProductionTerrainPalette::from_ids(&[])
    }

    fn adapter_from_buffer(buffer: &MeshBuffer<u8>) -> AdapterCpuMesh {
        adapter_cpu_mesh_from_buffer(buffer, |key| palette().color(u16::from(*key)))
            .expect("MeshBuffer emits adapter geometry")
    }

    fn adapter_mesh(voxels: &[TestVoxel], dimensions: PaddedChunk) -> AdapterCpuMesh {
        adapter_from_buffer(
            visible_faces(voxels, dimensions, source())
                .expect("valid samples")
                .geometry(),
        )
    }

    fn nonempty_group_count<K>(buffer: &MeshBuffer<K>) -> usize {
        MeshGroup::ALL
            .into_iter()
            .filter(|&group| {
                Face::ALL
                    .into_iter()
                    .any(|face| !buffer.group(group, face).is_empty())
            })
            .count()
    }

    impl AdapterCpuMesh {
        fn quad_count(&self) -> usize {
            self.indices.len() / 6
        }

        fn vertex_count(&self) -> usize {
            self.positions.len()
        }

        fn index_count(&self) -> usize {
            self.indices.len()
        }

        fn group_count(&self) -> usize {
            self.group_count
        }
    }

    fn adapter_bounds(mesh: &AdapterCpuMesh) -> Option<Aabb> {
        let mut positions = mesh.positions.iter().copied();
        let first = positions.next()?;
        let mut bounds = Aabb {
            minimum: first,
            maximum: first,
        };
        for position in positions {
            for (axis, coordinate) in position.into_iter().enumerate() {
                bounds.minimum[axis] = bounds.minimum[axis].min(coordinate);
                bounds.maximum[axis] = bounds.maximum[axis].max(coordinate);
            }
        }
        Some(bounds)
    }

    fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
        [
            a[1] * b[2] - a[2] * b[1],
            a[2] * b[0] - a[0] * b[2],
            a[0] * b[1] - a[1] * b[0],
        ]
    }

    fn subtract(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
        [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
    }

    fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
        a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
    }

    fn assert_adapter_counts_match_buffer(adapter: &AdapterCpuMesh, buffer: &MeshBuffer<u8>) {
        assert_eq!(
            adapter.quad_count(),
            buffer.quad_count(),
            "adapter quads {} != MeshBuffer quads {}",
            adapter.quad_count(),
            buffer.quad_count()
        );
        assert_eq!(
            adapter.vertex_count(),
            buffer.vertex_count(),
            "adapter vertices {} != MeshBuffer vertices {}",
            adapter.vertex_count(),
            buffer.vertex_count()
        );
        assert_eq!(
            adapter.index_count(),
            buffer.index_count(),
            "adapter indices {} != MeshBuffer indices {}",
            adapter.index_count(),
            buffer.index_count()
        );
        assert_eq!(
            adapter.group_count(),
            nonempty_group_count(buffer),
            "adapter groups {} != MeshBuffer groups {}",
            adapter.group_count(),
            nonempty_group_count(buffer)
        );
        assert_eq!(adapter_bounds(adapter), buffer.bounds());
    }

    #[test]
    fn adapter_triangles_have_outward_winding() {
        let (voxels, dimensions) = volume([1, 1, 1], |_| TestVoxel::opaque(1));
        let mesh = adapter_mesh(&voxels, dimensions);
        assert!(!mesh.indices.is_empty());
        for triangle in mesh.indices.chunks_exact(3) {
            let i0 = usize::try_from(triangle[0]).expect("index 0 fits usize");
            let i1 = usize::try_from(triangle[1]).expect("index 1 fits usize");
            let i2 = usize::try_from(triangle[2]).expect("index 2 fits usize");
            let p0 = mesh.positions[i0];
            let p1 = mesh.positions[i1];
            let p2 = mesh.positions[i2];
            let outward = mesh.normals[i0];
            let winding = cross(subtract(p1, p0), subtract(p2, p0));
            assert!(
                dot(winding, outward) > 0.0,
                "adapter triangle {triangle:?} opposes outward {outward:?}"
            );
        }
    }

    #[test]
    fn adapter_counts_equal_source_mesh_buffer() {
        for face in Face::ALL {
            let (mut voxels, dimensions) = volume([1, 1, 1], |_| TestVoxel::opaque(1));
            let padded = [1_usize, 1, 1];
            let neighbor = {
                let [dx, dy, dz] = face.normal_offset();
                [
                    padded[0].wrapping_add_signed(dx),
                    padded[1].wrapping_add_signed(dy),
                    padded[2].wrapping_add_signed(dz),
                ]
            };
            let index = dimensions
                .linearize(neighbor)
                .expect("halo coordinate is in bounds");
            voxels[index] = TestVoxel::opaque(2);
            let buffer = visible_faces(&voxels, dimensions, source())
                .expect("valid samples")
                .geometry()
                .clone();
            let adapter = adapter_from_buffer(&buffer);
            assert_adapter_counts_match_buffer(&adapter, &buffer);
        }
    }

    #[test]
    fn adapter_counts_equal_multi_group_source_mesh_buffer() {
        let (voxels, dimensions) = volume([2, 1, 1], |position| {
            if position[0] == 0 {
                TestVoxel::opaque(1)
            } else {
                TestVoxel::grouped(2, MeshGroup::Cutout)
            }
        });
        let buffer = visible_faces(&voxels, dimensions, source())
            .expect("valid samples")
            .geometry()
            .clone();
        let adapter = adapter_from_buffer(&buffer);
        assert_adapter_counts_match_buffer(&adapter, &buffer);
    }
}
