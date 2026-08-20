# latticeaxiom-gameplay

`latticeaxiom-gameplay` is the engine-independent D8 sandbox gameplay contract
crate. It reuses canonical `latticeaxiom-storage` identifiers, coordinates, and
revision observations while owning no Bevy app, scheduler, clock, thread pool,
world writer, revision allocator, or persistence codec.

`GameplayKernel` validates a command against one catalog-bound loaded snapshot
and returns a bounded `GameplayPlanV1`. The plan contains dimension-qualified
block keys and an explicit chunk/domain target for every staged edit. It is not
a storage receipt or a `WorldTransaction`: a production host applies the plan
to validated loaded chunks, captures complete replacement `ChunkMutation`
payloads for every affected chunk, and submits one complete storage
`WorldTransaction`. Only the storage kernel advances `WorldRevision` and
`ChunkRevision` or acknowledges durability.

`ReferencePlanApplier` is a fault-injectable in-memory oracle for atomic apply,
rollback, and exact retry behavior. Its `RuntimePlanReceiptV1` proves only an
in-memory reference apply; it must not be exposed as a storage commit receipt.
The continuation handoff test likewise transfers this in-memory state between
two reference appliers. It does not prove unload/reload or offline persistence.

The compiled catalog intentionally keeps typed raw `StableId` text keys as a
clear reference oracle. Production registration may intern or compile those
keys into runtime handles, but must preserve their stable identity, semantic
tag membership, frozen role bindings, and order-independent results.

This crate reserves versioned schema identities for its gameplay values but
registers no world-wire codec. Durable unload/reload, crash recovery, gameplay
codec compatibility, and canonical decoded-snapshot equality remain D3/D8
integration gates and are deliberately not claimed here.

Headless conformance covers:

- dimension-qualified drop, pickup, placement, mining progress, tools, tiers,
  and durability;
- shaped and shapeless recipes with Tag/Predicate inputs and frozen concrete
  Role outputs;
- atomic inventory/container transfers and furnace input/fuel/output admission;
- scheduled continuation across an in-memory reference-authority handoff;
- loaded-empty versus unloaded cells, cross-dimension isolation, full-width
  storage identifiers, exact retry fingerprints and replay horizons;
- invalid decoded stack/tool state, boundary-plus-one limits, mid-plan faults,
  discovery permutation, property conservation, canonical hash golden bytes,
  and a 10,000-command soak.

Run the automated gates with:

```text
cargo fmt --all -- --check
cargo test --locked -p latticeaxiom-gameplay --all-features
cargo clippy --locked -p latticeaxiom-gameplay --all-features --all-targets -- -D warnings
cargo doc --locked -p latticeaxiom-gameplay --all-features --no-deps
cargo bench --locked -p latticeaxiom-gameplay --bench planning
```