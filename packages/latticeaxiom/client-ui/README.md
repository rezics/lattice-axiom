# `@latticeaxiom/client-ui`

Owns renderer-neutral UI contracts and the optional `bevy-ui` desktop style.
The shared sRGB palette is converted to linear colors for headless theme tokens;
Bevy nodes consume the same palette. UI is drawn with code and uses system UI
fonts without raster skin assets.

Implementation entry and membership are declared in `latticeaxiom-package.toml`.
Public APIs and tests are maintained beside the code.

Native widgets and InputFocus own keyboard activation and tab order. Focus has a
visible outline. Shell creation supports bounded Unicode text and IME commits;
form actions remain reachable outside the scrolling content region.

Validation: `cargo test -p latticeaxiom-client-ui -p latticeaxiom-engine --lib --all-features`.
Native captures use `LATTICEAXIOM_CAPTURE_PATH`, optional `LATTICEAXIOM_CAPTURE_ROUTE`,
`LATTICEAXIOM_CAPTURE_SIZE` and `LATTICEAXIOM_CAPTURE_TEXT`. Captures are local QA
artifacts and are never committed.
