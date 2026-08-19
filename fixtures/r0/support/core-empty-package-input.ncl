# Minimal package authoring input before the versioned constructor applies
# defaults and validates cross-field invariants.
let fixture_provenance = {
  source_id = "latticeaxiom:source/r0-fixtures",
  logical_path = "fixtures/r0/support/core-empty-package-input.ncl",
  content_hash = std.hash 'Sha256 "core-empty-package",
  span = null,
  origin_chain = [],
  producer = null,
  toolchain = null,
  generated_fragment = null,
}
in
{
  name = "core",
  version = "0.1.0",
  metadata = {
    display_name = "Core empty package",
    documentation = null,
    license = "MIT OR Apache-2.0",
  },
  realizations = {
    data = {
      id = "data",
      kind = "data",
      domains = ["authoritative"],
      targets = [],
      interfaces = {},
      required_features = [],
      artifact = {
        kind = "data-root",
        path = "data",
      },
      trust = "data-only",
      engine_build = null,
      registration_fragment =
        std.hash 'Sha256 "core-empty-package-registration",
    },
  },
  domains = ["authoritative"],
  trust = "data-only",
  provenance = fixture_provenance,
}
