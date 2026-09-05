# Package-owned native sources

Lattice package manifests own implementation identity and build dependencies:

```toml
[rust]
entry = "crates/application/Cargo.toml"
members = ["crates/application/Cargo.toml", "crates/model/Cargo.toml"]

[rust.dependencies]
"@latticeaxiom/host" = "=0.1.0"
```

`entry` selects the package's implementation entry; `members` declares every
internal crate. `rust.dependencies` names the other source owners required by
normal/build dependencies, including optional and platform-specific Cargo edges.
The repository pins these source versions and rejects cycles, unknown owners,
undeclared Cargo edges and unused declarations. Development-only test edges do
not enter the product build closure.

Gameplay/data dependencies retain their ordinary manifest fields and projection
rules. Native source dependencies describe the compiler's input closure. Cargo
compiles internal crates; development workspace membership does not choose game
features. Resource locks remain separate client presentation inputs.

`python tools/package_graph.py --json` reads the manifest graph and cross-checks
it against Cargo. It does not generate another authored inventory. Public API
documentation stays beside the implementation and package READMEs explain roles.

The generic `@latticeaxiom/host` verifies locked images and artifacts without
depending on Terrenia. `@terrenia/client` owns the current game application and
consumes that public API. Ownership validation and actual application activation
are separate checks; the product build and native acceptance cover the latter.

## Frozen application builds

`products/terrenia-dev.toml` selects `@terrenia/client`; its manifest exposes the
two Cargo binaries in `rust.binaries`. The product's root and manifest dependency
edges select the source closure. The development workspace is an explicit source
inventory and validation input, not the game-feature selector.

`tools/native_product.py` snapshots the selected source-inclusion paths, Cargo
pins, compiler configuration and declared `third_party/` patches. The immutable
snapshot stays under `.temp/native/<digest-prefix>/source`, with its complete
SHA-256 plan verified on reuse. A checked copy under a stable build path avoids
recompiling Bevy merely because the snapshot directory changed. Sources are
compared with the snapshot before and after Cargo runs.

Cargo metadata must place every local source under that generated build root.
Offline resolution may prune unused seed-lock entries but cannot select new
upstream versions/checksums. Compilation uses `--frozen`, the manifest-declared
binary targets and explicit implementation features. The receipt records input,
lock, compiler/environment and executable identities. `task play` and the normal
development tasks use this path. Runtime gameplay and resource locks are still
verified separately by the application.

Native source admission requires an explicit trusted-native realization. The
bootstrap trust ceiling defaults to data-only and is now honored by both lock
paths. Game/shell development profiles explicitly admit their native source
owners while retaining data realization selection; resource profiles stay
data-only. Cargo build scripts run with producer authority. These source checks
are not an OS sandbox or a portable-module ABI attestation.

Upstream references checked 2026-09-05:

- [Cargo workspaces](https://doc.rust-lang.org/cargo/reference/workspaces.html):
  workspace membership, shared dependency pins and root-only profiles/patches.
- [Cargo dependencies](https://doc.rust-lang.org/cargo/reference/specifying-dependencies.html):
  package aliases, local source paths and normal/build/development scopes.
- [Cargo external tools](https://doc.rust-lang.org/cargo/reference/external-tools.html):
  stable metadata and compiler-artifact/build-script JSON messages.
