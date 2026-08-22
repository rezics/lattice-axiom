# Voltage/recipe graph. Does not own mana.
let latticeaxiom = import latticeaxiom_lib_v3 in
let package = latticeaxiom.package in
let registration = latticeaxiom.registration in
fun kernel_receipts =>
let package_provenance = kernel_receipts.package_provenance in
let registration_fragment_hash = kernel_receipts.registration_fragment_hash in
let metallurgy_capability = "terrenia:capability/metallurgy@1" in
let science_capability = "terrenia:capability/science-processing@1" in
let fragment = registration.constructors.fragment {
  registrations = [{
    id = science_capability,
    kind = "capability",
    declared_by = "@terrenia/science",
    schema = null,
    provenance = package_provenance,
  }],
} in

package.make {
  name = "@terrenia/science",
  version = "0.1.0",
  metadata = {
    display_name = "Terrenia Science",
    documentation = null,
    license = "MIT OR Apache-2.0",
  },
  features = {
    metallurgy = {
      default = true,
      domains = ["authoritative"],
    },
  },
  dependencies = {
    "@terrenia/metallurgy" = {
      version = "~0.1.0",
      optional = true,
      when_features = ["metallurgy"],
      domains = ["authoritative"],
    },
  },
  requires = {
    "terrenia:capability/metallurgy@1" = {
      capability = metallurgy_capability,
      version = "^1.0.0",
      provider = "@terrenia/metallurgy",
      cardinality = "zero-or-more",
      domains = ["authoritative"],
    },
  },
  provides = {
    "terrenia:capability/science-processing@1" = {
      capability = science_capability,
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
    "terrenia:capability/science-processing",
    "terrenia:state/voltage",
  ],
  trust = "data-only",
  registration = fragment,
  provenance = package_provenance,
}
