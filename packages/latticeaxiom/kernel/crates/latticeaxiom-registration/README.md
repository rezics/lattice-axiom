# latticeaxiom-registration

`latticeaxiom-registration` deterministically compiles a validated
`CompositionSpec`, its exact `LockedGameGraph`, and one registration contract
for every locked package. The operation runs before package code is loaded and
contains no Bevy types, package resolver, artifact loader, or second lock model.

The lock is authoritative for closure order, selected providers, package hashes,
and owner-bound `NamespaceGrant` rows. The compiler revalidates the complete
profile-to-root and package-to-direct-dependency grant chain before accepting any
exact registration, schema, callback, system, setting, observability, semantic
contract, or bundle.

The output separates four content-addressed receipts:

- semantic resolution, including active bundles and Role bindings;
- image layout and schedule;
- callback bindings;
- source and producer provenance.

All receipts carry the same closure-wide `registration_semantic_hash`.
`RegistrationImageReceipt::numeric_ids` is partitioned by `RegistrationKind`;
each table independently assigns the dense range `0..n` in canonical `StableId`
byte order. Reusing numeric value zero in two different kinds is valid. Consumers
must retain the kind with every numeric ID and must use `CallbackMapReceipt`
rather than treating a scheduled system ID as a callback key.

`PackageRegistrationInput` is the compiler-owned validated bridge for schema
value contracts, system signatures, callback declarations, capability evidence,
and semantic contribution grants that are not yet embedded in the compose
manifest DTO. `LockedPackage`/`BuildPlan` must eventually freeze the resulting
registration semantic hash and callback-map hash, and the SDK should reuse one
shared system-signature/callback contract instead of parallel DTOs. Until that
migration lands, activation must bind the complete `CompiledRegistration` to the
exact build plan before any callback runs.

Run the headless gates with the workspace lock:

```text
cargo test --locked -p latticeaxiom-registration --all-features
cargo clippy --locked -p latticeaxiom-registration --all-targets --all-features -- -D warnings -D clippy::pedantic
cargo doc --locked -p latticeaxiom-registration --all-features --no-deps
```