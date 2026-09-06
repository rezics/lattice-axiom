# latticeaxiom-worldgen

Deterministic, bounded world-generation contracts and algorithms. The crate is
implemented in Rust and uses Bevy task pools for selected bounded parallel
planners. It has no renderer, package loader or storage writer dependency;
the production host owns streaming admission and durable publication. The
planned independently releasable toolkit boundaries are described in
[the research and extraction direction](../../docs/worldgen-toolkit-research.md).

## Worldgen V2

Worldgen V2 intentionally starts a new world-generation epoch. Reusing V1
chunk bytes would preserve incompatible terrain edges, the smaller vertical
range, and old cave/hydrology semantics. Existing prototype worlds may be
deleted or archived; automatic V1-to-V2 terrain blending is not claimed.

The V2 seed root depends only on the normalized world seed. Provider versions,
implementation fingerprints, locks, and activation receipts remain part of
provenance and stale-result rejection, but no longer act as accidental PRNG
salt. A coordinator-only upgrade therefore does not reroll terrain. A real
algorithm or resolved-config change still changes the generation epoch and is
an explicit compatibility decision.

The profiles supplied by `@terrenia/worldgen` are:

- `balanced`: mixed continents, hills, mountains, lakes, rivers, and caves;
- `continental`: broad interiors, long ranges, and deeper oceans;
- `archipelago`: island groups, shelves, coastlines, and more surface water;
- `alpine`: tall continuous ridges with stronger vertical relief;
- `eroded`: plateaus, valleys, wetlands, and stronger erosion;
- `wild`: rare volcanoes, greater relief, and a 1024-voxel vertical column.

The standard profiles use the inclusive range `-128..=383` (512 voxels) with
sea level 64. `wild` uses `-256..=767` (1024 voxels). Resolved configs are
integer-only, validated, canonicalized, hashed, and persisted with world
metadata. Terrain shape is separated from climate/material selection.

The macro field composes continentalness, islands, erosion, ridges, hills,
plateaus, detail, wetlands, volcanoes, deterministic lake basins, river
incision, aquifers, lava, and cave topology. It uses correlated fixed-point
gradient fractals and stable ordered iteration. Cave topology has its own
64-voxel planning scale rather than inheriting the much broader climate grid.

## Streaming boundary

Chunk generation is intentionally allowed to be expensive. In production it
runs as owned jobs on Bevy's `AsyncComputeTaskPool`; the fixed path only admits
bounded work, polls completed tasks, and applies a capped number of results.
Pending, in-flight, waiting-to-apply, resident, derived, and dirty sets all have
hard bounds. Mesh/collider readiness gates movement without executing worldgen
on the main loop. Render, simulation, resident, and prefetch distances are
separate contracts; shrinking view distance rechecks retained chunks until the
grace period expires.

## Design references

Minecraft is a behavioral and data-format reference, not a terrain template.
Java 1.18 established independent terrain shape/biome placement, separate
simulation distance, background worldgen threads, and the `-64..319` generated
range. The V2 standard range deliberately exceeds that vertical span while
keeping power-of-two storage-friendly bounds.

Terrain-mod references were inspected for concepts such as large coherent
landforms, erosion, configurable presets, underground rivers, and height-map
extensibility. No source or assets were copied, and none is a dependency:

- [Tectonic](https://github.com/Apollounknowndev/tectonic), revision
  `34241bdb35acda67b5367d49f354c66c05e098e2`;
- [NeoTerraForged](https://github.com/equalizer32/NeoTerraForged), revision
  `2c029cad1598e53bfca3ef2465128f6d78ca5e22`;
- [BigGlobe](https://github.com/Builderb0y/BigGlobe), revision
  `de3dec9cc40e56ae7a6313ba62fdb5c688412197`;
- [Minecraft Java 1.18 release notes](https://feedback.minecraft.net/hc/en-us/articles/4415128577293-Minecraft-Java-Edition-1-18);
- [Minecraft Java 21w38a notes](https://feedback.minecraft.net/hc/en-us/articles/4409891990285-Minecraft-Java-Edition-Snapshot-21w38a);
- [Minecraft Wiki world-generation index](https://minecraft.wiki/w/World_generation).

Reference checkouts may live only under `.temp/reference/` and remain
untracked.

## Evidence

Run correctness, lint, documentation, and optimized worker-throughput evidence
with:

```text
cargo test --locked -p latticeaxiom-worldgen
cargo clippy --locked -p latticeaxiom-worldgen --all-targets -- -D warnings
cargo rustdoc --locked -p latticeaxiom-worldgen --lib -- -D warnings
cargo bench --locked -p latticeaxiom-worldgen --bench chunk_generation -- --test
```

`D4SnapshotCandidateV1` is not the final portable world-wire envelope and does
not by itself prove durability. The host must still validate activation, CAS,
package provenance, bounded scheduling, and storage publication.
