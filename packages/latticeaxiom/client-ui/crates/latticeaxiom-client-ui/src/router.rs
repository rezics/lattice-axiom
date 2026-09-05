//! Typed shell and game surface routers. Widgets never mutate these directly.

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::focus::FocusOwner;
use crate::semantic::SemanticKey;
use crate::surface::{
    ActiveInputContextStack, InputContextKind, MAX_ROUTE_DEPTH, ROUTE_VOCABULARY_MAJOR,
    SurfaceEpoch,
};

/// Game overlay occupying the middle route layer.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum GameOverlayV1 {
    /// No overlay.
    None,
    /// Inventory overlay.
    Inventory,
    /// Workbench overlay. Mutually exclusive with [`Self::Inventory`].
    Workbench,
}

/// Game modal occupying the top route layer.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum GameModalV1 {
    /// No modal.
    None,
    /// Pause.
    Pause,
    /// Settings. Game Back returns to Pause.
    Settings,
    /// Confirm Save & Quit.
    ConfirmSaveQuit,
    /// Binding capture, always a child of Settings.
    BindingCapture,
}

/// Lifecycle transition that closes interactive surfaces.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum GameTransitionV1 {
    /// No transition.
    None,
    /// Save & Quit is in progress.
    SavingAndExiting,
    /// Bounded recovery; supervisor starts the recovery shell after exit.
    FatalRecovery,
}

/// Save & Quit projection. Only Durable may return to the shell.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum SaveQuitProjection {
    /// Durability barrier is running.
    Saving,
    /// Writer reported Written; this is not a completed save.
    WrittenNotDurable,
    /// Durability barrier reported Durable; the child may exit.
    Durable,
    /// Shutdown timed out.
    Timeout,
    /// Storage failed; keep the latest recoverable state.
    StorageFailure,
}

/// Shell screens. Selecting a world does not create a game route.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ShellRouteV1 {
    /// Home.
    Home,
    /// World library.
    Worlds,
    /// New-world flow.
    NewWorld,
    /// Packages and profiles.
    PackagesProfiles,
    /// Settings.
    Settings,
    /// Diagnostics and about.
    DiagnosticsAbout,
    /// Loading before or after the writer opens.
    Loading,
    /// Recovery.
    Recovery,
    /// Quit confirmation.
    QuitConfirm,
}

/// Typed command that is the only legal way to change a route.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub enum SurfaceCommandV1 {
    /// Toggle inventory, closing workbench when switching.
    ToggleInventory,
    /// Open workbench, closing inventory when switching.
    OpenWorkbench,
    /// Open pause from playing.
    Pause,
    /// Resume from pause.
    Resume,
    /// Open settings from pause or the shell.
    OpenSettings,
    /// Open binding capture as a Settings child.
    OpenBindingCapture {
        /// Controls row to restore.
        row: SemanticKey,
    },
    /// Unwind one layer.
    Back,
    /// Request Save & Quit confirmation.
    RequestSaveQuit,
    /// Confirm the active modal.
    Confirm,
    /// Cancel the active modal or capture.
    Cancel,
    /// Writer reported Written. This is not Durable.
    AcknowledgeWritten,
    /// Durability barrier reported Durable.
    AcknowledgeDurable,
    /// Shutdown timed out.
    AcknowledgeSaveTimeout,
    /// Storage failed during Save & Quit.
    AcknowledgeStorageFailure,
    /// Enter fatal recovery.
    EnterFatalRecovery,
    /// Navigate the shell to a named screen.
    OpenShell(ShellRouteV1),
    /// Loading observed that a world writer opened.
    MarkWriterOpened,
}

/// Game route triple plus remembered parents for Back unwind.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GameSurfaceRoute {
    overlay: GameOverlayV1,
    modal: GameModalV1,
    transition: GameTransitionV1,
    save_quit: Option<SaveQuitProjection>,
    binding_row: Option<SemanticKey>,
}

impl GameSurfaceRoute {
    /// Playing with no overlay, modal, or transition.
    #[must_use]
    pub const fn playing() -> Self {
        Self {
            overlay: GameOverlayV1::None,
            modal: GameModalV1::None,
            transition: GameTransitionV1::None,
            save_quit: None,
            binding_row: None,
        }
    }

    /// Returns the overlay layer.
    #[must_use]
    pub const fn overlay(&self) -> GameOverlayV1 {
        self.overlay
    }

    /// Returns the modal layer.
    #[must_use]
    pub const fn modal(&self) -> GameModalV1 {
        self.modal
    }

    /// Returns the transition layer.
    #[must_use]
    pub const fn transition(&self) -> GameTransitionV1 {
        self.transition
    }

    /// Returns the Save & Quit projection.
    #[must_use]
    pub const fn save_quit(&self) -> Option<SaveQuitProjection> {
        self.save_quit
    }

    /// Returns the Controls row captured under Settings, if any.
    #[must_use]
    pub const fn binding_row(&self) -> Option<&SemanticKey> {
        self.binding_row.as_ref()
    }

    /// Returns the occupied stack depth in `1..=3`.
    #[must_use]
    pub fn depth(&self) -> u8 {
        if self.transition != GameTransitionV1::None {
            return 1;
        }
        let mut depth = 1_u8;
        if self.overlay != GameOverlayV1::None {
            depth = depth.saturating_add(1);
        }
        if self.modal != GameModalV1::None {
            depth = depth.saturating_add(1);
        }
        depth
    }

    /// Derives the input-context stack from this route.
    #[must_use]
    pub fn input_context(&self) -> ActiveInputContextStack {
        if self.transition != GameTransitionV1::None {
            return ActiveInputContextStack::from_layers(vec![InputContextKind::Surface]);
        }
        let mut layers = vec![InputContextKind::Gameplay];
        match self.modal {
            GameModalV1::None => {
                if matches!(
                    self.overlay,
                    GameOverlayV1::Inventory | GameOverlayV1::Workbench
                ) {
                    layers.push(InputContextKind::HudOverlay);
                }
            }
            GameModalV1::BindingCapture => {
                layers.push(InputContextKind::Surface);
                layers.push(InputContextKind::BindingCapture);
            }
            GameModalV1::Pause | GameModalV1::Settings | GameModalV1::ConfirmSaveQuit => {
                layers.push(InputContextKind::Surface);
            }
        }
        ActiveInputContextStack::from_layers(layers)
    }

    fn default_focus_key(&self) -> SemanticKey {
        let raw = match (self.transition, self.modal, self.overlay) {
            (GameTransitionV1::SavingAndExiting, _, _) => "transition/saving",
            (GameTransitionV1::FatalRecovery, _, _) => "transition/recovery",
            (GameTransitionV1::None, GameModalV1::BindingCapture, _) => "modal/settings/capture",
            (GameTransitionV1::None, GameModalV1::ConfirmSaveQuit, _) => {
                "modal/confirm-save-quit/confirm"
            }
            (GameTransitionV1::None, GameModalV1::Settings, _) => "modal/settings",
            (GameTransitionV1::None, GameModalV1::Pause, _) => "modal/pause/resume",
            (GameTransitionV1::None, GameModalV1::None, GameOverlayV1::Inventory) => {
                "overlay/inventory"
            }
            (GameTransitionV1::None, GameModalV1::None, GameOverlayV1::Workbench) => {
                "overlay/workbench"
            }
            (GameTransitionV1::None, GameModalV1::None, GameOverlayV1::None) => "game/playing",
        };
        match SemanticKey::new(raw) {
            Ok(key) => key,
            Err(error) => unreachable!("static game focus key `{raw}` is valid: {error}"),
        }
    }
}

/// Receipt published after one accepted transition.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GameApplyReceipt {
    /// New epoch.
    pub epoch: SurfaceEpoch,
    /// Accepted route.
    pub route: GameSurfaceRoute,
    /// Derived input-context stack.
    pub context: ActiveInputContextStack,
    /// Unique focus owner.
    pub focus: FocusOwner,
    /// Whether live gameplay is suppressed.
    pub gameplay_suppressed: bool,
    /// Epoch whose pressed/capture state must be cleared.
    pub cleared_pressed_epoch: SurfaceEpoch,
}

/// Game process surface router. There is exactly one per game process.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GameSurfaceRouter {
    vocabulary_major: u32,
    epoch: SurfaceEpoch,
    route: GameSurfaceRoute,
}

impl GameSurfaceRouter {
    /// Creates a playing router at epoch 1.
    ///
    /// # Errors
    ///
    /// Returns [`SurfaceRouterError::UnsupportedRouteMajor`] when the
    /// vocabulary major is not supported.
    pub fn new(vocabulary_major: u32) -> Result<Self, SurfaceRouterError> {
        if vocabulary_major != ROUTE_VOCABULARY_MAJOR {
            return Err(SurfaceRouterError::UnsupportedRouteMajor {
                requested: vocabulary_major,
                supported: ROUTE_VOCABULARY_MAJOR,
            });
        }
        Ok(Self {
            vocabulary_major,
            epoch: SurfaceEpoch::FIRST,
            route: GameSurfaceRoute::playing(),
        })
    }

    /// Returns the live epoch.
    #[must_use]
    pub const fn epoch(&self) -> SurfaceEpoch {
        self.epoch
    }

    /// Returns the live route.
    #[must_use]
    pub const fn route(&self) -> &GameSurfaceRoute {
        &self.route
    }

    /// Applies a typed command in one stage.
    ///
    /// # Errors
    ///
    /// Returns [`SurfaceRouterError`] for an illegal transition. The live route
    /// is unchanged.
    pub fn apply(
        &mut self,
        command: &SurfaceCommandV1,
    ) -> Result<GameApplyReceipt, SurfaceRouterError> {
        let next = apply_game_command(&self.route, command)?;
        if next.depth() > MAX_ROUTE_DEPTH {
            return Err(SurfaceRouterError::RouteDepthExceeded);
        }
        let previous_epoch = self.epoch;
        self.epoch = self.epoch.next();
        self.route = next;
        let context = self.route.input_context();
        let focus = FocusOwner::new(self.epoch, self.route.default_focus_key());
        Ok(GameApplyReceipt {
            epoch: self.epoch,
            route: self.route.clone(),
            gameplay_suppressed: context.suppresses_gameplay(),
            context,
            focus,
            cleared_pressed_epoch: previous_epoch,
        })
    }
}

fn apply_game_command(
    current: &GameSurfaceRoute,
    command: &SurfaceCommandV1,
) -> Result<GameSurfaceRoute, SurfaceRouterError> {
    if current.transition == GameTransitionV1::SavingAndExiting {
        return apply_saving_command(current, command);
    }
    if current.transition == GameTransitionV1::FatalRecovery {
        return Err(SurfaceRouterError::IllegalTransition);
    }
    match command {
        SurfaceCommandV1::ToggleInventory => toggle_overlay(current, GameOverlayV1::Inventory),
        SurfaceCommandV1::OpenWorkbench => toggle_overlay(current, GameOverlayV1::Workbench),
        SurfaceCommandV1::Pause => {
            require_playing_interactive(current)?;
            if current.modal != GameModalV1::None {
                return Err(SurfaceRouterError::IllegalTransition);
            }
            Ok(with_modal(current, GameModalV1::Pause))
        }
        SurfaceCommandV1::Resume | SurfaceCommandV1::Back
            if current.modal == GameModalV1::Pause =>
        {
            Ok(with_modal(current, GameModalV1::None))
        }
        SurfaceCommandV1::OpenSettings => {
            if current.modal != GameModalV1::Pause {
                return Err(SurfaceRouterError::IllegalTransition);
            }
            Ok(with_modal(current, GameModalV1::Settings))
        }
        SurfaceCommandV1::Back if current.modal == GameModalV1::Settings => {
            Ok(with_modal(current, GameModalV1::Pause))
        }
        SurfaceCommandV1::OpenBindingCapture { row } => {
            if current.modal != GameModalV1::Settings {
                return Err(SurfaceRouterError::IllegalTransition);
            }
            let mut next = with_modal(current, GameModalV1::BindingCapture);
            next.binding_row = Some(row.clone());
            Ok(next)
        }
        SurfaceCommandV1::Back | SurfaceCommandV1::Cancel
            if current.modal == GameModalV1::BindingCapture =>
        {
            let mut next = with_modal(current, GameModalV1::Settings);
            next.binding_row.clone_from(&current.binding_row);
            Ok(next)
        }
        SurfaceCommandV1::RequestSaveQuit => {
            if current.modal != GameModalV1::Pause {
                return Err(SurfaceRouterError::IllegalTransition);
            }
            Ok(with_modal(current, GameModalV1::ConfirmSaveQuit))
        }
        SurfaceCommandV1::Cancel | SurfaceCommandV1::Back
            if current.modal == GameModalV1::ConfirmSaveQuit =>
        {
            Ok(with_modal(current, GameModalV1::Pause))
        }
        SurfaceCommandV1::Confirm if current.modal == GameModalV1::ConfirmSaveQuit => {
            Ok(GameSurfaceRoute {
                overlay: GameOverlayV1::None,
                modal: GameModalV1::None,
                transition: GameTransitionV1::SavingAndExiting,
                save_quit: Some(SaveQuitProjection::Saving),
                binding_row: None,
            })
        }
        SurfaceCommandV1::EnterFatalRecovery => Ok(GameSurfaceRoute {
            overlay: GameOverlayV1::None,
            modal: GameModalV1::None,
            transition: GameTransitionV1::FatalRecovery,
            save_quit: current.save_quit,
            binding_row: None,
        }),
        SurfaceCommandV1::Back if current.modal == GameModalV1::None => match current.overlay {
            GameOverlayV1::None => Err(SurfaceRouterError::IllegalTransition),
            GameOverlayV1::Inventory | GameOverlayV1::Workbench => {
                let mut next = current.clone();
                next.overlay = GameOverlayV1::None;
                Ok(next)
            }
        },
        _ => Err(SurfaceRouterError::IllegalTransition),
    }
}

fn apply_saving_command(
    current: &GameSurfaceRoute,
    command: &SurfaceCommandV1,
) -> Result<GameSurfaceRoute, SurfaceRouterError> {
    let mut next = current.clone();
    next.save_quit = Some(match command {
        SurfaceCommandV1::AcknowledgeWritten => SaveQuitProjection::WrittenNotDurable,
        SurfaceCommandV1::AcknowledgeDurable => SaveQuitProjection::Durable,
        SurfaceCommandV1::AcknowledgeSaveTimeout => SaveQuitProjection::Timeout,
        SurfaceCommandV1::AcknowledgeStorageFailure => SaveQuitProjection::StorageFailure,
        SurfaceCommandV1::EnterFatalRecovery => {
            next.transition = GameTransitionV1::FatalRecovery;
            return Ok(next);
        }
        _ => return Err(SurfaceRouterError::IllegalTransition),
    });
    if matches!(
        next.save_quit,
        Some(SaveQuitProjection::Timeout | SaveQuitProjection::StorageFailure)
    ) {
        next.transition = GameTransitionV1::FatalRecovery;
    }
    Ok(next)
}

fn require_playing_interactive(current: &GameSurfaceRoute) -> Result<(), SurfaceRouterError> {
    if current.transition == GameTransitionV1::None {
        Ok(())
    } else {
        Err(SurfaceRouterError::IllegalTransition)
    }
}

fn toggle_overlay(
    current: &GameSurfaceRoute,
    overlay: GameOverlayV1,
) -> Result<GameSurfaceRoute, SurfaceRouterError> {
    require_playing_interactive(current)?;
    if current.modal != GameModalV1::None {
        return Err(SurfaceRouterError::IllegalTransition);
    }
    let mut next = current.clone();
    next.overlay = if current.overlay == overlay {
        GameOverlayV1::None
    } else {
        overlay
    };
    Ok(next)
}

fn with_modal(current: &GameSurfaceRoute, modal: GameModalV1) -> GameSurfaceRoute {
    let mut next = current.clone();
    next.modal = modal;
    if modal != GameModalV1::BindingCapture {
        next.binding_row = None;
    }
    next
}

/// Shell process surface router. It never constructs a game route.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ShellSurfaceRouter {
    epoch: SurfaceEpoch,
    route: ShellRouteV1,
    previous: ShellRouteV1,
    writer_opened: bool,
}

impl ShellSurfaceRouter {
    /// Creates a Home router.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            epoch: SurfaceEpoch::FIRST,
            route: ShellRouteV1::Home,
            previous: ShellRouteV1::Home,
            writer_opened: false,
        }
    }

    /// Returns the live epoch.
    #[must_use]
    pub const fn epoch(&self) -> SurfaceEpoch {
        self.epoch
    }

    /// Returns the live shell route.
    #[must_use]
    pub const fn route(&self) -> ShellRouteV1 {
        self.route
    }

    /// Returns whether loading observed a writer open.
    #[must_use]
    pub const fn writer_opened(&self) -> bool {
        self.writer_opened
    }

    /// Applies a shell command.
    ///
    /// # Errors
    ///
    /// Returns [`SurfaceRouterError::IllegalTransition`] when the command is
    /// not legal for the current screen.
    pub fn apply(
        &mut self,
        command: &SurfaceCommandV1,
    ) -> Result<ShellApplyReceipt, SurfaceRouterError> {
        let (next, writer_opened) = apply_shell_command(self, command)?;
        self.previous = self.route;
        self.route = next;
        self.writer_opened = writer_opened;
        self.epoch = self.epoch.next();
        Ok(ShellApplyReceipt {
            epoch: self.epoch,
            route: self.route,
            context: ActiveInputContextStack::shell_surface(),
            writer_opened: self.writer_opened,
        })
    }
}

impl Default for ShellSurfaceRouter {
    fn default() -> Self {
        Self::new()
    }
}

/// Receipt for an accepted shell transition.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ShellApplyReceipt {
    /// New epoch.
    pub epoch: SurfaceEpoch,
    /// Accepted shell route.
    pub route: ShellRouteV1,
    /// Exclusive surface context. This is not a game route.
    pub context: ActiveInputContextStack,
    /// Whether a writer has opened during loading.
    pub writer_opened: bool,
}

fn apply_shell_command(
    current: &ShellSurfaceRouter,
    command: &SurfaceCommandV1,
) -> Result<(ShellRouteV1, bool), SurfaceRouterError> {
    match (current.route, command) {
        (ShellRouteV1::Loading, SurfaceCommandV1::MarkWriterOpened) => {
            Ok((ShellRouteV1::Loading, true))
        }
        (ShellRouteV1::Loading, SurfaceCommandV1::Cancel | SurfaceCommandV1::Back) => {
            if current.writer_opened {
                Err(SurfaceRouterError::WriterCancelRequiresShutdown)
            } else {
                Ok((ShellRouteV1::Home, false))
            }
        }
        (ShellRouteV1::Worlds, SurfaceCommandV1::OpenShell(ShellRouteV1::Recovery)) => {
            Ok((ShellRouteV1::Recovery, false))
        }
        (ShellRouteV1::Recovery, SurfaceCommandV1::OpenShell(ShellRouteV1::Home)) => {
            Ok((ShellRouteV1::Home, false))
        }
        (
            ShellRouteV1::Recovery,
            SurfaceCommandV1::OpenShell(ShellRouteV1::Worlds)
            | SurfaceCommandV1::Back
            | SurfaceCommandV1::Cancel,
        ) => Ok((ShellRouteV1::Worlds, false)),
        (ShellRouteV1::Home, SurfaceCommandV1::OpenSettings) => Ok((ShellRouteV1::Settings, false)),
        (ShellRouteV1::Home, SurfaceCommandV1::OpenShell(target)) => Ok((*target, false)),
        (
            ShellRouteV1::Worlds
            | ShellRouteV1::NewWorld
            | ShellRouteV1::PackagesProfiles
            | ShellRouteV1::Settings
            | ShellRouteV1::DiagnosticsAbout
            | ShellRouteV1::QuitConfirm,
            SurfaceCommandV1::Back | SurfaceCommandV1::Cancel,
        ) => Ok((ShellRouteV1::Home, false)),
        _ => Err(SurfaceRouterError::IllegalTransition),
    }
}

/// Illegal or unsupported surface-router command.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum SurfaceRouterError {
    /// Route vocabulary major is not supported before a Bevy surface is built.
    #[error("route vocabulary major {requested} is unsupported; crate supports {supported}")]
    UnsupportedRouteMajor {
        /// Requested major.
        requested: u32,
        /// Supported major.
        supported: u32,
    },
    /// Command is not legal for the current route.
    #[error("surface command is illegal for the current route")]
    IllegalTransition,
    /// Accepted command would exceed the three-layer stack.
    #[error("surface route depth exceeds the maximum of 3")]
    RouteDepthExceeded,
    /// Loading cancel after writer open must enter the shutdown barrier.
    #[error("loading cancel after writer open requires the shutdown barrier")]
    WriterCancelRequiresShutdown,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn router() -> GameSurfaceRouter {
        GameSurfaceRouter::new(1).expect("vocabulary")
    }

    #[test]
    fn unknown_route_major_fails_before_surface_construction() {
        assert_eq!(
            GameSurfaceRouter::new(2).err(),
            Some(SurfaceRouterError::UnsupportedRouteMajor {
                requested: 2,
                supported: 1,
            })
        );
    }

    #[test]
    fn settings_cannot_open_from_playing() {
        let mut router = router();
        assert_eq!(
            router.apply(&SurfaceCommandV1::OpenSettings),
            Err(SurfaceRouterError::IllegalTransition)
        );
        assert_eq!(router.route().modal(), GameModalV1::None);
    }

    #[test]
    fn inventory_pause_settings_capture_unwind_restores_overlay() {
        let mut router = router();
        router
            .apply(&SurfaceCommandV1::ToggleInventory)
            .expect("inventory");
        router.apply(&SurfaceCommandV1::Pause).expect("pause");
        router
            .apply(&SurfaceCommandV1::OpenSettings)
            .expect("settings");
        let row = SemanticKey::new("settings/controls/pause").expect("row");
        router
            .apply(&SurfaceCommandV1::OpenBindingCapture { row })
            .expect("capture");
        assert_eq!(router.route().modal(), GameModalV1::BindingCapture);
        assert_eq!(router.route().overlay(), GameOverlayV1::Inventory);
        assert!(router.route().input_context().suppresses_gameplay());
        router.apply(&SurfaceCommandV1::Back).expect("capture back");
        router
            .apply(&SurfaceCommandV1::Back)
            .expect("settings back");
        router.apply(&SurfaceCommandV1::Back).expect("pause back");
        assert_eq!(router.route().modal(), GameModalV1::None);
        assert_eq!(router.route().overlay(), GameOverlayV1::Inventory);
        assert_eq!(
            router.route().input_context().layers(),
            &[InputContextKind::Gameplay, InputContextKind::HudOverlay]
        );
    }

    #[test]
    fn saving_and_exiting_rejects_reopen_and_written_is_not_durable() {
        let mut router = router();
        router.apply(&SurfaceCommandV1::Pause).expect("pause");
        router
            .apply(&SurfaceCommandV1::RequestSaveQuit)
            .expect("confirm");
        router.apply(&SurfaceCommandV1::Confirm).expect("save");
        assert_eq!(
            router.apply(&SurfaceCommandV1::ToggleInventory),
            Err(SurfaceRouterError::IllegalTransition)
        );
        let written = router
            .apply(&SurfaceCommandV1::AcknowledgeWritten)
            .expect("written");
        assert_eq!(
            written.route.save_quit(),
            Some(SaveQuitProjection::WrittenNotDurable)
        );
        let durable = router
            .apply(&SurfaceCommandV1::AcknowledgeDurable)
            .expect("durable");
        assert_eq!(durable.route.save_quit(), Some(SaveQuitProjection::Durable));
    }

    #[test]
    fn shell_never_creates_a_game_route() {
        let mut shell = ShellSurfaceRouter::new();
        let receipt = shell
            .apply(&SurfaceCommandV1::OpenSettings)
            .expect("settings");
        assert_eq!(receipt.route, ShellRouteV1::Settings);
        assert_eq!(receipt.context.layers(), &[InputContextKind::Surface]);
        shell.apply(&SurfaceCommandV1::Back).expect("home");
        let loading = shell
            .apply(&SurfaceCommandV1::OpenShell(ShellRouteV1::Loading))
            .expect("loading");
        assert_eq!(loading.route, ShellRouteV1::Loading);
        shell
            .apply(&SurfaceCommandV1::MarkWriterOpened)
            .expect("writer");
        assert_eq!(
            shell.apply(&SurfaceCommandV1::Cancel),
            Err(SurfaceRouterError::WriterCancelRequiresShutdown)
        );
    }
}
