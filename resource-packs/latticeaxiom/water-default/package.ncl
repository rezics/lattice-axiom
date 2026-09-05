let latticeaxiom = import latticeaxiom_lib_v3 in
fun receipts => latticeaxiom.package.make {
  name = "@latticeaxiom/water-default", version = "0.1.0",
  metadata = { display_name = "water-default", documentation = null, license = "AGPL-3.0-only" },
  domains = ["client"], trust = "data-only", namespace_requests = [],
  realizations = { data = { id = "data", kind = "data", domains = ["client"],
    artifact = { kind = "data-root", path = "data" }, trust = "data-only", engine_build = null,
    registration_fragment = receipts.registration_fragment_hash,
  } },
  registration = latticeaxiom.registration.constructors.fragment {},
  provenance = receipts.package_provenance,
}
