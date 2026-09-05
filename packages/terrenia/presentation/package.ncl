# Client-only Terrenia presentation: package-authored materials, icons, HUD
# display, and deterministic fallbacks. Headless graphs omit this package.
let latticeaxiom = import latticeaxiom_lib_v3 in
let package = latticeaxiom.package in
let registration = latticeaxiom.registration in
fun kernel_receipts =>
let package_provenance = kernel_receipts.package_provenance in
let registration_fragment_hash = kernel_receipts.registration_fragment_hash in
let presentation_capability =
  "latticeaxiom:capability/content-presentation@1" in
let fragment = registration.constructors.fragment {} in

package.make {
  name = "@terrenia/presentation",
  version = "0.1.0",
  metadata = {
    display_name = "Terrenia Presentation",
    documentation = null,
    license = "MIT OR Apache-2.0",
  },
  provides = {
    "latticeaxiom:capability/content-presentation@1" = {
      capability = presentation_capability,
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
  namespace_requests = ["terrenia:asset/**", "terrenia:render-feature/**"],
  trust = "data-only",
  registration = fragment,
  provenance = package_provenance,
}
