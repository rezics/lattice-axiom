# D0 client-shell roots and local source universe. No world package is a root.
let latticeaxiom = import latticeaxiom_lib_v3 in
let game = latticeaxiom.game in
# The kernel binds verified whole-root and raw entry-file receipts before typing.
fun source_receipts =>
let source_candidate = fun candidate_source_id candidate_package candidate_path =>
  let receipt = std.record.get candidate_source_id source_receipts in
  {
    source_id = candidate_source_id,
    package = candidate_package,
    version = "0.1.0",
    path = candidate_path,
    content_hash = receipt.source_hash,
    priority = 100,
    provenance = receipt.entry_provenance,
  }
in
let auto_data = { mode = "auto" } in

game.make {
  profile = "latticeaxiom:profile/shell@1",
  projection = "client-shell",
  projection_domains = ["client"],
  roots = {
    "@latticeaxiom/front-end" = {
      version = "=0.1.0",
      realization = auto_data,
    },
    "@latticeaxiom/input" = {
      version = "=0.1.0",
      realization = auto_data,
    },
  },
  capabilities = {
    "latticeaxiom:capability/client-shell@1" = {
      capability = "latticeaxiom:capability/client-shell@1",
      version = "^1.0.0",
      provider = "@latticeaxiom/front-end",
      cardinality = "exactly-one",
      domains = ["client"],
    },
    "latticeaxiom:capability/settings-registry@1" = {
      capability = "latticeaxiom:capability/settings-registry@1",
      version = "^1.0.0",
      provider = "@latticeaxiom/settings",
      cardinality = "exactly-one",
      domains = ["client"],
    },
    "latticeaxiom:capability/input-actions@1" = {
      capability = "latticeaxiom:capability/input-actions@1",
      version = "^1.0.0",
      provider = "@latticeaxiom/input",
      cardinality = "exactly-one",
      domains = ["client"],
    },
    "latticeaxiom:capability/diagnostic-registry@1" = {
      capability = "latticeaxiom:capability/diagnostic-registry@1",
      version = "^1.0.0",
      provider = "@latticeaxiom/observability",
      cardinality = "exactly-one",
      domains = ["client"],
    },
  },
  source_universe = [
    source_candidate
      "latticeaxiom:source/front-end"
      "@latticeaxiom/front-end"
      "packages/latticeaxiom/front-end",
    source_candidate
      "latticeaxiom:source/world-library"
      "@latticeaxiom/world-library"
      "packages/latticeaxiom/world-library",
    source_candidate
      "latticeaxiom:source/settings-ui"
      "@latticeaxiom/settings-ui"
      "packages/latticeaxiom/settings-ui",
    source_candidate
      "latticeaxiom:source/settings"
      "@latticeaxiom/settings"
      "packages/latticeaxiom/settings",
    source_candidate
      "latticeaxiom:source/input"
      "@latticeaxiom/input"
      "packages/latticeaxiom/input",
    source_candidate
      "latticeaxiom:source/observability"
      "@latticeaxiom/observability"
      "packages/latticeaxiom/observability",
  ],
  realization_policy = ["data"],
  evaluation_policy = game.r0_evaluation_policy,
  policy = {
    namespace_grants = {
      "@latticeaxiom/front-end" = [
        "latticeaxiom:capability/client-shell",
        "latticeaxiom:capability/world-catalog",
        "latticeaxiom:capability/settings-surface",
        "latticeaxiom:capability/settings-registry",
        "latticeaxiom:capability/diagnostic-registry",
        "latticeaxiom:capability/input-actions",
      ],
      "@latticeaxiom/input" = [
        "latticeaxiom:capability/input-actions",
        "latticeaxiom:action/**",
        "latticeaxiom:capability/settings-registry",
      ],
    },
    maximum_trust = "trusted-native",
    allow_force_override = false,
    allow_recovery = true,
  },
}
