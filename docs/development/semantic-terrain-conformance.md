# Semantic terrain conformance evidence

Recorded 2026-08-30 for the first Terrenia semantic terrain revision.

## Frozen identities and compatibility

- Legacy Terrenia surface providers remain `@1`, algorithm revision 12. Their
  canonical program bytes and existing materialized snapshots are unchanged.
- Semantic Terrenia surface providers are `@2`, algorithm revision 13. The
  production host generation-plan revision is 4 and its built-in
  implementation fingerprint is `terrenia-worldgen-builtin-v2`.
- A semantic hydrologic plan is an optional, bounded generation-plan input.
  Its canonical hash participates in the generation input, provenance, and
  epoch hashes. It cannot be compiled with legacy terrain programs.
- Old/new joins use measured, direction-independent boundary profiles and the
  existing verified boundary-adapter receipt contract.

## Numeric contract

The authoritative macro inputs are independent fixed-point OpenSimplex field
domains for continentalness, uplift, lithology, temperature, precipitation,
infiltration, detail, and the three-dimensional terrain/geology volume. Every
field declares a nonzero voxel scale and a bounded amplitude. Adding a field
does not reseed an existing field.

Terrenia owns closed piecewise-linear splines for base elevation, relief,
roughness, and cliff tendency. Splines contain 2 through 32 strictly ordered
points, include both `-1024` and `1024` endpoints, use integer interpolation,
and clamp outside their authored input interval. Macro noise does not directly
select a final height.

The semantic DEM and effective runoff feed the finite hydrologic-domain
planner and connected topology builder. Runtime density uses the approved DEM
inside the artifact bounds. River, lake, wetland, and ocean envelopes are hard
water-protection masks: three-dimensional detail may carve nearby terrain but
may not add solid density above the approved bed or water surface.

## Conformance results

- The fixed Terrenia semantic quality corpus digest is
  `389e76008dbf36ce25a9684150e0310d4e464f51fb93472e886b5d15940a7020`.
- The corpus contains land, ocean, and high-relief samples and rejects an
  obvious X/Z axis lock.
- Unit conformance covers closed-spline boundaries, independent and bounded
  fields, water-protected density, connected topology, conservative runoff,
  constant lake levels, monotone river profiles, stable canonical hashes, and
  direction-independent epoch adapters.
- Legacy tests continue to exercise the original provider revision beside the
  semantic tests. The switch therefore does not reinterpret old program
  identities or snapshot bytes.

## Optimized baseline

Criterion was run locally with the repository release-style benchmark profile
on 2026-08-30:

| Workload | Observed interval |
| --- | ---: |
| One semantic terrain column | 505.55–605.58 ns |
| Semantic density over 32 cubed voxels | 18.823–22.009 ms |
| Derived density throughput | 1.49–1.74 million voxels/s |

These are recorded baselines, not cross-machine acceptance thresholds. Future
changes must rerun the same benchmark and explain any regression.

## Build-versus-buy record

No production dependency was added. The implementation reuses the
project-owned safe fixed-point OpenSimplex subset introduced by the preceding
field-conformance slice. Closed linear interpolation is a small stable Lattice
contract; adding a general spline crate would increase the dependency and
serialization surface without supplying the required integer rounding and
closed-boundary semantics. The reviewed noise candidates, revisions, licenses,
maintenance status, dependency impact, and semantic gaps remain recorded in
the implementation plan.
