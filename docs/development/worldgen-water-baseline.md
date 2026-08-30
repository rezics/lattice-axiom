# Terrain, hydrology, fluid, and water baseline

Status: Slice 0 characterization record, 2026-08-30.

This record freezes the evidence immediately before implementing
`docs/plan/world-generation-hydrology-and-water.md`. It characterizes the
existing V2 terrain field, V5 river proxy, V6 occupancy path, `FluidStateV1`
planner, and generic translucent mesh route. It is not an acceptance record for
the replacement epoch.

## Reproduction identity

- Commit: `d10a382e504040802e4eafed99f64ce30573d719`.
- Build profile: Cargo `bench` (`optimized`), Criterion one-second warm-up,
  two-second measurement, ten samples.
- Toolchain: `rustc 1.97.1 (8bab26f4f 2026-07-14)`, LLVM 22.1.6,
  `x86_64-pc-windows-msvc`.
- Machine: Intel Core i9-14900HX, 24 physical/32 logical cores, 32 GiB RAM.
- Operating system: Windows 11 Pro for Workstations 10.0.26200, build 26200,
  64-bit.
- Terrain configuration: `TerrainConfigV2::representative_test_baseline()`,
  world seed integer `42`.
- Map corpus: inclusive `[-8192, 8192]` X/Z square, 64-voxel spacing,
  257 by 257 samples. Family tags use the explicit order in the test rather
  than Rust enum layout.

The machine was not placed in a controlled performance laboratory state.
Intervals below establish a local comparison baseline; later acceptance runs
must repeat the same commands on an otherwise idle accepted target profile.

## Fixed field evidence

`terrain_field::tests::legacy_fixed_seed_map_morphology_and_directional_energy_are_frozen`
locks the following current-path values:

| Evidence | Value |
| --- | ---: |
| Height/family map SHA-256 | `4b48cf41fb05290798cc8931c839ae63a29d15b01698f20403cd9f5b681f1236` |
| Drainage-distance map SHA-256 | `bf28bc2d11783b8e3140a24eba6c86eda83594ec1039c00e8895943e8966760a` |
| Minimum / maximum height | -1 / 168 |
| Land / ocean columns | 48,579 / 17,470 |
| Lake columns | 319 |
| Equal-height X/Z steps | 33,704 |
| X / Z first-difference energy | 215,370 / 210,911 |
| NE / SE diagonal first-difference energy | 384,855 / 355,511 |
| Drainage-center proxy columns | 34,139 |

The four directional-energy counters are an integer spatial-spectrum proxy,
not the replacement field's radial-spectrum acceptance test. They make the old
field's axis/diagonal balance reproducible without introducing floating-point
output into the authoritative crate.

## Optimized timings

| Benchmark | 95% interval |
| --- | ---: |
| V2 baseline 32-cubic snapshot candidate | 21.796-25.266 ms |
| V2 baseline, 4,096 terrain columns | 9.7628-10.773 ms |
| V2 high-relief, 4,096 terrain columns | 9.3377-9.7132 ms |
| V6 hydrology 32-cubic occupancy candidate | 43.108-53.018 ms |
| V6 hydrology 32-cubic generation pipeline | 26.247-31.446 ms |
| V6 hydrology 32-cubic snapshot reuse | 5.6740-6.1632 ms |
| Greedy mesh, solid 32-cubic plus halo | 0.77222-1.2566 ms |
| Greedy mesh, layered 32-cubic plus halo | 1.2432-1.3578 ms |
| Greedy mesh, checker 32-cubic plus halo | 1.7632-1.8998 ms |
| Fluid v1 planner, 32-cubic with 64 frontier cells | 0.54150-0.84806 ms |

The fluid-planner fixture intentionally excludes host scanning and storage
publication so later planner changes remain comparable.

## Frozen failure evidence

- `DrainageFieldV3` derives distance from the absolute value of a noise field;
  `NaturalSamplerV1` then uses a basin hash as a sparsity switch. Neither value
  proves a downstream receiver, contributing area, outlet, or conserved runoff.
- `TerrainFieldV2::lake_basin` chooses jittered hash cells and a quantized anchor
  height. It has no catchment, spill saddle, depression hierarchy, or declared
  endorheic policy.
- `host::fluid::fluid_frontier` scans every non-empty fluid cell each tick.
  `host::fluid::apply_plan` applies only in-chunk mutations and never consumes
  `FluidTickPlan::boundaries()` or persists `next_frontier()`.
- `MeshGroup::Translucent` is the only water-capable group and always culls back
  faces. The ignored regression test
  `water_must_not_inherit_generic_translucent_back_face_culling` remains red
  until Slice 1 introduces a distinct water identity.
- `ProductionCamera` carries no air/water/lava medium component, and the client
  schedule does not sample authoritative fluid occupancy at the eye.

These failures are independent: later commits should turn each corresponding
gate green without rewriting this legacy evidence.
