# Versioned R0 package authoring contracts and constructors.
let common = import "common.ncl" in
let registration_module = import "registration.ncl" in
let C = common.contracts in
let package_model_version = common.model_versions.package in

let FeatureSpecContract = {
  default | Bool,
  domains | Array C.PackageDomain,
}
in
let PackageDependencyContract = {
  version | C.VersionReq,
  optional | Bool,
  when_features | Array C.RealizationId | default = [],
  features | Array C.RealizationId | default = [],
  domains | Array C.PackageDomain,
}
in
let PackageMetadataContract = {
  display_name | std.string.NonEmpty,
  documentation | C.Nullable String,
  license | std.string.NonEmpty,
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
let CapabilityProvisionContract = {
  capability | C.CapabilityId,
  version | C.StrictSemVer,
  cardinality | std.string.Matches "^(exactly-one|zero-or-more|one-or-more)$",
  domains | Array C.PackageDomain,
}
in
let InterfaceRequirementContract = {
  version | C.VersionReq,
  optional | Bool,
}
in
let ArtifactIntentContract = {
  kind | std.string.Matches "^(source-build|local-prebuilt|data-root)$",
  path | C.LexicalLogicalPath | optional,
}
in
let RealizationSpecContract = {
  id | C.RealizationId,
  kind | C.RealizationKind,
  domains | Array C.PackageDomain,
  targets | Array C.TargetTriple | default = [],
  interfaces | { _ : InterfaceRequirementContract } | default = {},
  required_features | Array C.RealizationId | default = [],
  artifact | ArtifactIntentContract,
  trust | C.TrustClass,
  engine_build | C.Nullable C.CanonicalHash,
  registration_fragment | C.CanonicalHash,
}
in
let NamespaceRequestContract = {
  namespace | std.string.Matches "^%{common.patterns.realization_id}$",
  patterns | Array String,
}
in
let PackageSpecContract = {
  model_version
    | std.contract.Equal package_model_version
    | default
    = package_model_version,
  name | C.PackageName,
  version | C.StrictSemVer,
  metadata | PackageMetadataContract,
  features
    | { _ : FeatureSpecContract }
    | std.record.FieldsMatch common.patterns.realization_id
    | default
    = {},
  dependencies
    | { _ : PackageDependencyContract }
    | std.record.FieldsMatch common.patterns.package_name
    | default
    = {},
  requires | { _ : CapabilityRequirementContract } | default = {},
  provides | { _ : CapabilityProvisionContract } | default = {},
  realizations
    | { _ : RealizationSpecContract }
    | std.record.FieldsMatch common.patterns.realization_id,
  domains | Array C.PackageDomain,
  parameters | { _ : Dyn } | default = {},
  namespace_requests | { _ : NamespaceRequestContract } | default = {},
  trust | C.TrustClass,
  registration
    | registration_module.contracts.RegistrationFragment
    | default
    = {},
  provenance | C.SourceProvenance,
}
in

let make_package = fun value =>
  let checked = value | PackageSpecContract in
  let feature_domains_are_valid =
    checked.features
    |> std.record.values
    |> std.array.all (fun feature =>
      feature.domains
      |> std.array.all (fun domain => std.array.elem domain checked.domains)
    )
  in
  if std.array.length checked.domains == 0 then
    std.fail_with "a package must declare at least one domain"
  else if std.array.length (std.record.fields checked.realizations) == 0 then
    std.fail_with "a package must publish at least one realization"
  else if !feature_domains_are_valid then
    std.fail_with "feature domains must be a subset of package domains"
  else
    checked
in

{
  library_contract_major = common.library_contract_major,
  corpus_major = common.corpus_major,
  contract_major = common.contract_major,
  model_version = package_model_version,
  contracts = {
    FeatureSpec = FeatureSpecContract,
    PackageDependency = PackageDependencyContract,
    PackageMetadata = PackageMetadataContract,
    CapabilityRequirement = CapabilityRequirementContract,
    CapabilityProvision = CapabilityProvisionContract,
    InterfaceRequirement = InterfaceRequirementContract,
    ArtifactIntent = ArtifactIntentContract,
    RealizationSpec = RealizationSpecContract,
    NamespaceRequest = NamespaceRequestContract,
    PackageSpec = PackageSpecContract,
  },
  constructors = {
    package = make_package,
  },
  make = make_package,
}
