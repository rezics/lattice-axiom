let game = import "../../../nickel/latticeaxiom/game.ncl" in
let input = import "../support/headless-profile-input.ncl" in
let unsupported =
  std.record.update
    "evaluation_policy"
    "latticeaxiom:nickel-evaluation-policy/experimental@1"
    input
in

game.make unsupported
