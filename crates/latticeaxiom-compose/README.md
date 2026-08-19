# latticeaxiom-compose

The single Nickel evaluation boundary (ADR 0010). It evaluates `package.ncl`
and `game.ncl` with the versioned `latticeaxiom.lib` contracts and converts the
fully evaluated expression directly into `CompositionSpec` or
`PackageManifest` through serde. There is no intermediate JSON bus.
