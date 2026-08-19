# Engineering rules

Binding implementation conventions for this repository. Architecture,
rationale, and the active roadmap live in the
[`lattice-axiom`](https://github.com/rezics/lattice-axiom) documentation
repository. Accepted ADRs there override older implementation assumptions.

## Language and naming

- Code, comments, rustdoc, commit messages, and repository docs are English.
- The machine identifier is `latticeaxiom`. Crates are named
  `latticeaxiom-<component>`, and workspace folders match crate names.
- Do not create grab-bag modules such as `utils`, `helpers`, or `misc`.

## Bevy boundary

- Bevy is the game engine and the only App, ECS, scheduler, renderer, asset,
  input, time, task, window, and diagnostics runtime.
- Start clients from `DefaultPlugins`; headless and tests use the smallest
  required standard Bevy plugin composition.
- Core host code and static packages use Bevy types directly. Do not build a
  project-owned mirror facade around Bevy APIs.
- Process-external, persistent, network, and portable dynamic-module
  boundaries use Lattice-owned stable DTOs or generated C ABI types. Bevy
  `Entity`, `Handle`, `TypeId`, `World`, and Rust containers do not cross those
  boundaries.
- Bevy is pinned exactly in `Cargo.lock`. Upgrade it only as a dedicated
  migration with the roadmap's compatibility and performance gates.

## Coordinate system

- Use Bevy's native right-handed Y-up world coordinates: `+X` right, `+Y` up,
  and conventional forward `-Z`.
- Voxel and chunk coordinates are ordered `(x, y, z)`; `y` is height and
  `(x, z)` is the horizontal plane.
- Distances are meters and angles are radians. Do not add a global axis
  conversion layer.

## Error handling and determinism

- Library crates define domain errors with `thiserror`; `anyhow` is allowed
  only in binaries and developer tools.
- `unwrap` is denied outside tests. `expect` must state the invariant that
  makes failure unreachable. Panics are only for programmer errors.
- Output-affecting iteration must not depend on `HashMap` order. Use a stable
  collection or sort by a stable key.
- Runtime and persisted identifiers that cross module boundaries are typed
  newtypes, never per-frame strings.

## Lints, docs, and tests

- Every crate inherits `[workspace.lints]`. CI treats warnings as errors.
- `unsafe_code` is denied. A future ABI crate may override this only locally;
  every unsafe block requires a `// SAFETY:` explanation.
- Public items carry rustdoc. Public `Result` functions document `# Errors`.
- Use stable rustfmt and `tracing`; binaries alone install subscribers.
- Put unit tests beside their code. Use property, conformance, golden, and
  fault-injection tests when their corresponding roadmap invariants appear.
- CI remains headless and must never require a window or GPU device.

## Dependencies and commits

- Declare versions once in `[workspace.dependencies]`; member crates inherit
  them. Commit `Cargo.lock` as the exact dependency and feature record.
- Prefer Bevy built-ins, maintained Bevy ecosystem plugins, upstream
  extensions, and minimal upstreamable forks before project-owned engine
  infrastructure.
- Use Conventional Commits with a component scope. Keep refactors, behavior
  changes, and dependency upgrades in separate commits.
