# Versioned R0 game-profile authoring contracts and constructors.
let common = import "common.ncl" in
let C = common.contracts in
let profile_model_version = common.model_versions.game_profile in

let RealizationPreferenceContract = {
  mode | std.string.Matches "^(auto|exact)$",
  kind | C.RealizationKind | optional,
}
in
let PackageRequestContract = {
  version | C.VersionReq,
  features | Array C.RealizationId | default = [],
  realization | RealizationPreferenceContract,
}
in
let CapabilityRequirementContract = {
  capability | C.CapabilityId,
  version | C.VersionReq,
  provider | C.Nullable C.PackageName,
  cardinality | std.string.Matches "^(exactly-one|zero-or-more|one-or-more)$",
  domains | Array C.PackageDomain,
}
in
let SourceCandidateContract = {
  source_id | C.SourceId,
  package | C.PackageName,
  version | C.StrictSemVer,
  path | C.LexicalLogicalPath,
  content_hash | C.CanonicalHash,
  priority | std.number.Integer,
  provenance | C.SourceProvenance,
}
in
let EvaluationLimitsContract = {
  wall_clock_ms | std.number.PosNat | default = 10000,
  memory_bytes | std.number.PosNat | default = 268435456,
  recursion_depth | std.number.PosNat | default = 512,
  import_depth | std.number.PosNat | default = 64,
  imported_files | std.number.PosNat | default = 4096,
  source_bytes | std.number.PosNat | default = 16777216,
  output_bytes | std.number.PosNat | default = 16777216,
  retained_diagnostics | std.number.PosNat | default = 256,
}
in
let r0_evaluation_limits = {} | EvaluationLimitsContract in
let ProfilePolicyContract = {
  namespace_grants | { _ : Array String } | default = {},
  maximum_trust | C.TrustClass,
  allow_force_override | Bool,
  allow_recovery | Bool,
}
in
let OverlaySpecContract = {
  semantic_hash | C.CanonicalHash,
  provenance | C.SourceProvenance,
}
in
let GameProfileContract = {
  model_version
    | std.contract.Equal profile_model_version
    | default
    = profile_model_version,
  profile | C.StableId,
  projection
    | std.string.Matches "^(client-shell|client-world|dedicated-server|headless-test|tool)$",
  projection_domains | Array C.PackageDomain,
  roots
    | { _ : PackageRequestContract }
    | std.record.FieldsMatch common.patterns.package_name,
  capabilities | { _ : CapabilityRequirementContract } | default = {},
  source_universe | Array SourceCandidateContract,
  features | { _ : Array C.RealizationId } | default = {},
  parameters | { _ : Dyn } | default = {},
  realization_policy | Array C.RealizationKind,
  semantic_bindings | { _ : C.StableId } | default = {},
  overlays | Array OverlaySpecContract | default = [],
  evaluation_policy | C.StableId,
  evaluation_limits | EvaluationLimitsContract | default = {},
  policy | ProfilePolicyContract,
}
in

let same_domain_set = fun left right =>
  std.array.length left == std.array.length right
  && (left |> std.array.all (fun domain => std.array.elem domain right))
in
let projection_is_valid = fun profile =>
  if profile.projection == "client-shell" then
    same_domain_set profile.projection_domains ["client"]
  else if profile.projection == "client-world" then
    same_domain_set profile.projection_domains ["authoritative", "client"]
  else if profile.projection == "dedicated-server" then
    same_domain_set profile.projection_domains ["authoritative", "server"]
  else if profile.projection == "tool" then
    same_domain_set profile.projection_domains ["tool"]
  else
    std.array.length profile.projection_domains > 0
in
let make_profile = fun value =>
  let checked = value | GameProfileContract in
  let source_provenance_matches =
    checked.source_universe
    |> std.array.all (fun source =>
      source.source_id == source.provenance.source_id
    )
  in
  if std.array.length checked.realization_policy == 0 then
    std.fail_with "a game profile needs a non-empty realization policy"
  else if !projection_is_valid checked then
    std.fail_with "projection domains do not match the selected profile projection"
  else if checked.evaluation_policy != common.r0_evaluation_policy
  && (
    checked.projection != "tool"
    || !(
      std.string.is_match
        common.patterns.evaluation_policy
        checked.evaluation_policy
    )
  ) then
    std.fail_with "unsupported R0 Nickel evaluation policy"
  else if checked.evaluation_policy == common.r0_evaluation_policy
  && checked.evaluation_limits != r0_evaluation_limits then
    std.fail_with "the R0 Nickel policy requires its frozen evaluation limits"
  else if !source_provenance_matches then
    std.fail_with "source candidate and provenance source IDs must match"
  else
    checked
in

{
  contract_major = common.contract_major,
  model_version = profile_model_version,
  r0_evaluation_policy = common.r0_evaluation_policy,
  default_evaluation_limits = r0_evaluation_limits,
  contracts = {
    RealizationPreference = RealizationPreferenceContract,
    PackageRequest = PackageRequestContract,
    CapabilityRequirement = CapabilityRequirementContract,
    SourceCandidate = SourceCandidateContract,
    EvaluationLimits = EvaluationLimitsContract,
    ProfilePolicy = ProfilePolicyContract,
    OverlaySpec = OverlaySpecContract,
    GameProfile = GameProfileContract,
  },
  constructors = {
    game_profile = make_profile,
  },
  make = make_profile,
}
