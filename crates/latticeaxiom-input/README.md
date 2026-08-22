# latticeaxiom-input

Headless-first owner of the ADR 0033 input foundation:

- stable `ActionSpecV1` / `ClientSurfaceActionV1` / player-action mappings
- versioned `InputBindingV1` and `BindingProfileV1`
- exactly-one `latticeaxiom:capability/input-actions@1` provider selection
- deterministic catalog compile, same-context conflicts, and dense indexes
- Leafwing-facing compiled gameplay, surface, and HUD-overlay maps
- `ActiveInputContextStack` with independent capture vs gameplay-suppression
  policies, pressed-state generation, and Esc safety fallback
- headless logical-input injection that produces the same action IDs a client
  adapter must emit

This crate does not read Bevy `ButtonInput`, does not install Leafwing, and
does not `include_str!` a production catalog. Production hosts must compile the
catalog bytes selected by a reopened product lock.

Shipped package data lives at
`packages/latticeaxiom/input/data/action-catalog-v1.json`. Tests load that file
from disk; host crates must not treat the source path as a hidden fallback.

## Integration (not done in this change)

Do not wire this crate until the following host/lock work lands. This crate is
intentionally not a root workspace member yet.

1. Root `Cargo.toml`: add `crates/latticeaxiom-input` to `[workspace.members]`.
   Remove the nested `[workspace]` table from this crate manifest and switch
   package/lints/dependency versions to workspace inheritance.
2. Profiles (`profiles/dev.toml`, `headless.toml`, `shell.toml`, `test.toml`
   and matching `.ncl`): add `@latticeaxiom/input` as a graph root / source so
   the lock selects exactly one `input-actions@1` provider. Do not keep a host
   default catalog.
3. `latticeaxiom-player`: replace `default_leafwing_input_map()` with maps
   compiled here. Keep `PlayerActionV1` / `PlayerActionFrameV1` unchanged.
   Discriminants in `AuthoritativePlayerActionV1` match the player crate
   (`Move=1` … `Pause=7`, `PickBlock=9`; `SurfaceActivate=8` stays player-only).
4. Engine HUD/shell: delete business-action `ButtonInput<KeyCode>` paths.
   Consume `ClientSurfaceActionV1` and the HUD overlay allowlist maps.
5. Context adapter: `ActiveInputContextStack` is the only caller of
   `ActionFrameInbox::suppress_live_gameplay()`. Pause/inventory/workbench must
   not keep private suppression latches.
6. Settings: persist `BindingProfileV1` through user-scope canonical atomic
   replace owned by `@latticeaxiom/settings`. On parse failure keep the original
   file, load package defaults, and surface the typed `InputError`.
7. Registration: compile the package catalog from the reopened lock. Fail
   before App start on missing/duplicate providers, catalog/enum mismatch, or
   unknown required binding/profile/catalog majors.

Focused tests, until step 1:

```text
cargo test --manifest-path crates/latticeaxiom-input/Cargo.toml
cargo clippy --manifest-path crates/latticeaxiom-input/Cargo.toml --all-targets -- -D warnings
```
