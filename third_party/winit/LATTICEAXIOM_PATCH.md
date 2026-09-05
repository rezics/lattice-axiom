# Lattice Axiom winit patch

- Upstream: <https://github.com/rust-windowing/winit>
- Release: `v0.30.13`
- Crate checksum source: Cargo registry package locked by this repository
- Local change: the Windows backend synchronously clears `ClipCursor` and
  restores cursor visibility before emitting `WindowEvent::Focused(false)`.

Microsoft documents the cursor as a shared resource and requires applications
to release a clip before relinquishing control to another application. The
upstream `v0.30.13` Windows backend applies cursor clip changes only while its
window is focused, so changing the requested grab mode after focus loss cannot
release the native clip.

The patch is intentionally limited to the Win32 focus-loss boundary. Remove the
`[patch.crates-io]` entry once a compatible upstream winit release provides the
same invariant and Bevy's pinned dependency has been validated against it.
