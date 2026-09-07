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
