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

## Interactive lifecycle

The shell reads only catalog metadata. Create publishes the new world before
showing it; Continue reopens the selected world's image using the frozen gameplay
lock. Resource-pack locks are independent and never change saved-world identity.
The game flushes player/chunk state through the sealed writer, closes its lease,
and commits the image before publishing a Save & Quit receipt. A failed save
stays visible with a retry action; retries reopen the physical connection first.
An unsuccessful event loop does not overwrite the last good image during teardown.

Normal supervisor completion archives only its terminal control files under
`run/launcher-history/`. The next launch starts a fresh protocol generation.
World files stay under `run/worlds/`; failed launcher state stays available for
recovery inspection.
The native adapter refreshes validation time after process waits. A healthy,
running child is observed again; only an explicitly failed shutdown deadline
permits timeout recovery. Ordinary play is not limited to one observation window.

The native acceptance driver is opt-in and requires a marked, isolated runtime
directory. After preparing the normal locks and building development binaries,
run `python tools/verify_native_lifecycle.py --binary <latticeaxiom-play>
--runtime <prepared-workspace> --output <new-output-directory>`. It copies only
locks/CAS, creates a world through native controls, saves, exits, launches again,
and continues the same world. Each world stays active past the 30-second
observation window. It keeps both process logs and supervisor reports.
It does not copy or migrate user worlds. This checks lifecycle behavior, not
GPU performance or the complete survival journey.

The headless persistence acceptance is
`cargo test -p latticeaxiom-engine --no-default-features --features acceptance
--test physical_world --offline`. Its optional feature composes fresh locks
without making the Nickel evaluator a dependency of ordinary client tests.

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

## Retaining world rules across default-profile changes

Each successful lock transaction archives the previous and new verified locks
under `catalog/locks/<product-lock-hash>.lock`. Archive entries are immutable;
a modified existing archive is rejected. A world's catalog entry keeps its
original gameplay hash. The shell checks only archived lock metadata during
listing, then verifies its CAS closure when the world is selected. The supervisor
starts the world process with that exact archived lock. Changing the default
profile therefore affects new worlds without rewriting existing world rules.

`task play` selects the survival profile. A new survival inventory is empty;
the creative starter kit and developer telemetry remain in the explicit dev
profile. Hand crafting stays available through inventory; opening advanced
crafting in survival requires aiming at a crafting workstation.
