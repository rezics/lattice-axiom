# `latticeaxiom.lib`

Versioned Nickel contracts for the milestone 2 composition boundary. Game and
package authors import `latticeaxiom/game.ncl` or
`latticeaxiom/package.ncl`; the embedded evaluator receives this directory as
its controlled library import root.

The contracts validate the declarative shape and primitive field types. The
Rust `latticeaxiom-compose` models then validate cross-field semantics such as
stable identifiers, exact versions, uniqueness, and source paths.
