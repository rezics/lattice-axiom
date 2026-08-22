# Terrenia chapter pages. Empty until D10+ journey content is authored.
let latticeaxiom = import latticeaxiom_lib_v3 in
let package = latticeaxiom.package in
let registration = latticeaxiom.registration in
fun kernel_receipts =>
let package_provenance = kernel_receipts.package_provenance in
let registration_fragment_hash = kernel_receipts.registration_fragment_hash in
let progress_capability = "latticeaxiom:capability/progress-graph@1" in
let fragment = registration.constructors.fragment {} in

package.make {
  name = "@terrenia/journey",
  version = "0.1.0",
  metadata = {
    display_name = "Terrenia Journey",
    documentation = null,
    license = "MIT OR Apache-2.0",
  },
  dependencies = {
    "@latticeaxiom/progress" = {
      version = "~0.1.0",
      optional = false,
      domains = ["authoritative"],
    },
  },
  requires = {
    "latticeaxiom:capability/progress-graph@1" = {
      capability = progress_capability,
      version = "^1.0.0",
      provider = "@latticeaxiom/progress",
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
  namespace_requests = ["terrenia:quest"],
  trust = "data-only",
  registration = fragment,
  provenance = package_provenance,
}
