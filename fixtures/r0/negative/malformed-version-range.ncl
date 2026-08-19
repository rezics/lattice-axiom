let game = import "../../../nickel/latticeaxiom/game.ncl" in
let input = import "../support/headless-profile-input.ncl" in
let malformed_root =
  std.record.update "version" "0.1.0" input.roots.core
in
let malformed_roots = std.record.update "core" malformed_root input.roots in
let malformed = std.record.update "roots" malformed_roots input in

game.make malformed
