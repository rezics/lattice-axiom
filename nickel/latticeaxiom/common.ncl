{
  Identity = {
    id | String,
    version | String,
  },

  Source = {
    kind | String,
    path | String,
  },

  PackageRequest = {
    id | String,
    version | String,
    source | {
      kind | String,
      path | String,
    },
    realization | Dyn,
  },

  CapabilityRequirement = {
    capability | String,
    provider | Dyn,
  },

  Dependency = {
    id | String,
    version | String,
  },

  Block = {
    id | String,
    display_name | String,
    solid | Bool,
    is_air | Bool,
    color | Array Number,
  },
}
