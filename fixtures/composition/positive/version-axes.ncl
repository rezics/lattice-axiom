let common = import "../../../nickel/latticeaxiom/common.ncl" in

{
  library_contract_major = common.library_contract_major,
  corpus_major = common.corpus_major,
  package_model = common.model_versions.package,
  game_profile_model = common.model_versions.game_profile,
  composition_schema = common.model_versions.composition,
  registration_manifest_schema = common.model_versions.registration_manifest,
  contract_major_alias = common.contract_major,
}
