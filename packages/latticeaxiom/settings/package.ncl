# D0 pure-data skeleton for the platform settings registry.
let latticeaxiom = import latticeaxiom_lib_v3 in
let package = latticeaxiom.package in
let registration = latticeaxiom.registration in
fun kernel_receipts =>
let package_provenance = kernel_receipts.package_provenance in
let registration_fragment_hash = kernel_receipts.registration_fragment_hash in
let settings_capability = "latticeaxiom:capability/settings-registry@1" in
let fragment = registration.constructors.fragment {
  registrations = [{
    id = settings_capability,
    kind = "capability",
    declared_by = "@latticeaxiom/settings",
    schema = null,
    provenance = package_provenance,
  }],
} in

package.make {
  name = "@latticeaxiom/settings",
  version = "0.1.0",
  metadata = {
    display_name = "Lattice Axiom Settings Registry",
    documentation = null,
    license = "MIT OR Apache-2.0",
  },
  provides = {
    "latticeaxiom:capability/settings-registry@1" = {
      capability = settings_capability,
      version = "1.0.0",
      cardinality = "exactly-one",
      domains = ["client", "server", "tool"],
    },
  },
  realizations = {
    data = {
      id = "data",
      kind = "data",
      domains = ["client", "server", "tool"],
      artifact = { kind = "data-root", path = "data" },
      trust = "data-only",
      engine_build = null,
      registration_fragment =
        registration_fragment_hash,
    },
  },
  domains = ["client", "server", "tool"],
  namespace_requests = ["latticeaxiom:capability/settings-registry"],
  trust = "data-only",
  registration = fragment,
  provenance = package_provenance,
}
