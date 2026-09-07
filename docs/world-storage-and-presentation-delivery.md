# World storage and presentation delivery

Status: implementation complete; remaining manual acceptance transferred to the
user at their request on 2026-09-07. Further automated tests and the final
optimized capture build were stopped.

The user authorized the complete storage, water, material, vegetation, tool and
inventory work discussed in this task, including verified slice commits. The
existing ecosystem, package ownership, text-only and saved-world rules apply.

## Delivery slices

1. Persist generated and edited chunks incrementally, bound all resident copies
   and write queues, autosave during exploration, and hydrate existing chunks.
2. Generate connected channel footprints, constrain their initial water to final
   banks, and share water levels between near and far presentation.
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

Water: fixed cross-sections exercise final banks and shared near/far levels.
Surface vertices meet across chunk boundaries and internal faces are culled.
Above/below-water appearance and broader terrain cases remain manual acceptance.

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

The indexed backend is committed in `4b96cd8`. Its focused storage/world-db
suite passed 60 tests and strict Clippy. Independent physical reopen, checkpoint
verification, corruption rejection, snapshot isolation and legacy-image
preservation are covered by that suite. The page cache is limited to 64 MiB;
normal open/read paths do not materialize the world-wide record map.

The production disk-world acceptance test passed creation, pristine-chunk
publication, independent reopen, inventory/pose restoration, original-lock
selection and gameplay after saving. Eight locations spaced 256 m apart exercise
incremental saving and eviction: pending writes drain, kernel residency stays
within the host ceiling, and visited terrain exceeds retained kernel copies.
Client integration is committed in `4fba868`.

Client writeback captures player state and the complete edited set under the
same spine lock, then releases it before physical I/O. One background task writes
at most 32 chunks per batch; gameplay checks this atomic-write capacity before
mutation, and fluid simulation retains its continuation when capacity is full.
Unmodified newly generated chunks fill remaining batch space. Exact revision
acknowledgements release persistence pins; eviction then releases the kernel
payload and loaded gameplay observations. A failed write retains those pins.

The new channel policy is explicit in new game locks. It samples final dry-bank
support with bounded cross-bank probes and shares water levels between near and
far terrain. It does not activate the separate finite, global hydrologic-plan
implementation. Existing locks retain the previous generation policy.
This slice is committed in `ca1c153`; its 11 hydrology tests and strict
all-target Clippy passed. The legacy valley-distance scale is preserved for
old policy inputs, while the connected channel policy uses its own distance.

Browser fixture checks have passed inventory movement, catalogue filtering,
keyboard interaction and desktop/compact layout with no browser errors. The
engine's 186 library tests, 7 gameplay library tests, 8 WebView library tests and
4 resource-contract tests passed. The existing 87 worldgen library tests also
passed before the final channel-distance refinement; the 11 hydrology tests
cover that final refinement.

Actual native WebView inspection decoded 79 model-image instances successfully.
The native renderer exported 78 unique item previews. Grass blocks, leaf cutout,
the stone pickaxe and grass-tuft previews were inspected. GPU readback verified
all 102 base texture-array layers against the CPU atlas with zero differing
bytes. Scene capture then identified a separate interpolated-float ID bug:
truncation could select the previous material. Color and depth shaders now round
the ID, and a subsequent native diagnostic capture showed the stripes and wrong
leaf materials removed.

The corrected scene capture used an engine `opt-level=0` diagnostic build, so it
does not establish final optimized performance. One diagnostic run timed out
after inventory was opened while previews were still being generated. Its cause
is unconfirmed; final optimized reproduction was not completed before the user
took over testing. This remains an explicit manual acceptance item.

## Manual handoff

Run `task dev` from the repository to rebuild the manifest-selected native product
and refresh its game, shell and independent resource locks. Use a newly created
world to evaluate the new water-generation policy. Existing worlds keep their
original frozen generation rules.

Prioritize opening inventory immediately after world entry: models should keep
appearing, Escape should close the panel, and gameplay should resume. Also check
the hotbar, recipe previews, grass top/side/bottom, leaves, grass tufts and water
banks at near/far distances. The final optimized native capture and new-world
visual sweep are intentionally left to the user.
