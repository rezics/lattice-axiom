let package = import "../../../nickel/latticeaxiom/package.ncl" in
let input = import "../support/core-empty-package-input.ncl" in
let decomposed_provenance =
  std.record.update
    "logical_path"
    "fixtures/composition/support/cafe\u{301}.ncl"
    input.provenance
in
let decomposed_input =
  std.record.update "provenance" decomposed_provenance input
in

package.make decomposed_input
