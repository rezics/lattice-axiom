//! Backend-confirmed primary-window focus for relative mouse capture.

use bevy::{
    ecs::system::NonSendMarker,
    input::{ButtonInput, keyboard::KeyCode, mouse::MouseButton},
    prelude::{Entity, MessageReader, Query, ResMut, Resource, With},
    window::{CursorGrabMode, CursorOptions, PrimaryWindow, Window, WindowFocused},
};
use bevy_winit::WINIT_WINDOWS;
use thiserror::Error;
use winit::{dpi::PhysicalPosition, window::CursorGrabMode as WinitCursorGrabMode};

/// Native failure while applying a requested cursor transition.
#[derive(Debug, Error)]
pub(crate) enum CursorCaptureBackendError {
    /// Bevy has not created the corresponding native window yet.
    #[error("native primary window is unavailable")]
    WindowUnavailable,
    /// The native window lost focus before capture could be proved.
    #[error("native primary window lost focus during cursor capture")]
    FocusLost,
    /// The native backend could not move the cursor to the viewport center.
    #[error("native cursor centering failed")]
    Center(#[source] winit::error::ExternalError),
    /// The native backend could not acquire relative cursor capture.
    #[error("native cursor capture failed")]
    Capture(#[source] winit::error::ExternalError),
    /// The native backend could not release relative cursor capture.
    #[error("native cursor release failed")]
    Release(#[source] winit::error::ExternalError),
}

/// Gesture-owned relative cursor-capture state.
///
/// Focus never grants capture by itself. A focused gameplay viewport must
/// observe a fresh primary-button press, center and lock the cursor, and then
/// wait for that capture press to be released before gameplay can consume it.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Resource)]
pub(crate) enum CursorCaptureState {
    /// The cursor is released and a new viewport gesture is required.
    #[default]
    ReleasedAwaitingGesture,
    /// A viewport gesture requested capture but the cursor has not been warped yet.
    CaptureRequested,
    /// The cursor is locked but the gesture that acquired it is still held.
    CapturedAwaitingRelease,
    /// The viewport owns relative mouse input.
    Captured,
    /// Gameplay is suppressed until the native backend confirms release.
    ReleaseRequested,
}

/// State of the primary-button gesture that may acquire cursor capture.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CursorCaptureGesture {
    /// The primary button is not held.
    Released,
    /// The primary button remains held from an earlier frame.
    Held,
    /// The primary button was freshly pressed this frame.
    JustPressed,
}

impl CursorCaptureGesture {
    /// Projects Bevy's button state into the capture state machine vocabulary.
    pub(crate) fn from_primary_button(mouse: &ButtonInput<MouseButton>) -> Self {
        if mouse.just_pressed(MouseButton::Left) {
            Self::JustPressed
        } else if mouse.pressed(MouseButton::Left) {
            Self::Held
        } else {
            Self::Released
        }
    }
}

impl CursorCaptureState {
    /// Reconciles backend focus, route ownership, and the primary capture gesture.
    pub(crate) fn reconcile(
        &mut self,
        capture_allowed: bool,
        gesture: CursorCaptureGesture,
        release_requested: bool,
    ) {
        if !capture_allowed || release_requested {
            self.request_release();
            return;
        }
        match *self {
            Self::ReleasedAwaitingGesture if gesture == CursorCaptureGesture::JustPressed => {
                *self = Self::CaptureRequested;
            }
            Self::CapturedAwaitingRelease if gesture == CursorCaptureGesture::Released => {
                *self = Self::Captured;
            }
            Self::ReleasedAwaitingGesture
            | Self::CaptureRequested
            | Self::CapturedAwaitingRelease
            | Self::Captured
            | Self::ReleaseRequested => {}
        }
    }

    /// Returns whether native capture has been requested but not acknowledged.
    pub(crate) const fn capture_pending(self) -> bool {
        matches!(self, Self::CaptureRequested)
    }

    /// Returns whether gameplay may consume local input.
    pub(crate) const fn owns_gameplay_input(self) -> bool {
        matches!(self, Self::Captured)
    }

    /// Returns whether native release has been requested but not acknowledged.
    pub(crate) const fn release_pending(self) -> bool {
        matches!(self, Self::ReleaseRequested)
    }

    fn mark_capture_applied(&mut self) {
        if self.capture_pending() {
            *self = Self::CapturedAwaitingRelease;
        }
    }

    fn request_release(&mut self) {
        if !matches!(self, Self::ReleasedAwaitingGesture) {
            *self = Self::ReleaseRequested;
        }
    }

    fn mark_release_applied(&mut self) {
        if self.release_pending() {
            *self = Self::ReleasedAwaitingGesture;
        }
    }
}

/// Applies one capture state to the native window and mirrors it into Bevy.
///
/// The state advances to an applied variant only after the native winit calls
/// return successfully. This keeps input ownership fail-closed when centering,
/// grabbing, or releasing fails at the operating-system boundary. The
/// [`NonSendMarker`] proof keeps access to winit's thread-local window table on
/// the event-loop thread that owns it.
pub(crate) fn apply_cursor_capture(
    capture: &mut CursorCaptureState,
    capture_allowed: bool,
    window_entity: Entity,
    window: &mut Window,
    cursor: &mut CursorOptions,
    _main_thread: &NonSendMarker,
) -> Result<(), CursorCaptureBackendError> {
    if !capture_allowed {
        capture.request_release();
    }

    if capture.capture_pending() {
        let native_result = capture_native_cursor(window_entity);
        if let Err(error) = native_result {
            capture.request_release();
            cursor.grab_mode = CursorGrabMode::None;
            cursor.visible = true;
            let _ = release_native_cursor(window_entity);
            return Err(error);
        }
        window.set_cursor_position(Some(window.size() * 0.5));
        cursor.grab_mode = CursorGrabMode::Locked;
        cursor.visible = false;
        capture.mark_capture_applied();
        return Ok(());
    }

    if capture.release_pending() {
        release_native_cursor(window_entity)?;
        cursor.grab_mode = CursorGrabMode::None;
        cursor.visible = true;
        capture.mark_release_applied();
        return Ok(());
    }

    if capture.owns_gameplay_input()
        || matches!(capture, CursorCaptureState::CapturedAwaitingRelease)
    {
        cursor.grab_mode = CursorGrabMode::Locked;
        cursor.visible = false;
    } else {
        cursor.grab_mode = CursorGrabMode::None;
        cursor.visible = true;
    }
    Ok(())
}

fn capture_native_cursor(window_entity: Entity) -> Result<(), CursorCaptureBackendError> {
    WINIT_WINDOWS.with_borrow(|windows| {
        let native = windows
            .get_window(window_entity)
            .ok_or(CursorCaptureBackendError::WindowUnavailable)?;
        if !native.has_focus() {
            return Err(CursorCaptureBackendError::FocusLost);
        }
        let size = native.inner_size();
        let center =
            PhysicalPosition::new(f64::from(size.width) * 0.5, f64::from(size.height) * 0.5);
        native
            .set_cursor_position(center)
            .map_err(CursorCaptureBackendError::Center)?;
        native
            .set_cursor_grab(WinitCursorGrabMode::Locked)
            .map_err(CursorCaptureBackendError::Capture)?;
        if !native.has_focus() {
            let _ = native.set_cursor_grab(WinitCursorGrabMode::None);
            return Err(CursorCaptureBackendError::FocusLost);
        }
        native.set_cursor_visible(false);
        Ok(())
    })
}

fn release_native_cursor(window_entity: Entity) -> Result<(), CursorCaptureBackendError> {
    WINIT_WINDOWS.with_borrow(|windows| {
        let native = windows
            .get_window(window_entity)
            .ok_or(CursorCaptureBackendError::WindowUnavailable)?;
        native
            .set_cursor_grab(WinitCursorGrabMode::None)
            .map_err(CursorCaptureBackendError::Release)?;
        native.set_cursor_visible(true);
        Ok(())
    })
}

/// Returns whether a platform screenshot gesture must release relative input.
pub(crate) fn screenshot_release_requested(keyboard: &ButtonInput<KeyCode>) -> bool {
    keyboard.any_just_pressed([
        KeyCode::PrintScreen,
        KeyCode::SuperLeft,
        KeyCode::SuperRight,
        KeyCode::AltLeft,
        KeyCode::AltRight,
    ])
}

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
        input::{ButtonInput, keyboard::KeyCode},
        window::{PrimaryWindow, Window, WindowFocused},
    };

    use super::{
        ConfirmedPrimaryWindowFocus, CursorCaptureGesture, CursorCaptureState,
        observe_primary_window_focus, screenshot_release_requested,
    };

    #[test]
    fn viewport_capture_centers_and_consumes_the_acquiring_click() {
        let mut capture = CursorCaptureState::default();
        capture.reconcile(true, CursorCaptureGesture::JustPressed, false);
        assert_eq!(capture, CursorCaptureState::CaptureRequested);
        assert!(capture.capture_pending());
        assert!(!capture.owns_gameplay_input());

        capture.mark_capture_applied();
        assert_eq!(capture, CursorCaptureState::CapturedAwaitingRelease);
        assert!(!capture.owns_gameplay_input());

        capture.reconcile(true, CursorCaptureGesture::Released, false);
        assert_eq!(capture, CursorCaptureState::Captured);
        assert!(capture.owns_gameplay_input());

        capture.reconcile(false, CursorCaptureGesture::Released, false);
        assert_eq!(capture, CursorCaptureState::ReleaseRequested);
        assert!(!capture.owns_gameplay_input());
        capture.mark_release_applied();
        assert_eq!(capture, CursorCaptureState::ReleasedAwaitingGesture);
    }

    #[test]
    fn screenshot_shortcuts_request_release() {
        let mut keyboard = ButtonInput::<KeyCode>::default();
        keyboard.press(KeyCode::PrintScreen);
        assert!(screenshot_release_requested(&keyboard));

        keyboard.reset_all();
        keyboard.press(KeyCode::SuperLeft);
        assert!(screenshot_release_requested(&keyboard));

        keyboard.reset_all();
        keyboard.press(KeyCode::AltLeft);
        assert!(screenshot_release_requested(&keyboard));
    }

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
