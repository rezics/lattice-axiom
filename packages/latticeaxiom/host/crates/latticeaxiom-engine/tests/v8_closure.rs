//! V8/D9 product-loop closure: catalog, surfaces, fluids, and presentation omit.
#![allow(clippy::expect_used)]

use std::fs;

use latticeaxiom_client_ui::{
    GameModalV1, GameOverlayV1, GameSurfaceSession, InputSource, ShellRouteV1, ShellSurfaceSession,
    SurfaceCommandV1, check_accesskit_tree, surface_command, surface_key,
};
use latticeaxiom_content::{
    FluidCollisionPolicyV1, FluidFlowV1, FluidLevelV1, FluidSelectionPolicyV1, FluidStateV1,
    inspect_fluid_cell,
};
use latticeaxiom_core::StableId;
use latticeaxiom_engine::{
    AuthoredContentCatalogSourcesV1, AuthoredContentDisplayCatalogSourcesV1,
    AuthoredGameplayCatalogSourcesV1, AuthoredPresentationCatalogSourcesV1,
    compile_authored_content_catalog, compile_authored_content_display_catalog,
    compile_authored_gameplay_catalog,
};
use latticeaxiom_voxel_runtime::{FluidRevisionStamp, admit_fluid_completion};
use latticeaxiom_world_wire::{
    CHUNK_EDGE_V1, FluidPaletteOpenDispositionV1, SOLID_FLUID_PALETTE_SCHEMA_ID_V1,
    classify_fluid_palette_open,
};

const AUTHORED_BLOCKS_JSON: &str =
    include_str!("../../../../../terrenia/blocks/data/authored-catalog-v1.json");
const AUTHORED_BLOCK_DISPLAY_JSON: &str =
    include_str!("../../../../../terrenia/blocks/data/authored-display-v1.json");
const AUTHORED_RULES_JSON: &str =
    include_str!("../../../../../terrenia/gameplay/data/authored-rules-v1.json");
const AUTHORED_ITEM_BROWSER_JSON: &str =
    include_str!("../../../../../terrenia/gameplay/data/authored-item-browser-v1.json");
const AUTHORED_TOOLS_JSON: &str =
    include_str!("../../../../../terrenia/tools/data/authored-tools-v1.json");
const AUTHORED_TOOL_DISPLAY_JSON: &str =
    include_str!("../../../../../terrenia/tools/data/authored-display-v1.json");
const AUTHORED_BIOMES_JSON: &str =
    include_str!("../../../../../terrenia/worldgen/data/authored-biomes-v1.json");
const D7_BIOME_IDS: &str =
    include_str!("../../../../../terrenia/worldgen/data/goldens/d7-biome-ids.txt");
const D9_BLOCK_IDS: &str =
    include_str!("../../../../../terrenia/blocks/data/goldens/d9-block-ids.txt");
const FLUID_IDS: &str = include_str!("../../../../../terrenia/blocks/data/goldens/fluid-ids.txt");
const PRESENTATION_DISPLAY_JSON: &str =
    include_str!("../../../../../terrenia/presentation/data/authored-display-v1.json");
const PRESENTATION_ASSETS_JSON: &str =
    include_str!("../../../../../terrenia/presentation/data/authored-assets-v1.json");

fn activate(target: &'static str, source: InputSource) -> latticeaxiom_client_ui::SemanticCommand {
    surface_command(
        surface_key(target),
        latticeaxiom_client_ui::ClientSurfaceActionV1::Activate.semantic_action(),
        source,
    )
}

fn stable_id(value: &str) -> StableId {
    value.parse().expect("stable id")
}

#[test]
fn locked_catalog_is_exactly_72_blocks_and_water_lava() {
    let catalog = compile_authored_content_catalog(AuthoredContentCatalogSourcesV1 {
        blocks: AUTHORED_BLOCKS_JSON,
        biomes: AUTHORED_BIOMES_JSON,
        d7_biome_ids: D7_BIOME_IDS,
        d9_block_ids: D9_BLOCK_IDS,
    })
    .expect("D9 catalog compiles");
    assert_eq!(catalog.blocks().len(), 72);
    let fluids = catalog
        .fluids()
        .iter()
        .map(|fluid| fluid.header.stable_id.as_str())
        .collect::<Vec<_>>();
    assert_eq!(fluids, ["terrenia:fluid/lava", "terrenia:fluid/water"]);
    let gameplay = compile_authored_gameplay_catalog(AuthoredGameplayCatalogSourcesV1 {
        blocks: AUTHORED_BLOCKS_JSON,
        rules: AUTHORED_RULES_JSON,
        browser: AUTHORED_ITEM_BROWSER_JSON,
        tools: AUTHORED_TOOLS_JSON,
        d9_block_ids: D9_BLOCK_IDS,
    })
    .expect("gameplay catalog compiles");
    for id in [
        "terrenia:block/workbench",
        "terrenia:block/furnace",
        "terrenia:block/chest",
        "terrenia:block/torch",
    ] {
        let block = latticeaxiom_gameplay::BlockId::parse(id).expect("canonical block");
        assert!(
            gameplay.block(&block).is_some() || gameplay.block_schema_binding(&block).is_some(),
            "{id} must bind D8 mechanics"
        );
    }
}

#[test]
fn omitted_presentation_matches_overlay_and_does_not_enter_content_catalog() {
    let omitted =
        compile_authored_content_display_catalog(AuthoredContentDisplayCatalogSourcesV1 {
            block_display: AUTHORED_BLOCK_DISPLAY_JSON,
            tool_display: AUTHORED_TOOL_DISPLAY_JSON,
            tool_catalog: AUTHORED_TOOLS_JSON,
            d9_block_ids: D9_BLOCK_IDS,
            fluid_ids: FLUID_IDS,
            presentation: None,
        })
        .expect("omitted presentation catalog compiles");
    let presented =
        compile_authored_content_display_catalog(AuthoredContentDisplayCatalogSourcesV1 {
            block_display: AUTHORED_BLOCK_DISPLAY_JSON,
            tool_display: AUTHORED_TOOL_DISPLAY_JSON,
            tool_catalog: AUTHORED_TOOLS_JSON,
            d9_block_ids: D9_BLOCK_IDS,
            fluid_ids: FLUID_IDS,
            presentation: Some(AuthoredPresentationCatalogSourcesV1 {
                display: PRESENTATION_DISPLAY_JSON,
                assets: PRESENTATION_ASSETS_JSON,
            }),
        })
        .expect("presentation catalog compiles");
    assert!(!omitted.includes_presentation());
    assert!(presented.includes_presentation());
    let omitted_ids = omitted.locked_ids().collect::<Vec<_>>();
    let presented_ids = presented.locked_ids().collect::<Vec<_>>();
    assert_eq!(omitted_ids, presented_ids);
    assert_eq!(omitted_ids.len(), 81);
    for id in omitted_ids {
        assert_eq!(omitted.lookup(id), presented.lookup(id), "{id}");
    }
    let catalog = compile_authored_content_catalog(AuthoredContentCatalogSourcesV1 {
        blocks: AUTHORED_BLOCKS_JSON,
        biomes: AUTHORED_BIOMES_JSON,
        d7_biome_ids: D7_BIOME_IDS,
        d9_block_ids: D9_BLOCK_IDS,
    })
    .expect("content catalog compiles");
    assert_eq!(catalog.blocks().len(), 72);
    assert_eq!(catalog.fluids().len(), 2);
}

#[test]
fn keyboard_mouse_and_gamepad_complete_home_create_inventory_pause_settings_save_quit() {
    for source in [
        InputSource::Keyboard,
        InputSource::Mouse,
        InputSource::Gamepad,
    ] {
        let mut shell = ShellSurfaceSession::home().expect("home");
        assert_eq!(shell.router().route(), ShellRouteV1::Home);
        check_accesskit_tree(shell.snapshot().root()).expect("shell a11y");
        shell
            .inject(&activate("home/new-world", source))
            .expect("create");
        assert_eq!(shell.router().route(), ShellRouteV1::NewWorld);
        shell
            .inject(&activate("new-world/back", source))
            .expect("home");
        assert_eq!(shell.router().route(), ShellRouteV1::Home);

        let mut game = GameSurfaceSession::playing().expect("playing");
        game.apply(&SurfaceCommandV1::ToggleInventory)
            .expect("inventory");
        assert_eq!(game.router().route().overlay(), GameOverlayV1::Inventory);
        game.apply(&SurfaceCommandV1::OpenWorkbench)
            .expect("workbench");
        assert_eq!(game.router().route().overlay(), GameOverlayV1::Workbench);
        let paused = game.apply(&SurfaceCommandV1::Pause).expect("pause");
        assert_eq!(paused.route.modal(), GameModalV1::Pause);
        game.inject(&activate("modal/pause/settings", source))
            .expect("settings");
        assert_eq!(game.router().route().modal(), GameModalV1::Settings);
        game.apply(&SurfaceCommandV1::Back).expect("back to pause");
        game.inject(&activate("modal/pause/save-quit", source))
            .expect("confirm save");
        game.inject(&activate("modal/confirm-save-quit/confirm", source))
            .expect("save");
        game.apply(&SurfaceCommandV1::AcknowledgeWritten)
            .expect("written");
        let durable = game
            .apply(&SurfaceCommandV1::AcknowledgeDurable)
            .expect("durable");
        assert_eq!(
            durable.route.save_quit(),
            Some(latticeaxiom_client_ui::SaveQuitProjection::Durable)
        );
        check_accesskit_tree(game.snapshot().root()).expect("game a11y");
    }
}

#[test]
fn stale_fluid_completion_cannot_overwrite_a_newer_chunk_revision() {
    let captured = FluidRevisionStamp::new(
        latticeaxiom_storage::WorldRevision::new(4),
        latticeaxiom_storage::ChunkRevision::new(2),
        latticeaxiom_storage::VoxelRevision::new(2),
    );
    let current = FluidRevisionStamp::new(
        latticeaxiom_storage::WorldRevision::new(4),
        latticeaxiom_storage::ChunkRevision::new(3),
        latticeaxiom_storage::VoxelRevision::new(3),
    );
    assert!(matches!(
        admit_fluid_completion(captured, current),
        Err(latticeaxiom_voxel_runtime::FluidRuntimeError::Stale { .. })
    ));
    assert!(admit_fluid_completion(captured, captured).is_ok());
}

#[test]
fn fluid_inspect_fragment_omits_presentation_and_unknown_palette_is_not_writable() {
    let collision =
        FluidCollisionPolicyV1::new(stable_id("fixture:fluid-collision-policy/volume@1"))
            .expect("volume collision");
    let selection =
        FluidSelectionPolicyV1::new(stable_id("fixture:fluid-selection-policy/source@1"))
            .expect("source selection");
    let fragment = inspect_fluid_cell(
        stable_id("terrenia:fluid/water"),
        FluidStateV1 {
            level: FluidLevelV1::SOURCE,
            flow: FluidFlowV1::Still,
        },
        &collision,
        &selection,
    )
    .expect("inspect");
    assert_eq!(fragment.fluid().as_str(), "terrenia:fluid/water");
    assert_eq!(fragment.level(), 0);
    let encoded = serde_json::to_string(&fragment).expect("fragment serializes");
    assert!(!encoded.contains("asset"));
    assert!(!encoded.contains("icon"));
    assert_eq!(
        classify_fluid_palette_open(&SOLID_FLUID_PALETTE_SCHEMA_ID_V1.parse().expect("schema")),
        FluidPaletteOpenDispositionV1::Writable
    );
    assert_eq!(
        classify_fluid_palette_open(
            &"other:schema/solid-fluid-palette@1"
                .parse()
                .expect("unknown schema")
        ),
        FluidPaletteOpenDispositionV1::ReadOnlyRecovery
    );
    assert_eq!(CHUNK_EDGE_V1, 32);
}

#[test]
fn product_profiles_omit_science_magic_progress_and_relations() {
    for relative in [
        "profiles/dev.ncl",
        "profiles/headless.ncl",
        "profiles/test.ncl",
        "profiles/shell.ncl",
    ] {
        let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../../../..")
            .join(relative);
        let text = fs::read_to_string(&path).expect("profile is readable");
        for forbidden in [
            "@terrenia/science",
            "@terrenia/thaumaturgy",
            "@latticeaxiom/progress",
            "@latticeaxiom/relations",
            "@terrenia/metallurgy",
            "@terrenia/journey",
        ] {
            assert!(
                !text.contains(forbidden),
                "{} must not lock {forbidden}",
                path.display()
            );
        }
    }
    let headless = fs::read_to_string(
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../../../../profiles/headless.ncl"),
    )
    .expect("headless profile");
    assert!(
        !headless.contains("terrenia:source/presentation"),
        "headless profile must omit the presentation package"
    );
    let dev = fs::read_to_string(
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../../../../profiles/dev.ncl"),
    )
    .expect("dev profile");
    assert!(
        dev.contains("terrenia:source/presentation"),
        "client-world profile must keep presentation in the source universe"
    );
}
