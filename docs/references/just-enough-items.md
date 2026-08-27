# Just Enough Items UI reference

The inventory item browser was informed by the following external reference:

- Upstream: <https://github.com/mezz/JustEnoughItems>
- Inspected revision: `590069b289f12740332d9e60aab263dea135562b`
- License at that revision: MIT

The implementation borrows only interaction and layout concepts: a dense item
grid beside the player inventory, text search, and bounded page navigation.
No JEI source code, assets, or Minecraft UI assets are copied, and the
reference checkout is not a build dependency.

Lattice Axiom adds package-authored primary categories as tabs. An item has at
most one primary category so tab placement is deterministic. Semantic tags
remain many-to-many and are searched with `#tag` tokens. This separates UI
navigation from gameplay semantics and avoids treating a display category as
an item capability.

The client uses a small typed reducer for inventory, workbench, filter, focus,
and pagination state. Bevy remains the sole state and scheduling runtime; no
second state-machine runtime is introduced for local synchronous UI state.
