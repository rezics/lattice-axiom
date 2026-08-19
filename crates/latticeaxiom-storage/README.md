# latticeaxiom-storage

Backend-independent authoritative world storage for milestone 3. The crate
defines `WorldStorage`, persistent DTOs, the atomic `ChunkCommit` contract, the
`MemoryWorldStorage` reference implementation, and a shared conformance suite.

A commit completely replaces one chunk's snapshot, persistent spatial
entities, simulation continuation, and artifact receipts. It is guarded by an
optimistic revision condition and a caller-supplied idempotency ID. Exact
retries return the original receipt; reuse with different contents is rejected.

Persistent values use an explicit `LAXE` envelope containing an envelope
version, record kind, schema version, and exact payload length. Only the
payload is encoded with bincode. Chunk keys encode `(world, dimension, x, y,
z)` with sign-bit-flipped big-endian signed coordinates, so byte order follows
numeric order across negative and positive positions.

Run the reference and key/envelope tests with:

```shell
cargo test -p latticeaxiom-storage
cargo clippy -p latticeaxiom-storage --all-targets -- -D warnings
```
