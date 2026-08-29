//! Cached isometric voxel thumbnails for client HUD surfaces.
//!
//! Minecraft-style block items are rendered once into small transparent
//! images. HUD systems only swap typed image handles; they never rasterize a
//! cube or allocate GPU assets in a frame-critical system.

use std::collections::BTreeMap;

use bevy::{
    asset::{Assets, Handle, RenderAssetUsages},
    image::Image,
    prelude::{Commands, Res, ResMut, Resource},
    render::render_resource::{Extent3d, TextureDimension, TextureFormat},
};
use latticeaxiom_gameplay::{BlockId, ItemId};

use super::{
    ProductionSpine,
    chunk_mesh::{block_color, nearest_clamp_sampler},
};

const ICON_EDGE: u32 = 48;
const ICON_EDGE_USIZE: usize = ICON_EDGE as usize;
const TOP_FACE: [(i32, i32); 4] = [(24, 2), (44, 12), (24, 22), (4, 12)];
const LEFT_FACE: [(i32, i32); 4] = [(4, 12), (24, 22), (24, 44), (4, 34)];
const RIGHT_FACE: [(i32, i32); 4] = [(24, 22), (44, 12), (44, 34), (24, 44)];

/// Immutable typed lookup of pre-rasterized HUD voxel images.
#[derive(Clone, Debug, Resource)]
pub(super) struct ProductionVoxelIconCache {
    blocks: BTreeMap<BlockId, Handle<Image>>,
    items: BTreeMap<ItemId, Handle<Image>>,
    fallback: Handle<Image>,
}

impl ProductionVoxelIconCache {
    /// Returns the cached cube for a block, or the stable fallback cube.
    #[must_use]
    pub(super) fn block(&self, block: &BlockId) -> &Handle<Image> {
        self.blocks.get(block).unwrap_or(&self.fallback)
    }

    /// Returns the cached cube for an item, or the stable fallback cube.
    #[must_use]
    pub(super) fn item(&self, item: &ItemId) -> &Handle<Image> {
        self.items.get(item).unwrap_or(&self.fallback)
    }
}

/// Builds every block and item thumbnail once during interactive startup.
#[allow(clippy::needless_pass_by_value)] // Bevy systems receive SystemParams by value.
pub(super) fn build_production_voxel_icons(
    mut commands: Commands<'_, '_>,
    spine: Res<'_, ProductionSpine>,
    mut images: ResMut<'_, Assets<Image>>,
) {
    let fallback = images.add(voxel_icon_image("latticeaxiom:missing/voxel-icon"));
    let mut blocks = BTreeMap::new();
    let mut items = BTreeMap::new();

    for block in spine.palette_ids() {
        insert_block_icon(&mut blocks, &mut images, block);
    }
    if let Some(catalog) = spine.gameplay_catalog() {
        for block in catalog.blocks().keys().cloned() {
            insert_block_icon(&mut blocks, &mut images, block);
        }
        for (item, definition) in catalog.items() {
            let handle = definition
                .placement_block
                .as_ref()
                .and_then(|block| blocks.get(block))
                .cloned()
                .unwrap_or_else(|| images.add(voxel_icon_image(item.as_str())));
            items.insert(item.clone(), handle);
        }
    }

    commands.insert_resource(ProductionVoxelIconCache {
        blocks,
        items,
        fallback,
    });
}

fn insert_block_icon(
    blocks: &mut BTreeMap<BlockId, Handle<Image>>,
    images: &mut Assets<Image>,
    block: BlockId,
) {
    if blocks.contains_key(&block) {
        return;
    }
    let handle = images.add(voxel_icon_image(block.as_str()));
    blocks.insert(block, handle);
}

fn voxel_icon_image(identity: &str) -> Image {
    let mut rgba = vec![0_u8; ICON_EDGE_USIZE * ICON_EDGE_USIZE * 4];
    let base = block_color(identity);
    let seed = stable_hash(identity.as_bytes());
    paint_face(&mut rgba, TOP_FACE, base, 1.12, seed, 0);
    paint_face(&mut rgba, LEFT_FACE, base, 0.72, seed, 1);
    paint_face(&mut rgba, RIGHT_FACE, base, 0.90, seed, 2);

    let mut image = Image::new(
        Extent3d {
            width: ICON_EDGE,
            height: ICON_EDGE,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        rgba,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD,
    );
    image.sampler = nearest_clamp_sampler();
    image
}

fn paint_face(
    rgba: &mut [u8],
    polygon: [(i32, i32); 4],
    base: [f32; 4],
    shade: f32,
    seed: u32,
    face: u32,
) {
    for y in 0..ICON_EDGE {
        for x in 0..ICON_EDGE {
            if !inside_convex_polygon(x, y, polygon) {
                continue;
            }
            let variation = texture_variation(seed, x, y, face);
            let color = [
                base[0] * shade * variation,
                base[1] * shade * variation,
                base[2] * shade * variation,
                base[3],
            ];
            set_pixel(rgba, x, y, rgba8(color));
        }
    }
}

fn inside_convex_polygon(x: u32, y: u32, polygon: [(i32, i32); 4]) -> bool {
    let sample_x = i32::try_from(x).unwrap_or_default().saturating_mul(2) + 1;
    let sample_y = i32::try_from(y).unwrap_or_default().saturating_mul(2) + 1;
    let mut has_positive = false;
    let mut has_negative = false;
    for index in 0..polygon.len() {
        let current = polygon[index];
        let next = polygon[(index + 1) % polygon.len()];
        let cross = (next.0 - current.0).saturating_mul(sample_y - current.1 * 2)
            - (next.1 - current.1).saturating_mul(sample_x - current.0 * 2);
        has_positive |= cross > 0;
        has_negative |= cross < 0;
        if has_positive && has_negative {
            return false;
        }
    }
    true
}

fn texture_variation(seed: u32, x: u32, y: u32, face: u32) -> f32 {
    let mixed = seed
        ^ x.wrapping_mul(0x9e37_79b1)
        ^ y.wrapping_mul(0x85eb_ca77)
        ^ face.wrapping_mul(0xc2b2_ae3d);
    match mixed.rotate_left((x + y + face) & 31) & 0b11 {
        0 => 0.91,
        1 => 0.97,
        2 => 1.03,
        _ => 1.08,
    }
}

fn stable_hash(bytes: &[u8]) -> u32 {
    let mut hash = 2_166_136_261_u32;
    for byte in bytes {
        hash ^= u32::from(*byte);
        hash = hash.wrapping_mul(16_777_619);
    }
    hash
}

fn set_pixel(rgba: &mut [u8], x: u32, y: u32, pixel: [u8; 4]) {
    let offset = usize::try_from(y)
        .unwrap_or_default()
        .saturating_mul(ICON_EDGE_USIZE)
        .saturating_add(usize::try_from(x).unwrap_or_default())
        .saturating_mul(4);
    if let Some(destination) = rgba.get_mut(offset..offset.saturating_add(4)) {
        destination.copy_from_slice(&pixel);
    }
}

fn rgba8(color: [f32; 4]) -> [u8; 4] {
    color.map(channel8)
}

fn channel8(value: f32) -> u8 {
    let scaled = (value.clamp(0.0, 1.0) * 255.0).round();
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "the channel is clamped and rounded into the exact u8 domain"
    )]
    {
        scaled as u8
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pixels(image: &Image) -> &[u8] {
        image
            .data
            .as_deref()
            .expect("generated image retains pixels")
    }

    fn pixel(image: &Image, x: usize, y: usize) -> [u8; 4] {
        let offset = (y * ICON_EDGE_USIZE + x) * 4;
        pixels(image)[offset..offset + 4]
            .try_into()
            .expect("test coordinate names one complete pixel")
    }

    #[test]
    fn voxel_icon_is_deterministic_and_keeps_a_transparent_margin() {
        let first = voxel_icon_image("terrenia:block/grass");
        let second = voxel_icon_image("terrenia:block/grass");

        assert_eq!(pixels(&first), pixels(&second));
        assert_eq!(pixel(&first, 0, 0), [0, 0, 0, 0]);
        assert_eq!(pixel(&first, 47, 47), [0, 0, 0, 0]);
        assert!(pixels(&first).chunks_exact(4).any(|pixel| pixel[3] != 0));
    }

    #[test]
    fn voxel_icon_exposes_three_separately_lit_cube_faces() {
        let image = voxel_icon_image("terrenia:block/stone");
        let top = pixel(&image, 24, 10);
        let left = pixel(&image, 14, 28);
        let right = pixel(&image, 34, 28);
        let luminance =
            |pixel: [u8; 4]| u32::from(pixel[0]) + u32::from(pixel[1]) + u32::from(pixel[2]);

        assert_eq!(top[3], 255);
        assert_eq!(left[3], 255);
        assert_eq!(right[3], 255);
        assert!(luminance(top) > luminance(right));
        assert!(luminance(right) > luminance(left));
    }

    #[test]
    fn voxel_identity_changes_the_cached_pixels() {
        let stone = voxel_icon_image("terrenia:block/stone");
        let grass = voxel_icon_image("terrenia:block/grass");

        assert_ne!(pixels(&stone), pixels(&grass));
    }
}
