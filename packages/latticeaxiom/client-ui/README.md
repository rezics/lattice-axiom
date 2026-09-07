# `@latticeaxiom/client-ui`

Owns renderer-neutral UI contracts and the optional `bevy-ui` desktop style.
The shared sRGB palette is converted to linear colors for headless theme tokens;
Bevy nodes consume the same palette. UI is drawn with code and uses system UI
fonts without raster skin assets.

Implementation entry and membership are declared in `latticeaxiom-package.toml`.
Public APIs and tests are maintained beside the code.

The optional native widget adapter retains its own InputFocus and tab order.
Terrenia now uses `@latticeaxiom/web-ui` for menus, settings and inventory;
this package's surface contracts still own gameplay suppression and route state.

Validation: `cargo test -p latticeaxiom-client-ui -p latticeaxiom-engine --lib --all-features`.
`LATTICEAXIOM_CAPTURE_PATH` captures the Bevy render target, which excludes the
OS-composited WebView. It is scene/GPU evidence, not a screenshot of the complete
desktop UI. Use whole-window inspection for WebView transparency and input.
Captures are local QA artifacts and are never committed.
