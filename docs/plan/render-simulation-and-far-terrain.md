# Render, simulation, and far-terrain plan

Status: approved; Slices 0-5 implemented, awaiting Slice 6 visual and soak
acceptance, 2026-08-31.

## Decision

Replace the single overloaded view-distance path with three independently
bounded products:

1. **Render Distance** is the selected total horizontal terrain horizon in
   chunks. It is a radius, and one Lattice chunk is 32 meters. A value of 21
   therefore targets a 672-meter cardinal horizon and must actually converge
   to visible terrain at that distance.
2. **Full Detail Distance** is the smaller near radius containing authoritative
   editable voxel meshes, complete vertical chunk occupancy, colliders, and
   normal vegetation. It owns the existing 1,183-resident-chunk budget and is
   not allowed to redefine Render Distance.
3. **Simulation Distance** is the independent radius for gameplay activation
   and ticking. It does not grow just because the visual horizon grows.

The far ring between Full Detail Distance and Render Distance uses derived,
surface-aware terrain tiles with bounded levels of detail (LOD). It does not
load seven full vertical chunks for every distant horizontal coordinate. The
normal Settings page contains controls, not runtime diagnostics: remove the
always-visible `Render radius ...` status row. Detailed requested, loaded,
presented, queue, and clamp values belong in developer diagnostics.

This supersedes the UI-truthfulness-only recovery in
[`view-distance-runtime-recovery.md`](view-distance-runtime-recovery.md). The
earlier change identified the resident clamp correctly but did not satisfy the
player-facing behavior.

## Reproduced failure and current architecture

The selected request reaches settings persistence and the host, but
`crates/latticeaxiom-engine/src/host/stream.rs` currently derives one
`StreamDistances` value set for all of the following:

- authored and persisted render request;
- host admission;
- full-resolution resident radius;
- visible/render scope;
- simulation scope;
- camera far distance and fog presentation.

The resident budget is 1,183 full chunks. The current square interest set at
radius six is `(2 * 6 + 1)^2 * 7 = 1,183`: thirteen by thirteen horizontal
columns, each retaining seven vertical chunks. `is_render_chunk` then tests the
budget-derived effective radius, so any selection above six cannot expand the
visible boundary. At radius 21, the same representation would require
`(2 * 21 + 1)^2 * 7 = 12,943` chunks, about 10.9 times the accepted working set.

Consequently:

- the UI setting is wired, but to a representation that cannot deliver its
  advertised semantics;
- the earlier `effective/requested` text exposed the mismatch without solving
  it;
- the wording `view distance` hides materially different costs and lifetimes;
- changing the full-chunk cap would reproduce the problem at a larger memory
  footprint.

## Current reference survey

All external projects are study-only. No source, assets, constants, or UI
layout are copied. Reference checkouts live under ignored
`.temp/reference/` and are not dependencies.

| Reference inspected on 2026-08-31 | Revision/version and license | Relevant contract | Decision |
| --- | --- | --- | --- |
| [Sodium](https://github.com/CaffeineMC/sodium) | `2c9463c05df8835296b5bf330b830a56a9c0a336`, 2026-08-29; PolyForm Shield 1.0.0 | Its General page has independent Render Distance and Simulation Distance sliders; renderer reload is explicit. Quality and Performance have separate pages. | Adopt the semantic separation, not code or exact layout. Current Sodium is source-available rather than OSI-open-source. |
| [Bobby](https://github.com/Johni0702/bobby) | `47868a67f24f2dc2670cbbb22fd3e3363cb152a1`, 5.2.15, 2026-07-12; LGPL-3.0-or-later | Stores received chunks and can display them beyond a server's current view; `maxRenderDistance`, server-view overwrite, cache, and unload behavior are separate. | Preserve the distinction between authoritative/near availability and client visual coverage. Do not adopt its network cache because this slice is local/provider-backed. |
| [Distant Horizons](https://gitlab.com/distant-horizons-team/distant-horizons/-/tree/main) | main `9a1f808826d66aeb9d980ef22fc416bb57e209b2`, core `77a04304cafc3d1110e3a326d7a695f231b5d4a4`, 3.2.1-b-dev, 2026-08-30; LGPL-3.0 | Separates LOD radius, horizontal quality, maximum resolution, vanilla overlap, distant generation, thread budget, and real-time update distance. | Adopt distinct distance, quality, overlap, and work-budget concepts. Build a much smaller Lattice-owned surface-tile contract. |
| [Voxy](https://github.com/MCRcortex/voxy) | `02dfb1b7a91cddd02891a057cdd38478ea195c26`, 0.2.19-beta, 2026-08-29; all rights reserved | Keeps Voxy distance separate from vanilla full-detail distance, tracks hierarchical rings, rate-limits work, and transitions/fades the overlap. | Adopt the hierarchical-ring and bounded-work lessons only. Its license forbids reuse. |
| [Minecraft Java Edition 26.2](https://www.minecraft.net/en-us/article/minecraft-java-edition-26-2) and generated reports | data pack 107.1 | Render and simulation are distinct player concepts; current Overworld surface rules are a separate final-surface composition stage. | Use it as the behavior/data-format reference required by repository policy, not as a source dependency. |
| [Bevy 0.19.1 `VisibilityRange`](https://docs.rs/bevy/0.19.1/bevy/camera/visibility/struct.VisibilityRange.html) | workspace-locked 0.19.1; MIT OR Apache-2.0 | Native distance visibility with start/end margins supports overlapping HLOD and dither transitions. | Use this Bevy primitive for near/far overlap. Do not create a parallel visibility runtime. |
| [Bevy meshlets](https://github.com/bevyengine/bevy/blob/v0.19.1/crates/bevy_pbr/src/meshlet/mod.rs) | Bevy 0.19.1; MIT OR Apache-2.0 | Provides mesh LOD/culling but has preprocessing, base-overhead, and current backend/feature constraints. | Reject for the first dynamic-terrain slice; retain conventional meshes and portable headless tests. |
| [Malyshau 2026, *Six Ways to Draw Vangers with WebGPU*](https://arxiv.org/abs/2608.17390) | arXiv:2608.17390, 2026-08-18 | Eye-level views expose errors hidden by top-down tests; over-simplified terrain can lose walls, while measured methods trade memory against performance. | Require cliff/wall silhouettes and an error envelope in far tiles; never average each tile to one top height. |

Exact inspected Sodium option construction is in
[`SodiumConfigBuilder.java`](https://github.com/CaffeineMC/sodium/blob/dev/common/src/main/java/net/caffeinemc/mods/sodium/client/gui/SodiumConfigBuilder.java).
Bobby's independent limits are in
[`BobbyConfig.java`](https://github.com/Johni0702/bobby/blob/master/src/main/java/de/johni0702/minecraft/bobby/BobbyConfig.java).

### Build-versus-buy decision

No dependency is added initially:

- Sodium is not a terrain LOD library and its current license is unsuitable for
  copied integration code.
- Bobby addresses retained server chunks rather than provider-derived local
  terrain.
- Distant Horizons is a complete Minecraft-specific subsystem whose storage,
  networking, coordinate, and renderer contracts do not match Lattice.
- Voxy is source-visible but reuse is not licensed.
- Bevy already supplies task pools, meshes, ECS scheduling, visibility ranges,
  and render extraction. A small Lattice-specific derived tile is the narrower
  auditable boundary.
- Bevy meshlets are not the portable first step for mutable generated terrain.

Reconsider a dependency only after the representative benchmark and
conformance suite demonstrate a missing Bevy facility.

## Settings and product contract

### General page

1. **Render Distance**: `2..=32 chunks`, persisted integer, default retained
   from the existing catalog. Help text: `Horizontal radius of visible terrain;
   one chunk is 32 m.` The selected value remains the target while data loads.
2. **Simulation Distance**: a separate persisted integer and stable setting ID.
   Its initial authored range is `2..=32 chunks`; the active world profile may
   clamp it to its separately named simulation limit. Help text explains that
   it controls active gameplay rather than the visible horizon.

Changing Render Distance must not silently change Simulation Distance. On
migration, the existing `view distance` value becomes Render Distance and the
new Simulation Distance starts from the current host profile default, so
existing users do not unexpectedly multiply gameplay work.

### Quality page

3. **Distant Terrain Quality**: `Performance`, `Balanced`, or `Quality`. This
   selects geometric-error thresholds, LOD transition widths, and optional far
   surface detail. It never changes the target horizon.

### Advanced page

4. **Full Detail Distance**: the expensive editable-voxel radius, initially
   bounded to the currently certified `2..=6 chunks` desktop-reference range.
   Profiles may lower it. Its help text states that it affects nearby block,
   vegetation, and collision detail, not the far horizon.

The default product surface may keep Full Detail Distance under Advanced, but
the runtime type and persisted meaning must be explicit from the first slice.

### Removed and relocated UI

- Remove the top Settings status text beginning with `Render radius`.
- Do not add an `effective/requested` fraction to the normal HUD.
- Keep `Apply`, `Undo`, validation, persistence, and restart messages only in
  the ordinary Settings status area.
- Put `render target`, `full detail`, `presented contiguous radius`, `simulation
  radius`, `far queue`, `resident chunks`, and clamp reasons in the existing
  developer/F3 diagnostic path.
- A temporary unobtrusive `Loading distant terrain...` notice is allowed only
  while the contiguous visible frontier is catching up; it must not replace or
  rewrite the selected option.

## Honest runtime types

The implementation must make invalid substitutions difficult at compile time.
Names below describe semantics; exact module ownership is decided in Slice 1.

| Type | Meaning | May constrain |
| --- | --- | --- |
| `RequestedRenderDistanceChunks` | Raw validated user target before world-profile admission | Persistence and admission only |
| `TargetRenderDistanceChunks` | Admitted total terrain horizon | Far-ring scheduling and camera target |
| `FullDetailDistanceChunks` | Near authoritative voxel radius | Full chunks, colliders, near vegetation |
| `SimulationDistanceChunks` | Gameplay/tick radius | Activation only |
| `PresentedRenderDistanceChunks` | Largest contiguous ready near-plus-far radius for this frame | Temporary fog/coverage diagnostics, never persistence |
| `FarTerrainQuality` | Geometric-error policy | LOD selection, not radius |
| `FarTileProvenance` | Generated provider identity or committed snapshot revision/checksum | Cache reuse and invalidation |

Delete or narrow APIs that return an unqualified `effective_view_distance`.
No camera, UI, or profiler may infer one semantic from another integer. Checked
chunk-to-meter conversion remains owned by the compiled world geometry.

## Far-terrain representation

### Near ring

The near ring preserves the existing correctness contract:

- complete authoritative chunk occupancy and current vertical extent;
- editable voxel meshes, water surfaces, collision, and normal vegetation;
- the 1,183 resident and 405 active caps remain unchanged until separately
  re-certified;
- near work has strict scheduling priority over far work.

### Far ring

For horizontal coordinates outside Full Detail Distance and inside Render
Distance, build a derived surface shell rather than seven full chunks:

- use a circular Euclidean horizontal footprint so corners do not consume a
  square full-distance budget;
- sample the authoritative final solid surface, surface material, and water
  surface at a tile-owned resolution;
- retain min/max height and geometric-error bounds per cell;
- emit top surfaces plus vertical skirts/walls where simplification would
  otherwise erase cliffs, terraces, shore banks, or tile-border closure;
- keep water in a distinct mesh/material path so overlap cannot create
  coplanar water surfaces;
- use at least two far LOD levels, selected by projected geometric error and
  distance, rather than a fixed coordinate stride alone;
- use deterministic border samples shared by adjacent tiles, with golden seam
  tests for positive and negative coordinates;
- omit full tree geometry initially. A later measured slice may add distant
  vegetation clusters or impostors after terrain coverage is correct.

This is a presentation cache, not a second world authority. It cannot be used
for collision, mining, simulation, or persistence.

### Provenance and edit correctness

A far tile derives from one of two typed sources:

1. a committed snapshot/chunk revision with a stable checksum when authoritative
   chunk data exists; or
2. the exact procedural generation epoch, plan ID, provider fingerprint, seed,
   and coordinate when it does not.

The cache key includes source provenance, tile coordinate, LOD level, and mesh
algorithm revision. A committed block edit invalidates the affected tile and
all coarser ancestors. When a near full-detail chunk becomes ready, the overlap
is regenerated or certified against the same revision before transition. A
stale far tile may never cover a newer near chunk.

## Streaming, scheduling, and presentation

- Use Bevy's compute/async task pools and ECS resources. Do not introduce a
  project-owned thread runtime.
- Maintain bounded, deduplicated queues for near generation, near meshing, far
  sampling, and far meshing. Apply cancellation when the player moves and
  discard completed results whose provenance or interest generation is stale.
- Sort output-affecting work by stable priority keys. Queue completion order
  cannot alter tile bytes or visible ownership.
- Reserve capacity and frame-time slices for near chunks; far jobs use only the
  remaining configured budget and back off under rapid travel.
- Admit far coordinates in concentric rings around the current player chunk,
  prioritizing the view direction only as a stable secondary presentation
  optimization. Contiguous coverage, not the furthest isolated tile, defines
  `PresentedRenderDistanceChunks`.
- Crossfade near and far owners with matching Bevy `VisibilityRange` margins.
  Exactly one representation owns collision and edits; render overlap may be
  deliberate but must not produce depth-fighting surfaces.
- Derive the perspective far plane from Target Render Distance plus bounded
  height/safety margin. Derive normal air fog from the contiguous presented
  frontier while loading, then converge it to the target. Preserve the existing
  underwater and lava overrides.

## Implementation slices and autonomous commit sequence

Implementation begins only after this plan is approved. Each slice receives
its own tests, diff review, and Conventional Commit before the next behavior
change.

### Slice 0: plan and baseline evidence

- record this reference survey, revisions, licenses, and rejection decisions;
- capture fixed-camera 4/6/21 screenshots and current target-to-visible-boundary
  measurements;
- record optimized full-chunk streaming, generation, meshing, frame-time,
  RAM, and queue baselines.

Commit: `docs(streaming): plan split distance and far terrain`.

### Slice 1: typed settings and migration

- add stable Render Distance, Simulation Distance, Full Detail Distance, and
  Distant Terrain Quality contracts;
- migrate the existing persisted view value to Render Distance without losing
  unknown/orphan settings;
- introduce the typed runtime values above and remove ambiguous internal APIs;
- keep the new far path feature-disabled until it is complete.

Commit: `refactor(settings): split terrain distance contracts`.

### Slice 2: deterministic far-tile contract

- implement source provenance, cache keys, final-surface sampling, error bounds,
  cliff-preserving shell construction, and shared borders;
- add characterization tests before adapting any existing surface path;
- add golden, permutation, edit-invalidation, and property/conformance tests;
- add an optimized far-tile benchmark and compare memory per covered column
  with seven full chunks.

Commit: `feat(world): add deterministic far terrain tiles`.

### Slice 3: bounded hierarchical streaming

- add circular far rings, stable priority, bounded task queues, cancellation,
  backpressure, stale-result rejection, and near-work reservation;
- keep active/simulation/full-detail selection independent;
- expose structured developer diagnostics and reproducible profiling evidence.

Commit: `feat(streaming): schedule bounded far terrain rings`.

### Slice 4: Bevy presentation and transition

- materialize conventional Bevy terrain/water meshes;
- use `VisibilityRange` for near/far and inter-LOD overlap;
- synchronize camera and loading fog with target/presented distance;
- prove no cracks, holes, depth fighting, or water flicker at boundaries.

Commit: `feat(render): present terrain across lod rings`.

### Slice 5: settings UI correction

- split General, Quality, and Advanced rows as specified;
- remove the top `Render radius` status line and normal HUD view telemetry;
- add accessible names, help text, keyboard/controller behavior, migration, and
  apply/undo/persistence tests;
- verify that selecting 21 means the 672-meter target, not a hidden six-chunk
  full-detail clamp.

Commit: `fix(settings): separate render simulation and detail distance`.

### Slice 6: integrated acceptance and tuning

- execute every functional, visual, determinism, memory, and latency gate;
- tune only named quality/error/work-budget parameters, never redefine a
  distance control to make a performance result pass;
- update this document with exact measurements and user visual feedback.

Commit: `test(streaming): certify far terrain presentation`.

## Acceptance gates

### Player-facing behavior

- At the same seed, camera, window, and quality, Render Distance 4 and 21 have
  visibly and measurably different cardinal terrain boundaries: 128 meters and
  672 meters after convergence.
- Render Distance changes never silently modify Simulation or Full Detail
  Distance.
- The Settings page has no always-visible `Render radius` diagnostic row and no
  ambiguous `6/21` label.
- The displayed unit says `chunks`, help text says `radius`, and conversion uses
  the compiled 32-meter chunk edge.

### Geometry and correctness

- Fixed corpora contain no cracks or holes at tile borders, LOD transitions,
  negative coordinates, steep cliffs, shorelines, or world-height extremes.
- Far silhouettes preserve corpus cliffs/walls within the quality preset's
  declared maximum vertical and screen-space error.
- Water has one visible owner per surface through transitions and passes the
  adjacent-frame shimmer metric already used by the water plan.
- Same source identity and coordinates produce byte-identical tile data across
  generation order and worker completion permutations.
- Committed edits invalidate and update every affected LOD; stale far terrain
  never overlays newer full-detail data.

### Bounded performance

- Full-resolution resident chunks remain at or below 1,183 and active chunks
  remain at or below 405 on the desktop reference profile.
- All queues have enforced finite capacities; rapid flight/teleport tests prove
  bounded memory, cancellation, stale-result rejection, and near-work progress.
- Record optimized before/after p50, p98, and p99.5 frame time, generation and
  mesh latency, far-terrain convergence time, process RAM, and renderer memory
  where Bevy/wgpu diagnostics expose it. Averages alone are insufficient.
- Run a ten-minute movement/rotation soak at distances 6, 21, and 32. No
  monotonic RAM growth, unbounded pending work, or recurring frame-time spikes
  may remain.
- The fixed 21-distance camera rotation test must not cause mass rebuilds when
  coordinates remain in the admitted circle.

### Verification boundary

- focused unit, property, integration, golden, headless-host, and settings-page
  tests;
- strict affected-crate Clippy, rustfmt, and `git diff --check`;
- optimized reproducible benchmarks with raw evidence recorded here;
- fixed-window visual acceptance at 4, 6, and 21 chunks. Pause for user-provided
  screenshots/feedback when automated evidence cannot judge composition,
  transition visibility, fog feel, or distant cliff readability.

## Risks and controls

| Risk | Control |
| --- | --- |
| Far terrain becomes a second world authority | Presentation-only typed API; provenance keys; no collision, edits, ticks, or persistence through far tiles |
| Simplification erases cliffs | Min/max and error bounds, explicit wall/skirt geometry, eye-level golden views, recent height-field comparison as the adversarial reference |
| Fast movement starves nearby terrain | Separate bounded queues, strict near priority/reservation, cancellation, and soak tests |
| LOD overlap produces z-fighting or water shimmer | Matching `VisibilityRange` margins, single surface ownership rules, separate water mesh, seam and temporal tests |
| UI exposes implementation jargon | Normal pages contain user goals; detailed target/full/presented/queue values remain developer diagnostics |
| Migration changes existing gameplay cost | Existing value maps only to Render Distance; Simulation begins at the certified profile default |
| A source-visible mod becomes accidental copied design/code | Exact license record, independent Lattice contracts, no source copying, review diff against project-owned terminology |
| Tuning hides a broken contract | Distances, quality, and work budgets remain different types; acceptance measures physical boundary in meters |

## Progress and evidence

- [x] 2026-08-31: Slice 0 research and implementation plan committed as
  `4783789`. The referenced upstream projects remain study-only checkouts under
  `.temp/reference/`; no code, asset, or runtime dependency was copied.
- [x] 2026-08-31: Slice 1 typed settings and migration implemented. The legacy
  stable ID `latticeaxiom:setting/view-distance` now names Render Distance, so
  an existing stored value migrates without a persistence rewrite. Simulation
  Distance, Full Detail Distance, and Distant Terrain Quality have independent
  stable IDs, defaults, value domains, page sections, apply values, and typed
  runtime projections. Unknown stored settings and orphan bindings remain
  preserved. Engine streaming now distinguishes requested render, target
  render, requested/effective full detail, requested/effective simulation, and
  presented render distance; the incomplete far path is feature-disabled by
  setting presented distance equal to the ready full-detail frontier.
  Runtime-contract tests passed 57 unit, 6+7+2 integration, and doc-test gates;
  settings UI passed 1 unit and 15+13 integration tests; client UI passed 17
  unit and 6+7+10 integration tests; engine passed 166 all-feature library
  tests plus the focused GPU-free production-host distance integration test.
  Strict affected-crate Clippy, rustfmt, and diff checks passed. A full 54-test
  GPU-free host run was also attempted: the distance integration passes, while
  the separate physical cave journey still fails to navigate from the opened
  entrance to topology cell centers after the midscale-terrain change. That
  failure does not mutate or exercise the new distance requests, is not counted
  as Slice 1 evidence, and remains an explicit integrated-acceptance item rather
  than being hidden by weakening its assertion. No external dependency was
  added.
- [x] 2026-08-31: Slice 2 deterministic far-terrain contract committed as
  `1a4b812`. The presentation-only worldgen API now has typed procedural and
  committed provenance, cache identity, bounded LOD and base-edge values,
  exact final-density/cave/slope-aware surface sampling, separate solid and
  water lanes, dense min/max and bilinear-error envelopes, representative
  cliff walls, border skirts, and hierarchical edit invalidation. The same
  natural material query was characterized against materialized chunk tops.
  Signed-coordinate seams, provider/order permutations, serialization
  invariants, edit boundaries, and canonical bytes are covered by unit,
  integration, property, and golden tests; the version-one tile golden hash is
  `8a9db7eb39f80ff35ef94f69e67c8252e50a46c37938b41794807046293a0b32`.
  The all-feature worldgen run passed 82 unit tests and 67 integration tests
  before the final invariant hardening; the final nine-test far-terrain suite,
  strict all-target Clippy, rustfmt, and diff checks also passed. No dependency
  was added. The optimized Criterion run (10 samples, one-second warmup and
  measurement) recorded LOD0/1/2 build intervals of 129.87-138.84,
  262.93-273.83, and 705.50-734.93 microseconds. Each 32-edge tile retained
  101,912 bytes of vectors versus 458,752, 1,835,008, and 7,340,032 bytes for
  two-byte indices across the same horizontal coverage and seven full vertical
  chunks: 22.21%, 5.55%, and 1.39% of that conservative baseline. A hard
  64-voxel base-edge and LOD3 cap bounds the worst dense build to 513 by 513
  samples.
- [x] 2026-08-31: Slice 3 bounded hierarchical streaming committed as
  `f210cb9`. The production host now selects non-overlapping far tiles over a
  Euclidean chunk-center circle, recursively splits the near boundary, varies
  LOD by the independent quality contract, and publishes completed tiles in
  stable priority/sequence order. The Bevy `AsyncComputeTaskPool` is reused;
  no thread runtime or dependency was added. Hard capacities are 128 pending,
  two in flight, and 1,024 ready tiles. Near worldgen and derived work reserve
  the shared CPU lane before far work. Interest changes cancel queued/task
  ownership and an atomic generation check stops stale dense sampling; old
  completions are rejected. Ready tiles shared by the new interest are reused.
  Procedural tiles intersecting edited chunks or their shared borders are
  conservatively suppressed until a committed-surface adapter can certify
  them, so stale terrain cannot cover an authoritative edit.

  The structured host snapshot reports desired, edit-blocked, pending,
  in-flight, waiting, ready, retained vector bytes, interest generation,
  cancellation/stale counts, near reservations, and the contiguous presented
  frontier. Presented distance now advances only when every tile intersecting
  the next radius is ready and remains clamped between full detail and target.
  All three quality selections at the authored 32-chunk maximum fit the ready
  cap; exact signed-coordinate coverage tests prove one owner for every chunk
  center in the circle and no ownership inside the near square. The engine's
  170 all-feature unit tests and strict all-target Clippy passed. Focused real
  host tests proved a 4-chunk target converges from full detail 2 to presented
  4 without increasing the authoritative resident cap (5.64-second complete
  test runtime), a 32-to-4 change cancels the old bounded queue, and camera-only
  rotation does not rebuild far interest.
- [x] 2026-08-31: Slice 4 Bevy presentation committed as `26c553d`. Derived
  far tiles now materialize through conventional Bevy solid and translucent
  water mesh lanes. Surface heights convert to top planes, upward normals and
  winding are tested, and east/south half-open skirt ownership gives every
  shared border one wall owner. Near and far entities use Bevy
  `VisibilityRange` with abrupt, non-overlapping ranges: a single rendered
  surface owner was selected instead of a crossfade because the current
  materials cannot guarantee depth-safe coplanar blending. Water is likewise
  single-owned across the boundary. Camera range and loading fog follow the
  contiguous GPU-uploaded frontier rather than CPU-ready tiles, preventing a
  tile from opening a visible hole before its mesh assets exist. Uploads are
  nearest-first and hard-capped at two tiles per frame; replaced and evicted
  mesh assets are explicitly released. Far entities carry no collider, edit,
  simulation, or persistence authority. Five focused far-mesh tests and the
  final 176-test all-feature engine library suite passed; strict affected-crate
  Clippy, rustfmt, and diff checks passed. GPU-visible cracks, transitions, and
  water shimmer remain part of Slice 6 rather than being claimed from
  headless tests.
- [x] 2026-08-31: Slice 5 settings information architecture and diagnostics
  cleanup committed as `4ecaed5`. The compatibility ID
  `latticeaxiom:setting/view-distance` now presents as **Render Distance**.
  Its row and detail show a chunk radius and compiled meter conversion, so 21
  is explicitly `21 chunks radius (672 m)`. Player help distinguishes total
  terrain draw radius, authoritative Simulation Distance, Full Detail
  Distance, and radius-neutral Distant Terrain Quality. The existing General,
  Gameplay, Quality, and Advanced section routing is covered by contract
  tests. Generic catalog keys, package ownership, requested/admitted/effective
  internals, and raw stable IDs were removed from the normal detail view;
  actionable availability limits remain. Sliders and buttons now expose their
  real labels and distance help through AccessKit. Always-visible
  target/presented distance telemetry was removed from the settings hint and
  normal HUD without removing the typed diagnostic contracts. Client UI
  passed 17 unit plus 6, 7, and 10 integration tests; Settings UI passed one
  unit plus 15 and 13 integration tests; the engine passed 176 all-feature
  library tests. Strict all-target Clippy for all three crates, rustfmt, and
  diff checks passed.
- [ ] Slice 6 integrated acceptance completed.

## Approval boundary

Approval of this document authorizes the six implementation slices above and
their dedicated commits. It does not authorize raising the 1,183/405 full
chunk budgets, adopting an external mod/library, adding distant vegetation, or
changing serialized terrain authority. Those require new evidence and an
explicit plan amendment.
