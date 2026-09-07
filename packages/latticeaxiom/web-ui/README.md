# Web client interface

This package owns the React client, framework-independent TypeScript bridge, and
trusted package panel registration API. Bevy owns gameplay state and native
crosshair/mining rendering. JavaScript imports do not activate world systems.

## Development and release

Run `npm ci`, `npm run build`, and `npm test` from this directory. `npm run dev`
starts the interface at `http://127.0.0.1:1420`. A browser without the native host
shows a connection screen. Explicit development fixtures are available with
`?fixture=shell`, `inventory`, `catalog`, `settings`, `debug`, `empty`, `loading`,
`saving`, or `save-error`; add `&scale=2` to verify scaling. Production builds
eliminate fixture code. Fixtures never access or modify saved worlds.

The product build uses the `[web]` manifest entry and ships the resulting `dist`
files beside its executable. Generated bundles and installed dependencies are
ignored and never included in source receipts. All runtime resources are local.

## Bridge and authority

The native host calls `window.__latticeReceive` with version-one snapshots,
request results, or semantic navigation. The page sends `ready` after installing
the receiver. Every state-changing request includes the active session and
revision. The bridge rejects responses after a session change, bounds pending
requests to 64, and exposes operation failures. UI-only search, hover, focus,
and stack selection remain local; inventory and setting values are host-owned.

`src/types.ts` documents the wire boundary. `ClientBridge` is independent of
React; `registerPanel` accepts package-qualified contribution IDs and lifecycle
callbacks. Products explicitly import and register their selected code packages.
Registered code shares the page's trust; this API is not a JavaScript sandbox.
The example in `examples/package-panel.ts` registers only when called by a
product. Registered panels can be opened from the package panel navigation;
their mount/update/dispose callbacks receive the same bounded client bridge.
Package endpoints may return JSON values without depending on React.

The interface preserves semantic world management actions, supports inventory
dragging and keyboard-accessible two-step movement, and groups real settings by
owner/category. Target information appears only when a target exists, centered
along the top edge by default. F3 diagnostics render only host-supplied values.
Catalog searches and page changes use the host's cached full catalog. The page
never truncates an already paginated host response. Applied input bindings drive
game shortcuts while native DOM traversal handles ordinary form controls.
Theme updates accept a fixed set of plain color tokens only, and UI scale uses
container-based reflow independently of window DPI.

## Design and acceptance

Code-native charcoal surfaces, mint focus/accent, flat world lists, and a compact
game HUD share typography and spacing. Text remains selectable in input controls;
modal focus is visible and bounded. There are no binary art assets or remote
fonts. Browser QA is separate from native transparency, mouse capture, IME,
controller routing, and GPU acceptance, which require the real game window.
