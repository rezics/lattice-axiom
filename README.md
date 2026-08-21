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
- the current authoring boundary is Nickel library contract 3, authoring
  corpus 3, package model 3, game-profile model 3, normalized composition
  schema 3, and registration-manifest schema 1;
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
  typed response validation, and direct canonical response-byte parity for
  one import-free trusted package success plus one controller-fatal failure
  fixture; these fixtures do not establish production-worker conformance.

These crates deliberately contain no Bevy or process-local handles. The client
scene is still a temporary bootstrap; it is not yet the package-driven D0 host.
The in-process evaluator does not enforce the frozen wall-clock, memory, or
recursion limits and is not an untrusted-code boundary. The controlled worker
path enforces the complete-request monotonic deadline, but production R0
receipts still fail closed until the source-table-only Nickel loader, OS memory
containment, and evaluator call-frame instrumentation are present. The trusted
D0 adapter verifies an immutable source closure and stages only receipt-covered
bytes in a fresh private tree without passing original source paths to Nickel;
Nickel still reads that private tree through its filesystem resolver, so this
adapter is not a production containment boundary.

The production worker path therefore accepts only explicitly trusted,
import-free Tool or
test-fixture requests under a non-production policy. Any static import is
preflighted from immutable snapshots and then rejected with a capability
diagnostic before Nickel can fall back to the ambient filesystem. Production
`r0@1` remains fail-closed.

## First run

Ordinary launch reopens `latticeaxiom.lock` and freeze-verifies `catalog/cas`.
Those files are generated locally and are not committed. The production client
does not create them.

From the workspace root, lock the client-world bootstrap, then launch:

```powershell
cargo run -p latticeaxiom-compose --bin latticeaxiom-compose --features nickel-evaluator -- lock --offline --bootstrap profiles/dev.toml
cargo run -p latticeaxiom-engine --no-default-features --features client
```

`profiles/dev.toml` is the `client-world` projection. Workspace-root
`latticeaxiom.toml` is the dedicated-server projection and is not the
interactive client lock. After a lock exists, `cargo run` from the workspace
root is the same production client (default members and default features).

The first Bevy build can take several minutes. Missing lock or CAS fails
closed. Relock after changing shipped packages.

A lock-free development slice remains available as an extra binary; it is not
ordinary launch:

```powershell
cargo run -p latticeaxiom-engine --bin latticeaxiom-playable-fixture --no-default-features --features client
```

Development builds enable Bevy dynamic linking and use `rust-lld` on Windows
to shorten later link times. A distributable build must disable the
development feature:

```powershell
cargo build --release --no-default-features --locked
```

Do not add a project-owned App, ECS, scheduler, renderer, asset, input, or task
facade. Remaining package, world, persistence, and ABI behavior is introduced
only through the vertical-slice roadmap in the documentation repository.

## Headless checks

```powershell
cargo fmt --all --check
cargo metadata --locked --format-version 1 --no-deps
cargo check --workspace --all-targets --all-features --locked
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test --workspace --all-targets --all-features --locked
cargo build --workspace --release --no-default-features --locked
```

The model tests are headless and do not create a window or GPU device.
The release command is a compile-only CI gate; it does not launch the client.
CI installs the exact `cargo-deny` version frozen by the repository and runs
the complete locked advisory, ban, license, and source policy on every change
and on a daily schedule. The equivalent local commands are:

```powershell
cargo install --locked cargo-deny --version 0.20.2
cargo deny --locked check
```

Temporary advisory ignores in `deny.toml` carry an exact package version,
owner, mitigation, tracking issue, replacement trigger, and a maximum 30-day
window. CI checks those fields against `cargo metadata --locked` and fails when
an exception is stale, expired, duplicated, malformed, or no longer resolves.

Release and manually dispatched CI generate target-specific CycloneDX 1.5 JSON
SBOMs and third-party notices with exact `cargo-cyclonedx 0.5.9` and
`cargo-about 0.9.1` pins. The wrapper records the explicit release profile and
feature selection, derives the actual normal/build graph and enabled features
from locked `cargo tree` output rather than guessing them from `Cargo.lock`,
corroborates the selection with target-filtered metadata, and checks SBOM
packages, dependency edges, and registry checksums in both directions. It also
removes timestamps, random IDs,
absolute paths, and unstable ordering before hashing every evidence file in a
release-evidence manifest. Published releases retain these files as GitHub
Release assets; manual runs retain the workflow artifact for 90 days.

This Cargo evidence deliberately excludes content assets and is not yet bound
to a host binary or `EngineBuildReceiptV1`. The eventual product release
assembler must bind all three without feeding the SBOM hash back into
`EngineBuildId`. See [`supply-chain/README.md`](supply-chain/README.md) for the
local generator, validation, and fault-test commands.

The composition corpus can also be checked with Nickel CLI 1.17; see
[`fixtures/composition/README.md`](fixtures/composition/README.md) for the exact positive and
negative fixture commands.
