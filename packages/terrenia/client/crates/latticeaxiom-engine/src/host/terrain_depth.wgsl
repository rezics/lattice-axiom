#import bevy_pbr::prepass_io::VertexOutput

@group(#{MATERIAL_BIND_GROUP}) @binding(100) var terrain_tiles: texture_2d_array<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(101) var terrain_sampler: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(102) var<uniform> settings: vec4<f32>;

fn leaf_coverage(in: VertexOutput) {
    let alpha = textureSample(terrain_tiles, terrain_sampler, in.uv, i32(round(in.uv_b.x))).a;
    if settings.x > 0.5 && alpha < 0.5 { discard; }
}

#ifdef PREPASS_FRAGMENT
#import bevy_pbr::prepass_io::FragmentOutput
#ifdef MOTION_VECTOR_PREPASS
#import bevy_pbr::pbr_prepass_functions::calculate_motion_vector
#endif
@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) front: bool) -> FragmentOutput {
    leaf_coverage(in);
    var out: FragmentOutput;
#ifdef NORMAL_PREPASS
    out.normal = vec4<f32>(select(-in.world_normal, in.world_normal, front) * 0.5 + vec3<f32>(0.5), 1.0);
#endif
#ifdef UNCLIPPED_DEPTH_ORTHO_EMULATION
    out.frag_depth = in.unclipped_depth;
#endif
#ifdef MOTION_VECTOR_PREPASS
    out.motion_vector = calculate_motion_vector(in.world_position, in.previous_world_position);
#endif
    return out;
}
#else
@fragment
fn fragment(in: VertexOutput) { leaf_coverage(in); }
#endif
