# Rebuild execution

The accepted proposal is being implemented on `codex/package-first-rebuild`.
The user's resource-pack and text-only commit amendments take precedence over
the original proposal's asset delivery and future asset-management suggestions.

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
- [ ] Documentation consolidation, release checks, and history convergence.

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
