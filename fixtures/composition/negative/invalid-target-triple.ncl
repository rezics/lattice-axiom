let package = import "../../../nickel/latticeaxiom/package.ncl" in
let input = import "../support/core-empty-package-input.ncl" in
let invalid_data = std.record.update "targets" ["X86_64/windows"] input.realizations.data in
let invalid_realizations = std.record.update "data" invalid_data input.realizations in
let invalid = std.record.update "realizations" invalid_realizations input in

package.make invalid
