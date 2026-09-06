# World-generation toolkits: research and implementation direction

Status: research-backed design, 2026-09-06. The user confirmed that different
Lattice packages are the primary consumers; independent package publication is
an objective. The proposed package names below are not implemented or published.
The current terrain corrections are a verified compatibility slice, not a
claim that the complete world-generation platform is finished.

## Product and architectural objective

World generation is a long-lived content platform. A world package should be
able to choose generators, combine them, declare its own materials/ecology and
ship a reproducible world recipe. Terrenia is one consumer of that platform.
Earth-like terrain is one supported family, not a requirement for every world.

A toolkit release includes algorithms, explicit contracts, authoring and
inspection tools, examples, compatibility fixtures and measured limits. Merely
moving files into several Cargo crates would not establish this product.

Static execution remains Bevy-native: App/ECS, scheduling and task pools. A
second executor or a cross-engine abstraction is not required by the accepted
scope. Public package identities, edges and source entries remain manifest
owned. External boundaries retain stable DTOs or the existing C ABI.

## Academic evidence and engineering implications

These are specific results and limitations, not blanket endorsements of one
generator. Paper sections and relevant figures were inspected from author-hosted
copies; no paper, dataset or image is included in version control.

| Source and inspected scope | Finding | Consequence for this project |
| --- | --- | --- |
| [Galin et al., A Review of Digital Terrain Modeling, 2019](https://perso.liris.cnrs.fr/eric.galin/Articles/2019-star.pdf), representations, procedural/simulation categories, sections 6.1-6.4 | Different landforms and scales favor different methods. Visual realism, geomorphological consistency, controllability and computational cost are distinct criteria. Camera and rendering choices can bias comparisons. | Evaluate both shape statistics and controlled native views. Expose multiple landform operators and their valid scales; avoid a universal "more noise = better terrain" control. |
| [Genevaux et al., Terrain Generation Using Procedural Models Based on Hydrology, 2013](https://perso.liris.cnrs.fr/eric.galin/Articles/2013-river-networks.pdf), algorithm overview, construction tree, results and limitations | A drainage graph can organize terrain features before local surface evaluation. The upstream-tree method has limitations for deltas, oxbows and transitions between some landforms. | Publish regional graph outputs separately from sampled surfaces. Do not pretend a tree-only river model covers every water system. |
| [Barnes, Lehman and Mulla, Priority-Flood, 2014](https://richard.science/sci/2014_depressions.pdf), queue algorithm and depression handling | Boundary-seeded priority traversal resolves depressions; a FIFO path reduces work inside filled depressions. Its drainage guarantee is relative to the provided DEM domain. | Keep boundary conditions and retained-lake policy explicit. Independently filling arbitrary chunk rectangles cannot establish globally consistent watersheds. |
| [Bridson, Fast Poisson Disk Sampling in Arbitrary Dimensions, 2007](https://www.cs.ubc.ca/~rbridson/docs/bridson-siggraph07-poissondisk.pdf), complete algorithm and complexity argument | Minimum-distance sampling uses a spatial grid and bounded attempts around an active list. This controls spacing, but the algorithm itself does not establish chunk-order independence. | Use it as a bounded-domain reference. Streaming placement needs stable candidate identities, ownership, overlap rules and halo verification in addition to a spacing distribution. |
| [Cordonnier et al., Tectonic Uplift and Fluvial Erosion, 2016](https://doi.org/10.1111/cgf.12820), published abstract/method overview | Uplift and stream-power erosion can organize large-scale drainage and relief under high-level controls. | Treat erosion as a regional planning option with documented units and convergence limits; do not run geological simulation in the frame loop. A detailed numerical conformance audit is still required before extending our solver. |

This review establishes a baseline bibliography, not an exhaustive literature
review of climate, caves, ecosystems or settlements. Each subsequent toolkit
slice must review the specific algorithms it adopts and retain its own evidence.

## Real engineering practice

| Project | Evidence | What to adopt or avoid |
| --- | --- | --- |
| [Minecraft developer Q&A](https://www.minecraft.net/en-us/article/caves---cliffs-update--part-ii-dev-q-a) and [official generation overview](https://learn.microsoft.com/en-us/minecraft/creator/documents/world-generation?view=minecraft-bedrock-stable) | The team describes map overlays, density sections, stage toggles and chunk reload tools as central to development. The documented pipeline distinguishes base generation, surfaces and later features. | Make inspection a first delivery, sharing the exact generator used by the game. Keep diagnostic explanations tied to stages and input fields. |
| [FastNoise2 node architecture](https://github.com/Auburn/FastNoise2/wiki/Node-Graph-Architecture) | Fused node evaluation retains intermediate data in SIMD registers; node metadata supports tooling. | Compile field expressions and support batched sampling. Do not assume floating-point SIMD variants meet our persisted-output determinism contract without cross-target tests. |
| [Voxel Tools generators](https://voxel-tools.readthedocs.io/en/latest/generators/) | Distinct voxel channels, block/point sampling, graph previews, and explicit warnings about overlapping multipass features and generation order. | Separate density, material and feature outputs. A tree placement claim must include a cross-chunk ownership/overwrite contract. |
| [Unreal PCG generation modes](https://dev.epicgames.com/documentation/en-us/unreal-engine/using-pcg-generation-modes-in-unreal-engine) | Hierarchical grids cache coarse results for finer generation. Replicating larger-grid points into smaller cells can duplicate content. | Plan at the appropriate scale, then clip emissions to one stable owner. Do not duplicate feature placements when refining or streaming. |
| [Fastscape](https://fastscape.org/) | Composable landscape-evolution components, lower-level solvers, and interactive scientific tooling are separate parts of the ecosystem. | Separate algorithm kernels, model composition and inspection. Keep optional expensive simulation distinct from cheap evaluation of a frozen result. |
| [No Man's Sky GDC session](https://www.gdcvault.com/play/1024265/Continuous_World_Generation_in__No_Man_s_Sky_) | The published session description covers continuous voxel generation through polygonization, texturing, population and simulation. Only the description was reviewed here, not the full recording. | Relevant further engineering reading; no undocumented internal scheduling or determinism claim is inferred from the abstract. |

## Audit of the current repository

There is already substantial reusable work in `latticeaxiom-worldgen`:

- `seed.rs`, `fixed_field.rs`: versioned seed derivation, bounded integer noise,
  known-answer vectors and numerical envelopes.
- `hydrologic_domain.rs`, `hydrologic_topology.rs`, `landscape_evolution.rs`:
  bounded plans, routing, lakes, integer accumulation, incision/diffusion and
  final rerouting. Some parallel entry points already use Bevy task pools.
- `generation.rs`, `provider.rs`, `region.rs`: immutable generation plans,
  provider identities, candidate production and finite work limits.
- `cave_topology.rs`, `tree_morphology.rs`, `far_terrain.rs`: topology,
  procedural feature geometry and presentation sampling with provenance.
- `@terrenia/worldgen`: game-owned presets, semantic field parameters and role
  bindings. These are the correct kind of consumer-owned policy.

Important remaining coupling:

1. `@latticeaxiom/world` owns storage, territory and worldgen. The territory
   crate depends on worldgen; worldgen depends on storage. Simply extracting
   all of worldgen into a new package that depends on `@latticeaxiom/world`
   would create a package-level cycle through territory. Extract lower-level
   contracts/operators first and preserve an adapter during migration.
2. `TerrainStyleV1` has four closed terrestrial styles, while
   `D4MaterialRoleV1` embeds an established terrain/ecology vocabulary. These
   are compatibility contracts, not sufficient generic outputs for arbitrary
   worlds. New toolkit operators should use typed field/feature contracts and
   package-owned IDs; keep the old enums in the legacy adapter until migrated.
3. The host still orchestrates product generation and contains default provider
   revisions. The new frozen policy file fixes two specific version choices;
   it is not the final general-purpose generator manifest.
4. World seeds currently derive from the world UUID. A future world recipe
   needs an explicit immutable seed separate from world identity, with legacy
   UUID-derived behavior preserved for existing saves.
5. Current inspection is rich in typed point reports and tests but lacks a
   coherent map/slice/corpus workbench for world authors.
6. The crate's old "no Bevy dependency" README claim was stale. Actual manifests
   and task-pool imports are the source of truth.

## Proposed package boundaries

Names are provisional; each implemented crate must have exactly one owner under
`packages/<scope>/<package>/crates/`.

| Proposed Lattice package | Responsibility | Inputs and outputs |
| --- | --- | --- |
| `@latticeaxiom/worldgen-foundation` | Seeds, numerical domains, coordinates, sampling bounds, immutable recipe/operator identities | Typed contracts, not Terrenia styles or block catalogs |
| `@latticeaxiom/worldgen-fields` | Coherent scalar/vector fields, composition, warps, remaps, bounded density/height operations | Point/batch/tile field evaluation with range and work metadata |
| `@latticeaxiom/worldgen-hydrology` | Regional drainage, basin/lake policy, flow topology and optional evolution | Explicit domain/boundary inputs to immutable hydrology products |
| `@latticeaxiom/worldgen-landforms` | Ridges, valleys, coasts, volumetric caves and other feature operators | Fields and optional regional plans to terrain/density products |
| `@latticeaxiom/worldgen-placement` | Habitat filters, spacing, ownership and structural placement | Stable feature candidates, support/clearance evidence and bounded emissions |
| `@latticeaxiom/worldgen-runtime` | Plan compilation/admission and Bevy execution integration | Manifest-selected operators to capped jobs and generation candidates |
| `@latticeaxiom/worldgen-tools` | Map/slice inspection, corpus runs, comparisons and profiling | The same frozen recipe and operators used by the production runtime |

Foundation has no dependency on storage/worldgen orchestration. Fields build on
foundation. Hydrology and landforms exchange declared immutable products;
feedback such as erosion/rerouting is a closed operator, not a package cycle.
Placement consumes the declared terrain/habitat inputs. Runtime and tools are
consumers of the algorithm packages. The exact storage/territory adapter edges
must be proven acyclic before moving a crate.

A game package owns its world recipe, enabled stages, landform mix, habitat
definitions, resource distribution, structures and gameplay constraints.
Resource packs supply client appearance and never determine generated matter.

## Contracts before extraction

Each operator must declare its ID/version, parameter schema, numerical model,
input/output types, units, supported coordinate range, execution mode, spatial
support/halo, work/memory limits, cancellation points and determinism guarantees.
Useful execution modes differ: point sample, bounded tile evaluation, regional
planning and feature emission. They need not all masquerade as one noise node.

Separate an immutable recipe/plan from materialized chunks and player edits.
Cache identity includes all output-affecting inputs; provenance-only changes
must not accidentally reseed the world. Upgrades distinguish byte-compatible
changes, new generation epochs and explicit copied-world migrations. Persisted
old recipes remain reopenable. Do not rely on Cargo crate versions alone as
the world-generation compatibility contract.

Independent publication means a self-contained manifest/source closure, pinned
dependencies, documented operator/recipe schemas, test fixtures, compatibility
policy and reproducible acceptance receipts. An independently consumable Rust
crate or cross-engine SDK is not the first milestone. Current `publish = false`
settings are not changed, and no registry publication is performed by this work.
Release preparation must also reconcile package license metadata and retained
upstream notices with the actual source closure; this work changes no licenses.

## Delivery sequence and acceptance

1. **Evidence workbench and corpus.** Reuse production plans to expose field
   maps, material/biome maps, final-surface and density slices, water levels,
   support/clearance, stage timings and bounded cache/queue state. Save seed,
   recipe, region, camera and comparison settings in text. Include the uphill
   regression and the `59cf1f69-6774-43a7-8353-0d0c034b7432` biome-boundary case.
2. **Foundation and fields.** Move existing proven primitives with compatibility
   re-exports/adapters and unchanged goldens. Keep source ownership and normal/
   build dependency DAG checks. Add a second world package that requires no
   temperate/arid/boreal vocabulary before declaring the interface generic.
3. **Regional products.** Extract hydrology and landforms with explicit edge
   conditions, halo equivalence and conserved quantities. Compare the actual
   discretization against selected reference algorithms, including units and
   fixed-point error. Calibrate each landform family with a documented corpus.
4. **Placement and recipes.** Prove vegetation/structure support, separation,
   cross-border ownership, deterministic rejection and overwrite policy. World
   packages compose these tools using manifests, without application-code edits.
5. **Independent release.** Build examples from exported frozen package closures
   without the Terrenia application or demo checkout. Exercise old-lock reopen,
   upgrade/recovery, optimized throughput and native visual acceptance.

Acceptance includes exact hashes across order/thread permutations and supported
targets; boundary equality; slope/curvature/relief and categorical patch metrics;
water conservation and drainage; support and traversal constraints; and fixed
camera visual review. These measure different properties. A realistic-looking
image does not establish determinism, and a passing hash does not establish
good terrain. Performance budgets must cover regional planning, cold chunks,
steady traversal, memory peaks and cancellation, not only noise throughput.

The current bug fixes establish three useful regression cases. They do not
establish calibrated climate simulation, realistic erosion at all scales,
complete ecosystem modeling, arbitrary-world support or a published toolkit.
