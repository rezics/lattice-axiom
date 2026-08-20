# D0 dedicated-server projection of the Terrenia authoritative closure.
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
  profile = "latticeaxiom:profile/headless@1",
  projection = "dedicated-server",
  projection_domains = ["authoritative", "server"],
  roots = {
    terrenia = {
      version = "=0.1.0",
      realization = auto_data,
    },
    "@latticeaxiom/settings" = {
      version = "=0.1.0",
      realization = auto_data,
    },
    "@latticeaxiom/observability" = {
      version = "=0.1.0",
      realization = auto_data,
    },
  },
  capabilities = {
    "latticeaxiom:capability/settings-registry@1" = {
      capability = "latticeaxiom:capability/settings-registry@1",
      version = "^1.0.0",
      provider = "@latticeaxiom/settings",
      cardinality = "exactly-one",
      domains = ["server"],
    },
    "latticeaxiom:capability/diagnostic-registry@1" = {
      capability = "latticeaxiom:capability/diagnostic-registry@1",
      version = "^1.0.0",
      provider = "@latticeaxiom/observability",
      cardinality = "exactly-one",
      domains = ["server"],
    },
    "latticeaxiom:capability/content-blocks@1" = {
      capability = "latticeaxiom:capability/content-blocks@1",
      version = "^1.0.0",
      provider = "@terrenia/blocks",
      cardinality = "exactly-one",
      domains = ["authoritative"],
    },
    "latticeaxiom:capability/worldgen-terrain-provider@2" = {
      capability = "latticeaxiom:capability/worldgen-terrain-provider@2",
      version = "^2.0.0",
      provider = "@terrenia/worldgen",
      cardinality = "exactly-one",
      domains = ["authoritative"],
    },
  },
  source_universe = [
    source_candidate
      "latticeaxiom:source/settings"
      "@latticeaxiom/settings"
      "packages/latticeaxiom/settings",
    source_candidate
      "latticeaxiom:source/observability"
      "@latticeaxiom/observability"
      "packages/latticeaxiom/observability",
    source_candidate "terrenia:source/main" "terrenia" "packages/terrenia/main",
    source_candidate
      "terrenia:source/blocks"
      "@terrenia/blocks"
      "packages/terrenia/blocks",
    source_candidate
      "terrenia:source/worldgen"
      "@terrenia/worldgen"
      "packages/terrenia/worldgen",
    source_candidate
      "terrenia:source/gameplay"
      "@terrenia/gameplay"
      "packages/terrenia/gameplay",
    source_candidate
      "terrenia:source/tools"
      "@terrenia/tools"
      "packages/terrenia/tools",
  ],
  realization_policy = ["data"],
  evaluation_policy = game.r0_evaluation_policy,
  policy = {
    namespace_grants = {
      "@latticeaxiom/settings" = [
        "latticeaxiom:capability/settings-registry",
      ],
      "@latticeaxiom/observability" = [
        "latticeaxiom:capability/diagnostic-registry",
      ],
      terrenia = [
        "terrenia:dimension/terrenia",
        "terrenia:block/**",
        "terrenia:fluid/**",
        "terrenia:item/**",
        "terrenia:block-tag/**",
        "terrenia:block-role/**",
        "terrenia:worldgen/**",
        "terrenia:biome/**",
        "terrenia:system/**",
        "terrenia:schema/**",
        "terrenia:asset/**",
        "terrenia:render-feature/**",
      ],
    },
    maximum_trust = "data-only",
    allow_force_override = false,
    allow_recovery = false,
  },
}
