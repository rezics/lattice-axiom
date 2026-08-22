//! Typed shell/game surface journey: routes, focus, AccessKit, IME, and scale.

#![allow(
    clippy::expect_used,
    reason = "integration fixtures state their construction invariants"
)]

use latticeaxiom_client_ui::{
    BindingConflict, CjkFallbackFontId, ClientSurfaceActionV1, GameModalV1, GameOverlayV1,
    GameSurfaceSession, GameTransitionV1, HUD_HOTBAR_SLOTS, HomeContinueV1, INVENTORY_SLOT_COUNT,
    ImeTextState, InputBindingCandidateV1, InputSource, InspectOverlayV1, InventoryClickV1,
    InventorySlotV1, KeyCapturePhase, KeyCaptureSession, LogicalViewport, RecipeRowV1,
    SaveQuitProjection, SemanticKey, ShellRouteV1, ShellSurfaceSession, SurfaceCommandV1,
    SurfaceInjectEffect, SurfaceRouterError, SurfaceSessionError, ThemeTokens, UiScale,
    check_accesskit_tree, surface_command, surface_key, verify_scroll_layout,
};

fn activate(target: &'static str, source: InputSource) -> latticeaxiom_client_ui::SemanticCommand {
    surface_command(
        surface_key(target),
        ClientSurfaceActionV1::Activate.semantic_action(),
        source,
    )
}

#[test]
fn keyboard_mouse_and_gamepad_complete_home_create_continue_loading() {
    let mut shell = ShellSurfaceSession::home().expect("home");
    assert_eq!(shell.router().route(), ShellRouteV1::Home);
    assert_eq!(shell.context().layers().len(), 1);
    let tree = check_accesskit_tree(shell.snapshot().root()).expect("a11y");
    assert_eq!(
        tree.role,
        latticeaxiom_client_ui::AccessKitRole::Application
    );
    assert!(
        shell
            .snapshot()
            .root()
            .find(&surface_key("home/new-world"))
            .is_some()
    );

    for source in [
        InputSource::Keyboard,
        InputSource::Mouse,
        InputSource::Gamepad,
    ] {
        let mut session = ShellSurfaceSession::home().expect("home");
        session
            .inject(&activate("home/new-world", source))
            .expect("create");
        assert_eq!(session.router().route(), ShellRouteV1::NewWorld);
        session
            .inject(&activate("new-world/back", source))
            .expect("back");
        assert_eq!(session.router().route(), ShellRouteV1::Home);
    }

    shell
        .inject(&activate("home/continue", InputSource::Headless))
        .expect_err("continue unmapped without an exact world is a host effect");
    shell.presentation_mut().home.continue_action = HomeContinueV1::Continue {
        label: "Alpha".to_owned(),
        summary: "ReadyExact lock durable-frontier 4".to_owned(),
    };
    shell.refresh_projection().expect("continue visible");
    let effect = shell
        .inject(&activate("home/continue", InputSource::Keyboard))
        .expect("continue");
    assert_eq!(effect, SurfaceInjectEffect::Continue);

    shell
        .apply(&SurfaceCommandV1::OpenShell(ShellRouteV1::Loading))
        .expect("loading");
    assert_eq!(shell.router().route(), ShellRouteV1::Loading);
    shell
        .apply(&SurfaceCommandV1::MarkWriterOpened)
        .expect("writer");
    assert_eq!(
        shell.apply(&SurfaceCommandV1::Cancel),
        Err(SurfaceSessionError::Router(
            SurfaceRouterError::WriterCancelRequiresShutdown
        ))
    );
}

#[test]
fn recovery_and_quit_confirm_stay_on_the_typed_shell_router() {
    let mut shell = ShellSurfaceSession::home().expect("home");
    shell
        .inject(&activate("home/worlds", InputSource::Gamepad))
        .expect("worlds");
    shell
        .inject(&activate("worlds/trash", InputSource::Keyboard))
        .expect("recovery");
    assert_eq!(shell.router().route(), ShellRouteV1::Recovery);
    shell
        .inject(&activate("recovery/back", InputSource::Mouse))
        .expect("back to worlds");
    assert_eq!(shell.router().route(), ShellRouteV1::Worlds);
    shell
        .inject(&activate("worlds/back", InputSource::Keyboard))
        .expect("back to home");
    assert_eq!(shell.router().route(), ShellRouteV1::Home);
    shell
        .inject(&activate("home/quit", InputSource::Keyboard))
        .expect("quit");
    assert_eq!(shell.router().route(), ShellRouteV1::QuitConfirm);
    let quit = shell
        .inject(&activate("modal/quit/confirm", InputSource::Gamepad))
        .expect("confirm");
    assert_eq!(quit, SurfaceInjectEffect::QuitProduct);
}

#[test]
fn hud_inspect_omits_occupancy_and_hotbar_has_nine_slots() {
    let session = GameSurfaceSession::playing().expect("playing");
    let inspect = &session.presentation().hud.inspect;
    assert!(!inspect.contains_occupancy());
    assert_eq!(
        session.presentation().hud.hotbar.slots.len(),
        usize::from(HUD_HOTBAR_SLOTS)
    );
    assert_eq!(
        session.presentation().inventory.slots().len(),
        usize::from(INVENTORY_SLOT_COUNT)
    );
    let tree = session.snapshot().root();
    assert!(tree.find(&surface_key("hud/status")).is_some());
    assert!(tree.find(&surface_key("hud/inspect/header-name")).is_some());
    assert!(
        tree.find(&surface_key("hud/inspect/harvestability"))
            .is_some()
    );
    assert!(tree.find(&surface_key("hud/inspect/declared-by")).is_some());
    assert!(tree.find(&surface_key("hud/inspect/stable-id")).is_some());
    assert!(tree.find(&surface_key("hud/hotbar/slot-8")).is_some());
    assert!(tree.find(&surface_key("hud/inspect")).is_some_and(|node| {
        !node
            .value
            .as_deref()
            .is_some_and(|value| value.contains("occupancy"))
    }));
}

#[test]
fn inventory_click_to_swap_is_draft_state_and_does_not_write_slots() {
    let mut session = GameSurfaceSession::playing().expect("playing");
    session
        .apply(&SurfaceCommandV1::ToggleInventory)
        .expect("open");
    session.presentation_mut().inventory.sync_slots(vec![
        InventorySlotV1 {
            index: 0,
            label: "Oak Log".to_owned(),
            quantity: Some(4),
        },
        InventorySlotV1 {
            index: 1,
            label: String::new(),
            quantity: None,
        },
    ]);
    session.refresh_projection().expect("sync");
    let first = session
        .inject(&activate("overlay/inventory/slot-0", InputSource::Mouse))
        .expect("latch");
    assert_eq!(
        first,
        SurfaceInjectEffect::InventoryClick(InventoryClickV1::Latched(0))
    );
    assert_eq!(session.presentation().inventory.cursor_slot(), Some(0));
    assert_eq!(
        session.presentation().inventory.slots()[0].quantity,
        Some(4),
        "draft must not mutate slot contents"
    );
    let mv = session
        .inject(&activate("overlay/inventory/slot-1", InputSource::Keyboard))
        .expect("move intent");
    assert_eq!(
        mv,
        SurfaceInjectEffect::InventoryClick(InventoryClickV1::RequestMove { from: 0, to: 1 })
    );
    assert!(session.presentation().inventory.cursor_slot().is_none());
    assert_eq!(
        session.presentation().inventory.slots()[0].quantity,
        Some(4)
    );
}

#[test]
fn inventory_pause_settings_capture_unwind_has_one_focus_owner() {
    let mut session = GameSurfaceSession::playing().expect("playing");
    session
        .apply(&SurfaceCommandV1::ToggleInventory)
        .expect("inventory");
    session.presentation_mut().hand_recipes.recipes = vec![RecipeRowV1 {
        id: "example:recipe/planks@1".to_owned(),
        name: "Planks".to_owned(),
        craftable: true,
    }];
    session.refresh_projection().expect("recipes");
    let pause = session.apply(&SurfaceCommandV1::Pause).expect("pause");
    assert_eq!(pause.route.modal(), GameModalV1::Pause);
    assert_eq!(pause.route.overlay(), GameOverlayV1::Inventory);
    assert!(pause.gameplay_suppressed);
    assert_eq!(session.focus().epoch(), pause.epoch);

    session
        .inject(&activate("modal/pause/settings", InputSource::Gamepad))
        .expect("settings");
    assert_eq!(session.router().route().modal(), GameModalV1::Settings);

    let row = SemanticKey::new("settings/controls/pause").expect("row");
    session
        .apply(&SurfaceCommandV1::OpenBindingCapture { row: row.clone() })
        .expect("capture route");
    session.presentation_mut().capture =
        Some(KeyCaptureSession::begin(session.router().epoch(), row));
    session.refresh_projection().expect("capture view");
    assert_eq!(
        session.router().route().modal(),
        GameModalV1::BindingCapture
    );
    assert_eq!(
        session.context().layers(),
        &[
            latticeaxiom_client_ui::InputContextKind::Gameplay,
            latticeaxiom_client_ui::InputContextKind::Surface,
            latticeaxiom_client_ui::InputContextKind::BindingCapture
        ]
    );

    session
        .apply(&SurfaceCommandV1::Back)
        .expect("capture back");
    session
        .apply(&SurfaceCommandV1::Back)
        .expect("settings back");
    session.apply(&SurfaceCommandV1::Back).expect("pause back");
    assert_eq!(session.router().route().modal(), GameModalV1::None);
    assert_eq!(session.router().route().overlay(), GameOverlayV1::Inventory);
    assert_eq!(session.focus().epoch(), session.router().epoch());
    check_accesskit_tree(session.snapshot().root()).expect("one a11y root");
}

#[test]
fn save_and_quit_rejects_reopen_and_written_is_not_durable() {
    let mut session = GameSurfaceSession::playing().expect("playing");
    session.apply(&SurfaceCommandV1::Pause).expect("pause");
    session
        .inject(&activate("modal/pause/save-quit", InputSource::Keyboard))
        .expect("confirm");
    session
        .inject(&activate(
            "modal/confirm-save-quit/confirm",
            InputSource::Mouse,
        ))
        .expect("save");
    assert_eq!(
        session.router().route().transition(),
        GameTransitionV1::SavingAndExiting
    );
    assert_eq!(
        session.apply(&SurfaceCommandV1::ToggleInventory),
        Err(SurfaceSessionError::Router(
            SurfaceRouterError::IllegalTransition
        ))
    );
    let written = session
        .apply(&SurfaceCommandV1::AcknowledgeWritten)
        .expect("written");
    assert_eq!(
        written.route.save_quit(),
        Some(SaveQuitProjection::WrittenNotDurable)
    );
    let durable = session
        .apply(&SurfaceCommandV1::AcknowledgeDurable)
        .expect("durable");
    assert_eq!(durable.route.save_quit(), Some(SaveQuitProjection::Durable));
}

#[test]
fn stale_async_epoch_cannot_reopen_a_closed_surface() {
    let mut session = GameSurfaceSession::playing().expect("playing");
    session.apply(&SurfaceCommandV1::Pause).expect("pause");
    session
        .apply(&SurfaceCommandV1::OpenSettings)
        .expect("settings");
    let open_epoch = session.router().epoch();
    session.apply(&SurfaceCommandV1::Back).expect("close");
    assert!(session.accept_async(open_epoch).is_err());
    assert!(
        session
            .snapshot()
            .root()
            .find(&surface_key("modal/settings"))
            .is_none()
    );
}

#[test]
fn layout_and_cjk_gates_hold_at_800x600_for_both_scales() {
    let tokens = ThemeTokens::plain_v2();
    assert_eq!(
        tokens.cjk_fallback_font.as_str(),
        CjkFallbackFontId::v1().as_str()
    );
    let mut ime = ImeTextState::default();
    ime.set_composition("世");
    assert_eq!(ime.composing(), Some("世"));
    ime.commit("世界");
    assert_eq!(ime.committed(), "世界");

    let mut shell = ShellSurfaceSession::home().expect("home");
    shell.presentation_mut().new_world.name = ime.committed().to_owned();
    shell
        .apply(&SurfaceCommandV1::OpenShell(ShellRouteV1::NewWorld))
        .expect("new world");
    let name = shell
        .snapshot()
        .root()
        .find(&surface_key("new-world/name"))
        .expect("ime field");
    assert_eq!(name.value.as_deref(), Some("世界"));

    let focus = shell.snapshot().root().focus_order();
    let viewport = LogicalViewport {
        width: 800,
        height: 600,
    };
    for scale in [UiScale::One, UiScale::Two] {
        let evidence = verify_scroll_layout(viewport, scale, &tokens, &focus).expect("layout");
        assert!(evidence.essentials_reachable(&focus));
        assert!(evidence.scroll_viewport_height >= 88);
    }
}

#[test]
fn binding_conflict_cancel_does_not_half_apply() {
    let row = SemanticKey::new("settings/controls/pause").expect("row");
    let mut capture = KeyCaptureSession::begin(latticeaxiom_client_ui::SurfaceEpoch::FIRST, row);
    let candidate = InputBindingCandidateV1 {
        schema_version: 1,
        encoding: "keyboard:escape".to_owned(),
    };
    capture
        .receive_candidate(
            candidate.clone(),
            Some(BindingConflict {
                occupying_action: "latticeaxiom:action/ui/back@1".to_owned(),
                candidate,
            }),
        )
        .expect("conflict");
    assert_eq!(capture.phase(), KeyCapturePhase::ConfirmingConflict);
    assert_eq!(capture.cancel(), Ok(KeyCapturePhase::Cancelled));
}

#[test]
fn inspect_fixture_does_not_copy_occupancy_into_player_overlay() {
    let inspect = InspectOverlayV1 {
        name: "Oak Log".to_owned(),
        icon: "oak-log".to_owned(),
        harvestability: "axe 1 12".to_owned(),
        declared_by: "example".to_owned(),
        stable_id: "example:block/oak-log".to_owned(),
    };
    assert!(!inspect.contains_occupancy());
    assert!(!inspect.overlay_lines().contains("r4"));
    assert!(inspect.overlay_lines().contains("declared-by example"));
}
