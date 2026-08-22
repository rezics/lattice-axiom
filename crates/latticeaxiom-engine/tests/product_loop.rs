//! V1/V2 product-loop, input, and hidden-Terrenia integration coverage.
#![allow(clippy::expect_used)]

use std::{collections::BTreeSet, fs, io, path::PathBuf};

use latticeaxiom_client_ui::{
    GameOverlayV1, GameSurfaceRouter, ROUTE_VOCABULARY_MAJOR, SurfaceCommandV1,
};
use latticeaxiom_core::StableId;
use latticeaxiom_engine::{HostUserSettings, ProductionMemoryStart, ProductionSurfaceRouter};
use latticeaxiom_input::{
    ActionCatalogDocumentV1, AuthoritativePlayerActionV1, BindingProfileV1, ClientSurfaceActionV1,
    HeadlessInputSession, HudOverlayKindV1, INPUT_PACKAGE_NAME, InputActionsProviderV1,
    InputBindingV1, InputContextV1, compile_input_catalog,
};
use latticeaxiom_player::PlayerActionV1;

#[test]
fn headless_move_inspect_break_place_share_player_action_discriminants() {
    assert_eq!(
        PlayerActionV1::from(AuthoritativePlayerActionV1::Move) as u8,
        PlayerActionV1::Move as u8
    );
    assert_eq!(
        PlayerActionV1::from(AuthoritativePlayerActionV1::Inspect) as u8,
        PlayerActionV1::Inspect as u8
    );
    assert_eq!(
        PlayerActionV1::from(AuthoritativePlayerActionV1::BreakBlock) as u8,
        PlayerActionV1::BreakBlock as u8
    );
    assert_eq!(
        PlayerActionV1::from(AuthoritativePlayerActionV1::PlaceBlock) as u8,
        PlayerActionV1::PlaceBlock as u8
    );
}

#[test]
fn rebind_survives_replacement_process_restart() {
    let directory = TestDirectory::create();
    let mut settings = HostUserSettings::load(&directory.0).expect("empty settings load");
    let compiled = compile_shipped_catalog(&BindingProfileV1::empty());
    let inventory = action("latticeaxiom:action/hud/toggle-inventory@1");
    let rebound = compiled
        .preview_rebind(
            &inventory,
            InputBindingV1::Keyboard {
                usage: "KeyI".to_owned(),
                modifiers: BTreeSet::default(),
            },
            settings.binding_profile(),
        )
        .expect("KeyI is free");
    settings
        .persist_binding_profile(&directory.0, rebound)
        .expect("binding profile persists atomically");
    let restored = HostUserSettings::load(&directory.0).expect("settings reopen");
    let compiled = compile_shipped_catalog(restored.binding_profile());
    let bindings = &compiled
        .action_by_id(&inventory)
        .expect("inventory")
        .effective_bindings;
    assert!(bindings.iter().any(
        |binding| matches!(binding, InputBindingV1::Keyboard { usage, .. } if usage == "KeyI")
    ));
}

#[test]
fn closing_inventory_clears_started_surface_and_unsuppresses_gameplay() {
    let mut session =
        HeadlessInputSession::new(compile_shipped_catalog(&BindingProfileV1::empty()));
    assert!(
        session
            .press_surface(ClientSurfaceActionV1::ToggleInventory)
            .expect("toggle")
    );
    session
        .push_context(
            InputContextV1::HudOverlay,
            Some(HudOverlayKindV1::Inventory),
        )
        .expect("open inventory");
    session
        .release_surface(ClientSurfaceActionV1::ToggleInventory)
        .expect("release");
    assert!(
        session
            .press_surface(ClientSurfaceActionV1::ToggleInventory)
            .expect("close toggle")
    );
    session.pop_context().expect("close");
    let after = session.sample();
    assert!(!after.gameplay_suppressed);
    assert!(after.started_surface.is_empty());

    let mut router = GameSurfaceRouter::new(ROUTE_VOCABULARY_MAJOR).expect("router");
    router
        .apply(&SurfaceCommandV1::ToggleInventory)
        .expect("open");
    let closed = router
        .apply(&SurfaceCommandV1::ToggleInventory)
        .expect("close");
    assert!(!closed.gameplay_suppressed);
    assert_eq!(closed.route.overlay(), GameOverlayV1::None);
}

#[test]
fn production_lock_graph_does_not_hide_terrenia_or_input() {
    assert!(ProductionMemoryStart::lock_roots_select_shell([
        "@latticeaxiom/front-end",
        "@latticeaxiom/input"
    ]));
    assert!(
        !ProductionMemoryStart::lock_roots_select_shell(["@latticeaxiom/front-end", "terrenia"]),
        "a terrenia root must boot the game process, not a hidden host plugin list"
    );
    assert!(
        compile_input_catalog([], &BindingProfileV1::empty())
            .expect_err("missing provider")
            .to_string()
            .contains("no input-actions provider")
    );
    assert_eq!(INPUT_PACKAGE_NAME, "@latticeaxiom/input");
}

#[test]
fn surface_router_vocabulary_is_the_game_process_owner() {
    let router = ProductionSurfaceRouter::playing().expect("playing router");
    assert_eq!(router.inner().epoch().get(), 1);
}

fn compile_shipped_catalog(
    profile: &BindingProfileV1,
) -> latticeaxiom_input::CompiledInputCatalogV1 {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../packages/latticeaxiom/input/data/action-catalog-v1.json");
    let bytes = fs::read(path).expect("shipped catalog exists");
    let catalog = ActionCatalogDocumentV1::from_bytes(&bytes).expect("catalog parses");
    let provider = InputActionsProviderV1::new(
        catalog.owner_package.clone(),
        catalog.capability.clone(),
        catalog,
    )
    .expect("provider");
    compile_input_catalog([provider], profile).expect("compile")
}

fn action(id: &str) -> StableId {
    id.parse().expect("action id")
}

struct TestDirectory(PathBuf);

impl TestDirectory {
    fn create() -> Self {
        let path = std::env::temp_dir().join(format!(
            "latticeaxiom-engine-product-loop-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |duration| duration.as_nanos())
        ));
        fs::create_dir_all(&path).expect("temp dir");
        Self(fs::canonicalize(&path).expect("canonical temp dir"))
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        if let Err(error) = fs::remove_dir_all(&self.0)
            && error.kind() != io::ErrorKind::NotFound
        {
            panic!("temp dir cleanup failed: {error}");
        }
    }
}
