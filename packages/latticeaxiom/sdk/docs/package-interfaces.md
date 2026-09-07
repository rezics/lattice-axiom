# Author-owned package interfaces

Status: accepted architectural direction, 2026-09-07; implementation pending.
This document refines the [ecosystem objective](../../../../docs/ecosystem-direction.md).
Examples are illustrative, not a new manifest syntax or a shipped SDK API.
Existing source and generated ABI layouts remain authoritative for current use.

## Ownership and scope

A package can be a library, a framework, an implementation, a framework extension
or an application root. It can export APIs that other packages import. This
relationship must work without adding the exported domain to a platform-owned
gameplay enum, command list or schema catalog.

The kernel owns package identity, dependency resolution, binding receipts and
activation validation. The SDK owns author-facing declarations and generated
boundary bindings. World/storage packages own authoritative state and durable
commit invariants. Domain packages own their interfaces, behaviors, state
schemas, tests and migrations. A contract package can be separate from its
implementations, which keeps normal/build dependencies acyclic.

Finite runtime primitives and fixed authority boundaries are legitimate. They
must not be confused with a closed inventory of every future gameplay concept.
Packages may build their own abstractions above those primitives. A new hardware
or engine facility still needs a suitable host implementation or source extension.

## Two authoring paths

| Path | Authoring and compatibility model |
| --- | --- |
| Source packages compiled into one native product | Normal Rust public functions, types, traits, generics and Bevy APIs; Cargo compiles the declared package closure; engine-coupled code records exact build requirements |
| Interfaces crossing a portable module boundary | Explicit versioned contracts and generated typed bindings over the supported ABI; no arbitrary Rust references, trait objects or Bevy internals cross that boundary |

Pure code reuse must not require ECS registration, a global service registry or
an event bus. Importing a library and activating world systems are distinct
operations. Products explicitly install stateful contributions into a world or
client instance. Tooling should expose package activation and shared state
instead of relying on import-time global side effects.

The portable SDK's supported type vocabulary must not restrict all source-level
Rust APIs. A source-only package is a supported distributable building block,
not merely a development exception. Authors build and publish product artifacts;
ordinary players should not need a Rust compiler.

## A domain the platform does not know

```text
@alice/orbit-api
  exports: BodyId, OrbitState, PredictionError
  exports: OrbitSolver.predict(state, duration) -> Result

@alice/orbit-simulation
  imports: @alice/orbit-api
  implements: OrbitSolver

@bob/spaceflight
  imports: @alice/orbit-api
  requires: an OrbitSolver implementation

@carol/my-space-game
  selects: @bob/spaceflight and @alice/orbit-simulation
  binds: spaceflight's solver import to the selected solver instance
```

The first acceptance can use ordinary Rust exports and explicit product
construction. A portable realization needs generated ABI bindings as well.
`OrbitSolver` belongs to its API author; it does not become a kernel feature.
An alternate implementation can satisfy the same contract. The consumer may
define its own persistent flight state and presentation, using the appropriate
world/client contracts. Pure prediction and authoritative world mutation are
different operations even if both are initiated by the same package.

## Identity and binding

Keep these concepts distinct in the design; names here are descriptive, not
new DTO declarations:

- Logical package identity and its release version.
- The exact resolved source/artifact realization and target/build requirements.
- An author-owned interface identity and its contract version.
- The provider runtime instance and its world/client scope.
- Persistent state identity and schema version.

A dependency edge must resolve an import in the consumer's scope to an exact
provider/export. A matching capability name alone is not an instance binding.
The product lock/build records must eventually preserve the chosen imports,
providers, interface contracts and realization evidence. A runtime handle must
not stand in for a persistent object ID.

The existing ABI uses an **unversioned canonical interface name**, with version
information carried separately. Preserve that identity rule; an illustrative
API version must not silently change `InterfaceName` parsing or existing tokens.
Likewise, a new release hash must not replace a persistent schema identity.

## Dependency relationships

| Relationship | Required meaning |
| --- | --- |
| Ordinary dependency | Use an identified library or implementation; reuse need not create a shared world service |
| Shared-instance dependency | Consumers require a compatible contract and the same designated runtime instance, scoped to the relevant world/client |
| Replaceable provider | Depend on an API contract; the product selects and binds a conforming implementation |
| Optional integration | Activate an explicit adapter when its participating packages and contracts are present |

The shared-instance relationship is informed by npm's `peerDependencies`, but
games additionally need state ownership and world scope.
[npm package relationships](https://docs.npmjs.com/cli/v11/configuring-npm/package-json/#peerdependencies)

Private libraries may eventually permit isolated versions. Shared inventories,
registries or authoritative simulation owners must not be duplicated merely to
satisfy incompatible version ranges. Types or handles from different resolved
instances cannot be treated as interchangeable because their names match.

General multi-version Lattice package resolution is not implemented or specified
here. An initial single-version implementation should reject unsupported
combinations explicitly. Evolution requires instance-aware graph/lock semantics,
source dependency mapping and old-lock compatibility; renaming a map key is not
a complete migration. Cargo remains the compiler dependency resolver, while
Lattice accounts for package ownership, selected products and runtime bindings.

## What a boundary contract describes

| Part | Required information |
| --- | --- |
| Ownership and version | Owner, canonical interface identity, supported contract versions and compatibility rules |
| Values and operations | Input/output schemas, functions, declared domain errors, events and resource types |
| Memory and resources | Copy/borrow/transfer semantics, allocation ownership, release operations, handle scope, generation and invalidation |
| Execution | Permitted context/stage, direct or staged mutation, concurrency, blocking and re-entry behavior |
| Binding | Which export and runtime instance satisfy each import; required versus optional bindings |
| Failure and shutdown | Error effects, partial-write policy, outstanding calls and resource lifetime through shutdown |
| Evolution | API/ABI compatibility, semantic changes and explicit adapters; persistent migrations are separate |

Descriptions and versions must cover behavior such as units, update timing and
failure atomicity. A layout/signature hash proves an exact descriptor, not that
two implementations behave equivalently. Document which additive changes old
bindings tolerate; do not require exact full-descriptor equality while claiming
all compatible minor versions are interchangeable.

Portable values need an explicit supported representation. Common records,
variants, bounded buffers and opaque resources are candidates; arbitrary Rust
generics or memory layouts do not automatically become portable. Source APIs can
retain richer types and generate a narrower boundary where needed.

Evaluate existing interface-description and binding tools before choosing the
portable authoring format. WIT supplies a useful model of author-defined types,
imports, exports and resources, but it does not define behavioral semantics.
Its suitability for our existing C ABI and workload needs a concrete prototype.
No mandatory Wasm runtime or new interface language is selected here.
[WIT reference](https://component-model.bytecodealliance.org/design/wit.html)

## Resolve, link and activate

The intended lifecycle is:

1. Resolve declared dependencies and selected implementations; validate ownership
   and target compatibility without executing package initialization.
2. Compile/check exported contracts and consumer requirements; generate typed
   bindings and freeze the selected import-to-export relationships.
3. Build a NativeStatic product, or verify and load the appropriate portable
   artifacts using a supported loader. These are different evidence paths.
4. Construct scoped instances and bind imports, then activate systems in an
   explicit dependency-compatible lifecycle order. Diagnose initialization cycles.
5. Execute through resolved bindings, observe declared failures, and shut down
   while respecting outstanding work, borrowed memory and instance lifetimes.

For native source packages, direct typed calls and ordinary construction can
implement these relationships. For portable native modules, bootstrap/query
operations negotiate a supported table and generated code calls that binding.
A global string lookup and JSON dispatch on every hot call are not required.
Runtime binding cannot invent dependencies absent from the frozen composition.

The current native ABI targets a specific calling convention, pointer width and
endianness. Its name does not mean one native binary runs on every operating
system or CPU. Build and distribute each supported target as needed. If a later
Wasm realization is added, it must separately prove host API availability and
behavioral compatibility; portability cannot be inferred from a file extension.

## Bevy integration and authority

Use Bevy's App, ECS, scheduler, rendering, assets and task pools. Do not build a
parallel ECS or scheduler to make the package model look self-contained.
Packages can contribute native components, systems and SystemSets. Public
cross-boundary schemas are deliberately separate from Bevy's process-local types.

Package-local ordering and shared integration points need explicit contracts.
Stable commit/capture boundaries may constrain ordering without making every
domain-specific system phase a kernel enum. New package-owned grouping must map
to Bevy and preserve the world's mutation/commit invariants.

Cross-package calls do not bypass scheduler access declarations. A call that
mutates shared state must be represented in the caller/callee execution model,
use an appropriate staged command path, or run under an explicit exclusive
contract. Pure computations need no world transaction. Asynchronous results
enter world state at a defined boundary with bounded work and stale-result checks.

Portable memory ownership, error containment and direct-write failure rules
must remain explicit. Do not assume a failed native callback can always be
unloaded or rolled back. The current ABI's library-lifetime and recovery rules
remain in force until a separately validated lifecycle change replaces them.
Hot reload of arbitrary native code is not an implicit requirement of openness.

First-party system replacement should follow the same package contracts as
external replacement. If a deep engine change requires internal APIs, expose a
supported pinned source-build path and record that compatibility scope. Repeated
community integrations can motivate a new shared API without forcing every
experimental feature into a permanent core contract.

## Persistence and presentation

Packages own their durable schema declarations and migrations; storage owns
durable commit and recovery. A saved world must identify the relevant package
combination and schemas. Runtime handles and compiler type identities must not
be serialized as persistent domain identities. Removing or replacing a provider
requires an explicit state-preservation/conversion policy.

Resource packs remain independent client presentation providers. Code packages
may expose client integration APIs, but selecting a texture, shader or other
resource pack must not introduce hidden authoritative gameplay dependencies.
Source-control rules for binary art remain unchanged.

## Current evidence and gaps

The following is a source inspection on 2026-09-07, not a new runtime test run.

| Existing foundation | Limit that remains |
| --- | --- |
| [SDK declarations](../crates/latticeaxiom-sdk/src/lib.rs) allow authored components and row systems | The finite `dual` parameter vocabulary is an adapter subset, not a general package export/import API |
| [Component modes](../crates/latticeaxiom-sdk/src/component.rs) distinguish host-typed, shared-schema and dynamic intent | The first prototype's fixed-width field markers do not prove arbitrary dynamic component registration |
| [Interface identities/descriptors](../crates/latticeaxiom-abi/src/interface.rs) and [ABI query layouts](../crates/latticeaxiom-abi/schema/portable_native_abi_v0_1.rs) support general names and table negotiation | They do not by themselves implement author-defined package exports, consumer bindings or a native loader |
| [Registration inputs](../../kernel/crates/latticeaxiom-registration/src/model.rs) include schemas, callbacks and system contracts | General package-to-package API construction and activation must be demonstrated, beyond scheduled callbacks |
| [Locked graph](../../kernel/crates/latticeaxiom-compose/src/graph.rs) selects exact packages | Nodes are keyed by `PackageName`; general same-name multi-version/instance resolution is not represented |
| [Schedule compiler](../../kernel/crates/latticeaxiom-registration/src/schedule.rs) orders declared systems | It currently accepts ten fixed stages; extensible grouping must preserve authority boundaries |
| [Dual fixture](../../../example/dual-gameplay/crates/latticeaxiom-dual-fixture/src/lib.rs) compares static and reference portable behavior | It explicitly does not load or unload native code |

The [ecosystem delivery gates](../../../../docs/ecosystem-direction.md#delivery-gates)
are the acceptance contract for advancing this design. In particular, repeat
the author-defined-domain example with an alternate provider, incompatible
versions, consumer-owned saved state and an independent product. Include a real
loaded module before claiming portable-native support.

Detailed authoring syntax/code generation, multi-version graph evolution,
shared-instance scope, interface evolution rules and native loader lifecycle
remain technical design work. Select them through bounded implementations and
upstream study rather than treating this document as a completed ABI specification.
