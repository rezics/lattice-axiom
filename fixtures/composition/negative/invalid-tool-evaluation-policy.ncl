let game = import "../../../nickel/latticeaxiom/game.ncl" in
let input = import "../support/tool-profile-input.ncl" in
let invalid =
  std.record.update
    "evaluation_policy"
    "latticeaxiom:anything/developer@1"
    input
in

game.make invalid
