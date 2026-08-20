# Terrenia authored content data

This directory is the owner-managed source of truth for the staged Terrenia
catalog frozen by the active roadmap:

- D3: 6 blocks;
- D4: 18 cumulative blocks;
- D7: 40 cumulative blocks;
- D9: 72 cumulative blocks plus water and lava.

`authored-catalog-v1.json` embeds each intrinsic block and fluid definition in
the exact DTO shape accepted by `latticeaxiom-content`. The adjacent physical,
semantic, gameplay, worldgen, and presentation references remain package-owned
data instead of expanding the platform schema or inferring semantics from ID
suffixes. Finite axis, facing, lit, growth, half, form, fluid-level, and flow
values are state rows and never additional content IDs.

The related closure is split by owner:

- `semantic-registry-v1.json` closes block-owned physical and fluid policies;
- `../../gameplay/data/authored-rules-v1.json` closes mining, drops, tools,
  items, and recipes;
- `../../worldgen/data/authored-block-bindings-v1.json` closes Roles,
  Predicates, and provenance;
- `../../presentation/data/authored-assets-v1.json` closes explicit
  placeholder presentation assets.

## Compose bridge status

The committed `package.ncl` already declares `data` as this package's
data-root artifact, but its registration fragment is still the active D0 empty
surface. These authored files deliberately do not edit that compose source.
A follow-up compose change must teach the registration surface to ingest the
compiler-shaped rows and adjacent owner data without copying the catalog into
Nickel. Until that bridge lands, these files are authoritative package data and
contract-tested fixtures, not a claim that runtime composition publishes them.
