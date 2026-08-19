let latticeaxiom = import "latticeaxiom/package.ncl" in
{
  schema_version = 1,
  package = {
    id = "latticeaxiom.official",
    version = "0.1.0",
  },
  dependencies = [],
  provides = {
    capabilities = ["content.blocks@1"],
    blocks = [
      {
        id = "air",
        display_name = "Air",
        solid = false,
        is_air = true,
        color = [0, 0, 0, 0],
      },
      {
        id = "stone",
        display_name = "Stone",
        solid = true,
        is_air = false,
        color = [112, 118, 126, 255],
      },
      {
        id = "dirt",
        display_name = "Dirt",
        solid = true,
        is_air = false,
        color = [134, 96, 67, 255],
      },
      {
        id = "grass",
        display_name = "Grass",
        solid = true,
        is_air = false,
        color = [83, 142, 68, 255],
      },
    ],
  },
  realizations = ['Data],
} | latticeaxiom.Package
