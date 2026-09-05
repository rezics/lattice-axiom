# Lattice Axiom

A package-composed voxel game built with Bevy. This repository owns the game,
its implementation packages, independent resource-pack contracts and tooling.

## Structure

- `packages/<scope>/<name>/`: Lattice packages. Rust implementation is under each
  package's `crates/`; data, schemas, tests and usage notes stay with their owner.
- `resource-packs/`: independently selectable presentation resources. Only code
  and text definitions are versioned here; binary art assets are not committed.
- `profiles/`: current shell/world/headless composition inputs.
- `tools/`, `scripts/`: repository checks, build and acceptance entry points.
- `third_party/`: explicitly vendored upstream source and licenses.

## Development

Use the pinned Rust toolchain in `rust-toolchain.toml`:

```powershell
python tools/repository_check.py
cargo check --workspace --all-targets --locked
task dev:plain
```

`task dev:plain` prepares a local package lock and starts the client. Runtime
worlds and caches are local, ignored data. Rebuild delivery and outstanding
acceptance gates are recorded in [REBUILD_PROGRESS.md](REBUILD_PROGRESS.md).

Package manifests and executable tests define current behavior. Public APIs
are documented with rustdoc; implementation details are not duplicated into a
parallel documentation tree.

## License

AGPL-3.0-only; see [LICENSE](LICENSE). Third-party source retains its own terms.
