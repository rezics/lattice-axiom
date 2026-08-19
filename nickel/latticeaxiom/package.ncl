let common = import "latticeaxiom/common.ncl" in
{
  Package = {
    schema_version | Number,
    package | common.Identity,
    dependencies | Array common.Dependency,
    provides | {
      capabilities | Array String,
      blocks | Array common.Block,
    },
    realizations | Array Dyn,
  },
}
