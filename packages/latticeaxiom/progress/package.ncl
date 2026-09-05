# Chapter/Quest/Task DAG. Cycles fail at compile, not via a global quest file.
let latticeaxiom = import latticeaxiom_lib_v3 in
let package = latticeaxiom.package in
let registration = latticeaxiom.registration in
fun kernel_receipts =>
let package_provenance = kernel_receipts.package_provenance in
let registration_fragment_hash = kernel_receipts.registration_fragment_hash in
let progress_capability = "latticeaxiom:capability/progress-graph@1" in
let fragment = registration.constructors.fragment {
  registrations = [{
    id = progress_capability,
    kind = "capability",
    declared_by = "@latticeaxiom/progress",
    schema = null,
    provenance = package_provenance,
  }],
} in

package.make {
  name = "@latticeaxiom/progress",
  version = "0.1.0",
  metadata = {
    display_name = "Lattice Axiom Progress Graph",
    documentation = null,
    license = "MIT OR Apache-2.0",
  },
  provides = {
    "latticeaxiom:capability/progress-graph@1" = {
      capability = progress_capability,
      version = "1.0.0",
      cardinality = "exactly-one",
      domains = ["authoritative", "client"],
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
  namespace_requests = ["latticeaxiom:capability/progress-graph"],
  trust = "data-only",
  registration = fragment,
  provenance = package_provenance,
}
