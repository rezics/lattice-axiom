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
They expose `library_contract_major = 3` and `corpus_major = 3`. The legacy
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
for library contract 3 and corpus 3. Package model 3 carries typed namespace
requests plus direct-dependency delegations; game-profile model 3 binds trusted
namespace grants to exact root-package grantees. Registration-manifest schema 1
remains wire-compatible. Composition schema 3 records the normalized target,
owner-bound profile grants,
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
