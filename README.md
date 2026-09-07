# Lattice Axiom

A package-composed voxel game and the foundation for an open game-development
ecosystem, built with Bevy. Terrenia is the current playable product. The intended
ecosystem lets authors publish libraries and frameworks, deeply extend or replace
systems, and compose independently distributed games, including commercial works
under the applicable licenses.

The [ecosystem direction](docs/ecosystem-direction.md) records this objective,
compatibility goals and pending acceptance gates. The
[package interface design](packages/latticeaxiom/sdk/docs/package-interfaces.md)
describes author-owned APIs and supported extension paths. These are architectural
commitments, not a claim that a general dynamic-mod ecosystem is already delivered.

The Windows desktop uses an embedded WebView2 interface for menus, settings,
inventory and HUD. Bevy renders the world, crosshair and mining progress.
See [Web UI delivery](docs/web-ui-delivery.md) for ownership and acceptance.

## Structure

- `packages/<scope>/<name>/`: Lattice packages. Rust implementation is under each
  package's `crates/`; data, schemas, tests and usage notes stay with their owner.
- `resource-packs/`: independently selectable presentation resources. Only code
  and text definitions are versioned here; binary art assets are not committed.
- `profiles/`: current shell/world/headless composition inputs.
- `products/`: native application roots, implementation build features and launch targets.
- `docs/`: cross-package direction; implementation design stays with its package owner.
- `tools/`, `scripts/`: repository checks, build and acceptance entry points.
- `third_party/`: explicitly vendored upstream source and licenses.

## Development

Use the pinned Rust toolchain in `rust-toolchain.toml`:

```powershell
python tools/repository_check.py
cargo check --workspace --all-targets --locked
task play
task dev:plain
```

`task play` creates survival worlds without a debug starter inventory.
`task dev:plain` selects the explicit developer/creative profile. Both prepare
gameplay, shell and independent resource locks, freeze
the selected native package closure, build it with Cargo and start the client.
`task dev` uses the same developer profile with the local Logdy Web viewer.
`task dev:prepare` only generates the three locks, in sequence. Independent
composer processes can also safely publish identical objects to the shared CAS.
For isolated acceptance, `RUNTIME=<directory>` selects an existing runtime
containing copies of the locks and catalog; the default is the repository root.
`task native:prepare` verifies the frozen source graph without compiling binaries;
`task native:build PRODUCT=products/terrenia.toml` selects the release build.
Native product builds also install the pinned JavaScript dependencies and build
the selected Web UI in an isolated source copy. Node/npm are build tools only;
the client loads executable-adjacent `client-ui` assets through a local protocol.
Windows clients require Microsoft WebView2 Runtime. `task web:build` prepares
the local developer bundle, and `task web:test` checks the browser protocol.
Runtime worlds and caches are local, ignored data. Rebuild delivery and outstanding
acceptance gates are recorded in [REBUILD_PROGRESS.md](REBUILD_PROGRESS.md).
Worlds retain their original verified gameplay lock in the local catalog when
the default profile is relocked; changing defaults does not rewrite old worlds.

Package manifests and executable tests define current behavior. Public APIs
are documented with rustdoc; implementation details are not duplicated into a
parallel documentation tree.

## License

AGPL-3.0-only; see [LICENSE](LICENSE). Third-party source retains its own terms.
