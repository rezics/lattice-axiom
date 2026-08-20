# Lattice Axiom Nickel authoring contracts

These modules provide the R0 authoring surface for Nickel configurations:

- `latticeaxiom/common.ncl` owns shared identifier, SemVer, hash, domain, and
  provenance contracts;
- `latticeaxiom/package.ncl` owns package contracts and the package
  constructor;
- `latticeaxiom/game.ncl` owns game-profile contracts, frozen R0 evaluation
  limits, and the profile constructor;
- `latticeaxiom/registration.ncl` owns registration fragment and manifest
  contracts.

The modules target `nickel-lang-core` 0.18.x, shipped by Nickel CLI 1.17.x.
They expose `library_contract_major = 2` and `corpus_major = 2`. The legacy
`contract_major` field is an explicit alias for the library contract axis, not
the corpus axis. A consumer must reject unsupported majors instead of guessing
compatibility.

## Authority and compatibility

The Rust DTOs and validators in `latticeaxiom-core` and
`latticeaxiom-compose` are normative. Nickel contracts provide early authoring
feedback and deterministic defaults, but successful Nickel evaluation is not
authorization to resolve, load, or persist a package. The exported JSON must
still deserialize and validate through Rust.

The composition golden corpus in `../fixtures/composition` is the executable authoring receipt
for library contract 2 and corpus 2. Package model 2 carries the typed
target-triple acceptance boundary; game-profile model 2 carries the narrowed
canonical source-path acceptance boundary; registration-manifest schema 1
remains wire-compatible. Composition schema 2 records the normalized target
and complete profile policy outside the Nickel authoring model. The later
controlled evaluator corpus adds
import-policy, resource-limit, and structured-provenance coverage. A breaking
field, encoding, default, or invariant change requires a coordinated major
bump of every affected library, corpus, model, or schema axis and matching
Rust support. Additive diagnostics that do not change exported JSON may remain
within the same majors.

Nickel checks the lexical shape of logical paths and accepts Unicode text. The
normative Rust DTO boundary performs the Unicode NFC check because Nickel 0.18
does not expose normalization to contracts. Every CLI or embedded evaluator
must include that typed Rust conversion; raw `nickel export` alone is authoring
feedback, not final acceptance.

All exported DTO contracts are closed unless the field is intentionally a
dictionary. Unknown top-level or row fields therefore fail during authoring.
