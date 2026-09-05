# Data-only wooden and stone tools. Mining runtime is V7, not this package.
# The hand is an intrinsic no-item fallback and is not registered as an item.
let latticeaxiom = import latticeaxiom_lib_v3 in
let package = latticeaxiom.package in
let registration = latticeaxiom.registration in
fun kernel_receipts =>
let package_provenance = kernel_receipts.package_provenance in
let registration_fragment_hash = kernel_receipts.registration_fragment_hash in
let blocks_capability = "latticeaxiom:capability/content-blocks@1" in
let tools_capability = "latticeaxiom:capability/sandbox-tools@1" in
let fragment = registration.constructors.fragment {} in

package.make {
  name = "@terrenia/tools",
  version = "0.1.0",
  metadata = {
    display_name = "Terrenia Tools",
    documentation = null,
    license = "MIT OR Apache-2.0",
  },
  dependencies = {
    "@terrenia/blocks" = {
      version = "~0.1.0",
      optional = false,
      domains = ["authoritative"],
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
  },
  provides = {
    "latticeaxiom:capability/sandbox-tools@1" = {
      capability = tools_capability,
      version = "1.0.0",
      cardinality = "exactly-one",
      domains = ["authoritative"],
    },
  },
  realizations = {
    data = {
      id = "data",
      kind = "data",
      domains = ["authoritative"],
      artifact = { kind = "data-root", path = "data" },
      trust = "data-only",
      engine_build = null,
      registration_fragment =
        registration_fragment_hash,
    },
  },
  domains = ["authoritative"],
  namespace_requests = [
    "terrenia:item/stick",
    "terrenia:item/stone-axe",
    "terrenia:item/stone-pickaxe",
    "terrenia:item/stone-shovel",
    "terrenia:item/wooden-axe",
    "terrenia:item/wooden-pickaxe",
    "terrenia:item/wooden-shovel",
  ],
  trust = "data-only",
  registration = fragment,
  provenance = package_provenance,
}
