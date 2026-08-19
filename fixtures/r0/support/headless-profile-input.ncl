# Minimal deterministic headless profile before constructor defaults.
let source_provenance = {
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
  profile = "latticeaxiom:profile/core-empty-headless@1",
  projection = "headless-test",
  projection_domains = ["authoritative"],
  roots = {
    core = {
      version = "=0.1.0",
      features = ["headless"],
      realization = {
        mode = "exact",
        kind = "data",
      },
    },
  },
  source_universe = [
    {
      source_id = "latticeaxiom:source/r0-fixtures",
      package = "core",
      version = "0.1.0",
      path = "packages/core",
      content_hash = std.hash 'Sha256 "core-empty-package",
      priority = 0,
      provenance = source_provenance,
    },
  ],
  features = {
    core = ["headless"],
  },
  realization_policy = ["data"],
  evaluation_policy = "latticeaxiom:nickel-evaluation-policy/r0@1",
  policy = {
    namespace_grants = {
      core = ["block/**"],
    },
    maximum_trust = "build",
    allow_force_override = true,
    allow_recovery = true,
  },
}
