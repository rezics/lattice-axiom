# latticeaxiom-storage

`latticeaxiom-storage` provides a sealed, pure-Rust transaction kernel for
already-materialized authoritative chunks. It contains no Bevy, RocksDB,
task-runtime, filesystem, or renderer types. It is reference evidence for part
of D3, not the complete D3 `WorldStorage` product boundary.

A `WorldTransaction` atomically publishes bounded complete chunk replacements
under one checked `WorldRevision`. Each chunk has a total `ChunkRevision` plus
voxel, persistent-entity, and continuation revisions. Optimistic world and
chunk preconditions, checked revision increments, world-scoped persistent
entity uniqueness, and count/byte preflight checks all reject before publish.

`MemoryTransactionKernel` is deliberately non-durable and reports only the
single reference state `ReferenceDurability::Queued`; it does not define a
production durability ladder. It clones one world into private staging and
swaps it under one write lock. Deterministic failpoints prove no partial pre-publish
state. Exact transaction replay is supported only inside an explicit bounded
per-world receipt horizon; an older retry is rejected and is never applied
again. The derived persistent-entity index is published atomically with chunk
state.

`ReferenceWorldSnapshot` clones the full materialized chunk projection. Its
`MaterializedChunkStateHash` includes chunk/entity/continuation/provenance data,
but explicitly excludes world metadata, requirement closure, and exact lock
data. The snapshot exists for deterministic conformance and fault evidence,
not production scale.

Scale-sensitive in-process callers may use `MemoryTransactionKernel::publish`
to receive a typed `PublicationReceipt` without recomputing the full-world
reference hash on every transaction. It preserves atomic publication,
optimistic validation, revisions, changed-domain evidence, and bounded exact
replay. A full `CommitReceipt` cannot be reconstructed later for a transaction
first accepted through this path; callers that need that reference-only
evidence must use `commit` or capture an explicit `ReferenceWorldSnapshot`.

The reference limits (32 chunks, 64 MiB payload, 64 receipts by default) are
non-normative safety values and are not ADR 0027 production budgets.

The next D3 product boundary must add typed world metadata, requirement closure
and exact lock data in the same atomic transaction; bounded chunk reads,
contiguous frontier and checkpoint receipts; a versioned wire envelope; and a
RocksDB write-batch/WAL implementation. Current Serde derives are DTO
convenience and do not freeze that persistent wire contract.

After workspace registration, run:

```shell
cargo test -p latticeaxiom-storage --all-features --locked
cargo clippy -p latticeaxiom-storage --all-targets --all-features --locked -- -D warnings
cargo rustdoc -p latticeaxiom-storage --all-features --locked -- -D warnings
```
