//! Shared client-ui typed-route projection for the start shell.

use latticeaxiom_client_ui::{
    DiagnosticsAboutViewV1, HomeContinueV1, HomeViewV1, LoadingViewV1, NewWorldViewV1,
    PackagesProfilesViewV1, QuitConfirmViewV1, RecoveryViewV1, SemanticSnapshot, ShellRouteV1,
    ShellSurfaceRouter, SurfaceCommandV1, SurfaceEpoch, WorldsViewV1, application_root,
    surface_key,
};

use crate::{HomePrimaryAction, LoadingProgress, ShellScreen, StartShellModel};

impl StartShellModel {
    /// Maps the start-ui screen onto the shared shell route vocabulary.
    ///
    /// Playing and Pause belong to the game process router and return [`None`].
    #[must_use]
    pub const fn typed_shell_route(&self) -> Option<ShellRouteV1> {
        match self.screen {
            ShellScreen::Home => Some(ShellRouteV1::Home),
            ShellScreen::Worlds => Some(ShellRouteV1::Worlds),
            ShellScreen::NewWorld => Some(ShellRouteV1::NewWorld),
            ShellScreen::Settings => Some(ShellRouteV1::Settings),
            ShellScreen::Loading => Some(ShellRouteV1::Loading),
            ShellScreen::Trash => Some(ShellRouteV1::Recovery),
            ShellScreen::PackagesProfiles => Some(ShellRouteV1::PackagesProfiles),
            ShellScreen::DiagnosticsAbout => Some(ShellRouteV1::DiagnosticsAbout),
            ShellScreen::QuitConfirm => Some(ShellRouteV1::QuitConfirm),
            ShellScreen::Playing | ShellScreen::Pause => None,
        }
    }

    /// Projects Home through the shared client-ui widgets.
    ///
    /// # Errors
    ///
    /// Returns [`latticeaxiom_client_ui::UiRootError`] if a second application
    /// root would be created.
    pub fn typed_home_snapshot(
        &self,
    ) -> Result<SemanticSnapshot, latticeaxiom_client_ui::UiRootError> {
        let continue_action = match self.worlds.home_primary_action() {
            HomePrimaryAction::Continue { label, .. } => HomeContinueV1::Continue {
                label,
                summary: "ReadyExact; health, lock, and durability from the catalog".to_owned(),
            },
            HomePrimaryAction::Review { label, health, .. } => HomeContinueV1::Review {
                label,
                reason: format!("{health:?}"),
            },
            HomePrimaryAction::Worlds => HomeContinueV1::None,
        };
        let home = HomeViewV1 { continue_action }.semantic_node(None);
        let root = application_root(surface_key("shell"), "Lattice Axiom", vec![home])?;
        Ok(SemanticSnapshot::new(SurfaceEpoch::FIRST, root))
    }

    /// Projects loading through the shared client-ui widgets.
    ///
    /// # Errors
    ///
    /// Returns [`latticeaxiom_client_ui::UiRootError`] if a second application
    /// root would be created.
    pub fn typed_loading_snapshot(
        &self,
    ) -> Result<SemanticSnapshot, latticeaxiom_client_ui::UiRootError> {
        let (stage, progress, current_item, writer_opened) = self.loading.as_ref().map_or_else(
            || {
                (
                    "Loading".to_owned(),
                    "indeterminate".to_owned(),
                    None,
                    false,
                )
            },
            |loading| {
                (
                    loading.stage.fallback_label().to_owned(),
                    match loading.progress {
                        LoadingProgress::Indeterminate => "indeterminate".to_owned(),
                        LoadingProgress::Determinate { completed, total } => {
                            format!("{completed}/{total}")
                        }
                    },
                    loading.current_item.clone(),
                    matches!(
                        loading.cancel_disposition(),
                        crate::LoadingCancelDisposition::ShutdownBarrierRequired
                    ),
                )
            },
        );
        let view = LoadingViewV1 {
            stage,
            progress,
            current_item,
            writer_opened,
        }
        .semantic_node(None);
        let root = application_root(surface_key("shell"), "Lattice Axiom", vec![view])?;
        Ok(SemanticSnapshot::new(SurfaceEpoch::FIRST, root))
    }

    /// Returns a shell router parked on the mapped route without creating a game route.
    ///
    /// # Errors
    ///
    /// Returns [`latticeaxiom_client_ui::SurfaceRouterError`] for an illegal
    /// transition from Home.
    pub fn typed_router(
        &self,
    ) -> Result<ShellSurfaceRouter, latticeaxiom_client_ui::SurfaceRouterError> {
        let mut router = ShellSurfaceRouter::new();
        if let Some(route) = self.typed_shell_route()
            && route != ShellRouteV1::Home
        {
            if route == ShellRouteV1::Settings {
                router.apply(&SurfaceCommandV1::OpenSettings)?;
            } else {
                router.apply(&SurfaceCommandV1::OpenShell(route))?;
            }
        }
        Ok(router)
    }

    /// Projects the current shell screen through the shared client-ui widgets.
    ///
    /// Playing and Pause belong to the game process and return [`None`].
    /// Settings is owned by `@latticeaxiom/settings-ui` and also returns
    /// [`None`] so this crate does not grow a second settings root.
    ///
    /// # Errors
    ///
    /// Returns [`latticeaxiom_client_ui::UiRootError`] if a second application
    /// root would be created.
    pub fn typed_snapshot(
        &self,
    ) -> Result<Option<SemanticSnapshot>, latticeaxiom_client_ui::UiRootError> {
        let view = match self.typed_shell_route() {
            Some(ShellRouteV1::Home) => return self.typed_home_snapshot().map(Some),
            Some(ShellRouteV1::Loading) => return self.typed_loading_snapshot().map(Some),
            Some(ShellRouteV1::NewWorld) => typed_new_world(String::new()).semantic_node(None),
            Some(ShellRouteV1::Worlds) => {
                let Ok(view) = typed_worlds_view().semantic_node(None) else {
                    return Ok(None);
                };
                view
            }
            Some(ShellRouteV1::Recovery) => {
                let Ok(view) = RecoveryViewV1::default().semantic_node(None) else {
                    return Ok(None);
                };
                view
            }
            Some(ShellRouteV1::PackagesProfiles) => PackagesProfilesViewV1::semantic_node(None),
            Some(ShellRouteV1::DiagnosticsAbout) => DiagnosticsAboutViewV1 {
                summary: "Lattice Axiom".to_owned(),
            }
            .semantic_node(None),
            Some(ShellRouteV1::QuitConfirm) => QuitConfirmViewV1::semantic_node(None),
            Some(ShellRouteV1::Settings) | None => return Ok(None),
        };
        let root = application_root(surface_key("shell"), "Lattice Axiom", vec![view])?;
        Ok(Some(SemanticSnapshot::new(SurfaceEpoch::FIRST, root)))
    }
}

/// Shared new-world view using the client-ui text field and IME contract.
#[must_use]
pub fn typed_new_world(name: impl Into<String>) -> NewWorldViewV1 {
    NewWorldViewV1 { name: name.into() }
}

/// Shared worlds list projection.
#[must_use]
pub fn typed_worlds_view() -> WorldsViewV1 {
    WorldsViewV1::default()
}
