# Lattice Axiom engine bootstrap

This repository is the implementation workspace for
[Lattice Axiom](https://github.com/rezics/lattice-axiom). The previous custom
runtime and renderer have been removed. The current bootstrap is a standard
[Bevy](https://github.com/bevyengine/bevy) application pinned to `0.19.1`.

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
facade. Product-specific package, world, persistence, and ABI contracts will
be introduced through the vertical-slice roadmap in the documentation
repository.
