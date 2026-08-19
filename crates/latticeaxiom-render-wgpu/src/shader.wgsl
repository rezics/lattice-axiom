// Forward pass for flat-colored, instanced meshes.
//
// World space is right-handed Z-up (ADR 0011); `globals.view_proj` already
// contains the full world-to-clip transform, so this shader never needs to
// know about axis conventions.

struct Globals {
  view_proj: mat4x4<f32>,
};

@group(0) @binding(0)
var<uniform> globals: Globals;

struct VertexIn {
  @location(0) position: vec3<f32>,
  @location(1) normal: vec3<f32>,
};

struct InstanceIn {
  @location(2) model_0: vec4<f32>,
  @location(3) model_1: vec4<f32>,
  @location(4) model_2: vec4<f32>,
  @location(5) model_3: vec4<f32>,
  @location(6) color: vec4<f32>,
};

struct VertexOut {
  @builtin(position) clip_position: vec4<f32>,
  @location(0) world_normal: vec3<f32>,
  @location(1) color: vec4<f32>,
};

@vertex
fn vs_main(vertex: VertexIn, instance: InstanceIn) -> VertexOut {
  let model = mat4x4<f32>(
    instance.model_0,
    instance.model_1,
    instance.model_2,
    instance.model_3,
  );

  var out: VertexOut;
  out.clip_position = globals.view_proj * model * vec4<f32>(vertex.position, 1.0);
  // Rigid transforms only at milestone 1, so the linear part is fine for
  // normals; revisit when non-uniform scaling appears.
  out.world_normal = (model * vec4<f32>(vertex.normal, 0.0)).xyz;
  out.color = instance.color;
  return out;
}

// Sun direction in world space: high in the sky (+Z up), slightly east-north.
const SUN_DIRECTION: vec3<f32> = vec3<f32>(0.35, 0.2, 0.91);
const AMBIENT: f32 = 0.28;

@fragment
fn fs_main(in: VertexOut) -> @location(0) vec4<f32> {
  let normal = normalize(in.world_normal);
  let diffuse = max(dot(normal, normalize(SUN_DIRECTION)), 0.0);
  let lit = in.color.rgb * (AMBIENT + (1.0 - AMBIENT) * diffuse);
  return vec4<f32>(lit, in.color.a);
}
