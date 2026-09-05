# D0 empty world-generation provider skeleton; terrain data arrives in D4.
let latticeaxiom = import latticeaxiom_lib_v3 in
let package = latticeaxiom.package in
let registration = latticeaxiom.registration in
fun kernel_receipts =>
let package_provenance = kernel_receipts.package_provenance in
let registration_fragment_hash = kernel_receipts.registration_fragment_hash in
let blocks_capability = "latticeaxiom:capability/content-blocks@1" in
let worldgen_capability =
  "latticeaxiom:capability/worldgen-terrain-provider@2"
in
let fragment = registration.constructors.fragment {} in

package.make {
  name = "@terrenia/worldgen",
  version = "0.1.0",
  metadata = {
    display_name = "Terrenia World Generation",
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
    "latticeaxiom:capability/worldgen-terrain-provider@2" = {
      capability = worldgen_capability,
      version = "2.0.0",
      cardinality = "exactly-one",
      domains = ["authoritative"],
    },
  },
  realizations = {
    "native-static" = {
      id = "native-static", kind = "native-static",
      domains = ["authoritative"],
      artifact = { kind = "source-build" }, trust = "trusted-native",
      engine_build = null, registration_fragment = registration_fragment_hash,
    },
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
  namespace_requests = ["terrenia:worldgen/**", "terrenia:biome/**"],
  trust = "trusted-native",
  registration = fragment,
  provenance = package_provenance,
}
