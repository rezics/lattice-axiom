---
title: First-playable player surfaces (inspect, pick-block, inventory, craft)
status: active
type: planning
updated: 2026-08-22
decision:
  - ../decisions/0014-adopt-bevy-upstream-first.md
  - ../decisions/0025-freeze-client-shell-settings-observability-and-player-contracts.md
  - ../decisions/0027-freeze-authoritative-world-and-persistence-contract.md
---

# First-playable player surfaces

This page is the implementation contract for the remaining player loop after
HUD overlay, hotbar digits, mining, and view-distance exist. It does not
rewrite [roadmap-first-demo.md](roadmap-first-demo.md) D0–D10, does not
authorize a same-App start-then-Playing transition, and does not copy
Minecraft or Jade source.

External reference under the demo repo `.temp/reference/Jade` is **study
only**. Absorb the *problem* (the player must understand the aimed target)
and the *composition idea* (named fragments, one owner per key). Do not
absorb Java providers, NBT dumps, mixins, ITooltip, WailaPlugin, or
Minecraft registry names.

## Current gap (facts)

| Surface | Shipped | Missing |
| --- | --- | --- |
| Crosshair target | `refresh_target` every tick; overlay shows display name + chunk | Typed inspect fragments; harvest / package owner / stable id |
| F inspect | Records `TargetInspectReceiptV1` | Must stay an inspect *edge*, not the only way to see the target |
| Middle-click pick | No `PlayerActionV1` | Survival pick: select/swap the inventory stack that places the aimed block |
| Inventory | E toggles a display-only 36-slot grid | Click-to-move stacks through an authoritative command |
| Craft / workbench | Headless `craft_recipe` + catalog recipes | Client recipe list; hand craft in inventory; workbench list when the aimed block binds `latticeaxiom:workstation/crafting@1` |

## Lattice practice (not a Minecraft clone)

### Inspect is a typed overlay, not a HUD grab-bag

[`diagnostics-inspection-and-debug-visualization.md`](../architecture/diagnostics-inspection-and-debug-visualization.md)
already splits three surfaces. Player inspect is **Target Inspect**:

- Overlay is **always on** from the live DDA hit (`refresh_target`). Occupancy
  (`r/a/i/d`) stays on the working-set / F3 path, not on this overlay.
- F remains `PlayerActionV1::Inspect` and writes a receipt. It does not own
  the overlay.
- v1 fragments are typed fields on `HeadlessTargetInspectV1`, filled from the
  gameplay catalog and lock display labels. Keys match the architecture doc:

  | Fragment key | Source | Player overlay |
  | --- | --- | --- |
  | `header.name` | lock display label / missing-presentation name | yes |
  | `header.icon` | lock display icon identity (color swatch until PNG) | yes |
  | `summary.harvestability` | `BlockDefinitionV1.mining` (tool class, min tier, hardness ticks) | yes |
  | `technical.declared-by` | package namespace of `BlockId` (`terrenia` from `terrenia:block/oak-log`) | yes |
  | `technical.stable-id` | `BlockId` text | yes, compact |
  | occupancy / working-set | runtime diagnostics | **no** (dev-tools) |

- One primary owner per key. v1 owner is the host composing catalog + lock
  data. Later `@latticeaxiom/inspect` registers `InspectFragmentProvider`
  packages; do not invent a Java-style plugin bus now.
- Packages return typed values. The host formats, wraps, and localizes.
  No rich-text blobs, no screen coordinates from content.

Jade mapping (absorb / reject):

| Jade | Absorb | Reject |
| --- | --- | --- |
| Object name on the aimed block | yes, `header.name` | `IBlockComponentProvider`, `ITooltip` |
| Harvest tool | yes, from `MiningRuleV1` | scanning Minecraft tags / `HarvestToolProvider` |
| Mod name | yes, as package namespace `declared-by` | Forge/Fabric modid lookup |
| Registry name | yes, as `BlockId` | dumping NBT / blockstate properties in v1 |
| Energy / fluid / item storage | later, via fragment keys | universal capability adapters |

### Pick-block is an action, not creative give

`PlayerActionV1::PickBlock = 9`, Leafwing middle mouse. Authority:

1. Rerun or reuse the live DDA target (never trust a client voxel id).
2. Find an inventory stack whose `ItemDefinitionV1.placement_block` equals
   the aimed `BlockId`.
3. If that stack is in hotbar `0..8`, select it.
4. If it is in `9..35`, `MoveStack` it with the currently selected hotbar
   slot, then select that hotbar slot.
5. If the player does not hold a matching stack, **fail closed**. No
   spawned creative stack. First demo is catalog survival, not infinite
   give.

Do not bind pick-block to F. Do not place or break as a side effect.

### Inventory is a container, not a client array

36 slots; hotbar is the prefix. Mutations go through
`GameplayCommandV1::MoveStack` and `inventory_diff`, the same path as
craft / transfer. The overlay does not write slots.

v1 interaction is **click-to-swap** (two clicks), not a floating cursor
sprite:

- First click on slot A latches `cursor_slot = A`.
- Second click on slot B submits `MoveStack { from: A, to: B, quantity: None }`.
- Same item + state merges up to `stack_limit`; remainder stays in `from`.
- Different items swap (only when quantity is the full stack).
- Clicking the same slot clears the latch.
- Empty `from` rejects `EmptySlot`.

Bevy: inventory and hotbar slot nodes are `Button` + `Interaction` while
the overlay is open. Overlay root still ignores world picking except those
buttons. Pause and closed inventory keep `Pickable::IGNORE` on slots.

E still toggles the overlay. 1–9 still select hotbar when the overlay is
closed; while open, 1–9 still select hotbar (do not steal digits for
crafting grid coordinates).

### Crafting is a catalog consumer, not a 3×3 guessing grid

Recipes already live in `GameplayCatalog` (`recipes()` is a `BTreeMap`,
stable identity order). `ProductionGameplay::craft` already lays out
shaped inputs onto `CRAFTING_GRID_ORIGIN` and commits
`RecipeCraftCommandV1`. The client does **not** reimplement shapeless
matching.

v1 UI is a **recipe list**, because packages declare recipes. Players
click a known recipe; the kernel consumes matching stacks. This is the
Lattice path (catalog → command). A free-form 3×3 discovery grid can
come later as another surface on the same command; it is not required
for first playable.

Surfaces:

| Surface | Key | Recipes shown |
| --- | --- | --- |
| Inventory (E) | hand crafting | `workstation == None` and inputs currently owned |
| Workbench | C, or `SurfaceActivate` while the live target block binds `latticeaxiom:workstation/crafting@1` | recipes that require that workstation; bind the container first via existing `bind_workstation` |

Clicking a recipe calls `ProductionSpine::craft_recipe`. Failure stays
`GameplayReject` (mismatch, full inventory, unbound workstation). The
list is rebuilt from `catalog.recipes()` each sync; no host-hardcoded
Terrenia recipe ids.

Do not open the workbench by stealing right-click from `PlaceBlock`.

## Command and action additions

### `PlayerActionV1::PickBlock = 9`

Numeric discriminants stay explicit. `PlayerActionButtonsV1` bit
`1 << 6`. Exhaustive matches in `action.rs`, Leafwing adapter, and the
playable keyboard/mouse adapter (`MouseButton::Middle`) must include it.

### `GameplayCommandV1::MoveStack(MoveStackCommandV1)`

```text
player, from, to, quantity: Option<NonZeroU32>, expected_inventory_revision
```

`quantity == None` means the entire `from` stack. Outcome
`CommandOutcomeV1::StackMoved { from, to }`. Planner uses the same
`inventory_diff` as transfer. Stale inventory revision fails closed.

Host helpers (no new thread runtime):

- `ProductionGameplay::move_stack(from, to)`
- `ProductionSpine::move_stack` / `pick_aimed_block` / `craftable_recipe_ids(workstation)`
- `inspect_from_eye` fills harvest + declared-by from catalog + `BlockId`

## Tests (headless, no window)

- `HeadlessTargetInspectV1` overlay lines include name, harvest, declared-by,
  and stable id; they do **not** include occupancy.
- Pick-block selects a hotbar stack that places the aimed block; swaps a
  body-inventory stack onto the selected hotbar; rejects when the item is
  absent.
- `MoveStack` merge / swap / empty / stale revision.
- Inventory click latch is unit-tested on the surface resource (no GPU).
- Crafting: existing gather-and-craft journey still passes; add a host
  test that `craftable_recipe_ids(None)` lists oak-planks after wood is
  gathered, and workbench recipes appear only after bind.

## Out of scope

- Jade plugin API, NBT, mixins, pin-on-screen, themes.
- Creative infinite pick, JEI recipe lookup, floating dragged item mesh.
- Same-App title + world. GregTech / TLM / Botania ids in the engine.
- Changing mesh topology, P7 renderer work, or `LaunchIntent` process spawn.

## Implementation sequence

Kernel (player + gameplay crates) can land in parallel: they do not share
files. Host HUD / spine wiring is one sequential pass after both land,
because `hud.rs` is a single module. Verify with headless tests only.
