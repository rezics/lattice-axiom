#import bevy_pbr::{forward_io::{Vertex, VertexOutput}, mesh_functions, view_transformations::position_world_to_clip, mesh_view_bindings::{globals,view}}
@group(#{MATERIAL_BIND_GROUP}) @binding(100) var<uniform> wind: vec4<f32>;
@vertex
fn vertex(input: Vertex) -> VertexOutput {
    var out: VertexOutput;
    let transform = mesh_functions::get_world_from_local(input.instance_index);
    var world = mesh_functions::mesh_position_local_to_world(transform, vec4<f32>(input.position,1.0));
    let fade = 1.0-smoothstep(wind.z,wind.w,distance(world.xyz,view.world_position.xyz));
    world.y -= input.uv.x*(1.0-fade);
    let phase = globals.time*wind.y + world.x*0.65 + world.z*0.41;
    world.x += sin(phase)*wind.x*input.uv.x*input.uv.x*fade;
    world.z += cos(phase*0.73)*wind.x*input.uv.x*input.uv.x*fade;
    out.world_position=world; out.position=position_world_to_clip(world.xyz);
    out.world_normal=mesh_functions::mesh_normal_local_to_world(input.normal,input.instance_index);
    out.uv=input.uv; out.color=input.color;
#ifdef VERTEX_OUTPUT_INSTANCE_INDEX
    out.instance_index=input.instance_index;
#endif
#ifdef VISIBILITY_RANGE_DITHER
    out.visibility_range_dither=mesh_functions::get_visibility_range_dither_level(input.instance_index,transform[3]);
#endif
    return out;
}
