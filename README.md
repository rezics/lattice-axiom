# Lattice Axiom demo workspace

This repository is the implementation workspace for
[Lattice Axiom](https://github.com/rezics/lattice-axiom). The previous custom
runtime and renderer have been removed. The current bootstrap is a standard
[Bevy](https://github.com/bevyengine/bevy) application pinned to `0.19.1`.

The first R0 foundation is also present:

- `latticeaxiom-core` defines validated stable identifiers, the project SemVer
  range grammar, canonical JSON/SHA-256, and source provenance;
- `latticeaxiom-compose` defines the engine-independent composition, lock,
  registration, semantic, settings, observability, runtime-image, and world
  preflight DTOs.

These crates deliberately contain no Bevy or process-local handles. The client
scene is still a temporary bootstrap; it is not yet the package-driven D0 host.

## First run

```powershell
cargo run
```

The first build compiles Bevy and can take several minutes. A successful run
opens a window containing a lit blue cube on a dark ground plane. The scene
uses Bevy's native right-handed Y-up coordinate system.

Development builds enable Bevy dynamic linking and use `rust-lld` on Windows
to shorten later link times. A distributable build must disable the
development feature:

```powershell
cargo build --release --no-default-features
```

Do not add a project-owned App, ECS, scheduler, renderer, asset, input, or task
facade. Remaining package, world, persistence, and ABI behavior is introduced
only through the vertical-slice roadmap in the documentation repository.

## Headless checks

```powershell
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-targets
```

The model tests are headless and do not create a window or GPU device.
