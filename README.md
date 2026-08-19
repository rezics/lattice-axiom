# Lattice Axiom — first playable demo

Implementation workspace for the first playable demo of **Lattice Axiom**, a
composable voxel world platform. Architecture, decisions (ADRs), and the
roadmap live in the documentation repository
([`lattice-axiom`](https://github.com/rezics/lattice-axiom)); this repository
contains the Rust workspace that implements them milestone by milestone.

Current status: **milestone 1** — workspace, empty host, rendering facade with
wgpu and headless implementations, one cube, camera controls.

## Quick start

```bash
# Windowed demo (WASD + right-mouse-drag to fly, Escape to quit).
cargo run --package latticeaxiom-demo

# Headless mode: simulates and validates N frames without a window or GPU.
cargo run --package latticeaxiom-demo -- --headless 60

# Everything CI runs (fmt, clippy, tests, cargo-deny if installed).
cargo xtask ci
```

The pinned toolchain from `rust-toolchain.toml` is installed automatically by
rustup on first use.

### Controls

| Input             | Action                          |
| ----------------- | ------------------------------- |
| `W` `A` `S` `D`   | Move forward / left / back / right |
| `Space` / `Ctrl`  | Move up / down (world +Z / −Z)  |
| Right mouse drag  | Look around                     |
| `Escape`          | Quit                            |

## Workspace layout

```text
crates/
├── latticeaxiom-core             M1  world/space conventions, simulation contracts
├── latticeaxiom-render           M1  backend-agnostic rendering facade + conformance suite
├── latticeaxiom-render-wgpu      M1  the only crate allowed to depend on wgpu
├── latticeaxiom-render-headless  M1  GPU-free renderer used by tests and CI
├── latticeaxiom-demo             M1  executable host: winit window, camera, scene
├── latticeaxiom-compose          M2  Nickel evaluation -> CompositionSpec (placeholder)
├── latticeaxiom-packages         M2  package kernel: sources, resolve, lock, build plan (placeholder)
├── latticeaxiom-modules          M2  registration contracts, RuntimeImage, ABI descriptors (placeholder)
├── latticeaxiom-cli              M2  check / lock / build / pack / doctor (placeholder)
├── latticeaxiom-storage          M3  WorldStorage contract + in-memory impl (placeholder)
├── latticeaxiom-storage-rocksdb  M3  the only crate allowed to depend on rocksdb (placeholder)
└── latticeaxiom-voxel-mesh       M3  in-house voxel meshing (culling + greedy); implemented, integrates at M3
nickel/       M2  versioned latticeaxiom.lib Nickel contracts
packages/     M3+ content package sources (official content is an ordinary package)
profiles/     M2  game.ncl root profiles
tools/xtask   dev automation (cargo xtask ci)
run/          gitignored local runtime directory (world data, scanned packages)
```

`M{n}` is the milestone in the
[demo roadmap](https://github.com/rezics/lattice-axiom/blob/main/docs/planning/roadmap-first-demo.md)
at which a directory gains its implementation. Placeholder directories carry a
README describing their planned scope.

## Engineering rules

See [`AGENTS.md`](AGENTS.md) for the binding conventions: facade boundaries
(enforced by `deny.toml`), coordinate system (ADR 0011: right-handed, Z-up),
error handling, lints, testing taxonomy, and the dependency upgrade policy.

## License

Not yet decided; all rights reserved until a license is chosen.
