#import bevy_pbr::{
    forward_io::{VertexOutput, FragmentOutput},
    pbr_fragment::pbr_input_from_standard_material,
    pbr_functions::{alpha_discard, apply_pbr_lighting, main_pass_post_lighting_processing},
}

@group(#{MATERIAL_BIND_GROUP}) @binding(100) var terrain_tiles: texture_2d_array<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(101) var terrain_sampler: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(102) var<uniform> settings: vec4<f32>;

@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) front: bool) -> FragmentOutput {
    var pbr = pbr_input_from_standard_material(in, front);
    // Perspective interpolation can turn a constant 37 into 36.999996.
    // Recover the integer ID; truncation would sample the adjacent material.
    let layer = i32(round(in.uv_b.x));
    let texel = textureSample(terrain_tiles, terrain_sampler, in.uv, layer);
    if settings.x > 0.5 && texel.a < 0.5 { discard; }
    pbr.material.base_color *= texel;
    let surface = u32(round(in.uv_b.y));
    pbr.material.perceptual_roughness = max(0.05, f32(surface % 256u) / 255.0);
    pbr.material.metallic = f32(surface / 256u) / 255.0;
    pbr.material.base_color = alpha_discard(pbr.material, pbr.material.base_color);
    var out: FragmentOutput;
    out.color = main_pass_post_lighting_processing(pbr, apply_pbr_lighting(pbr));
    return out;
}
