# latticeaxiom-packages

`latticeaxiom-packages` is the bounded, engine-independent package resolver.
It consumes already evaluated `CompositionSpec` and `PackageSpec` values and
produces a deterministic **pre-build resolution receipt** plus build intent.

The receipt is not `latticeaxiom.lock`, `LockedGameGraph`, the compose crate's
`BuildPlan`, or a world frozen-closure receipt. It intentionally contains no
claim that an artifact, registration manifest, toolchain, ABI, or engine build
has been produced or verified. Those receipts belong to the later build and
package-kernel boundary before a normative lock may be persisted.

## Current fail-closed boundary

The resolver supports already evaluated in-memory, workspace, local-directory,
and local-prebuilt candidates; strict compatible-version selection; feature
and optional-dependency closure; capability cardinality/provider selection;
owner-qualified profile and active direct-dependency namespace grants; and
data or native-static realization intent. Search is deterministic and bounded
by versioned logical counters rather than wall-clock time.

The resolver rejects inputs that require information it does not own:

- dynamic interface availability or an exact engine-coupled host build;
- package parameters;
- verified artifact, manifest, producer, toolchain, ABI, or engine receipts.

## Next package-kernel gates

Before this receipt can be finalized into `latticeaxiom.lock`, the package
kernel must provide and verify:

- workspace/local acquisition, canonical source hashing, and symlink corpus;
- parameter evaluation and registration compilation against the selected namespace-grant receipt;
- host interface and exact engine-build compatibility;
- registration manifest and artifact hashes, producer/toolchain fingerprints,
  model/library versions, and authoritative compatibility fingerprints;
- atomic lock read/write plus shell and per-world frozen fixtures.

