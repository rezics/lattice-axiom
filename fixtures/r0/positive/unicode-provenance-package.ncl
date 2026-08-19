let package = import "../../../nickel/latticeaxiom/package.ncl" in
let input = import "../support/core-empty-package-input.ncl" in
let unicode_provenance =
  std.record.update
    "logical_path"
    "fixtures/r0/support/café.ncl"
    input.provenance
in
let unicode_input = std.record.update "provenance" unicode_provenance input in

package.make unicode_input
