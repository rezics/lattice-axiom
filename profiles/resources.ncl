# Client-only resource composition; no world authority is selected here.
let latticeaxiom = import latticeaxiom_lib_v3 in
fun receipts =>
let candidate = fun source_id package path =>
  let receipt = std.record.get source_id receipts in
  { source_id = source_id, package = package, version = "0.1.0", path = path, content_hash = receipt.source_hash, priority = 100, provenance = receipt.entry_provenance }
in latticeaxiom.game.make {
  profile = "latticeaxiom:profile/resources@1",
  projection = "client-shell", projection_domains = ["client"],
  roots = {
    "@terrenia/textures-default" = { version = "=0.1.0", realization = { mode = "auto" } },
    "@latticeaxiom/water-default" = { version = "=0.1.0", realization = { mode = "auto" } },
  },
  source_universe = [
    candidate "terrenia:source/textures-default" "@terrenia/textures-default" "resource-packs/terrenia/textures-default",
    candidate "latticeaxiom:source/water-default" "@latticeaxiom/water-default" "resource-packs/latticeaxiom/water-default",
  ],
  realization_policy = ["data"],
  evaluation_policy = latticeaxiom.game.r0_evaluation_policy,
  policy = { namespace_grants = {}, maximum_trust = "data-only", allow_force_override = false, allow_recovery = true },
}
