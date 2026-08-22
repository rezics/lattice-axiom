//! GPU chunk meshes built from halo-aware derived geometry.

use std::collections::BTreeMap;

use bevy::{
    asset::{Assets, Handle, RenderAssetUsages},
    camera::primitives::Aabb as GpuAabb,
    image::{Image, ImageAddressMode, ImageFilterMode, ImageSampler, ImageSamplerDescriptor},
    mesh::{Indices, Mesh, PrimitiveTopology},
    prelude::{
        AlphaMode, Commands, Component, Entity, Mesh3d, MeshMaterial3d, Resource, StandardMaterial,
        Transform, Vec3,
    },
    render::render_resource::{Extent3d, TextureDimension, TextureFormat},
};
use latticeaxiom_core::StableId;
use latticeaxiom_gameplay::BlockId;
use latticeaxiom_render_contracts::{CompiledTerrainLayerTableV1, TerrainFaceV1};
use latticeaxiom_voxel_mesh::{
    Aabb, Face, LayerMergeKey, MeshAlphaMode, MeshBuffer, MeshGroup, Quad,
};

/// Pixel edge length of one color-block atlas tile.
const TILE_PX: u32 = 16;

/// Linear fallback used for an empty palette or an out-of-range index.
const FALLBACK_COLOR: [f32; 4] = [0.38, 0.41, 0.43, 1.0];

/// One Bevy material per [`MeshGroup`], sharing the nearest color-block atlas.
#[derive(Clone, Debug, Resource)]
pub(super) struct ProductionTerrainMaterials {
    handles: [Handle<StandardMaterial>; 4],
}

impl ProductionTerrainMaterials {
    pub(super) fn from_atlas(
        materials: &mut Assets<StandardMaterial>,
        atlas: Handle<Image>,
    ) -> Self {
        let handles =
            MeshGroup::ALL.map(|group| materials.add(group_material(atlas.clone(), group)));
        Self { handles }
    }

    fn handle(&self, group: MeshGroup) -> Handle<StandardMaterial> {
        self.handles[group.index()].clone()
    }
}

fn group_material(atlas: Handle<Image>, group: MeshGroup) -> StandardMaterial {
    let mut material = StandardMaterial {
        base_color: bevy::prelude::Color::WHITE,
        base_color_texture: Some(atlas),
        perceptual_roughness: 0.95,
        reflectance: 0.06,
        alpha_mode: match group.alpha_mode() {
            MeshAlphaMode::Opaque => AlphaMode::Opaque,
            MeshAlphaMode::Mask => AlphaMode::Mask(0.5),
            MeshAlphaMode::Blend => AlphaMode::Blend,
        },
        ..StandardMaterial::default()
    };
    if group.is_emissive() {
        material.emissive = bevy::color::LinearRgba::rgb(2.0, 1.6, 0.8);
    }
    material
}

/// Palette-index to linear RGBA table and a deterministic color-block atlas.
#[derive(Clone, Debug, Resource)]
pub(super) struct ProductionTerrainPalette {
    colors: Vec<[f32; 4]>,
    #[cfg_attr(not(test), allow(dead_code))]
    tiles: Vec<AtlasTile>,
    fallback_tile: AtlasTile,
    atlas_rgba: Vec<u8>,
    atlas_width: u32,
    atlas_height: u32,
    layer_table: Option<CompiledTerrainLayerTableV1>,
    layer_tiles: BTreeMap<StableId, AtlasTile>,
}

/// One 16×16 atlas tile in UV space, inset by half a texel.
#[derive(Clone, Copy, Debug, PartialEq)]
struct AtlasTile {
    min: [f32; 2],
    size: [f32; 2],
}

impl ProductionTerrainPalette {
    pub(super) fn from_ids(ids: &[BlockId]) -> Self {
        let colors: Vec<[f32; 4]> = ids.iter().map(|id| block_color(id.as_str())).collect();
        let tile_count = colors.len().max(1);
        let columns = ceil_sqrt(tile_count);
        let rows = div_ceil_u32(tile_count, columns);
        let atlas_width = columns.saturating_mul(TILE_PX).max(TILE_PX);
        let atlas_height = rows.saturating_mul(TILE_PX).max(TILE_PX);
        let pixel_count =
            usize::try_from(atlas_width.saturating_mul(atlas_height).saturating_mul(4))
                .unwrap_or(0);
        let mut atlas_rgba = vec![0_u8; pixel_count];
        fill_solid(&mut atlas_rgba, FALLBACK_COLOR);

        let mut tiles = Vec::with_capacity(colors.len());
        if colors.is_empty() {
            blit_tile(
                &mut atlas_rgba,
                atlas_width,
                0,
                0,
                &solid_tile(FALLBACK_COLOR),
            );
        } else {
            for (tile_index, color) in colors.iter().copied().enumerate() {
                let index = u32::try_from(tile_index).unwrap_or(0);
                let col = index % columns.max(1);
                let row = index / columns.max(1);
                blit_tile(&mut atlas_rgba, atlas_width, col, row, &solid_tile(color));
                tiles.push(atlas_tile(tile_index, columns, atlas_width, atlas_height));
            }
        }

        let fallback_tile = atlas_tile(0, columns, atlas_width, atlas_height);
        Self {
            colors,
            tiles,
            fallback_tile,
            atlas_rgba,
            atlas_width,
            atlas_height,
            layer_table: None,
            layer_tiles: BTreeMap::new(),
        }
    }

    pub(super) fn from_layer_table(table: CompiledTerrainLayerTableV1) -> Self {
        let mut layer_ids = BTreeMap::new();
        for row in table.rows() {
            for face in TerrainFaceV1::ALL {
                layer_ids.insert(row.faces().layer(face).clone(), ());
            }
        }
        let unique_layers = layer_ids.into_keys().collect::<Vec<_>>();
        let colors: Vec<[f32; 4]> = table
            .rows()
            .iter()
            .map(|row| block_color(row.content().as_str()))
            .collect();
        let tile_count = unique_layers.len().max(1);
        let columns = ceil_sqrt(tile_count);
        let rows = div_ceil_u32(tile_count, columns);
        let atlas_width = columns.saturating_mul(TILE_PX).max(TILE_PX);
        let atlas_height = rows.saturating_mul(TILE_PX).max(TILE_PX);
        let pixel_count =
            usize::try_from(atlas_width.saturating_mul(atlas_height).saturating_mul(4))
                .unwrap_or(0);
        let mut atlas_rgba = vec![0_u8; pixel_count];
        fill_solid(&mut atlas_rgba, FALLBACK_COLOR);

        let mut layer_tiles = BTreeMap::new();
        let mut tiles = Vec::with_capacity(unique_layers.len());
        if unique_layers.is_empty() {
            blit_tile(
                &mut atlas_rgba,
                atlas_width,
                0,
                0,
                &solid_tile(FALLBACK_COLOR),
            );
        } else {
            for (tile_index, layer) in unique_layers.iter().enumerate() {
                let index = u32::try_from(tile_index).unwrap_or(0);
                let col = index % columns.max(1);
                let row = index / columns.max(1);
                let color = table
                    .rows()
                    .iter()
                    .find(|candidate| {
                        TerrainFaceV1::ALL
                            .iter()
                            .any(|face| candidate.faces().layer(*face) == layer)
                    })
                    .map(|row| block_color(row.content().as_str()))
                    .unwrap_or(FALLBACK_COLOR);
                blit_tile(&mut atlas_rgba, atlas_width, col, row, &solid_tile(color));
                let tile = atlas_tile(tile_index, columns, atlas_width, atlas_height);
                tiles.push(tile);
                layer_tiles.insert(layer.clone(), tile);
            }
        }

        let fallback_tile = atlas_tile(0, columns, atlas_width, atlas_height);
        Self {
            colors,
            tiles,
            fallback_tile,
            atlas_rgba,
            atlas_width,
            atlas_height,
            layer_table: Some(table),
            layer_tiles,
        }
    }

    #[cfg_attr(not(test), allow(dead_code))]
    fn color(&self, palette_index: u16) -> [f32; 4] {
        self.colors
            .get(usize::from(palette_index))
            .copied()
            .unwrap_or(FALLBACK_COLOR)
    }

    #[cfg_attr(not(test), allow(dead_code))]
    fn tile(&self, palette_index: u16) -> AtlasTile {
        self.tiles
            .get(usize::from(palette_index))
            .copied()
            .unwrap_or(self.fallback_tile)
    }

    /// Unit-quad atlas UVs for `palette_index`. Stable for a given ID table.
    #[cfg(test)]
    fn tile_uvs(&self, palette_index: u16) -> [[f32; 2]; 4] {
        let tile = self.tile(palette_index);
        [
            map_uv([0.0, 0.0], tile, 1.0, 1.0),
            map_uv([1.0, 0.0], tile, 1.0, 1.0),
            map_uv([1.0, 1.0], tile, 1.0, 1.0),
            map_uv([0.0, 1.0], tile, 1.0, 1.0),
        ]
    }

    #[cfg_attr(not(test), allow(dead_code))]
    fn vertex_uvs<K>(&self, palette_index: u16, face: Face, quad: &Quad<K>) -> [[f32; 2]; 4] {
        self.map_local_uvs(self.tile(palette_index), face, quad)
    }

    fn layer_uvs(
        &self,
        key: &LayerMergeKey,
        face: Face,
        quad: &Quad<LayerMergeKey>,
    ) -> [[f32; 2]; 4] {
        let tile = self
            .layer_table
            .as_ref()
            .and_then(|table| table.rows().get(usize::from(key.layer_index())))
            .and_then(|row| {
                TerrainFaceV1::ALL
                    .get(face.index())
                    .and_then(|terrain_face| self.layer_tiles.get(row.faces().layer(*terrain_face)))
            })
            .copied()
            .unwrap_or(self.fallback_tile);
        self.map_local_uvs(tile, face, quad)
    }

    fn layer_color(&self, key: &LayerMergeKey) -> [f32; 4] {
        self.layer_table
            .as_ref()
            .and_then(|table| table.rows().get(usize::from(key.layer_index())))
            .map(|row| block_color(row.content().as_str()))
            .or_else(|| self.colors.get(usize::from(key.layer_index())).copied())
            .unwrap_or(FALLBACK_COLOR)
    }

    fn map_local_uvs<K>(&self, tile: AtlasTile, face: Face, quad: &Quad<K>) -> [[f32; 2]; 4] {
        let local = quad.uvs(face);
        let width = local[1][0].max(1.0);
        let height = local[3][1].max(1.0);
        [
            map_uv(local[0], tile, width, height),
            map_uv(local[1], tile, width, height),
            map_uv(local[2], tile, width, height),
            map_uv(local[3], tile, width, height),
        ]
    }

    /// Deterministic solid-color atlas from palette IDs in index order.
    ///
    /// Missing PNG textures use this 16×16 color-block fallback. Tiles are
    /// packed left-to-right, top-to-bottom, with nearest sampling and clamp.
    pub(super) fn atlas_image(&self) -> Image {
        let mut image = Image::new_uninit(
            Extent3d {
                width: self.atlas_width,
                height: self.atlas_height,
                depth_or_array_layers: 1,
            },
            TextureDimension::D2,
            TextureFormat::Rgba8UnormSrgb,
            RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD,
        );
        image.data = Some(self.atlas_rgba.clone());
        image.sampler = nearest_clamp_sampler();
        image
    }
}

/// Nearest-filtered, clamp-to-edge sampler for voxel color-block textures.
pub(super) fn nearest_clamp_sampler() -> ImageSampler {
    ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::ClampToEdge,
        address_mode_v: ImageAddressMode::ClampToEdge,
        address_mode_w: ImageAddressMode::ClampToEdge,
        mag_filter: ImageFilterMode::Nearest,
        min_filter: ImageFilterMode::Nearest,
        mipmap_filter: ImageFilterMode::Nearest,
        ..ImageSamplerDescriptor::nearest()
    })
}

/// Marker that a chunk entity currently presents a complete GPU mesh.
#[derive(Clone, Component, Debug)]
pub(super) struct ChunkGpuMesh {
    groups: [Option<Entity>; 4],
}

/// Child entity presenting one [`MeshGroup`] of a chunk.
#[derive(Clone, Copy, Component, Debug, Eq, PartialEq)]
pub(super) struct ChunkGroupMesh(MeshGroup);

/// Builds or replaces the visible mesh on a presented chunk entity.
///
/// A complete Bevy mesh asset is inserted before the previous handle is
/// dropped. Empty accepted geometry removes the GPU mesh; missing geometry
/// leaves the previous presentation in place.
#[allow(clippy::too_many_arguments)] // Palette, bounds, and previous GPU mesh are presentation inputs.
pub(super) fn apply_chunk_mesh(
    commands: &mut Commands<'_, '_>,
    meshes: &mut Assets<Mesh>,
    materials: &ProductionTerrainMaterials,
    palette: &ProductionTerrainPalette,
    entity: Entity,
    geometry: &MeshBuffer<LayerMergeKey>,
    bounds: Option<Aabb>,
    existing: Option<&ChunkGpuMesh>,
) {
    if let Some(existing) = existing {
        for child in existing.groups.into_iter().flatten() {
            commands.entity(child).despawn();
        }
    }
    let mut groups = [None; 4];
    let mut spawned = 0_usize;
    for group in MeshGroup::ALL {
        let Some(mesh) = mesh_from_group(geometry, palette, group) else {
            continue;
        };
        let handle = meshes.add(mesh);
        let mut child = commands.spawn((
            Transform::IDENTITY,
            Mesh3d(handle),
            MeshMaterial3d(materials.handle(group)),
            ChunkGroupMesh(group),
        ));
        if let Some(bounds) = bounds.or_else(|| geometry.bounds()) {
            child.insert(gpu_aabb(bounds));
        }
        let child_entity = child.id();
        commands.entity(entity).add_child(child_entity);
        groups[group.index()] = Some(child_entity);
        spawned = spawned.saturating_add(1);
    }
    if spawned == 0 {
        commands.entity(entity).remove::<ChunkGpuMesh>();
        return;
    }
    commands.entity(entity).insert(ChunkGpuMesh { groups });
}

fn mesh_from_group(
    geometry: &MeshBuffer<LayerMergeKey>,
    palette: &ProductionTerrainPalette,
    group: MeshGroup,
) -> Option<Mesh> {
    let cpu = adapter_cpu_mesh_from_group(
        geometry,
        group,
        |key| palette.layer_color(key),
        |key, face, quad| palette.layer_uvs(key, face, quad),
    )?;
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
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, cpu.uvs);
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
    uvs: Vec<[f32; 2]>,
    indices: Vec<u32>,
    group_count: usize,
}

#[cfg_attr(not(test), allow(dead_code))]
fn adapter_cpu_mesh_from_buffer<K>(
    geometry: &MeshBuffer<K>,
    color_of: impl FnMut(&K) -> [f32; 4],
    uv_of: impl FnMut(&K, Face, &Quad<K>) -> [[f32; 2]; 4],
) -> Option<AdapterCpuMesh> {
    emit_adapter_mesh(geometry, MeshGroup::ALL, color_of, uv_of)
}

fn adapter_cpu_mesh_from_group(
    geometry: &MeshBuffer<LayerMergeKey>,
    group: MeshGroup,
    color_of: impl FnMut(&LayerMergeKey) -> [f32; 4],
    uv_of: impl FnMut(&LayerMergeKey, Face, &Quad<LayerMergeKey>) -> [[f32; 2]; 4],
) -> Option<AdapterCpuMesh> {
    emit_adapter_mesh(geometry, [group], color_of, uv_of)
}

fn emit_adapter_mesh<K, const N: usize>(
    geometry: &MeshBuffer<K>,
    groups: [MeshGroup; N],
    mut color_of: impl FnMut(&K) -> [f32; 4],
    mut uv_of: impl FnMut(&K, Face, &Quad<K>) -> [[f32; 2]; 4],
) -> Option<AdapterCpuMesh> {
    if geometry.is_empty() {
        return None;
    }
    let mut positions = Vec::with_capacity(geometry.vertex_count());
    let mut normals = Vec::with_capacity(geometry.vertex_count());
    let mut colors = Vec::with_capacity(geometry.vertex_count());
    let mut uvs = Vec::with_capacity(geometry.vertex_count());
    let mut indices = Vec::with_capacity(geometry.index_count());
    let mut group_count = 0_usize;

    for group in groups {
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
                let quad_uvs = uv_of(quad.merge_key(), face, quad);
                for ((position, normal), uv) in quad
                    .positions(face)
                    .into_iter()
                    .zip(face.quad_normals())
                    .zip(quad_uvs)
                {
                    positions.push(position);
                    normals.push(normal);
                    colors.push(color);
                    uvs.push(uv);
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
        uvs,
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

fn map_uv(local: [f32; 2], tile: AtlasTile, width: f32, height: f32) -> [f32; 2] {
    [
        tile.min[0] + local[0] / width * tile.size[0],
        tile.min[1] + local[1] / height * tile.size[1],
    ]
}

fn ceil_sqrt(count: usize) -> u32 {
    let count = u32::try_from(count.max(1)).unwrap_or(u32::MAX);
    let root = count.isqrt();
    if root.saturating_mul(root) < count {
        root.saturating_add(1).max(1)
    } else {
        root.max(1)
    }
}

fn div_ceil_u32(count: usize, divisor: u32) -> u32 {
    let count = u32::try_from(count.max(1)).unwrap_or(u32::MAX);
    count.div_ceil(divisor.max(1))
}

#[expect(
    clippy::cast_precision_loss,
    reason = "atlas pixel coordinates stay below f32's exact integer range"
)]
fn atlas_tile(index: usize, columns: u32, atlas_width: u32, atlas_height: u32) -> AtlasTile {
    let index = u32::try_from(index).unwrap_or(0);
    let col = index % columns.max(1);
    let row = index / columns.max(1);
    let atlas_w = atlas_width.max(1) as f32;
    let atlas_h = atlas_height.max(1) as f32;
    let tile = TILE_PX as f32;
    let inset = 0.5;
    AtlasTile {
        min: [
            (col as f32 * tile + inset) / atlas_w,
            (row as f32 * tile + inset) / atlas_h,
        ],
        size: [(tile - 1.0) / atlas_w, (tile - 1.0) / atlas_h],
    }
}

fn solid_tile(color: [f32; 4]) -> Vec<u8> {
    let pixel = rgba8_unorm(color);
    let len = usize::try_from(TILE_PX.saturating_mul(TILE_PX).saturating_mul(4)).unwrap_or(0);
    pixel.iter().copied().cycle().take(len).collect()
}

fn fill_solid(atlas: &mut [u8], color: [f32; 4]) {
    let pixel = rgba8_unorm(color);
    for chunk in atlas.chunks_exact_mut(4) {
        chunk.copy_from_slice(&pixel);
    }
}

fn rgba8_unorm(color: [f32; 4]) -> [u8; 4] {
    [
        channel_unorm(color[0]),
        channel_unorm(color[1]),
        channel_unorm(color[2]),
        channel_unorm(color[3]),
    ]
}

fn channel_unorm(value: f32) -> u8 {
    let scaled = (value.clamp(0.0, 1.0) * 255.0).round();
    if scaled >= 255.0 {
        255
    } else if scaled <= 0.0 {
        0
    } else {
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "scaled is in 0..255 after clamp and round"
        )]
        {
            scaled as u8
        }
    }
}

fn blit_tile(atlas: &mut [u8], atlas_width: u32, col: u32, row: u32, tile: &[u8]) {
    let tile_stride = usize::try_from(TILE_PX.saturating_mul(4)).unwrap_or(0);
    let atlas_stride = usize::try_from(atlas_width.saturating_mul(4)).unwrap_or(0);
    if tile_stride == 0 || atlas_stride == 0 {
        return;
    }
    for y in 0..TILE_PX {
        let src_start = usize::try_from(y).unwrap_or(0).saturating_mul(tile_stride);
        let src_end = src_start.saturating_add(tile_stride);
        let dst_y = usize::try_from(row.saturating_mul(TILE_PX).saturating_add(y)).unwrap_or(0);
        let dst_x = usize::try_from(col.saturating_mul(TILE_PX))
            .unwrap_or(0)
            .saturating_mul(4);
        let dst_start = dst_y.saturating_mul(atlas_stride).saturating_add(dst_x);
        let dst_end = dst_start.saturating_add(tile_stride);
        if let (Some(src), Some(dst)) = (
            tile.get(src_start..src_end),
            atlas.get_mut(dst_start..dst_end),
        ) {
            dst.copy_from_slice(src);
        }
    }
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use latticeaxiom_gameplay::BlockId;
    use latticeaxiom_voxel_mesh::{
        Aabb, ChunkCoordinate, Face, FaceDescriptor, FaceOcclusion, MeshBuffer, MeshGroup,
        MeshSource, PaddedChunk, SourceEpoch, SourceFingerprint, SourceRevision, Voxel,
        greedy_quads, visible_faces,
    };

    use super::{
        AdapterCpuMesh, ImageAddressMode, ImageFilterMode, ImageSampler, ProductionTerrainPalette,
        adapter_cpu_mesh_from_buffer, nearest_clamp_sampler, rgba8_unorm,
    };

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
        let palette = palette();
        adapter_cpu_mesh_from_buffer(
            buffer,
            |key| palette.color(u16::from(*key)),
            |key, face, quad| palette.vertex_uvs(u16::from(*key), face, quad),
        )
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
        assert_eq!(
            adapter.uvs.len(),
            adapter.vertex_count(),
            "adapter UVs {} != adapter vertices {}",
            adapter.uvs.len(),
            adapter.vertex_count()
        );
        assert_eq!(
            adapter.uvs.len(),
            buffer.vertex_count(),
            "adapter UVs {} != MeshBuffer vertices {}",
            adapter.uvs.len(),
            buffer.vertex_count()
        );
    }

    fn block_id(id: &str) -> BlockId {
        BlockId::parse(id).expect("fixture block ID is canonical")
    }

    fn sample_atlas(image: &bevy::image::Image, uv: [f32; 2]) -> [u8; 4] {
        let data = image
            .data
            .as_ref()
            .expect("color-block atlas keeps CPU data");
        let size = image.texture_descriptor.size;
        let x = uv_to_texel(uv[0], size.width);
        let y = uv_to_texel(uv[1], size.height);
        let offset = usize::try_from(
            y.saturating_mul(size.width)
                .saturating_add(x)
                .saturating_mul(4),
        )
        .expect("atlas pixel offset fits usize");
        data.get(offset..offset + 4)
            .expect("atlas UV lands inside the image")
            .try_into()
            .expect("RGBA pixel is 4 bytes")
    }

    fn uv_to_texel(uv: f32, extent: u32) -> u32 {
        if extent == 0 {
            return 0;
        }
        let max = extent - 1;
        let scaled = (uv.clamp(0.0, 0.999_999) * atlas_extent_f32(extent)).floor();
        if scaled <= 0.0 {
            0
        } else {
            #[expect(
                clippy::cast_possible_truncation,
                reason = "test atlas UVs stay within the tile pixel range"
            )]
            {
                u32::try_from(scaled as i64).unwrap_or(max).min(max)
            }
        }
    }

    #[expect(
        clippy::cast_precision_loss,
        reason = "test atlas extents stay below f32 exact integer range"
    )]
    fn atlas_extent_f32(value: u32) -> f32 {
        value as f32
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
    fn adapter_uv_count_equals_vertex_count() {
        let (voxels, dimensions) = volume([2, 1, 1], |position| {
            if position[0] == 0 {
                TestVoxel::opaque(1)
            } else {
                TestVoxel::grouped(2, MeshGroup::Cutout)
            }
        });
        let mesh = adapter_mesh(&voxels, dimensions);
        assert_eq!(mesh.uvs.len(), mesh.positions.len());
        assert_eq!(mesh.uvs.len(), mesh.vertex_count());
        assert_eq!(mesh.colors.len(), mesh.vertex_count());
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

    #[test]
    fn atlas_uv_for_palette_index_is_stable() {
        let ids = [
            block_id("terrenia:block/stone"),
            block_id("terrenia:block/dirt"),
            block_id("terrenia:block/grass"),
        ];
        let first = ProductionTerrainPalette::from_ids(&ids);
        let rebuilt = ProductionTerrainPalette::from_ids(&ids);
        for index in 0..ids.len() {
            let index = u16::try_from(index).expect("palette index fits u16");
            assert_eq!(first.tile_uvs(index), rebuilt.tile_uvs(index));
        }
        assert_ne!(first.tile_uvs(0), first.tile_uvs(1));
        assert_ne!(first.tile_uvs(1), first.tile_uvs(2));

        let (voxels, dimensions) = volume([2, 1, 1], |_| TestVoxel::opaque(1));
        let greedy = greedy_quads(&voxels, dimensions, source()).expect("valid samples");
        for (_group, face, quad) in greedy.geometry().iter() {
            let index = u16::from(*quad.merge_key());
            assert_eq!(first.vertex_uvs(index, face, quad), first.tile_uvs(index));
        }

        let atlas = first.atlas_image();
        assert_eq!(atlas.sampler, nearest_clamp_sampler());
        match &atlas.sampler {
            ImageSampler::Descriptor(descriptor) => {
                assert_eq!(descriptor.mag_filter, ImageFilterMode::Nearest);
                assert_eq!(descriptor.min_filter, ImageFilterMode::Nearest);
                assert_eq!(descriptor.address_mode_u, ImageAddressMode::ClampToEdge);
                assert_eq!(descriptor.address_mode_v, ImageAddressMode::ClampToEdge);
            }
            ImageSampler::Default => {
                panic!("expected nearest clamp sampler, got ImageSampler::Default")
            }
        }

        let pixel = sample_atlas(&atlas, first.tile_uvs(1)[0]);
        assert_eq!(pixel, rgba8_unorm(first.color(1)));
    }
}
