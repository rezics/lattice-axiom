# Hydrologic topology and static-reservoir conformance

Recorded 2026-08-30 for
`latticeaxiom:hydrologic-topology/channel-dag@1` on the development target
(`x86_64-pc-windows-msvc`, Rust 1.97.1, optimized Criterion profile).

## Contract

Topology extraction consumes one immutable bounded
`HydrologicDomainPlanV1`. It selects a canonical receiver for channel cells
from routed flux, slope, and stable coordinate order, then performs a second
exact conservative accumulation. Non-channel cells retain the fixed MFD
weights; channel cells send all discharge to their selected receiver. Segment,
basin, outlet, and water-body identities are domain-separated hashes of
canonical inputs.

Revision 1 has an acyclic single-receiver channel graph. Every released
segment ends at another segment, a level retained lake, an ocean outlet, or a
declared boundary port. Strahler order is evaluated in stable topological
order. River profiles contain two quantized points and cannot rise downstream;
large drops are explicitly marked as waterfalls. River geometry publishes a
fixed-point centerline, tangent, bed, bank, floodplain, and signed-distance
influence envelope.

Static generated water is a sparse, canonical chunk candidate. Its top cells
encode the accepted `FluidStateV1` levels `0..=7`, its flow is derived from the
published river tangent or waterfall state, and its runtime frontier is always
empty. Work is bounded by a chunk edge of 128 and the configured cell/result
byte budgets. Oceans and retained lakes use one equipotential surface.

## Headless evidence

The unit corpus covers:

- a plane whose channels are connected, acyclic, and runoff-conservative;
- a retained bowl that publishes one constant-level lake and spill outlet;
- downstream-monotone profiles and continuous SDF/profile joins;
- deterministic query order and a canonical topology digest;
- level-aware static materialization, presentation flow, and an empty runtime
  frontier;
- configured graph limits and the chunk-edge boundary-plus-one failure.

The 17 by 17 known-answer topology digest is
`5bcc123eefa54f6c43b460f617531a278704563f16ba8938c40cf9e860740d1e`.
The underlying domain digest changed to
`063b37417cb15e0f0e6f71718ffdb33f6f17f9273c57d80e99e9f0287f707068`
because Slice 6 persists the source-runoff field needed to prove the topology
pass's conservation equation.

## Build-versus-buy record

No production dependency was added. Priority-Flood/MFD inputs come from the
project-owned Slice 5 contract. Channel extraction, stable graph traversal,
fixed-point segment projection, integer square root, and reservoir occupancy
are small contract-specific operations. Landlab revision
`168494aa9658e08d2e7219d0904004a10a600236` (MIT) was retained only as an
offline routing/contributing-area oracle because its Python scientific stack
does not fit the runtime, deterministic-byte, dependency, or target-platform
requirements. NeoTerraForged revision
`9a7c782cbf642fc9cc9b86201159a0735a38b388` (MIT) informed batching and river
geometry conformance ideas but was not copied or adopted as a Java runtime.

## Benchmark method

Run:

```text
cargo bench -p latticeaxiom-worldgen --bench hydrologic_domain -- hydrologic_topology_v1
```

The fixture is a deterministic 64 by 64 domain at eight-voxel spacing. The
benchmark reports cold topology extraction and sparse materialization of one
intersecting 16-cubed chunk through a prevalidated sampler. The first measured
cold topology interval is 49.234-51.356 ms (79.757-83.194 thousand domain
samples/s); intersecting chunk materialization is 29.881-30.697 microseconds
(8.34-8.57 million columns/s). These measurements establish the first local
baseline rather than a release threshold.
