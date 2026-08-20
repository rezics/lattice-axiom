let package = import "../../../nickel/latticeaxiom/package.ncl" in
let input = import "../support/core-empty-package-input.ncl" in
let invalid_features = {
  client_ui = {
    default = false,
    domains = ["client"],
  },
}
in
let invalid = std.record.update "features" invalid_features input in

package.make invalid
