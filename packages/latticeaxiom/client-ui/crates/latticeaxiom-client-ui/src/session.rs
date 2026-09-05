//! Unique surface sessions. One router, one focus owner, one epoch-bound tree.

use crate::focus::{FocusController, FocusOwner, LinearFocusDirection};
use crate::key_capture::KeyCaptureSession;
use crate::projection::{ProjectionError, SemanticSnapshot, accept_async_epoch, diff_snapshots};
use crate::router::{
    GameApplyReceipt, GameModalV1, GameOverlayV1, GameSurfaceRouter, GameTransitionV1,
    SaveQuitProjection, ShellApplyReceipt, ShellRouteV1, ShellSurfaceRouter, SurfaceCommandV1,
    SurfaceRouterError,
};
use crate::semantic::{
    InputSource, SemanticAction, SemanticCommand, SemanticCommandError, SemanticKey, SemanticNode,
    validate_semantic_command,
};
use crate::surface::{ActiveInputContextStack, ROUTE_VOCABULARY_MAJOR, SurfaceEpoch};
use crate::views::{
    BindingCaptureViewV1, ConfirmSaveQuitViewV1, DiagnosticsAboutViewV1, FatalRecoveryViewV1,
    HomeViewV1, HudModelV1, InventoryClickV1, InventoryDraftV1, LoadingViewV1, NewWorldViewV1,
    PackagesProfilesViewV1, PauseViewV1, QuitConfirmViewV1, RecipeListV1, RecoveryViewV1,
    SavingViewV1, WorkbenchOverlayV1, WorldsViewV1, surface_key,
};
use crate::widgets::{UiRootError, application_root};
use thiserror::Error;

/// Presentation models consumed by the game session. Authority stays outside this crate.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GamePresentationV1 {
    /// HUD status, hotbar, and inspect.
    pub hud: HudModelV1,
    /// Inventory click-to-swap draft.
    pub inventory: InventoryDraftV1,
    /// Hand-crafting recipes (`workstation == None`).
    pub hand_recipes: RecipeListV1,
    /// Workbench overlay.
    pub workbench: WorkbenchOverlayV1,
    /// Optional settings fragment. Must not contain a second application root.
    pub settings_fragment: Option<SemanticNode>,
    /// Binding-capture session while Settings is the parent.
    pub capture: Option<KeyCaptureSession>,
    /// Whether a world writer is open.
    pub writer_open: bool,
}

impl GamePresentationV1 {
    /// Empty playing presentation.
    #[must_use]
    pub fn playing() -> Self {
        Self {
            hud: HudModelV1::empty(),
            inventory: InventoryDraftV1::empty(),
            hand_recipes: RecipeListV1::default(),
            workbench: WorkbenchOverlayV1::empty("latticeaxiom:workstation/crafting@1"),
            settings_fragment: None,
            capture: None,
            writer_open: true,
        }
    }
}

/// Unique game-process owner of route, focus, context, and projection.
#[derive(Clone, Debug)]
pub struct GameSurfaceSession {
    router: GameSurfaceRouter,
    focus: FocusController,
    snapshot: SemanticSnapshot,
    presentation: GamePresentationV1,
}

impl GameSurfaceSession {
    /// Playing session at vocabulary major 1.
    ///
    /// # Errors
    ///
    /// Returns [`SurfaceSessionError`] when the vocabulary is unsupported or
    /// projection fails.
    pub fn playing() -> Result<Self, SurfaceSessionError> {
        Self::new(GamePresentationV1::playing())
    }

    /// Playing session with caller presentation.
    ///
    /// # Errors
    ///
    /// Returns [`SurfaceSessionError`] when projection fails.
    pub fn new(presentation: GamePresentationV1) -> Result<Self, SurfaceSessionError> {
        let router = GameSurfaceRouter::new(ROUTE_VOCABULARY_MAJOR)?;
        let focus =
            FocusController::new(FocusOwner::new(router.epoch(), surface_key("game/playing")));
        let mut session = Self {
            router,
            focus,
            snapshot: SemanticSnapshot::new(
                SurfaceEpoch::FIRST,
                application_root(surface_key("game"), "Lattice Axiom", Vec::new())?,
            ),
            presentation,
        };
        session.rebuild()?;
        Ok(session)
    }

    /// Returns the live router.
    #[must_use]
    pub const fn router(&self) -> &GameSurfaceRouter {
        &self.router
    }

    /// Returns the unique focus owner.
    #[must_use]
    pub const fn focus(&self) -> &FocusOwner {
        self.focus.owner()
    }

    /// Returns the live semantic snapshot.
    #[must_use]
    pub const fn snapshot(&self) -> &SemanticSnapshot {
        &self.snapshot
    }

    /// Returns derived input context.
    #[must_use]
    pub fn context(&self) -> ActiveInputContextStack {
        self.router.route().input_context()
    }

    /// Returns presentation draft.
    #[must_use]
    pub const fn presentation(&self) -> &GamePresentationV1 {
        &self.presentation
    }

    /// Returns a mutable presentation draft. Caller must [`Self::refresh_projection`]
    /// after HUD/inventory visuals change.
    pub const fn presentation_mut(&mut self) -> &mut GamePresentationV1 {
        &mut self.presentation
    }

    /// Rebuilds the tree for the live epoch without changing the route.
    ///
    /// # Errors
    ///
    /// Returns [`SurfaceSessionError`] if projection fails.
    pub fn refresh_projection(&mut self) -> Result<(), SurfaceSessionError> {
        self.rebuild()
    }

    /// Applies a typed surface command in one stage.
    ///
    /// # Errors
    ///
    /// Returns [`SurfaceSessionError`] for an illegal transition or projection
    /// failure. The live route is unchanged on illegal commands.
    pub fn apply(
        &mut self,
        command: &SurfaceCommandV1,
    ) -> Result<GameApplyReceipt, SurfaceSessionError> {
        let receipt = self.router.apply(command)?;
        if matches!(
            command,
            SurfaceCommandV1::ToggleInventory
                | SurfaceCommandV1::OpenWorkbench
                | SurfaceCommandV1::Confirm
        ) && self.router.route().overlay() == GameOverlayV1::None
        {
            self.presentation.inventory.clear_latch();
        }
        self.focus = FocusController::new(receipt.focus.clone());
        self.rebuild()?;
        Ok(receipt)
    }

    /// Validates a semantic command against the live tree and maps it onto a
    /// route command, presentation latch, or no-op.
    ///
    /// # Errors
    ///
    /// Returns [`SurfaceSessionError`] when the target vanished, is disabled,
    /// or the mapped transition is illegal.
    pub fn inject(
        &mut self,
        command: &SemanticCommand,
    ) -> Result<SurfaceInjectEffect, SurfaceSessionError> {
        validate_semantic_command(self.snapshot.root(), command)?;
        if matches!(
            command.action,
            SemanticAction::FocusNext | SemanticAction::FocusPrevious
        ) {
            let direction = if command.action == SemanticAction::FocusNext {
                LinearFocusDirection::Next
            } else {
                LinearFocusDirection::Previous
            };
            self.focus.move_linear(self.snapshot.root(), direction)?;
            self.rebuild()?;
            return Ok(SurfaceInjectEffect::FocusMoved);
        }
        let target = command.target.as_str();
        if let Some(slot) = target.strip_prefix("overlay/inventory/slot-") {
            let slot: u16 = slot
                .parse()
                .map_err(|_| SurfaceSessionError::UnmappedCommand)?;
            let click = self.presentation.inventory.click_slot(slot);
            self.rebuild()?;
            return Ok(SurfaceInjectEffect::InventoryClick(click));
        }
        if target.starts_with("overlay/inventory/recipe/")
            || target.starts_with("overlay/workbench/recipe/")
        {
            return Ok(SurfaceInjectEffect::Craft {
                recipe: target.to_owned(),
            });
        }
        if let Some(command) = map_game_command(self.router.route().modal(), target, command.action)
        {
            let receipt = self.apply(&command)?;
            return Ok(SurfaceInjectEffect::Route(receipt));
        }
        Err(SurfaceSessionError::UnmappedCommand)
    }

    /// Rejects a stale async payload instead of rebuilding a closed surface.
    ///
    /// # Errors
    ///
    /// Returns [`ProjectionError::StaleEpoch`] when `payload_epoch` is not live.
    pub fn accept_async(&self, payload_epoch: SurfaceEpoch) -> Result<(), ProjectionError> {
        accept_async_epoch(self.router.epoch(), payload_epoch)
    }

    fn rebuild(&mut self) -> Result<(), SurfaceSessionError> {
        if self
            .presentation
            .settings_fragment
            .as_ref()
            .is_some_and(|node| {
                node.role == crate::SemanticRole::Application || node.has_second_application_root()
            })
        {
            return Err(SurfaceSessionError::SecondUiRoot);
        }
        let focused = self.focus.owner().key().cloned();
        let children = project_game(
            self.router.route().overlay(),
            self.router.route().modal(),
            self.router.route().transition(),
            self.router.route().save_quit(),
            &self.presentation,
            focused.as_ref(),
        )?;
        let root = application_root(surface_key("game"), "Lattice Axiom", children)?;
        let next = SemanticSnapshot::new(self.router.epoch(), root);
        let _ = diff_snapshots(Some(&self.snapshot), &next)?;
        let _ = self.focus.restore(next.root(), self.router.epoch());
        self.snapshot = next;
        Ok(())
    }
}

fn map_game_command(
    modal: GameModalV1,
    target: &str,
    action: SemanticAction,
) -> Option<SurfaceCommandV1> {
    if target == "modal/pause/resume"
        || (action == SemanticAction::Back && modal == GameModalV1::Pause)
    {
        return Some(SurfaceCommandV1::Resume);
    }
    if target == "modal/pause/settings" {
        return Some(SurfaceCommandV1::OpenSettings);
    }
    if target == "modal/pause/save-quit" {
        return Some(SurfaceCommandV1::RequestSaveQuit);
    }
    if target == "modal/confirm-save-quit/confirm" || action == SemanticAction::Confirm {
        return Some(SurfaceCommandV1::Confirm);
    }
    if target.ends_with("/cancel")
        || matches!(action, SemanticAction::Cancel | SemanticAction::Back)
    {
        return Some(if action == SemanticAction::Back {
            SurfaceCommandV1::Back
        } else {
            SurfaceCommandV1::Cancel
        });
    }
    if target == "settings/back" {
        return Some(SurfaceCommandV1::Back);
    }
    if target.starts_with("settings/controls/") && action == SemanticAction::Activate {
        return Some(SurfaceCommandV1::OpenBindingCapture {
            row: match SemanticKey::new(target) {
                Ok(row) => row,
                Err(_) => return None,
            },
        });
    }
    None
}

fn project_game(
    overlay: GameOverlayV1,
    modal: GameModalV1,
    transition: GameTransitionV1,
    save_quit: Option<SaveQuitProjection>,
    presentation: &GamePresentationV1,
    focused: Option<&SemanticKey>,
) -> Result<Vec<SemanticNode>, SurfaceSessionError> {
    if transition == GameTransitionV1::SavingAndExiting {
        let projection = save_quit.unwrap_or(SaveQuitProjection::Saving);
        return Ok(vec![SavingViewV1 { projection }.semantic_node()]);
    }
    if transition == GameTransitionV1::FatalRecovery {
        return Ok(vec![
            FatalRecoveryViewV1 {
                diagnostic: "bounded recovery".to_owned(),
            }
            .semantic_node(),
        ]);
    }
    let overlay_open = matches!(overlay, GameOverlayV1::Inventory | GameOverlayV1::Workbench);
    let mut children = presentation
        .hud
        .semantic_children(overlay_open && modal == GameModalV1::None, focused)?;
    if modal == GameModalV1::None {
        match overlay {
            GameOverlayV1::Inventory => {
                children.push(presentation.inventory.semantic_node(focused, true)?);
                children.push(
                    presentation
                        .hand_recipes
                        .semantic_node(false, focused, true)?,
                );
            }
            GameOverlayV1::Workbench => {
                children.push(presentation.workbench.semantic_node(focused, true)?);
            }
            GameOverlayV1::None => {}
        }
    } else {
        // Modal pauses overlay interaction; overlay draft is retained, not copied.
        match overlay {
            GameOverlayV1::Inventory => {
                children.push(presentation.inventory.semantic_node(None, false)?);
            }
            GameOverlayV1::Workbench => {
                children.push(presentation.workbench.semantic_node(None, false)?);
            }
            GameOverlayV1::None => {}
        }
        match modal {
            GameModalV1::Pause => children.push(
                PauseViewV1 {
                    writer_open: presentation.writer_open,
                }
                .semantic_node(focused),
            ),
            GameModalV1::Settings => {
                if let Some(fragment) = &presentation.settings_fragment {
                    children.push(fragment.clone());
                } else {
                    children.push(
                        crate::widgets::ButtonWidget::new(
                            surface_key("modal/settings"),
                            "Settings",
                            Some("Typed settings surface".to_owned()),
                            true,
                        )
                        .semantic_node(focused == Some(&surface_key("modal/settings"))),
                    );
                }
            }
            GameModalV1::BindingCapture => {
                if let Some(session) = &presentation.capture {
                    children.push(BindingCaptureViewV1 { session }.semantic_node(focused));
                }
            }
            GameModalV1::ConfirmSaveQuit => {
                children.push(ConfirmSaveQuitViewV1::semantic_node(focused));
            }
            GameModalV1::None => {}
        }
    }
    Ok(children)
}

/// Shell presentation models.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ShellPresentationV1 {
    /// Home continue/review.
    pub home: HomeViewV1,
    /// New-world committed name.
    pub new_world: NewWorldViewV1,
    /// World catalog.
    pub worlds: WorldsViewV1,
    /// Managed-trash recovery.
    pub recovery: RecoveryViewV1,
    /// Loading stage.
    pub loading: LoadingViewV1,
    /// Diagnostics summary.
    pub diagnostics: DiagnosticsAboutViewV1,
    /// Optional settings fragment.
    pub settings_fragment: Option<SemanticNode>,
}

impl Default for ShellPresentationV1 {
    fn default() -> Self {
        Self {
            home: HomeViewV1 {
                continue_action: crate::HomeContinueV1::None,
            },
            new_world: NewWorldViewV1 {
                name: String::new(),
            },
            worlds: WorldsViewV1::default(),
            recovery: RecoveryViewV1::default(),
            loading: LoadingViewV1 {
                stage: "Checking world".to_owned(),
                progress: "indeterminate".to_owned(),
                current_item: None,
                writer_opened: false,
            },
            diagnostics: DiagnosticsAboutViewV1 {
                summary: "Lattice Axiom".to_owned(),
            },
            settings_fragment: None,
        }
    }
}

/// Unique shell-process owner of route, focus, context, and projection.
#[derive(Clone, Debug)]
pub struct ShellSurfaceSession {
    router: ShellSurfaceRouter,
    focus: FocusController,
    snapshot: SemanticSnapshot,
    presentation: ShellPresentationV1,
}

impl ShellSurfaceSession {
    /// Home session.
    ///
    /// # Errors
    ///
    /// Returns [`SurfaceSessionError`] when projection fails.
    pub fn home() -> Result<Self, SurfaceSessionError> {
        Self::new(ShellPresentationV1::default())
    }

    /// Home session with caller presentation.
    ///
    /// # Errors
    ///
    /// Returns [`SurfaceSessionError`] when projection fails.
    pub fn new(presentation: ShellPresentationV1) -> Result<Self, SurfaceSessionError> {
        let router = ShellSurfaceRouter::new();
        let mut session = Self {
            router,
            focus: FocusController::new(FocusOwner::new(
                SurfaceEpoch::FIRST,
                surface_key("home/worlds"),
            )),
            snapshot: SemanticSnapshot::new(
                SurfaceEpoch::FIRST,
                application_root(surface_key("shell"), "Lattice Axiom", Vec::new())?,
            ),
            presentation,
        };
        session.rebuild()?;
        Ok(session)
    }

    /// Returns the live router.
    #[must_use]
    pub const fn router(&self) -> &ShellSurfaceRouter {
        &self.router
    }

    /// Returns the unique focus owner.
    #[must_use]
    pub const fn focus(&self) -> &FocusOwner {
        self.focus.owner()
    }

    /// Returns the live snapshot.
    #[must_use]
    pub const fn snapshot(&self) -> &SemanticSnapshot {
        &self.snapshot
    }

    /// Exclusive shell surface context. This is not a game route.
    #[must_use]
    pub fn context(&self) -> ActiveInputContextStack {
        ActiveInputContextStack::shell_surface()
    }

    /// Returns presentation.
    #[must_use]
    pub const fn presentation(&self) -> &ShellPresentationV1 {
        &self.presentation
    }

    /// Returns mutable presentation.
    pub const fn presentation_mut(&mut self) -> &mut ShellPresentationV1 {
        &mut self.presentation
    }

    /// Applies a typed shell command.
    ///
    /// # Errors
    ///
    /// Returns [`SurfaceSessionError`] for an illegal transition.
    pub fn apply(
        &mut self,
        command: &SurfaceCommandV1,
    ) -> Result<ShellApplyReceipt, SurfaceSessionError> {
        let receipt = self.router.apply(command)?;
        self.focus = FocusController::new(FocusOwner::new(
            receipt.epoch,
            default_shell_focus(receipt.route),
        ));
        self.rebuild()?;
        Ok(receipt)
    }

    /// Injects a semantic command from keyboard, mouse, gamepad, or headless.
    ///
    /// # Errors
    ///
    /// Returns [`SurfaceSessionError`] when the command is illegal.
    pub fn inject(
        &mut self,
        command: &SemanticCommand,
    ) -> Result<SurfaceInjectEffect, SurfaceSessionError> {
        validate_semantic_command(self.snapshot.root(), command)?;
        if matches!(
            command.action,
            SemanticAction::FocusNext | SemanticAction::FocusPrevious
        ) {
            let direction = if command.action == SemanticAction::FocusNext {
                LinearFocusDirection::Next
            } else {
                LinearFocusDirection::Previous
            };
            self.focus.move_linear(self.snapshot.root(), direction)?;
            self.rebuild()?;
            return Ok(SurfaceInjectEffect::FocusMoved);
        }
        let mapped =
            map_shell_command(self.router.route(), command.target.as_str(), command.action);
        let Some(mapped) = mapped else {
            return Err(SurfaceSessionError::UnmappedCommand);
        };
        match mapped {
            MappedShell::Route(command) => {
                let receipt = self.apply(&command)?;
                Ok(SurfaceInjectEffect::ShellRoute(receipt))
            }
            MappedShell::QuickCreate => Ok(SurfaceInjectEffect::QuickCreate),
            MappedShell::Continue => Ok(SurfaceInjectEffect::Continue),
            MappedShell::Review => Ok(SurfaceInjectEffect::Review),
            MappedShell::QuitProduct => Ok(SurfaceInjectEffect::QuitProduct),
        }
    }

    /// Rejects a stale async payload.
    ///
    /// # Errors
    ///
    /// Returns [`ProjectionError::StaleEpoch`] when `payload_epoch` is not live.
    pub fn accept_async(&self, payload_epoch: SurfaceEpoch) -> Result<(), ProjectionError> {
        accept_async_epoch(self.router.epoch(), payload_epoch)
    }

    /// Rebuilds the tree after presentation edits.
    ///
    /// # Errors
    ///
    /// Returns [`SurfaceSessionError`] if projection fails.
    pub fn refresh_projection(&mut self) -> Result<(), SurfaceSessionError> {
        self.rebuild()
    }

    fn rebuild(&mut self) -> Result<(), SurfaceSessionError> {
        if self
            .presentation
            .settings_fragment
            .as_ref()
            .is_some_and(|node| {
                node.role == crate::SemanticRole::Application || node.has_second_application_root()
            })
        {
            return Err(SurfaceSessionError::SecondUiRoot);
        }
        let focused = self.focus.owner().key().cloned();
        let child = project_shell(
            self.router.route(),
            self.router.writer_opened(),
            &self.presentation,
            focused.as_ref(),
        )?;
        let root = application_root(surface_key("shell"), "Lattice Axiom", vec![child])?;
        let next = SemanticSnapshot::new(self.router.epoch(), root);
        let _ = diff_snapshots(Some(&self.snapshot), &next)?;
        let _ = self.focus.restore(next.root(), self.router.epoch());
        self.snapshot = next;
        Ok(())
    }
}

enum MappedShell {
    Route(SurfaceCommandV1),
    QuickCreate,
    Continue,
    Review,
    QuitProduct,
}

fn map_shell_command(
    route: ShellRouteV1,
    target: &str,
    action: SemanticAction,
) -> Option<MappedShell> {
    if matches!(action, SemanticAction::Back | SemanticAction::Cancel) {
        return Some(MappedShell::Route(SurfaceCommandV1::Back));
    }
    match (route, target) {
        (_, "home/worlds") => Some(MappedShell::Route(SurfaceCommandV1::OpenShell(
            ShellRouteV1::Worlds,
        ))),
        (_, "home/new-world") => Some(MappedShell::Route(SurfaceCommandV1::OpenShell(
            ShellRouteV1::NewWorld,
        ))),
        (_, "home/packages-profiles") => Some(MappedShell::Route(SurfaceCommandV1::OpenShell(
            ShellRouteV1::PackagesProfiles,
        ))),
        (_, "home/settings") => Some(MappedShell::Route(SurfaceCommandV1::OpenSettings)),
        (_, "home/diagnostics-about") => Some(MappedShell::Route(SurfaceCommandV1::OpenShell(
            ShellRouteV1::DiagnosticsAbout,
        ))),
        (_, "home/quit") => Some(MappedShell::Route(SurfaceCommandV1::OpenShell(
            ShellRouteV1::QuitConfirm,
        ))),
        (_, "home/continue") => Some(MappedShell::Continue),
        (_, "home/review") => Some(MappedShell::Review),
        (_, "new-world/quick-create") => Some(MappedShell::QuickCreate),
        (_, "worlds/trash") => Some(MappedShell::Route(SurfaceCommandV1::OpenShell(
            ShellRouteV1::Recovery,
        ))),
        (_, "modal/quit/confirm") => Some(MappedShell::QuitProduct),
        (ShellRouteV1::Loading, "loading/cancel") => {
            Some(MappedShell::Route(SurfaceCommandV1::Cancel))
        }
        _ if target.ends_with("/back") || target.ends_with("/cancel") => {
            Some(MappedShell::Route(SurfaceCommandV1::Back))
        }
        _ => None,
    }
}

fn default_shell_focus(route: ShellRouteV1) -> SemanticKey {
    surface_key(match route {
        ShellRouteV1::Home => "home/worlds",
        ShellRouteV1::Worlds => "worlds/back",
        ShellRouteV1::NewWorld => "new-world/name",
        ShellRouteV1::PackagesProfiles => "packages-profiles/back",
        ShellRouteV1::Settings => "settings/back",
        ShellRouteV1::DiagnosticsAbout => "diagnostics-about/back",
        ShellRouteV1::Loading => "loading/cancel",
        ShellRouteV1::Recovery => "recovery/back",
        ShellRouteV1::QuitConfirm => "modal/quit/confirm",
    })
}

fn project_shell(
    route: ShellRouteV1,
    writer_opened: bool,
    presentation: &ShellPresentationV1,
    focused: Option<&SemanticKey>,
) -> Result<SemanticNode, SurfaceSessionError> {
    Ok(match route {
        ShellRouteV1::Home => presentation.home.semantic_node(focused),
        ShellRouteV1::Worlds => presentation.worlds.semantic_node(focused)?,
        ShellRouteV1::NewWorld => presentation.new_world.semantic_node(focused),
        ShellRouteV1::PackagesProfiles => PackagesProfilesViewV1::semantic_node(focused),
        ShellRouteV1::Settings => presentation.settings_fragment.clone().unwrap_or_else(|| {
            crate::widgets::ButtonWidget::new(
                surface_key("settings/back"),
                "Back",
                Some("Return to home".to_owned()),
                true,
            )
            .semantic_node(focused == Some(&surface_key("settings/back")))
        }),
        ShellRouteV1::DiagnosticsAbout => presentation.diagnostics.semantic_node(focused),
        ShellRouteV1::Loading => {
            let mut loading = presentation.loading.clone();
            loading.writer_opened = writer_opened;
            loading.semantic_node(focused)
        }
        ShellRouteV1::Recovery => presentation.recovery.semantic_node(focused)?,
        ShellRouteV1::QuitConfirm => QuitConfirmViewV1::semantic_node(focused),
    })
}

/// Result of an accepted semantic injection.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SurfaceInjectEffect {
    /// Route changed.
    Route(GameApplyReceipt),
    /// Shell route changed.
    ShellRoute(ShellApplyReceipt),
    /// Focus moved inside the current tree.
    FocusMoved,
    /// Inventory presentation latch or move intent.
    InventoryClick(InventoryClickV1),
    /// Recipe activation. Host submits the catalog command.
    Craft {
        /// Semantic recipe key.
        recipe: String,
    },
    /// Quick-create intent should be published.
    QuickCreate,
    /// Continue the exact-ready world.
    Continue,
    /// Review a non-exact world.
    Review,
    /// Confirm product quit without a launch intent.
    QuitProduct,
}

/// Surface session failure.
#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum SurfaceSessionError {
    /// Router rejected the command.
    #[error(transparent)]
    Router(#[from] SurfaceRouterError),
    /// Semantic command was rejected by the live tree.
    #[error(transparent)]
    Semantic(#[from] SemanticCommandError),
    /// Projection rejected a stale or duplicate tree.
    #[error(transparent)]
    Projection(#[from] ProjectionError),
    /// Focus traversal was empty.
    #[error(transparent)]
    Focus(#[from] crate::FocusError),
    /// Generated semantic key was invalid.
    #[error(transparent)]
    SemanticKey(#[from] crate::SemanticKeyError),
    /// A second application root was supplied.
    #[error(transparent)]
    UiRoot(#[from] UiRootError),
    /// Settings fragment attempted a second UI root.
    #[error("surface session cannot host a second application root")]
    SecondUiRoot,
    /// Advertised command has no mapping on the current route.
    #[error("semantic command is not mapped on the current route")]
    UnmappedCommand,
}

/// Keyboard, mouse, and gamepad share this constructor.
#[must_use]
pub fn surface_command(
    target: SemanticKey,
    action: SemanticAction,
    source: InputSource,
) -> SemanticCommand {
    SemanticCommand {
        target,
        action,
        source,
    }
}
