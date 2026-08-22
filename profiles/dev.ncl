# D0 client-world profile for Terrenia plus opt-in developer surfaces.
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
  profile = "latticeaxiom:profile/dev@1",
  projection = "client-world",
  projection_domains = ["authoritative", "client"],
  roots = {
    terrenia = {
      version = "=0.1.0",
      realization = auto_data,
    },
    "@latticeaxiom/settings" = {
      version = "=0.1.0",
      realization = auto_data,
    },
    "@latticeaxiom/input" = {
      version = "=0.1.0",
      realization = auto_data,
    },
    "@latticeaxiom/settings-ui" = {
      version = "=0.1.0",
      realization = auto_data,
    },
    "@latticeaxiom/observability" = {
      version = "=0.1.0",
      realization = auto_data,
    },
    "@latticeaxiom/inspect" = {
      version = "=0.1.0",
      realization = auto_data,
    },
    "@latticeaxiom/dev-tools" = {
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
      domains = ["client"],
    },
    "latticeaxiom:capability/input-actions@1" = {
      capability = "latticeaxiom:capability/input-actions@1",
      version = "^1.0.0",
      provider = "@latticeaxiom/input",
      cardinality = "exactly-one",
      domains = ["client"],
    },
    "latticeaxiom:capability/settings-surface@1" = {
      capability = "latticeaxiom:capability/settings-surface@1",
      version = "^1.0.0",
      provider = "@latticeaxiom/settings-ui",
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
    "latticeaxiom:capability/target-inspect-surface@1" = {
      capability = "latticeaxiom:capability/target-inspect-surface@1",
      version = "^1.0.0",
      provider = "@latticeaxiom/inspect",
      cardinality = "exactly-one",
      domains = ["client"],
    },
    "latticeaxiom:capability/debug-workbench@1" = {
      capability = "latticeaxiom:capability/debug-workbench@1",
      version = "^1.0.0",
      provider = "@latticeaxiom/dev-tools",
      cardinality = "exactly-one",
      domains = ["client"],
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
    "latticeaxiom:capability/sandbox-gameplay@1" = {
      capability = "latticeaxiom:capability/sandbox-gameplay@1",
      version = "^1.0.0",
      provider = "@terrenia/gameplay",
      cardinality = "exactly-one",
      domains = ["authoritative"],
    },
    "latticeaxiom:capability/sandbox-tools@1" = {
      capability = "latticeaxiom:capability/sandbox-tools@1",
      version = "^1.0.0",
      provider = "@terrenia/tools",
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
      "latticeaxiom:source/input"
      "@latticeaxiom/input"
      "packages/latticeaxiom/input",
    source_candidate
      "latticeaxiom:source/settings-ui"
      "@latticeaxiom/settings-ui"
      "packages/latticeaxiom/settings-ui",
    source_candidate
      "latticeaxiom:source/observability"
      "@latticeaxiom/observability"
      "packages/latticeaxiom/observability",
    source_candidate
      "latticeaxiom:source/inspect"
      "@latticeaxiom/inspect"
      "packages/latticeaxiom/inspect",
    source_candidate
      "latticeaxiom:source/dev-tools"
      "@latticeaxiom/dev-tools"
      "packages/latticeaxiom/dev-tools",
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
    source_candidate
      "terrenia:source/presentation"
      "@terrenia/presentation"
      "packages/terrenia/presentation",
  ],
  realization_policy = ["data"],
  evaluation_policy = game.r0_evaluation_policy,
  policy = {
    namespace_grants = {
      "@latticeaxiom/settings" = [
        "latticeaxiom:capability/settings-registry",
      ],
      "@latticeaxiom/input" = [
        "latticeaxiom:capability/input-actions",
        "latticeaxiom:action/**",
        "latticeaxiom:capability/settings-registry",
      ],
      "@latticeaxiom/settings-ui" = [
        "latticeaxiom:capability/settings-surface",
        "latticeaxiom:capability/settings-registry",
      ],
      "@latticeaxiom/observability" = [
        "latticeaxiom:capability/diagnostic-registry",
      ],
      "@latticeaxiom/inspect" = [
        "latticeaxiom:capability/target-inspect-surface",
      ],
      "@latticeaxiom/dev-tools" = [
        "latticeaxiom:capability/debug-workbench",
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
    allow_force_override = true,
    allow_recovery = true,
  },
}
