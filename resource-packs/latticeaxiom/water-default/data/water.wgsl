#import bevy_pbr::{
    forward_io::{VertexOutput, FragmentOutput},
    mesh_view_bindings,
    pbr_fragment::pbr_input_from_standard_material,
    pbr_functions::{
        alpha_discard,
        apply_pbr_lighting,
        main_pass_post_lighting_processing,
    },
    prepass_utils,
    view_transformations,
}

struct WaterMaterialSettings {
    absorption_above_alpha: vec4<f32>,
    absorption_below_alpha: vec4<f32>,
    tint_above_deep_alpha: vec4<f32>,
    tint_below_deep_alpha: vec4<f32>,
    surface: vec4<f32>,
    motion: vec4<f32>,
    normal_depth: vec4<f32>,
    fresnel_tint_strength: vec4<f32>,
    view: vec4<f32>,
}

@group(#{MATERIAL_BIND_GROUP}) @binding(100)
var water_normal_map: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(101)
var water_normal_sampler: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(102)
var<uniform> water: WaterMaterialSettings;

fn schlick_fresnel(normal_reflectance: f32, cosine: f32, exponent: f32) -> f32 {
    let f0 = clamp(normal_reflectance, 0.0, 1.0);
    return f0 + (1.0 - f0) * pow(1.0 - clamp(cosine, 0.0, 1.0), max(exponent, 1.0));
}

fn beer_lambert(absorption_per_m: vec3<f32>, depth_m: f32) -> vec3<f32> {
    return exp(-max(absorption_per_m, vec3<f32>(0.0)) * max(depth_m, 0.0));
}

fn flow_direction(encoded: vec2<f32>, oriented_normal: vec3<f32>) -> vec3<f32> {
    // The (-1, -1) UV sentinel is explicit downward waterfall flow.
    if encoded.x < -0.5 && encoded.y < -0.5 {
        return vec3<f32>(0.0, -1.0, 0.0);
    }

    let horizontal = encoded * 2.0 - 1.0;
    if dot(horizontal, horizontal) > 0.01 {
        return normalize(vec3<f32>(horizontal.x, 0.0, horizontal.y));
    }

    // Vertical water sheets scroll down. Planar still water uses the frozen
    // material direction so two animated octaves do not stand motionless.
    if abs(oriented_normal.y) < 0.45 {
        return vec3<f32>(0.0, -1.0, 0.0);
    }
    return normalize(vec3<f32>(water.motion.x, 0.0, water.motion.y));
}

fn sample_flow_normal(
    uv: vec2<f32>,
    local_flow: vec2<f32>,
    time: f32,
    scale: f32,
    speed: f32,
    strength: f32,
) -> vec3<f32> {
    let encoded = textureSample(
        water_normal_map,
        water_normal_sampler,
        uv * scale + local_flow * time * speed,
    );
    let mean_normal = encoded.rgb * 2.0 - 1.0;
    let mean_direction = normalize(vec3<f32>(mean_normal.xy, max(mean_normal.z, 0.0001)));
    let normal_coherence = encoded.a;
    return normalize(vec3<f32>(
        mean_direction.xy * strength * normal_coherence,
        max(mean_direction.z, 0.2),
    ));
}

fn opaque_water_depth(
    in: VertexOutput,
    surface_world: vec3<f32>,
    sample_index: u32,
) -> f32 {
#ifdef DEPTH_PREPASS
    let opaque_depth = prepass_utils::prepass_depth(in.position, sample_index);
    // Bevy uses reverse-Z: a smaller positive value is farther from the view.
    if opaque_depth > 0.000001 && opaque_depth < in.position.z {
        let opaque_ndc = view_transformations::frag_coord_to_ndc(
            vec4<f32>(in.position.xy, opaque_depth, 1.0),
        );
        let opaque_world = view_transformations::position_ndc_to_world(opaque_ndc);
        return distance(surface_world, opaque_world);
    }
#endif
    return water.normal_depth.z;
}

@fragment
fn fragment(
    in: VertexOutput,
    @builtin(front_facing) is_front: bool,
#ifdef MULTISAMPLED
    @builtin(sample_index) sample_index: u32,
#endif
) -> FragmentOutput {
    var pbr_input = pbr_input_from_standard_material(in, is_front);
    let oriented_normal = normalize(pbr_input.N);

#ifdef VERTEX_UVS_B
    let encoded_flow = in.uv_b;
#else
    let encoded_flow = vec2<f32>(0.5);
#endif

    let world_flow = flow_direction(encoded_flow, oriented_normal);
    var reference_axis = vec3<f32>(0.0, 1.0, 0.0);
    if abs(oriented_normal.y) > 0.92 {
        reference_axis = vec3<f32>(0.0, 0.0, 1.0);
    }
    let tangent = normalize(cross(reference_axis, oriented_normal));
    let bitangent = normalize(cross(oriented_normal, tangent));
    let world_position = pbr_input.world_position.xyz;
    let normal_uv = vec2<f32>(dot(world_position, tangent), dot(world_position, bitangent));
    var local_flow = vec2<f32>(dot(world_flow, tangent), dot(world_flow, bitangent));
    if dot(local_flow, local_flow) < 0.01 {
        local_flow = vec2<f32>(1.0, 0.0);
    } else {
        local_flow = normalize(local_flow);
    }

    let normal_a = sample_flow_normal(
        normal_uv,
        local_flow,
        mesh_view_bindings::globals.time,
        water.normal_depth.x,
        water.motion.z,
        water.surface.z,
    );
    let normal_b = sample_flow_normal(
        normal_uv + vec2<f32>(0.37, 0.71),
        vec2<f32>(-local_flow.y, local_flow.x),
        mesh_view_bindings::globals.time,
        water.normal_depth.y,
        water.motion.w,
        water.surface.z * 0.65,
    );
    let tangent_normal = normalize(vec3<f32>(
        normal_a.xy + normal_b.xy,
        max(normal_a.z + normal_b.z, 0.2),
    ));
    pbr_input.N = normalize(
        tangent * tangent_normal.x
        + bitangent * tangent_normal.y
        + oriented_normal * tangent_normal.z
    );

    let camera_underwater = water.view.x > 0.5;
    var absorption_alpha = water.absorption_above_alpha;
    var tint_deep_alpha = water.tint_above_deep_alpha;
#ifdef MULTISAMPLED
    let water_sample_index = sample_index;
#else
    let water_sample_index = 0u;
#endif
    var optical_depth = opaque_water_depth(in, world_position, water_sample_index);
    if camera_underwater {
        absorption_alpha = water.absorption_below_alpha;
        tint_deep_alpha = water.tint_below_deep_alpha;
        optical_depth = distance(mesh_view_bindings::view.world_position.xyz, world_position)
            * water.normal_depth.w;
    }
    optical_depth = clamp(optical_depth, 0.0, water.surface.w);

    let transmittance = beer_lambert(absorption_alpha.rgb, optical_depth);
    let absorbed = clamp(
        1.0 - dot(transmittance, vec3<f32>(0.2126, 0.7152, 0.0722)),
        0.0,
        1.0,
    );
    let absorbed_color =
        pbr_input.material.base_color.rgb * transmittance
        + tint_deep_alpha.rgb * (vec3<f32>(1.0) - transmittance);
    pbr_input.material.base_color = vec4<f32>(
        absorbed_color,
        pbr_input.material.base_color.a,
    );

    let cosine = abs(dot(pbr_input.N, pbr_input.V));
    let fresnel = schlick_fresnel(water.surface.x, cosine, water.surface.y);
    let depth_alpha = mix(absorption_alpha.a, tint_deep_alpha.a, absorbed);
    pbr_input.material.base_color.a = max(
        depth_alpha,
        absorption_alpha.a + fresnel * (1.0 - absorption_alpha.a) * 0.45,
    );
    pbr_input.material.base_color = alpha_discard(
        pbr_input.material,
        pbr_input.material.base_color,
    );

    var out: FragmentOutput;
    out.color = apply_pbr_lighting(pbr_input);
    out.color = vec4<f32>(
        mix(
            out.color.rgb,
            water.fresnel_tint_strength.rgb,
            fresnel * water.fresnel_tint_strength.a,
        ),
        out.color.a,
    );
    out.color = main_pass_post_lighting_processing(pbr_input, out.color);
    // `AlphaMode::Premultiplied` selects Bevy's one / one-minus-source-alpha
    // blend state and deliberately leaves RGB untouched in its shared
    // post-processing helper. Author the required premultiplication here.
    out.color = vec4<f32>(out.color.rgb * out.color.a, out.color.a);
    return out;
}
