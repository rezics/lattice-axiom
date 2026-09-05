# latticeaxiom-content

Engine-independent D9 content-state contracts for blocks, fluids, biomes,
explicit material Role bindings, and stable-ID palettes.

The crate accepts authored DTOs only through `ContentCatalogV1::compile`, which
validates configured bounds and references and canonicalizes discovery order.
Compiled types do not deserialize, and no process-local numeric ID crosses the
API. The catalog verification hash excludes presentation bindings.

`serde` input DTOs are representation types, not a streaming decoder. Compile
limits are enforced after decode and before compiler-owned cloning/allocation;
an untrusted byte-stream owner must separately cap input bytes and decode depth.

Deliberate boundaries:

- no Bevy, renderer, gameplay implementation, scheduler, or package evaluator;
- no generic material ontology or guessed semantics from names/assets;
- no public cube/slab/stair/wall form enum—form tokens remain
  definition-scoped pending the accepted consumer gate;
- no snapshot bit packing, chunk traversal, numeric registry ID, or persistence
  format;
- no invented canonical values for the still package-owned Terrenia 72-row
  matrix.

`fixtures/terrenia` contains the accepted 72-ID inventory and a small schema
fixture covering axis, facing/lit, water/lava, both biomes, and explicit Role
resolution. It is test data, not a duplicate authored Terrenia package.
