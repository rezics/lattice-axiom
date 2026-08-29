//! Backend-confirmed primary-window focus for relative mouse capture.

use bevy::{
    prelude::{Entity, MessageReader, Query, ResMut, Resource, With},
    window::{PrimaryWindow, WindowFocused},
};

/// Focus state proved by a backend [`WindowFocused`] message.
///
/// Bevy initializes [`bevy::window::Window::focused`] to `true` before the
/// platform window reports its actual state. Keeping the unconfirmed state
/// distinct prevents a newly launched client from capturing the cursor while
/// another application still owns focus.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Resource)]
pub(crate) enum ConfirmedPrimaryWindowFocus {
    /// No focus message has been observed for the current primary window.
    #[default]
    Unconfirmed,
    /// The backend confirmed that this primary window owns focus.
    Focused(Entity),
    /// The backend confirmed that this primary window does not own focus.
    Unfocused(Entity),
}

impl ConfirmedPrimaryWindowFocus {
    pub(crate) fn is_focused(self, window: Entity) -> bool {
        matches!(self, Self::Focused(confirmed) if confirmed == window)
    }

    fn belongs_to(self, window: Entity) -> bool {
        match self {
            Self::Unconfirmed => true,
            Self::Focused(confirmed) | Self::Unfocused(confirmed) => confirmed == window,
        }
    }

    fn record(&mut self, window: Entity, focused: bool) {
        *self = if focused {
            Self::Focused(window)
        } else {
            Self::Unfocused(window)
        };
    }
}

/// Records only backend focus messages for the current primary window.
#[allow(clippy::needless_pass_by_value)] // Bevy systems receive SystemParams by value.
pub(crate) fn observe_primary_window_focus(
    mut confirmed: ResMut<'_, ConfirmedPrimaryWindowFocus>,
    primary_window: Query<'_, '_, Entity, With<PrimaryWindow>>,
    mut focus_events: MessageReader<'_, '_, WindowFocused>,
) {
    let Ok(primary_window) = primary_window.single() else {
        *confirmed = ConfirmedPrimaryWindowFocus::Unconfirmed;
        return;
    };
    if !confirmed.belongs_to(primary_window) {
        *confirmed = ConfirmedPrimaryWindowFocus::Unconfirmed;
    }
    for event in focus_events.read() {
        if event.window == primary_window {
            confirmed.record(primary_window, event.focused);
        }
    }
}

#[cfg(test)]
mod tests {
    use bevy::{
        app::{App, Update},
        window::{PrimaryWindow, Window, WindowFocused},
    };

    use super::{ConfirmedPrimaryWindowFocus, observe_primary_window_focus};

    #[test]
    fn cursor_focus_waits_for_the_backend_and_tracks_focus_loss() {
        let mut app = App::new();
        app.add_message::<WindowFocused>()
            .init_resource::<ConfirmedPrimaryWindowFocus>()
            .add_systems(Update, observe_primary_window_focus);
        let window = app
            .world_mut()
            .spawn((Window::default(), PrimaryWindow))
            .id();

        app.update();
        assert!(
            app.world()
                .entity(window)
                .get::<Window>()
                .is_some_and(|window| window.focused)
        );
        assert!(
            !app.world()
                .resource::<ConfirmedPrimaryWindowFocus>()
                .is_focused(window)
        );

        app.world_mut().write_message(WindowFocused {
            window,
            focused: true,
        });
        app.update();
        assert!(
            app.world()
                .resource::<ConfirmedPrimaryWindowFocus>()
                .is_focused(window)
        );

        app.world_mut().write_message(WindowFocused {
            window,
            focused: false,
        });
        app.update();
        assert!(
            !app.world()
                .resource::<ConfirmedPrimaryWindowFocus>()
                .is_focused(window)
        );
    }
}
