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
  preflight DTOs;
- `nickel/latticeaxiom` contains the versioned R0 authoring contracts, while
  `fixtures/composition` pins positive canonical outputs and negative diagnostic intent;
- the current authoring boundary is Nickel library contract 2, R0 authoring
  corpus 2, package model 2, game-profile model 2, normalized composition
  schema 2, and registration-manifest schema 1;
- the composition crate embeds Nickel 0.18 for trusted typed evaluation and
  provides immutable raw-byte source snapshots, deterministic source-table
  preflight, NFC and case-fold collision checks, explicit whole-root
  acquisition budgets, and conservative rejection of links and reparse
  points;
- profile normalization preserves package-qualified features and namespace,
  trust, override, and recovery policy in the typed `CompositionSpec`; target
  triples and evaluator enforcement backends use validated identifiers;
- the versioned diagnostic policy has deterministic ordering, exact
  deduplication, and bounded truncation, while evaluation receipts distinguish
  hard, soft, and unsupported deadline, memory, and recursion enforcement.
- a versioned single-request worker protocol, explicit-path supervisor, and
  public controller CLI now provide bounded framing, monotonic timeout with
  kill/reap, host/backend-bound receipts, atomic controller-fatal diagnostics,
  typed response validation, and byte-identical embedded/CLI worker
  conformance for import-free trusted fixtures.

These crates deliberately contain no Bevy or process-local handles. The client
scene is still a temporary bootstrap; it is not yet the package-driven D0 host.
The in-process evaluator does not enforce the frozen wall-clock, memory, or
recursion limits and is not an untrusted-code boundary. The controlled worker
path enforces the complete-request monotonic deadline, but production R0
receipts still fail closed until the source-table-only Nickel loader, OS memory
containment, and evaluator call-frame instrumentation are present. Relative
imports in the current trusted adapter still use Nickel's ambient filesystem
resolver.

The worker path therefore accepts only explicitly trusted, import-free Tool or
test-fixture requests under a non-production policy. Any static import is
preflighted from immutable snapshots and then rejected with a capability
diagnostic before Nickel can fall back to the ambient filesystem. Production
`r0@1` remains fail-closed.

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
cargo test --workspace --all-targets --all-features
```

The model tests are headless and do not create a window or GPU device.
The composition corpus can also be checked with Nickel CLI 1.17; see
[`fixtures/composition/README.md`](fixtures/composition/README.md) for the exact positive and
negative fixture commands.
