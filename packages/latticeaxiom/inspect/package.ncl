# D0 pure-data skeleton for package-composed target inspection.
let latticeaxiom = import latticeaxiom_lib_v3 in
let package = latticeaxiom.package in
let registration = latticeaxiom.registration in
fun kernel_receipts =>
let package_provenance = kernel_receipts.package_provenance in
let registration_fragment_hash = kernel_receipts.registration_fragment_hash in
let provided_capability =
  "latticeaxiom:capability/target-inspect-surface@1"
in
let diagnostics_capability =
  "latticeaxiom:capability/diagnostic-registry@1"
in
let fragment = registration.constructors.fragment {
  registrations = [{
    id = provided_capability,
    kind = "capability",
    declared_by = "@latticeaxiom/inspect",
    schema = null,
    provenance = package_provenance,
  }],
} in

package.make {
  name = "@latticeaxiom/inspect",
  version = "0.1.0",
  metadata = {
    display_name = "Lattice Axiom Target Inspect Surface",
    documentation = null,
    license = "MIT OR Apache-2.0",
  },
  dependencies = {
    "@latticeaxiom/observability" = {
      version = "~0.1.0",
      optional = false,
      domains = ["client"],
    },
  },
  requires = {
    "latticeaxiom:capability/diagnostic-registry@1" = {
      capability = diagnostics_capability,
      version = "^1.0.0",
      provider = "@latticeaxiom/observability",
      cardinality = "exactly-one",
      domains = ["client"],
    },
  },
  provides = {
    "latticeaxiom:capability/target-inspect-surface@1" = {
      capability = provided_capability,
      version = "1.0.0",
      cardinality = "exactly-one",
      domains = ["client"],
    },
  },
  realizations = {
    data = {
      id = "data",
      kind = "data",
      domains = ["client"],
      artifact = { kind = "data-root", path = "data" },
      trust = "data-only",
      engine_build = null,
      registration_fragment =
        registration_fragment_hash,
    },
  },
  domains = ["client"],
  namespace_requests = ["latticeaxiom:capability/target-inspect-surface"],
  trust = "data-only",
  registration = fragment,
  provenance = package_provenance,
}
