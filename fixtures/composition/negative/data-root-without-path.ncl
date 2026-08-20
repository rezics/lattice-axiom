let package = import "../../../nickel/latticeaxiom/package.ncl" in
let input = import "../support/core-empty-package-input.ncl" in
let invalid_artifact = { kind = "data-root" } in
let invalid_data = std.record.update "artifact" invalid_artifact input.realizations.data in
let invalid_realizations = std.record.update "data" invalid_data input.realizations in
let invalid = std.record.update "realizations" invalid_realizations input in

package.make invalid