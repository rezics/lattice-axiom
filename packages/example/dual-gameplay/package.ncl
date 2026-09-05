# Package-composed D1 conformance data and native source realization.
let latticeaxiom = import latticeaxiom_lib_v3 in
let package = latticeaxiom.package in
let registration = latticeaxiom.registration in
fun kernel_receipts =>
let package_provenance = kernel_receipts.package_provenance in
let registration_fragment_hash = kernel_receipts.registration_fragment_hash in
let fragment = registration.constructors.fragment {
  registrations = [],
} in

package.make {
  name = "@example/dual-gameplay",
  version = "0.1.0",
  metadata = {
    display_name = "Dual Gameplay Conformance Fixture",
    documentation = null,
    license = "AGPL-3.0-only",
  },
  dependencies = {},
  requires = {},
  provides = {},
  realizations = {
    "native-static" = {
      id = "native-static", kind = "native-static",
      domains = ["authoritative"],
      artifact = { kind = "source-build" }, trust = "trusted-native",
      engine_build = null, registration_fragment = registration_fragment_hash,
    },
    data = {
      id = "data",
      kind = "data",
      domains = ["authoritative"],
      targets = [],
      interfaces = {},
      required_features = [],
      artifact = { kind = "data-root", path = "tests/fixtures" },
      trust = "data-only",
      engine_build = null,
      registration_fragment = registration_fragment_hash,
    },
  },
  domains = ["authoritative"],
  parameters = {},
  namespace_requests = [],
  namespace_delegations = {},
  trust = "trusted-native",
  registration = fragment,
  provenance = package_provenance,
}
