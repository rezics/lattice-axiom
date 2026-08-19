let package = import "../../../nickel/latticeaxiom/package.ncl" in
let input = import "../support/core-empty-package-input.ncl" in
let invalid_provenance =
  std.record.update
    "logical_path"
    "fixtures/r0/./core-empty-package-input.ncl"
    input.provenance
in
let invalid = std.record.update "provenance" invalid_provenance input in

package.make invalid
