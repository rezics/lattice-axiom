# Physical world publication

`DiskWorldStore` uses redb **4.2.0**, pinned in Cargo.lock, to atomically publish
a portable world image and its small catalog record. Only a successful
`Durability::Immediate` commit acknowledges physical persistence. An error is
not reported as a durable save and is not assumed to imply rollback.

The existing `DeterministicWorldStorage` remains a contract/recovery oracle.
Its synchronized image can be exported and restored without copying writer
leases. It does not become a physical database merely by reporting its logical
WAL/sync capability. The disk adapter supplies that missing physical boundary.

Catalog listing reads metadata without decoding voxel payloads. One previous
image is retained. This initial adapter publishes complete portable snapshots;
it is not yet an incremental disk-backed chunk cache or a performance claim.
Those costs must be included in the streaming and save acceptance workloads.

## Adoption evidence

- redb 4.2.0 is Rust-native, requires Rust 1.90 (below the locked 1.97 toolchain),
  and uses MIT OR Apache-2.0 licensing. `cargo info redb@4.2.0` corroborated this.
- Cargo added one new locked package. Default `std` is used; no optional feature
  or C/C++ toolchain is added.
- Existing code had no RocksDB implementation to preserve. Adding a large native
  toolchain or implementing a bespoke atomic database was unnecessary for this
  first physical-persistence boundary. The portable record contracts remain
  independent of this backend.
- Tests close every original storage/database handle and reopen from the real
  file, and verify that an abandoned transaction cannot publish catalog changes.

Primary references, checked 2026-09-05:

- [redb design](https://github.com/cberner/redb/blob/master/docs/design.md)
- [Database](https://docs.rs/redb/4.2.0/redb/struct.Database.html)
- [Durability](https://docs.rs/redb/4.2.0/redb/enum.Durability.html)
- [Commit failure semantics](https://docs.rs/redb/4.2.0/redb/struct.WriteTransaction.html)
