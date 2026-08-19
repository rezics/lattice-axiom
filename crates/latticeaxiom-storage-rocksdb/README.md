# latticeaxiom-storage-rocksdb

Production `WorldStorage` implementation on RocksDB (ADR 0009), and the only
workspace crate allowed to depend on `rocksdb` (enforced by `deny.toml`). Open
or create a world store with `RocksDbWorldStorage::open(path)`; no RocksDB type
appears in the facade or public method signatures.

Each `ChunkCommit` becomes one WAL-backed RocksDB write batch. `Durability::Wal`
acknowledges after the WAL write and `Durability::Sync` requests a synchronous
durable write. Multi-record loads use a RocksDB read snapshot. Checkpoints use
the native checkpoint API and can be opened independently.

The automated suite runs the shared facade conformance checks, closes and
reopens a database, restores a checkpoint, and launches child processes that
are forcibly terminated immediately before/after batch and checkpoint writes
and during a consistent multi-record load.

Building `librocksdb-sys` requires a C++ build toolchain and LLVM/libclang.
After those native prerequisites are available, run:

```shell
cargo test -p latticeaxiom-storage-rocksdb -- --test-threads=1
cargo clippy -p latticeaxiom-storage-rocksdb --all-targets -- -D warnings
```
