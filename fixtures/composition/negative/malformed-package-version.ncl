let package = import "../../../nickel/latticeaxiom/package.ncl" in
let input = import "../support/core-empty-package-input.ncl" in
let malformed = std.record.update "version" "01.0.0" input in

package.make malformed
