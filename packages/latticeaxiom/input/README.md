# @latticeaxiom/input

This directory is both the logical `@latticeaxiom/input` package source root
and the home of its Rust crate, `latticeaxiom-input`. The package and Cargo
crate names intentionally remain distinct identities.

- `latticeaxiom-package.toml` and `package.ncl` define the package contract.
- `data/` owns the shipped action catalog and default bindings.
- `src/` owns the typed action, binding, conflict, context, and headless input
  contracts.
- `tests/` is reserved for package-level conformance evidence.

Headless-first owner of the ADR 0033 input foundation:

- stable `ActionSpecV1` / `ClientSurfaceActionV1` / player-action mappings
- versioned `InputBindingV1` and `BindingProfileV1`
- exactly-one `latticeaxiom:capability/input-actions@1` provider selection
- deterministic catalog compile, same-context conflicts, and dense indexes
- Leafwing-facing compiled gameplay, surface, and HUD-overlay maps
- `ActiveInputContextStack` with independent capture vs gameplay-suppression
  policies, pressed-state generation, and Esc safety fallback
- headless logical-input injection that produces the same action IDs a client
  adapter must emit

This crate does not read Bevy `ButtonInput`, does not install Leafwing, and
does not `include_str!` a production catalog. Production hosts must compile the
catalog bytes selected by a reopened product lock.

Platform dependencies resolve through workspace contracts. That workspace
compilation is an explicit compatibility bridge: the shipped package continues
to advertise only its frozen data realization. It does not claim a NativeStatic
SourceBuild until an immutable, receipt-injected dependency closure can build
independently from CAS.

Focused checks:

```text
cargo test -p latticeaxiom-input
cargo clippy -p latticeaxiom-input --all-targets -- -D warnings
```
