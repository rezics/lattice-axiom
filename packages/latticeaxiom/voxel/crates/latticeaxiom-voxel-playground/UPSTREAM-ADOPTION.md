# `bevy_voxel_world` 0.17.0 adoption report

Assessment date: 2026-08-20. Target: Bevy 0.19.1, native right-handed Y-up.

## Decision

Adopt 0.17.0 conditionally as the D2 presentation/working-set adapter. Do not
adopt its voxel overlay as authority, its `MaterialIndex` as stable content,
its raycast as authoritative selection, or its task completion as the source
revision gate.

The upstream compatibility table maps Bevy 0.19 to `bevy_voxel_world` ^0.17.0.
The workspace must pin exactly `=0.17.0` and let `Cargo.lock` record source and
checksum. The upstream crate is MIT OR Apache-2.0.

## Minimal reproductions and gaps

### 1. Modified voxels are stored in an upstream `HashMap`

Reproduction: call `VoxelWorld::set_voxel`; the public documentation describes
the persistent override layer, and the 0.17.0 source pushes the write into
`VoxelWriteBuffer` before it enters `ModifiedVoxels`. `get_voxel` checks that
buffer and modified-voxel map before generated chunk data.

Gap: this layer has no Lattice `WorldId`, dimension/schema identity, storage
transaction, durability receipt, or canonical ordering. Treating it as a save
would violate D2/D3 authority and determinism.

Adoption response: one-way writes only. Full reload comes from authoritative
decoded chunks; no code in this crate reads upstream state back into gameplay
or persistence.

### 2. `set_voxel` carries no source revision

Reproduction: the 0.17.0 signature is `set_voxel(IVec3,
WorldVoxel<MaterialIndex>)`. It queues a pair and returns no revision-aware
completion receipt.

Gap: an old asynchronous presentation result can otherwise overwrite a newer
authoritative edit.

Adoption response: `ProjectionBatch` carries the authoritative
`ChunkRevision`; `RevisionGate` rejects lower and equal revisions before the
sink and advances only after the full batch succeeds. The test
`stale_and_duplicate_results_never_reach_the_sink` is the executable
reproduction/fix. This does not prove the plugin's internal meshing task is
revision-aware; it prevents stale Lattice submissions at the adapter seam.

### 3. `MaterialIndex` is a compact rendering key, not content identity

Reproduction: `VoxelWorldConfig::MaterialIndex` is constrained to a small
copyable/hashable value and the texture mapper converts it to top/side/bottom
texture indices. It contains no stable package/content identity.

Gap: saving it would make material-table reorderings change world meaning.

Adoption response: place accepts a stable `BlockId` from the authoritative
success receipt and resolves it into a material only while building the
discardable projection. The reverse mapping does not exist.

### 4. Headless separation is feature-level, not plugin-install-level

Reproduction: 0.17.0 exposes no Cargo features and its dependency manifest
enables Bevy asset, log, PBR, render, gizmos, and PNG.

Gap: merely omitting `VoxelWorldPlugin` from a headless App would still compile
the GPU/render dependency closure.

Adoption response: the entire dependency and concrete sink are optional behind
`presentation-adapter`. `--no-default-features` does not compile upstream.

### 5. Coordinates and targeting

Reproduction: upstream keys voxels by Bevy `IVec3` in XYZ order and its raycast
returns presentation data from the plugin's loaded bounds.

Gap: the Lattice runtime has wider signed coordinates, deterministic 5-meter
authoritative DDA, revision observations, and gameplay filtering. Upstream
raycast cannot replace that contract.

Adoption response: conversion is checked from `i64` to `i32`; chunk ownership
uses `div_euclid` so negative boundaries are correct. All six faces reuse the
existing `BlockFaceV1` Y-up contract. Upstream raycast remains suitable only
for client observation/debugging and is absent from this authority adapter.

### 6. Chunk size and performance evidence

Reproduction: the upstream public API documents chunks in 32-voxel units and
default meshing uses block-mesh's simple algorithm. It offers multithreaded
generation/meshing and spawn limits but does not expose Lattice's queue byte
caps or edit-to-visible receipt.

Gap: adoption alone does not prove D2's 128-job/128-MiB mesh caps, 2-ms/16-MiB
apply budget, or 100-ms P95 edit-to-visible target.

Adoption response: use the plugin for the 32-cubed provisional spike, keep the
authoritative bounded queue in `latticeaxiom-voxel-runtime`, and instrument the
client presentation host before claiming D2 performance. The included
Criterion benchmark measures only CPU adapter/revision overhead and is a
regression signal, not a render-budget pass.

## Upstream contribution candidates

- Optional data/core features that do not force Bevy render/PBR dependencies.
- Public revision or generation token carried through remesh completion.
- Explicit bounded queue/in-flight-byte diagnostics and cancellation receipts.
- A documented external-authority mode that disables ownership language for
  the modified-voxel overlay and supports authoritative snapshot refresh.

Until those exist, the narrow one-way adapter is maintainable and does not
justify a fork, project-owned renderer, or physics solver.
