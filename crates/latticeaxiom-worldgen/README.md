# latticeaxiom-worldgen

Pure Rust algorithm scaffolding for the D4 world-generation slice. The
crate has no Bevy, task-runtime, renderer, package loader, registration-image
compiler, storage writer, or dynamic-module dependency.

It provides:

- exact `WorldSeedV1` integer/text derivation and a closed, integer-only,
  versioned `WorldgenConfigV1`;
- fail-closed resolution of exactly one coordinator/provider identity per fixed
  D4 reference slot;
- package-owned Role IDs resolved to concrete block IDs against an externally
  supplied 18-block-or-larger catalog closure;
- scaffold-local Predicate evaluation receipts tied to permitted placement
  Roles; their package-facing registration schema is not frozen here;
- allocation-free hot-path territory samples for two deterministic fixture
  algorithms (temperate woodland and arid badlands), height/density, resources,
  and chunk+halo vegetation rasterization; package-owned style identities remain
  an external registration input;
- a bounded integer cave field with direction-independent shared-face field
  requests, plus separate validation counts for raw-field and final occupancy;
  minimum cover can legitimately suppress a requested final opening;
- structurally complete four-cardinal planning-cell epoch snapshots; D4 rejects
  every cross-epoch write, including one with only an unverified adapter
  declaration, until a boundary verifier can mint applied evidence;
- snapshot-first candidates with provenance kept in an atomic sidecar rather
  than output bytes, plus a runtime plan-activation token and storage
  `ChunkRevisionExpectation` for stale-result rejection;
- hard count, identity-byte, aggregate-input, live-working-set, work, voxel, and
  snapshot-size preflight limits.

`D4SnapshotCandidateV1` is not the final D3 `world-wire@1` envelope and does not
prove durability. `ExistingSnapshotEvidenceV1` is explicitly caller-trusted;
a storage adapter may construct it only from one authoritative read. The final
package/RegistrationImage adapter must still supply typed graph I/O, package
provenance, compositor/influence budgets, capability checks, and publisher
activation/CAS validation. Accordingly this crate must not be used alone as a
claim that the whole D4 package/runtime exit gate is closed. It is an
algorithm scaffold, not a frozen provider, style, Predicate-receipt, or content
contract.

Run the automated evidence with:

```text
cargo test --locked -p latticeaxiom-worldgen
cargo clippy --locked -p latticeaxiom-worldgen --all-targets -- -D warnings
cargo rustdoc --locked -p latticeaxiom-worldgen --lib -- -D warnings
cargo bench --locked -p latticeaxiom-worldgen --bench chunk_generation -- --test
```