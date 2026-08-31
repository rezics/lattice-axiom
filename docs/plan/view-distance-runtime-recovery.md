# View-distance runtime recovery plan

Status: superseded after failed fixed-window visual acceptance, 2026-08-31.

The runtime-truthfulness work below was implemented as an uncommitted candidate,
but the 2026-08-31 visual review correctly rejected it: reporting the six-chunk
full-resolution budget more precisely did not make a selected 21-chunk render
distance render 21 chunks. The extra Settings status row also added internal
telemetry where the player asked for a normal option. Do not commit or extend
this plan as the product solution. Its evidence remains useful, but the
replacement contract and implementation sequence are now in
[`render-simulation-and-far-terrain.md`](render-simulation-and-far-terrain.md).

## Problem and observed evidence

The 2026-08-31 visual review showed `View 6/26` while the authored settings
slider held 26. Moving the slider above six changed the persisted request but
did not change visible terrain. The display did not name either number or the
binding constraint, so it reasonably appeared to claim a 26-chunk view.

The runtime currently means:

- `26`: requested and host-admitted horizontal radius in chunks;
- `6`: effective full-resolution radius after the resident working-set budget;
- one chunk edge: 32 voxels/meters, so radius 6 is 192 meters to the cardinal
  boundary and the current square interest footprint is 13 by 13 columns;
- seven vertical chunks are retained per horizontal column;
- `13 * 13 * 7 = 1,183`, exactly the frozen desktop-reference resident cap.

The setting therefore reaches the typed runtime but cannot raise the effective
radius. This is a capability clamp with insufficient product feedback, not a
camera far-plane failure. Bevy 0.19.1's default perspective far plane is 1,000
world units; the current terrain ends around 192 meters. Air fog is separately
fixed at 512 meters and would become a second mismatch if effective streaming
were raised without synchronizing camera presentation.

## Binding constraints and current references

- Lattice ADR 0034 requires the authored request range to remain `2..=32`
  chunks and requires requested, effective, and stable clamp reason to be shown
  together. It explicitly forbids presenting an uncertified higher request as
  supported.
- Lattice ADR 0026 freezes the desktop-reference full-resolution resident cap
  at 1,183 chunks, active cap at 405, and process RAM high-water at 4 GiB.
  Raising those values requires representative optimized performance evidence,
  not a constant edit.
- Minecraft Java 1.18 separated simulation distance from render distance so a
  larger visual radius need not expand authoritative ticking, and changed view
  loading to a cylindrical horizontal footprint. Current Java 26.3 preview
  continues to expose render and simulation as distinct sliders.
- Bevy 0.19.1 `PerspectiveProjection::far` is a world-unit distance and defaults
  to 1,000. `DistanceFog` is a separate presentation value; both must be derived
  from the same effective world-distance contract when streaming changes.

Primary references:

- Lattice ADR 0026 and ADR 0034 at revision
  `D:/rezics-repos/lattice-axiom-demo/.temp/reference/lattice-axiom`;
- [Minecraft Java 1.18 release notes](https://feedback.minecraft.net/hc/en-us/articles/4415128577293-Minecraft-Java-Edition-1-18);
- [Minecraft Java 26.3 Snapshot 9](https://feedback.minecraft.net/hc/en-us/articles/48195693403661-Minecraft-Java-Edition-26-3-Snapshot-9);
- [Bevy 0.19.1 `PerspectiveProjection`](https://docs.rs/bevy/0.19.1/bevy/camera/struct.PerspectiveProjection.html).

## Selected recovery contract

This slice repairs the product truthfulness and unit propagation without
claiming uncertified long-distance rendering:

1. Preserve the package-authored `2..=32` requested-radius slider and the
   persisted request, as required by ADR 0034.
2. Replace ambiguous `View effective/admitted` HUD text with named effective
   radius, its meter conversion, requested radius when different, and the
   stable clamp reason. A request of 26 under the current profile must read as
   effective 6 chunks / 192 m and resident-budget limited.
3. Make the pause settings status use the same formatter and terminology so
   HUD and settings cannot drift.
4. Add a checked chunks-to-meters conversion based on the compiled world's
   chunk edge and expose it through one typed status projection.
5. Synchronize camera far distance and fog visibility from the effective world
   radius with a bounded safety margin, while retaining underwater/lava fog
   overrides. This closes the path for future certified radius increases.
6. Prove immediate requests below the resident cap change effective interest,
   render scope, status text, and presentation distance. Prove requests above
   the cap retain their authored value but visibly report the limit.

## Explicit non-goal and follow-up gate

This repair does not raise the desktop-reference hard cap and does not label 21
or 32 chunks as effective. A real 21-to-32-chunk visual radius requires a
separate outer presentation working set: surface-aware vertical selection or
far-terrain LOD, circular horizontal admission, bounded task scheduling, and
the ten-minute RAM/VRAM/frame/latency evidence required by ADR 0026/0034.

That work must be a separate provider/profile slice because loading
`(2 * 21 + 1)^2 * 7 = 12,943` full-resolution chunks would exceed the accepted
1,183-chunk model by roughly 10.9 times and would be a historical-burden design,
not a current best-practice fix.

## Implementation slices

### Slice A: runtime meaning and UI truthfulness

- introduce the typed effective world-distance projection;
- share one formatter between HUD and pause settings;
- display effective chunks, meters, requested chunks, and clamp reason;
- add unit and headless behavior tests.

Commit: `fix(settings): clarify effective view distance`.

### Slice B: camera synchronization

- derive air fog visibility and perspective far plane from effective meters;
- preserve medium-specific fog behavior;
- add headless projection/fog tests and a fixed-window visual check.

Commit: `fix(render): synchronize camera view range`.

### Follow-up: certified far-terrain presentation

Produce a separate plan and implementation only after measuring the current
full-resolution baseline. Compare at least full chunks, surface-section
streaming, geometry clipmaps, and a Distant-Horizons-style LOD ring. The chosen
path must retain edit correctness near the player, stable seams, deterministic
generation, bounded queues, and the accepted performance profile.

## Acceptance

- No UI surface emits an unlabeled `effective/requested` fraction.
- A 26-chunk request under the current profile visibly reports `6 chunks`,
  `192 m radius`, and `resident budget`.
- A request from six down to four reduces render scope and camera/fog range in
  the same update; restoring six reverses it.
- Chunk-to-meter conversion uses the compiled chunk edge, not a hard-coded 16
  or 32.
- Existing request persistence and the authored `2..=32` catalog contract stay
  byte-compatible.
- Focused settings, streaming, HUD, camera, and headless-host tests pass with
  strict Clippy and rustfmt.

## Implementation evidence

- `EffectiveRenderDistanceMetersV1` derives meters from the compiled chunk edge
  and is carried by `ViewDistanceStatusV1`; no UI code performs its own unit
  conversion.
- HUD and pause settings share one formatter. The prior `View 6/26` label is
  removed; a clamped 26-chunk request reports effective 6 chunks, 192 m radius,
  the retained request, and the resident-memory reason.
- Requests at or below four chunks alter the desired set from 1,183 to 567
  chunks and derive a 128 m effective radius. Stream tests prove both scope
  changes without raising the certified resident cap.
- `ProductionCameraViewRangeV1` derives air fog and the perspective far plane
  from the same effective radius. Six chunks produce 208 m air fog / 448 m far;
  four chunks produce 144 m / 320 m. Underwater and lava overrides remain
  unchanged. The camera system is explicitly ordered after settings apply so
  a runtime change reaches presentation in the same update.
- Thirteen stream tests, the focused HUD test, five client camera tests, and the
  complete 163-test engine library suite pass. Strict worldgen/engine library
  Clippy and rustfmt pass. Fixed-window visual review is the remaining gate.
