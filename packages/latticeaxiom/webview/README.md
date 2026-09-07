# Native WebView transport

This client-only source package embeds one Wry 0.56.1 / Windows WebView2 child
inside a product-owned native window. It does not own game schemas, activation,
world state or a second event loop. Keep `WebViewHost` in a Bevy non-send resource
on the winit event-loop thread, and drop it before dropping the parent window.

`AssetBundle::from_directory` prepares an immutable app snapshot. `discover`
checks `LATTICEAXIOM_WEB_UI_DIR`, then `client-ui` beside the executable, then the
first-party source build in debug builds only. Distributions must ship their
built HTML/JS/CSS directory and install Microsoft WebView2 Runtime. Runtime
installation is a product installer responsibility, never a hidden download.
The writable WebView2 profile lives in `%LOCALAPPDATA%/LatticeAxiom/WebView2/`
under the executable's name, or `LATTICEAXIOM_WEBVIEW_DATA_DIR` when explicitly
configured. It is not written beside installed binaries or inside saved worlds.

Native products select their UI through pinned `[web.dependencies]` in their
Lattice manifest. Web source owners declare `[web]` with `entry`, `package`,
`lock` and `output` paths; npm packages and lockfiles remain build inputs, while
Lattice manifests own the package selection. `native_product.py --prepare-only`
captures the source closure without installing Node dependencies. A full build
installs and builds a separate source copy, then publishes an executable-adjacent
`client-ui` bundle and records every output hash in the product build receipt.
These frontend hashes do not alter a saved world's authoritative gameplay lock.

The local `lattice://localhost` protocol is mapped to an HTTPS localhost origin
by Wry. Only this exact origin may navigate or send IPC. External windows,
downloads, device permissions, frames and network resources are denied. Bundle
loading rejects path escapes and symlinks and bounds asset count and size.
Requests read the immutable in-memory map, never arbitrary disk paths. Trusted
UI source packages share a page and are not isolated from one another.

The page installs `window.__latticeReceive(envelope)` and then posts its product
`ready` request using `window.ipc.postMessage(JSON.stringify(request))`. The host
drains typed requests in its scheduled phase and delivers state/result envelopes
with `send_state`. Queue limits are 256 requests, 64 KiB per request and 2 MiB per
outgoing envelope. Rejected ingress increments `dropped_commands`; products must
use request timeouts and may report this diagnostic. Gameplay remains Rust-owned.

`EndpointRegistry<C>` lets a native package install its own fully qualified
methods, such as `@example/metrics:read`, without adding a core gameplay enum.
It validates the declared owner namespace, rejects duplicates, and projects
metadata in stable method order. `lookup` returns a copied function pointer,
allowing a Bevy caller to release a registry resource borrow before invoking the
handler with `World`. Handler parameter validation and domain authority remain
with the owning package and product. The registry does not execute handlers,
activate systems on import, or sandbox trusted source extensions.

The `package_endpoints` example contains a reusable metrics library and an
independent consumer with explicit installation. Run it with
`cargo run -p latticeaxiom-webview --example package_endpoints`.

`set_interactive(false)` disables the Wry child HWND and focuses its parent. A
disabled child does not receive native mouse/keyboard input; it remains visible
as a HUD. This differs from CSS pointer-events, which cannot route input through
native windows. `set_interactive(true)` enables and focuses the WebView for menus
and text entry. Call on mode transitions, not every frame or while refocusing an
inactive application. Physical resize dimensions avoid DPI double scaling.

The parent composition guard clears winit's default `WS_CLIPCHILDREN` for the
host's lifetime. Otherwise Windows removes the child's whole rectangle from the
GPU parent surface and transparent HTML reveals a blank area instead of the game.
A scoped native subclass preserves this policy across fullscreen/style changes;
teardown restores the original clipping bit without replacing unrelated styles.
See the upstream GPU-overlay diagnosis in https://github.com/tauri-apps/wry/issues/1212.
`window_has_focus` checks exact OS keyboard focus separately from application
foreground ownership, so products can reconcile delayed framework focus events
when moving between a web panel and captured game input.

## Verification boundaries

Unit tests cover exact origins, path traversal, snapshot immutability and queue
bounds. They do not establish transparent GPU composition, input forwarding,
IME behavior or frame cost. Product acceptance must check real game windows in
both modes, Alt-Tab, Chinese IME, resizing, DPI changes, fullscreen transitions
and WebView2 failure handling. Non-Windows construction returns an explicit
unsupported-platform error until platform integration is implemented.

Upstream references studied for this implementation:

- https://docs.rs/wry/0.56.1/wry/struct.WebViewBuilder.html
- https://docs.rs/wry/0.56.1/wry/trait.WebViewExtWindows.html
- https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-enablewindow
- https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/threading-model
