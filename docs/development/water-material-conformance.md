# Depth-aware water material conformance evidence

Recorded 2026-08-30 for the native-static production water material.

## Pipeline and optical contract

Water owns one typed
`ExtendedMaterial<StandardMaterial, WaterMaterialExtension>` asset. A water
mesh cannot obtain a `StandardMaterial` handle, while opaque, cutout,
translucent, and emissive groups retain their prior standard materials. The
water base selects Bevy's premultiplied-alpha transparent pipeline, disables
face culling, and is two-sided. The extension fragment shader explicitly
premultiplies RGB before returning it to Bevy's one/one-minus-source-alpha
blend state.

The production camera carries `DepthPrepass`. Above water, the shader samples
the corresponding MSAA depth sample, reconstructs the opaque world position,
and uses surface-to-opaque distance for Beer-Lambert absorption. A bounded
fallback depth is used when no opaque sample or prepass is available. Below
water, optical depth is camera-to-surface distance. Above and below water have
separate absorption, deep tint, and alpha parameters.

The material uses Schlick Fresnel with the air-to-water normal-incidence value
`F0 = 0.02037`, a two-octave animated tangent-space normal map, and Bevy's
two-sided normal orientation. The authoritative `CameraMediumV1` selects the
below-water parameters. The shared material is dirtied only when that
component changes; visibility, winding, and screen position never infer the
camera medium.

## Hydrology presentation boundary

Water meshes alone carry UV1 flow data. The frozen encoding is:

| Flow | UV1 |
| --- | --- |
| no flow / still | `(0.5, 0.5)` |
| down | `(-1.0, -1.0)` |
| east / west | `(1.0, 0.5)` / `(0.0, 0.5)` |
| south / north | `(0.5, 1.0)` / `(0.5, 0.0)` |

Horizontal vectors animate in hydrologic world direction. The down sentinel
and vertical sheets animate downwards. Still planar water uses one fixed
fallback direction so its two normal octaves do not freeze.

One nonempty water group still contributes at most one transparent phase item
per presented chunk, so the dedicated material does not increase water draw
submissions relative to the former generic translucent group. Bevy may batch
compatible phase items, so a phase-item count is an upper bound rather than a
claim about driver draw calls. `DepthPrepass` adds one standard opaque prepass
for the view; water itself is excluded from that prepass and stays in Bevy's
existing transparent phase after opaque geometry. No render slot or
render-contract declaration changed.

## Headless and shader conformance

Headless tests prove:

- the water group has no standard-material fallback and its handle resolves
  only in `Assets<WaterMaterial>`;
- premultiplied alpha, two-sided rendering, cull state, IOR, and the initial
  above-water medium;
- exact UV1 flow encodings and one flow coordinate per emitted water vertex;
- Schlick endpoints and monotonicity, Beer-Lambert depth monotonicity and
  channel ordering, and above/below normal orientation;
- the generated 32 by 32 normal texture is linear RGBA, repeat sampled,
  forward-facing, approximately unit length, and byte bounded;
- the embedded shader retains every required optics stage.

The WGSL was composed against the exact Bevy 0.19.1 shader modules and
validated with the already locked `naga 29.0.4` / `naga_oil 0.22.0` stack in
three permutations: depth prepass plus MSAA, depth prepass without MSAA, and
the bounded no-prepass fallback. A production Vulkan client then compiled and
ran the final embedded shader with no shader, pipeline, or validation errors.
The temporary validator was not added to the workspace or dependency graph.

## Resource and optimized runtime baseline

The dedicated material adds one 144-byte uniform payload and one 32 by 32
RGBA8 normal texture (4,096 texel bytes). UV1 adds eight bytes per water mesh
vertex. The frozen 4,608-vertex fluid-mesh corpus therefore has 36,864 bytes
of added flow data and 41,104 bytes of attributable material-plus-flow
payload. The color atlas is shared with existing terrain materials. GPU
allocator alignment, pipeline caches, and driver metadata are deliberately
reported separately rather than hidden inside that payload count.

The optimized distributable was built with
`cargo build --release -p latticeaxiom-engine --bin latticeaxiom-engine
--no-default-features --features client`. The build completed with the
workspace's release settings (`opt-level = 3`, one codegen unit, thin LTO), and
the resulting executable ran for a bounded 25-second sample. The target was
Windows 11 on an Intel Core i9-14900HX and an NVIDIA GeForce RTX 4080 Laptop
GPU using Vulkan and driver 616.56. Startup reached the lock-verified client
host and reported no shader, pipeline, validation, or panic error.

`nvidia-smi` reported 3,739 MiB of total adapter memory immediately before the
release run and a 5,075 MiB peak during it, a coarse whole-adapter increase of
1,336 MiB. The 25 one-second samples ranged from 3,731 to 5,075 MiB with a
4,666.4 MiB mean. This includes the complete client, streamed world, driver
caches, desktop, and other GPU users; it is not attributed to water. The exact
41,104-byte water payload above is the bounded attributable measurement.

Bevy's per-pass GPU timestamps require the development diagnostics feature.
With the same final shader, 1280 by 720 viewport, GPU, driver, and Vulkan
backend, the final eight one-second samples recorded:

| Diagnostic | Observed interval | Mean |
| --- | --- | --- |
| transparent 3D pass GPU time | 0.346316-0.404590 ms | 0.378997 ms |
| early prepass GPU time | 0.010520-0.021596 ms | 0.015321 ms |
| transparent fragment invocations | 1.766-1.930 million | 1.858 million |

That diagnostic run used the workspace development profile (`opt-level = 1`
for project crates and `opt-level = 3` for dependencies) and a still-growing
streamed working set, so it is a reproducible local baseline rather than a
universal budget. The development-only `RenderDiagnosticsPlugin` keeps these
measurements available without adding diagnostics overhead to the static
client feature set.

The local capture at the default 1280 by 720 window covers a waterline,
shoreline, multiple fluid levels, and visible chunk seams. It was inspected
after the final shader compiled. Captures are optional local evidence and are
not a GPU requirement for normative CI.

## Advanced feature decisions

Each optional feature was considered independently. None passed the evidence
and render-contract gate for this slice.

| Feature | Capability and measured-cost assessment | Decision |
| --- | --- | --- |
| Screen-space refraction | Bevy's transmission path allocates a full-resolution view transmission texture and copies the main color texture for every configured step before additional transmissive draws. It also cannot recover off-screen information. No water-specific GPU baseline or provider requirement justifies that bandwidth and ordering change. | Deferred. The accepted depth absorption path needs no screen-color copy or new slot. |
| Order-independent transparency | Bevy 0.19.1 uses a linked fragment list plus a fullscreen resolve, requires three storage buffers per shader stage, and rejects MSAA. At 1280 by 720 with Bevy's default four fragments per pixel, node and head buffers alone are approximately 45.7 MiB before counters and uniforms. The production view uses MSAA. | Rejected for this realization. Reconsider only through a capability/provider change with overlap quality and GPU evidence. |
| Screen-space reflections | Bevy's SSR path requires depth and deferred prepasses and a deferred view, then performs a configurable fullscreen ray march. The water material is forward transparent, and screen-space misses remain view dependent. | Deferred; it would change the view realization and needs a render-contract gate. |
| Planar reflections | A planar path requires another reflected view, color/depth targets, culling work, and scene draws for every admitted water plane. No bounded plane-selection rule or measured budget exists. | Deferred; no extra camera, target, pass, or slot was added. |
| Caustics | Receiver-side caustics require a world/light integration contract or another screen-space/volume pass, plus stable behavior at chunk and fluid-level seams. The current water shader alone cannot make receivers authoritative. | Deferred until a receiver contract, quality corpus, and target-GPU measurement exist. |

## Build-versus-buy and license record

No dependency, feature, lockfile entry, or external texture was added. The
implementation uses the already pinned Bevy 0.19.1 material-extension,
prepass, PBR, and render-diagnostic facilities. Bevy is dual MIT/Apache-2.0;
its existing transitive `wgpu 29.0.4`, `naga 29.0.4`, and
`naga_oil 0.22.0` footprint is unchanged. The normal texture is generated by
small project-owned deterministic code.

Exact upstream material studied:

- Bevy 0.19.1 extended-material shader template:
  <https://github.com/bevyengine/bevy/blob/v0.19.1/assets/shaders/extended_material.wgsl>
- Bevy 0.19.1 water-material example shader:
  <https://github.com/bevyengine/bevy/blob/v0.19.1/assets/shaders/water_material.wgsl>
- Bevy 0.19.1 externally driven headless renderer:
  <https://github.com/bevyengine/bevy/blob/v0.19.1/examples/app/externally_driven_headless_renderer.rs>
- GPU Gems water normals and Fresnel reference:
  <https://developer.nvidia.com/gpugems/gpugems/part-i-natural-effects/chapter-1-effective-water-simulation-physical-models>
- GPU Gems 2 screen-space refraction reference:
  <https://developer.nvidia.com/gpugems/gpugems2/part-ii-shading-lighting-and-shadows/chapter-19-generic-refraction-simulation>
