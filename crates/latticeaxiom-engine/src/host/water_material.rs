//! Dedicated native-static water presentation material.

use std::f32::consts::TAU;

use bevy::{
    app::App,
    asset::{Asset, AssetPath, Handle, RenderAssetUsages, embedded_asset, embedded_path},
    image::{Image, ImageAddressMode, ImageFilterMode, ImageSampler, ImageSamplerDescriptor},
    pbr::{ExtendedMaterial, MaterialExtension, MaterialPlugin},
    prelude::{AlphaMode, Color, Reflect, StandardMaterial, Vec3, Vec4},
    render::render_resource::{AsBindGroup, Extent3d, ShaderType, TextureDimension, TextureFormat},
    shader::ShaderRef,
};

/// Edge length of the deterministic, tileable tangent-space normal map.
const WATER_NORMAL_MAP_EDGE: u32 = 32;

/// Complete mip count for the power-of-two production normal map.
const WATER_NORMAL_MIP_LEVELS: u32 = 6;

/// Maximum sampler anisotropy for the grazing-angle water plane.
const WATER_NORMAL_ANISOTROPY: u16 = 16;

/// Tightly packed RGBA8 bytes across the `32, 16, 8, 4, 2, 1` mip chain.
const WATER_NORMAL_MAP_BYTE_LEN: usize = 5_460;

/// Air-to-water reflectance at normal incidence for an IOR of approximately 1.333.
const WATER_NORMAL_REFLECTANCE: f32 = 0.020_37;

/// Bevy material used exclusively by the water mesh group.
pub(super) type WaterMaterial = ExtendedMaterial<StandardMaterial, WaterMaterialExtension>;

/// Uniform payload shared by the CPU optical oracle and the water fragment shader.
#[derive(Clone, Copy, Debug, Reflect, ShaderType)]
struct WaterMaterialUniform {
    /// RGB Beer-Lambert coefficients per meter and shallow alpha above water.
    absorption_above_alpha: Vec4,
    /// RGB Beer-Lambert coefficients per meter and shallow alpha below water.
    absorption_below_alpha: Vec4,
    /// Linear RGB deep-water tint and deep alpha above water.
    tint_above_deep_alpha: Vec4,
    /// Linear RGB deep-water tint and deep alpha below water.
    tint_below_deep_alpha: Vec4,
    /// Fresnel F0, Fresnel exponent, normal strength, and maximum optical depth.
    surface: Vec4,
    /// Still-water flow X/Z followed by the two octave speeds in meters per second.
    motion: Vec4,
    /// Two normal-map scales, fallback depth, and underwater distance multiplier.
    normal_depth: Vec4,
    /// Fresnel RGB tint and the final Fresnel blend strength.
    fresnel_tint_strength: Vec4,
    /// `x` is one when the authoritative camera medium is water, zero otherwise.
    view: Vec4,
}

impl Default for WaterMaterialUniform {
    fn default() -> Self {
        Self {
            absorption_above_alpha: Vec4::new(0.42, 0.105, 0.032, 0.34),
            absorption_below_alpha: Vec4::new(0.58, 0.16, 0.055, 0.52),
            tint_above_deep_alpha: Vec4::new(0.018, 0.16, 0.29, 0.84),
            tint_below_deep_alpha: Vec4::new(0.012, 0.105, 0.18, 0.92),
            surface: Vec4::new(WATER_NORMAL_REFLECTANCE, 5.0, 0.24, 24.0),
            motion: Vec4::new(0.8, -0.6, 0.055, -0.031),
            normal_depth: Vec4::new(0.42, 1.37, 1.5, 1.0),
            fresnel_tint_strength: Vec4::new(0.58, 0.78, 0.96, 0.42),
            view: Vec4::ZERO,
        }
    }
}

/// Extra bindings and shader behavior layered over Bevy's `StandardMaterial`.
#[derive(Asset, AsBindGroup, Clone, Debug, Reflect)]
pub(super) struct WaterMaterialExtension {
    #[texture(100)]
    #[sampler(101)]
    normal_map: Handle<Image>,
    #[uniform(102)]
    settings: WaterMaterialUniform,
}

impl WaterMaterialExtension {
    fn new(normal_map: Handle<Image>) -> Self {
        Self {
            normal_map,
            settings: WaterMaterialUniform::default(),
        }
    }

    /// Selects the shader's above-water or below-water optical parameters.
    pub(super) fn set_camera_underwater(&mut self, underwater: bool) {
        self.settings.view.x = f32::from(u8::from(underwater));
    }

    #[cfg(test)]
    fn camera_is_underwater(&self) -> bool {
        self.settings.view.x > 0.5
    }
}

impl MaterialExtension for WaterMaterialExtension {
    fn fragment_shader() -> ShaderRef {
        let path =
            AssetPath::from_path_buf(embedded_path!("water_material.wgsl")).with_source("embedded");
        path.into()
    }
}

/// Registers the embedded shader and its typed Bevy material pipeline.
pub(super) fn install_water_material(app: &mut App) {
    embedded_asset!(app, "water_material.wgsl");
    app.add_plugins(MaterialPlugin::<WaterMaterial>::default());
}

/// Constructs the single shared production water material.
pub(super) fn production_water_material(
    atlas: Handle<Image>,
    normal_map: Handle<Image>,
) -> WaterMaterial {
    WaterMaterial {
        base: StandardMaterial {
            base_color: Color::WHITE,
            base_color_texture: Some(atlas),
            perceptual_roughness: 0.18,
            reflectance: (WATER_NORMAL_REFLECTANCE / 0.16).sqrt(),
            ior: 1.333,
            alpha_mode: AlphaMode::Premultiplied,
            cull_mode: None,
            double_sided: true,
            ..StandardMaterial::default()
        },
        extension: WaterMaterialExtension::new(normal_map),
    }
}

/// Builds a deterministic, tileable linear-RGBA normal map for flow animation.
///
/// Coarse levels retain the unnormalized mean normal in RGB and its coherence
/// in alpha. The shader uses that coherence to suppress detail whose child
/// normals cancel within a distant pixel footprint.
pub(super) fn water_normal_image() -> Image {
    let edge = WATER_NORMAL_MAP_EDGE;
    let data = water_normal_mip_chain();

    let mut image = Image::new_uninit(
        Extent3d {
            width: edge,
            height: edge,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        TextureFormat::Rgba8Unorm,
        RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD,
    );
    image.texture_descriptor.mip_level_count = WATER_NORMAL_MIP_LEVELS;
    image.data = Some(data);
    image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::Repeat,
        address_mode_v: ImageAddressMode::Repeat,
        address_mode_w: ImageAddressMode::Repeat,
        mag_filter: ImageFilterMode::Linear,
        min_filter: ImageFilterMode::Linear,
        mipmap_filter: ImageFilterMode::Linear,
        anisotropy_clamp: WATER_NORMAL_ANISOTROPY,
        ..ImageSamplerDescriptor::linear()
    });
    image
}

#[expect(
    clippy::cast_precision_loss,
    reason = "normal-map texel coordinates are bounded to 0..32"
)]
fn water_normal_mip_chain() -> Vec<u8> {
    let edge = WATER_NORMAL_MAP_EDGE;
    let mut level =
        Vec::with_capacity(usize::try_from(edge.saturating_mul(edge)).unwrap_or_default());
    let edge_f32 = edge as f32;
    for y in 0..edge {
        let phase_y = TAU * y as f32 / edge_f32;
        for x in 0..edge {
            let phase_x = TAU * x as f32 / edge_f32;
            let gradient_x = 0.46 * phase_x.cos()
                + 0.19 * (phase_x * 2.0 + phase_y).cos()
                + 0.11 * (phase_x - phase_y * 3.0).cos();
            let gradient_y = 0.39 * phase_y.cos() + 0.17 * (phase_x * 2.0 + phase_y).cos()
                - 0.15 * (phase_x - phase_y * 3.0).cos();
            let normal = Vec3::new(-gradient_x, -gradient_y, 1.0).normalize();
            level.push(normal);
        }
    }

    let mut data = Vec::with_capacity(WATER_NORMAL_MAP_BYTE_LEN);
    let mut level_edge = edge;
    loop {
        append_water_normal_level(&mut data, &level);
        if level_edge == 1 {
            break;
        }
        level = downsample_water_normals(&level, level_edge);
        level_edge /= 2;
    }
    data
}

fn append_water_normal_level(data: &mut Vec<u8>, normals: &[Vec3]) {
    for normal in normals {
        data.extend([
            pack_normal_channel(normal.x),
            pack_normal_channel(normal.y),
            pack_normal_channel(normal.z),
            pack_unit_channel(normal.length()),
        ]);
    }
}

fn downsample_water_normals(source: &[Vec3], source_edge: u32) -> Vec<Vec3> {
    let source_edge = usize::try_from(source_edge).unwrap_or_default();
    let target_edge = source_edge / 2;
    let mut target = Vec::with_capacity(target_edge.saturating_mul(target_edge));
    for row_pair in source.chunks_exact(source_edge.saturating_mul(2)) {
        let (top, bottom) = row_pair.split_at(source_edge);
        for (top_pair, bottom_pair) in top.chunks_exact(2).zip(bottom.chunks_exact(2)) {
            let [top_left, top_right] = top_pair else {
                continue;
            };
            let [bottom_left, bottom_right] = bottom_pair else {
                continue;
            };
            target.push((*top_left + *top_right + *bottom_left + *bottom_right) * 0.25);
        }
    }
    target
}

#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "the clamped and rounded normal channel is in 0..=255"
)]
fn pack_normal_channel(value: f32) -> u8 {
    ((value * 0.5 + 0.5).clamp(0.0, 1.0) * 255.0).round() as u8
}

#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "the clamped and rounded unit channel is in 0..=255"
)]
fn pack_unit_channel(value: f32) -> u8 {
    (value.clamp(0.0, 1.0) * 255.0).round() as u8
}

#[cfg(test)]
fn schlick_fresnel(normal_reflectance: f32, cosine: f32, exponent: f32) -> f32 {
    let reflectance = normal_reflectance.clamp(0.0, 1.0);
    reflectance + (1.0 - reflectance) * (1.0 - cosine.clamp(0.0, 1.0)).powf(exponent.max(1.0))
}

#[cfg(test)]
fn beer_lambert_transmittance(absorption_per_m: Vec3, depth_m: f32) -> Vec3 {
    let depth = depth_m.max(0.0);
    Vec3::new(
        (-absorption_per_m.x.max(0.0) * depth).exp(),
        (-absorption_per_m.y.max(0.0) * depth).exp(),
        (-absorption_per_m.z.max(0.0) * depth).exp(),
    )
}

#[cfg(test)]
fn orient_normal_toward_view(normal: Vec3, direction_to_view: Vec3) -> Vec3 {
    let normal = normal.normalize_or_zero();
    if normal.dot(direction_to_view) < 0.0 {
        -normal
    } else {
        normal
    }
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use bevy::{
        asset::Handle,
        image::{Image, ImageAddressMode, ImageFilterMode, ImageSampler},
        prelude::{AlphaMode, Vec2, Vec3},
        render::render_resource::TextureFormat,
    };

    use super::{
        WATER_NORMAL_ANISOTROPY, WATER_NORMAL_MAP_BYTE_LEN, WATER_NORMAL_MAP_EDGE,
        WATER_NORMAL_MIP_LEVELS, WATER_NORMAL_REFLECTANCE, beer_lambert_transmittance,
        orient_normal_toward_view, production_water_material, schlick_fresnel, water_normal_image,
        water_normal_mip_chain,
    };

    fn close(left: f32, right: f32) -> bool {
        (left - right).abs() <= 1.0e-5
    }

    #[test]
    fn water_material_is_a_dedicated_two_sided_premultiplied_pipeline() {
        let material =
            production_water_material(Handle::<Image>::default(), Handle::<Image>::default());
        assert_eq!(material.base.alpha_mode, AlphaMode::Premultiplied);
        assert!(material.base.cull_mode.is_none());
        assert!(material.base.double_sided);
        assert!(close(material.base.ior, 1.333));
        assert!(!material.extension.camera_is_underwater());
    }

    #[test]
    fn authoritative_medium_selects_below_water_parameters() {
        let mut material =
            production_water_material(Handle::<Image>::default(), Handle::<Image>::default());
        material.extension.set_camera_underwater(true);
        assert!(material.extension.camera_is_underwater());
        material.extension.set_camera_underwater(false);
        assert!(!material.extension.camera_is_underwater());
    }

    #[test]
    fn schlick_oracle_has_physical_endpoints_and_is_monotone() {
        let facing = schlick_fresnel(WATER_NORMAL_REFLECTANCE, 1.0, 5.0);
        let diagonal = schlick_fresnel(WATER_NORMAL_REFLECTANCE, 0.5, 5.0);
        let grazing = schlick_fresnel(WATER_NORMAL_REFLECTANCE, 0.0, 5.0);
        assert!(close(facing, WATER_NORMAL_REFLECTANCE));
        assert!(facing < diagonal);
        assert!(diagonal < grazing);
        assert!(close(grazing, 1.0));
    }

    #[test]
    fn beer_lambert_oracle_darkens_with_depth_and_absorbs_red_first() {
        let absorption = Vec3::new(0.42, 0.105, 0.032);
        let surface = beer_lambert_transmittance(absorption, 0.0);
        let shallow = beer_lambert_transmittance(absorption, 1.0);
        let deep = beer_lambert_transmittance(absorption, 8.0);
        assert_eq!(surface, Vec3::ONE);
        assert!(deep.cmplt(shallow).all());
        assert!(deep.x < deep.y);
        assert!(deep.y < deep.z);
    }

    #[test]
    fn two_sided_normal_oracle_faces_the_view_from_above_and_below() {
        let above = orient_normal_toward_view(Vec3::Y, Vec3::Y);
        let below = orient_normal_toward_view(Vec3::Y, Vec3::NEG_Y);
        assert_eq!(above, Vec3::Y);
        assert_eq!(below, Vec3::NEG_Y);
    }

    #[test]
    fn generated_normal_map_is_linear_tileable_and_forward_facing() {
        let image = water_normal_image();
        assert_eq!(image.texture_descriptor.size.width, WATER_NORMAL_MAP_EDGE);
        assert_eq!(image.texture_descriptor.size.height, WATER_NORMAL_MAP_EDGE);
        assert_eq!(
            image.texture_descriptor.mip_level_count,
            WATER_NORMAL_MIP_LEVELS
        );
        assert_eq!(image.texture_descriptor.format, TextureFormat::Rgba8Unorm);
        match &image.sampler {
            ImageSampler::Descriptor(descriptor) => {
                assert_eq!(descriptor.address_mode_u, ImageAddressMode::Repeat);
                assert_eq!(descriptor.address_mode_v, ImageAddressMode::Repeat);
                assert_eq!(descriptor.mag_filter, ImageFilterMode::Linear);
                assert_eq!(descriptor.min_filter, ImageFilterMode::Linear);
                assert_eq!(descriptor.mipmap_filter, ImageFilterMode::Linear);
                assert_eq!(descriptor.anisotropy_clamp, WATER_NORMAL_ANISOTROPY);
            }
            ImageSampler::Default => panic!("water normal map must use its explicit sampler"),
        }
        let data = image.data.expect("generated image retains its CPU texels");
        assert_eq!(data.len(), WATER_NORMAL_MAP_BYTE_LEN);
        assert_eq!(data, water_normal_mip_chain());

        let base = mip_level(&data, 0);
        for texel in base.chunks_exact(4) {
            let (normal, coherence) = decode_normal_texel(texel);
            assert!(normal.z > 0.0);
            assert!((normal.length() - 1.0).abs() < 0.015);
            assert!(close(coherence, 1.0));
        }
    }

    #[test]
    fn vector_mips_preserve_coherence_and_remove_coarse_horizontal_energy() {
        let data = water_normal_mip_chain();
        let mut previous_energy = f32::INFINITY;
        for level in 0..WATER_NORMAL_MIP_LEVELS {
            let mip = mip_level(&data, level);
            let mut horizontal_energy = 0.0;
            let mut texel_count = 0_u16;
            for texel in mip.chunks_exact(4) {
                let (mean, coherence) = decode_normal_texel(texel);
                assert!(mean.z > 0.0);
                assert!((mean.length() - coherence).abs() < 0.02);
                assert!((0.0..=1.0).contains(&coherence));
                horizontal_energy += mean.x * mean.x + mean.y * mean.y;
                texel_count = texel_count.saturating_add(1);
            }
            horizontal_energy /= f32::from(texel_count.max(1));
            assert!(
                horizontal_energy <= previous_energy + 1.0e-4,
                "mip {level} increased horizontal energy from {previous_energy} to {horizontal_energy}"
            );
            previous_energy = horizontal_energy;
        }
        assert!(previous_energy < 1.0e-4);
    }

    #[test]
    fn coarse_mip_reduces_adjacent_frame_normal_error() {
        let data = water_normal_mip_chain();
        let mut base_error = 0.0;
        let mut coarse_error = 0.0;
        for y in 0_u8..16 {
            for x in 0_u8..16 {
                let uv = Vec2::new(
                    f32::from(x) * 0.173 + f32::from(y) * 0.031,
                    f32::from(y) * 0.149,
                );
                let next_uv = uv + Vec2::new(0.013, -0.009);
                base_error += shader_normal(sample_mip(&data, 0, uv))
                    .distance(shader_normal(sample_mip(&data, 0, next_uv)));
                coarse_error += shader_normal(sample_mip(&data, 3, uv))
                    .distance(shader_normal(sample_mip(&data, 3, next_uv)));
            }
        }
        assert!(base_error > 1.0);
        assert!(
            coarse_error < base_error * 0.55,
            "coarse temporal error {coarse_error} must be materially below base error {base_error}"
        );
    }

    #[test]
    fn embedded_shader_declares_every_water_optics_stage() {
        let shader = include_str!("water_material.wgsl");
        for required in [
            "schlick_fresnel",
            "beer_lambert",
            "flow_direction",
            "sample_flow_normal",
            "normal_coherence",
            "DEPTH_PREPASS",
            "camera_underwater",
            "main_pass_post_lighting_processing",
            "out.color.rgb * out.color.a",
        ] {
            assert!(shader.contains(required), "shader is missing `{required}`");
        }
    }

    fn decode_normal_texel(texel: &[u8]) -> (Vec3, f32) {
        (
            Vec3::new(
                f32::from(texel[0]) / 255.0 * 2.0 - 1.0,
                f32::from(texel[1]) / 255.0 * 2.0 - 1.0,
                f32::from(texel[2]) / 255.0 * 2.0 - 1.0,
            ),
            f32::from(texel[3]) / 255.0,
        )
    }

    fn mip_level(data: &[u8], level: u32) -> &[u8] {
        let mut edge = WATER_NORMAL_MAP_EDGE;
        let mut offset = 0_usize;
        for _ in 0..level {
            offset = offset.saturating_add(
                usize::try_from(edge.saturating_mul(edge).saturating_mul(4))
                    .expect("test mip byte count fits usize"),
            );
            edge /= 2;
        }
        let byte_len = usize::try_from(edge.saturating_mul(edge).saturating_mul(4))
            .expect("test mip byte count fits usize");
        &data[offset..offset.saturating_add(byte_len)]
    }

    #[allow(
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "wrapped test texture coordinates remain inside the 32-texel mip"
    )]
    fn sample_mip(data: &[u8], level: u32, uv: Vec2) -> (Vec3, f32) {
        let edge = WATER_NORMAL_MAP_EDGE >> level;
        let edge_f32 = f32::from(u16::try_from(edge).expect("test mip edge fits u16"));
        let signed_edge = edge.cast_signed();
        let position =
            Vec2::new(uv.x.rem_euclid(1.0), uv.y.rem_euclid(1.0)) * edge_f32 - Vec2::splat(0.5);
        let floor = position.floor();
        let fraction = position - floor;
        let x0 = (floor.x as i32).rem_euclid(signed_edge) as u32;
        let y0 = (floor.y as i32).rem_euclid(signed_edge) as u32;
        let x1 = x0.saturating_add(1).rem_euclid(edge);
        let y1 = y0.saturating_add(1).rem_euclid(edge);
        let mip = mip_level(data, level);
        let sample = |x: u32, y: u32| {
            let index = usize::try_from(y.saturating_mul(edge).saturating_add(x).saturating_mul(4))
                .expect("test sample index fits usize");
            decode_normal_texel(&mip[index..index.saturating_add(4)])
        };
        let (top_left, top_left_coherence) = sample(x0, y0);
        let (top_right, top_right_coherence) = sample(x1, y0);
        let (bottom_left, bottom_left_coherence) = sample(x0, y1);
        let (bottom_right, bottom_right_coherence) = sample(x1, y1);
        let top = top_left.lerp(top_right, fraction.x);
        let bottom = bottom_left.lerp(bottom_right, fraction.x);
        let top_coherence =
            top_left_coherence + (top_right_coherence - top_left_coherence) * fraction.x;
        let bottom_coherence =
            bottom_left_coherence + (bottom_right_coherence - bottom_left_coherence) * fraction.x;
        (
            top.lerp(bottom, fraction.y),
            top_coherence + (bottom_coherence - top_coherence) * fraction.y,
        )
    }

    fn shader_normal((mean, coherence): (Vec3, f32)) -> Vec3 {
        let direction = Vec3::new(mean.x, mean.y, mean.z.max(0.0001)).normalize();
        Vec3::new(
            direction.x * 0.24 * coherence,
            direction.y * 0.24 * coherence,
            direction.z.max(0.2),
        )
        .normalize()
    }
}
