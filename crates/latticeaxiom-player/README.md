# latticeaxiom-player

`latticeaxiom-player` owns the D2 player action, fixed-cycle capsule controller,
and authoritative block-edit request boundary. The host installs
`PhysicsPlugins::default()`; this crate schedules gameplay input and movement
in `FixedUpdate`, evaluates edits after Avian's `PhysicsSystems::Last` barrier
in `FixedPostUpdate`, and advances the stable tick in `FixedLast`.

Headless callers inject `PlayerActionFrameV1` directly. Platform input is an
optional static adapter and does not alter the stable action, save, dynamic
module, or authoritative-world contracts.

## Dependency adoption record

- Bevy is workspace-locked to exactly `0.19.1`.
  The base crate requests only `std`, `bevy_log`, and `multi_threaded`.
  `client-input` additionally activates Bevy's `keyboard` and `mouse` gates;
  Leafwing activates its `gamepad`, `keyboard`, and `mouse` adapters. Neither
  graph contains Bevy render, asset, window, Winit, PBR, UI, audio, debug, or
  dynamic-linking features.
- Avian is workspace-locked to exactly `0.7.0`, with only `3d`, `f32`,
  `default-collider`, `parallel`, and `parry-f32`. Its rendering, scene,
  picking, and debug-plugin features are deliberately absent from this crate's
  headless composition.
- Leafwing Input Manager is workspace-locked to exactly `0.21.0` and is
  optional behind `client-input`. The adapter enables only `gamepad`,
  `keyboard`, and `mouse`; its default features are disabled. It was selected
  for typed device mapping, remapping, dead-zone processing, input-focus
  handling, and deterministic input mocking. Leafwing types remain inside the
  adapter and never cross a Lattice boundary.
- The reviewed Leafwing feature closure contains `dyn-eq` exactly `0.1.3`
  under MPL-2.0. ADR 0031 records the narrow license exception and checksum
  `5c2d40fdd16847043da3d74883eb0710920645c45fb95093f9975304761d5388`;
  generated notices provide attribution. This crate does not copy that
  license or widen the allowlist.

If Leafwing fails the D2 performance, maintenance, compatibility, or
supply-chain gates, remove the optional adapter and sample Bevy's built-in
`ButtonInput` resources into `PlayerActionFrameV1`. That fallback preserves
headless command injection, fixed-cycle behavior, saves, and all authoritative
interfaces.

## Automated acceptance

The headless integration suite uses Bevy `MinimalPlugins` plus Avian without a
window or GPU. It checks command batching invariance, capsule walking, 1.3×
forward sprint, double-jump creative flight with Space/Shift vertical and
sprint-boosted air speed, jump apex, command-input coyote and buffer
boundaries, the inclusive 45°/rejected 46° slope boundary, the inclusive
0.60 m/rejected 0.61 m step boundary, exact static-collider seams, 10 cm
ground snap with a 1 cm controller skin, the shared 200 ms break/place
limiter, the 5 m authoritative request interface, and the rule that detached
spectators neither mutate the player body nor submit authoritative edits. No
visual or manual acceptance is required.

## Deferred D2 hardening

The following review items are outside this crate's current D2 acceptance
evidence and remain next-stage work:

- The bounded support scan fails closed after 64 geometric hits; sensors count
  toward that ceiling. A future collision-layer policy should exclude sensors
  before traversal, then add overlapping-sensor support and step tests.
- Add Avian journeys for simultaneous floor/wall support, sustained downhill
  snap, falling onto a 46° slope, and an oblique ceiling.
- Extend spectator evidence to a complete authoritative state/hash projection.
- Test authority voxel DDA at exactly 5 m and beyond 5 m; this crate currently
  verifies only the stable request and reach interface.
- Expose a validated custom movement-profile constructor or remove the current
  non-constructible customization path, and validate upright, unit-scale,
  finite spawn transforms.
- Add end-to-end keyboard/mouse versus standard-gamepad fixed-command and
  state-hash parity evidence.
