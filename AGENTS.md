# Engineering rules

This is the unified implementation repository. The user has authorized the
package-first rebuild and a Conventional Commit after every verified slice.
Do not ask for repeated commit approval during this rebuild.

- Follow `docs/ecosystem-direction.md` and
  `packages/latticeaxiom/sdk/docs/package-interfaces.md` for ecosystem/interface
  design. Packages may own
  APIs and frameworks consumed by other packages; do not restrict the ecosystem
  to host-defined gameplay concepts or the portable row-kernel subset.
- Preserve first-party-equivalent source extension and independent game
  distribution paths. Separate reusable library imports from world activation,
  and distinguish exact locks from behavioral and saved-state compatibility.
- All project-owned Rust crates live in `packages/<scope>/<package>/crates/`.
  Each crate has exactly one Lattice package owner. Third-party vendored source
  remains under `third_party/` with its existing license.
- Package identities, dependency edges and source entries live in package
  manifests. Cargo handles compilation; workspace membership does not select
  game features. Keep package-level normal/build dependencies acyclic.
- Resource packs are independent, client-only presentation providers. Do not
  introduce gameplay authority or hidden code dependencies through a resource pack.
- Commit only UTF-8 text. Do not commit images, audio, models, fonts, compiled
  artifacts, archives, or base64/byte-array copies of those assets. Editable
  resource references and procedural drawing/shader code are allowed. Dedicated
  art-asset management is deferred by the user.
- Use Bevy's existing App, ECS, scheduler, rendering, UI, assets and task pools.
  Use native Flexbox/Grid, text, colors, borders and gradients for UI. Keep
  keyboard focus visible, controls readable and layout scalable.
- Study current upstream APIs before implementing reusable infrastructure.
  Pin dependencies; keep engine upgrades separate from the rebuild.
- Use typed IDs and stable iteration across persistence/ABI boundaries. Static
  code uses Bevy directly; external boundaries use stable DTOs or C ABI.
- Measure performance-sensitive changes in optimized builds. Keep queues and
  per-frame work bounded. Headless checks cannot prove visual or GPU performance.
- Keep tests and public rustdoc next to code. Run appropriate checks for each
  slice, inspect staged files, and run `python tools/repository_check.py --staged`
  before committing. Never stage unrelated changes or generated local data.
- Keep English source, rustdoc, commit messages and implementation docs.
- Preserve existing saved worlds; use copies for migrations and acceptance.
  The old demo checkout and external recovery bundles are comparison/recovery
  inputs, never production build dependencies.
