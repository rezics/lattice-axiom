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
- [`water_material.wgsl`](../../crates/latticeaxiom-engine/assets/shaders/water_material.wgsl)
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

The image descriptor declares six mip levels and the sampler remains linear
for minification, magnification, and mip interpolation. Tests prove byte count,
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
- [ ] Slice 1 final-surface contract implemented and committed.
- [ ] Slice 2 water temporal stability implemented and committed.
- [ ] Slice 3 tree morphology implemented and committed.
- [ ] Slice 4 middle-scale terrain/cliffs implemented and committed.
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
