let latticeaxiom = import latticeaxiom_lib_v3 in
fun receipts => latticeaxiom.package.make {
  name = "@latticeaxiom/host", version = "0.1.0",
  metadata = { display_name = "@latticeaxiom/host", documentation = null, license = "AGPL-3.0-only" },
  domains = ["authoritative", "client", "server", "tool"],
  trust = "trusted-native",
  realizations = { "native-static" = {
    id = "native-static", kind = "native-static",
    domains = ["authoritative", "client", "server", "tool"],
    artifact = { kind = "source-build" }, trust = "trusted-native",
    engine_build = null, registration_fragment = receipts.registration_fragment_hash,
  } },
  namespace_requests = [],
  registration = latticeaxiom.registration.constructors.fragment {},
  provenance = receipts.package_provenance,
}
