# World storage and presentation delivery

Status: implementation in progress, 2026-09-07.

The user authorized the complete storage, water, material, vegetation, tool and
inventory work discussed in this task, including verified slice commits. The
existing ecosystem, package ownership, text-only and saved-world rules apply.

## Delivery slices

1. Persist generated and edited chunks incrementally, bound all resident copies
   and write queues, autosave during exploration, and hydrate existing chunks.
2. Constrain generated river/lake water to beds, banks and downstream outlets;
   preserve connected surface meshing and coherent near/far water presentation.
3. Resolve materials per face, repeat textures at voxel scale, distinguish soil
   families, and provide actual leaf cutout coverage.
4. Share procedural item models/materials between native held items and cached
   model-rendered WebView inventory, hotbar, catalogue and recipe previews.
5. Render shaped grass/foliage with deterministic placement, bounded distance
   detail and restrained wind; validate the combined native experience.

## Acceptance

Storage: untouched explored chunks survive independent process reopen; reads do
not invoke world generation for stored chunks; long traversal keeps resident
payload and pending-write bytes bounded; failed writes retain dirty state;
multi-chunk edits and fluid continuations remain atomic. Migrations and native
acceptance use isolated copies, never the user's active world database.

Water: fixed river cross-sections exercise banks, gradients, confluences and
outlets. Surface vertices meet across chunk boundaries, internal faces are
culled, and native above/below-water and near/far views agree.

Presentation: grass top/side/bottom, soil, leaves, grass tufts and tools remain
recognizable at near and medium distances. Inventory uses the same model and
material source as held/world presentation; no two-letter item placeholders.
Resource changes cannot alter collision, selection, item identity or saves.

Run focused tests, staged repository checks and optimized measurements. Record
headless, browser and native GPU evidence separately. User screenshots may
supplement reproducible native captures; code tests do not certify appearance.

## Upstream basis

- redb 4.2.0: bounded page cache, concurrent read transactions and one atomic
  writer; physical acknowledgements follow Immediate commits.
  https://docs.rs/redb/4.2.0/redb/struct.Database.html
- Bevy 0.19.1: native assets, mesh/material extensions, render-to-texture and
  asynchronous GPU readback; WebView receives bounded cached image resources.
  https://docs.rs/bevy/latest/bevy/render/gpu_readback/enum.Readback.html
- Grid liquid state and surface geometry are distinct. Shared liquid surfaces
  cull internal faces and represent partial heights.
  https://api.luanti.org/nodes/

## Evidence

Pending implementation and validation. Earlier repository acceptance remains
historical evidence and does not establish these new gates.
