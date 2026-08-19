# latticeaxiom-voxel-mesh

In-house voxel meshing: turns padded voxel sample volumes into per-face quad
lists, either as culled unit quads (`visible_faces`) or greedily merged
rectangles (`greedy_quads`). Native to the project's right-handed Z-up
convention (ADR 0011): face directions are named after world axes, and quad
corners wind counter-clockwise seen from outside, matching the rendering
facade's front-face rule.

Pure data-to-data: no renderer, world-model, or backend dependencies, so it
runs in tests, headless CI, and worker threads. The chunk mesh compiler
integrates it at milestone 3; benchmarks join with that integration.

Correctness is guarded by property tests: culled output matches a brute-force
exposed-face count, and greedy quads tile the visible faces exactly (equal
area, no overlap, no leakage, uniform merge values per quad).

## Attribution

The algorithm structure — padded sample volumes, per-layer face masks, greedy
rectangle growth — references
[block-mesh-rs](https://github.com/bonsairobo/block-mesh-rs)
(MIT OR Apache-2.0, unmaintained since 2022). Reimplemented from scratch for
this project's conventions; no code was copied verbatim.
