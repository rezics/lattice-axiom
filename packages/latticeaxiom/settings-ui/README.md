# `latticeaxiom-settings-ui`

This directory is both the logical `@latticeaxiom/settings-ui` package source
root and the home of its Rust crate, `latticeaxiom-settings-ui`.

- `latticeaxiom-package.toml` and `package.ncl` define the package contract.
- `data/` owns the authored category and control vocabularies.
- `src/` owns the typed catalog projection, presentation lifecycle, and
  immutable settings-surface apply request.
- `tests/` checks the code against the colocated authored data.

The host consumes each apply request through the authoritative settings
registry. Runtime apply, persistence, rollback, and transaction phases are not
re-exported or adapted by this presentation package.

The Cargo crate consumes platform crates through workspace dependencies so the
package source receipt remains independent from repository-relative escape
paths. The current package realization remains data-only; enabling a native
SourceBuild requires a separate, explicit trust-policy decision.

The logical package name and Cargo crate name intentionally remain distinct
identities even though they share one source root.
