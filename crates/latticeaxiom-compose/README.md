# Lattice Axiom composition models

`latticeaxiom-compose` contains the engine-independent data models that connect
Nickel authoring, deterministic package resolution, registration compilation,
runtime activation planning, settings, observability, and world preflight.

The optional `nickel-evaluator` feature adds direct typed evaluation for
import-free expressions and trusted local authoring fixtures. It deliberately
retains Nickel's ambient filesystem resolver and is not the controlled-root or
resource-isolated production evaluator. The crate does not resolve packages,
load package code, or wrap Bevy. Its DTOs are safe to serialize across
persistent and process-external boundaries and deliberately contain no Bevy
entities, handles, type IDs, or function pointers.

The current schema version is the R0 skeleton. Cross-field `validate` methods
and hash verification methods are part of the boundary: deserializing a DTO is
not sufficient authorization to load code or open a world writer.
