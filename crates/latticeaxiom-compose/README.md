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

Whole-root acquisition can retain an immutable raw-byte `SourceSnapshot`; its
budget is deliberately separate from the evaluated import-closure limits.
Versioned policy receipts describe hard, soft, or unsupported deadline,
memory, and Nickel call-frame enforcement. The production R0 policy accepts
only hard enforcement and therefore remains fail-closed until the worker
backends and the required Nickel loader/VM hooks are connected.

Protocol v1 uses one four-byte big-endian length-prefixed JSON request and one
response, with a 256 MiB payload cap. `latticeaxiom-compose-worker` is the
internal process endpoint. The public `latticeaxiom-compose evaluate` command
requires an explicit absolute worker path and a validated request JSON file;
the embedded API and CLI use the same complete-request controller. The parity
fixture directly compares CLI response bytes, after removing exactly one
terminal LF, with the embedded canonical encoding for one import-free trusted
package success and one controller-fatal failure. It does not establish broad
production-worker conformance. Process separation limits failure impact but is
not a hostile-code sandbox.

The current R0 skeleton uses Nickel library contract 2, authoring corpus 2,
composition schema 2, package model 2, and game-profile model 2; the
registration-manifest schema remains 1. Profile model 2 records the narrowed
canonical source-path acceptance boundary. Cross-field `validate` methods and
hash verification methods are part of the boundary: deserializing a DTO is not
sufficient authorization to load code or open a world writer.
