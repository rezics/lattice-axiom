# Terrain, vegetation, and water stabilization plan

Status: implementation authorized, 2026-08-31.

This plan turns the current visual defects into versioned, testable world-
generation and rendering contracts. The user authorized autonomous incremental
implementation and commits on 2026-08-31. This is not an accepted architecture
decision; accepted ADRs in the `lattice-axiom` documentation repository take
precedence.

The implementation must follow this document. Material changes to scope,
compatibility, or acceptance gates must be recorded here before the relevant
code change is committed.

## Outcome

The next Terrenia generation revision will provide all of the following:

- grass and every tree root are derived from the final materializable surface,
  never from an earlier height estimate;
- trees have deterministic species and bounded morphology variation rather
  than one template per biome style;
- the procedural water normal map has a complete, vector-aware mip chain so
  distant animated detail does not collapse into temporal shimmer;
- terrain varies at a useful middle scale, includes intentional cliffs and
  two-or-more-voxel ledges, and retains bounded hydrology and density behavior;
- old provider identities and existing materialized snapshots remain
  authoritative and bit-for-bit compatible;
- the new output is covered by fixed-corpus conformance tests, optimized
  benchmarks, and a reproducible visual comparison.

The selected order is support correctness, water stability, vegetation
variety, then terrain morphology. Later slices may rely on the checked surface
contract, while no aesthetic change is allowed to hide an invalid placement.

## Scope and non-goals

In scope:

1. Final-surface discovery and checked vegetation placement.
2. Static grass placement and procedurally assembled tree blueprints.
3. Water normal-map minification and temporal-stability diagnostics.
4. A new Terrenia semantic-terrain algorithm/provider revision.
5. Generation-identity, receipt, golden, property, benchmark, and headless
   rendering evidence required by those changes.

Not in scope:

- changing the meaning of an existing provider revision or rewriting saved
  chunks;
- copying Mojang code, generated assets, or proprietary game data;
- a learned terrain model in the runtime;
- a general L-system or space-colonization engine;
- order-independent transparency or a custom water render pipeline unless an
  A/B diagnostic proves that transparent sorting or coplanar geometry remains
  a separate cause after mip filtering;
- dynamic vegetation growth, seasons, or runtime fluid semantics;
- replacing the hydrology architecture selected in
  [`world-generation-hydrology-and-water.md`](world-generation-hydrology-and-water.md).

## Current failure model

### Vegetation uses an intermediate surface

The generator currently computes a column height, places grass at
`column.height + 1`, and anchors trees to `terrain_height`. Final solidity is a
separate three-dimensional density decision, and cave/material rules are
evaluated later. A height can therefore be a valid terrain intent while the
actual support voxel at that coordinate is empty. The current materializer also
accepts vegetation only above the intermediate height, which makes a simple
downward correction incomplete.

Relevant code:

- [`generation.rs`](../../crates/latticeaxiom-worldgen/src/generation.rs)
- [`natural.rs`](../../crates/latticeaxiom-worldgen/src/natural.rs)
- [`semantic_terrain.rs`](../../crates/latticeaxiom-worldgen/src/semantic_terrain.rs)

### Tree identity and shape are coupled

`NaturalSamplerV1::tree_roles` selects only a log/leaf pair from the surface
style. The materializer then emits one fixed trunk/canopy geometry. Species,
archetype, dimensions, branch structure, collision validation, and placement
fitness are not independent decisions, so even seed variation cannot produce
meaningful silhouettes.

### Water requests mip filtering without providing mips

The 32 by 32 procedural water normal texture contains one mip level even though
its sampler enables mipmap filtering. The shader combines two moving normal
octaves. At distance, many texels project into one pixel but the GPU has no
prefiltered signal to sample, producing horizontal bands and frame-to-frame
shimmer. The captured evidence in `.temp/water-render-evidence/` is consistent
with minification aliasing. The current chunk water mesh does not yet provide
evidence of overlapping coplanar surfaces, so a more invasive transparency
pipeline is not the first intervention.

Relevant code:

- [`water_material.rs`](../../crates/latticeaxiom-engine/src/host/water_material.rs)
- [`water_material.wgsl`](../../crates/latticeaxiom-engine/src/host/water_material.wgsl)
- [`chunk_mesh.rs`](../../crates/latticeaxiom-engine/src/host/chunk_mesh.rs)

### Terrain has a scale gap

The semantic policy has continental, uplift, lithology, climate, local-detail,
and bounded 3D volume signals. The production package currently emphasizes
continent-scale and broad uplift changes, while local detail is small. There is
no explicit middle-scale landform composition or plateau edge contract. This
produces large plains and long one-voxel stair sequences instead of clustered
walkable ground separated by occasional decisive ledges and cliffs.

Relevant code:

- [`terrain_program.rs`](../../crates/latticeaxiom-worldgen/src/terrain_program.rs)
- [`semantic_terrain.rs`](../../crates/latticeaxiom-worldgen/src/semantic_terrain.rs)
- [`packages/terrenia/worldgen/src/lib.rs`](../../packages/terrenia/worldgen/src/lib.rs)

## Research baseline

Research was performed on 2026-08-31. Stable production semantics are based on
Minecraft Java Edition 26.2, released 2026-06-16, rather than a snapshot. The
26.3 snapshot series is used only as evidence that Mojang continues to move
hardcoded feature behavior into configurable data; it is not a compatibility
target.

### Reproducible Mojang report extraction

The following artifacts were kept only under ignored `.temp/reference/` for
study and are not dependencies or distributable project assets:

- Minecraft Java 26.2 official server jar:
  `823e2250d24b3ddac457a60c92a6a941943fcd6a` (SHA-1), 60,894,273 bytes.
- Microsoft OpenJDK `25.0.4.1+1-LTS`, released 2026-08-18:
  `3c9099e60a82e17f5847052f50f3b107bfc1d32b366e1a20adbd1d0f303760e7`
  (SHA-256). The JDK is GPL-2.0 with the Classpath Exception.
- Report command:
  `java -DbundlerMainClass=net.minecraft.data.Main -jar server.jar --all`.

Selected generated-report hashes:

| File | Size | SHA-256 |
| --- | ---: | --- |
| `worldgen/noise_settings/overworld.json` | 119,936 | `a23396a05b33f9189f1351ddc56006d4e9740160cf15c8a5f586834fec15ef69` |
| `worldgen/configured_feature/oak.json` | 1,465 | `0bda9d50cf3a15a04dce1a05d3ffb1b1d4c03b71c5b8f8ec903ed1c330bd6000` |
| `worldgen/configured_feature/fancy_oak.json` | 1,536 | `c662c5c0ceb5bfeb78e6ff1b7fb2fd801ed10f0c614aa1c800bb90f7d1d2f47d` |
| `worldgen/configured_feature/spruce.json` | 1,820 | `7b98788e936d2c43a5b573fad7b91fa65a7f481a16331406a6ff09253155c4ed` |
| `worldgen/placed_feature/fancy_oak_checked.json` | 329 | `c32bc87cc36456e1822e6b6384a3d8674d1b4f235329721058d207baab393c7c` |

The factual observations used by this plan are:

- Overworld generation exposes separate `continents`, `erosion`, `depth`,
  `ridges`, `final_density`, preliminary-surface, and aquifer signals.
- Offset, factor, and jaggedness are composed through nested splines over
  semantic fields. Noise is an input to controllable landform composition,
  rather than every octave being summed into one height.
- `final_density` remains a three-dimensional decision. A preliminary surface
  is not a safe block-placement contract.
- Oak, fancy oak, and spruce use independent trunk, foliage, size, and
  survival/configuration components. A checked placed feature uses a
  `would_survive` predicate rather than assuming that the earlier height is
  suitable.

These observations inform independently implemented contracts only. Mojang's
jar, code, report output, and assets are proprietary and must not be copied into
the repository.

### Terrain literature

The design follows the common separation between macro constraints and local
geometry:

- Génevaux et al., *Terrain Generation Using Procedural Models Based on
  Hydrology* (2013), for drainage-constrained terrain.
- Cordonnier et al., *Large Scale Terrain Generation from Tectonic Uplift and
  Fluvial Erosion* (2016), and Braun and Willett, *A very efficient O(n),
  implicit and parallel method to solve the stream power equation governing
  fluvial incision and landscape evolution* (2013), for semantic uplift and
  erosion rather than octave-only morphology.
- Barnes et al., *Priority-Flood* (2014), for bounded depression handling.
- Peytavie et al., *Arches: a Framework for Modeling Complex Terrains* (2009),
  for combining a surface representation with local volumetric geometry.

The existing hydrology plan already owns most of that long-range work. This
stabilization slice adds a bounded middle-scale relief composition and explicit
cliff observables; it does not create another competing hydrology system.

Recent learned generators were also reviewed. World-GAN (2021), modular voxel
map generation (2021), a 2025 FQVAE Minecraft paper, and InfiniteDiffusion
(SIGGRAPH 2026) are valuable comparators for coherent style and unbounded lazy
generation. InfiniteDiffusion's reference repository was inspected at commit
`e8dcb4b1a834ab2f6b1a6f5256ed7c9f2f3e8230`; its Minecraft integration was
inspected at `23d3f50e5108882bb88a03c3ab048aa63633a02f`. Both are MIT licensed.
They are not selected for the runtime because model weights, GPU-oriented
inference, memory requirements, training provenance, headless CI, and exact
cross-platform determinism do not satisfy this project's current contracts.
They remain candidates for an offline authoring or comparison tool under a
separate decision.

### Tree literature

Weber and Penn's *Creation and Rendering of Realistic Trees* (1995) motivates
separating species parameters from realization. Runions et al.'s
space-colonization method (2007) provides a strong future route for organic
branching. A compact descriptor/blueprint design is selected now because it is
bounded, auditable, chunk-order independent, and sufficient for voxel
silhouette variety. A general growth simulation would add complexity without
solving the immediate placement invariant.

### Rendering references

The renderer is locked to Bevy `0.19.1`, tag commit
`b56fc29d3016e641754765244b5ba3f9cc504671`. Its `Image` descriptor supports
raw mip chains, while `Image::new` validates base-level byte size. Transparent
`StandardMaterial` items use a sorted transparent phase. The initial repair
therefore supplies a valid mip chain through Bevy's image representation and
keeps draw topology unchanged. GPU Gems' water chapter supports summing
multiple normal waves, but such detail still requires a band-limited signal at
distance.

Minecraft 26.2 separately fixed a distant z-fighting defect involving side and
bottom faces of waterlogged blocks. That reinforces the need to distinguish
coplanar/topology flicker from texture minification. This plan requires that
diagnostic distinction rather than assuming every water artifact has one
cause.

## Build-versus-buy record

No new dependency is selected.

| Candidate | Version/revision and status | License and feature/dependency impact | Semantic or operational gap | Decision |
| --- | --- | --- | --- | --- |
| Existing integer semantic fields | Current workspace | Project code; no new dependency, feature, or attack surface | Needs explicit middle-scale and plateau/cliff composition | Keep and extend behind a new revision |
| `noise` | `0.9.0`, released 2024-03-23 | Apache-2.0/MIT; default features are empty; optional `image`/`std` paths and numeric/RNG dependencies | Float behavior and library algorithms do not establish the project's fixed-point determinism or semantic terrain contract; no recent release | Reject |
| `fastnoise-lite` | `1.1.1`, released 2024-03-05 | MIT; default `std`, `num-traits`, optional `f64`/`libm` | Same determinism and semantic-control gap; no recent release | Reject |
| `image` | `0.25.10`, released 2026-03-10 and already transitive in `Cargo.lock` | MIT OR Apache-2.0; default features pull Rayon and many codecs | General image processing is unnecessary for a fixed 32 by 32 RGBA mip reduction; making it direct would broaden the crate contract | Use existing Bevy `Image`; do not add a direct dependency |
| Bevy `Image` and sampler | `0.19.1`, exact lock/tag above | MIT OR Apache-2.0; already required | Raw mip byte layout and normal-aware reduction remain project work | Adopt through a small local bounded implementation |
| InfiniteDiffusion | commits above, active 2026 research/reference | MIT code, but weights/inference add large model, GPU, RAM, and ML dependency costs | Does not meet seed receipt, portable headless CI, latency, memory, or exact deterministic semantics | Reject for runtime; retain as research comparator |
| L-system / space colonization crate | No candidate selected | Would add a general graph/growth dependency | Current need is a small bounded voxel blueprint with project-specific collision and chunk rules | Implement the bounded contract locally |

The two noise crates and `image` were checked through current crates.io package
metadata on 2026-08-31. Because no package is adopted, `Cargo.lock`, transitive
features, and the dependency attack surface do not change. Final advisory and
license checks still run against the existing lock.

## Selected contracts

### 1. Checked final vegetation surface

Introduce a refined internal value such as `VegetationSurfaceV1`. It may be
constructed only by a function that proves, against the same pre-vegetation
occupancy used by materialization, that:

1. `(x, surface_y, z)` is final solid terrain after semantic density and cave
   rules;
2. `(x, surface_y + 1, z)` is empty before vegetation;
3. the column has no surface river or standing-water occupancy at the anchor;
4. the resolved surface style permits the requested vegetation;
5. the search remained inside an explicit bound derived from the terrain
   policy's maximum density displacement.

The constructor returns `None` when no valid surface exists. It does not scan
unbounded vertical space and does not silently fall back to the preliminary
height. The value exposes the support and placement coordinates so call sites
cannot confuse the two integers.

The chunk materializer computes and caches final surfaces for the chunk's
columns. Halo queries needed by a tree footprint are deterministic on-demand
queries through the same function. Vegetation becomes an overlay over proven
pre-vegetation occupancy; the old `y > column.height` guard is removed only
after equivalent final-occupancy validation exists.

### 2. Tree species, archetype, and blueprint

Introduce three distinct concepts:

- `TreeSpeciesV1`: material identity and biome suitability;
- `TreeArchetypeV1`: bounded structural descriptor;
- `TreeBlueprintV1`: checked, sorted, deduplicated voxel roles in world-relative
  coordinates.

The first revision targets at least six visible combinations using existing
oak/pine material roles:

- oak: round, tall, and branched;
- pine: conical, tall, and old-growth.

Descriptors own trunk range, crown start, radius/profile, and optional bounded
branches. Independent canonical hash domains select anchor eligibility,
species, archetype, dimensions, canopy perturbation, and branch direction.
There is no mutable or traversal-order-dependent RNG.

A blueprint is accepted only if its root surface is checked, its slope and
footprint meet the descriptor policy, every target is empty in
pre-vegetation occupancy, and all dimensions stay within declared maxima. The
voxel list is sorted by stable `(y, z, x, role)` ordering and deduplicated
before materialization. Cross-chunk ownership continues to be determined by
the canonical anchor so generation order cannot duplicate or erase trees.

Biome selection remains dominant rather than exclusive: a biome has a primary
species and a bounded minority chance where ecologically allowed. This avoids
one silhouette/material pair covering every tree in a large region without
turning the map into uniform random mixtures.

The Slice 3 implementation contract is frozen before morphology tuning:

| Species/archetype | Trunk height | Crown start | Maximum radius | Branches |
| --- | ---: | ---: | ---: | ---: |
| oak round | 5-6 | relative Y 3 | 2 | 0 |
| oak tall | 7-8 | trunk height minus 3 | 2 | 0 |
| oak branched | 6-7 | relative Y 4 | 3 | 2-3, length 2 |
| pine conical | 6-7 | relative Y 2 | 3 | 0 |
| pine tall | 8-9 | relative Y 4 | 2 | 0 |
| pine old-growth | 8-9 | relative Y 3 | 3 | 3-4, length 2 |

Canopy tips may extend one voxel above the trunk, so the closed implementation
limits are height 10, horizontal radius 3, four branches, and 512 unique voxels
per blueprint. Natural-layer validation must reserve that full headroom and an
exclusion radius of at least six voxels, twice the maximum crown radius. This
makes accepted blueprint extents disjoint even at chunk boundaries; the
existing canonical `(rank, x, z)` anchor priority resolves competing anchors
before either blueprint is emitted. Logs win over leaves only while
deduplicating one blueprint, never as an order-dependent collision repair.
Diagonal branches use a deterministic staircase so every log remains
six-neighbor connected to the root.

Temperate woodland selects oak with weight 7/8 and pine with weight 1/8;
boreal wetland applies the inverse weights. Each species selects its three
archetypes uniformly. Eligibility density is unchanged except for the
provider-revision boundary. The independent domains are named
`tree-eligibility`, `tree-species`, `tree-archetype`, `tree-dimensions`,
`tree-canopy`, `tree-branch`, and `tree-priority`; adding a field must not reuse
bits from another decision.

The pre-Slice-3 baseline emits the same four-voxel trunk and radius-two crown
for both materials and therefore has exactly one normalized occupancy
signature. The fixed morphology corpus must emit all six archetypes, both
species, and at least five normalized signatures while staying within the
limits above.

### 3. Vector-aware water mip chain

Generate all six mip levels for the 32 by 32 RGBA8 normal texture:

`32, 16, 8, 4, 2, 1`.

The expected byte count is:

`4 * (32^2 + 16^2 + 8^2 + 4^2 + 2^2 + 1^2) = 5,460`.

For each 2 by 2 reduction, decode child normals, average the vectors, and store
the unnormalized mean direction. The mean length is normal coherence and is
encoded in alpha. The shader normalizes the direction used for lighting and
attenuates that octave's perturbation by coherence. This prevents incoherent
high-frequency waves from regaining full strength in coarse mips.

The image descriptor declares six mip levels. The sampler is linear for
minification, magnification, and mip interpolation and requests 16-times
anisotropy for the grazing-angle water plane. This is the maximum accepted by
the locked `wgpu 29.0.4`; its downlevel path deterministically falls back to
one when anisotropic filtering is unsupported. Tests prove byte count,
descriptor consistency, valid forward-facing normals, deterministic output,
and non-increasing coarse-level horizontal energy. Roughness or wave strength
may change only if the fixed camera A/B evidence shows that the mip repair
alone is insufficient; any such adjustment is recorded in this plan first.

### 4. Versioned middle-scale terrain composition

Existing semantic terrain algorithms and provider IDs remain unchanged. Add a
new explicit algorithm revision and new Terrenia provider identities. The new
field retains separate continentalness, uplift, lithology, climate, detail,
and 3D volume signals while adding:

- a middle-scale relief field between broad uplift and local detail;
- spline-mapped regional relief amplitude rather than raw additive noise;
- a bounded terrace/plateau signal whose sharp transition can create
  two-or-more-voxel ledges;
- a cliff gate informed by uplift, lithology, and the plateau transition,
  rather than uplift alone;
- local 3D density expression near cliff regions while preserving the global
  displacement bound used by final-surface discovery.

The initial middle-scale wavelength is derived from the package terrain scale
and is expected near 512 voxels for the balanced preset, subject to corpus
metrics. It is neither another continent field nor per-block noise. Plateau
height and coverage reuse meaningful preset controls instead of introducing
unbounded magic values.

Old `@1`/algorithm 12 and `@2`/algorithm 13 behavior receives characterization
tests before the new default moves to `@3`/algorithm 14. The host worldgen plan
revision and affected generation/materializer/vegetation provider revisions
must increase together. Existing snapshots remain authoritative; there is no
in-place reinterpretation or migration.

The Slice 4 field contract is frozen before corpus thresholds are measured:

- `@2` retains a `None` morphology extension which is omitted from canonical
  serialization; its policy hash, samples, and density output must remain byte
  identical.
- `@3` adds independent `middle-relief.v1` and `plateau.v1` fixed-field domains.
  Their wavelength is `max(mountain_scale / 4, 256)` voxels, which is exactly
  512 voxels for the balanced preset. Both retain amplitude 1024. Middle
  relief is the bounded spectral sum of base weight 1 and octave weights
  `3/4, 1/2, 1/4, 1/5` at `1/2, 1/4, 1/8, 1/16` wavelengths. Each octave has
  an explicit seed subdomain, the sum is normalized by the complete spectral
  weight, and the final control is clamped to `[-1024, 1024]`.
- coherent middle-relief and plateau controls use the monotone signed
  ease-out `sign(x) * abs(x) * (2048 - abs(x)) / 1024`. This retains zero and
  both endpoints while preventing the central concentration of coherent noise
  from making package coverage controls much rarer than authored. Plateau
  and the multi-octave middle control each apply it once before their authored
  splines, still inside the same closed amplitude bound.
- middle relief multiplies the field value by `hill_height_voxels`, coast mask,
  the erosion retention `(2048 - erosion_strength) / 2048`, and the closed
  uplift-amplitude spline `(-1024,384), (-256,640), (0,960), (384,1440),
  (768,2048), (1024,2048)`.
- plateau coverage converts `plateau_amount_per_1024` to threshold
  `1024 - 2 * amount`. A closed 64-unit half-transition maps the independent
  plateau field continuously from zero to `plateau_height_voxels`. Lithology
  blends a smooth profile with a sharpened smoothstep profile; no fixed equal
  height levels are emitted.
- the plateau-transition signal is zero in plateau interiors and exteriors and
  peaks at the edge. The 3D cliff gate combines two parts uplift tendency, one
  part lithology resistance from spline `(-1024,128), (-256,256), (256,640),
  (768,1024), (1024,1024)`, and two parts plateau transition. Positive volume
  remains suppressed by the existing water-protection contract.
- middle and plateau displacement apply only to semantic land before the
  existing world-height clamp and are multiplied by the same continuous coast
  ownership weight as the macro land contribution. All arithmetic remains
  integer/fixed-point, every new value is canonical policy data, and the
  density displacement envelope remains the existing eight voxels because
  only the gate—not the 3D volume maxima—changes.

#### Visual-review correction: coast discontinuity and equal terraces

The first `@3` candidate did not pass visual review and must not ship. A fixed
seed capture supplied on 2026-08-31 is retained as the ignored artifact
`.temp/terrain-morphology-evidence/user-shoreline-feedback.png`, SHA-256
`6e692d1bad5feaee34ee050cde2c83c22c18f3accfeaa8ed2aaaa27b2c00fe2b`.
It shows that the visually dominant relief is a shoreline wall followed by
nearly equal two-to-three-voxel terrace bands, rather than independently placed
inland landforms.

The defect is structural, not a missing wavelength:

- the land branch adds the balanced preset's 22-voxel base height as soon as
  continentalness changes sign, while the ocean branch approaches sea level;
- the independent 176-voxel mountain term and 36-voxel plateau term are also
  admitted only on the land side, so a high uplift or plateau sample can become
  an accidental coastal wall;
- the middle-relief field is multiplied by the coast mask and therefore
  disappears exactly where the discontinuous macro terms dominate;
- the plateau transition is projected onto 16 equal levels. A 36-voxel plateau
  therefore requests 2.25-voxel increments by construction; this mistook
  "occasional unwalkable ledges" for "every plateau edge is a staircase";
- the original four adjacency windows happen to contain three all-land windows
  and one all-ocean window. Their land/land metrics contain zero shoreline
  edges, so the acceptance corpus could not detect this failure.

Six newly frozen 192-square shoreline windows start at `(-15008,-16480)`,
`(-9248,-12384)`, `(1632,-8288)`, `(-5024,-4192)`, `(9184,-96)`, and
`(16224,4000)`. They contain 1,553 identical coast crossings in `@2` and the
rejected `@3` candidate. The legacy branch has a maximum adjacent shoreline
jump of 136 voxels and a summed jump of 120,969; rejected `@3` regresses those
to 171 and 138,012. All 13,369 delta-at-least-two land edges in the shoreline
windows are plateau-transition edges, of which 10,387 are exactly two voxels.
In the original land corpus, 11,940 of 14,648 such edges (81.5%) also lie in
the quantized plateau transition. The middle field is exactly saturated at
`+/-1024` in 4,083 of 147,456 original columns (27,689 per million), confirming
that division by the base weight rather than the sum of spectral weights also
turns a large fraction of fBm into clipped mesas.

The corrected `@3` contract is frozen before its output is inspected:

- `@2` remains byte-for-byte unchanged. Only the uncommitted `@3` algorithm may
  change.
- The land macro contribution is multiplied by a monotone smoothstep coast
  weight that is zero at continentalness zero and one at the configured coast
  width. Base height, continental lift, uplift, middle relief, and plateau
  displacement therefore join the ocean branch continuously instead of
  switching at a sign test. Rare coastal cliffs require a future independent
  authored selector; they are not an accidental side effect of land ownership.
- Middle-relief octave weights are normalized by their sum, not by the base
  weight, and use one signed ease rather than two. This preserves the existing
  independent domains and wavelengths without clipping broad positive and
  negative regions into mesas.
- Plateau displacement is continuous. A smooth profile and a
  lithology-controlled sharpened profile are blended without fixed equal
  levels. The plateau edge may still become locally unwalkable, but it is one
  spatially varying scarp rather than 16 concentric ledges.
- The new shoreline corpus must retain 1,553 crossings, keep p95 adjacent
  shoreline jump at most two voxels and maximum jump at most 12, and keep fewer
  than five percent of crossings at delta at least two. Middle-relief saturation
  must stay below 10,000 columns per million. No more than two thirds of the
  original land corpus's delta-at-least-two edges may be attributed to plateau
  transitions. The earlier relief, walkability, flat-component, height-bound,
  hydrology, and density-displacement gates continue to apply.

This correction matches the current Minecraft 26.2 generated report more
closely: its `overworld/offset` is a continuous spline over continents with
nested erosion and folded-ridge splines; `depth` combines that offset with a Y
gradient, while factor and jaggedness remain separate controls in
`sloped_cheese`. It does not use a binary land-sign height addition. Minecraft
26.3 Snapshot 10 is not a compatibility target, but its addition of named
density debug functions and explicit domain-warp inputs reinforces making
control-field contributions observable. Tectonic 3.0.25 was inspected at
commit `34241bdb35acda67b5367d49f354c66c05e098e2` (2026-06-21); it likewise
separates continental offset, factor, and jaggedness paths, and explicitly
documents staircasing as a tradeoff of its smoothness modes. It declares no
repository license, so it remains study-only and no code or data is copied.
Grenier et al., *Real-time Terrain Enhancement with Controlled Procedural
Patterns* (Computer Graphics Forum 43(1), 2024, DOI
`10.1111/cgf.14992`) further supports applying spatially varying detail through
control maps consistent with the underlying terrain instead of repeating one
global contour pattern.

#### Visual-review correction: final-surface material depth

The corrected morphology passed the next visual review, but the fixed-seed
capture supplied on 2026-08-31 exposed an older material-layer defect. The
ignored evidence artifact is
`.temp/terrain-morphology-evidence/user-surface-distance-feedback.png`, SHA-256
`a5bf36e36223a77c66f3dc965c0e7cda53ce61c8c82ce967a1d304cb27f6218f`.
Large horizontal dirt patches and repeated dirt bands are visible even though
the intended biome surface is temperate grass.

The existing geology path computes material depth from the preliminary
two-dimensional `column.height`. The current terrain revision can move the
actual uppermost solid voxel below that intent through its bounded 3D density
field. A final top voxel two blocks below the preliminary height is therefore
misclassified as depth two and receives subsurface dirt rather than the
surface role. Independently, a fixed three-voxel soil layer exposes all three
dirt rows on a two-or-more-voxel side drop. Both failures became much more
visible once middle-scale relief produced more real slopes.

The initial correction proposed a versioned surface rule owned by the new
geology provider revision:

- determine the uppermost final solid voxel after density and cave arbitration
  with the same bounded search used by checked vegetation placement;
- compute surface-material depth from that final solid Y, never from the
  preliminary height intent;
- measure the final four-neighbor surface descent once per column, including a
  deterministic one-column chunk halo;
- keep the normal three-voxel temperate soil profile on flat and one-step
  terrain, but use grass directly over temperate base rock when any neighboring
  final surface is at least two voxels lower;
- leave old geology revision 2 byte-identical and bind the correction to
  geology revision 3, its implementation fingerprint, and the new generation
  identity;
- add material conformance that counts exposed temperate dirt faces on a fixed
  morphology corpus and proves that every dry temperate final top receives the
  surface role.

Minecraft Java 26.2's generated Overworld noise settings provide the current
reference model: surface composition is a separate rule tree over final stone
occupancy, includes explicit `stone_depth` conditions, and contains `steep`
conditions that replace exposed slope materials in applicable terrain. The
project does not copy Mojang data or special-case its biomes; it adopts only
the general final-occupancy and slope-aware material separation.

#### Research correction: variable sediment depth

The fixed three-voxel bullet above is rejected after follow-up review. Three
voxels can be one valid local outcome, but a global constant is not a credible
soil/bedrock boundary and makes every two- or three-voxel terrace reveal the
same repeated dirt stripe. The replacement is a bounded, deterministic
surface-sediment profile; old geology revisions retain the historical constant
profile byte-for-byte.

The implementation decision is grounded in the following current/reference
work:

- Minecraft Java 26.2 (data pack version 107.1) is the current compatibility
  reference inspected on 2026-08-31. Its generated Overworld surface-rule tree
  repeatedly combines `minecraft:stone_depth` with explicit
  `minecraft:steep` predicates. The public surface-rule description records a
  noise-varying surface depth rather than one constant thickness. Sources:
  <https://www.minecraft.net/en-us/article/minecraft-java-edition-26-2>, the
  locally generated 26.2 report at
  `.temp/reference/minecraft-java-26.2/generated-all/generated/data/minecraft/worldgen/noise_settings/overworld.json`,
  and <https://minecraft.wiki/w/Surface_rule>. Mojang code and data remain
  reference-only and are not copied.
- Luanti revision `bd2bda63889fd985d40acbda5750ea8ed83a3a09`
  (2026-08-31, LGPL-2.1-or-later) computes filler depth as biome top depth plus
  biome filler depth plus `noise_filler_depth`; Mapgen V7 uses a three-octave
  field with a 150-node spread. Source:
  <https://github.com/luanti-org/luanti/blob/bd2bda63889fd985d40acbda5750ea8ed83a3a09/src/mapgen/mapgen.cpp#L697-L698>
  and `mapgen_v7.cpp`. This is evidence for coherent spatial variation, not a
  dependency or copied implementation.
- Veloren revision `483b0822fdb8a75718a13d5b0ff591471ee18098`
  (2026-08-29, GPL-3.0) explicitly stores terrain altitude and bedrock
  `basement`, treats their difference as sediment thickness, applies different
  bedrock/sediment transport, and suppresses tree growth directly on bedrock.
  Its erosion implementation cites Cordonnier et al. and Dietrich et al.
  Source: <https://gitlab.com/veloren/veloren/-/tree/483b0822fdb8a75718a13d5b0ff591471ee18098/world/src>.
  The copyleft implementation is study-only; no code is copied.
- Heimsath, Dietrich, Nishiizumi, and Finkel, *The soil production function and
  landscape equilibrium* (Nature 388, 1997, DOI `10.1038/41056`) provides field
  evidence that soil depth and hillslope curvature are inversely related and
  that soil production declines exponentially with soil thickness. The more
  recent state of terrain synthesis includes process-based sediment transport
  and erosion, including Yang et al., *Unerosion* (Computer Graphics Forum,
  2024, DOI `10.1111/cgf.15182`) and *Stochastic geomorphological transport for
  terrain erosion simulation* (ACM TOG, 2026, DOI `10.1145/3811336`). A full
  evolution solve is inappropriate in this bounded chunk materialization
  slice, but the explicit sediment/bedrock state is retained as the future
  seam.

The accepted geology revision 3 contract is therefore:

1. Find the final uppermost solid voxel after semantic density and cave
   arbitration. Material depth, resource depth, and vegetation support all use
   this same typed final-surface value.
2. Sample one independent fixed-point OpenSimplex2S soil field at a 128-voxel
   scale. Quantize it to a base subsurface depth of one through four voxels;
   this produces coherent patches without adding a dependency or per-voxel
   work.
3. Apply at most one voxel of climate correction: high precipitation plus
   infiltration deepens the profile, while aridity or high effective runoff
   thins it. Apply at most one voxel of topographic correction: a four-neighbor
   concavity deepens the profile and convex exposure thins it. Clamp the flat
   profile to `0..=5` subsurface voxels.
4. Apply the final-surface exposure rule after those broad controls. A one-
   voxel descent caps subsurface soil at one voxel; a two-voxel descent keeps a
   vegetated surface skin directly over rock; a descent of three or more emits
   exposed base rock at the final top and has no soil profile.
5. Resource and intrusion replacement cannot enter the authored sediment
   profile. Vegetation accepts only a typed vegetated surface profile, so bare
   rock cannot become a tree or ground-cover anchor.
6. Compute the four-neighbor profile with a deterministic one-column halo and
   cache it once per generated column. No surface-rule sampling occurs in the
   inner per-voxel loop.

This is deliberately a bounded geomorphic approximation, not a claim to
simulate pedogenesis. It adopts the shared best-practice structure—coherent
depth variation, an explicit sediment/bedrock boundary, final-topography
exposure, environmental controls, and vegetation/material agreement—while
keeping the authoritative algorithm integer-only and generation-order
independent.

#### Second visual-review correction: excessive generic rock caps

The variable-sediment candidate substantially reduced exposed dirt, but the
2026-08-31 visual review rejected its remaining generic rock exposure. The
cause is the last part of item 4 above: one neighboring descent of three or
more voxels turns the entire column's uppermost temperate or boreal surface
into bare base rock. Middle-scale cliffs make that predicate common, and a
single low neighbor is enough even when the column itself is a broad grassy
cliff top. The rule therefore paints repeated rock caps rather than revealing
rock only on actual vertical faces.

The current Minecraft Java 26.2 generated Overworld surface rules do not
support applying `steep => top rock` globally. The inspected report contains
five `minecraft:steep` conditions, all scoped within frozen peaks, snowy
slopes, or jagged peaks branches. Temperate terrain retains biome-owned surface
rules. The local report is
`.temp/reference/minecraft-java-26.2/generated-all/generated/data/minecraft/worldgen/noise_settings/overworld.json`;
the corresponding current release is
<https://www.minecraft.net/en-us/article/minecraft-java-edition-26-2>.

Veloren revision `483b0822fdb8a75718a13d5b0ff591471ee18098`
(GPL-3.0, study-only) likewise models altitude and basement separately and
keeps a surface material above the rock boundary. A shallow cap exposes rock
on a cliff side without requiring every steep column's top voxel to become
rock. Source:
<https://gitlab.com/veloren/veloren/-/blob/483b0822fdb8a75718a13d5b0ff591471ee18098/world/src/block.rs>.

The revised, still-uncommitted geology revision 3 candidate is:

1. Preserve the coherent `0..=5` flat subsurface depth and the final-surface
   ownership established above.
2. A one-voxel maximum neighbor descent caps subsurface soil at one voxel.
3. A descent of two or more keeps the biome's vegetated top voxel but sets
   subsurface soil depth to zero. Rock is then visible immediately below the
   cap on the vertical face, while the cliff top remains grass-covered.
4. Remove the generic temperate/boreal `exposed_base_rock` top replacement.
   Rare rocky summits or outcrops require a future coherent biome, lithology,
   altitude, and/or erosion-exposure rule. They must not be inferred from one
   four-neighbor height delta.
5. Keep vegetation support typed: the severe-footprint-relief rule may still
   reject trees on unsafe ledges even though the material top remains a
   vegetated biome role. Material and placement eligibility are related but
   not the same boolean.

This correction updates the current revision 3 candidate and its provider
fingerprint before it is committed; it does not introduce revision 4. Any
development snapshots produced by the rejected candidate must miss the new
identity and be regenerated rather than silently reused.

Additional acceptance gates:

- every dry temperate and boreal final top in the fixed corpus has its
  biome-owned vegetated surface role; generic bare-rock top count is zero;
- columns with neighbor descent two or more have no exposed dirt stripe and
  place the surface cap directly over base rock;
- the flat-land corpus still emits at least three distinct sediment depths;
- diagnostics count horizontal bare-rock tops separately from vertical
  exposed rock faces, preventing one metric from hiding the other;
- tree footprint-relief, final support, hydrology, chunk-border, permutation,
  and optimized natural-chunk performance gates remain unchanged;
- any future rocky-outcrop feature needs its own coherent field/biome contract,
  fixed visual threshold, provider identity, and approval.

## Fixed-corpus quality gates

All metrics use canonical seed/coordinate corpora checked into tests as small
constants. Thresholds compare the legacy semantic revision with the new one
over identical samples and are fixed before aesthetic tuning is accepted.

### Placement correctness

- Every emitted grass voxel has final solid support immediately below it and
  was empty in pre-vegetation occupancy.
- Every emitted tree has a checked final root surface; no trunk, branch, or leaf
  target intersects pre-vegetation solid or water.
- Density-depressed and near-cave fixtures either move to the valid final
  surface or emit no vegetation.
- The invariants hold at negative coordinates, chunk edges, and for multiple
  chunk generation orders.

### Tree diversity

- The fixed corpus emits both material species and all six target archetypes.
- At least five distinct normalized occupancy signatures occur; material-only
  differences do not count as shape diversity.
- Every blueprint respects the declared maximum radius and height.
- Same seed, anchor, provider revision, and context produce identical sorted
  roles; a changed generation order does not change the result.

### Terrain morphology

Record at minimum:

- adjacent-column height deltas and the fraction with delta at least two;
- maximum and percentile flat-run lengths along both horizontal axes;
- local relief in 64, 256, and 1,024 voxel windows;
- walkable-neighbor fraction, exposed cliff-face count, and absolute height
  bounds;
- river/lake continuity and the semantic density displacement bound.

The balanced preset must show a material increase in middle-scale relief and
delta-at-least-two edges while preserving connected walkable regions. Exact
numeric thresholds are frozen in the slice's first red tests after printing
the legacy corpus distribution; they must not be chosen after inspecting only
the new output.

The pre-implementation balanced `@2` corpus contains 220,032 land/land edges:
zero have adjacent delta at least two, maximum delta is one, walkable-neighbor
fraction is 1,000,000 per million, and exposed cliff faces are zero. Maximum
equal-height runs are 192 on X and 136 on Z; p95 runs are 13 and 9. Its
four-neighbor two-dimensional flat-interior fraction is 609,167 per million,
with a largest flat component of 19,935 cells spanning 190 voxels. Median
sampled local relief for 64/256/1,024-voxel windows is 2/7/92 voxels, with land
heights 93..=224. Before observing `@3`, its acceptance bounds are frozen as:

- the same 220,032 land edges, because continental ownership is unchanged;
- 2,500..=75,000 delta-at-least-two edges per million, maximum adjacent delta
  in 2..=12, at least 550 exposed cliff faces, and at least 925,000 walkable
  neighbors per million;
- maximum equal-height runs at most 128 on both axes; one-dimensional p95 runs
  remain diagnostic only because a one-cell-wide contour can be long without
  representing a flat plain;
- at most 450,000 four-neighbor flat-interior cells per million land columns,
  excluding authored plateau interiors, with no connected flat component above
  8,000 cells and component span below the legacy 190-voxel maximum; component
  area is authoritative because a long, narrow flat valley is not a broad
  plain;
- median local relief at least 3/12/80 voxels for 64/256/1,024 windows and at
  most 160 voxels for the 1,024 window;
- every approved height remains within the configured world bounds and the
  existing 16-voxel ceiling reserve.

### Water stability

- Unit tests validate the mip contract and coarse normal energy.
- A deterministic headless or scripted camera capture compares identical
  distant-water views at adjacent animation times.
- The temporal-difference metric over stable geometry must improve relative to
  the one-mip baseline without flattening the near-field normal signal.
- A mesh diagnostic confirms no duplicate coplanar top surfaces in the tested
  chunk seam corpus. If duplicates are found, that becomes a separate topology
  slice; it is not masked by shader tuning.

## Performance gates

Pre-change optimized baseline on the local Windows 11, Intel Core i9-14900HX,
RTX 4080 Laptop environment:

| Benchmark | Baseline, 2026-08-31 |
| --- | ---: |
| `v5_production_boreal_chunk_32_cubic_snapshot_candidate` | 11.537-11.647 ms (median estimate 11.592 ms) |
| semantic terrain column sampling | 426.86-431.22 ns |
| semantic density, 32 cubed | 15.220-15.340 ms |
| Slice 1 checked-surface natural chunk | 11.986-12.077 ms (median estimate 12.031 ms, +3.79%) |
| Slice 3 checked tree-blueprint natural chunk | 12.438-12.614 ms, 12.729-12.834 ms, and final 12.377-12.513 ms (median estimates 12.510/12.772/12.437 ms; worst median +10.18% from the original baseline and +6.16% from Slice 1) |
| Slice 4 natural chunk after retained-column controls | 13.191-13.372 ms (median estimate 13.270 ms, +14.48% from the original baseline and below 15 ms) |
| Slice 4 provider `@2` semantic column / density | 441.12-444.14 ns / 4.107-4.206 ms |
| Slice 4 provider `@3` semantic column / density | 972.49-1,001.8 ns / 4.504-4.588 ms |

Gates:

- terrain column and density benchmarks may not regress by more than 15%
  without a documented profile and a compensating budget decision;
- the production natural 32-cubic chunk benchmark may not regress by more than
  25%, and should remain below 15 ms on the baseline machine;
- water mip construction is startup-only, deterministic, and bounded to 5,460
  bytes; it must not add per-frame allocation or CPU work;
- final-surface caching must keep repeated per-voxel vertical searches out of
  the materialization hot loop;
- all scale-sensitive generation remains eligible for Bevy task-pool and
  scheduler parallelism; no global lock or project-owned thread runtime is
  introduced.

Criterion noise is evaluated from intervals and repeated runs, not one point
estimate. If a gate fails, profile and optimize within the current slice before
the commit.

The Slice 4 isolated `@3` column interval is 2.29 times the original `@2`
median and therefore invokes the documented compensating-budget path rather
than passing the 15% isolated-column gate. The measured cause is the accepted
five-octave middle-relief spectrum plus the independent plateau control; these
are the signals that supply the user-approved middle-scale morphology and
cannot be removed without reverting the feature. The implementation instead
retains one typed semantic sample per materialized column and reuses its height,
cliff, climate, and runoff controls. Density controls are prepared once per
vertical column, not once per voxel. The resulting 32-cubic density interval is
4.504-4.588 ms, 70% below the original 15.220-15.340 ms measurement, while the
end-to-end natural chunk remains 13.191-13.372 ms, 14.48% above the original
baseline, below the 25% gate, and below 15 ms. This is the compensating budget
decision: retain the visually material field cost only while fused column and
chunk budgets remain inside their stronger end-to-end limits.

## Incremental implementation and commit protocol

Each slice starts with failing characterization/conformance evidence, ends with
the listed focused and workspace checks, updates the progress record below, and
is committed separately using Conventional Commits. Refactors that do not
change behavior remain separate from provider behavior changes when needed.

### Slice 0: plan and evidence

Deliverables:

- this plan, including authoritative sources, versions/revisions, dependency
  decisions, compatibility rules, baselines, and acceptance gates;
- ignored reproducibility artifacts only under `.temp/reference/`.

Checks: Markdown/path review and `git diff --check`.

Commit: `docs(worldgen): plan terrain and vegetation stabilization`.

### Slice 1: final-surface contract

Deliverables:

- one shared pre-vegetation occupancy path;
- bounded checked final-surface discovery and per-chunk surface cache;
- grass and legacy tree anchoring migrated to the checked value;
- removal of intermediate-height assumptions only where final occupancy proves
  the replacement;
- property and cross-chunk conformance tests;
- affected provider/materializer revision bump without changing old identities.

Checks:

- focused `latticeaxiom-worldgen` unit/integration tests;
- `cargo fmt --check`;
- `cargo clippy -p latticeaxiom-worldgen --all-targets --all-features -- -D warnings`;
- production natural chunk benchmark against baseline.

Commit: `fix(worldgen): anchor vegetation to final surfaces`.

### Slice 2: water temporal stability

Deliverables:

- complete vector-aware mip generation;
- coherence-aware shader sampling;
- image, shader, mesh-topology, and temporal-capture tests;
- updated water material conformance evidence.

Checks:

- focused engine tests in headless mode;
- shader validation through the project's existing asset/pipeline checks;
- `cargo fmt --check` and affected-crate Clippy;
- deterministic adjacent-frame capture and metric.

Commit: `fix(render): stabilize distant water normals`.

### Slice 3: data-driven tree morphology

Deliverables:

- species, archetype, descriptor, and checked blueprint types;
- six bounded combinations using existing materials;
- independent deterministic hash domains;
- complete footprint/headroom/collision validation;
- diversity, bounds, determinism, and chunk-order tests;
- updated provider identities and receipts.

Checks:

- focused worldgen conformance and property tests;
- `cargo fmt --check` and affected-crate Clippy;
- production natural chunk benchmark and fixed-corpus signature report.

Commit: `feat(worldgen): add deterministic tree archetypes`.

### Slice 4: middle-scale terrain and cliffs

Deliverables:

- legacy terrain characterization/golden tests;
- explicit new semantic algorithm revision and Terrenia `@3` providers;
- middle-scale relief, spline/terrace composition, and expanded cliff gate;
- host generation plan/provider revision update;
- terrain morphology corpus, density-bound, hydrology, determinism, and
  negative-coordinate tests;
- package/engine integration fixtures updated only for the new identity.

Checks:

- focused `latticeaxiom-worldgen`, `latticeaxiom-terrenia-worldgen`, and engine
  tests;
- `cargo fmt --check` and affected-crate Clippy;
- both optimized semantic-terrain benchmarks and production natural chunk
  benchmark;
- fixed-seed visual flyover/captures at old and new revisions.

Commit: `feat(worldgen): add midscale relief and cliffs`.

### Slice 5: integrated acceptance

Deliverables:

- final metrics and exact commands appended to this document;
- water conformance document synchronized with shipped behavior;
- no stale test names or fixtures claiming an older production revision;
- release/rollback notes for the new generation identity.

Checks:

- `cargo test --workspace --all-targets --all-features`;
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`;
- `cargo fmt --all --check`;
- `cargo deny check advisories` and the repository's license/source checks when
  available;
- optimized benchmark reruns and final visual evidence.

Commit: `docs(worldgen): record stabilization conformance` if evidence-only
changes remain. Code fixes discovered here receive their own scoped commit.

## Compatibility, rollout, and rollback

- Never mutate the output of an existing algorithm/provider revision.
- New morphology and terrain are selected only through new stable provider IDs,
  fingerprints, algorithm revisions, and a new host plan revision.
- Existing persisted chunks/snapshots remain authoritative. Regeneration under
  the new plan creates a new world-generation identity; there is no mixed silent
  upgrade.
- Receipt and closed-world provider tests must reject a reused revision with a
  changed fingerprint.
- Rollback means selecting the previous complete provider set and plan revision,
  not reverting serialized meaning. Water rendering can be rolled back
  independently because it does not alter world data.

## Risks and controls

| Risk | Control |
| --- | --- |
| Final-surface search multiplies density/cave sampling cost | Derive a small explicit displacement bound, cache local columns, query only bounded halo anchors, benchmark before commit |
| Tree canopies collide across anchor/chunk boundaries | Canonical anchor ownership, full pre-vegetation footprint validation, stable priority for overlapping blueprints, permutation tests |
| More variation becomes visual noise | Biome-weighted species, bounded archetype descriptors, separate hash domains, fixed-corpus distribution assertions |
| Plateau quantization recreates artificial contour bands | Apply terraces regionally through semantic gates and transition splines; measure long contour runs and axis bias |
| Cliffs destroy traversal | Track connected walkable fraction and cluster cliffs rather than maximizing every adjacent delta |
| Mips flatten all water detail | Preserve base mip exactly; encode coherence per level; compare near-field energy and distant temporal error |
| Flicker is actually overlapping geometry or transparent sorting | Keep a seam/topology diagnostic and adjacent-camera A/B; escalate to a separate render-path slice only with evidence |
| New behavior contaminates old saves | Characterize old providers first and require new provider/plan identities |

## Progress and evidence log

- [x] 2026-08-31: repository constraints, branch state, affected contracts, and
  locked toolchain inspected.
- [x] 2026-08-31: Minecraft Java 26.2 official reports generated with verified
  jar/JDK hashes; 26.3 preview direction reviewed without adopting snapshot
  semantics.
- [x] 2026-08-31: terrain, tree, water, Bevy, and current learned-generation
  references surveyed; build-versus-buy decision recorded.
- [x] 2026-08-31: pre-change optimized benchmarks recorded.
- [x] 2026-08-31: Slice 0 plan approved for its dedicated commit.
- [x] 2026-08-31: Slice 1 final-surface contract implemented, verified, and
  approved for its dedicated commit. The production semantic fixture found
  vegetation on columns where final density differed from height intent and
  proved solid support, empty placement, and no hydrology overlap. Worldgen,
  Terrenia, and engine-worldgen suites passed; strict affected-crate Clippy and
  rustfmt passed. The optimized natural-chunk interval was 11.986-12.077 ms
  (+3.79% median estimate), within both performance gates.
- [x] 2026-08-31: Slice 2 water temporal stability implemented and approved
  for its dedicated commit. The deterministic normal image now contains the
  complete 5,460-byte vector mip chain, coarse coherence attenuates canceled
  detail, and the grazing plane uses a 16-times anisotropic sampler with the
  locked wgpu fallback. A temporary exact one-mip runtime baseline and the
  restored production path each captured nine frames over the same fixed
  water view. Across eight adjacent pairs (mean intervals 177.991 ms and
  176.903 ms), production reduced ROI mean RGB-channel MAE from 0.1368725 to
  0.0962664 (-29.67%), median MAE by 31.14%, and changing-pixel fraction by
  25.94%. The shared-halo mesh fixture also proved matching seam vertices and
  normals without coplanar overlap; no transparency-pipeline change was
  justified. The final executable test boundary passed 162 engine unit tests,
  54 headless-host tests, the remaining engine integration/doc tests (one
  pre-existing explicitly ignored journey), 32 voxel-mesh unit tests, four
  upstream-audit tests, and voxel-mesh doc tests. Strict all-target/all-feature
  Clippy, rustfmt, and diff checks passed. `cargo test --all-targets` is not an
  executable gate because the repository's parameterized
  `performance_evidence` bench intentionally rejects invocation without its
  required `--output`; Clippy still compiled that target. The full gate also
  exposed a latent cave fixture that loaded only one chunk of a four-chunk
  shaft; commit `804316c` made that fixture await every touched chunk under a
  finite contended-run bound before this render commit.
- [x] 2026-08-31: Slice 3 deterministic tree morphology implemented and
  approved for its dedicated commit. Seven independent hash domains now own
  eligibility, species, archetype, dimensions, canopy perturbation, branch
  direction, and anchor priority. The checked blueprint boundary sorts and
  deduplicates complete tree voxels, preserves log-over-leaf structure, and
  rejects any target intersecting final terrain, caves, world bounds, or
  hydrology occupancy before materialization. The fixed corpus emits oak and
  pine, all six archetypes, and six distinct material-independent occupancy
  signatures; its 8,192-coordinate species corpus proves dominant but
  non-exclusive biome weighting. Height, radius, branch, voxel-count,
  headroom, and non-overlap bounds are closed and tested. Cross-biome anchor
  exclusion now samples each neighbor's own style, accepted blueprints are
  emitted in canonical priority order, and existing chunk/offer permutation
  tests remain byte-identical. Production plan revision 6 binds coordinator
  and materializer revision 10 plus vegetation revision 4; cave, style,
  terrain-transition, hydrology, geology, and resource identities remain on
  their prior revisions. The complete worldgen boundary passed 70 unit tests
  and 64 integration tests, the Terrenia provider passed seven tests, and the
  engine passed 162 unit tests. Strict affected-crate all-target/all-feature
  Clippy, rustfmt, and diff checks passed. Three optimized natural-chunk runs
  measured 12.438-12.614 ms, 12.729-12.834 ms, and a final post-connectivity
  12.377-12.513 ms, remaining below 15 ms and within the +25% budget. No
  dependency was added.
- [ ] Slice 4 middle-scale terrain/cliffs and variable final-surface sediment
  implemented, automated gates passed, and fixed-seed visual acceptance in
  progress. The corrected corpus contains 6,769 delta-at-least-two land edges,
  maximum delta 7, 969,237 walkable neighbors per million, 12,524 exposed cliff
  faces, and median 64/256/1,024-window relief of 7/32/141 voxels. The shoreline
  corpus retains 1,553 crossings with p95/max jump one and no saturated middle
  field samples. Old provider policies and outputs remain frozen. Worldgen has
  75 passing unit tests; Terrenia has seven unit and four morphology integration
  tests; the engine has 163 passing unit tests. Strict affected-library Clippy
  and rustfmt pass. The optimized compensating-budget evidence is recorded in
  the performance table above. The next visual review accepted the variable
  dirt improvement but rejected globally slope-triggered rock caps; the
  evidence and replacement material contract are recorded in the second
  visual-review correction above and await implementation.
- [ ] Slice 5 integrated acceptance completed and evidence committed.

## Authoritative and primary references

- [Minecraft Java Edition 26.2 release notes](https://www.minecraft.net/da-dk/article/minecraft-java-edition-26-2)
- [Minecraft Java Edition 26.3 Snapshot 3](https://www.minecraft.net/en-us/article/minecraft-26-3-snapshot-3)
- [Minecraft Java Edition 26.1 technical changes](https://www.minecraft.net/fr-fr/article/minecraft-java-edition-26-1)
- [Minecraft Java Edition 1.18 release notes](https://feedback.minecraft.net/hc/en-us/articles/4415128577293-Minecraft-Java-Edition-1-18)
- [Minecraft 1.18 experimental world generation](https://www.minecraft.net/en-us/article/new-world-generation-java-available-testing)
- [Bevy 0.19.1 `StandardMaterial`](https://docs.rs/bevy/0.19.1/bevy/pbr/struct.StandardMaterial.html)
- [Bevy 0.19.1 `Image`](https://docs.rs/bevy/0.19.1/bevy/image/struct.Image.html)
- [Bevy 0.19.1 source](https://github.com/bevyengine/bevy/tree/v0.19.1)
- [Génevaux et al. 2013](https://doi.org/10.1145/2461912.2461996)
- [Cordonnier et al. 2016](https://doi.org/10.1111/cgf.12820)
- [Barnes et al. 2014](https://doi.org/10.1016/j.cageo.2013.04.024)
- [Braun and Willett 2013](https://doi.org/10.1016/j.geomorph.2012.10.008)
- [Peytavie et al. 2009](https://diglib.eg.org/items/fe20de3e-be7c-49c9-b003-f1afcf282db5)
- [Weber and Penn 1995](https://doi.org/10.1145/218380.218427)
- [Runions et al. 2007](https://diglib.eg.org/items/b5d756ee-0ab3-436e-a5cb-617d50df78fb)
- [World-GAN](https://arxiv.org/abs/2106.10155)
- [InfiniteDiffusion project and SIGGRAPH 2026 paper](https://xandergos.github.io/terrain-diffusion/)
- [GPU Gems: Effective Water Simulation from Physical Models](https://developer.nvidia.com/gpugems/gpugems/part-i-natural-effects/chapter-1-effective-water-simulation-physical-models)
