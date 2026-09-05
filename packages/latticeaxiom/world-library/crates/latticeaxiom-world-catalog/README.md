# latticeaxiom-world-catalog

Pure-data contracts for the D3 world library and metadata-only open preflight.
The crate implements the safety boundary described by ADR 0027 `WORLD-10`
through `WORLD-18`:

- strict UUIDv4 directory identity and separately indexed display names;
- bounded canonical `world-header.json` decoding and DB-first projection
  reconciliation;
- typed next-safe-step open plans whose writer permission is derived only from
  an accepted exact or compatible activation action;
- checkpointed migration planning, low-disk admission, and managed-trash
  restore conflict rules;
- a deterministic memory source with bounded-read fault injection.

The crate deliberately has no RocksDB adapter, filesystem mutation, package
loader, dynamic-library loading, Bevy world, writer, or migration executor.
Hosts provide read-only sidecar and metadata adapters, then execute accepted
actions through separately reviewed lifecycle boundaries.
