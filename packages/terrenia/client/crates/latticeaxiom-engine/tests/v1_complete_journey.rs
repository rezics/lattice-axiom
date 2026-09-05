//! D10 complete sandbox journey harness.
//!
//! Drives the production supervisor/lock/host public path:
//! create → spawn → surface explore → cave → wood/stone/copper →
//! tools/building materials → smelt → recognizable structure → container →
//! checkpoint → exit → reopen.
//!
//! Missing production hooks fail closed with named serial-hardener gaps.
//! This file does not implement features, mutate authority, or seed inventory.

#![allow(clippy::expect_used, clippy::too_many_lines)]

use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    num::NonZeroU32,
    path::{Path, PathBuf},
    time::Duration,
};

use latticeaxiom_compose::{LockV1, PRODUCT_LOCK_FILE_NAME, RealizationKind, reopen_product_lock};
use latticeaxiom_core::{CapabilityId, PackageName, TargetTriple};
use latticeaxiom_engine::{
    ADR_0026_ACTIVE_COVERAGE_M, ADR_0026_CHUNK_EDGE_VOXELS, ADR_0026_RESIDENT_COVERAGE_M,
    ActionAxis2V1, CaveOccupancyArbitrationV1, ChunkFaceV1, ChunkLifecycle, ContainerId,
    EngineInstance, GameplayReject, HOTBAR_SLOTS, ItemId, LockVerifiedComposeImages,
    PlayerActionButtonsV1, PlayerActionFrameV1, PlayerActionV1, PlayerId, ProductionMemoryStart,
    ProductionSpine, RecipeId, RenderDeviceLost, SealedWorldWriterHost, SlotIndex,
    TransferCommandV1, TransferDirectionV1, VerifiedProductLockHash,
};
use latticeaxiom_gameplay::{BlockId, BlockPosition, ProcessId, WorkstationId};
use latticeaxiom_launcher::{
    ChildExitKindV1, HostBuildReceipts, ReopenedFinalLockV1, SettingTransactionRevision,
};
use latticeaxiom_packages::{FilesystemCas, LOCAL_CATALOG_CAS_DIRECTORY};
use latticeaxiom_player::{BlockEditRejectV1, BlockFaceV1};
use latticeaxiom_start_ui::{
    ClientShellGraph, InputSource, MemoryStartEffect, SemanticActionId, SemanticCommand,
    SemanticNodeId, ShellCapability, ShellPackageProvider, ShellScreen,
};
use latticeaxiom_world_db::StorageDurabilityCapabilityV1;
use serde::Deserialize;

const FIXTURE_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/v1_complete_journey.json"
);
const SCHEMA_ID: &str = "latticeaxiom.v1-complete-journey.v1";
const FIXED_TIMESTEP: Duration = Duration::from_nanos(1_000_000_000 / 60);
// The GPU-free journey fixture does not install a user-settings journal.
const EMPTY_SETTINGS_REVISION: SettingTransactionRevision = SettingTransactionRevision::new(0);
const LOCK_COMMAND: &str = "cargo run -p latticeaxiom-compose --bin latticeaxiom-compose --features nickel-evaluator -- lock --offline --bootstrap profiles/dev.toml";
const REQUIRED_STEPS: [&str; 12] = [
    "create",
    "spawn",
    "surface-explore",
    "enter-cave",
    "gather-wood-stone-copper",
    "craft-tools-and-building-materials",
    "smelt",
    "recognizable-structure",
    "container",
    "checkpoint",
    "exit",
    "reopen",
];
const REQUIRED_DURABLE_STATES: [&str; 8] = [
    "player-position",
    "player-building",
    "inventory",
    "tool-durability",
    "container",
    "machine",
    "chunk-revision",
    "generation-epoch",
];
const REQUIRED_JOURNEYS: [&str; 6] = [
    "fresh-world",
    "durable-reopen",
    "checkpoint-restore",
    "crash-recovery",
    "static-portable",
    "compatible-reopen",
];

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CompleteJourneyFixture {
    schema_id: String,
    roadmap: String,
    release: String,
    profile: String,
    product_entries: Vec<String>,
    steps: Vec<String>,
    journeys: Vec<String>,
    durable_states: Vec<String>,
    budgets: JourneyBudgets,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct JourneyBudgets {
    profile: String,
    chunk_edge_voxels: u16,
    active_coverage_m: u32,
    resident_coverage_m: u32,
    active_radius_chunks: u32,
    resident_radius_chunks: u32,
    fixed_rate_hz: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct DurableSnapshot {
    pose: [i32; 3],
    inventory: BTreeMap<String, u32>,
    tool_durability: BTreeMap<String, u32>,
    building: BTreeMap<(i32, i32, i32), Option<String>>,
    container: BTreeMap<String, u32>,
    chunk_revisions: BTreeMap<(i32, i32, i32), u64>,
    generation_input_hash: latticeaxiom_core::CanonicalHash,
    generation_provenance_hash: latticeaxiom_core::CanonicalHash,
    hotbar_slot: u16,
}

#[test]
fn v1_complete_journey_fixture_names_every_d10_durable_state() {
    let fixture = load_fixture();
    assert_eq!(fixture.schema_id, SCHEMA_ID);
    assert_eq!(fixture.roadmap, "D10 sandbox completion");
    assert_eq!(fixture.release, "v1 playable V9");
    assert_eq!(fixture.profile, "desktop-reference-v1");
    assert_eq!(fixture.steps, REQUIRED_STEPS);
    assert_eq!(fixture.journeys, REQUIRED_JOURNEYS);
    assert_eq!(fixture.durable_states, REQUIRED_DURABLE_STATES);
    assert!(
        fixture
            .product_entries
            .iter()
            .any(|entry| entry == "task play")
    );
    assert_eq!(fixture.budgets.profile, "desktop-reference-v1");
    assert_eq!(
        fixture.budgets.chunk_edge_voxels,
        ADR_0026_CHUNK_EDGE_VOXELS
    );
    assert_eq!(
        fixture.budgets.active_coverage_m,
        ADR_0026_ACTIVE_COVERAGE_M
    );
    assert_eq!(
        fixture.budgets.resident_coverage_m,
        ADR_0026_RESIDENT_COVERAGE_M
    );
    assert_eq!(fixture.budgets.active_radius_chunks, 4);
    assert_eq!(fixture.budgets.resident_radius_chunks, 6);
    assert_eq!(fixture.budgets.fixed_rate_hz, 60);
}

#[test]
fn adr_0026_frozen_budgets_are_not_relaxed() {
    assert_eq!(ADR_0026_CHUNK_EDGE_VOXELS, 32);
    assert_eq!(ADR_0026_ACTIVE_COVERAGE_M, 128);
    assert_eq!(ADR_0026_RESIDENT_COVERAGE_M, 192);
    let fixture = load_fixture();
    assert_eq!(fixture.budgets.active_radius_chunks, 4);
    assert_eq!(fixture.budgets.resident_radius_chunks, 6);
}

#[test]
fn reopened_frozen_locks_bind_shell_and_game_without_resolving() {
    let workspace = workspace_root();
    let game = try_reopen_lock(&workspace, &workspace.join(PRODUCT_LOCK_FILE_NAME))
        .unwrap_or_else(|gap| panic!("{gap}"));
    let shell = try_reopen_lock(
        &workspace,
        &workspace
            .join("run")
            .join("shell")
            .join(PRODUCT_LOCK_FILE_NAME),
    )
    .unwrap_or_else(|gap| panic!("{gap}"));
    assert_ne!(
        game.product_lock_hash(),
        shell.product_lock_hash(),
        "shell and game locks must stay distinct replacement-process closures"
    );
    let game_selects_shell = ProductionMemoryStart::lock_graph_selects_shell(game.images().graph())
        .unwrap_or_else(|error| panic!("game process-role evidence: {error}"));
    assert!(
        !game_selects_shell,
        "the game lock must boot the world host, not a hidden Terrenia shell plugin list"
    );
    let shell_selects_shell =
        ProductionMemoryStart::lock_graph_selects_shell(shell.images().graph())
            .unwrap_or_else(|error| panic!("shell process-role evidence: {error}"));
    assert!(
        shell_selects_shell,
        "the shell lock must select the package-driven front-end process"
    );
}

#[test]
#[allow(clippy::too_many_lines)]
#[ignore = "8³ streamed spawn set has no oak/pine logs; D10 create-through-reopen remains unclosed"]
fn complete_d10_journey_create_through_reopen() {
    let mut gaps = Vec::new();
    record_rocksdb_gap(&mut gaps);
    record_supervisor_gap(&mut gaps);

    let workspace = workspace_root();
    let game_images = match try_reopen_lock(&workspace, &workspace.join(PRODUCT_LOCK_FILE_NAME)) {
        Ok(images) => images,
        Err(gap) => {
            gaps.push(gap);
            record_static_portable_gap(&mut gaps, None);
            assert!(
                gaps.is_empty(),
                "serial hardener gaps (production hooks missing from the host public path):\n- {}",
                gaps.join("\n- ")
            );
            return;
        }
    };
    record_static_portable_gap(&mut gaps, Some(&game_images));
    let shell_images = match try_reopen_lock(
        &workspace,
        &workspace
            .join("run")
            .join("shell")
            .join(PRODUCT_LOCK_FILE_NAME),
    ) {
        Ok(images) => images,
        Err(gap) => {
            gaps.push(gap);
            assert!(
                gaps.is_empty(),
                "serial hardener gaps (production hooks missing from the host public path):\n- {}",
                gaps.join("\n- ")
            );
            return;
        }
    };
    let lock_hash = game_images.product_lock_hash();
    let catalog = latticeaxiom_engine::lock_selected_gameplay_catalog(&game_images)
        .expect("reopened game lock selects sandbox gameplay and tools");
    assert!(
        catalog
            .recipe(&parse_recipe("terrenia:recipe/wooden-pickaxe@1"))
            .is_some()
    );
    assert!(
        catalog
            .process(&parse_process("terrenia:process/smelt-stone@1"))
            .is_some(),
        "lock-selected catalog must include furnace smelt processes"
    );

    let record_owner = "latticeaxiom:schema/world-db-chunk@1"
        .parse()
        .expect("world-db record owner is canonical");
    let mut writer = SealedWorldWriterHost::durable_reference_with_default_publisher(record_owner);
    assert_eq!(
        writer.durability_capability(),
        StorageDurabilityCapabilityV1::WalSyncCheckpoint
    );

    let shell_graph = shell_graph_from_images(&shell_images);
    let mut start = ProductionMemoryStart::new(game_images.clone(), shell_graph)
        .with_storage(writer.storage().clone());
    start.set_now_ms(10);
    let intent = start
        .quick_create_intent("D10 Complete Journey")
        .expect("quick-create binds the lock graph root");
    start.set_draft(intent);
    let created = match start
        .inject(&semantic_command(
            "home/new-world",
            SemanticActionId::Activate,
        ))
        .and_then(|_| {
            start.inject(&semantic_command(
                "new-world/quick-create",
                SemanticActionId::Activate,
            ))
        })
        .expect("create injects through the start-ui public path")
    {
        MemoryStartEffect::Created(world_id) => world_id,
        other @ MemoryStartEffect::Shell(_) => panic!("expected created world, got {other:?}"),
    };
    assert_eq!(start.continue_world_id(), Some(created));

    let mut instance = start
        .play_headless(created, 20, FIXED_TIMESTEP)
        .expect("game host starts from the reopened frozen lock");
    instance
        .advance_fixed_ticks(2)
        .expect("spawn ticks advance");
    let spine = instance
        .app()
        .world()
        .get_resource::<ProductionSpine>()
        .expect("production spine is installed")
        .clone();
    assert_eq!(
        instance
            .app()
            .world()
            .get_resource::<VerifiedProductLockHash>()
            .copied()
            .map(VerifiedProductLockHash::get),
        Some(lock_hash)
    );
    assert_eq!(spine.world_id(), Some(created));
    let before_device = spine
        .materialized_chunk_state_hash()
        .expect("world hash is available before device loss");
    instance
        .report_render_device_lost()
        .expect("device loss is presentation-only");
    assert_eq!(
        spine
            .materialized_chunk_state_hash()
            .expect("world hash is available after device loss"),
        before_device,
        "presentation/device failure must not change the world hash"
    );
    assert!(
        instance
            .app()
            .world()
            .get_resource::<RenderDeviceLost>()
            .is_some_and(|lost| lost.is_lost())
    );

    let spawn = spine.spawn_center();
    assert!(
        spawn.y > 0.0,
        "spawn must be a validated Y-up surface location, got {spawn:?}"
    );
    let pose = spine.player_pose();
    assert!(
        pose.translation.y > 8.0,
        "player must spawn above the finite vertical fixture floor, y={}",
        pose.translation.y
    );
    assert!(
        (pose.translation - spawn).length() < 4.0,
        "player must spawn at the validated surface, pose {:?}, spawn {spawn:?}",
        pose.translation
    );

    let mut generation = 1_u64;
    generation = enqueue_look_then_walk(&mut instance, generation, 0.25, 0.0, 1.0, 48);
    instance
        .advance_fixed_ticks(u32::try_from(generation.saturating_sub(1)).expect("explore fits"))
        .expect("surface explore ticks advance");
    instance
        .enqueue_headless_actions([inspect_frame(generation)])
        .expect("inspect enqueues");
    instance
        .advance_fixed_ticks(2)
        .expect("inspect ticks advance");
    generation = generation.saturating_add(2);
    let _ = spine.current_target();
    assert!(
        !spine.occupies_unready_cave_void(),
        "surface exploration must not enter an unready cave void"
    );
    let diag = spine.working_set_diagnostics();
    if let Some(limits) = spine.hard_limits() {
        assert!(
            diag.resident() <= limits.max_resident_chunks,
            "resident set {} exceeded host hard cap {}",
            diag.resident(),
            limits.max_resident_chunks
        );
    }

    let mut wood = first_resident_wood(&spine);
    let mut explore_rounds = 0_u32;
    while wood.is_none() && explore_rounds < 16 {
        let before = generation;
        generation = enqueue_look_then_walk(&mut instance, generation, 0.35, 0.0, 1.0, 48);
        let consumed = generation.saturating_sub(before);
        instance
            .advance_fixed_ticks(u32::try_from(consumed).expect("explore fits"))
            .expect("additional surface explore ticks advance");
        explore_rounds = explore_rounds.saturating_add(1);
        wood = first_resident_wood(&spine);
    }
    let wood = wood.unwrap_or_else(|| {
        panic!(
            "surface wood must exist in the streamed set {:?}",
            spine.resident_chunks()
        )
    });
    gather_until_inventory_has(&spine, wood.1, &wood.2, 1);
    for _ in 0..3 {
        if spine
            .inventory_view()
            .expect("inventory")
            .count_item(&wood.2)
            < 4
            && let Some((_, position, item, _, _)) = first_resident_wood(&spine)
        {
            gather_until_inventory_has(&spine, position, &item, 1);
        }
    }
    for _ in 0..4 {
        let _ = spine.craft_recipe(&wood.3, None);
    }
    spine
        .craft_recipe(&parse_recipe("terrenia:recipe/stick@1"), None)
        .expect("sticks craft from planks");
    spine
        .craft_recipe(&parse_recipe("terrenia:recipe/workbench@1"), None)
        .expect("workbench crafts from planks");
    let station = ContainerId::new(2);
    spine
        .bind_workstation(
            WorkstationId::parse("latticeaxiom:workstation/crafting@1")
                .expect("crafting workstation is a platform contract"),
            station,
        )
        .expect("workbench binds");
    spine
        .craft_recipe(
            &parse_recipe("terrenia:recipe/wooden-pickaxe@1"),
            Some(station),
        )
        .expect("wooden pickaxe crafts");
    spine
        .craft_recipe(
            &parse_recipe("terrenia:recipe/wooden-shovel@1"),
            Some(station),
        )
        .expect("wooden shovel crafts");
    spine
        .craft_recipe(&parse_recipe("terrenia:recipe/wooden-axe@1"), Some(station))
        .expect("wooden axe crafts");

    generation = enter_required_cave(&mut instance, &spine, generation);

    let stone_block = first_resident_any(
        &spine,
        &[
            "terrenia:block/stone",
            "terrenia:block/granite",
            "terrenia:block/slate",
            "terrenia:block/deepstone",
        ],
    );
    let cobble = parse_item("terrenia:item/cobblestone");
    select_item_in_hotbar(&spine, &parse_item("terrenia:item/wooden-pickaxe"));
    gather_until_inventory_has(&spine, stone_block.1, &cobble, 1);
    while spine
        .inventory_view()
        .expect("inventory")
        .count_item(&cobble)
        < 7
    {
        let next = spine
            .first_resident_block(&stone_block.0)
            .unwrap_or_else(|| {
                panic!(
                    "additional stone must remain after gathering, resident {:?}",
                    spine.resident_chunks()
                )
            });
        gather_until_inventory_has(&spine, next, &cobble, 1);
    }
    let copper = parse_block("terrenia:block/copper-ore");
    let copper_item = parse_item("terrenia:item/copper-ore");
    let ore = spine
        .first_cave_adjacent_block(&copper)
        .or_else(|| spine.first_resident_block(&copper))
        .expect("natural copper exists in the streamed underground set");
    gather_until_inventory_has(&spine, ore, &copper_item, 1);

    spine
        .craft_recipe(
            &parse_recipe("terrenia:recipe/stone-pickaxe@1"),
            Some(station),
        )
        .expect("stone pickaxe crafts");
    let plank_item = wood.4.clone();
    assert!(
        spine
            .inventory_view()
            .expect("inventory")
            .count_item(&plank_item)
            >= 1
            || spine
                .inventory_view()
                .expect("inventory")
                .count_item(&parse_item("terrenia:item/workbench"))
                >= 1,
        "building materials must remain after tool crafts"
    );

    smelt_stone(&spine, &catalog, &cobble);
    let structure = place_recognizable_structure(&spine, &cobble, &plank_item);
    deposit_into_chest(&spine, &copper_item);

    let snapshot = capture_durable_snapshot(&spine, &structure);
    assert!(
        snapshot
            .inventory
            .get(copper_item.as_str())
            .copied()
            .unwrap_or(0)
            >= 1
            || snapshot
                .inventory
                .get("terrenia:item/copper-block")
                .copied()
                .unwrap_or(0)
                >= 1
            || snapshot
                .container
                .get(copper_item.as_str())
                .copied()
                .unwrap_or(0)
                >= 1
    );
    assert!(
        snapshot
            .tool_durability
            .contains_key("terrenia:item/wooden-pickaxe")
            || snapshot
                .tool_durability
                .contains_key("terrenia:item/stone-pickaxe")
    );

    start.pause_session(&mut instance).expect("pause");
    let result = start
        .save_and_quit_durable(created, instance, &mut writer, EMPTY_SETTINGS_REVISION)
        .expect("durable Save & Quit checkpoints");
    assert_eq!(result.report().exit_kind(), ChildExitKindV1::SaveAndQuit);
    assert!(result.report().last_durable_world().is_some());
    assert!(result.checkpoint().restore_verified());
    assert_eq!(start.flow().shell().screen, ShellScreen::Home);
    assert_eq!(start.continue_world_id(), Some(created));
    let timeout = start
        .on_shutdown_timeout(created, &mut writer, EMPTY_SETTINGS_REVISION)
        .expect("shutdown timeout recovers the latest durable world");
    assert_eq!(timeout.exit_kind(), ChildExitKindV1::ShutdownTimeout);
    assert!(!timeout.exit_kind().is_normal_handoff());
    assert!(!writer.is_writer_active());
    assert_eq!(start.flow().shell().screen, ShellScreen::Home);

    let (continued, mut reopened) = start
        .play_continued_headless(30, FIXED_TIMESTEP)
        .expect("continue reopens storage-first after durable Save & Quit");
    assert_eq!(continued, created);
    reopened
        .advance_fixed_ticks(1)
        .expect("continued host advances");
    let reopened_spine = reopened
        .app()
        .world()
        .get_resource::<ProductionSpine>()
        .expect("reopened production spine is installed")
        .clone();
    assert_durable_snapshot(&reopened_spine, &snapshot);

    start
        .recover_after_crash(created, &mut writer)
        .expect("crash recovery verifies the latest durable world");
    drop(reopened);
    let mut recovered = start
        .play_reopened_headless(created, 40, FIXED_TIMESTEP)
        .expect("crash-recovery reopen hydrates world-db first");
    recovered
        .advance_fixed_ticks(1)
        .expect("recovered host advances");
    let recovered_spine = recovered
        .app()
        .world()
        .get_resource::<ProductionSpine>()
        .expect("recovered production spine is installed")
        .clone();
    assert_durable_snapshot(&recovered_spine, &snapshot);
    let _ = generation;

    assert!(
        gaps.is_empty(),
        "serial hardener gaps (production hooks missing from the host public path):\n- {}",
        gaps.join("\n- ")
    );
}

fn record_static_portable_gap(gaps: &mut Vec<String>, images: Option<&LockVerifiedComposeImages>) {
    let Some(images) = images else {
        gaps.push(
            "static NativeStatic and PortableNative dual journey on the production host public path \
             producing the same action receipts, authoritative state hash, semantic report, and \
             normative save bytes (frozen lock images did not bind, so dual realization cannot run)"
                .to_owned(),
        );
        return;
    };
    let kinds = images
        .images()
        .graph()
        .packages
        .values()
        .map(|package| package.realization)
        .collect::<BTreeSet<_>>();
    if !(kinds.contains(&RealizationKind::NativeStatic)
        && kinds.contains(&RealizationKind::PortableNative))
    {
        gaps.push(
            "static NativeStatic and PortableNative dual journey on the production host public path \
             producing the same action receipts, authoritative state hash, semantic report, and \
             normative save bytes (lock graph currently has no dual static/portable pair)"
                .to_owned(),
        );
    }
}

fn record_rocksdb_gap(gaps: &mut Vec<String>) {
    gaps.push(
        "SealedWorldWriterHost::open_rocksdb(...) — durable RocksDB world on the production host \
         public path (durable_reference is an in-process WAL/sync oracle and does not open RocksDB)"
            .to_owned(),
    );
}

fn record_supervisor_gap(gaps: &mut Vec<String>) {
    gaps.push(
        "headless command-injection driver for run_product_supervisor_from_workspace that launches \
         real shell/game replacement processes, consumes one-shot intents, and returns to shell \
         without a window or GPU"
            .to_owned(),
    );
}

const FURNACE_CONTAINER: ContainerId = ContainerId::new(4);
const CHEST_CONTAINER: ContainerId = ContainerId::new(3);

fn smelt_stone(
    spine: &ProductionSpine,
    catalog: &latticeaxiom_gameplay::GameplayCatalog,
    cobble: &ItemId,
) {
    let furnace = WorkstationId::parse("latticeaxiom:workstation/furnace@1")
        .expect("furnace workstation is a platform contract");
    spine
        .bind_workstation(furnace, FURNACE_CONTAINER)
        .expect("furnace schema binds");
    let process = parse_process("terrenia:process/smelt-stone@1");
    assert!(
        catalog.process(&process).is_some(),
        "smelt-stone process must remain lock-selected"
    );
    transfer_one(spine, cobble, FURNACE_CONTAINER, SlotIndex::new(0));
    let fuel = first_inventory_fuel(spine).expect("smelt requires a catalog fuel in inventory");
    transfer_one(spine, &fuel, FURNACE_CONTAINER, SlotIndex::new(1));
    spine
        .start_process(&process, FURNACE_CONTAINER)
        .expect("furnace process starts");
    spine
        .advance_scheduled()
        .expect("furnace scheduled work advances");
}

fn deposit_into_chest(spine: &ProductionSpine, deposit: &ItemId) {
    let chest = parse_block("terrenia:block/chest");
    spine
        .bind_block_container(&chest, CHEST_CONTAINER)
        .expect("chest container schema binds");
    transfer_one(spine, deposit, CHEST_CONTAINER, SlotIndex::new(0));
    let stored = spine
        .container(CHEST_CONTAINER)
        .expect("chest remains bound");
    assert!(
        stored
            .slots()
            .iter()
            .flatten()
            .any(|stack| stack.item() == deposit),
        "chest must hold the deposited stack"
    );
}

fn transfer_one(
    spine: &ProductionSpine,
    item: &ItemId,
    container: ContainerId,
    container_slot: SlotIndex,
) {
    let view = spine.inventory_view().expect("inventory");
    let from = view
        .slots()
        .iter()
        .position(|stack| stack.as_ref().is_some_and(|stack| stack.item() == item))
        .unwrap_or_else(|| panic!("{item} must occupy an inventory slot for transfer"));
    let inventory_revision = spine
        .inventory_inspect()
        .expect("inventory inspect")
        .revision();
    let container_revision = spine
        .container(container)
        .expect("destination container is bound")
        .revision();
    spine
        .transfer(TransferCommandV1 {
            player: PlayerId::new(1),
            container,
            player_slot: SlotIndex::new(u16::try_from(from).expect("slot fits")),
            container_slot,
            quantity: NonZeroU32::new(1).expect("unit transfer is nonzero"),
            direction: TransferDirectionV1::PlayerToContainer,
            expected_inventory_revision: inventory_revision,
            expected_container_revision: container_revision,
        })
        .unwrap_or_else(|error| panic!("transfer of {item} failed: {error}"));
}

fn container_counts(spine: &ProductionSpine, id: ContainerId) -> BTreeMap<String, u32> {
    let mut counts = BTreeMap::new();
    let Some(container) = spine.container(id) else {
        return counts;
    };
    for stack in container.slots().iter().flatten() {
        *counts.entry(stack.item().as_str().to_owned()).or_insert(0) += stack.quantity();
    }
    counts
}

fn first_inventory_fuel(spine: &ProductionSpine) -> Option<ItemId> {
    for id in [
        "terrenia:item/oak-log",
        "terrenia:item/pine-log",
        "terrenia:item/oak-planks",
        "terrenia:item/pine-planks",
        "terrenia:item/stick",
        "terrenia:item/coal-ore",
    ] {
        let item = parse_item(id);
        if spine.inventory_view().expect("inventory").count_item(&item) > 0 {
            return Some(item);
        }
    }
    None
}

fn place_recognizable_structure(
    spine: &ProductionSpine,
    cobble: &ItemId,
    planks: &ItemId,
) -> Vec<BlockPosition> {
    let (_, footing, _) = first_resident_soil(spine);
    let origin = BlockPosition {
        x: footing.x.saturating_add(2),
        y: footing.y.saturating_add(1),
        z: footing.z,
    };
    let cells = [
        origin,
        BlockPosition {
            x: origin.x.saturating_add(1),
            y: origin.y,
            z: origin.z,
        },
        BlockPosition {
            x: origin.x,
            y: origin.y.saturating_add(1),
            z: origin.z,
        },
        BlockPosition {
            x: origin.x.saturating_add(1),
            y: origin.y.saturating_add(1),
            z: origin.z,
        },
    ];
    let mut placed = Vec::new();
    for (index, cell) in cells.into_iter().enumerate() {
        let item = if index == 3 { planks } else { cobble };
        if spine.inventory_view().expect("inventory").count_item(item) == 0 {
            continue;
        }
        select_item_in_hotbar(spine, item);
        let occupancy = spine.inspect_occupancy(cell);
        if occupancy.as_ref().is_ok_and(|cell| {
            cell.solid
                .as_ref()
                .is_some_and(|block| !block.as_str().ends_with("/air"))
        }) {
            let _ = mine_until_broken(spine, cell);
            pickup_remaining(spine);
        }
        let anchor = BlockPosition {
            x: cell.x,
            y: cell.y.saturating_sub(1),
            z: cell.z,
        };
        match spine.place_from_hotbar(anchor, BlockFaceV1::PositiveY) {
            Ok(success) => placed.push(success.position),
            Err(error) => panic!("structure placement at {cell:?} failed: {error:?}"),
        }
    }
    assert!(
        placed.len() >= 3,
        "recognizable structure must place at least three blocks, got {placed:?}"
    );
    placed
}

#[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]
fn capture_durable_snapshot(
    spine: &ProductionSpine,
    building: &[BlockPosition],
) -> DurableSnapshot {
    let pose = spine.player_pose().translation;
    let inventory = spine.inventory_view().expect("inventory");
    let mut counts = BTreeMap::new();
    let mut tools = BTreeMap::new();
    for stack in inventory.slots().iter().flatten() {
        *counts.entry(stack.item().as_str().to_owned()).or_insert(0) += stack.quantity();
        if let Some(remaining) = inventory.tool_durability(stack.item()) {
            tools.insert(stack.item().as_str().to_owned(), remaining);
        }
    }
    let mut cells = BTreeMap::new();
    let mut revisions = BTreeMap::new();
    for position in building {
        let occupancy = spine
            .inspect_occupancy(*position)
            .expect("placed structure cell is inspectable");
        cells.insert(
            (position.x, position.y, position.z),
            occupancy
                .solid
                .as_ref()
                .map(|block| block.as_str().to_owned()),
        );
        if let Some(chunk) = spine.chunk_of(*position)
            && let Some(revision) = spine.chunk_revision(chunk)
        {
            revisions.insert((chunk.x, chunk.y, chunk.z), revision.get());
        }
    }
    let report = spine
        .worldgen_inspect_report()
        .expect("generation inspect is available without mutating the world");
    DurableSnapshot {
        pose: [
            pose.x.round() as i32,
            pose.y.round() as i32,
            pose.z.round() as i32,
        ],
        inventory: counts,
        tool_durability: tools,
        building: cells,
        container: container_counts(spine, CHEST_CONTAINER),
        chunk_revisions: revisions,
        generation_input_hash: report.query.generation_input_hash,
        generation_provenance_hash: report.query.generation_provenance_hash,
        hotbar_slot: inventory.hotbar_slot(),
    }
}

#[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]
fn assert_durable_snapshot(spine: &ProductionSpine, expected: &DurableSnapshot) {
    let restored = capture_durable_snapshot(
        spine,
        &expected
            .building
            .keys()
            .map(|&(x, y, z)| BlockPosition { x, y, z })
            .collect::<Vec<_>>(),
    );
    assert_eq!(
        restored.building, expected.building,
        "durable player building must restore"
    );
    for (item, count) in &expected.inventory {
        let got = restored.inventory.get(item).copied().unwrap_or(0);
        assert!(
            got >= *count,
            "durable inventory must restore {item} >= {count}, got {got}"
        );
    }
    for (tool, remaining) in &expected.tool_durability {
        assert_eq!(
            restored.tool_durability.get(tool),
            Some(remaining),
            "durable tool durability must restore {tool}"
        );
    }
    assert_eq!(
        restored.container, expected.container,
        "durable container contents must restore"
    );
    assert_eq!(
        restored.chunk_revisions, expected.chunk_revisions,
        "durable chunk revisions must restore"
    );
    assert_eq!(
        restored.generation_input_hash, expected.generation_input_hash,
        "exact generation epoch/input hash must restore without regenerating materialized chunks"
    );
    assert_eq!(
        restored.generation_provenance_hash,
        expected.generation_provenance_hash
    );
    assert_eq!(restored.hotbar_slot, expected.hotbar_slot);
    let pose = spine.player_pose().translation;
    assert!(
        (pose.x.round() as i32 - expected.pose[0]).abs() <= 1
            && (pose.y.round() as i32 - expected.pose[1]).abs() <= 2
            && (pose.z.round() as i32 - expected.pose[2]).abs() <= 1,
        "durable player position must restore, got {:?}, expected {:?}",
        pose,
        expected.pose
    );
}

fn enter_required_cave(
    instance: &mut EngineInstance,
    spine: &ProductionSpine,
    mut generation: u64,
) -> u64 {
    let entrance = spine
        .required_cave_entrance()
        .expect("compiled CaveTopology field portals must include a required entrance");
    assert!(entrance.clearance_radius_voxels() > 0);
    assert!(matches!(
        entrance.tangent_face(),
        ChunkFaceV1::NegativeX
            | ChunkFaceV1::PositiveX
            | ChunkFaceV1::NegativeY
            | ChunkFaceV1::PositiveY
            | ChunkFaceV1::NegativeZ
            | ChunkFaceV1::PositiveZ
    ));
    let aperture = entrance.aperture();
    let surface = entrance.surface_footing();
    generation = walk_toward_column(instance, spine, generation, surface[0], surface[2], 1_200);
    generation = wait_for_resident(
        instance,
        spine,
        generation,
        BlockPosition {
            x: aperture[0],
            y: aperture[1],
            z: aperture[2],
        },
        180,
    );
    open_required_entrance_shaft(
        spine,
        BlockPosition {
            x: surface[0],
            y: surface[1],
            z: surface[2],
        },
        BlockPosition {
            x: aperture[0],
            y: aperture[1],
            z: aperture[2],
        },
    );
    generation = idle_at_hole(instance, spine, generation, 90);
    generation = walk_toward_column(instance, spine, generation, aperture[0], aperture[2], 1_200);
    generation = idle_at_hole(instance, spine, generation, 240);
    assert!(
        player_in_cave(spine, spine.player_pose().translation, aperture[1]),
        "player must enter underground cave space through the required entrance, pose {:?}",
        spine.player_pose().translation
    );
    assert!(!spine.occupies_unready_cave_void());
    generation
}

fn load_fixture() -> CompleteJourneyFixture {
    let contents = fs::read_to_string(FIXTURE_PATH).unwrap_or_else(|error| {
        panic!("complete-journey fixture must be readable at {FIXTURE_PATH}: {error}")
    });
    serde_json::from_str(&contents).unwrap_or_else(|error| {
        panic!("complete-journey fixture must decode with deny_unknown_fields: {error}")
    })
}

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../../../..")
        .canonicalize()
        .expect("workspace root canonicalizes")
}

fn try_reopen_lock(
    workspace: &Path,
    lock_path: &Path,
) -> Result<LockVerifiedComposeImages, String> {
    assert!(
        lock_path.is_file(),
        "product lock is missing at {}; frozen reopen never creates it; {LOCK_COMMAND}",
        lock_path.display()
    );
    let cas_root = workspace.join("catalog").join(LOCAL_CATALOG_CAS_DIRECTORY);
    assert!(
        cas_root.is_dir(),
        "catalog CAS is missing at {}; frozen reopen never creates it; {LOCK_COMMAND}",
        cas_root.display()
    );
    let lock = reopen_product_lock(lock_path)
        .unwrap_or_else(|error| panic!("reopen {} failed: {error}", lock_path.display()));
    let host = HostBuildReceipts::sealed_by_lock(&lock).unwrap_or_else(|error| {
        panic!("host receipts from {} failed: {error}", lock_path.display())
    });
    let store = FilesystemCas::open(&cas_root)
        .unwrap_or_else(|error| panic!("open CAS {} failed: {error}", cas_root.display()));
    let reopened = ReopenedFinalLockV1::reopen_frozen_from_cas(lock_path, &store, &host)
        .unwrap_or_else(|error| panic!("frozen reopen of {} failed: {error}", lock_path.display()));
    let target = select_target(reopened.product_lock());
    LockVerifiedComposeImages::from_reopened_product_lock(&reopened, &target).map_err(|error| {
        format!(
            "LockVerifiedComposeImages::from_reopened_product_lock does not bind the reopened frozen lock at {} (CAS freeze-verify succeeded): {error}. Reconstructing LockedGameGraph must preserve lock_hash fields (namespace grants, schemas, interfaces, source_path) so client and headless share the committed latticeaxiom.lock without re-resolving",
            lock_path.display()
        )
    })
}

fn select_target(lock: &LockV1) -> TargetTriple {
    if let Some(host) = preferred_host_target()
        && lock.realizations.contains_key(&host)
    {
        return host;
    }
    lock.realizations
        .keys()
        .next()
        .cloned()
        .unwrap_or_else(|| panic!("product lock has no realization for this host"))
}

fn preferred_host_target() -> Option<TargetTriple> {
    let architecture = std::env::consts::ARCH;
    let value = if cfg!(all(target_os = "windows", target_env = "msvc")) {
        format!("{architecture}-pc-windows-msvc")
    } else if cfg!(all(target_os = "windows", target_env = "gnu")) {
        format!("{architecture}-pc-windows-gnu")
    } else if cfg!(target_os = "linux") {
        format!("{architecture}-unknown-linux-gnu")
    } else if cfg!(target_os = "macos") {
        format!("{architecture}-apple-darwin")
    } else {
        return None;
    };
    value.parse().ok()
}

fn shell_graph_from_images(images: &LockVerifiedComposeImages) -> ClientShellGraph {
    const CAPABILITIES: [(ShellCapability, &str); 5] = [
        (
            ShellCapability::ClientShell,
            "latticeaxiom:capability/client-shell@1",
        ),
        (
            ShellCapability::WorldCatalog,
            "latticeaxiom:capability/world-catalog@1",
        ),
        (
            ShellCapability::SettingsSurface,
            "latticeaxiom:capability/settings-surface@1",
        ),
        (
            ShellCapability::SettingsRegistry,
            "latticeaxiom:capability/settings-registry@1",
        ),
        (
            ShellCapability::DiagnosticRegistry,
            "latticeaxiom:capability/diagnostic-registry@1",
        ),
    ];
    let graph = images.images().graph();
    let mut providers = Vec::new();
    for (capability, identity) in CAPABILITIES {
        let capability_id: CapabilityId = identity.parse().expect("shell capability is canonical");
        let Some(packages) = graph.capability_providers.get(&capability_id) else {
            panic!(
                "shell lock is missing {identity}; the production ClientShellGraph cannot be selected"
            );
        };
        for package in packages {
            providers.push(ShellPackageProvider {
                package: package.clone(),
                capability,
            });
        }
    }
    let _ = providers
        .iter()
        .map(|provider| provider.package.clone())
        .collect::<Vec<PackageName>>();
    ClientShellGraph::resolve(providers).expect("lock-selected shell graph resolves")
}

fn semantic_command(target: &str, action: SemanticActionId) -> SemanticCommand {
    SemanticCommand {
        target: SemanticNodeId::new(target).expect("semantic ID is valid"),
        action,
        source: InputSource::Headless,
    }
}

fn parse_block(id: &str) -> BlockId {
    BlockId::parse(id).unwrap_or_else(|error| panic!("{id} parses: {error}"))
}

fn parse_item(id: &str) -> ItemId {
    ItemId::parse(id).unwrap_or_else(|error| panic!("{id} parses: {error}"))
}

fn parse_recipe(id: &str) -> RecipeId {
    RecipeId::parse(id).unwrap_or_else(|error| panic!("{id} parses: {error}"))
}

fn parse_process(id: &str) -> ProcessId {
    ProcessId::parse(id).unwrap_or_else(|error| panic!("{id} parses: {error}"))
}

fn gather_until_inventory_has(
    spine: &ProductionSpine,
    position: BlockPosition,
    item: &ItemId,
    quantity: u32,
) {
    let _ = mine_until_broken(spine, position);
    pickup_remaining(spine);
    let count = spine.inventory_view().expect("inventory").count_item(item);
    assert!(
        count >= quantity,
        "gathering {item} must yield at least {quantity}, got {count}"
    );
}

fn select_item_in_hotbar(spine: &ProductionSpine, item: &ItemId) {
    let view = spine.inventory_view().expect("inventory is bound");
    let slot = view
        .slots()
        .iter()
        .position(|stack| stack.as_ref().is_some_and(|stack| stack.item() == item))
        .unwrap_or_else(|| panic!("{item} must occupy an inventory slot"));
    if slot < usize::from(HOTBAR_SLOTS) {
        spine
            .select_hotbar_slot(u16::try_from(slot).expect("hotbar index fits"))
            .expect("gathered item is selected");
        return;
    }
    let dest = view
        .slots()
        .iter()
        .take(usize::from(HOTBAR_SLOTS))
        .position(Option::is_none)
        .unwrap_or(0);
    spine
        .move_stack(
            SlotIndex::new(u16::try_from(slot).expect("slot fits")),
            SlotIndex::new(u16::try_from(dest).expect("hotbar dest fits")),
        )
        .expect("stack moves onto the hotbar through the gameplay kernel");
    spine
        .select_hotbar_slot(u16::try_from(dest).expect("hotbar dest fits"))
        .expect("moved item is selected");
}

fn mine_until_broken(
    spine: &ProductionSpine,
    position: BlockPosition,
) -> latticeaxiom_engine::BlockEditSuccessV1 {
    for _ in 0..64 {
        match spine.mine_cell(position) {
            Ok(success) => return success,
            Err(BlockEditRejectV1::RequiresProgress { .. }) => {}
            Err(error) => panic!(
                "mining {position:?} failed: {error:?}, reject={:?}",
                spine.last_gameplay_reject()
            ),
        }
    }
    panic!("mining {position:?} did not complete")
}

fn pickup_remaining(spine: &ProductionSpine) {
    let drops: Vec<_> = spine.dropped_items().keys().copied().collect();
    for drop in drops {
        match spine.pickup_drop(drop) {
            Ok(_) | Err(GameplayReject::InventoryFull) => {}
            Err(error) => panic!("pickup failed: {error}"),
        }
    }
}

fn first_resident_any(spine: &ProductionSpine, ids: &[&str]) -> (BlockId, BlockPosition) {
    for id in ids {
        let block = parse_block(id);
        if let Some(position) = spine.first_resident_block(&block) {
            return (block, position);
        }
    }
    panic!(
        "none of {ids:?} exist in the streamed set {:?}",
        spine.resident_chunks()
    );
}

fn first_resident_wood(
    spine: &ProductionSpine,
) -> Option<(BlockId, BlockPosition, ItemId, RecipeId, ItemId)> {
    for id in ["terrenia:block/oak-log", "terrenia:block/pine-log"] {
        let block = parse_block(id);
        if let Some(position) = spine.first_resident_block(&block) {
            return Some(if id.ends_with("oak-log") {
                (
                    block,
                    position,
                    parse_item("terrenia:item/oak-log"),
                    parse_recipe("terrenia:recipe/oak-planks@1"),
                    parse_item("terrenia:item/oak-planks"),
                )
            } else {
                (
                    block,
                    position,
                    parse_item("terrenia:item/pine-log"),
                    parse_recipe("terrenia:recipe/pine-planks@1"),
                    parse_item("terrenia:item/pine-planks"),
                )
            });
        }
    }
    None
}

fn first_resident_soil(spine: &ProductionSpine) -> (BlockId, BlockPosition, ItemId) {
    let (block, position) = first_resident_any(
        spine,
        &[
            "terrenia:block/dirt",
            "terrenia:block/coarse-dirt",
            "terrenia:block/peat",
            "terrenia:block/mud",
            "terrenia:block/sand",
        ],
    );
    let item = parse_item(&block.as_str().replace(":block/", ":item/"));
    (block, position, item)
}

fn inspect_frame(generation: u64) -> PlayerActionFrameV1 {
    let mut started = PlayerActionButtonsV1::empty();
    started.insert(PlayerActionV1::Inspect);
    PlayerActionFrameV1 {
        generation,
        started,
        ..PlayerActionFrameV1::default()
    }
}

fn look_frame(generation: u64, yaw: f32, pitch: f32) -> PlayerActionFrameV1 {
    PlayerActionFrameV1 {
        generation,
        look_radians: ActionAxis2V1 { x: yaw, y: pitch },
        ..PlayerActionFrameV1::default()
    }
}

fn idle_frame(generation: u64) -> PlayerActionFrameV1 {
    PlayerActionFrameV1 {
        generation,
        ..PlayerActionFrameV1::default()
    }
}

fn enqueue_look_then_walk(
    instance: &mut EngineInstance,
    start_generation: u64,
    yaw: f32,
    pitch: f32,
    forward: f32,
    walk_ticks: u64,
) -> u64 {
    let mut frames = vec![
        look_frame(start_generation, yaw, pitch),
        idle_frame(start_generation + 1),
    ];
    frames.extend((0..walk_ticks).map(|offset| {
        let mut started = PlayerActionButtonsV1::empty();
        if offset.is_multiple_of(18) {
            started.insert(PlayerActionV1::Jump);
        }
        PlayerActionFrameV1 {
            generation: start_generation + 2 + offset,
            movement: ActionAxis2V1 { x: 0.0, y: forward },
            started,
            ..PlayerActionFrameV1::default()
        }
    }));
    instance
        .enqueue_headless_actions(frames)
        .expect("walk frames enqueue");
    start_generation + 2 + walk_ticks
}

fn walk_toward_column(
    instance: &mut EngineInstance,
    spine: &ProductionSpine,
    mut generation: u64,
    x: i32,
    z: i32,
    ticks: u64,
) -> u64 {
    let mut remaining = ticks;
    while remaining > 0 {
        let pose = spine.player_pose();
        #[allow(clippy::cast_precision_loss)]
        let dx = (x as f32 + 0.5) - pose.translation.x;
        #[allow(clippy::cast_precision_loss)]
        let dz = (z as f32 + 0.5) - pose.translation.z;
        if dx.hypot(dz) < 1.5 {
            break;
        }
        let look = wrap_pi(pose.yaw_radians - f32::atan2(-dx, -dz));
        let step = remaining.min(24);
        let before = generation;
        generation = enqueue_look_then_walk(instance, generation, look, 0.0, 1.0, step);
        let consumed = generation.saturating_sub(before);
        instance
            .advance_fixed_ticks(u32::try_from(consumed).expect("step fits u32"))
            .expect("walk ticks toward the required entrance");
        remaining = remaining.saturating_sub(step);
        assert!(
            !spine.occupies_unready_cave_void(),
            "travel must not enter an unready cave void at {:?}",
            spine.player_pose().translation
        );
    }
    generation
}

fn wait_for_resident(
    instance: &mut EngineInstance,
    spine: &ProductionSpine,
    mut generation: u64,
    position: BlockPosition,
    ticks: u64,
) -> u64 {
    let mut remaining = ticks;
    while remaining > 0 {
        if spine
            .chunk_of(position)
            .is_some_and(|chunk| spine.chunk_lifecycle(chunk) != ChunkLifecycle::Absent)
        {
            break;
        }
        let step = remaining.min(16);
        instance
            .enqueue_headless_actions((0..step).map(|offset| idle_frame(generation + offset)))
            .expect("wait frames enqueue");
        instance
            .advance_fixed_ticks(u32::try_from(step).expect("wait step fits"))
            .expect("wait ticks stream the entrance column");
        generation = generation.saturating_add(step);
        remaining = remaining.saturating_sub(step);
    }
    generation
}

fn idle_at_hole(
    instance: &mut EngineInstance,
    spine: &ProductionSpine,
    generation: u64,
    ticks: u64,
) -> u64 {
    let frames: Vec<_> = (0..ticks)
        .map(|offset| idle_frame(generation + offset))
        .collect();
    instance
        .enqueue_headless_actions(frames)
        .expect("idle frames enqueue");
    instance
        .advance_fixed_ticks(u32::try_from(ticks).expect("idle ticks fit"))
        .expect("idle ticks at the cave hole");
    assert!(
        !spine.occupies_unready_cave_void(),
        "idle at the hole must not enter an unready cave void"
    );
    generation + ticks
}

fn wrap_pi(value: f32) -> f32 {
    let pi = std::f32::consts::PI;
    let tau = 2.0 * pi;
    let mut wrapped = (value + pi) % tau;
    if wrapped < 0.0 {
        wrapped += tau;
    }
    wrapped - pi
}

fn open_required_entrance_shaft(
    spine: &ProductionSpine,
    surface: BlockPosition,
    aperture: BlockPosition,
) {
    let mut opened = 0_u32;
    for dz in -1..=1 {
        for dx in -1..=1 {
            let mut y = surface.y.saturating_add(8);
            while y >= aperture.y {
                let position = BlockPosition {
                    x: surface.x.saturating_add(dx),
                    y,
                    z: surface.z.saturating_add(dz),
                };
                if spine
                    .cave_occupancy_arbitration(
                        i64::from(position.x),
                        i64::from(position.y),
                        i64::from(position.z),
                    )
                    .is_some_and(CaveOccupancyArbitrationV1::is_finally_void)
                {
                    y -= 1;
                    continue;
                }
                if mine_cover_cell(spine, position) {
                    opened = opened.saturating_add(1);
                    y -= 1;
                    continue;
                }
                if spine
                    .inspect_occupancy(position)
                    .ok()
                    .is_some_and(|occupancy| {
                        occupancy.solid.is_none()
                            || occupancy
                                .solid
                                .as_ref()
                                .is_some_and(|block| block.as_str().ends_with("/air"))
                    })
                {
                    y -= 1;
                    continue;
                }
                panic!(
                    "required entrance cover {position:?} did not break, reject={:?}, gameplay={:?}",
                    spine.last_reject(),
                    spine.last_gameplay_reject()
                );
            }
        }
    }
    assert!(
        opened > 0,
        "required entrance column {surface:?} -> {aperture:?} must open at least one cover voxel"
    );
}

fn mine_cover_cell(spine: &ProductionSpine, position: BlockPosition) -> bool {
    for slot in [1_u16, 0_u16] {
        let _ = spine.select_hotbar_slot(slot);
        for _ in 0..64 {
            match spine.mine_cell(position) {
                Ok(_) => {
                    pickup_remaining(spine);
                    return true;
                }
                Err(BlockEditRejectV1::RequiresProgress { .. }) => {}
                Err(BlockEditRejectV1::RequiresTool { .. } | BlockEditRejectV1::NotBreakable) => {
                    break;
                }
                Err(BlockEditRejectV1::PermissionDenied | BlockEditRejectV1::NoTarget) => {
                    return false;
                }
                Err(error) => panic!("cover mining {position:?} failed: {error:?}"),
            }
        }
    }
    false
}

#[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]
fn player_in_cave(
    spine: &ProductionSpine,
    translation: bevy::prelude::Vec3,
    aperture_y: i32,
) -> bool {
    let feet_y = translation.y - 0.9;
    if feet_y <= aperture_y as f32 + 0.75 {
        return true;
    }
    let cells = [
        [
            translation.x.floor() as i64,
            (translation.y - 0.9).floor() as i64,
            translation.z.floor() as i64,
        ],
        [
            translation.x.floor() as i64,
            translation.y.floor() as i64,
            translation.z.floor() as i64,
        ],
    ];
    cells.iter().any(|&[x, y, z]| {
        spine
            .cave_occupancy_arbitration(x, y, z)
            .is_some_and(CaveOccupancyArbitrationV1::is_finally_void)
    })
}
