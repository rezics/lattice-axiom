# latticeaxiom-voxel-mesh

Pure CPU derived-data crate for deterministic voxel face culling and greedy
meshing. It consumes a chunk interior plus a one-voxel halo and produces
renderer-independent quads in stable opaque, cutout, translucent, and
emissive groups.

The crate follows Bevy-native right-handed Y-up coordinates (`+X` right,
`+Y` up, forward `-Z`) but does not depend on or wrap Bevy. It owns no world,
renderer, task, or process handles. Every result carries the caller-supplied
chunk coordinate, source epoch, revision, and canonical fingerprint receipt;
the authoritative owner must compare that receipt again before applying an
asynchronous result.

`GreedyMesher::mesh_into` and `visible_faces_into` retain scratch/output
allocations for benchmark and worker-loop use. Output order never depends on
a hash map.
The reproducible 32³ solid/layered/checker throughput corpus runs with:

```text
cargo bench -p latticeaxiom-voxel-mesh --bench meshing
```
