# Rebuild execution

The accepted package-first implementation is now maintained on `main`.
The merged `codex/package-first-rebuild` branch and imported `demo/main`
reference were removed on 2026-09-06 after a verified external Git bundle.
Historical execution notes below retain the branch names used at the time.
The user's resource-pack and text-only commit amendments take precedence over
the original proposal's asset delivery and future asset-management suggestions.

The accepted [ecosystem direction](docs/ecosystem-direction.md) and
[package interface design](packages/latticeaxiom/sdk/docs/package-interfaces.md)
were recorded on 2026-09-07. Their E1-E5 gates are additional pending ecosystem
acceptance; the historical rebuild checkmarks below do not establish them.

The native-only UI decision below was superseded on 2026-09-07 by the
[embedded Web UI delivery](docs/web-ui-delivery.md). Desktop menus, inventory,
settings and diagnostic panels move to WebView; the crosshair and mining
progress remain native. Historical native-UI checks do not certify the Web UI.

## Recovery baseline

- Documentation source: `57c37e2334ea019521fcd85237cb8dba47d51d24`.
- Implementation source: `3fa521ecb663ca076f460f86b746e5b016e58660`.
- Both bundles were verified and cloned into independent no-checkout repositories;
  both source commits were verified in the restored repositories.
- External recovery directory: `D:/rezics-repos/lattice-axiom-recovery/20260905-package-first`.
- The demo's 19 runtime files (49,260,348 bytes) were copied separately. The demo
  checkout and its existing benchmark worktree remain the comparison baseline.
- No new frame-time or visual acceptance is claimed by this recovery checkpoint.

## Delivery gates

- [x] Recovery bundles and runtime-data backup.
- [x] Package-owned workspace and text-only repository checks.
- [x] Multi-crate source realization and frozen-source link conformance.
- [x] Frozen manifest-selected product assembly used by the actual client.
- [x] Independently selectable texture/shader resource graph and runtime consumers.
- [x] Code-designed shell/settings style, native focus, editable name, and initial GPU captures.
- [ ] Streaming measurements, corrective changes, and acceptance.
- [x] Physical world creation, save, process replacement and independent reopen.
- [ ] Complete gameplay journey and compatibility/recovery acceptance.
- [x] Both original Git histories converge without restoring the old tree.
- [ ] Final documentation consolidation and release checks.

## UI research decision

Retain Bevy 0.19.1. Use its Flexbox/Grid `Node`, text, borders, gradients and
shadows with shared design tokens. Use native widget/focus behavior, including
visible keyboard focus and disabled states; widgets are experimental, so keep
the version locked and put styling in our UI package. No webview, HTML runtime,
React runtime, or raster UI skins are introduced.

- https://docs.rs/bevy/latest/bevy/ui/index.html
- https://docs.rs/bevy/latest/bevy/ui_widgets/index.html
- https://docs.rs/bevy/latest/bevy/input_focus/index.html
- https://learn.microsoft.com/en-us/gaming/accessibility/xbox-accessibility-guidelines/102

Use at least 4.5:1 contrast for ordinary essential text, a visible focus shape
in addition to color, scalable layout/text, and mouse/keyboard parity. Test the
actual native UI; headless widget tests do not claim visual acceptance.

## Package-owned workspace validation

All 29 members compile with `cargo check --workspace --all-targets --offline`.
The composer library passed 106 existing tests and two new Rust source-owner
validation tests. Repository boundary tests reject disguised binary content and
media extensions. The import excludes all 201 tracked PNG files.

The old smoke artifact is saved in the recovery directory; it is a development
headless smoke result, not an optimized or GPU performance certification.

## Nested package build validation

SourceBuild honors the package manifest Rust entry. NativeStatic plan schema 2
records the primary manifest and all internal crates. Six generated-product
tests and four source-build tests passed, including a two-crate package whose
text resource is read from frozen CAS after the mutable original changes.
`cargo clippy -p latticeaxiom-compose --all-targets --all-features --offline -- -D warnings` passed.
These link proofs do not yet claim production-client activation.

## Independent resource graph validation

Client resources resolve through `profiles/resources.toml` to a separate lock.
A resource-only relock and frozen verify succeeded and left the gameplay lock
SHA-256 unchanged. Material patterns feed near terrain, distant color and item
previews; independently locked WGSL replaces the versioned water shader.
Four Python boundary tests, two resource overlay tests, all 176 engine library
tests and engine/render-contract clippy passed. GPU presentation is still pending.
No binary asset was imported or committed.

## Code-designed UI validation

The native 1280x720 home and 960x640 create screens were captured and inspected.
A real Bevy KeyboardInput message entered Chinese text into the creation form.
Create/back actions remain outside the scroll region, profile choices wrap, and
focus scrolling reacts to focus changes rather than fighting mouse scrolling.
All 18 client-ui and 177 engine library tests passed, including contrast and
Unicode editing checks. Clippy passed for both crates with all features/targets.
Screenshots remain ignored under `.temp/qa/`, never committed.

## Physical world and process lifecycle validation

The interactive shell now opens `DiskWorldStore`, lists only catalog metadata,
creates a physically published world, and hands off the exact selected world ID.
The game reads that world's image, flushes player/chunk state through the sealed
writer, closes its lease, and commits to disk before reporting Save & Quit.
Errors stay visible for retry; a failed event loop preserves the last good image.
Shell and gameplay hashes remain distinct. Normal exits retire only completed
launcher control files; existing worlds are never removed by the acceptance tools.

Validation completed on 2026-09-05:

- The fresh `physical_world` acceptance composes isolated locks, closes all original
  engine/store handles, and independently reopens the file with the same world,
  inventory count and player position. Passed with no ignored cases.
- The native driver created a world from an empty library and completed two
  separate shell/world/shell/quit runs. Both remained in the world for 35 seconds,
  exceeded the supervisor's observation window, and returned three hops with zero
  failures. The same world advanced from durable revision 1 to 2. Two completed
  control directories were archived; no manual cleanup occurred between runs.
- Local logs and JSON reports are ignored under
  `.temp/qa-native-lifecycle-20260905-2/`; no media or local world data is committed.
- All 178 engine, 63 launcher, 30 world-db and 7 start-ui library tests passed.
  Default-feature Clippy passed for all four crates and their targets.
- The real native run exposed and corrected invalid shell report fields,
  consumed-intent rollover, startup-time clock reuse, and treating ordinary
  continued play as a shutdown timeout. Regression tests cover these boundaries.

The optional engine `acceptance` feature enables fresh Nickel fixture composition;
ordinary client tests do not link the evaluator. No engine dependency was upgraded.

## Remaining acceptance limits

The older `v1_complete_journey` suite contains an ignored D10 test and hard-coded
gap reports. Its three passing metadata tests are not full gameplay acceptance.
The physical test deliberately seeds an inventory fixture; it does not prove
natural gathering, crafting, building, machines, or progression.

The old optimized traversal harness failed inside rustc (access violation, then
illegal instruction). Further development compiles also faulted inside
`rustc_driver`. For this functional acceptance only, command-line overrides set
`profile.dev.package.latticeaxiom-engine.opt-level=0` and, for fresh lock tests,
`profile.dev.package.nickel-lang-parser.opt-level=0`; dependencies otherwise kept
their normal profiles. These local overrides are not project profile changes.
Functional tests and native lifecycle passed, but no optimized or reference-host
performance certification is claimed. The physical backend still publishes
complete snapshots and has not passed large-world save/streaming budgets.

## Native ownership and frozen product work in progress

Generic lock preparation moved to `@latticeaxiom/host`; the Terrenia application
now lives under `@terrenia/client`. Thirty Rust members have fifteen unique
package owners. Package manifests explicitly declare forty-two acyclic native
build edges and source entry/membership. The redundant authored crate/package
inventory was removed; ownership and license checks read real manifests.

The manifest graph, source containment and dependency-free frozen compile tests
pass. The composer passed 144 all-feature library tests and eight shipped-source
conformance tests. The moved application passed 178 library tests. Its broader
headless suite completed with 49 passes and six failures in 780.43 seconds with
unoptimized project code. Failed cases remain open:

- `production_host_streams_past_v2_neighborhood_in_two_horizontal_directions`
- `durable_save_and_quit_restores_unpicked_drops`
- `render_mesh_backpressure_eventually_fills_every_resident_render_slot`
- `production_host_reaches_both_underground_territories_and_three_resource_classes`
- `negative_coordinate_eviction_revisit_restores_identical_clean_chunk`
- `retain_keeps_former_core_after_immediate_boundary_reversal`

These expose traversal, resident-set completion and fixture terrain assumptions;
no cause or optimized performance pass is inferred from the unoptimized run.
Full native-product compilation also faulted in rustc on render-contracts (even
at opt-level 0) and leafwing-input-manager. Build environment stability remains
an explicit acceptance limitation, not a reason to count unfinished gates as done.

The first full frozen native product build completed from the generated source
root using one build job, project opt-level 0 and a process-local CPU affinity
mask selecting logical CPU 24. This is a functional build workaround, not a
hardware diagnosis or a performance acceptance. The frozen binaries completed
two native sessions with the same saved world and no failures; durable revision
was 1 in both runs. Evidence is local under `.temp/qa-frozen-native-20260905/`.
All-feature Clippy passed for host, application, composer and package resolver.
The 12 Python repository/build checks passed, including real frozen compilation.

Cache cleanup is incomplete: automatic approval review rejected deletion of
unused files copied into `target/native-products/debug/deps`, reporting only
`blocked by policy`. No deletion was executed. Approximately 334.6 GiB of copied
older cache variants remain, in addition to about 7.1 GiB selected by the build.
The original demo cache and saved worlds were not deleted or moved.

The frozen product path is now the normal `task play` / `task dev:plain` path.
The latest source snapshot (`bf9bbc2336692f42...`) rebuilt successfully in the
stable materialized build directory after the final lint fixes. Both binary
hashes and the compiler/Cargo/lock/input receipts are generated locally. Release,
portable ABI, broad gameplay and optimized streaming gates remain open.

## Survival defaults and original-lock continuation

`task play` now selects `profiles/play.toml`: ordinary survival rules, empty
initial inventory and no developer working-set overlay. `task dev:plain` retains
its explicit creative/developer profile. Advanced crafting no longer binds a
remote workstation from the keyboard; hand crafting remains available.

Lock transactions retain both the previous and new verified lock in the catalog.
The shell and supervisor select a saved world's archived lock instead of silently
reinterpreting it with the latest default. The headless physical acceptance
changes the default from survival to developer mode between close/reopen and
verifies the original inventory, pose, handoff hash and survival rules. It passed.
145 composer library tests, eight source/profile conformance tests and all-feature
Clippy passed. Native acceptance then performed the same default switch across
two independent supervisor runs: both retained the same world, original lock,
`survival` mode and an empty inventory. Local evidence is under
`.temp/qa-survival-state-20260906/`.

## Optimized traversal observation and measurement corrections

The optimized headless bench now builds with the pinned release profile. Build
process affinity was limited to logical CPU 24 to work around the compiler
failures; the measurement process itself ran with the normal CPU availability.
The final run completed 7,200 warmup and 36,000 measured ticks in 106.881 seconds,
visited nine player chunks, and observed return motion. Fixed-update P95 was
5.4374 ms, P99 7.8262 ms and maximum 33.3293 ms. Resident occupancy reached 1,183;
queued mesh jobs peaked at 128, combined jobs at 142 and reserved derived bytes
at 184,417,912. The evaluator reports no failing measured metric and an overall
`insufficient-evidence` result, not a release/GPU certification.

The harness previously double-reversed movement after the 180-degree turn,
carried excess warmup input into measurement, reported a constant seed of 42,
and compared queued-plus-running jobs against a pending-only cap. These are
corrected. Empty mesh buffers no longer count as prepared geometry. Headless
prepared geometry is explicitly unsupported as GPU/frustum visibility evidence;
the GPU-visible budget remains unchanged at 512. A stationary run now fails the
traversal check. Twelve performance-runner tests and engine Clippy passed.

Local observations are `.temp/performance-optimized-traversal-final.json` and
`.temp/performance-optimized-evidence-final.json`. They do not measure GPU frames,
physics substep duration, cold-load latency or durable-save latency. The six
broader headless failures listed above and the complete gameplay/recovery gates
remain open. No old failed or partial run was relabeled as a pass.

## History convergence

Commit `5953aee` joins the already imported demo history as a second parent.
Both original baseline commits are verified ancestors. The merge used the
already rebuilt tree, and its tree ID was checked to be identical before and
after the join. Historical source commits remain available for review/recovery;
no old directory or binary asset was restored to the current source tree.
This local branch has not been pushed and does not claim the open gameplay,
GPU, recovery, or release gates are complete.

## Concurrent catalog publication and development startup

The original `task dev` preparation used Task dependencies, which execute in
parallel. Game, shell and resource composers shared a fixed `<digest>.tmp` file;
one writer could rename or remove another writer's temporary file. The reported
destination then misleadingly appeared to be missing. Three cold-catalog runs
of the original executable reproduced the failure in all three runs.

CAS objects and archived product locks now use unique temporary files in the
destination directory and no-clobber publication. Identical concurrent winners
are accepted; conflicting bytes fail without replacing the existing object.
The publisher synchronizes payload bytes, without claiming a cross-platform
directory durability barrier. `task dev`, `task dev:plain` and `task play` also
prepare their three locks sequentially through `task dev:prepare`.
The implementation follows the upstream contracts for
[Task command ordering](https://taskfile.dev/docs/guide) and
[no-clobber temporary-file publication](https://docs.rs/tempfile/3.27.0/tempfile/struct.NamedTempFile.html#method.persist_noclobber).

The fixed executable passed all nine lock commands across three fresh shared
catalogs, all nine frozen verifications, and digest checks on all 48 objects in
each catalog. Evidence is under `.temp/dev-cas-before-pcv3yziu/` and
`.temp/dev-cas-after-ypoa34km/`. Deterministic simultaneous publication and real
three-process cold-catalog regressions are kept beside the composer.

The wider checks exposed five obsolete shipped-source assertions from before
the package/crate and trust-model migration. They now check package-local crates,
matching entry/manifest ownership, and the separate data/native realizations.
All 184 composer tests, 44 package resolver tests, 14 repository/graph/Logdy
Python tests, and all-target/all-feature Clippy for both Rust crates passed.
The functional test build used project opt-level 0, one build job and a
process-local affinity mask selecting logical CPU 24 after the ordinary test
compiler faulted with `STATUS_ACCESS_VIOLATION`. This does not change the pinned
toolchain or repository optimization settings.

The actual `task dev RUNTIME=<isolated QA directory>` command then passed with
the repository's default optimized development profile. The initial cold build
hit the QA harness's 900-second deadline while compiling the application; the
retry reused completed dependencies and used a longer harness deadline. The
build still used one job, disabled incremental compilation and the process-local
CPU affinity workaround. The Logdy page returned HTTP 200. The native product
completed shell -> creative world -> durable save -> shell -> normal quit with
exit code 0, durable revision 1 and no supervisor failures. The QA world remained
active for at least 35 seconds. Original runtime world files were unchanged;
all generated QA state is under `.temp/qa-task-dev-cas-_hj9rt52/`. This is a native
functional acceptance, not a GPU/frame-time performance certification.

After the user's explicit cleanup authorization, deletion of the reviewed
unused files in `target/native-products/debug/deps` was attempted again with
resolved-path containment and a retained-artifact list. Automatic approval
review again rejected it before execution with `blocked by policy`. No files
were deleted; approximately 334.6 GiB of the copied older cache variants remain.
This is an execution-policy block, not a pending request for user authorization.

## Terrain corrections and toolkit research (2026-09-06)

The client now has a bounded native FPS/frame-time/P95/main-CPU monitor, plus
fixed-identity, copied-world capture tooling. The current host did not reproduce
the reported severe performance problem; the user also confirmed normal
performance today. No general performance-regression resolution is claimed.

Inspection found per-column stochastic biome mixing across wide transition
bands, uphill differences treated as maximum descent, and inconsistent sRGB
handling between near textures and far vertex colors. New frozen worldgen data
selects geology revision 4 and transition revision 9. Archived locks without
that policy retain geology 3 and transition 8. Existing saves were not rewritten.
Unchanged terrain visibility ranges and hidden UI no longer invalidate Bevy
state each frame.

Validation: 182 engine and 87 worldgen unit tests passed. Five far-mesh tests
were rerun after the final fallback-color correction. All-target Clippy for
engine/worldgen passed with client/development features and warnings denied.
Optimized native products completed create/save/reopen in two independent
sessions with the fixed world identity
`59cf1f69-6774-43a7-8353-0d0c034b7432`. The 21-chunk, 1280x720 overview shows a
continuous grass/sand boundary instead of scattered columns. The 45-second
capture observed about 201 FPS after its ten-second warmup; this one case is
not a general GPU certification. Source QA databases remained byte-identical.
Evidence is local under `.temp/qa-terrain-fixed-world/`,
`.temp/qa-terrain-fixed-overview/`, and `.temp/terrain-*-final.log`.

The user confirmed a long-term toolkit direction whose primary consumers are
Lattice packages, with independent release capability. The research document
maps academic methods and real engineering practice to current code, identifies
the storage/territory/worldgen extraction cycle, and proposes staged contracts,
tooling, algorithm packages and release gates. It does not claim those proposed
packages have already been extracted or published:

- `packages/latticeaxiom/world/docs/worldgen-toolkit-research.md`
- `packages/terrenia/client/docs/terrain-debugging.md`

`main` was fast-forwarded without rewriting either imported history. The merged
rebuild branch and unused `demo/main` reference were removed. A complete verified
bundle is retained outside the repository at
`D:/rezics-repos/lattice-axiom-recovery/20260906-worldgen/before-ref-cleanup.bundle`.
The original demo checkout remains intact. No push or registry publication was
performed. The older gameplay, recovery and release gates remain separately
tracked rather than being inferred from this terrain slice.
