# Versioned R0 registration authoring contracts and constructors.
let common = import "common.ncl" in
let C = common.contracts in
let manifest_schema_version = common.model_versions.registration_manifest in

let RegistrationKindContract =
  std.string.Matches "^(dimension|block|item|fluid|biome|component|message|system|asset|capability|schema|render)$"
in
let ExactRegistrationContract = {
  id | C.StableId,
  kind | RegistrationKindContract,
  declared_by | C.PackageName,
  schema | C.Nullable C.SchemaId,
  provenance | C.SourceProvenance,
}
in

# Semantic, settings, and observability rows are still validated by their
# normative Rust DTOs. This R0 contract closes the fragment field set and
# validates exact registration rows while preserving those typed JSON rows.
let RegistrationFragmentContract = {
  registrations | Array ExactRegistrationContract | default = [],
  semantics | Array Dyn | default = [],
  settings | Array Dyn | default = [],
  composition_parameters | Array Dyn | default = [],
  info_items | Array Dyn | default = [],
  metrics | Array Dyn | default = [],
  inspect | Array Dyn | default = [],
  visualizers | Array Dyn | default = [],
}
in
let ManifestProducerContract = {
  tool | std.string.NonEmpty,
  version | C.StrictSemVer,
  input_hash | C.CanonicalHash,
}
in
let RegistrationManifestContract = {
  schema_version
    | std.contract.Equal manifest_schema_version
    | default
    = manifest_schema_version,
  package | C.PackageName,
  version | C.StrictSemVer,
  fragment | RegistrationFragmentContract,
  producer | ManifestProducerContract,
  semantic_hash | C.CanonicalHash,
}
in
let make_fragment = fun value => value | RegistrationFragmentContract in
let make_manifest = fun value => value | RegistrationManifestContract in

{
  library_contract_major = common.library_contract_major,
  corpus_major = common.corpus_major,
  contract_major = common.contract_major,
  schema_version = manifest_schema_version,
  contracts = {
    RegistrationKind = RegistrationKindContract,
    ExactRegistration = ExactRegistrationContract,
    RegistrationFragment = RegistrationFragmentContract,
    ManifestProducer = ManifestProducerContract,
    RegistrationManifest = RegistrationManifestContract,
  },
  constructors = {
    fragment = make_fragment,
    manifest = make_manifest,
  },
  empty_fragment = make_fragment {},
}
