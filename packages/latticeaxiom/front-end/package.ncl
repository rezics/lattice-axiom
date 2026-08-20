# D0 pure-data skeleton for the package-driven client shell root.
let latticeaxiom = import latticeaxiom_lib_v3 in
let package = latticeaxiom.package in
let registration = latticeaxiom.registration in
fun kernel_receipts =>
let package_provenance = kernel_receipts.package_provenance in
let registration_fragment_hash = kernel_receipts.registration_fragment_hash in
let provided_capability = "latticeaxiom:capability/client-shell@1" in
let world_catalog_capability = "latticeaxiom:capability/world-catalog@1" in
let settings_surface_capability =
  "latticeaxiom:capability/settings-surface@1"
in
let diagnostics_capability =
  "latticeaxiom:capability/diagnostic-registry@1"
in
let fragment = registration.constructors.fragment {
  registrations = [{
    id = provided_capability,
    kind = "capability",
    declared_by = "@latticeaxiom/front-end",
    schema = null,
    provenance = package_provenance,
  }],
} in

package.make {
  name = "@latticeaxiom/front-end",
  version = "0.1.0",
  metadata = {
    display_name = "Lattice Axiom Client Shell",
    documentation = null,
    license = "MIT OR Apache-2.0",
  },
  dependencies = {
    "@latticeaxiom/world-library" = {
      version = "~0.1.0",
      optional = false,
      domains = ["client"],
    },
    "@latticeaxiom/settings-ui" = {
      version = "~0.1.0",
      optional = false,
      domains = ["client"],
    },
    "@latticeaxiom/observability" = {
      version = "~0.1.0",
      optional = false,
      domains = ["client"],
    },
  },
  requires = {
    "latticeaxiom:capability/world-catalog@1" = {
      capability = world_catalog_capability,
      version = "^1.0.0",
      provider = "@latticeaxiom/world-library",
      cardinality = "exactly-one",
      domains = ["client"],
    },
    "latticeaxiom:capability/settings-surface@1" = {
      capability = settings_surface_capability,
      version = "^1.0.0",
      provider = "@latticeaxiom/settings-ui",
      cardinality = "exactly-one",
      domains = ["client"],
    },
    "latticeaxiom:capability/diagnostic-registry@1" = {
      capability = diagnostics_capability,
      version = "^1.0.0",
      provider = "@latticeaxiom/observability",
      cardinality = "exactly-one",
      domains = ["client"],
    },
  },
  provides = {
    "latticeaxiom:capability/client-shell@1" = {
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
  namespace_requests = [
    "latticeaxiom:capability/client-shell",
    "latticeaxiom:capability/world-catalog",
    "latticeaxiom:capability/settings-surface",
    "latticeaxiom:capability/settings-registry",
    "latticeaxiom:capability/diagnostic-registry",
  ],
  namespace_delegations = {
    "@latticeaxiom/world-library" = ["latticeaxiom:capability/world-catalog"],
    "@latticeaxiom/settings-ui" = [
      "latticeaxiom:capability/settings-surface",
      "latticeaxiom:capability/settings-registry",
    ],
    "@latticeaxiom/observability" = [
      "latticeaxiom:capability/diagnostic-registry",
    ],
  },
  trust = "data-only",
  registration = fragment,
  provenance = package_provenance,
}
