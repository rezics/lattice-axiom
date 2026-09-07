# An ecosystem for independently authored voxel games

Status: accepted product and architectural direction, 2026-09-07.
This delivery updates documentation only. The implementation gates below remain
open; it does not introduce a new runtime, manifest format, ABI or license.

This document owns the ecosystem objective and acceptance criteria. The
[package interface design](../packages/latticeaxiom/sdk/docs/package-interfaces.md)
owns the proposed interface boundaries. Package manifests, source and executable
tests remain authoritative for current behavior; [rebuild execution](../REBUILD_PROGRESS.md)
records delivered work and outstanding acceptance.

## Product objective

Lattice Axiom should support an ecosystem with the composability of a programming
language ecosystem: authors can publish libraries, define interfaces, implement
frameworks, extend other authors' systems, compose games and distribute those
games independently. Terrenia is the first playable product and a reference
consumer of that ecosystem. External authors must have a supported path to the
same implementation capabilities used by first-party packages.

A familiar voxel survival baseline can be a useful starting point. Distinctive
first-party gameplay is not a prerequisite for the ecosystem's value. Deep
modification, reusable systems, manageable upgrades and independent commercial
distribution are product objectives in their own right. The platform must also
support games and frameworks its maintainers did not anticipate.

The desired order-of-magnitude increase in progress is a target for ecosystem
throughput through reuse, parallel development and less integration rework. It
is not a measured performance result or a promise that every feature takes one
tenth of the effort. Content creation, testing and maintenance still need owners.

## Lessons to apply

- Preserve the depth of Java Edition modding: authors can introduce systems,
  change existing behavior and build on other authors' work. Fabric explicitly
  supports class transformations, including transformations of other mods'
  classes. We seek the resulting creative freedom, without requiring every
  integration to patch implementation internals. [Fabric Loader](https://docs.fabricmc.net/develop/loader/)
- Avoid making the official extension surface the ceiling of all development.
  Bedrock's supported custom components operate through its scripting API and
  inherit that API's constraints. A different implementation language alone
  does not remove this limitation. [Bedrock custom components](https://learn.microsoft.com/en-us/minecraft/creator/documents/scripting/custom-components?view=minecraft-bedrock-stable)
- Improve integration beyond dependency metadata. Existing Java loaders already
  declare versions, incompatible dependencies and ordering. Lattice must also
  address shared state, behavioral conflicts, migration and explanations.
  [NeoForge mod files](https://docs.neoforged.net/docs/gettingstarted/modfiles/)
- Adopt the language-ecosystem model of author-owned exports, dependencies and
  reusable frameworks. Reproducing that model does not require adopting
  JavaScript as the gameplay language or creating a new programming language.
  [Node package exports](https://nodejs.org/api/packages.html)

## Ecosystem commitments

1. **Packages can define APIs.** Package-to-package calls are ordinary supported
   composition. New domain types, functions and interfaces must not require
   additions to a central list of gameplay concepts.
2. **Libraries do not have to be active plugins.** Pure algorithms and data
   structures can be reused without installing systems into a world. Products
   explicitly activate stateful systems for the relevant world or client.
3. **Deep extension is a supported delivery path.** Source packages may use
   Bevy directly and participate in product builds with a pinned engine and
   toolchain. Authors must not need a permanent whole-engine fork merely to
   distribute such an extension. Portable interfaces provide a separate
   compatibility promise, not a universal restriction on source packages.
4. **Composition is game development.** A game author owns package selection,
   provider choices, explicit overrides, compatibility adapters, progression,
   release configuration and acceptance scenarios.
5. **Community work can ship independently.** A package or game need not wait
   for inclusion in a Terrenia theme update or an official curated distribution.
   Third-party registries and distribution tools must have a supported path.
6. **Installed worlds have continuity.** Reopening a frozen combination and
   upgrading a copied world are distinct workflows. A lock alone neither
   proves upgrade compatibility nor guarantees future artifact availability.

These commitments preserve existing package ownership, acyclic normal/build
dependencies, native Bevy runtime use, client-only resource packs, text-only
source control and saved-world preservation. They do not introduce a second ECS,
scheduler or general-purpose asset editor.

## Independent release boundaries

| Release unit | Responsibility |
| --- | --- |
| Runtime and SDK | Execution boundaries, stable host contracts, supported targets and migration tooling |
| Contract and framework packages | Author-owned APIs, shared systems, compatibility suites and adapters |
| Gameplay and content packages | New mechanics, world recipes, content and presentation under their declared dependencies |
| Game distributions | A tested package combination, product identity, installable artifacts and an upgrade policy |

These units have independent versions. A gameplay change that only uses existing
contracts should not require an engine upgrade. A source extension that changes
engine internals can require a new exact build; the affected consumers and
rebuild requirements must be explicit. The product composer links these units
into a frozen release without imposing one ecosystem-wide content version.

Terrenia may curate a conservative combination while another game adopts a new
framework immediately. Package popularity or inclusion in Terrenia does not
make a package's abstractions mandatory for unrelated games.

## Compatibility and integration

Compatibility has several independent dimensions:

| Dimension | Required evidence or behavior |
| --- | --- |
| Dependency and target compatibility | Resolve declared requirements and exact sources/realizations; explain conflicting requirements before activation |
| API and ABI compatibility | Bind a supported interface version and layout to a specific provider; preserve documented semantic behavior or use an adapter |
| Shared-instance compatibility | Identify which consumers must share one world service or state owner; do not duplicate it merely to satisfy version ranges |
| Gameplay compatibility | Exercise representative recipes, progression, overrides, system interactions and multiplayer paths when supported |
| Persistence compatibility | Define package-owned schemas, migrations and recovery for saved state; preserve the old world and runtime combination |

Version numbers and hashes establish declarations and exact identity. They do
not prove behavioral equivalence. Changing units, failure atomicity or update
timing can break consumers without changing a function signature. Independent
packages also cannot be assumed compatible merely because each passes its own
tests. Arbitrary internal patches cannot receive universal compatibility promises.

Composition tooling should explain the dependency chain, selected provider,
override provenance and affected state. For example, an author should be able
to inspect an effective recipe and see its original definition, each explicit
patch and the winning decision. Unknown combinations, tested combinations and
known conflicts must be distinguished. Structural checks protect activation
invariants; optional curation must not become permission to publish a package.

Upgrade previews should identify changed contracts, dependent packages and
persistent schemas. Test the affected integrations and the distribution's
representative journeys rather than claiming exhaustive coverage of every
possible package combination. Retain the previous obtainable package/runtime
closure and migrate copies. Removing a package must not silently erase its
state; define preservation, explicit conversion or a refusal to open for writes.

## Independent distribution and commercial use

A game author should be able to select reusable packages, add original gameplay
and resources, choose their own product name and entry point, and distribute
installable client/server artifacts. Running that product must not require a
Terrenia installation, an original development checkout, a mandatory Lattice
account, a central launcher or an official marketplace.

This is broader than distributing Minecraft mods: Minecraft's EULA distinguishes
mods from redistribution of modified game clients and servers.
[Minecraft EULA](https://www.minecraft.net/en-us/eula)

The repository remains AGPL-3.0-only. AGPL permits charging for copies, with its
source and redistribution obligations. Independent commercial distribution does
not imply permission to make a proprietary combined derivative. A proprietary
product policy, if desired, requires a separate licensing decision; dynamic
linking or process separation alone does not establish that permission.
[Current license](../LICENSE), [AGPL text](https://opensource.org/license/agpl-3-0)

Package and distribution authors must account for each included dependency and
asset's license. Future packaging should retain license/attribution information
and make restrictions visible. This document neither changes ownership nor
grants rights over third-party work. The text-only repository rule governs
source control; it does not prohibit a separately managed game distribution
from including properly licensed binary assets.

## Delivery gates

All gates in this table are **pending**. They define future implementation and
acceptance slices, not tests executed by this documentation change.

| Gate | Required demonstration |
| --- | --- |
| E1: ordinary reusable library | A package with no activated world systems exports code used by another package through declared source dependencies |
| E2: author-defined interface | An API package, an implementation package and a consumer package introduce a domain absent from the platform; a fourth product composes them without platform source edits |
| E3: binding and replacement | The product selects an alternate conforming implementation; missing providers, incompatible contracts and wrong shared-instance bindings produce useful failures |
| E4: integration and continuity | The consumer owns persistent state; copied-world upgrade, old-version reopen, a deliberately incompatible upgrade and explicit override provenance are exercised |
| E5: independent game | A non-Terrenia product uses first-party-equivalent extension paths and runs from its release artifacts without the source checkout, Terrenia or platform-required central services |

Start E2 with a real NativeStatic build. A portable-native claim additionally
requires actual separately built module loading and calls through its ABI;
reference tables and headless equivalence fixtures do not establish this.
The same boundary applies to any later Wasm realization. Realization scope must
be recorded rather than inferred from the package label.

Before implementation, refine each slice against current upstream APIs and the
existing source; precise manifest fields, an interface-description toolchain,
general multi-version resolution and runtime loading policy are not frozen by
this document. Preserve old locks and schemas during any graph evolution.

Measure author time to a working package and release, required platform edits,
unrelated packages affected by an upgrade, conflict diagnosis effort, reuse
without forks and successful copied-world migrations. Add optimized CPU/memory
and native frame-time measurements for affected runtime paths. Report functional,
visual and performance evidence separately. A package count, documentation
check or single successful launch is not ecosystem acceptance.
