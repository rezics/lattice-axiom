# Landscape evolution conformance evidence

Recorded 2026-08-30 for
`latticeaxiom:landscape-evolution/implicit-stream-power-linear-diffusion@1`.

## Selected subset

The implementation independently applies the linear stream-power case
`m = 1/2`, `n = 1` on the canonical single-receiver channel DAG. For each
channel substep it uses the downstream-to-upstream implicit update

```text
h(new) = (h(old) + alpha * h(new, downstream)) / (1 + alpha)
alpha  = K * sqrt(Q) * dt / segment_length
```

All values, square roots, division, and rounding are integer and fixed point.
The v1 algorithm does not implement nonlinear slope exponents, sediment
transport, deposition, or discharge thresholds.

Hillslopes use an explicit four-neighbor linear diffusion stencil with two
buffers. External/halo samples and approved channel samples are fixed boundary
conditions. Config validation proves `D*dt/dx^2 <= 1/4` before work begins.
This channel-boundary choice avoids silently treating diffused hillslope
sediment as bedrock that the channel must re-erode.

Routing and topology are immutable during an outer iteration. Priority-Flood,
MFD accumulation, canonical channel extraction, lake classification, and water
profiles are rebuilt only after incision and diffusion finish. Every final
topology therefore re-passes the same DAG, runoff-conservation, outlet, lake,
and monotone-profile validators as a non-eroded plan.

## Bounds and determinism

The closed config caps outer iterations, incision and diffusion sweeps, work,
temporary bytes, and result bytes. It also defines timestep, uplift,
erodibility, diffusivity, and convergence in fixed-point units. Cancellation
is observed at deterministic work intervals and no partial plan is returned.

Independent domains run on Bevy task pools. Inputs and results are sorted by
stable domain identity, so task count, completion order, and request order do
not affect plan hashes. Incision inside one domain remains serial because each
upstream implicit update consumes its already-updated downstream elevation.

## Conformance results

- Disabled evolution returns domain and topology bytes identical to direct
  planning.
- Analytic sloping DEMs produce positive incision and retain downstream-
  monotone final river profiles.
- A peaked synthetic hillslope is smoothed while every fixed halo sample stays
  byte-exact.
- Boundary-plus-one iteration counts, unstable diffusion coefficients, and
  hard resource limits fail before unbounded work.
- Reversed multi-domain request order and four-worker completion order produce
  identical canonical hashes.
- The evolved 17-by-17 plane golden hash is
  `6a1335a0637311d48e13ac442cf5efcf1bdbd3c59a5ff4a3f17c905b129933c2`.
- Semantic terrain artifacts retain both the untouched initial DEM and the
  final evolved DEM/topology evidence.

## Optimized baseline

Criterion was run locally with the repository optimized benchmark profile on
2026-08-30. A 64-by-64 domain with two outer iterations, one incision sweep,
two diffusion sweeps, and a hydrology/topology rebuild per iteration measured
418.61–429.86 ms (9.53–9.78 thousand raster samples/s). This is the first
regression baseline, not a cross-machine acceptance threshold.

## Research and build-versus-buy record

The implementation follows the dependency ordering and linear implicit form
described by Braun and Willett (2013), while intentionally excluding the
nonlinear Newton solve and threshold extension discussed by Braun and Deal
(2023). The coupled-model review by Litwin, Malatesta, and Sklar (2025)
motivated making channel cells fixed diffusion boundaries rather than applying
one undifferentiated stream-power-plus-diffusion material model everywhere.

No production dependency was added. FastScape and Landlab were considered as
algorithm/conformance references, but their floating-point runtimes, external
array/data-model contracts, and additional dependency surface do not define
the persisted Lattice fixed-point artifact. The project-owned subset is small,
bounded, covered by analytic/golden tests, and delegates routing to the
existing validated planner instead of copying an upstream implementation.

Primary references:

- Braun and Willett, *Geomorphology* 180–181 (2013),
  <https://doi.org/10.1016/j.geomorph.2012.10.008>
- Braun and Deal, *JGR Earth Surface* 128 (2023),
  <https://doi.org/10.1029/2023JF007140>
- Litwin, Malatesta, and Sklar, *Earth Surface Dynamics* 13 (2025),
  <https://doi.org/10.5194/esurf-13-277-2025>
