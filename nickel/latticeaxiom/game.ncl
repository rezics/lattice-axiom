let common = import "latticeaxiom/common.ncl" in
{
  GameProfile = {
    schema_version | Number,
    root_profile | common.Identity,
    package_requests | Array common.PackageRequest,
    capability_requirements | Array common.CapabilityRequirement,
    realization_preferences | Array Dyn,
    parameters | { _ | String },
    policy | {
      native_code | Dyn,
      target | String,
      toolchain | String,
      authoritative | Array String,
    },
  },
}
