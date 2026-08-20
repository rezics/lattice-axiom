# Lattice Axiom v1 Playable Delivery Plan

Status: active; sparse infinite streaming precedes durability  
Repository baseline: `0c38407` (`feat(engine): add playable host and dual fixture`)  
Date: 2026-08-21

## 1. Purpose

Deliver a package-driven, durable, single-player voxel sandbox that is genuinely
playable as Lattice Axiom v1. The release must provide an operationally unbounded
horizontal world, deterministic procedural generation, traversable caves, a
basic tool progression package, block editing, crafting, persistence, recovery,
and bounded client/headless execution from the same final product lock.

This plan is an implementation and integration sequence. It does not grant
world-writer authority, approve a security boundary, change an ADR, or authorize
implementation by itself.

In this plan, **v1 playable** means the accepted D0-D10 sandbox completion
target. It does not mean ABI 1.0, a public package registry, multiplayer, or a
complete long-term content catalog.

## 2. Current baseline and the real gap

The repository already contains substantial component evidence, but the
production player journey does not exist.


| Area | Present now | Missing for v1 |
| --- | --- | --- |
| Client interaction | A finite `-8..8` development fixture supports movement, mouse look, jump, break, and place | Package-driven world boot, chunk streaming, durable authority, real voxel projection, inventory, tools, crafting, and world lifecycle |
| Composition | Resolver, registration, SDK, launcher, profiles, and controlled Nickel foundations exist | Final product lock, local package acquisition through immutable CAS, production Nickel import path, and one launcher-driven client/headless pipeline |
| World data | Storage, world-wire, catalog, and world-db contracts exist | Authorized catalog-to-writer activation receipt and production integration |
| World generation | Deterministic D4 chunk generation, cave fields, epochs, territory, and hydrology contracts exist | A production coordinator, streaming integration, complete Terrenia plan, cave-domain realization, spawn selection, and persistence-first materialization |
| Voxel runtime | Mesh, working-set, derived-job, DDA, and projection contracts exist | Bevy client systems that load/generate/mesh/collide/evict real chunks instead of spawning one entity per fixture block |
| Sandbox gameplay | Generic inventory, drop, mining, tool, recipe, and transaction contracts have conformance fixtures | Runtime integration and package-authored tool items, recipes, hotbar, crafting, drops, and durable player state |
| Terrenia content | Authored 72-block, water/lava, gameplay, worldgen, and presentation data exists | Registration through the final package graph, complete reference validation, runtime consumers, and release-quality assets/behavior |

The current `playable` engine module is useful as an interaction smoke test, but
it explicitly bypasses the frozen package lock and durable writer, hard-codes
Terrenia identifiers, uses in-memory authority, and presents blocks as
individual entities. It must not evolve into the production world host. It is
retained until the production vertical slice supersedes its tests, then reduced
to a focused fixture or removed.

## 3. v1 player promise

A clean local installation with no network access can complete this journey:

1. Start the launcher and open the package-driven world library.
2. Create a Terrenia world from a shipped local package closure and a chosen or
   generated seed.
3. Complete frozen-lock preflight before any world writer is opened.
4. Spawn at a validated, safe surface location in a Y-up world.
5. Walk, look, jump, inspect the targeted block, and traverse continuously as
   chunks stream around the player.
6. Explore multiple surface territories, find a cave entrance, and enter a
   traversable underground region.
7. Gather wood, soil, stone, and at least one ore without duplication or loss.
8. Craft a workbench and basic wooden and stone tools from package-authored
   definitions.
9. Use the correct tool to mine faster, consume durability, receive drops, and
   place blocks from the hotbar.
10. Build a small recognizable structure, checkpoint, exit, and reopen.
11. Recover player position, inventory, tool durability, placed/broken blocks,
    containers or scheduled work included in v1, and the exact generation
    epoch without regenerating materialized chunks.

The same authoritative journey must run through command injection in a
headless Bevy App with no window or GPU.

## 4. Meaning of an infinite v1 world

“Infinite” is an execution property, not a promise to allocate or persist an
infinite object.

- Horizontal `x/z` space is operationally unbounded within the persistent
  chunk-coordinate representation. Normal play exposes no authored world edge.
- Vertical `y` space is finite and declared in the world header/configuration.
  v1 does not require infinite vertical terrain.
- Chunks are addressed and generated independently on demand from the world
  seed, frozen generation inputs, planning-cell epoch, and exact package lock.
- Only bounded active, resident, visible, generating, meshing, collider, save,
  and eviction sets may exist at once.
- Previously visited chunks may be evicted from memory. Durable materialized
  snapshots and edits are loaded before generation is considered.
- Generation order, task concurrency, approach direction, restart, and package
  discovery order must not change authoritative chunk bytes.
- Negative coordinates and checked coordinate-boundary behavior are first-class
  acceptance cases.
- View distance, generation radius, save radius, and queue budgets are settings
  selected through the package graph and clamped by hard host limits.

The v1 proof is sustained traversal with bounded memory and queues, not a large
pre-generated map.

## 5. Meaning of complete v1 world generation

“Complete” means a coherent, end-to-end generation pipeline with every layer
needed by the v1 player journey. It does not mean every future biome, structure,
or geological simulation.

The Terrenia v1 pipeline must contain:

1. **Frozen inputs** — world seed, exact package/artifact receipts,
   `WorldgenConfig`, provider selections, semantic image, role bindings,
   algorithm revisions, and generation epoch.
2. **Territory planning** — at least three queryable surface territories and
   two underground territories with stable primary/secondary ownership,
   boundary distance, bounded planning cells, and deterministic arbitration.
3. **Base terrain** — large-scale elevation, regional variation, local density,
   finite vertical bounds, surface/subsurface strata, and stable biome/style
   selection.
4. **Geology and resources** — queryable strata/geologic fields and stable,
   bounded distributions for the resources required by the v1 tool journey.
5. **Cave topology** — world-scale entrance/portal/connectivity intent combined
   with local bounded cave fields, chambers, tunnels, and branch contributors.
6. **Hydrology** — a continuous surface river/basin plan, underground drainage
   constraints, aquifer/fluid occupancy decisions, and explicit water/lava
   interaction boundaries.
7. **Surface features** — deterministic vegetation and simple natural features
   with exclusion radii and cross-chunk ownership rules.
8. **Spawn selection** — deterministic candidate search with clearance,
   footing, hazard, fluid, cave, and active-chunk readiness checks.
9. **Materialization** — role-resolved concrete content, snapshot-first chunk
   candidates, activation evidence, storage CAS, and immutable generation
   receipts.
10. **Diagnostics** — territory, biome, cave, hydrology, generation provenance,
    rejection, timing, queue, and memory evidence available without changing
    authoritative state.

Complex settlements, dungeons, mobs, combat, civilization simulation, erosion
simulation, seasons, and post-v1 biome expansion are not required.

## 6. Target package graph

The production world must be assembled from local packages and then frozen into
the final lock. The host must not contain a hidden Terrenia plugin list or
fallback dimension.

```text
terrenia
├── @terrenia/blocks
├── @terrenia/worldgen
├── @terrenia/gameplay
│   └── @terrenia/tools
└── @terrenia/presentation       # client projection only

client/headless profile
├── terrenia
├── @latticeaxiom/settings
├── @latticeaxiom/observability
└── client-only inspect/settings/dev surfaces as selected by the graph
```

| Package | v1 responsibility | Required relationship |
| --- | --- | --- |
| `terrenia` | Replaceable dimension root and namespace owner | Aggregates the authoritative closure; no concrete Terrenia fallback in the host |
| `@terrenia/blocks` | Block/fluid definitions, block-item mappings, physical/selection/collision semantics, worldgen roles | Provides the content catalog; contains no generic inventory or mining implementation |
| `@terrenia/worldgen` | Terrenia generation configuration, providers, territories, cave domains, hydrology, geology, resources, vegetation, and role bindings | Requires the block catalog; provides the exactly-one terrain provider for Terrenia |
| `@terrenia/gameplay` | Terrenia drops, placement rules, recipes, workstation/furnace/container bindings, and gameplay schema contributions | Uses host-agnostic sandbox contracts; depends on blocks and tools without embedding mechanics in the host |
| `@terrenia/tools` | Actual basic tool item definitions, tiers, classes, durability, mining modifiers, and tool recipes | Data-only package; depends on Terrenia material/item inputs and generic versioned gameplay schemas |
| `@terrenia/presentation` | Textures/materials/icons, display metadata, client render registrations, and HUD fragments | Omitted by headless without changing authoritative registration or world hash |

Namespace delegation must be explicit and non-overlapping. Existing stable item
IDs should be preserved. Tool-specific namespaces are delegated to
`@terrenia/tools`; any broad existing `terrenia:item/**` delegation is narrowed
or subdivided through a deterministic generated grant list before v1 worlds are
created. Namespace ownership and all cross-package references are compiled and
tested from the locked graph, never inferred from directory layout.

## 7. Basic tools package scope

`@terrenia/tools` is a real package-manager consumer, not a Rust crate with
Terrenia defaults.

### Required v1 content

- Hand behavior as an intrinsic no-item fallback for explicitly hand-breakable
  material only.
- Wooden pickaxe, axe, and shovel.
- Stone pickaxe, axe, and shovel.
- Stick and any other non-block item needed to make the recipes closed.
- Tool class, tier, maximum durability, mining multiplier, and compatible
  material/tool predicates for every tool.
- Shaped recipes for the six tools and the prerequisite stick/workbench path.
- Display name, icon, inspect fragment, and missing-presentation fallback for
  every tool item.

### Required mechanics

- Fixed-tick break progress based on block hardness and the selected hotbar
  item.
- Explicit failure when a block requires a tool class/tier not present.
- Correct-tool drop predicates independent from mining-speed predicates.
- Atomic durability decrement with the successful authoritative action.
- Tool breakage, full-inventory behavior, drop/pickup conservation, and
  placement-item conservation.
- Durable inventory, hotbar selection, recipe result, and durability state.
- Batched portable callbacks; no per-slot or per-block FFI round trips.

### Package acceptance

- Removing `@terrenia/tools` changes the candidate lock and fails required
  dependency/capability preflight before the writer opens.
- Replacing it with a conforming fixture tools package requires no host source
  change.
- The host and platform packages contain no `terrenia:*` tool, recipe, or drop
  defaults.
- Registration and recipe output are independent of source discovery order.

Iron/copper tool ladders, weapons, armor, enchantments, repair, and combat are
post-v1 unless later promoted by an accepted scope change.

## 8. Runtime architecture to build

### 8.1 One production boot path

```text
bootstrap TOML
  -> local package acquisition and immutable CAS
  -> source-table-only Nickel evaluation
  -> exact graph resolution
  -> final latticeaxiom.lock written and reopened
  -> registration compilation
  -> RuntimeImage
  -> world catalog preflight
  -> sealed writer activation receipt
  -> Bevy client or headless App
```

No native code, world writer, world generation, or Bevy world session may start
from an unresolved intent or an in-memory candidate lock.

### 8.2 Chunk lifecycle

The production coordinator owns an explicit state machine such as:

```text
Absent
  -> StorageQueued
  -> Loading
  -> GenerationQueued (only when storage proves absence)
  -> Generating
  -> CommitQueued
  -> ResidentData
  -> Mesh/ColliderQueued
  -> Active
  -> SaveQueued / EvictPending
  -> Evicted
```

Every async result carries the world epoch, chunk coordinate, source revision,
semantic fingerprints, and expected storage revision. Stale work is discarded
without mutating authoritative state. Worldgen workers return candidates; they
never open storage or publish chunks directly.

### 8.3 Bounded interest and scheduling

- Derive desired chunks from authoritative player position plus bounded
  look-ahead, not from an ever-growing visited set.
- Prioritize safety collider/data work before presentation mesh work; prioritize
  near and movement-direction chunks without making results order-dependent.
- Use Bevy task pools and bounded queues with cancellation, byte reservations,
  backpressure, and per-tick admission limits.
- Separate active, resident, visible, generating, meshing, collider, dirty, and
  saving counts in diagnostics.
- Do not perform blocking I/O, full-chunk allocation, or mesh construction in a
  frame-critical system.
- Evict only after authoritative dirty state is durable or the eviction is
  explicitly refused.

### 8.4 Authoritative and derived boundaries

- Chunk voxel/fluid palettes, block entities, player state, inventory,
  scheduled gameplay work, revisions, and generation receipts are
  authoritative.
- Meshes, colliders, render entities, visibility, and visual diagnostics are
  derived and rebuildable.
- A presentation or GPU failure must not change the world hash.
- Production rendering uses chunk mesh batches and upstream Bevy/ecosystem
  facilities; it must not retain the development fixture's entity-per-block
  model.

## 9. Cave design for v1

Use a hybrid cave model so local noise cannot accidentally define global
connectivity and global planning does not require voxel-scale world allocation.

### Planning layer

- The Territory Atlas selects a default cave topology domain plus two
  underground-territory-owned subdomains.
- Stable portal contracts define required entrances, cross-domain connections,
  tangent, clearance, fluid state, and dependency receipts.
- At least one entrance crosses four planning cells and two topology domains.
- At least one large underground destination is marked must-connect.
- Hydrology can require drainage connections without becoming the cave owner.

### Local realization layer

- Bounded integer/SDF fields realize tunnels and chambers inside each chunk.
- Shared-face requests are direction-independent and validated against final
  occupancy on both sides.
- Domain-specific algorithms may contribute local fields only inside their
  declared influence bounds.
- A bounded branch contributor may add optional side passages but cannot break
  required portal clearance or minimum surface cover.
- Material, ore, vegetation, and fluid placement run against the final cave
  occupancy rather than an earlier raw field.

### Cave acceptance

- Fixed-seed traversal reaches the required entrance, both underground
  territories, and three resource classes.
- Portal position, tangent, clearance, fluid state, shared-face occupancy, and
  must-connect destinations satisfy machine-readable assertions.
- Connectivity, passability, loop/dead-end distribution, generation cost,
  rejected contributors, and fallback use are reported.
- Chunk order, task parallelism, restart, and approach direction produce the
  same cave plan and chunk bytes.
- No entrance opens into an unready collider void, unsafe spawn, or unexplained
  fluid discontinuity.

## 10. Milestone sequence

Each milestone ends with a runnable client slice and a headless command-input
slice from the same authoritative closure. Component tests alone do not close a
milestone.

Execution order is not numeric order. Sparse operationally unbounded streaming
(V4) is the playable core and follows the package-driven spine (V2) immediately.
Complete terrain and caves (V5–V6) are content layers on that core. Durable
save/recovery (V3) still requires a sealed writer-activation receipt and lands
after streaming so eviction can become durable without blocking the first
unbounded traversal. Milestone numbers stay aligned with D0–D10; do not read
V3 as a prerequisite of V4.

### V0 — Freeze scope and preserve the current fixture

**Goal:** establish the v1 contract and a reproducible baseline without adding
more disconnected foundation surface.

Deliverables:

- Record the current finite playable fixture behavior as a smoke test.
- Define the v1 world header fields: vertical range, chunk edge, seed,
  generation epoch, required package closure, semantic image, and hard limits.
- Define the automated v1 journey assertions before production integration.
- Inventory every existing crate/package contract as reuse, adaptation, or
  replacement; do not create parallel abstractions.
- Freeze explicit non-goals and the D0-D10 mapping in the active roadmap.

Exit gate:

- A machine-readable fixture describes create, spawn, explore, enter cave,
  gather, craft, mine, place, checkpoint, exit, and reopen expectations.
- No v1 feature relies on the in-memory fixture authority as a production
  dependency.

### V1 — Close D0 package, lock, Nickel, and launcher execution

**Goal:** make the product boot path real before attaching world mutation.

Deliverables:

- Local path/catalog package `check -> pack -> publish-to-directory -> acquire`
  through immutable CAS, with no remote service required.
- Package-local Nickel aliases loaded only from the verified locked source
  table.
- Production worker containment, source spans, and fail-closed diagnostics.
- Final `latticeaxiom.lock` with source, package manifest, toolchain,
  `EngineBuildId`, artifact, registration, and required runtime receipts.
- Atomic write, flush, replace, reopen, corruption, offline, and `--frozen`
  validation.
- Launcher drives profile acquisition, resolution, registration compilation,
  `RuntimeImage`, and either client or headless App.
- Settings and observability are selected exactly once by the graph.

Exit gate:

- The same reopened final lock starts one `DefaultPlugins` client App and a
  GPU-free headless fixed-tick App without re-resolving.
- Missing/tampered source, manifest, toolchain, engine build, artifact, alias,
  or registration evidence fails before native loading or world access.
- The entire shipped local-package flow succeeds offline from a clean checkout.

### V2 — Replace the fixture with a package-driven minimal playable spine

**Goal:** obtain the first ugly but production-shaped playable build.

Deliverables:

- Construct the client scene from `RuntimeImage` and registered content, not
  built-in Terrenia identifiers.
- Integrate player movement, input, authoritative target DDA, break/place
  commands, block receipts, chunk revisions, voxel projection, chunk meshing,
  and colliders.
- Use `MemoryWorldStorage` behind the production storage interfaces for this
  milestone; do not claim durability.
- Materialize a small deterministic generated region through the real
  worldgen-to-storage-candidate path.
- Retire entity-per-block presentation from the production path.
- Keep the start UI, loading stages, HUD, inspect, and diagnostics connected to
  graph-selected providers.
- Do not wait for complete Terrenia terrain, caves, or the tools package.

Exit gate:

- Client input and headless command input perform the same move, inspect,
  break, and place sequence with identical authoritative receipts.
- Six faces, negative coordinates, collider replacement, mesh invalidation,
  stale-result rejection, and chunk revision behavior pass.
- No package, worldgen, gameplay, or presentation path contains a hidden
  Terrenia fallback.

### V4 — Sparse unbounded chunk streaming

**Goal:** make horizontal exploration operationally unbounded while content is
still sparse. Bounded residency and the chunk lifecycle are the playable core;
complete territories, caves, and hydrology are not.

Deliverables:

- Production `DimensionGenerationCoordinator` and explicit chunk lifecycle.
- Player/camera interest calculation, near-to-far priority, movement look-ahead,
  bounded load/generation/mesh/collider/save queues, cancellation, and
  backpressure.
- Storage-first load against the production storage interfaces. V4 may keep
  `MemoryWorldStorage`; it must not claim crash recovery or durable eviction.
- Absent-chunk generation, atomic commit of candidates, derived-job projection,
  neighbor invalidation, and eviction of clean generated chunks.
- Dirty edited chunks stay in a bounded dirty set and are not evicted until V3.
- Configurable vertical range and view distance with hard host clamps.
- Diagnostics for every resident/active/visible/in-flight class and byte budget,
  hooked to the inspect/HUD path from V2.
- Headless high-speed traversal fixture for positive and negative coordinates.
- Sparse D4 catalog and generator are enough. Do not block on V5 territories or
  V6 cave topology.

Exit gate:

- Long traversal continuously crosses newly generated chunks without an
  authored boundary, blocking frame I/O, holes caused by missing lifecycle
  transitions, or unbounded memory/queue growth.
- Clean generated chunks may be evicted and regenerated identically. Edited
  dirty chunks remain resident.
- Generation and projection remain deterministic under randomized task
  completion and chunk request order.
- Returning to an evicted *edited* chunk is not a V4 promise; that is V3.

### V3 — Authorize and integrate durable world lifecycle

**Goal:** make the already-streaming world recoverable. Streaming and sparse
generation already exist; this milestone does not invent unbounded traversal.

Security prerequisite:

- Obtain explicit authorization for a sealed catalog-to-world-db activation
  validation receipt. This plan does not grant that authority. Until it exists,
  `ActivationEvidenceUnavailable` remains the correct production result.

Deliverables after authorization:

- Validate exact lock, required packages, schema, semantic image, active bundle,
  world health, generation epoch, and recovery mode before opening a writer.
- Create/list/continue worlds; save player, chunk, inventory-ready schema, and
  generation evidence through atomic transactions.
- Implement checkpoint, crash marker, read-only recovery, export checksum,
  clone-before-migration, trash, and restore flows.
- Storage read always precedes generation; a materialized chunk is never
  regenerated because a provider changed.
- Durable eviction of previously dirty streaming chunks; revisit restores
  durable state rather than regenerating it.
- Low-disk and shutdown-timeout paths preserve the latest durable state.

Exit gate:

- Break/place edits and player position survive normal restart and crash
  recovery.
- Returning to evicted edited chunks restores durable state rather than
  regenerating it.
- Stale async work cannot overwrite a newer revision.
- Recoverable read-only mode never opens a writer.
- Package/lock/schema failure occurs before the first mutation.

### V5 — Complete natural terrain, geology, resources, and spawn

**Goal:** replace the minimal generator with the coherent Terrenia v1 surface
and underground plan.

Deliverables:

- Production multi-scale Territory Atlas with three surface and two underground
  territories.
- Terrain elevation/density, biome/style transitions, strata, geology, stable
  ore fields, vegetation, and simple features.
- Continuous river/basin planning across surface territory boundaries.
- Package-authored role/predicate binding to concrete blocks from the locked
  catalog.
- Safe deterministic spawn selection and readiness barrier.
- Generation provenance, planning-cell epoch freeze, old/new epoch transition
  policy, and diagnostic visualizers.

Exit gate:

- Same seed/lock/config/query range yields identical plan and chunk bytes across
  chunk order, parallelism, registration order, and restart.
- All required natural materials can be found by the automated journey.
- Territory, river, geology, resource, vegetation, and spawn boundaries have no
  unexplained seams.
- Old materialized chunks remain unchanged after a compatible provider update.

### V6 — Complete caves, hydrology, and fluid occupancy

**Goal:** make underground exploration a designed, traversable part of v1.

Deliverables:

- Default cave topology domain, two underground-owned subdomains, and one
  bounded branch contributor.
- Stable surface entrances, cross-domain portals, must-connect destination,
  local cave fields, shared-face validation, and final-occupancy arbitration.
- Underground river/drainage constraints, aquifer decisions, and initial
  water/lava occupancy integrated with the solid palette.
- Collider/mesh readiness rules that prevent entry into unloaded or unsafe
  cave space.
- Cave/hydrology inspect data and bounded visualizers.

Exit gate:

- Fixed headless inputs travel from surface through a required entrance into
  both underground territories and obtain three natural resource classes.
- Required portal and connectivity assertions pass across planning cells and
  topology domains.
- Water/lava boundaries, cave faces, and terrain transitions are deterministic
  and continuous.
- Cave generation, SDF evaluation, queue, and memory costs remain within the
  current hard limits and are ready for v1 budget freeze.

### V7 — Integrate generic sandbox mechanics and `@terrenia/tools`

**Goal:** turn exploration and block editing into a repeatable survival-free
resource/tool/build loop.

Deliverables:

- Runtime integration for inventory, hotbar, drops, pickup, mining progress,
  tool predicates, durability, recipes, workstation, placement items, and
  authoritative transactions.
- New `@terrenia/tools` package with the v1 contents defined in Section 7.
- `@terrenia/gameplay` graph dependency and non-overlapping namespace
  delegation.
- Crafting UI/command input, inspect fragments, and actionable rejection
  messages.
- Durable player inventory, selected slot, tool durability, dropped items, and
  any workstation/container state used by the journey.

Exit gate:

- Automated journey performs gather -> pickup -> craft workbench -> craft
  wooden tool -> mine stone -> craft stone tool -> accelerated mining -> place,
  with quantity, durability, and revision conservation.
- Full inventory, stale revision, wrong tool, broken tool, failed craft, crash,
  and unload/reload do not duplicate or lose state.
- A fixture dimension reuses the generic mechanics without `terrenia:*` host
  references.

### V8 — Finish Terrenia content, fluids, presentation, and world UX

**Goal:** expose the authored content through a coherent client experience
without altering authoritative behavior.

Deliverables:

- Exact 72-block and two-fluid catalog registration with complete references.
- Bounded water/lava updates, versioned solid/fluid palette, collision,
  selection, inspect, and persistence.
- Obtainable v1 building materials and workbench/furnace/chest/torch behavior
  required by the accepted D9 scope.
- Complete block/tool/fluid presentation assets and deterministic fallback
  presentation.
- Start page, create/continue, loading stages, health/lock/durability summaries,
  pause/save/exit, recovery actions, settings, inspect, and performance presets.
- Client-only presentation package remains removable in headless mode.

Exit gate:

- Every locked asset, drop, recipe, role, predicate, placement, and worldgen
  reference closes.
- Randomized break/drop/pickup/place coverage finds no missing definition.
- Solid/fluid state round-trips at positive and negative Y-up coordinates and
  chunk boundaries.
- Headless omission of presentation does not change authoritative registration,
  action receipts, snapshot bytes, or world hash.

### V9 — Freeze budgets, harden faults, and release v1

**Goal:** prove the complete sandbox journey is bounded, recoverable, and
reproducible from a clean checkout.

Deliverables:

- Release-profile baselines for frame, fixed tick, active/resident/visible
  chunks, worldgen, mesh, collider, save, tasks, I/O, FFI, RAM, and VRAM.
- Freeze exact budgets in the accepted roadmap/ADR evidence; this plan does not
  invent replacement numbers.
- Ten-minute fixed-path traversal and complete automated player journey.
- Normal, crash-recovery, checkpoint-restore, static-to-portable realization,
  and compatible-reopen journeys.
- Fault corpus for low disk, missing/tampered package, bad schema, bad ABI,
  device loss, stale async result, task panic, corrupt lock/header, and shutdown
  timeout.
- Linux and Windows CI/release evidence; use Linux CI as the independent root
  gate when the documented Windows Rust compiler crash recurs.
- Reproducible local offline release instructions and machine-readable evidence.

Exit gate:

- The complete Section 3 journey passes from a clean checkout and a reopened
  final frozen lock.
- Ten-minute traversal stays inside frozen budgets with no unbounded queue or
  residency growth.
- Static and portable realization journeys produce the same authoritative
  receipts, state hash, semantic report, and normative save bytes.
- Every failure preserves the latest durable recoverable world; presentation
  failures do not change world state.
- All root check/test/clippy/rustdoc, dependency/license, lock, headless smoke,
  client smoke, deterministic corpus, and release-evidence jobs pass.

## 11. Dependency order and parallel work boundaries

The critical path is:

```text
V0 -> V1 -> V2 -> V4 -> V3 -> V5 -> V6 -> V7 -> V8 -> V9
```

V4 is sparse unbounded streaming. V3 is durable recovery of that already-streaming
world. V5–V6 add complete terrain and caves on the same coordinator; they must
not replace V4's lifecycle.

The following work may proceed in parallel only while preserving the milestone
integration gate:

- `@terrenia/tools` authoring and generic gameplay conformance can proceed
  during V4–V6, but V7 is not complete until the real runtime journey uses it.
- Presentation assets and inspect/HUD fragments can proceed during V2–V7, but
  authoritative content IDs and schema must be frozen first.
- Cave algorithm prototypes can run during V4–V5 against the accepted topology
  contracts, but cannot bypass final occupancy, hydrology, or storage receipts
  and cannot delay V4 traversal.
- Fault fixtures and diagnostics should be added with each subsystem, not saved
  entirely for V9. Chunk-lifecycle inspect data is required at V4, not V8.
- Client visual work never blocks headless conformance, but both must consume the
  same authoritative lock closure.

Do not start content expansion beyond the accepted 72-block/two-fluid target
while a preceding production integration gate remains open.

## 12. Verification matrix

| Concern | Required evidence |
| --- | --- |
| Package/lock | Local offline package lifecycle, alias/import negative corpus, atomic lock fault corpus, frozen artifact/toolchain/engine verification |
| Client/headless equivalence | Same reopened final lock, same authoritative registration, command receipts, state hash, and save bytes |
| Determinism | Seed/lock/config goldens under randomized chunk order, task completion, registration order, negative coordinates, and restart |
| Infinite streaming | Sustained traversal, bounded high-water marks, eviction/revisit, cancellation, backpressure, and no world-edge dependency |
| Worldgen | Territory/biome/river/geology/resource/vegetation/spawn receipts and cross-boundary goldens |
| Caves | Portal/entrance/connectivity/passability/shared-face/fluid assertions plus traversal through two underground domains |
| Gameplay/tools | Conservation properties, wrong-tool/full-inventory/stale/fault cases, durability, recipes, drops, pickup, and placement |
| Persistence | Normal/crash/reload/checkpoint/read-only/export/migration/low-disk fixtures; materialized chunks never regenerate |
| Rendering/physics | Y-up six-face mesh, collider and DDA tests; stale derived work; device loss cannot affect world hash |
| Performance | Reproducible optimized benchmarks and machine-readable P50/P95/high-water evidence against frozen budgets |
| Supply chain | Locked metadata, dependency/license policy, artifact receipts, clean-checkout reproduction on supported targets |

Property and fault tests should operate below the full journey; the full journey
must exercise the same public production path rather than a test-only host.

## 13. Commit and review tranches

Implementation should be split into reviewable Conventional Commits. A
suggested sequence is:

1. `feat(compose): finalize local package acquisition and product lock`
2. `feat(launcher): boot client and headless images from reopened locks`
3. `feat(engine): integrate the package-driven playable spine`
4. `feat(runtime): add bounded sparse infinite chunk streaming`
5. `feat(world-db): validate sealed world activation receipts` — only after
   explicit authorization
6. `feat(worldgen): integrate Terrenia territory and materialization`
7. `feat(worldgen): realize cave topology and hydrology`
8. `feat(packages): add the Terrenia basic tools package`
9. `feat(gameplay): integrate durable sandbox transactions`
10. `feat(content): activate Terrenia v1 blocks fluids and presentation`
11. `feat(ui): complete world lifecycle and recovery surfaces`
12. `perf(runtime): freeze v1 traversal budgets`
13. `test(release): add the complete v1 journey and fault corpus`

Refactors, dependency upgrades, behavior changes, and performance tuning remain
separate commits. Every tranche must leave the currently reached client/headless
journey green.

## 14. Stop conditions and safety rules

- Do not replace `ActivationEvidenceUnavailable` with permissive behavior.
- Do not open a world writer before exact-lock and catalog activation receipts
  validate.
- Do not allow worldgen workers to write storage directly.
- Do not regenerate a durable materialized chunk because a package/provider was
  updated.
- Do not claim infinity by pre-generating a large finite map or retaining all
  visited chunks.
- Do not add project-owned ECS, scheduler, renderer, physics solver, asset
  runtime, or thread runtime.
- Do not move Terrenia block, biome, cave, tool, recipe, or drop defaults into
  host crates.
- Do not accept component-only evidence as a playable milestone.
- Stop and require an ADR or explicit authorization when a change alters writer
  authority, persistent compatibility, trust policy, ABI, world-coordinate
  representation, or frozen performance budgets.

## 15. v1 release definition

v1 is complete only when all of the following are true:

- A local package manager flow produces and reopens the exact final product
  lock without network access.
- The same lock drives the production client and headless authoritative closure.
- The launcher and start UI create, preflight, open, save, close, recover, and
  reopen a real world.
- Horizontal exploration is operationally unbounded and all live work remains
  bounded.
- Terrenia provides coherent surface terrain, underground territories,
  geology, resources, vegetation, rivers, caves, hydrology, safe spawn, 72
  blocks, and water/lava through packages.
- `@terrenia/tools` provides a complete wooden/stone pickaxe, axe, and shovel
  progression with crafting, mining, drops, inventory, hotbar, and durability.
- The automated journey explores, gathers, crafts, mines, builds, checkpoints,
  exits, and reopens with exact durable state.
- Determinism, security, persistence, recovery, performance, supply-chain,
  client, and headless gates pass from a clean checkout.
- No host/platform/foundation package contains Terrenia-specific defaults.
- The old finite in-memory playable fixture is no longer the product entry path.

Anything less is a useful component or intermediate playable slice, but not the
Lattice Axiom v1 game.
