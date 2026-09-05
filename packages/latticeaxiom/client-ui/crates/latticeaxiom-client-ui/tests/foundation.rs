//! Foundation-wide routing, focus, AccessKit, and layout gates.

#![allow(
    clippy::expect_used,
    reason = "integration fixtures state their construction invariants"
)]

use latticeaxiom_client_ui::{
    ButtonWidget, ClientSurfaceActionV1, GameModalV1, GameOverlayV1, GameSurfaceRouter,
    InputContextKind, LayoutEvidence, ListItemWidget, ListWidget, LogicalViewport, ModalWidget,
    SaveQuitProjection, SemanticAction, SemanticCommand, SemanticKey, SemanticRole,
    SemanticSnapshot, SurfaceCommandV1, SurfaceEpoch, ThemeTokens, UiScale, accept_async_epoch,
    application_root, check_accesskit_tree, diff_snapshots, validate_semantic_command,
    verify_scroll_layout,
};

fn key(value: &str) -> SemanticKey {
    SemanticKey::new(value).expect("static key")
}

fn button(id: &str, name: &str, focused: bool) -> latticeaxiom_client_ui::SemanticNode {
    ButtonWidget {
        key: key(id),
        name: name.to_owned(),
        description: Some(name.to_owned()),
        enabled: true,
    }
    .semantic_node(focused)
}

#[test]
fn keyboard_mouse_and_gamepad_activate_the_same_button() {
    let root = application_root(
        key("shell"),
        "Lattice Axiom",
        vec![button("pause/resume", "Resume", true)],
    )
    .expect("root");
    for source in [
        latticeaxiom_client_ui::InputSource::Keyboard,
        latticeaxiom_client_ui::InputSource::Mouse,
        latticeaxiom_client_ui::InputSource::Gamepad,
        latticeaxiom_client_ui::InputSource::Headless,
    ] {
        let command = SemanticCommand {
            target: key("pause/resume"),
            action: ClientSurfaceActionV1::Activate.semantic_action(),
            source,
        };
        validate_semantic_command(&root, &command).expect("same logical action");
    }
}

#[test]
fn modal_list_and_button_project_accesskit_roles() {
    let list = ListWidget {
        key: key("worlds"),
        name: "Worlds".to_owned(),
        items: vec![ListItemWidget {
            key: key("world:alpha"),
            name: "Alpha".to_owned(),
            value: Some("ready-exact".to_owned()),
            enabled: true,
        }],
        selected: Some(key("world:alpha")),
    }
    .semantic_node(None);
    let modal = ModalWidget {
        key: key("modal/confirm-save-quit"),
        name: "Save and quit".to_owned(),
        description: Some("Confirm durable exit".to_owned()),
        confirm: latticeaxiom_client_ui::ButtonWidget {
            key: key("modal/confirm-save-quit/confirm"),
            name: "Save and quit".to_owned(),
            description: None,
            enabled: true,
        },
        cancel: latticeaxiom_client_ui::ButtonWidget {
            key: key("modal/confirm-save-quit/cancel"),
            name: "Cancel".to_owned(),
            description: None,
            enabled: true,
        },
    }
    .semantic_node(Some(&key("modal/confirm-save-quit/confirm")));
    let root = application_root(key("shell"), "Lattice Axiom", vec![list, modal]).expect("root");
    let tree = check_accesskit_tree(&root).expect("a11y");
    assert_eq!(
        tree.children[0].role,
        latticeaxiom_client_ui::AccessKitRole::List
    );
    assert_eq!(
        tree.children[1].role,
        latticeaxiom_client_ui::AccessKitRole::Dialog
    );
    assert!(
        tree.children[1].actions.contains(&"cancel".to_owned())
            && tree.children[1].actions.contains(&"back".to_owned())
    );
}

#[test]
fn layout_gates_pass_at_800x600_for_both_required_scales() {
    let tokens = ThemeTokens::plain_v2();
    let viewport = LogicalViewport {
        width: 800,
        height: 600,
    };
    let focus = [
        key("home/continue"),
        key("home/settings"),
        key("settings/apply"),
        key("pause/resume"),
    ];
    for scale in [UiScale::One, UiScale::Two] {
        let evidence: LayoutEvidence =
            verify_scroll_layout(viewport, scale, &tokens, &focus).expect("layout");
        assert!(evidence.essentials_reachable(&focus));
        assert!(evidence.scroll_viewport_height >= 88);
    }
}

#[test]
fn overlay_then_modal_then_transition_clears_the_previous_epoch() {
    let mut router = GameSurfaceRouter::new(1).expect("router");
    let inventory = router
        .apply(&SurfaceCommandV1::ToggleInventory)
        .expect("inventory");
    assert_eq!(inventory.route.overlay(), GameOverlayV1::Inventory);
    let pause = router.apply(&SurfaceCommandV1::Pause).expect("pause");
    assert_eq!(pause.cleared_pressed_epoch, inventory.epoch);
    router
        .apply(&SurfaceCommandV1::RequestSaveQuit)
        .expect("confirm");
    let saving = router.apply(&SurfaceCommandV1::Confirm).expect("saving");
    assert_eq!(saving.route.modal(), GameModalV1::None);
    assert_eq!(saving.route.save_quit(), Some(SaveQuitProjection::Saving));
    assert_eq!(saving.context.layers(), &[InputContextKind::Surface]);
}

#[test]
fn stale_async_snapshot_cannot_rebuild_a_closed_settings_surface() {
    let open = SemanticSnapshot::new(
        SurfaceEpoch::from_raw(4),
        application_root(
            key("shell"),
            "Lattice Axiom",
            vec![button("modal/settings", "Settings", true)],
        )
        .expect("open"),
    );
    let closed = SemanticSnapshot::new(
        SurfaceEpoch::from_raw(5),
        application_root(
            key("shell"),
            "Lattice Axiom",
            vec![button("modal/pause/resume", "Resume", true)],
        )
        .expect("closed"),
    );
    let diff = diff_snapshots(Some(&open), &closed).expect("diff");
    assert!(diff.removed.contains(&key("modal/settings")));
    assert!(accept_async_epoch(closed.epoch(), open.epoch()).is_err());
    assert_eq!(closed.root().find(&key("modal/settings")), None);
}

#[test]
fn dialog_advertises_esc_safety_actions() {
    let modal = ModalWidget {
        key: key("modal/confirm-save-quit"),
        name: "Confirm".to_owned(),
        description: None,
        confirm: latticeaxiom_client_ui::ButtonWidget {
            key: key("modal/confirm/ok"),
            name: "Confirm".to_owned(),
            description: None,
            enabled: true,
        },
        cancel: latticeaxiom_client_ui::ButtonWidget {
            key: key("modal/confirm/cancel"),
            name: "Cancel".to_owned(),
            description: None,
            enabled: true,
        },
    }
    .semantic_node(None);
    assert!(modal.actions.contains(&SemanticAction::Back));
    assert_eq!(modal.role, SemanticRole::Dialog);
}
