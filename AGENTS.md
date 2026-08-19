# Engineering rules

Binding conventions for all code in this repository, for humans and AI agents
alike. Architecture and rationale live in the
[`lattice-axiom`](https://github.com/rezics/lattice-axiom) docs repository;
this file only encodes what implementation must obey.

## Language and naming

- Code, comments, rustdoc, commit messages, and repository docs are English.
  Design documents stay in the `lattice-axiom` repository (Traditional Chinese).
- The machine identifier of the project is `latticeaxiom` (ADR 0012). Crates
  are named `latticeaxiom-<component>`; workspace folders match crate names.
  Nickel-side names follow the same brand: `latticeaxiom.lib`,
  `latticeaxiom.official`, `latticeaxiom.lock`, `latticeaxiom-package.json`.
- No grab-bag modules (`utils`, `helpers`, `misc`). One concept per module.

## Facade boundaries (mechanically enforced)

`deny.toml` bans backend crates outside their facade implementation:

| Backend | Only allowed direct dependent |
| ------- | ----------------------------- |
| `wgpu`  | `latticeaxiom-render-wgpu`    |
| `winit` | `latticeaxiom-demo` (host layer) |
| `rocksdb` | `latticeaxiom-storage-rocksdb` (milestone 3) |

Backend types must not appear in facade signatures consumed by core or content
crates. When egui arrives (milestone 3), extend the wrapper lists with
`egui-wgpu`/`egui-winit` deliberately; never weaken a ban to "everywhere".

## Coordinate system (ADR 0011)

- World space is right-handed, **Z-up**: `+X` east, `+Y` north, `+Z` up.
  Coordinates and indices are always ordered `(x, y, z)`; height is `z`.
- Positive rotation follows the right-hand rule; around `+Z` it turns `+X`
  toward `+Y`.
- Voxel cell `(i, j, k)` spans the half-open box `[i, i+1) x [j, j+1) x [k, k+1)`.
  Continuous-to-discrete conversion floors toward negative infinity; chunk and
  local coordinates use `div_euclid` / `rem_euclid` (see `latticeaxiom_core::space`).
- Units: meters and radians. Clip-space and depth-range conversions live inside
  render backends only.
- Persistent keys that contain signed coordinates must use order-preserving
  encoding: flip the sign bit (`(v as u32) ^ 0x8000_0000`) then big-endian bytes
  (applies from milestone 3 on).

## Error handling and panics

- Library crates define domain errors with `thiserror`. `anyhow` is allowed
  only in binaries (`latticeaxiom-demo`, `latticeaxiom-cli`, xtask).
- `unwrap` is denied workspace-wide (tests exempt via `clippy.toml`); `expect`
  must state the invariant that makes it unreachable.
- Panics are for programmer errors (broken invariants) only. I/O, user data,
  and GPU/surface conditions return `Result`.

## Determinism and hot-path rules

- Cross-module identifiers are newtypes (`MeshId`, later `BlockId`, `ChunkPos`).
  String keys exist only during composition/registration, never per-frame,
  per-entity, or per-voxel.
- Iteration that affects output (ID assignment, registration order, hashing)
  must not depend on `HashMap` order: use `BTreeMap` or sort by a stable key.
- Persisted values are wrapped in a versioned schema envelope; `bincode` is
  used only inside that envelope. Storage DTOs are separate types from live
  ECS/runtime state (from milestone 3 on).

## Lints, formatting, docs

- Lints are defined once in `[workspace.lints]`; every crate inherits with
  `[lints] workspace = true`. CI runs clippy with `-Dwarnings`.
- Fix pedantic lints properly where reasonable; otherwise use a narrowly
  scoped `#[allow(...)]` with a one-line justification. Never disable a lint
  category workspace-wide to silence one site.
- `unsafe_code` is denied. If a future crate genuinely needs it (dynamic
  loading at milestone 7), it overrides the lint locally and every block gets
  a `// SAFETY:` comment (`undocumented_unsafe_blocks` is denied).
- All public items carry rustdoc; public `Result` functions document `# Errors`.
- Formatting is stable rustfmt only (`rustfmt.toml`); no nightly options.
- Logging uses `tracing`; subscribers are installed by binaries only. No
  `println!` in libraries (`print_stdout` is denied; xtask writes to stderr).

## Testing taxonomy

- **Unit**: colocated `#[cfg(test)] mod tests`.
- **Property**: `proptest` for round-trips and algebraic invariants (e.g.
  chunk/local split reconstruction in `latticeaxiom-core`).
- **Conformance**: facade crates export a shared suite (see
  `latticeaxiom_render::conformance`); every implementation runs the same
  suite. GPU-dependent runs are `#[ignore]`d and executed locally.
- **Golden** (`insta`) and **fault-injection** (kill tests) join at milestone 3
  with storage.
- Every roadmap milestone's exit criteria must be encoded as automated tests
  where possible; CI stays headless (no window, no GPU device).

## Dependencies

- Versions are declared once in `[workspace.dependencies]`; member crates use
  `dep.workspace = true`. `Cargo.lock` is committed and is the precise record.
- Add a dependency only when it replaces meaningful self-built work; record
  the boundary it must not cross (see the technology-stack page in the docs
  repo). Upgrades happen at roadmap milestones, not ad hoc.

## Commits

- Conventional Commits with the crate component as scope, e.g.
  `feat(render): add mesh upload to the facade` or `chore(ci): bump toolchain`.
- Keep refactors, behavior changes, and dependency bumps in separate commits.
