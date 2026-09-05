# D0 empty content-catalog skeleton; block definitions arrive in D3.
let latticeaxiom = import latticeaxiom_lib_v3 in
let package = latticeaxiom.package in
let registration = latticeaxiom.registration in
fun kernel_receipts =>
let package_provenance = kernel_receipts.package_provenance in
let registration_fragment_hash = kernel_receipts.registration_fragment_hash in
let blocks_capability = "latticeaxiom:capability/content-blocks@1" in
let fragment = registration.constructors.fragment {} in

package.make {
  name = "@terrenia/blocks",
  version = "0.1.0",
  metadata = {
    display_name = "Terrenia Block Catalog",
    documentation = null,
    license = "MIT OR Apache-2.0",
  },
  provides = {
    "latticeaxiom:capability/content-blocks@1" = {
      capability = blocks_capability,
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
        "terrenia:block/**",
        "terrenia:fluid/**",
        "terrenia:item/**",
        "terrenia:block-tag/**",
        "terrenia:block-role/**",
      ],
  trust = "data-only",
  registration = fragment,
  provenance = package_provenance,
}
