# Lattice Axiom — first playable demo

Implementation workspace for the first playable demo of **Lattice Axiom**, a
composable voxel world platform. Architecture, decisions (ADRs), and the
roadmap live in the documentation repository
([`lattice-axiom`](https://github.com/rezics/lattice-axiom)); this repository
contains the Rust workspace that implements them milestone by milestone.

Current status: **milestones 1–3 implemented** — deterministic Nickel package
composition, stable numeric registrations, a streamed palette-chunk world,
walking physics, greedy voxel meshing, durable break/place interactions,
RocksDB persistence, and an egui diagnostics overlay. The headless acceptance
path exercises the same composition and gameplay contracts without a GPU.

## Quick start

```bash
# Windowed persistent sandbox.
cargo run --package latticeaxiom-demo

# M1–M3 acceptance without a window or GPU: compose, stream, mesh, simulate,
# break/place, recreate the runtime, and verify the edits persisted.
cargo run --package latticeaxiom-demo -- --headless 60

# Validate and inspect the exact package closure.
cargo run --package latticeaxiom-cli -- check profiles/dev.ncl
cargo run --package latticeaxiom-cli -- doctor --lock latticeaxiom.lock

# Everything CI runs (fmt, clippy, tests, cargo-deny if installed).
cargo xtask ci
```

The pinned toolchain from `rust-toolchain.toml` is installed automatically by
rustup on first use. Building the production RocksDB backend also requires a
C++ build toolchain and LLVM/libclang; `librocksdb-sys` must be able to find
libclang and its resource headers.

### Controls

| Input | Action |
| --- | --- |
| Left click while released | Capture the mouse |
| Mouse | Look around |
| `W` `A` `S` `D` | Walk forward / left / back / right |
| `Space` | Jump |
| Left mouse | Break the targeted block |
| Right mouse | Place the selected block |
| `Escape` | Release the mouse; press again to quit |

## Workspace layout

```text
crates/
├── latticeaxiom-core             M1–M3  space, blocks/chunks, terrain, raycast, AABB physics
├── latticeaxiom-render           M1  backend-agnostic rendering facade + conformance suite
├── latticeaxiom-render-wgpu      M1–M3  wgpu backend + egui compositor
├── latticeaxiom-render-headless  M1  GPU-free renderer used by tests and CI
├── latticeaxiom-demo             M1–M3  winit host and first-person sandbox assembly
├── latticeaxiom-compose          M2  typed Nickel evaluation boundary
├── latticeaxiom-packages         M2  exact resolver, canonical lock, build plan
├── latticeaxiom-modules          M2  stable BlockId runtime image
├── latticeaxiom-cli              M2  check / lock / build / pack / doctor
├── latticeaxiom-storage          M3  WorldStorage facade, DTOs, memory reference backend
├── latticeaxiom-storage-rocksdb  M3  production RocksDB backend
└── latticeaxiom-voxel-mesh       M3  in-house face culling and greedy meshing
nickel/       M2  versioned latticeaxiom.lib contracts
packages/     M2+ content package sources (official content is an ordinary package)
profiles/     M2  game.ncl root profiles
tools/xtask   dev automation (cargo xtask ci)
run/          gitignored local runtime directory (world data, scanned packages)
```

`M{n}` is the milestone in the
[demo roadmap](https://github.com/rezics/lattice-axiom/blob/main/docs/planning/roadmap-first-demo.md)
at which a directory gains its implementation.

## Engineering rules

See [`AGENTS.md`](AGENTS.md) for the binding conventions: facade boundaries
(enforced by `deny.toml`), coordinate system (ADR 0011: right-handed, Z-up),
error handling, lints, testing taxonomy, and the dependency upgrade policy.

## License

Not yet decided; all rights reserved until a license is chosen.
