# Trusted tool profile using an explicit non-R0 evaluator policy.
{
  profile = "latticeaxiom:profile/content-audit@1",
  projection = "tool",
  projection_domains = ["tool"],
  roots = {},
  source_universe = [],
  realization_policy = ["data"],
  evaluation_policy = "latticeaxiom:nickel-evaluation-policy/developer@1",
  evaluation_limits = {
    wall_clock_ms = 20000,
  },
  policy = {
    maximum_trust = "data-only",
    allow_force_override = false,
    allow_recovery = false,
  },
}
