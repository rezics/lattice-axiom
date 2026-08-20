# Lattice Axiom composition models

`latticeaxiom-compose` contains the engine-independent data models that connect
Nickel authoring, deterministic package resolution, registration compilation,
runtime activation planning, settings, observability, and world preflight.

The optional `nickel-evaluator` feature adds direct typed evaluation for
import-free expressions and a trusted staged D0 adapter for local authoring
fixtures. The adapter verifies the requested source-table closure and immutable
snapshots before copying only receipt-covered bytes into a fresh staging tree;
the original physical source paths are never passed to Nickel. It deliberately
remains a trusted adapter because Nickel's evaluator retains ambient filesystem
access and does not provide the hard resource meters required by the production
policy. The crate does not resolve packages, load package code, or wrap Bevy.
Its DTOs are safe to serialize across persistent and process-external
boundaries and deliberately contain no Bevy entities, handles, type IDs, or
function pointers.

Shipped package manifests use a two-stage typed binder. The first evaluation
exports only authored semantics; Rust then computes the canonical registration
fragment receipt and evaluates again with kernel-owned package provenance and
that receipt. The binder rejects semantic drift and verifies every nested
registration provenance and realization receipt before returning a
`PackageSpec`. Whole-root source-table hashes and raw declaration-file hashes
remain distinct receipt types throughout this boundary.

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

The current D0 skeleton uses Nickel library contract 3, authoring corpus 3,
composition schema 3, package model 3, and game-profile model 3; the
registration-manifest schema remains 1. Package model 3 records typed namespace
requests and direct-dependency delegations, while profile model 3 binds trusted
namespace grants to exact root-package grantees. Cross-field `validate` methods
and hash verification methods are part of the boundary: deserializing a DTO is
not sufficient authorization to load code or open a world writer.
