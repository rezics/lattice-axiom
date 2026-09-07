let latticeaxiom = import latticeaxiom_lib_v3 in
fun receipts => latticeaxiom.package.make {
  name = "@latticeaxiom/web-ui", version = "0.1.0",
  metadata = { display_name = "Lattice Axiom Web UI", documentation = null, license = "AGPL-3.0-only" },
  domains = ["client"], trust = "trusted-native",
  realizations = { "native-static" = {
    id = "native-static", kind = "native-static", domains = ["client"],
    artifact = { kind = "source-build" }, trust = "trusted-native",
    engine_build = null, registration_fragment = receipts.registration_fragment_hash,
  } },
  namespace_requests = [],
  registration = latticeaxiom.registration.constructors.fragment {},
  provenance = receipts.package_provenance,
}
