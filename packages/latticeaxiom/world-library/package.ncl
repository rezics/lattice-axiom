# Catalog states, preflight, and recovery operations for the world library.
let latticeaxiom = import latticeaxiom_lib_v3 in
let package = latticeaxiom.package in
let registration = latticeaxiom.registration in
fun kernel_receipts =>
let package_provenance = kernel_receipts.package_provenance in
let registration_fragment_hash = kernel_receipts.registration_fragment_hash in
let provided_capability = "latticeaxiom:capability/world-catalog@1" in
let diagnostics_capability =
  "latticeaxiom:capability/diagnostic-registry@1"
in
let fragment = registration.constructors.fragment {
  registrations = [{
    id = provided_capability,
    kind = "capability",
    declared_by = "@latticeaxiom/world-library",
    schema = null,
    provenance = package_provenance,
  }],
} in

package.make {
  name = "@latticeaxiom/world-library",
  version = "0.1.0",
  metadata = {
    display_name = "Lattice Axiom World Library",
    documentation = null,
    license = "MIT OR Apache-2.0",
  },
  dependencies = {
    "@latticeaxiom/observability" = {
      version = "~0.1.0",
      optional = false,
      domains = ["client", "tool"],
    },
  },
  requires = {
    "latticeaxiom:capability/diagnostic-registry@1" = {
      capability = diagnostics_capability,
      version = "^1.0.0",
      provider = "@latticeaxiom/observability",
      cardinality = "exactly-one",
      domains = ["client", "tool"],
    },
  },
  provides = {
    "latticeaxiom:capability/world-catalog@1" = {
      capability = provided_capability,
      version = "1.0.0",
      cardinality = "exactly-one",
      domains = ["client", "tool"],
    },
  },
  realizations = {
    data = {
      id = "data",
      kind = "data",
      domains = ["client", "tool"],
      artifact = { kind = "data-root", path = "data" },
      trust = "data-only",
      engine_build = null,
      registration_fragment =
        registration_fragment_hash,
    },
  },
  domains = ["client", "tool"],
  namespace_requests = ["latticeaxiom:capability/world-catalog"],
  trust = "data-only",
  registration = fragment,
  provenance = package_provenance,
}
