# Graph-selected action catalog and default bindings.
let latticeaxiom = import latticeaxiom_lib_v3 in
let package = latticeaxiom.package in
let registration = latticeaxiom.registration in
fun kernel_receipts =>
let package_provenance = kernel_receipts.package_provenance in
let registration_fragment_hash = kernel_receipts.registration_fragment_hash in
let provided_capability = "latticeaxiom:capability/input-actions@1" in
let settings_capability = "latticeaxiom:capability/settings-registry@1" in
let fragment = registration.constructors.fragment {
  registrations = [{
    id = provided_capability,
    kind = "capability",
    declared_by = "@latticeaxiom/input",
    schema = null,
    provenance = package_provenance,
  }],
} in

package.make {
  name = "@latticeaxiom/input",
  version = "0.1.0",
  metadata = {
    display_name = "Lattice Axiom Input Actions",
    documentation = null,
    license = "MIT OR Apache-2.0",
  },
  dependencies = {
    "@latticeaxiom/settings" = {
      version = "~0.1.0",
      optional = false,
      domains = ["client", "server", "tool"],
    },
  },
  requires = {
    "latticeaxiom:capability/settings-registry@1" = {
      capability = settings_capability,
      version = "^1.0.0",
      provider = "@latticeaxiom/settings",
      cardinality = "exactly-one",
      domains = ["client", "server", "tool"],
    },
  },
  provides = {
    "latticeaxiom:capability/input-actions@1" = {
      capability = provided_capability,
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
  namespace_requests = [
    "latticeaxiom:capability/input-actions",
    "latticeaxiom:action/**",
    "latticeaxiom:capability/settings-registry",
  ],
  namespace_delegations = {
    "@latticeaxiom/settings" = [
      "latticeaxiom:capability/settings-registry",
    ],
  },
  trust = "data-only",
  registration = fragment,
  provenance = package_provenance,
}
