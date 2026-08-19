let latticeaxiom = import "latticeaxiom/game.ncl" in
{
  schema_version = 1,
  root_profile = {
    id = "latticeaxiom.demo",
    version = "2026.8",
  },
  package_requests = [
    {
      id = "latticeaxiom.official",
      version = "0.1.0",
      source = {
        kind = "path",
        path = "packages/official",
      },
      realization = 'Data,
    },
  ],
  capability_requirements = [
    {
      capability = "content.blocks@1",
      provider = null,
    },
  ],
  realization_preferences = ['Data, 'NativeStatic],
  parameters = {
    world_seed = "lattice-axiom-demo",
  },
  policy = {
    native_code = 'TrustedOnly,
    target = "x86_64-pc-windows-msvc",
    toolchain = "rust-1.97",
    authoritative = ["simulation", "worldgen"],
  },
} | latticeaxiom.GameProfile
