let game = import "../../../nickel/latticeaxiom/game.ncl" in
let input = import "../support/headless-profile-input.ncl" in
let invalid_request = std.record.update "realization" { mode = "auto", kind = "data" } input.roots.core in
let invalid_roots = std.record.update "core" invalid_request input.roots in
let invalid = std.record.update "roots" invalid_roots input in

game.make invalid