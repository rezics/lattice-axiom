# Composition authoring fixtures

This version-independent directory pins exported JSON and diagnostic intent
for Nickel library contract 2 and R0 authoring corpus 2. Package and game-profile fixtures use
model 2; the normalized composition fixture uses schema 2; the registration
manifest remains schema 1. The Rust DTOs remain normative; golden JSON is
compared as a JSON value, not by object-key order. It is the authoring-contract
slice, not yet the controlled-import, resource-limit, or structured-provenance
corpus.

The fixtures target `nickel-lang-core` 0.18.x (Nickel CLI 1.17.x). From the
repository root, evaluate the positive fixtures with:

```powershell
nickel export --format json fixtures/composition/positive/core-empty-package.ncl
nickel export --format json fixtures/composition/positive/headless-profile.ncl
nickel export --format json fixtures/composition/positive/tool-profile.ncl
nickel export --format json fixtures/composition/positive/unicode-provenance-package.ncl
nickel export --format json fixtures/composition/positive/version-axes.ncl
```

The results must equal the adjacent `.golden.json` files after JSON parsing.
The core package is intentionally empty of features, dependencies, capability
rows, and registrations; its single data realization keeps it valid under the
normative Rust package invariant. The tool profile demonstrates the ADR 0021
escape hatch for an explicit non-R0 policy with positive effective limits.
The Unicode package proves that an NFC logical path is accepted across the
Nickel authoring contract and normative Rust provenance validator.
The embedded Rust corpus additionally normalizes the headless profile into the
adjacent `headless-composition.golden.json`. Its package-qualified feature map,
namespace grant, elevated trust ceiling, force-override permission, and
recovery permission prove that graph inputs and policy survive the authoring
boundary. This profile-to-composition normalization fixture is not a resolver
conformance pair with the separately minimal, feature-empty core package.

Every `.ncl` file under `negative/` must exit unsuccessfully. Its adjacent
`.expected.txt` lists stable diagnostic substrings rather than source spans or
rendering details, which Nickel may improve without changing the contract.

| Fixture | Expected rejection |
| --- | --- |
| `unknown-package-field.ncl` | Closed package contract rejects an extra field. |
| `malformed-package-version.ncl` | Strict SemVer rejects a leading zero. |
| `malformed-version-range.ncl` | The project range grammar rejects a naked version. |
| `unsupported-evaluation-policy.ncl` | R0 rejects an unimplemented policy ID. |
| `feature-domain-outside-package.ncl` | Feature domains cannot escape package domains. |
| `noncanonical-logical-path.ncl` | Logical provenance paths reject dot segments. |
| `invalid-tool-evaluation-policy.ncl` | Tool policies must use the owned policy kind. |
| `foreign-tool-evaluation-policy.ncl` | Tool policies must use the `latticeaxiom` policy namespace. |
| `unversioned-tool-evaluation-policy.ncl` | Tool policies require an explicit major. |
| `invalid-target-triple.ncl` | Realization targets must use the typed lowercase target-triple grammar, which allows `_` and `.` within components. |

These failures intentionally retain upstream Nickel message substrings for
authoring feedback. Stable Lattice diagnostic codes and source provenance are
added by the controlled evaluator boundary rather than inferred from this
standalone CLI rendering.

`typed-negative/non-nfc-provenance.ncl` intentionally passes Nickel's lexical
path contract and is then rejected with `compose.schema_mismatch` by the Rust
DTO boundary because its path uses decomposed Unicode. Nickel 0.18 has no NFC
normalization primitive, so this fixture proves the complete evaluator's
acceptance rule without falsely presenting the lexical contract as canonical.

The negative fixtures can be exercised individually with the same `nickel
export --format json` command. A harness should assert a nonzero exit code and
all non-empty lines from the matching `.expected.txt` file.
