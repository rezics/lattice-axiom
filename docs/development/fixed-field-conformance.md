# Fixed authoritative field conformance

Status: Slice 4 implementation evidence, 2026-08-30.

This record covers the field candidates added for
`docs/plan/world-generation-hydrology-and-water.md`. They do not change the
active terrain epoch. The legacy V2 field remains the materialization path until
the later epoch-switch acceptance slice.

## Source and adoption decision

The algorithmic oracle is K.jpg's `OpenSimplex2` revision
`4cd120d35bfc27096698de90d1bcbf4f9d359a3b`, licensed CC0-1.0. The reviewed
Rust reference uses mutable static lookup initialization and floating-point
authoritative evaluation. It is therefore neither a production dependency nor
copied verbatim. The implementation independently adapts only the 2D
`OpenSimplex2S` vertex topology and 3D `OpenSimplex2F` XZ-improved topology,
using fixed constants, immutable arrays, wrapping lattice hashes, safe Rust,
and project-owned DTOs.

The exact alternative versions, licenses, maintenance state, feature and
transitive impact, semantic gaps, and dependency decision remain recorded in
the plan's build-versus-buy table. No dependency or Cargo feature changed in
this slice.

## Numeric contract

- Input is a signed integer lattice part plus a Q0.32 fraction.
- `from_voxel` uses Euclidean division for negative coordinates and rounds the
  fractional conversion toward the lower lattice point.
- The transformed phase and output use signed Q30. Multiplication reductions
  round to nearest; exact halves round away from zero. A lattice-floor phase
  conversion drops the two low Q32 bits toward zero to remain in `[0, 1)`.
- The inclusive integer magnitude is `2^47 - 1`. This covers the full `i32`
  chunk-coordinate range at 32 voxels per edge.
- At that bound, transformed coordinate products remain below `2^114` and
  attenuation/gradient products remain below `2^100`, inside signed `i128`.
- Lattice prime and seed mixing deliberately use wrapping `i64`, matching the
  upstream two's-complement hash contract.
- Results are clamped to signed `[-1, 1]` Q30 at the named output boundary.

The stable revision identities are:

- `latticeaxiom:field/open-simplex-2s-2d-fixed@1`
- `latticeaxiom:field/open-simplex-2f-3d-fixed@1`

## Oracle and deterministic evidence

`cargo run -p latticeaxiom-worldgen --example fixed_field_oracle` compares
eight frozen points against values emitted by the reviewed upstream revision.
The maximum absolute error is `0.000000304`; the tool fails above `0.000001`.
The vectors include zero, negative fractions, large coordinates, multiple
signed seeds, and a Q0.32 point one unit above a negative integer boundary.

The mixed 2D/3D little-endian Q30 corpus SHA-256 is:

```text
4dab4b07b2bc260dc8a2ec356a08f9f06f76596f382f23e4dd9ca65531dd97c0
```

Tests also cover both inclusive coordinate limits, one-past rejection,
Euclidean negative division, stable algorithm identities, derivative plateau
count, horizontal and diagonal energy balance, and a headless 2D DFT gate that
rejects axis- or diagonal-dominant spectral bands. The digest is the
cross-target byte-identity gate; CI on each supported target must reproduce it.

## Optimized benchmark

Machine, toolchain, OS, and optimized profile are the same as
`worldgen-water-baseline.md`. The command was:

```text
cargo bench -p latticeaxiom-worldgen --bench generation -- fixed_field_candidates_v1 --sample-size 10 --warm-up-time 1 --measurement-time 2
```

| Corpus | 95% interval | Throughput interval |
| --- | ---: | ---: |
| `OpenSimplex2S` 2D, 256 squared | 9.4638-9.7298 ms | 6.7356-6.9249 million samples/s |
| `OpenSimplex2F` 3D, 32 cubed | 6.4184-7.1549 ms | 4.5798-5.1053 million samples/s |

These are first-candidate measurements, not a terrain-generation release
budget. Later domain and epoch slices must measure complete field graphs,
cache behavior, chunk sampling, memory, and Bevy task-pool scaling.
