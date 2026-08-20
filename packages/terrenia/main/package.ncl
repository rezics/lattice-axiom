# D0 pure-data skeleton for the replaceable Terrenia dimension root.
let latticeaxiom = import latticeaxiom_lib_v3 in
let package = latticeaxiom.package in
let registration = latticeaxiom.registration in
fun kernel_receipts =>
let package_provenance = kernel_receipts.package_provenance in
let registration_fragment_hash = kernel_receipts.registration_fragment_hash in
let dimension_id = "terrenia:dimension/terrenia" in
let blocks_capability = "latticeaxiom:capability/content-blocks@1" in
let worldgen_capability =
  "latticeaxiom:capability/worldgen-terrain-provider@2"
in
let fragment = registration.constructors.fragment {
  registrations = [{
    id = dimension_id,
    kind = "dimension",
    declared_by = "terrenia",
    schema = null,
    provenance = package_provenance,
  }],
} in

package.make {
  name = "terrenia",
  version = "0.1.0",
  metadata = {
    display_name = "Terrenia Dimension",
    documentation = null,
    license = "MIT OR Apache-2.0",
  },
  dependencies = {
    "@terrenia/blocks" = {
      version = "~0.1.0",
      optional = false,
      domains = ["authoritative"],
    },
    "@terrenia/worldgen" = {
      version = "~0.1.0",
      optional = false,
      domains = ["authoritative"],
    },
    "@terrenia/gameplay" = {
      version = "~0.1.0",
      optional = false,
      domains = ["authoritative"],
    },
    "@terrenia/tools" = {
      version = "~0.1.0",
      optional = false,
      domains = ["authoritative"],
    },
    "@terrenia/presentation" = {
      version = "~0.1.0",
      optional = false,
      domains = ["client"],
    },
  },
  requires = {
    "latticeaxiom:capability/content-blocks@1" = {
      capability = blocks_capability,
      version = "^1.0.0",
      provider = "@terrenia/blocks",
      cardinality = "exactly-one",
      domains = ["authoritative"],
    },
    "latticeaxiom:capability/worldgen-terrain-provider@2" = {
      capability = worldgen_capability,
      version = "^2.0.0",
      provider = "@terrenia/worldgen",
      cardinality = "exactly-one",
      domains = ["authoritative"],
    },
  },
  realizations = {
    data = {
      id = "data",
      kind = "data",
      domains = ["authoritative", "client"],
      artifact = { kind = "data-root", path = "data" },
      trust = "data-only",
      engine_build = null,
      registration_fragment =
        registration_fragment_hash,
    },
  },
  domains = ["authoritative", "client"],
  namespace_requests = [
    "terrenia:dimension/terrenia",
    "terrenia:block/**",
    "terrenia:fluid/**",
    "terrenia:item/**",
    "terrenia:block-tag/**",
    "terrenia:block-role/**",
    "terrenia:worldgen/**",
    "terrenia:biome/**",
    "terrenia:system/**",
    "terrenia:schema/**",
    "terrenia:asset/**",
    "terrenia:render-feature/**",
  ],
  namespace_delegations = {
    "@terrenia/blocks" = [
      "terrenia:block/**",
      "terrenia:fluid/**",
      "terrenia:item/**",
      "terrenia:block-tag/**",
      "terrenia:block-role/**",
    ],
    "@terrenia/worldgen" = [
      "terrenia:worldgen/**",
      "terrenia:biome/**",
    ],
    "@terrenia/gameplay" = [
      "terrenia:system/**",
      "terrenia:schema/**",
    ],
    "@terrenia/tools" = [
      "terrenia:item/stick",
      "terrenia:item/stone-axe",
      "terrenia:item/stone-pickaxe",
      "terrenia:item/stone-shovel",
      "terrenia:item/wooden-axe",
      "terrenia:item/wooden-pickaxe",
      "terrenia:item/wooden-shovel",
    ],
    "@terrenia/presentation" = [
      "terrenia:asset/**",
      "terrenia:render-feature/**",
    ],
  },
  trust = "data-only",
  registration = fragment,
  provenance = package_provenance,
}
