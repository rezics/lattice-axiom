# latticeaxiom-voxel-playground

D2 adoption spike for `bevy_voxel_world` 0.17.0 on the workspace's exact Bevy
0.19.1 baseline. The adopted role is deliberately narrow: the plugin is a
discardable presentation/streaming sink for already-authoritative voxel
facts. `latticeaxiom-voxel-runtime`, gameplay validation, and storage remain
the only owners of cells, chunk revisions, DDA results, commands, and saves.

## Dependency boundary

The crate has no default features. The headless/default graph does not compile
`bevy_voxel_world`, Bevy render, PBR, windowing, or a GPU backend. The optional
`presentation-adapter` feature adds the upstream crate and its unavoidable
render closure:

```text
cargo check -p latticeaxiom-voxel-playground --no-default-features
cargo test -p latticeaxiom-voxel-playground --no-default-features
cargo check -p latticeaxiom-voxel-playground --features presentation-adapter
```

`bevy_voxel_world` has no feature flags in 0.17.0. It unconditionally enables
Bevy asset, log, PBR, render, gizmo, and PNG features, so it cannot participate
in the headless composition even when its plugin is not installed.

## Authority and apply flow

1. Gameplay and `latticeaxiom-voxel-runtime` validate and commit an edit.
2. A successful receipt is translated to a bounded `ProjectionBatch`.
3. `RevisionGate` rejects duplicate and stale per-chunk revisions.
4. The optional upstream sink translates native `(x, y, z)` coordinates and
   writes `WorldVoxel::Air` or `WorldVoxel::Solid(material)`.
5. The gate advances only after all sink writes succeed.

The upstream modified-voxel `HashMap`, `WorldVoxel`, and `MaterialIndex` are
never read back as world truth and are never serialized. Destroying the plugin
world or the revision gate loses only rebuildable presentation state.

The player break/place path consumes only `BlockEditSuccessV1`; rejected or
malformed commands cannot produce a projection batch. A stable `BlockId` is
mapped to a renderer-local material only after a successful place. The same
revision gate handles break (air) and place (solid) updates.

## Automated evidence

The headless suite covers stale/duplicate rejection, failure-before-revision,
all six Y-up faces, negative Euclidean chunk coordinates, chunk-boundary batch
validation, and break/place translation. The feature-gated upstream contract
test compiles the real `VoxelWorld<DefaultWorld>` sink and verifies that known
air is not collapsed into upstream `Unset`; it creates no App, window, renderer,
or GPU device.

The feature-gated Criterion benchmark measures 4,096 revision-checked edit
projections through the real upstream enum conversion without creating a GPU:

```text
cargo bench -p latticeaxiom-voxel-playground \
  --features presentation-adapter --bench projection
```

Timing from a normal developer machine is diagnostic only. A D2 provisional
performance claim still requires the exact optimized reference-host protocol,
including edit-to-presented latency; this CPU seam benchmark does not claim to
measure a rendered frame.

See [UPSTREAM-ADOPTION.md](UPSTREAM-ADOPTION.md) for the reproduction and
adoption decision.
