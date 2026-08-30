# World generation, hydrology, fluids, and water rendering plan

Status: proposed implementation plan, 2026-08-30.

This document selects an implementation direction for the current demo. It is
not an accepted architecture decision. Accepted decisions in the
`lattice-axiom` documentation repository override this plan. The review used
documentation revision `57c37e2334ea019521fcd85237cb8dba47d51d24`, especially
decisions 0004, 0028, and 0029.

## Decision summary

The Terrenia default surface generator should move to a hierarchical,
hydrology-constrained terrain pipeline with two representations:

1. A bounded, low-resolution 2.5D semantic surface plan owns continents,
   uplift, runoff, basins, outlets, river topology, lakes, erosion, and the
   base surface.
2. A bounded 3D density composition owns cliffs, arches, caves, overhangs,
   strata, and local terrain-provider expression.

The target pipeline is:

```text
canonical seed and resolved providers
  -> finite hydrologic-domain and boundary-port plan
  -> low-frequency semantic fields and spline-mapped initial DEM
  -> depression hierarchy and selective fill/breach
  -> MFD runoff accumulation
  -> canonical channel DAG and lake/outlet graph
  -> river water profiles and valley SDFs
  -> bounded stream-power incision and hillslope diffusion
  -> territory-owned 3D density composition
  -> material, cave/aquifer, and static-water realization
  -> chunk draft and immutable generation receipts
```

This is an internal architecture description, not a claim that
"hydrology-constrained hierarchical density field" is a published algorithm
or a new public contract name.

The following decisions are part of the direction:

- Do not tune or extend the current additive fBm terrain as the long-term
  generator.
- Do not replace it in place. A changed terrain field, hydrology plan, or
  materialized water surface requires a new provider algorithm revision and a
  new generation epoch. Existing materialized snapshots remain authoritative.
- Keep generic graph, raster, fixed-point, and validation mechanisms in
  platform crates. Terrenia owns its uplift, climate, terrain-family, spline,
  river, and material policy through its providers.
- Use MFD with exponent `p = 1` as the initial distributed runoff baseline,
  while keeping the routing interface replaceable and retaining D-infinity as
  a development comparator. Extract a deterministic single-receiver channel
  DAG for semantic rivers and the initial implicit erosion solver.
- Use Priority-Flood and a depression hierarchy for drainage correction and
  retained-lake decisions. Do not indiscriminately fill every depression.
- Use an independently implemented, deterministic subset of stream-power
  incision plus hillslope diffusion only after the topology-first slice meets
  its correctness and performance gates.
- Treat generated oceans, lakes, and rivers as static hydrologic reservoirs.
  Runtime fluid ticks process only a bounded active continuation, not every
  generated water cell.
- Retain the accepted `FluidStateV1` semantics for the immediate runtime fix.
  A volume-conserving fluid model is a future schema and ADR, not a silent
  reinterpretation of levels `0..=7`.
- Give water a dedicated mesh path, material, and camera-medium state. Water
  must not remain an alias of the generic translucent/glass path.

## Effect of the supplied research note

The supplied `.temp/调研地形生成算法.md` was treated as a set of hypotheses,
not as the selected design.

| Research-note proposal | Effect on the earlier plan | Final decision |
| --- | --- | --- |
| Dual 2.5D hydrology plus 3D density representation | Strengthens the earlier region-plan proposal and aligns it with the accepted territorial generation model. | Adopt. The dimension-level plan supplies long-range constraints; each `terrain.base` owner still controls its territory's realization. |
| Finite hydrologic domains in an infinite world | Resolves the earlier plan's under-specified upstream dependency and false boundary-outlet problem. | Adopt with explicit parent boundary ports and hard domain bounds. A halo alone is not a cross-domain solution. |
| Low-frequency semantic fields followed by splines | Replaces additive fBm as the macro shape mechanism. | Adopt. Noise supplies bounded variation; semantic splines supply controllable terrain families. |
| Priority-Flood plus selective lake retention | Makes lake levels, spill points, and drainage testable. | Adopt. Build the depression hierarchy from the unmodified DEM, then classify retain, fill, or breach policy. |
| MFD instead of D-infinity | Changes the earlier default. The 2025 evaluation found substantial D-infinity orientation bias and better MFD results for its tested contributing-area cases. | Adopt MFD `p = 1` as the first runoff baseline, but keep routing pluggable and require rotation/analytic conformance before freezing the revision. |
| FastScape-style implicit stream-power erosion | Upgrades erosion from optional visual polish to a target landscape-evolution stage. | Adopt in a later slice, not as the first hydrology milestone. The classic linear implicit path assumes a canonical downstream dependency and cannot be attached directly to a multi-receiver MFD graph. |
| River and valley SDFs | Adds a continuous geometric contract between the river graph and 3D density. | Adopt. Medium and large channels receive explicit SDF envelopes; detail fields may not sever them. |
| Fixed initial grid, region, and halo sizes | Provides useful experiment ranges only. | Do not freeze. Select them from measured quality, memory, latency, and parallel-scaling results. |
| The name `HCHDF` | Provides a convenient description but could be mistaken for a standard or accepted public API. | Do not place it in stable IDs or public schemas without a separate accepted decision. |
| Water simulation and underwater rendering | Not covered by the research note. | Retain and refine the earlier fluid-runtime and rendering plan below. |

## Current failure evidence

The existing implementation has four independent failure classes.

### Terrain field

`terrain_field.rs` uses a square gradient lattice with eight directions and a
cell interpolation unit of 1024. For a scale of 8192 voxels, the interpolation
phase changes only once per eight voxel coordinates. The same family of field
is then used for domain warp, continent, ridge, hill, valley, and detail terms.
The additive composition amplifies lattice orientation, quantization, and
cross-hatch artifacts instead of providing semantic terrain control.

Relevant code:

- [`terrain_field.rs`](../../crates/latticeaxiom-worldgen/src/terrain_field.rs)
- [`terrain_config.rs`](../../crates/latticeaxiom-worldgen/src/terrain_config.rs)
- [`packages/terrenia/worldgen/src/lib.rs`](../../packages/terrenia/worldgen/src/lib.rs)

### Surface hydrology

The current drainage value is an absolute-noise contour proxy. River
"accumulation" is a hash sparsity test rather than routed runoff. Lake
candidates are jittered grid blobs rather than depressions with catchments and
spill points. River fill reconstructs a fixed incision beneath the original
surface and therefore does not prove a downstream-monotone water profile.

Relevant code:

- [`natural.rs`](../../crates/latticeaxiom-worldgen/src/natural.rs)
- [`hydrology.rs`](../../crates/latticeaxiom-worldgen/src/hydrology.rs)

### Runtime fluid

The planner emits cross-chunk `FluidBoundaryIntent` values, but the current
host application path consumes only in-chunk mutations. The host rebuilds a
frontier by scanning non-empty fluid cells and applies chunk plans serially,
so the stored continuation is not the sole work source and results can depend
on traversal order.

Relevant code:

- [`voxel-runtime/src/fluid.rs`](../../crates/latticeaxiom-voxel-runtime/src/fluid.rs)
- [`engine/src/host/fluid.rs`](../../crates/latticeaxiom-engine/src/host/fluid.rs)

### Water presentation

Fluid level and flow are discarded before meshing. Water is routed through the
generic translucent group, all groups cull back faces, and the client camera
has no air/water/lava medium state. Looking up at the water surface from below
therefore exposes a culled back face while the rest of the scene still uses air
lighting and fog.

Relevant code:

- [`engine/src/host/spine.rs`](../../crates/latticeaxiom-engine/src/host/spine.rs)
- [`engine/src/host/layers.rs`](../../crates/latticeaxiom-engine/src/host/layers.rs)
- [`voxel-mesh/src/geometry.rs`](../../crates/latticeaxiom-voxel-mesh/src/geometry.rs)
- [`engine/src/host/chunk_mesh.rs`](../../crates/latticeaxiom-engine/src/host/chunk_mesh.rs)
- [`engine/src/host/client.rs`](../../crates/latticeaxiom-engine/src/host/client.rs)

On 2026-08-30, the locked `latticeaxiom-worldgen`,
`latticeaxiom-voxel-runtime`, and `latticeaxiom-voxel-mesh` suites completed
161 tests successfully. Those tests do not currently reject the morphology,
hydrology, cross-chunk execution, or underwater presentation failures above.

## Binding architecture constraints

The implementation must preserve the following accepted decisions.

### Provider and territory ownership

There is one dimension generation coordinator, not one global terrain
algorithm. The coordinator compiles ownership, dependency, boundary, and
budget rules. Each `channel x territory ownership domain` has one resolved
base provider, and long-range hydrology is a shared plan/constraint consumed by
those providers.

The new pipeline must therefore not hard-code Terrenia terrain policy in the
host or force deserts, wetlands, mountains, and floating-island territories to
share one base realization. A territory may replace its local `terrain.base`
while still satisfying the shared basin, river, coastline, and boundary-port
contracts.

### Epoch and persistence

The current D4 output is a versioned fixed-point contract. D4 is not required
to become the full hydrology system. The new generator is compiled as a new
provider algorithm revision and generation epoch. It may serve unmaterialized
planning cells only after the existing boundary receipt rules can join its
terrain, cave portals, and hydrology exits to frozen neighbors.

Hydrologic domains are not the current D4 planning cells. With the default
configuration a planning cell is only 128 by 128 voxels, which is too small for
meaningful drainage. A hydrologic domain spans a bounded set of planning cells
and has its own stable identity and artifact receipt.

A domain plan is keyed by generation epoch and cannot assume that all covered
planning cells materialize together. If an unmaterialized cell later selects a
different epoch, the old plan and materialized snapshots stay unchanged. The
new epoch treats the shared planning-cell edge as a hydrology boundary, consumes
the old direction-independent outlet/signature receipt, and publishes a new
adapter/port result. It must not reopen and recompute the old domain.

### Bevy boundary and parallelism

Bevy remains the only task, ECS, render, asset, and diagnostics runtime.
Domain planning and independent tile work use Bevy task pools. Result
publication validates instance, generation plan, epoch, domain, and chunk
revision. Stable sorted merges, not task completion order, determine output.

The authoritative surface plan is CPU and headless. GPU field evaluation,
erosion, or meshing may be an optional presentation/authoring accelerator only.

### Render-provider boundary

The first water implementation should remain a typed Bevy `NativeStatic`
material/mesh path and existing render data or feature. It must not invent a
portable wgpu mirror or a general engine-coupled interface. Screen-color
refraction that genuinely requires a new `after.opaque` semantic slot is gated
by decision 0029 and requires its own consumer evidence and mapping tests.

## Target semantic artifacts

The exact Rust names and schema majors are frozen only when their first slice
is implemented. The conceptual artifacts are:

### Hydrologic domain plan

```text
HydrologicDomainPlan
  identity and parent-domain identity
  generation epoch, input hash, algorithm revision, provenance
  bounded world-space core and halo bounds
  grid origin, spacing, dimensions, and quantization
  macro ocean/base-level and boundary-port constraints
  continentalness, uplift, lithology, and effective-runoff fields
  initial and eroded surface grids
  depression hierarchy and basin labels
  weighted MFD receiver graph
  canonical channel receiver graph
  lake, river, outlet, and wetland records
  cross-domain input/output ports
  hard work, queue, memory, and iteration accounting
```

Dense arrays are internal, bounded cache artifacts. Process-external or
persistent records use Lattice-owned stable DTOs and canonical ordering; they
do not expose Bevy handles, hash-map iteration, or Rust layout.

### River segment

```text
RiverSegment
  stable ID
  upstream segment IDs and optional downstream ID
  basin and water-body IDs
  quantized centerline and SDF influence envelope
  effective discharge and Strahler order
  width, bed depth, bank profile, and floodplain width
  monotone water-profile control points
  quantized presentation flow tangent
```

The channel graph is acyclic except for explicitly modeled distributary
features in a future algorithm revision. Every released segment must end at a
downstream segment, retained lake, ocean outlet, or declared boundary port.

### Water body

```text
WaterBody
  stable ID
  Ocean | Lake | River | Wetland
  basin and outlet identity
  surface model
  static-reservoir policy
  materialization bounds
  presentation parameters
```

Surface models have different semantics:

- Ocean: one dimension-defined equipotential level.
- Lake: one constant level for the retained depression, derived from its spill
  point and water policy.
- River: a continuous piecewise profile that never rises downstream unless a
  specifically represented hydraulic structure requires it. Waterfalls are
  explicit downward discontinuities.
- Wetland: a bounded shallow occupancy policy linked to basin saturation, not
  a random lake blob.

## Numeric and noise design

### Authoritative noise

Use the CC0 OpenSimplex2 reference only as an algorithmic source and oracle.
Implement the required 2D OpenSimplex2S and 3D OpenSimplex2F subset in safe
Rust, with no global mutable table and no `unsafe` code.

The prototype should use split integer lattice coordinates plus a high-
resolution fixed-point cell fraction. Q0.32 fractions with `i128`
intermediates are the first candidate, but the exact representation is frozen
only after interval analysis proves every supported coordinate, octave, skew,
dot product, interpolation, and accumulation bound. At least 24 fractional
phase bits are required for evaluation; terrain outputs may be quantized more
coarsely only at a named boundary.

Every operation must specify:

- Euclidean floor/division for negative coordinates;
- fixed constants for skew, unskew, gradients, and normalization;
- rounding direction at every reduction;
- checked or proven-bounded intermediate arithmetic;
- stable seed-domain separation per field and octave;
- exact octave weight normalization;
- output range and error against a double-precision oracle.

`fastnoise-lite 1.1.1` remains a development-only visual and statistical
oracle. Floating-point or SIMD implementations do not define authoritative
chunk bytes.

### Semantic macro fields

Noise does not directly decide final height. The dimension and territory
providers produce low-frequency, named inputs such as:

- continentalness and distance to base level;
- uplift or mountain-axis strength;
- ridge/plate/region distance;
- lithology and erodibility;
- temperature, precipitation, infiltration, and effective runoff;
- terrain-family and boundary-profile identity.

Closed, versioned splines map those inputs to base elevation, relief,
roughness, cliff tendency, and erosion coefficients. Each field and spline has
an explicit spatial frequency and maximum output envelope. Independent seed
domains prevent adding one field from perturbing unrelated content.

Medium- and high-frequency detail is applied after hydrology and is masked near
coastlines, retained lake surfaces, river beds, required cave portals, and
protected structures. Its displacement budget must be too small to change the
approved macro topology.

## Hydrologic-domain construction

### Infinite-world boundary strategy

Hydrology is never solved per chunk. A hierarchy converts unbounded world
coordinates into bounded work:

1. A deterministic macro plan identifies oceans/base levels, major uplift
   barriers, finite drainage provinces, and parent-level ports.
2. Each hydrologic domain receives a finite set of required outlets, permitted
   endorheic basins, and cross-domain inflow/outflow ports.
3. The full domain is solved as one semantic unit. Region tiles and halos are
   an implementation detail for raster work inside that unit.
4. A canonical owner publishes every shared river, lake, coastline, and port.
   Neighbors verify the same direction-independent signature.
5. A chunk samples an immutable domain plan and materializes only intersecting
   geometry.

Domains must have hard maximum area, grid points, ports, graph nodes, work,
memory, and wall-time cancellation checks. A parent plan must be obtainable by
bounded coordinate lookup; generation may not recursively search an unbounded
upstream world.

A tile halo reduces convolution, SDF, and erosion seams. It does not by itself
make drainage across independently opened tile boundaries correct. Parent
ports and canonical ownership are mandatory.

### Initial DEM and depression policy

Build a low-resolution initial DEM from the semantic fields and territory
constraints. Before destructive correction, derive a depression hierarchy
that records pits, nested basins, spill saddles, volumes, and ultimate outlets.

Classify each depression using closed policy inputs:

- retain as a permanent lake;
- retain as an endorheic or seasonal basin;
- realize as wetland;
- breach through an allowed saddle;
- fill as a small numerical/artifact pit.

Priority-Flood or Priority-Flood plus an exact epsilon/ordering rule provides
the corrected routing surface. The original elevation and retained depression
metadata remain available for lake geometry and diagnostics.

### Runoff routing

The first authoritative candidate is Freeman-style MFD with slope exponent
`p = 1`; a development oracle also evaluates the literature-default `p = 1.1`:

```text
weight(i -> j) = positive_downslope(i, j) / sum(positive_downslope(i, k))
Q(i) = effective_runoff(i) * cell_area + sum(weight(j -> i) * Q(j))
```

Weights are fixed-point rationals with a defined residual-allocation rule so
the distributed amount is exactly conserved. Equal elevations and equal
weights use canonical coordinate/direction tie breaks. Processing follows a
stable elevation topological order and cannot depend on a hash map or task
completion order.

MFD is first used to estimate distributed contributing runoff and select a
channel mask. It does not itself become the semantic river graph. For every
channelized cell, select one canonical downstream receiver from the largest
routed flux, then slope, then stable coordinate order. Run a second conservative
accumulation pass: non-channel cells retain MFD weights, while channel cells
route their complete discharge to the canonical receiver. Collapse those cells
into a semantic river DAG and compute Strahler order, discharge, length,
gradient, and endpoints. This preserves distributed hillslope capture without
silently losing water when the channel representation becomes single-receiver.

This separation is deliberate:

- MFD reduces the tested orientation bias of D-infinity for contributing-area
  fields and represents divergent hillslope runoff.
- Gameplay, SDF carving, water profiles, and the initial implicit erosion
  solver need a stable, inspectable channel graph.
- IDS is reserved for later wide-channel, floodplain, or delta work because it
  solves water depth iteratively and has materially greater parameter and
  runtime cost.

Before MFD is frozen, dev-only `p = 1.1` MFD and D-infinity implementations and
analytic plane, cone, rotation, and real-DEM corpora must compare error,
anisotropy, river topology, runtime, and memory. MFD `p = 1` remains the
authoritative candidate unless those Lattice measurements justify the extra
numeric complexity of `p = 1.1` or contradict the cited study for this
workload.

### Lakes and river profiles

Retained lake level is the depression spill level or a lower climate/storage
level explicitly computed by policy. Every cell belonging to one lake must
sample the same authoritative surface value, including across region and chunk
boundaries.

River width and depth are closed splines of effective discharge, order,
lithology, gradient, and terrain family. The centerline is smoothed within a
bounded catchment corridor; smoothing cannot move it across a watershed or
disconnect a confluence. The longitudinal water profile is solved from outlet
to source under a non-rising-downstream invariant. Explicit waterfalls are
the only discontinuities.

Medium and large channels generate valley, bank, bed, and protected-water SDF
envelopes. Small streams may use the eroded DEM if the same connectivity and
water-profile validators pass.

## Landscape evolution

The topology-first release does not wait for a full erosion model. After that
release is characterized, add a deterministic, bounded landscape-evolution
stage:

```text
dH/dt = uplift - K * Q^m * slope^n + D * laplacian(H)
```

The initial production candidate uses `n = 1` and a closed rational `m`
chosen to permit a deterministic integer/fixed-point implementation. If
`m = 1/2` is selected, discharge roots use a specified integer square-root
algorithm. This choice must first reproduce reference profiles within a
declared error; it is not frozen by this document.

Use a FastScape-style implicit downstream solve only on the canonical
single-receiver channel DAG. Classic FastScape's linear-time dependency is not
assumed for the weighted MFD graph. Hillslope diffusion uses a deterministic
two-buffer solver with a fixed iteration/convergence contract and a stable
boundary condition supplied by the domain halo and ports.

The erosion loop is bounded and explicit:

1. route runoff and build the canonical channel graph;
2. apply implicit channel incision;
3. apply hillslope diffusion;
4. recompute routing and validate topology;
5. repeat only up to a closed maximum iteration count;
6. produce the final graph, DEM, and diagnostic residuals.

Sediment transport, deltas, flood inundation, and GPU droplet erosion are not
part of the first authoritative erosion revision. They require separate
consumers, budgets, conservation tests, and algorithm revisions.

## Three-dimensional density and materialization

The eroded surface is a common plan, not the final terrain everywhere. A
resolved territory provider composes bounded contributions conceptually as:

```text
density(x, y, z)
  = surface_base(eroded_height, y)
  + bounded_terrain_family_volume
  + bounded_geologic_volume
  - approved_cave_volume
  - river_and_water_protection_volume
  + bounded_detail
```

Each contribution declares an owner, spatial envelope, maximum displacement,
revision, and compositor. River continuity, lake surface, coast, required cave
portal, structure protection, and territory boundary conditions are hard
constraints; they are not averaged away by density blending.

Surface material, geology, resources, vegetation, caves, aquifers, and fluid
occupancy consume the approved semantic plan after final solid occupancy is
known. The current rule that materialization follows cave/void occupancy is
retained.

## Generated water and runtime fluid

### Static generated reservoirs

Generated oceans, lakes, rivers, wetlands, aquifers, and lava pools are
authoritative world-generation occupancy, but they are not automatically
scheduled runtime simulations.

- Materialization writes canonical `FluidStateV1` values. Full interior cells
  use full level; surface cells encode the permitted `0..=7` fractional level
  derived from the water profile. Flow is supplied by the hydrology plan and
  quantized to the accepted cardinal states where required.
- The simulation continuation contains only active cells. A newly generated
  ocean does not enqueue every source cell.
- A gameplay edit can activate a bounded neighborhood. Existing v1 source
  semantics then apply honestly; they are not described as volume conserving.

### Immediate `FluidStateV1` runtime repair

One simulation tick becomes a two-phase operation over a revision-consistent
working-set snapshot:

1. Load the persisted active frontier and bounded neighboring cells.
2. Partition work by chunk and plan independent mutations on Bevy task pools.
3. Collect in-chunk mutations, cross-chunk boundary intents, next-frontier
   entries, and accounting without mutating the working set.
4. Sort by destination chunk, voxel coordinate, fluid identity, and stable
   intent key.
5. Resolve collisions and mixing policy deterministically.
6. Validate chunk revisions and all hard cell, queue, byte, and time budgets.
7. Apply the complete accepted batch and persist its continuation; reject stale
   or partial plans.

Tests must cover both sides of every chunk boundary, unloaded-neighbor
deferral, reload, order randomization, stale revision rejection, queue bounds,
and zero partial application after a fault.

### Future conservative fluid

A per-cell volume, pressure, diagonal/upward flux, mixing, or shallow-water
surface model changes accepted `FluidStateV1` semantics. It requires a new
schema, content revision, persistence migration, two real consumers, and an
accepted ADR. A likely candidate is sparse, double-buffered fixed-point volume
flux for locally active cells, with generated reservoirs acting as explicit
boundary conditions. It is not part of the immediate repair.

## Water meshing and rendering

### Immediate underwater correctness fix

1. Add a distinct water mesh/material classification instead of sharing the
   generic translucent/glass group.
2. Render the water surface two-sided, using `cull_mode = None` and correct
   double-sided normals, or emit a deliberate underside. Do not disable culling
   for every translucent material.
3. Sample the authoritative fluid layer at the camera eye every frame and
   maintain an `Air`, `Water`, or `Lava` medium state with waterline hysteresis.
4. Drive Bevy distance fog, color attenuation, and medium-specific clear/color
   grading from that state. Medium detection must remain correct when the
   surface is off-screen, culled, or temporarily missing.

This fixes the current below-surface view before advanced reflection or
refraction work.

### Fluid-aware mesh

The mesh-source input must preserve fluid identity, level, and flow. For water:

- place the top face at the presentation height defined by `FluidStateV1`;
- derive corner heights from a one-cell neighbor halo using a frozen smoothing
  rule;
- keep equal-level oceans and lakes exactly planar across chunk boundaries;
- generate side faces only against lower fluid or non-fluid occupancy;
- generate explicit vertical sheets for downward flow and waterfalls;
- cull interior water faces;
- use shared boundary samples so either chunk generation order yields the same
  seam vertices and normals.

Glass and other translucent blocks remain in their existing path. Water gets a
dedicated draw/material identity even if the first implementation still uses a
Bevy `StandardMaterial` derivative.

### Water shader stages

The first dedicated material should provide:

- depth-tested rendering after opaque geometry;
- premultiplied alpha;
- Schlick Fresnel with correct above/below normal orientation;
- animated normal maps and hydrology flow direction;
- depth-based Beer-Lambert absorption;
- separate above-water and underwater parameters.

Screen-color refraction, planar/SSR reflections, caustics, and order-
independent transparency are later measured features. Bevy OIT is not the
default fix because it has GPU-memory, MSAA, shader-integration, and platform
constraints. Refraction that needs a new render slot follows the decision 0029
gate rather than bypassing it through an arbitrary render-graph hook.

## Scheduling, caching, and bounded work

- Compile immutable provider and semantic-plan handles into the generation
  plan before chunk work starts.
- Cache hydrologic-domain plans by dimension, domain ID, generation input hash,
  algorithm revision, and exact config hash.
- Persist a domain plan only when recomputation cost or cross-session random
  access justifies it; persisted forms use versioned DTOs and provenance.
- Generate independent domains and erosion tiles on Bevy task pools. Within a
  domain, stable topological levels may run in parallel, followed by a stable
  merge.
- Never hold an exclusive Bevy `World` while performing DEM, routing, erosion,
  or SDF work.
- Bound every queue, raster, graph, halo, iteration count, candidate list,
  temporary byte count, and result byte count before allocation.
- Chunk materialization samples prepared plans; it does not rerun domain
  hydrology or scan all registered providers.

The research note's suggested 8-32 voxel macro spacing, 64-128 sample tile
edge, and 8-24 sample halo are experiment ranges only. The first benchmark
matrix should include those values, but no default enters a config schema until
quality, memory, cold/cached latency, and parallel scaling are recorded on the
target profile.

## Determinism and conformance

### Numeric field gates

- known-answer vectors for every hash, gradient, skew, octave, spline, and
  fixed-point primitive;
- negative, zero, boundary, and maximum supported coordinates;
- overflow interval proof plus boundary and one-past-boundary tests;
- cross-target byte-identical golden fields and domain plans;
- derivative/plateau detection and 2D radial-spectrum/axis-energy metrics;
- comparison against OpenSimplex2 and FastNoise Lite floating-point oracles
  with a declared error envelope.

### Hydrology gates

- every non-retained land cell reaches a retained basin, ocean, or declared
  boundary outlet;
- no cycle or orphan exists in the channel graph;
- routed runoff is conserved exactly after fixed-point residual assignment;
- upstream discharge does not disappear at confluences;
- every river endpoint and confluence has one canonical owner;
- river water profiles never rise downstream; waterfalls only lower them;
- every lake is level and has the declared spill/endorheic behavior;
- shared-domain and shared-chunk signatures are direction independent;
- chunk/domain query order, task count, scheduling, registration order, and
  restart do not alter bytes;
- analytic plane/cone and rotated DEM corpora quantify MFD, D-infinity, and
  grid orientation instead of relying on visual intuition.

### Terrain quality gates

For a fixed multi-seed corpus, record and compare:

- land/ocean ratio and connected-component distribution;
- elevation histogram, hypsometry, slope, curvature, and relief by scale;
- coastline length and orientation distribution;
- drainage density, bifurcation/Strahler statistics, basin area, and river
  length/gradient distributions;
- lake area/depth/spill distributions;
- axis and diagonal spectral energy;
- maps at macro, region, chunk, and player-view scales.

Metrics reject regressions and pathological outputs; they do not replace
art-direction review. Fixed maps and in-engine captures remain a required
human acceptance artifact, while normative CI remains headless and GPU-free.

### Fluid and rendering gates

- cross-chunk v1 boundary intents are consumed exactly once;
- simulation results are invariant under chunk and task ordering;
- persisted frontier, unload/reload, stale task, and fault-injection cases are
  bounded and atomic;
- water face counts, heights, cull state, material identity, and seam vertices
  have headless tests;
- camera-medium detection is independent of surface visibility and has tests
  immediately above, at, and below a waterline;
- local/optional render captures cover looking up/down across a surface,
  shoreline, chunk seam, waterfall, glass overlap, and multiple fluid levels.

### Performance gates

Measure optimized builds before replacing any current path:

- cold and cached domain-plan latency;
- peak raster, graph, queue, cache, and result memory;
- chunk sampling/materialization throughput;
- erosion cost by grid size and iteration count;
- single-core and Bevy task-pool scaling;
- runtime-fluid cells, intents, frontier size, and tick P50/P95/P99;
- water mesh vertices, build latency, draw calls, GPU time, and VRAM.

The first measurements establish budgets. This document does not invent a
frame-time or memory threshold that has not been measured on the accepted
target profile.

## Build-versus-buy and license record

The following sources were reviewed as of 2026-08-30. They are references or
development oracles unless a later slice explicitly adopts them.

| Candidate | Exact version/revision and license | Dependency/maintenance evidence | Decision |
| --- | --- | --- | --- |
| OpenSimplex2 | `4cd120d35bfc27096698de90d1bcbf4f9d359a3b`, CC0-1.0 | Canonical multi-language source; the Rust 4D path includes unsafe global-table behavior that is not acceptable here. | Independently adapt only the needed 2D/3D algorithm into safe fixed-point Rust; keep oracle fixtures and source attribution. |
| FastNoise Lite | Rust crate `1.1.1`, MIT | Broad field/fractal/domain-warp support; default Rust feature impact is small, but output is floating point. Already reviewed in `docs/development/upstream-library-audit.md`. | Development-only preview/statistical oracle; no authoritative dependency. |
| `noise` / `noise-functions` / `simdnoise` / FastNoise2 / `quick-noise` | `0.9.0` / `0.8.5` / `3.1.6` / `0.4.0` / `0.2.0`; permissive licenses | More transitive crates, unsafe/SIMD, native C++/FFI, cross-SIMD float variation, age, or immaturity depending on candidate. | Do not adopt for authoritative terrain. Revisit only with a new audit and conformance/benchmark evidence. |
| Priority-Flood paper/reference | Paper DOI `10.1016/j.cageo.2013.04.024`; reference repository HEAD `ddaf2d44a201bfa72b7a4bab270669ff2e57c83b` has no relied-upon license grant | Mature algorithm and short published pseudocode; repository delegates production use to RichDEM. | Implement from the paper/pseudocode and tests; do not copy repository code. |
| Fill-Spill-Merge | `r-barnes/Barnes2020-FillSpillMerge` HEAD `1c499ea475c09b9f4c5da74ee5cc995de169db63`, MIT | Paper-backed reference with correctness material; C++/GDAL-oriented rather than a Rust game-runtime dependency. | Use as a development oracle; write a bounded safe Rust adapter/implementation around Lattice DTOs. |
| Landlab | HEAD `168494aa9658e08d2e7219d0904004a10a600236`, MIT | Maintained Python landscape-modeling toolkit with MFD and Fastscape components; dependency stack is unsuitable for production runtime. | Optional offline oracle and corpus generator only. |
| FastScape Fortran | HEAD `45f9036b9e8a4bcb14b9ddd0a1c5cbfaec8eb4fc`, GPL-3.0; Braun-Willett algorithm DOI `10.1016/j.geomorph.2012.10.008` | Mature scientific implementation, but native Fortran integration and its model/data assumptions do not match the production contract. | Study and compare profiles; independently implement the smallest deterministic single-receiver subset. |
| NeoTerraForged | 1.21.1 branch revision `9a7c782cbf642fc9cc9b86201159a0735a38b388`, MIT | Practical tile+border batches, river graph/carving, smoothing, and droplet-erosion filters. | Adapt architecture and conformance ideas with notice; do not import its Java runtime or make droplet erosion the macro solver. |
| Tectonic | `3.0.25`, revision `34241bdb35acda67b5367d49f354c66c05e098e2`, no root license | Current Minecraft density/spline practice, but no permission to copy. | Study dataflow only; no code copy or dependency. |
| BigGlobe V6 | reviewed at `de3dec9cc40e56ae7a6313ba62fdb5c688412197`, custom all-rights-reserved license | License restricts reuse and AI-related use. | Exclude from implementation sources. |
| Riverbed | `00005a3a6b8f181ca2641f046434a4397a7c4dca`, MIT | Active Bevy voxel project, but its own roadmap still lists flowing rivers as future work. | Useful integration comparison, not a river/hydrology solution. |

The RustSec advisory database at revision
`b331df68b8f1e1748b03e669705d1438ab3c4ee4` had no exact package-name match for
the reviewed Rust noise candidates. That is not a security guarantee. Unsafe,
native, SIMD, maintenance, and supply-chain surface remain part of the
selection decision. No new production dependency is authorized by this plan.

Code may be adapted from CC0/MIT sources under their terms and notices. Paper
algorithms may be independently implemented with citation. Do not copy
Tectonic, BigGlobe, Mojang code/assets, or any source without an applicable
license.

## Incremental delivery

Each slice is independently verifiable and receives a separate Conventional
Commit. Characterization precedes replacement, and the old generator remains
available until the new epoch passes its gates.

### Slice 0: freeze current evidence

- Add fixed-seed maps, spectra, morphology statistics, hydrology diagnostics,
  and optimized benchmark baselines for the current path.
- Add failing characterization for river endpoints, lake levels, cross-chunk
  fluid application, water culling, and camera medium.
- Record machine, build profile, exact config, provider identities, and commit.

Suggested commit: `test(worldgen): characterize terrain and water failures`.

### Slice 1: correct underwater state

- Add distinct water presentation identity/material.
- Make only water surfaces two-sided.
- Add camera medium detection, hysteresis, and water/lava fog parameters.
- Add headless semantic tests and local render captures.

Suggested commit: `fix(render): correct underwater medium and water culling`.

### Slice 2: preserve fluid geometry

- Carry fluid identity, level, and flow into mesh-source input.
- Generate level-aware tops, sides, waterfalls, and seam-stable corner heights.
- Keep glass and generic translucent materials separate.
- Benchmark mesh size and build latency.

Suggested commit: `feat(voxel-mesh): add fluid-aware water surfaces`.

### Slice 3: repair `FluidStateV1` execution

- Make persisted continuation/frontier authoritative for scheduled work.
- Plan from a consistent multi-chunk snapshot.
- Consume boundary intents through a stable merge and reject stale revisions.
- Preserve accepted source/level/flow semantics and all hard budgets.

Suggested commit: `fix(fluid): apply deterministic cross-chunk boundary intents`.

### Slice 4: add field conformance infrastructure

- Add the safe fixed-point OpenSimplex2 subset behind a new algorithm revision.
- Add floating-point oracle tools, known-answer vectors, spectral tests, and
  optimized field benchmarks.
- Do not switch terrain output in this slice.

Suggested commit: `feat(worldgen): add fixed opensimplex field oracle`.

### Slice 5: establish finite hydrologic domains

- Add stable domain/port identities, bounded plan inputs, accounting, canonical
  serialization, cache keys, and failure diagnostics.
- Add Priority-Flood, depression hierarchy, MFD, and D-infinity development
  comparison on synthetic DEMs.
- Prove domain and boundary output is invariant under order and parallelism.

Suggested commit: `feat(worldgen): plan bounded hydrologic domains`.

### Slice 6: ship topology-first rivers and lakes

- Compute effective runoff, channel DAG, basin/lake/outlet records, Strahler
  order, water profiles, and river SDFs.
- Materialize static water with correct levels/flow and no global tick frontier.
- Add river-to-outlet, level, spill, seam, and morphology gates.

Suggested commit: `feat(worldgen): generate connected rivers and level lakes`.

### Slice 7: switch the Terrenia terrain epoch

- Replace additive macro fBm with semantic fields and closed splines.
- Compose territory-owned 3D density around the approved surface/hydrology
  plan.
- Add new provider revision, generation epoch, provenance, and old/new boundary
  adapter evidence.
- Run side-by-side quality and performance acceptance before making the new
  revision the new-world default.

Suggested commit: `feat(terrenia-worldgen): adopt hydrology-constrained terrain`.

### Slice 8: add bounded landscape evolution

- Add the deterministic single-receiver implicit incision subset and hillslope
  diffusion.
- Reroute only through a closed iteration contract and validate final topology.
- Compare disabled/enabled paths and reject regressions outside the accepted
  quality and resource budgets.

Suggested commit: `feat(worldgen): add bounded stream-power erosion`.

### Slice 9: advance the water material

- Add Fresnel, flow normals, depth absorption, and above/below shader behavior.
- Evaluate refraction, OIT, reflection, and caustics separately against target
  GPU capability and performance evidence.
- Add or change a render slot only through the accepted render-contract gate.

Suggested commit: `feat(render): add depth-aware water shading`.

## Release criteria

The new terrain epoch may become the default for newly created worlds only
when all of the following hold:

- deterministic and boundary conformance gates pass on every supported CPU
  target;
- all rivers, lakes, and outlets satisfy the hydrology invariants;
- fixed-seed quality evidence materially improves the current maps without
  introducing spectral grid artifacts;
- optimized generation meets the measured latency, memory, throughput, and
  parallel-scaling budgets;
- old materialized planning cells remain byte-identical and new/old boundaries
  possess valid receipts;
- water above, at, and below the surface is semantically and visually correct;
- cross-chunk runtime fluid work is bounded, deterministic, restart-safe, and
  fault-atomic;
- exact dependency, license, advisory, feature-tree, and benchmark records are
  updated for any adopted upstream code.

## Primary research and implementation references

- Accepted territorial-delegation decision at the reviewed documentation
  revision:
  <https://github.com/rezics/lattice-axiom/blob/57c37e2334ea019521fcd85237cb8dba47d51d24/docs/decisions/0004-territorial-delegation-for-spatial-generation.md>
- Accepted world-generation, epoch, content, and fluid decision:
  <https://github.com/rezics/lattice-axiom/blob/57c37e2334ea019521fcd85237cb8dba47d51d24/docs/decisions/0028-freeze-worldgen-content-and-asset-contract.md>
- Accepted render-capability and provider decision:
  <https://github.com/rezics/lattice-axiom/blob/57c37e2334ea019521fcd85237cb8dba47d51d24/docs/decisions/0029-freeze-render-capability-and-provider-contract.md>
- Composable world-generation architecture:
  <https://github.com/rezics/lattice-axiom/blob/57c37e2334ea019521fcd85237cb8dba47d51d24/docs/platform/world-generation/world-generation.md>

- Genevaux et al., hierarchical drainage networks and procedural terrain:
  <https://perso.liris.cnrs.fr/eric.galin/Articles/2013-river-networks.pdf>
- Cordonnier et al., uplift and stream-power terrain generation:
  <https://doi.org/10.1111/cgf.12820>
- Barnes et al., Priority-Flood:
  <https://doi.org/10.1016/j.cageo.2013.04.024>
- Barnes et al., Fill-Spill-Merge and depression hierarchies:
  <https://doi.org/10.5194/esurf-9-105-2021>
- Prescott et al., 2025 MFD, D-infinity, and IDS comparison:
  <https://doi.org/10.5194/esurf-13-239-2025>
- Tarboton, D-infinity flow direction:
  <https://doi.org/10.1029/96WR03137>
- Braun and Willett, linear-time implicit stream-power solver:
  <https://doi.org/10.1016/j.geomorph.2012.10.008>
- Braun, implicit threshold stream-power update and FastScape limitations:
  <https://doi.org/10.1029/2023JF007140>
- Hillslope diffusion and resolution effects in landscape evolution models:
  <https://doi.org/10.5194/esurf-13-277-2025>
- Minecraft Java 1.18 terrain/aquifer architecture:
  <https://feedback.minecraft.net/hc/en-us/articles/4415128577293-Minecraft-Java-Edition-1-18>
- Bevy 0.19.1 `StandardMaterial`:
  <https://docs.rs/bevy/0.19.1/bevy/prelude/struct.StandardMaterial.html>
- GPU Gems water surface and depth/Fresnel techniques:
  <https://developer.nvidia.com/gpugems/gpugems/part-i-natural-effects/chapter-1-effective-water-simulation-physical-models>
- GPU Gems 2 screen-space refraction:
  <https://developer.nvidia.com/gpugems/gpugems2/part-ii-shading-lighting-and-shadows/chapter-19-generic-refraction-simulation>
- OpenSimplex2 exact reviewed revision:
  <https://github.com/KdotJPG/OpenSimplex2/tree/4cd120d35bfc27096698de90d1bcbf4f9d359a3b>
- NeoTerraForged exact reviewed revision:
  <https://github.com/equalizer32/NeoTerraForged/tree/9a7c782cbf642fc9cc9b86201159a0735a38b388>
- Tectonic exact reviewed revision, design study only:
  <https://github.com/Apollounknowndev/tectonic/tree/34241bdb35acda67b5367d49f354c66c05e098e2>
- BigGlobe V6 restrictive license:
  <https://github.com/Builderb0y/BigGlobe/blob/V6/LICENSE.md>
