# Hydrologic-domain v1 conformance record

Date: 2026-08-30.

## Frozen candidate

`latticeaxiom:hydrologic-domain/priority-flood-mfd-p1@1` is a bounded CPU and
headless planning candidate. It keeps the original Q24.8 DEM, derives a
depression forest, applies closed retain/wetland/breach/fill policy, and routes
Q16 effective runoff with Freeman-style MFD `p = 1`. Every receiver row uses a
denominator of 1,000,000. Integer quotients are assigned first; remaining
weight and runoff units go to the largest fractional remainder, then the
lowest canonical receiver coordinate/direction. Receiver weights and routed
runoff therefore conserve exactly.

Priority-Flood uses `(elevation, row-major index)` heap order. Flats route by
the stable flood rank toward an outlet, while retained components receive a
stable distance rank toward their pit. The published graph is checked with a
stable Kahn traversal; a cycle or nonterminal orphan rejects the entire plan.

## Bounds and publication

Inputs validate grid, port, graph, depression, queue, work, temporary-byte,
result-byte, and world-coordinate bounds before or during allocation. A caller
cancellation flag is sampled at fixed work intervals and never enters output
bytes. Domain batches use Bevy 0.19.1 `TaskPool`; both inputs and completed
results are sorted by stable domain ID, so task count and completion order do
not affect publication.

The exact cache key contains dimension, domain ID, generation epoch, canonical
input hash, algorithm revision, and exact config hash. The cache is a bounded
`BTreeMap` of immutable `Arc` plans. It neither silently evicts nor accepts two
byte-distinct plans under one key.

## Sources and build-versus-buy

- Barnes, Lehman, and Mulla, *Priority-Flood: An Optimal Depression-Filling and
  Watershed-Labeling Algorithm for Digital Elevation Models* (2014), DOI
  `10.1016/j.cageo.2013.04.024`.
- Barnes et al., *Fill-Spill-Merge* depression hierarchies (2021), DOI
  `10.5194/esurf-9-105-2021`.
- Prescott et al., MFD, D-infinity, and IDS evaluation (2025), DOI
  `10.5194/esurf-13-239-2025`.
- Tarboton, D-infinity flow direction (1997), DOI `10.1029/96WR03137`.
- Bevy task pools from the locked Bevy `0.19.1` source and API.

No production hydrology dependency was added. General raster hydrology crates
reviewed through the plan's 2026-08-30 dependency survey did not jointly offer
the required fixed-point residual semantics, stable DTOs, cancellation and
allocation bounds, port identities, and byte-invariant merge contract. The
implementation is an independent, safe-Rust adaptation of the published
algorithms. The only dependency impact is enabling the already locked,
workspace-wide Bevy `0.19.1` `std` and `multi_threaded` surfaces for its task
pool; there is no new package or license in `Cargo.lock`.

## Conformance corpus

Unit tests cover analytic planes and bowls, exact weight and runoff
conservation, terminal reachability, retained-lake metadata, opposite-side
port signatures, port registration order, 1-thread/4-thread byte identity,
hard one-past bounds, cancellation, bounded cache behavior, and aggregate
MFD `p = 1`, development MFD `p = 1.1`, and D-infinity orientation metrics.

The 5 by 5 canonical plan fixture hashes to
`063b37417cb15e0f0e6f71718ffdb33f6f17f9273c57d80e99e9f0287f707068`.

## Optimized benchmark baseline

Measured with `cargo bench -p latticeaxiom-worldgen --bench
hydrologic_domain -- --noplot` on an Intel Core i9-14900HX, Windows MSVC,
Rust 1.97.1, optimized workspace bench profile:

| Workload | Median estimate | Throughput estimate |
| --- | ---: | ---: |
| 64 square, spacing 8, halo 8, cold | 43.435 ms | 94.301 K samples/s |
| 96 square, spacing 16, halo 16, cold | 106.87 ms | 86.234 K samples/s |
| 128 square, spacing 32, halo 24, cold | 192.39 ms | 85.159 K samples/s |
| exact immutable cache hit | 19.8-20.1 ns | lookup only |
| eight 64-square domains, one worker | 357.12 ms | 91.757 K samples/s |
| eight 64-square domains, two workers | 189.69 ms | 172.75 K samples/s |
| eight 64-square domains, four workers | 110.60 ms | 296.26 K samples/s |

The measured four-worker batch is 3.23 times faster than the one-worker batch.
These numbers establish the first baseline; they are not a main-loop budget.
