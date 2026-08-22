# Directed relation kinds. Companion/maid packages consume this; host has no tlm IDs.
let latticeaxiom = import latticeaxiom_lib_v3 in
let package = latticeaxiom.package in
let registration = latticeaxiom.registration in
fun kernel_receipts =>
let package_provenance = kernel_receipts.package_provenance in
let registration_fragment_hash = kernel_receipts.registration_fragment_hash in
let relations_capability = "latticeaxiom:capability/relations-graph@1" in
let fragment = registration.constructors.fragment {
  registrations = [{
    id = relations_capability,
    kind = "capability",
    declared_by = "@latticeaxiom/relations",
    schema = null,
    provenance = package_provenance,
  }],
} in

package.make {
  name = "@latticeaxiom/relations",
  version = "0.1.0",
  metadata = {
    display_name = "Lattice Axiom Relations Graph",
    documentation = null,
    license = "MIT OR Apache-2.0",
  },
  provides = {
    "latticeaxiom:capability/relations-graph@1" = {
      capability = relations_capability,
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
    "latticeaxiom:capability/relations-graph",
    "latticeaxiom:relation/pet",
    "latticeaxiom:relation/friend",
    "latticeaxiom:relation/subject",
    "latticeaxiom:relation/companion",
  ],
  trust = "data-only",
  registration = fragment,
  provenance = package_provenance,
}
