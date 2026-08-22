# Copper-first alloy table. Host does not own metal enums.
let latticeaxiom = import latticeaxiom_lib_v3 in
let package = latticeaxiom.package in
let registration = latticeaxiom.registration in
fun kernel_receipts =>
let package_provenance = kernel_receipts.package_provenance in
let registration_fragment_hash = kernel_receipts.registration_fragment_hash in
let metallurgy_capability = "terrenia:capability/metallurgy@1" in
let fragment = registration.constructors.fragment {
  registrations = [{
    id = metallurgy_capability,
    kind = "capability",
    declared_by = "@terrenia/metallurgy",
    schema = null,
    provenance = package_provenance,
  }],
} in

package.make {
  name = "@terrenia/metallurgy",
  version = "0.1.0",
  metadata = {
    display_name = "Terrenia Metallurgy",
    documentation = null,
    license = "MIT OR Apache-2.0",
  },
  provides = {
    "terrenia:capability/metallurgy@1" = {
      capability = metallurgy_capability,
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
    "terrenia:material/copper",
    "terrenia:material/annealed-copper",
    "terrenia:material/bronze",
    "terrenia:material/brass",
  ],
  trust = "data-only",
  registration = fragment,
  provenance = package_provenance,
}
