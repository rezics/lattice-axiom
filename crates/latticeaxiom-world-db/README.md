# `latticeaxiom-world-db`

`latticeaxiom-world-db` is a D3 persistence-boundary prototype and a volatile,
deterministic reference implementation. It does **not** provide RocksDB,
filesystem persistence, WAL behavior, media synchronization, crash/restart
recovery, or physical checkpoints.

## Current safety posture

`DeterministicWorldStorage` reports
`StorageDurabilityCapabilityV1::VolatileReference`. It accepts only `Written`
reference commits. Requests for `Durable`, `flush_durable`, physical checkpoint
creation, or physical checkpoint verification fail before publication with
`PhysicalDurabilityUnsupported`; their receipts and frontiers are never forged
by the memory implementation.

Writer activation also currently fails closed with
`ActivationEvidenceUnavailable`. Storage preflight still returns DB-derived
evidence, and `WriterActivationV1` consumes a catalog-accepted writable plan,
but the current `latticeaxiom-world-catalog::AcceptedWorldOpenPlan` does not
expose a first-party, non-forgeable receipt binding world ID, store ID, metadata
epoch/hash, projection hash, and plan generation. The storage permit alone is
not accepted as authority to bypass that missing catalog boundary.

The internal test-only writer fixture exists solely to exercise atomic
reference transitions. It is compiled only for this crate's unit tests and is
not a product API.

## Implemented reference invariants

The volatile reference currently exercises:

- bounded, read-only storage preflight and coherent captured reads;
- catalog-owned header reconciliation, including non-repairable world/store
  identity mismatch;
- one in-process reference writer lease;
- storage-owned shutdown lifecycle: activation writes an unclean marker,
  normal explicit close writes a clean marker, and `Drop` remains unclean;
- optimistic atomic multi-chunk replacement and a derived persistent-entity
  index;
- stable ordered maps and bounded exact-retry receipts;
- ordinary-commit frozen-lock immutability and monotonic requirement-closure
  expansion;
- DB-first deterministic header publication and typed pre-publication faults;
- versioned metadata and chunk-record keys delegated to
  `latticeaxiom-world-wire`.

The memory model is not a conformance substitute for a physical backend.
Frontier fields named `durable` and `checkpointed` remain zero in this backend.
The private reference-checkpoint prototype is deliberately not reachable
through the public implementation because its image is not yet the complete,
versioned, bounded-decode store image required for a recovery claim.

## Typed persisted-chunk wire boundary

Persisted chunk records use the sealed first-party
`latticeaxiom-world-wire::PersistedChunkSnapshotV1` codec with fixed
owner/schema/version constants. `world-db` explicitly converts its domain DTO
to and from that wire DTO, accounts the codec's exact postcard payload length,
and checks that the decoded logical key matches the record key. Constructor
arguments can no longer select a drifting snapshot owner or schema.

The codec performs envelope/contract validation plus payload, collection,
identifier, nested-payload, total nested-payload, and depth preflight before
postcard allocation. It rejects trailing, unknown-contract, malformed, and
non-canonical inputs. Raw postcard remains only in private reference-only
checkpoint/fingerprint prototypes, not in the persisted chunk value path.
## Stable keyspace definitions

The prototype defines these logical column-family names for a future physical
adapter:

| Family | Name | Rule |
| --- | --- | --- |
| Reserved | `default` | Must remain empty. |
| Metadata | `latticeaxiom-metadata-v1` | Versioned authoritative metadata values. |
| Records | `latticeaxiom-records-v1` | Portable chunk-record keys and typed snapshots. |

Metadata keys are fixed-width `u16be major || WorldId[16] || u16be kind`.
Chunk-record keys are encoded only by `latticeaxiom-world-wire`; opaque record
kinds are not writable aliases.

These are schema definitions, not evidence that a RocksDB column family,
`WriteBatch`, WAL, sync option, checkpoint directory, corruption corpus, or
crash-reopen path has been implemented.

## Bounds

`WorldStorageLimitsV1::D3_BOOTSTRAP` currently bounds one commit to 32 chunks
and 64 MiB of uncompressed payload, retains at most 64 retry receipts, limits a
canonical header to 64 KiB, a frozen lock to 4 MiB, and a requirement closure to
65,536 entries. A future physical adapter must additionally prove these bounds
at decode/allocation boundaries and under restart/corruption tests.

## Gates before a production claim

A production implementation must add, in a dedicated dependency change, all of
the following before advertising D3 readiness:

1. first-party sealed persisted-chunk codec integration and malicious-budget
   corpus;
2. non-forgeable catalog activation receipt with plan generation, consumed and
   revalidated under the final writer-lock acquisition;
3. a real RocksDB adapter with the required column families and one atomic
   authoritative `WriteBatch`;
4. WAL-enabled Written behavior, sync-backed Durable behavior, and crash/reopen
   frontier tests;
5. complete versioned checkpoint images, bounded decode into an independent
   instance, restore comparison, corruption corpus, and retained journal/index
   verification;
6. process crash tests for DB-first sidecar publication and clean/unclean
   shutdown recovery;
7. optimized workload measurements and warning-free test, clippy, rustdoc, and
   formatting gates.

Until those gates pass, this crate is a fail-closed reference boundary, not a
save-system implementation.