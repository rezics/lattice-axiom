let game = import "../../../nickel/latticeaxiom/game.ncl" in
let input = import "../support/tool-profile-input.ncl" in
let invalid =
  std.record.update
    "evaluation_policy"
    "latticeaxiom:nickel-evaluation-policy/developer"
    input
in

game.make invalid
