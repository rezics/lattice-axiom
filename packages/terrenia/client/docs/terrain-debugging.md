# Terrain and frame diagnostics

The client keeps a small native frame monitor visible during play. It reports
FPS, frame milliseconds, the rolling P95 frame interval, and main-schedule CPU
time. The history is bounded to 240 frames and the label refreshes four times
per second. `Background` identifies the normal unfocused-window frame cap.
CPU time excludes the render thread and GPU; it is not a GPU timer.

## Investigation

The package-first import preserved the demo's terrain presets, generic terrain
generation and voxel meshing code. Comparing those implementations did not find
a replacement terrain algorithm. Native overview captures did expose several
existing terrain defects which ordinary spawn-facing screenshots hid:

- Transition bands selected a different biome material independently for each
  column. This scattered grass, sand and rock across the same hillside, broke
  vegetation continuity, and fragmented otherwise mergeable voxel surfaces.
- Signed uphill height differences failed conversion to `u16`, becoming the
  maximum possible descent. A higher neighbor therefore incorrectly removed
  soil and suppressed vegetation as though it were a severe cliff.
- Distant mesh vertex colors consumed resource-pack sRGB values as linear
  values, unlike the correctly decoded near-terrain atlas.

Transition revision 9 displaces a shared boundary with coherent noise. Its sign
uses canonical material-style ordering on both sides of the boundary, including
negative world coordinates. Terrain heights still come from the existing
semantic terrain field; material selection does not invent new height noise.
Geology revision 4 clamps uphill differences to zero descent. Actual downward
cliffs retain thin soil and rock exposure. Both near and distant material
queries use the same policy.

The selected revisions live in the frozen worldgen package's
`data/generation-policy.json`. Missing policy data means legacy geology 3 and
transition 8. Reopening an existing world's archived lock therefore preserves
its generator. Existing worlds are not silently regenerated or migrated.
Create a new world with a newly composed lock to use the corrected policies.

## Rendering work

The renderer compares retained immutable far-tile keys instead of serializing
and hashing every ready tile on every frame. Unchanged visibility ranges,
camera transforms, hidden panels and slot selectors no longer trigger Bevy
change detection. Hidden inventory projection is skipped. Queue and upload
caps, render distance, simulation radius and geometry ownership are unchanged.

## Reproducing an overview

`tools/verify_render.py` copies the runtime and saved-world database into a new
output directory, selects the world's archived lock, and launches the actual
client. `--overview --render-distance 21 --seconds 45` captures an elevated view
over the streamed region. The output contains `scene.png`, `scene.json`,
`client.log`, and an executable/world/lock receipt. Capture timing samples are
bounded and omit the first ten seconds. Frame intervals include pacing; an
unfocused 30 FPS run must not be described as a GPU performance limit.

Use `tools/verify_native_lifecycle.py` separately for actual create/save/reopen
acceptance. Headless tests cannot prove visible terrain quality or GPU speed.
All images, databases and build outputs remain local and ignored by Git.

## Verified correction slice

On 2026-09-06 the optimized development client completed two independent native
create/play/save/reopen sessions with the same fixed QA identity. A 45-second
1280x720 overview at render distance 21 used the same UUID-derived seed as the
old comparison world. The former scattered grass/sand boundary became a
contiguous boundary; the original source database remained byte-identical.
The capture showed about 201 FPS over its post-warmup sample, including streaming
and frame pacing. This is one observed scenario, not a general GPU certification
or a claim that a performance regression was reproduced.

The source-level checks passed 182 engine and 87 worldgen unit tests, including
the soil regression, coherent signed-coordinate boundary and unchanged-layout
checks. All-target Clippy passed for both crates with the client/development
features. The new generation policy is selected by the frozen worldgen data;
old archived locks continue to select their original rules.

Local evidence is under `.temp/qa-terrain-fixed-world/`,
`.temp/qa-terrain-fixed-overview/`, `.temp/terrain-tests-final.log`, and
`.temp/terrain-clippy.log`. Earlier exploratory captures included a wall-facing
spawn and a background-capped run; neither is treated as an equivalent
performance comparison. A capture-only camera override is applied before
medium/fog classification so elevated views do not inherit underwater shading
from the player's eye position.

The longer-term direction and research limits are documented in
[World-generation toolkits](../../../latticeaxiom/world/docs/worldgen-toolkit-research.md).

## Upstream references

- [Bevy 0.19.1 frame diagnostics](https://docs.rs/bevy/0.19.1/bevy/diagnostic/struct.FrameTimeDiagnosticsPlugin.html)
- [Bevy change detection](https://docs.rs/bevy/0.19.1/bevy/prelude/trait.DetectChangesMut.html)
- [Minecraft world generation passes](https://learn.microsoft.com/en-us/minecraft/creator/documents/world-generation?view=minecraft-bedrock-stable)

The generation-pass reference supports keeping terrain shape, biome surface
materials and subsequent features distinct. This implementation reuses the
repository's deterministic integer noise and package/provider contracts; it
does not import another game's generator or add an engine dependency.
