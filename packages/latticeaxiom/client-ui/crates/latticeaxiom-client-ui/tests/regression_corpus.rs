//! Data-driven UI regression corpus for CLIENT-UI-001 and CLIENT-SURFACE-STATE-001.
//!
//! Headless semantic injection covers keyboard, mouse, and gamepad. GPU, window,
//! and manual visual cases are recorded as bounded non-blocking deferred evidence.

#![allow(
    clippy::expect_used,
    reason = "integration fixtures state their construction invariants"
)]

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

use latticeaxiom_client_ui::{
    AccessKitNode, AccessKitRole, ButtonWidget, CjkFallbackFontId, ClientSurfaceActionV1,
    GameModalV1, GameOverlayV1, GameSurfaceSession, HomeContinueV1, ImeTextState,
    InputBindingCandidateV1, InputContextKind, InputSource, KeyCapturePhase, KeyCaptureSession,
    LogicalViewport, MAX_ROUTE_DEPTH, SaveQuitProjection, SemanticAction, SemanticCommand,
    SemanticKey, SemanticNode, SemanticRole, ShellRouteV1, ShellSurfaceSession, SurfaceCommandV1,
    SurfaceInjectEffect, ThemeTokens, UiScale, check_accesskit_tree, surface_command, surface_key,
    verify_scroll_layout,
};
use serde::Deserialize;

const FIXTURE_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/ui-regression-corpus.json"
);
const SCHEMA_ID: &str = "latticeaxiom.ui-regression-corpus.v1";
const REQUIRED_REQUIREMENTS: [&str; 2] = ["CLIENT-UI-001", "CLIENT-SURFACE-STATE-001"];
const REQUIRED_SOURCES: [InputSource; 3] = [
    InputSource::Keyboard,
    InputSource::Mouse,
    InputSource::Gamepad,
];
const REQUIRED_SCALES: [UiScale; 2] = [UiScale::One, UiScale::Two];
const MAX_DEFERRED_CASES: usize = 8;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct UiRegressionCorpus {
    schema_id: String,
    requirement_ids: Vec<String>,
    viewport: LogicalViewport,
    scales: Vec<UiScale>,
    cjk_fallback_font: String,
    injection_sources: Vec<InputSource>,
    ime: ImeFixture,
    rebind: RebindFixture,
    accesskit_roles: BTreeMap<String, AccessKitRole>,
    shell_journey: Vec<ShellJourneyStep>,
    deferred_cases: Vec<DeferredCase>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ImeFixture {
    composition: String,
    commit: String,
    committed_name: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RebindFixture {
    row: String,
    action_id: String,
    candidate: InputBindingCandidateV1,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ShellJourneyStep {
    inject_target: String,
    expect_route: ShellRouteV1,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct DeferredCase {
    id: String,
    kind: DeferredKind,
    blocking: bool,
    requires_window: bool,
    requires_gpu: bool,
    reason: String,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "kebab-case")]
enum DeferredKind {
    Manual,
    Window,
    GpuWindow,
}

fn load_corpus() -> UiRegressionCorpus {
    let path = Path::new(FIXTURE_PATH);
    let contents = fs::read_to_string(path).unwrap_or_else(|error| {
        panic!(
            "UI regression fixture must be readable at {}: {error}",
            path.display()
        )
    });
    serde_json::from_str(&contents)
        .unwrap_or_else(|error| panic!("UI regression fixture must be valid JSON: {error}"))
}

fn activate(target: &str, source: InputSource) -> SemanticCommand {
    surface_command(
        SemanticKey::new(target).expect("corpus target is a stable key"),
        ClientSurfaceActionV1::Activate.semantic_action(),
        source,
    )
}

fn settings_fragment(row: &SemanticKey, encoding: Option<&str>) -> SemanticNode {
    let mut capture = ButtonWidget::new(
        row.clone(),
        "Pause binding",
        Some("Controls capture row. The UI never writes the profile file.".to_owned()),
        true,
    )
    .semantic_node(false);
    capture.value = encoding.map(ToOwned::to_owned);
    SemanticNode {
        key: surface_key("modal/settings"),
        role: SemanticRole::Group,
        name: "Settings".to_owned(),
        value: None,
        description: Some("Typed settings fragment without a second application root".to_owned()),
        state: latticeaxiom_client_ui::SemanticState::default(),
        actions: BTreeSet::new(),
        children: vec![
            ButtonWidget::new(
                surface_key("settings/back"),
                "Back",
                Some("Return to pause".to_owned()),
                true,
            )
            .semantic_node(false),
            capture,
        ],
    }
}

fn focused_keys(node: &SemanticNode) -> Vec<SemanticKey> {
    let mut keys = Vec::new();
    collect_focused(node, &mut keys);
    keys
}

fn collect_focused(node: &SemanticNode, keys: &mut Vec<SemanticKey>) {
    if node.state.focused {
        keys.push(node.key.clone());
    }
    for child in &node.children {
        collect_focused(child, keys);
    }
}

fn find_accesskit<'a>(node: &'a AccessKitNode, key: &str) -> Option<&'a AccessKitNode> {
    if node.key == key {
        return Some(node);
    }
    node.children
        .iter()
        .find_map(|child| find_accesskit(child, key))
}

fn assert_unique_focus_and_context(
    root: &SemanticNode,
    epoch: latticeaxiom_client_ui::SurfaceEpoch,
    focus_epoch: latticeaxiom_client_ui::SurfaceEpoch,
    context: &latticeaxiom_client_ui::ActiveInputContextStack,
) {
    let tree = check_accesskit_tree(root).expect("AccessKit tree");
    assert_eq!(tree.role, AccessKitRole::Application);
    let focused = focused_keys(root);
    assert!(
        focused.len() <= 1,
        "surface must have at most one focused node"
    );
    assert_eq!(focus_epoch, epoch, "focus owner must share the live epoch");
    let layers = context.layers();
    let unique_layers = layers.iter().copied().collect::<BTreeSet<_>>();
    assert_eq!(
        unique_layers.len(),
        layers.len(),
        "input context layers must be unique"
    );
    assert!(
        layers.len() <= usize::from(MAX_ROUTE_DEPTH),
        "context depth must stay inside the frozen route cap"
    );
}

fn assert_layout(root: &SemanticNode, corpus: &UiRegressionCorpus) {
    let tokens = ThemeTokens::plain_v2();
    let focus = root.focus_order();
    for scale in &corpus.scales {
        let evidence =
            verify_scroll_layout(corpus.viewport, *scale, &tokens, &focus).expect("layout");
        assert!(evidence.essentials_reachable(&focus));
        assert!(evidence.scroll_viewport_height >= 88);
    }
}

fn persist_rebind(candidate: &InputBindingCandidateV1) -> String {
    serde_json::to_string(candidate).expect("rebind persist encoding")
}

fn restore_rebind(bytes: &str) -> InputBindingCandidateV1 {
    serde_json::from_str(bytes).expect("rebind restore encoding")
}

fn assert_role(root: &SemanticNode, key: &str, role: AccessKitRole) {
    let tree = check_accesskit_tree(root).expect("AccessKit");
    assert_eq!(find_accesskit(&tree, key).expect("named node").role, role);
}

fn open_inventory_workbench_and_pause(
    session: &mut GameSurfaceSession,
    corpus: &UiRegressionCorpus,
) {
    session
        .apply(&SurfaceCommandV1::ToggleInventory)
        .expect("inventory");
    assert_eq!(session.router().route().overlay(), GameOverlayV1::Inventory);
    assert_eq!(
        session.context().layers(),
        &[InputContextKind::Gameplay, InputContextKind::HudOverlay]
    );
    assert_role(
        session.snapshot().root(),
        "overlay/inventory",
        *corpus
            .accesskit_roles
            .get("overlay/inventory")
            .expect("inventory role"),
    );

    session
        .apply(&SurfaceCommandV1::OpenWorkbench)
        .expect("workbench");
    assert_eq!(session.router().route().overlay(), GameOverlayV1::Workbench);
    assert_role(
        session.snapshot().root(),
        "overlay/workbench",
        AccessKitRole::Group,
    );

    let pause = session.apply(&SurfaceCommandV1::Pause).expect("pause");
    assert_eq!(pause.route.modal(), GameModalV1::Pause);
    assert_eq!(pause.route.overlay(), GameOverlayV1::Workbench);
    assert!(pause.gameplay_suppressed);
    assert_eq!(
        session.context().layers(),
        &[InputContextKind::Gameplay, InputContextKind::Surface]
    );
    assert_eq!(session.focus().epoch(), pause.epoch);
    assert_role(
        session.snapshot().root(),
        "modal/pause",
        AccessKitRole::Dialog,
    );
}

fn inject_rebind_and_persist(
    session: &mut GameSurfaceSession,
    source: InputSource,
    row: &SemanticKey,
    corpus: &UiRegressionCorpus,
    durable: &str,
) {
    session
        .inject(&activate("modal/pause/settings", source))
        .expect("settings");
    assert_eq!(session.router().route().modal(), GameModalV1::Settings);
    session
        .inject(&activate(row.as_str(), source))
        .expect("capture");
    assert_eq!(
        session.router().route().modal(),
        GameModalV1::BindingCapture
    );
    assert_eq!(
        session.context().layers(),
        &[
            InputContextKind::Gameplay,
            InputContextKind::Surface,
            InputContextKind::BindingCapture
        ]
    );
    session.presentation_mut().capture = Some(KeyCaptureSession::begin(
        session.router().epoch(),
        row.clone(),
    ));
    session.refresh_projection().expect("capture view");
    let capture = session
        .presentation_mut()
        .capture
        .as_mut()
        .expect("live capture");
    assert_eq!(
        capture
            .receive_candidate(corpus.rebind.candidate.clone(), None)
            .expect("accept"),
        KeyCapturePhase::Accepted
    );
    let restored = restore_rebind(durable);
    assert_eq!(restored, corpus.rebind.candidate);
    session.refresh_projection().expect("accepted capture");
    session
        .apply(&SurfaceCommandV1::Back)
        .expect("capture back");
    session
        .apply(&SurfaceCommandV1::Back)
        .expect("settings back");
    assert_eq!(session.router().route().modal(), GameModalV1::Pause);
}

fn inject_durable_save_quit(
    session: &mut GameSurfaceSession,
    source: InputSource,
    corpus: &UiRegressionCorpus,
) {
    session
        .inject(&activate("modal/pause/save-quit", source))
        .expect("confirm");
    session
        .inject(&activate("modal/confirm-save-quit/confirm", source))
        .expect("save");
    assert_eq!(
        session.router().route().save_quit(),
        Some(SaveQuitProjection::Saving)
    );
    let written = session
        .apply(&SurfaceCommandV1::AcknowledgeWritten)
        .expect("written");
    assert_eq!(
        written.route.save_quit(),
        Some(SaveQuitProjection::WrittenNotDurable)
    );
    let durable_receipt = session
        .apply(&SurfaceCommandV1::AcknowledgeDurable)
        .expect("durable");
    assert_eq!(
        durable_receipt.route.save_quit(),
        Some(SaveQuitProjection::Durable)
    );
    assert_layout(session.snapshot().root(), corpus);
}

fn assert_rebind_survives_replacement(row: &SemanticKey, durable: &str) {
    let mut restarted = GameSurfaceSession::playing().expect("replacement process");
    let restored = restore_rebind(durable);
    restarted.presentation_mut().settings_fragment =
        Some(settings_fragment(row, Some(&restored.encoding)));
    restarted
        .apply(&SurfaceCommandV1::Pause)
        .expect("pause after restart");
    restarted
        .apply(&SurfaceCommandV1::OpenSettings)
        .expect("settings after restart");
    let tree = check_accesskit_tree(restarted.snapshot().root()).expect("AccessKit");
    let rebound = find_accesskit(&tree, row.as_str()).expect("persisted controls row");
    assert_eq!(rebound.value.as_deref(), Some(restored.encoding.as_str()));
    assert!(
        restarted
            .snapshot()
            .root()
            .find(&surface_key("shell"))
            .is_none(),
        "game settings must not grow a second UI root"
    );
}

#[test]
fn corpus_records_required_gates_and_bounds_deferred_cases() {
    let corpus = load_corpus();
    assert_eq!(corpus.schema_id, SCHEMA_ID);
    assert_eq!(corpus.requirement_ids, REQUIRED_REQUIREMENTS);
    assert_eq!(corpus.viewport.width, 800);
    assert_eq!(corpus.viewport.height, 600);
    assert_eq!(corpus.scales, REQUIRED_SCALES);
    assert_eq!(corpus.injection_sources, REQUIRED_SOURCES);
    assert_eq!(corpus.cjk_fallback_font, CjkFallbackFontId::v1().as_str());
    assert!(!corpus.ime.committed_name.is_ascii());
    assert!(!corpus.shell_journey.is_empty());
    assert!(
        corpus.deferred_cases.len() <= MAX_DEFERRED_CASES,
        "GPU/window/manual cases must stay a bounded list"
    );
    assert!(!corpus.deferred_cases.is_empty());
    let mut seen = BTreeSet::new();
    for case in &corpus.deferred_cases {
        assert!(
            seen.insert(case.id.as_str()),
            "deferred case ids must be unique"
        );
        assert!(
            !case.blocking,
            "deferred GPU/window/manual case `{}` must not be a blocking gate",
            case.id
        );
        assert!(!case.reason.is_empty());
        match case.kind {
            DeferredKind::GpuWindow => {
                assert!(case.requires_gpu);
                assert!(case.requires_window);
            }
            DeferredKind::Window => assert!(case.requires_window),
            DeferredKind::Manual => {}
        }
    }
    assert!(
        corpus
            .deferred_cases
            .iter()
            .any(|case| case.kind == DeferredKind::GpuWindow && case.requires_gpu)
    );
    assert!(
        corpus
            .deferred_cases
            .iter()
            .any(|case| case.kind == DeferredKind::Manual)
    );
}

#[test]
fn keyboard_mouse_and_gamepad_inject_shell_create_continue() {
    let corpus = load_corpus();
    for source in &corpus.injection_sources {
        let mut shell = ShellSurfaceSession::home().expect("home");
        let tree = check_accesskit_tree(shell.snapshot().root()).expect("AccessKit");
        assert_eq!(
            tree.role,
            *corpus
                .accesskit_roles
                .get("shell")
                .expect("shell AccessKit role")
        );
        assert_eq!(
            find_accesskit(&tree, "shell/home").expect("home").role,
            *corpus
                .accesskit_roles
                .get("shell/home")
                .expect("home AccessKit role")
        );
        assert_unique_focus_and_context(
            shell.snapshot().root(),
            shell.router().epoch(),
            shell.focus().epoch(),
            &shell.context(),
        );
        assert_eq!(shell.context().layers(), &[InputContextKind::Surface]);

        for step in &corpus.shell_journey {
            shell
                .inject(&activate(&step.inject_target, *source))
                .expect("shell inject");
            assert_eq!(shell.router().route(), step.expect_route);
            assert_unique_focus_and_context(
                shell.snapshot().root(),
                shell.router().epoch(),
                shell.focus().epoch(),
                &shell.context(),
            );
            if step.expect_route == ShellRouteV1::NewWorld {
                let mut ime = ImeTextState::default();
                ime.set_composition(&corpus.ime.composition);
                assert_eq!(ime.composing(), Some(corpus.ime.composition.as_str()));
                ime.commit(&corpus.ime.commit);
                assert_eq!(ime.committed(), corpus.ime.committed_name);
                shell.presentation_mut().new_world.name = ime.committed().to_owned();
                shell.refresh_projection().expect("ime name");
                let name = shell
                    .snapshot()
                    .root()
                    .find(&surface_key("new-world/name"))
                    .expect("IME field");
                assert_eq!(name.role, SemanticRole::TextInput);
                assert_eq!(
                    name.value.as_deref(),
                    Some(corpus.ime.committed_name.as_str())
                );
                assert_eq!(
                    find_accesskit(
                        &check_accesskit_tree(shell.snapshot().root()).expect("AccessKit"),
                        "new-world/name"
                    )
                    .expect("name node")
                    .role,
                    AccessKitRole::TextInput
                );
            }
        }

        shell.presentation_mut().home.continue_action = HomeContinueV1::Continue {
            label: corpus.ime.committed_name.clone(),
            summary: "ReadyExact lock durable-frontier 4".to_owned(),
        };
        shell.refresh_projection().expect("continue visible");
        let effect = shell
            .inject(&activate("home/continue", *source))
            .expect("continue");
        assert_eq!(effect, SurfaceInjectEffect::Continue);
        shell
            .apply(&SurfaceCommandV1::OpenShell(ShellRouteV1::Loading))
            .expect("loading");
        assert_eq!(shell.router().route(), ShellRouteV1::Loading);
        assert_layout(shell.snapshot().root(), &corpus);
    }
}

#[test]
fn keyboard_mouse_and_gamepad_inject_inventory_pause_rebind_and_save_quit() {
    let corpus = load_corpus();
    let row = SemanticKey::new(&corpus.rebind.row).expect("controls row");
    let durable = persist_rebind(&corpus.rebind.candidate);

    for source in &corpus.injection_sources {
        let mut session = GameSurfaceSession::playing().expect("playing");
        session.presentation_mut().settings_fragment = Some(settings_fragment(&row, None));
        session.refresh_projection().expect("settings fragment");
        assert_unique_focus_and_context(
            session.snapshot().root(),
            session.router().epoch(),
            session.focus().epoch(),
            &session.context(),
        );
        assert_eq!(session.context().layers(), &[InputContextKind::Gameplay]);
        open_inventory_workbench_and_pause(&mut session, &corpus);
        inject_rebind_and_persist(&mut session, *source, &row, &corpus, &durable);
        inject_durable_save_quit(&mut session, *source, &corpus);
    }

    assert_rebind_survives_replacement(&row, &durable);
    assert_eq!(corpus.rebind.action_id, "latticeaxiom:action/ui/pause@1");
}

#[test]
fn stale_epoch_cannot_reopen_a_closed_surface() {
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
    assert_unique_focus_and_context(
        session.snapshot().root(),
        session.router().epoch(),
        session.focus().epoch(),
        &session.context(),
    );
}

#[test]
fn ime_cjk_and_required_scales_keep_controls_reachable() {
    let corpus = load_corpus();
    let tokens = ThemeTokens::plain_v2();
    assert_eq!(tokens.cjk_fallback_font.as_str(), corpus.cjk_fallback_font);
    let mut ime = ImeTextState::default();
    ime.set_composition(&corpus.ime.composition);
    ime.commit(&corpus.ime.commit);
    assert_eq!(ime.committed(), corpus.ime.committed_name);

    let mut shell = ShellSurfaceSession::home().expect("home");
    shell.presentation_mut().new_world.name = ime.committed().to_owned();
    shell
        .apply(&SurfaceCommandV1::OpenShell(ShellRouteV1::NewWorld))
        .expect("new world");
    assert_layout(shell.snapshot().root(), &corpus);

    let mut game = GameSurfaceSession::playing().expect("playing");
    game.apply(&SurfaceCommandV1::ToggleInventory)
        .expect("inventory");
    game.apply(&SurfaceCommandV1::Pause).expect("pause");
    assert_layout(game.snapshot().root(), &corpus);
}

#[test]
fn deferred_gpu_window_manual_cases_are_not_blocking_evidence() {
    let corpus = load_corpus();
    for case in &corpus.deferred_cases {
        assert!(!case.blocking);
        assert!(
            case.requires_gpu || case.requires_window || case.kind == DeferredKind::Manual,
            "deferred case `{}` must be classified as GPU, window, or manual",
            case.id
        );
    }
    assert!(
        !corpus
            .deferred_cases
            .iter()
            .any(|case| matches!(case.kind, DeferredKind::GpuWindow) && case.blocking),
        "GPU/window visual comparison must not be treated as a CI pass"
    );
}

#[test]
fn semantic_commands_are_source_neutral() {
    let corpus = load_corpus();
    let shell = ShellSurfaceSession::home().expect("home");
    let root = shell.snapshot().root();
    for source in &corpus.injection_sources {
        let command = SemanticCommand {
            target: surface_key("home/new-world"),
            action: SemanticAction::Activate,
            source: *source,
        };
        latticeaxiom_client_ui::validate_semantic_command(root, &command)
            .expect("same logical action");
    }
}
