# `@terrenia/client`

Owns the Terrenia application: Bevy plugin setup, shell/world process entry,
gameplay integration, terrain streaming/presentation and in-game controls.
Its internal crate currently retains the `latticeaxiom-engine` Cargo name for
existing developer commands and conformance callers. That name is not its
Lattice package identity.

Generic lock preparation and artifact verification live in `@latticeaxiom/host`.
World/storage, gameplay rules, voxel algorithms, input and shared UI contracts
remain in their respective packages. Resource packs remain independently
resolved client presentation inputs.

The desktop mounts `@latticeaxiom/web-ui` through `@latticeaxiom/webview` inside
the existing Bevy window. `host/web.rs` validates session-bound commands;
`host/web_projection.rs` caches catalog presentation and pages visible data;
`host/web_settings.rs` owns real catalog-backed settings and binding drafts.
Native aim and mining visuals remain in `host/hud.rs` and `host/mining_ring.rs`.

The package manifest defines its implementation entry and owned crates. Tests
and public rustdoc live with the implementation. `task play` launches the current
supervisor after building the manifest-selected frozen native product.
