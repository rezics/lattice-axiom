# Upstream library audit

Status: accepted development record, 2026-08-29.

This record captures the repository-wide build-versus-buy review requested for
general infrastructure, math, procedural generation, voxel traversal, meshing,
and development observability. It is not a rule that every small operation must
be delegated to a crate. The decision gates are the semantics, determinism,
performance, memory, safety, supported platforms, headless CI, license, and
dependency budget defined in `AGENTS.md`.

Run `task audit:upstream` to recheck the adopted adapters, compatibility corpus,
benchmark target, Logdy launcher, and RustSec advisory policy. Meshing changes
must also run `task audit:upstream:bench` in an otherwise idle optimized build
and record the machine and before/after intervals.

## Adopted upstream facilities

| Area | Exact upstream | Scope and dependency impact | Decision evidence |
| --- | --- | --- | --- |
| Engine math | Bevy 0.19.1 re-exporting glam 0.32.1, MIT OR Apache-2.0 | Already present through Bevy; no new direct dependency | Engine-native movement, transforms, collision normals, and distances use Bevy `Vec2`/`Vec3`. This preserves the repository's Bevy boundary instead of introducing a project math facade. |
| Checked alignment | Rust 1.97 `usize::checked_next_multiple_of` | Standard library; no dependency | Replaces the ABI build script's hand-written round-up formula while retaining explicit overflow failure and generated-layout tests. |
| Temporary test trees | tempfile 3.27.0, MIT OR Apache-2.0 | Dev dependency only; `getrandom` remains confined to fixtures | `TempDir` replaces repeated timestamp/process-ID directory allocation and manual cleanup. RAII cleanup and exclusive creation remove collision and stale-fixture failure modes. |
| Cargo protocol | cargo_metadata 0.23.1, MIT | `latticeaxiom-compose`; no default features | Upstream `Metadata`, `MetadataCommand`, and `Message::parse_stream` replace local mirrors of Cargo JSON. Existing containment, target matching, and artifact validation remain project adapters. |
| CLI parsing | clap 4.6.6, MIT OR Apache-2.0 | `latticeaxiom-compose`; `default-features = false`, `std` and `derive` only | A typed grammar replaces the hand-maintained state machine. Contract tests freeze aliases, repeated flags, defaults, and project error-domain mapping. |
| Developer Web logs | Bevy `LogPlugin` 0.19.1 plus Logdy 0.17.1 at `dd9a1c03694bd6ccfad05c1ad2cb0a5ce3c580eb`, Apache-2.0 | Logdy is a verified external development binary, not a Cargo or production runtime dependency | `task dev` streams client stdout/stderr to a localhost Web UI while preserving console output and the client exit status. Release artifacts are pinned by size and SHA-256. See [observability](observability.md). |

Primary references:

- <https://docs.rs/glam/0.32.1/glam/>
- <https://doc.rust-lang.org/1.97.1/std/primitive.usize.html#method.checked_next_multiple_of>
- <https://docs.rs/tempfile/3.27.0/tempfile/>
- <https://docs.rs/cargo_metadata/0.23.1/cargo_metadata/>
- <https://docs.rs/clap/4.6.6/clap/>
- <https://github.com/logdyhq/logdy-core/tree/v0.17.1>

## Meshing: keep the production implementation

`block-mesh` 0.2.0 (MIT OR Apache-2.0), release source revision
`a088508ed7ea4b69b62ea935cf94d141613127a3`, was evaluated as a direct dev
dependency. Its repository and API are the closest mature Rust candidate:

- <https://docs.rs/block-mesh/0.2.0/block_mesh/>
- <https://github.com/bonsairobo/block-mesh-rs/tree/a088508ed7ea4b69b62ea935cf94d141613127a3>

The audit target in
`crates/latticeaxiom-voxel-mesh/tests/block_mesh_audit.rs` proves equivalent
visible unit-face sets for opaque voxels across 64 deterministic corpora. It
also records two material semantic gaps:

- fixed corpus seed 19 produces 122 equivalent visible unit faces but a
  different stable greedy partition: 100 local quads versus 101 upstream;
- matching translucent neighbors require the local material-aware interface
  contract: 12 local faces versus 10 through the upstream visibility model.

Criterion comparison on the audit machine used a one-second warm-up, two-second
measurement, and ten samples:

| Corpus | Local 95% interval | `block-mesh` 95% interval | Result |
| --- | ---: | ---: | --- |
| Solid | 1.2197–1.2846 ms | 580.45–612.41 us | Upstream is about 2.1x faster. |
| Layered | 769.79–808.93 us | 347.87–375.26 us | Upstream is about 2.2x faster. |
| Checker | 1.0627–1.1605 ms | 1.0187–1.1623 ms | Intervals overlap. |

The performance win does not satisfy stable output and translucent-occlusion
semantics, so the production mesher remains project-owned. The upstream
implementation stays dev-only as an executable conformance and performance
comparison. Reconsider adoption when an adapter can preserve both gaps, or an
accepted ADR relaxes them, and rerun the full benchmark on representative
materials before changing the production path.

## Math and procedural generation: deliberate boundaries

### Portable action values

glam 0.32.1 provides mature vector normalization and is already present through
Bevy. `ActionAxis2V1` nevertheless remains a two-scalar stable DTO: it sanitizes
non-finite components, clamps only magnitudes above one, and deliberately
rescales `f32::MAX` inputs without producing zero or NaN. Converting this public
boundary into a Bevy/glam type would couple the portable action contract to an
engine release and would not remove its validation policy. Engine-side math
continues to convert into and use Bevy vectors.

### Authoritative terrain fields

FastNoise Lite 1.1.1 (MIT) was reviewed as a representative upstream noise
candidate:

- <https://docs.rs/fastnoise-lite/1.1.1/fastnoise_lite/>
- <https://github.com/Auburn/FastNoiseLite>

It offers a broad, portable floating-point noise API, but the current D4
world-generation output is a versioned fixed-point contract. The local field
uses domain-separated hashes, Euclidean division for signed coordinates,
saturating arithmetic, exact quintic interpolation, and frozen cross-platform
outputs. Replacing it would define a new world-generation version rather than a
refactor. Keep the fixed-point field; evaluate an upstream noise library only
for a new version with golden-map migration, cross-target conformance, and
representative generation benchmarks. The algorithmic reference remains
OpenSimplex2 revision `4cd120d35bfc27096698de90d1bcbf4f9d359a3b`
(CC0-1.0), as recorded beside the implementation.

### Committed voxel DDA

No general crate reviewed implements the runtime's full
selection contract: chunk-anchored `f64` origins, exact negative-boundary cell
ownership, simultaneous tied-axis advancement, stable X/Y/Z face priority,
five-meter validation, missing-projection fail-close behavior, and committed
revision evidence. Avian's ray queries operate on the physics collider world,
not storage-committed voxel projections. `dda-voxelize` 0.2.0-alpha.1 (MIT)
voxelizes triangle meshes and is both pre-release and a different operation:

- <https://docs.rs/dda-voxelize/0.2.0-alpha.1/dda_voxelize/>
- <https://github.com/MIERUNE/dda-voxelize-rs>

Keep `VoxelRuntime::raycast_committed` and its boundary/fault tests. A future
candidate must first pass those tests through an adapter and demonstrate no hot
path regression.

### Padded indexing

`ndshape` 0.3.0 (MIT OR Apache-2.0) supplies fast generic linearization and is
already present transitively in the dev-only `block-mesh` audit:

- <https://docs.rs/ndshape/0.3.0/ndshape/>
- <https://github.com/bonsairobo/ndshape-rs>

`PaddedChunk` also owns nonzero and maximum-edge validation, checked `usize`
volume construction, one-voxel halo conversion, and optional bounds failure.
Adopting `ndshape` directly would replace one x-fast expression while retaining
the entire checked wrapper and adding it to production. Keep the local type;
reconsider only if multiple production consumers need generic dimensional
shape operations beyond this contract.

## Maintenance, security, and revisit policy

All adopted crates use licenses already accepted by `deny.toml`; Cargo releases
are locked with registry checksums. `cargo deny check advisories` completed
without an unaccepted advisory failure on 2026-08-29. Time-bounded transitive
exceptions remain explicit in `deny.toml`. During this review, yanked
`chacha20` 0.10.1 was updated in place to 0.10.2 without changing its dependent
feature graph.

Logdy is local-development-only, binds to loopback, disables analytics and
update checks, and is verified before execution. `block-mesh` and `ndshape`
remain dev-only audit inputs. Candidate release age or popularity alone is not
proof of fitness: any revisit must record an exact release or revision, license,
advisory result, feature tree, characterization tests, and representative
before/after measurements.
