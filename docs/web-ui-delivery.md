# Embedded Web UI delivery

Accepted direction: 2026-09-07. This implementation replaces the desktop's
native menus and panels with package-owned Web UI in the existing Bevy window.
Crosshair, mining progress and world-space visuals remain native-first.

## Ownership

`@latticeaxiom/webview` owns the native embedding and bounded transport.
`@latticeaxiom/web-ui` owns the browser SDK and first-party React presentation.
Terrenia owns adapters to its shell, inventory, crafting, diagnostics and input.
Package-authored settings are projected from the selected typed catalogs.
Resource packs may replace presentation definitions; executable JavaScript is
code and must not enter authoritative state through resource selection.

Bevy keeps the window, ECS, scheduler, gameplay and persistence. Web UI receives
bounded snapshots and submits validated commands. A WebView reload does not
reload a world or grant authority to stale requests. UI animation and transient
editing state are local; item quantities and durable settings stay host-owned.

## Required visible behavior

- All shell routes and settings use Web UI, preserving world lifecycle actions.
- Inventory, crafting, item search, hotbar and pause use Web UI.
- F3 toggles structured diagnostics rather than permanent debug text.
- Target information is absent without a crosshair target and defaults to the
  top center of the game viewport.
- Module settings expose owner, schema constraints, defaults, apply/cancel and
  persistence. Unsupported authority remains visibly read-only.
- Mouse capture, keyboard navigation, controller input and IME have one owner.
- Production assets load locally; browser fixtures are explicit development
  inputs and never substitute for a failed native bridge.

## Acceptance

Protocol/domain checks, browser presentation checks and native window checks
are separate evidence. Native acceptance must cover transparent HUD input,
inventory movement, world entry/save/return, IME, focus loss and DPI changes.
Performance comparisons include the native baseline, empty WebView, idle HUD
and inventory; frame pacing and total browser-process memory are measured.

This decision supersedes the native-only UI choice recorded in
`REBUILD_PROGRESS.md` and `REBUILD_PROPOSAL.md`; historical results remain history.
Windows is the first native backend acceptance target. Each further platform
requires its own embedding and input evidence.

## Delivered boundaries

The first-party UI uses the existing semantic shell actions and domain methods,
including separate checkpoint-save and save-return operations. The bridge
rejects stale sessions, deduplicates request IDs and checks inventory revisions.
Package-owned endpoints may be explicitly registered through
`EngineInstance::register_web_endpoint`; the generic transport registry itself
does not depend on Bevy. This does not claim portable-native module loading.

Selected packages can declare `data/settings-v1.json`. Catalog compilation
checks the declaring owner, and the UI projects constraints, predicates and
authority. User values and input bindings publish in one durable transaction;
failed publication rolls back reversible runtime effects or requires restart
when visibility is uncertain. World-scoped authority is never granted by UI.

Inventory names, categories, recipes and resource colors are cached per world.
Item and recipe pages have row and byte budgets. Known UI theme colors can be
provided by client resource materials named `latticeaxiom:ui/<token>`; arbitrary
CSS, URLs and executable resource-pack code are not accepted through this path.

Windows child-WebView composition removes `WS_CLIPCHILDREN` from its parent for
the view lifetime and restores it on teardown. Native keyboard-focus proof is
reconciled with the Bevy input state after child focus transfers.

## Verification record

- Browser build and six protocol/extension/theme tests pass.
- Engine library suite passes 184 tests; repository tooling passes 22 tests.
- Workspace compilation passes for all targets, and frozen native source
  preparation validates the selected Web and Rust package closure.
- Real WebView2 smoke verifies local scripts, IPC, clipping-style lifetime and
  foreground-focus behavior. Runtime strict Clippy passes.
- In an isolated copy of the runtime, native mouse navigation, Chinese text
  entry, world creation, actual inventory movement, module-setting persistence,
  checkpoint save and durable save-return were exercised.
- Initial GPU-window testing found white child-window composition and a stale
  input focus gate. Both have code fixes and require final user window testing.
- The user requested to take over further native visual/input debugging. Final
  scene visibility, gameplay capture, Escape, DPI/fullscreen, controller and
  optimized performance acceptance are therefore not claimed by unit tests.
  Direct window close also needs manual verification: the shell now tracks the
  live world session so its teardown saves a world opened after app startup.

The opt-in lifecycle harness uses domain commands rather than removed native
widgets. Same-window lifecycle verification requires a fresh nonce-bound,
checksummed receipt emitted after actual durable disk publication; the existing
launcher shell-role receipt restrictions remain unchanged. Scene captures
explicitly exclude the WebView and cannot stand in for whole-window inspection.

Incremental rustc code generation crashed once on Windows. Rebuilding with
`CARGO_INCREMENTAL=0` completed successfully; no compiler/toolchain upgrade was
made. Original saved-world bytes were unchanged during isolated acceptance.
