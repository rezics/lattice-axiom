# Cargo release evidence

This directory freezes the Cargo dependency evidence contract for shipped host
configurations. It covers Rust packages only. It does not cover game packages,
content, fonts, textures, audio, models, or any other asset inventory.

`release-evidence.toml` is the single list of shipped target, profile, and
feature closures. Feature selection is passed to `cargo metadata`, `cargo tree`,
`cargo-cyclonedx`, and `cargo-about`; it is never inferred from `Cargo.lock`.
`Cargo.lock` remains authoritative for resolved versions, sources, and registry
checksums.

The generator uses exact `cargo-cyclonedx` and `cargo-about` versions. It runs
both tools in a temporary workspace snapshot because cargo-cyclonedx writes one
file beside every workspace manifest. No generated file is allowed to land in
the source tree. The wrapper then:

1. validates the source workspace with
   `cargo metadata --locked --filter-platform`, checks every workspace package
   license, and matches all candidate packages to the exact source lock;
2. derives the configured root's normal/build package, edge, and enabled-feature
   graph from locked `cargo tree` depth output, then requires every selected
   package and edge to be corroborated by target-filtered metadata;
3. requires cargo-about to report exactly that selected package set and rejects
   unknown, ignored, missing, empty, or out-of-closure attribution;
4. requires the official cargo-cyclonedx output to contain the selected graph,
   selects only that graph from the generator's metadata superset, and then
   checks CycloneDX packages and edges in both directions, including checksums;
5. removes timestamps, random identifiers, absolute paths, and unstable order;
6. writes normalized CycloneDX 1.5 JSON, third-party notices, a canonical Cargo
   closure receipt, and a SHA-256 release-evidence manifest plus sidecar hash.

All workspace packages must declare `AGPL-3.0-only`. This check is independent
of cargo-deny's private-package setting and fails before evidence is emitted.

## Local commands

Install the exact generators once:

```powershell
cargo install --locked cargo-cyclonedx --version 0.5.9
cargo install --locked cargo-about --version 0.9.1 --features cli
```

Run the dependency-free fixture and fault corpus:

```powershell
python scripts/supply-chain/test_release_evidence.py
```

Validate configuration, tool versions, locked metadata/tree closures, and
workspace license metadata without generating artifacts:

```powershell
python scripts/supply-chain/release_evidence.py smoke
```

After fetching the exact lock package set, generate one evidence set:

```powershell
cargo fetch --locked
python scripts/supply-chain/release_evidence.py generate `
  --configuration windows-x86_64-release `
  --output-dir target/release-evidence/windows-x86_64-release
```

Release evidence does not include a host binary or build receipt. The product
release assembler must bind this hashed evidence manifest to those artifacts;
the SBOM hash must not feed back into `EngineBuildId`.
