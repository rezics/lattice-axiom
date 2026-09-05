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

The package manifest defines its implementation entry and owned crates. Tests
and public rustdoc live with the implementation. `task play` launches the current
supervisor; generated frozen product assembly is tracked in the rebuild gates.
