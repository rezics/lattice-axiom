# Shared R0 authoring contracts for stable boundary primitives.
#
# These contracts target nickel-lang-core 0.18 and emit the string encodings
# consumed by latticeaxiom-core. Rust remains the normative validator.
let contract_major_value = 1 in
let package_model_version = 1 in
let game_profile_model_version = 1 in
let registration_manifest_schema_version = 1 in
let r0_evaluation_policy_id = "latticeaxiom:nickel-evaluation-policy/r0@1" in

let identifier_segment = "[a-z0-9]([a-z0-9._-]*[a-z0-9])?" in
let package_name_pattern = "^(%{identifier_segment}|@%{identifier_segment}/%{identifier_segment})$"
in
let stable_path = "%{identifier_segment}(/%{identifier_segment})*" in
let stable_id_pattern = "^%{identifier_segment}:%{identifier_segment}/%{stable_path}(@[1-9][0-9]*)?$"
in
let evaluation_policy_pattern = "^%{identifier_segment}:nickel-evaluation-policy/%{stable_path}@[1-9][0-9]*$"
in
let source_id_pattern = "^%{identifier_segment}:source/%{stable_path}$"
in
let capability_id_pattern = "^%{identifier_segment}:capability/%{stable_path}@[1-9][0-9]*$"
in
let schema_id_pattern = "^%{identifier_segment}:schema/%{stable_path}(@[1-9][0-9]*)?$"
in
let realization_id_pattern = "^[a-z0-9]([a-z0-9._-]{0,62}[a-z0-9])?$"
in

# This is the SemVer 2.0 grammar, including the leading-zero rule for numeric
# prerelease identifiers. Build metadata is retained as exact identity data.
let numeric_identifier = "(0|[1-9][0-9]*)" in
let non_numeric_identifier = "[0-9]*[A-Za-z-][0-9A-Za-z-]*" in
let prerelease_identifier = "(%{numeric_identifier}|%{non_numeric_identifier})"
in
let semver =
  "%{numeric_identifier}\\.%{numeric_identifier}\\.%{numeric_identifier}"
  ++ "(-%{prerelease_identifier}(\\.%{prerelease_identifier})*)?"
  ++ "(\\+[0-9A-Za-z-]+(\\.[0-9A-Za-z-]+)*)?"
in
let comparator = "(>=|<=|>|<)%{semver}" in
let version_req_pattern = "^(=%{semver}|[~^]%{semver}|%{comparator}([ \\t]+%{comparator})*)$"
in

let NullableContract = fun Contract =>
  std.contract.custom (fun label value =>
    if value == null then
      'Ok value
    else
      std.contract.check Contract label value
  )
in

let PackageNameContract = std.string.Matches package_name_pattern in
let StableIdContract = std.string.Matches stable_id_pattern in
let SourceIdContract = std.string.Matches source_id_pattern in
let CapabilityIdContract = std.string.Matches capability_id_pattern in
let SchemaIdContract = std.string.Matches schema_id_pattern in
let RealizationIdContract = std.string.Matches realization_id_pattern in
let StrictSemVerContract = std.string.Matches "^%{semver}$" in
let VersionReqContract = std.string.Matches version_req_pattern in
let CanonicalHashContract = std.string.Matches "^[0-9a-f]{64}$" in
let LexicalLogicalPathContract =
  std.contract.Sequence [
    String,
    std.contract.from_validator (fun value =>
      let segments = std.string.split "/" value in
      if value == ""
      || std.string.contains "\\" value
      || std.string.is_match "^[A-Za-z]:" value
      || !(
        segments
        |> std.array.all (fun segment =>
          segment != "" && segment != "." && segment != ".."
        )
      ) then
        'Error {
          message = "expected a root-relative canonical logical path",
          notes = [
            "use UTF-8 NFC text, `/` separators, and no empty, `.` or `..` segments",
          ],
        }
      else
        'Ok
    ),
  ]
in
let PackageDomainContract =
  std.string.Matches "^(authoritative|client|server|tool)$"
in
let RealizationKindContract =
  std.string.Matches "^(data|native-static|portable-native|engine-coupled-native)$"
in
let TrustClassContract = std.string.Matches "^(data-only|build|trusted-native)$" in

let SourceSpanContract = {
  start_byte | std.number.Nat,
  end_byte | std.number.Nat,
}
in
let SourceOriginContract = {
  kind | std.string.Matches "^(import|helper|overlay|generated)$",
  identity | std.string.NonEmpty,
}
in
let SourceProvenanceContract = {
  source_id | SourceIdContract,
  logical_path | LexicalLogicalPathContract,
  content_hash | CanonicalHashContract,
  span | NullableContract SourceSpanContract,
  origin_chain | Array SourceOriginContract,
  producer | NullableContract String,
  toolchain | NullableContract String,
  generated_fragment | NullableContract String,
}
in

{
  contract_major = contract_major_value,
  model_versions = {
    package = package_model_version,
    game_profile = game_profile_model_version,
    registration_manifest = registration_manifest_schema_version,
  },
  r0_evaluation_policy = r0_evaluation_policy_id,
  patterns = {
    package_name = package_name_pattern,
    stable_id = stable_id_pattern,
    source_id = source_id_pattern,
    realization_id = realization_id_pattern,
    evaluation_policy = evaluation_policy_pattern,
  },
  contracts = {
    Nullable = NullableContract,
    PackageName = PackageNameContract,
    StableId = StableIdContract,
    SourceId = SourceIdContract,
    CapabilityId = CapabilityIdContract,
    SchemaId = SchemaIdContract,
    RealizationId = RealizationIdContract,
    StrictSemVer = StrictSemVerContract,
    VersionReq = VersionReqContract,
    CanonicalHash = CanonicalHashContract,
    LexicalLogicalPath = LexicalLogicalPathContract,
    PackageDomain = PackageDomainContract,
    RealizationKind = RealizationKindContract,
    TrustClass = TrustClassContract,
    SourceSpan = SourceSpanContract,
    SourceOrigin = SourceOriginContract,
    SourceProvenance = SourceProvenanceContract,
  },
  constructors = {
    source_span = fun value => value | SourceSpanContract,
    source_origin = fun value => value | SourceOriginContract,
    source_provenance = fun value => value | SourceProvenanceContract,
  },
}
