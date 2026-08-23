//! Game surface router that owns pause, HUD overlays, cursor, and suppression.

use bevy::prelude::Resource;
#[cfg(feature = "client")]
use bevy::prelude::{Query, Res, ResMut, With};
#[cfg(feature = "client")]
use latticeaxiom_client_ui::{CursorPolicy, GameModalV1, GameOverlayV1, GameTransitionV1};
use latticeaxiom_client_ui::{GameSurfaceRouter, ROUTE_VOCABULARY_MAJOR, SurfaceCommandV1};
#[cfg(feature = "client")]
use latticeaxiom_input::ClientSurfaceActionV1;
#[cfg(feature = "client")]
use latticeaxiom_player::{
    ActionState, GameplaySuppressed, LeafwingPlayerAction, LocalPlayerInput, SurfaceActionFrame,
};

#[cfg(feature = "client")]
use super::{ProductionSessionPause, hud::ProductionHudSurfaces};

/// Exactly-one game-process surface router.
#[derive(Debug, Resource)]
pub struct ProductionSurfaceRouter {
    inner: GameSurfaceRouter,
}

impl ProductionSurfaceRouter {
    /// Playing router at vocabulary major 1.
    ///
    /// # Errors
    ///
    /// Returns the client-ui router error when the vocabulary major is unsupported.
    pub fn playing() -> Result<Self, latticeaxiom_client_ui::SurfaceRouterError> {
        Ok(Self {
            inner: GameSurfaceRouter::new(ROUTE_VOCABULARY_MAJOR)?,
        })
    }

    /// Returns the live router.
    #[must_use]
    pub const fn inner(&self) -> &GameSurfaceRouter {
        &self.inner
    }

    /// Applies a typed surface command.
    ///
    /// # Errors
    ///
    /// Returns the client-ui error for an illegal transition.
    pub fn apply(
        &mut self,
        command: &SurfaceCommandV1,
    ) -> Result<latticeaxiom_client_ui::GameApplyReceipt, latticeaxiom_client_ui::SurfaceRouterError>
    {
        self.inner.apply(command)
    }
}

#[cfg(feature = "client")]
#[allow(clippy::needless_pass_by_value)] // Bevy systems receive SystemParams by value.
pub(super) fn apply_surface_actions(
    action_states: Query<'_, '_, &ActionState<LeafwingPlayerAction>, With<LocalPlayerInput>>,
    mut router: ResMut<'_, ProductionSurfaceRouter>,
    mut pause: ResMut<'_, ProductionSessionPause>,
    mut surfaces: ResMut<'_, ProductionHudSurfaces>,
    mut suppressed: ResMut<'_, GameplaySuppressed>,
    mut frame: ResMut<'_, SurfaceActionFrame>,
    spine: Res<'_, super::ProductionSpine>,
) {
    let gameplay_pause_started = action_states
        .iter()
        .any(|state| state.just_pressed(&LeafwingPlayerAction::Pause));
    let started = frame.started().clone();
    for action in started {
        let command = surface_command(
            router.inner().route().modal(),
            action,
            gameplay_pause_started,
        );
        let Some(command) = command else {
            continue;
        };
        if let Ok(receipt) = router.apply(&command) {
            if receipt.route.overlay() == GameOverlayV1::Workbench {
                let Ok(workstation) = latticeaxiom_gameplay::WorkstationId::parse(
                    "latticeaxiom:workstation/crafting@1",
                ) else {
                    continue;
                };
                let _ =
                    spine.bind_workstation(workstation, latticeaxiom_gameplay::ContainerId::new(1));
            }
            sync_derived_state(&receipt, &mut pause, &mut surfaces, &mut suppressed);
            frame.clear();
        }
    }
}

#[cfg(feature = "client")]
fn surface_command(
    modal: GameModalV1,
    action: ClientSurfaceActionV1,
    gameplay_pause_started: bool,
) -> Option<SurfaceCommandV1> {
    match action {
        ClientSurfaceActionV1::ToggleInventory => Some(SurfaceCommandV1::ToggleInventory),
        ClientSurfaceActionV1::ToggleWorkbench => Some(SurfaceCommandV1::OpenWorkbench),
        ClientSurfaceActionV1::Pause => {
            if modal == GameModalV1::None {
                Some(SurfaceCommandV1::Pause)
            } else {
                Some(SurfaceCommandV1::Back)
            }
        }
        // Escape is intentionally shared across gameplay Pause and surface
        // Back. The pause edge owns that physical press so Back cannot undo
        // the modal transition later in the same frame.
        ClientSurfaceActionV1::Back if gameplay_pause_started => None,
        ClientSurfaceActionV1::Back => Some(SurfaceCommandV1::Back),
        _ => None,
    }
}

#[cfg(feature = "client")]
fn sync_derived_state(
    receipt: &latticeaxiom_client_ui::GameApplyReceipt,
    pause: &mut ProductionSessionPause,
    surfaces: &mut ProductionHudSurfaces,
    suppressed: &mut GameplaySuppressed,
) {
    let route = &receipt.route;
    pause.set(
        matches!(
            route.modal(),
            GameModalV1::Pause | GameModalV1::Settings | GameModalV1::ConfirmSaveQuit
        ) || route.transition() != GameTransitionV1::None,
    );
    match route.overlay() {
        GameOverlayV1::Inventory => surfaces.set_inventory_open(true),
        GameOverlayV1::Workbench => surfaces.set_workbench_open(true),
        GameOverlayV1::None => surfaces.set_inventory_open(false),
    }
    suppressed.0 = receipt.gameplay_suppressed;
}

#[cfg(feature = "client")]
#[allow(clippy::needless_pass_by_value)] // Bevy systems receive SystemParams by value.
pub(super) fn select_hotbar_from_surface(
    pause: Res<'_, ProductionSessionPause>,
    frame: Res<'_, SurfaceActionFrame>,
    spine: Res<'_, super::ProductionSpine>,
) {
    if pause.is_paused() {
        return;
    }
    for (action, slot) in [
        (ClientSurfaceActionV1::HotbarSlot1, 0_u16),
        (ClientSurfaceActionV1::HotbarSlot2, 1),
        (ClientSurfaceActionV1::HotbarSlot3, 2),
        (ClientSurfaceActionV1::HotbarSlot4, 3),
        (ClientSurfaceActionV1::HotbarSlot5, 4),
        (ClientSurfaceActionV1::HotbarSlot6, 5),
        (ClientSurfaceActionV1::HotbarSlot7, 6),
        (ClientSurfaceActionV1::HotbarSlot8, 7),
        (ClientSurfaceActionV1::HotbarSlot9, 8),
    ] {
        if frame.just_started(action) {
            let _ = spine.select_hotbar_slot(slot);
            return;
        }
    }
}

/// Cursor policy derived from the live router.
#[cfg(feature = "client")]
#[must_use]
pub fn cursor_locked(router: &ProductionSurfaceRouter) -> bool {
    router.inner().route().input_context().cursor_policy() == CursorPolicy::LockedGameplay
}

#[cfg(all(test, feature = "client"))]
mod tests {
    use super::*;

    #[test]
    fn gameplay_pause_edge_wins_over_the_same_escape_surface_back_edge() {
        assert!(surface_command(GameModalV1::None, ClientSurfaceActionV1::Back, true,).is_none());
        assert!(surface_command(GameModalV1::Pause, ClientSurfaceActionV1::Back, true,).is_none());
        assert!(matches!(
            surface_command(GameModalV1::None, ClientSurfaceActionV1::Back, false,),
            Some(SurfaceCommandV1::Back)
        ));
    }
}
