# Lattice Axiom composition models

`latticeaxiom-compose` contains the engine-independent data models that connect
Nickel authoring, deterministic package resolution, registration compilation,
runtime activation planning, settings, observability, and world preflight.

The crate does not evaluate Nickel, resolve packages, load code, or wrap Bevy.
Its DTOs are safe to serialize across persistent and process-external
boundaries and deliberately contain no Bevy entities, handles, type IDs, or
function pointers.

The current schema version is the R0 skeleton. Cross-field `validate` methods
and hash verification methods are part of the boundary: deserializing a DTO is
not sufficient authorization to load code or open a world writer.
