//! Independent client frame pacing and presentation-mode control.

use std::num::NonZeroU16;

use bevy::{
    app::{App, Plugin, Update},
    ecs::change_detection::DetectChanges,
    prelude::{Query, Res, ResMut, Resource, With},
    window::{PresentMode, PrimaryWindow, Window},
};
use bevy_framepace::{FramepacePlugin, FramepaceSettings, Limiter};
use thiserror::Error;

/// Lowest supported software frame cap.
pub const MIN_FRAME_RATE_LIMIT: u16 = 5;
/// Highest supported software frame cap.
pub const MAX_FRAME_RATE_LIMIT: u16 = 1_000;

/// Validated foreground or background render-frame limit.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FrameRateLimit {
    /// Render as quickly as the present mode and hardware permit.
    Unlimited,
    /// Apply a software frame pace at the validated frequency.
    Capped(NonZeroU16),
}

impl FrameRateLimit {
    /// Validates a requested software frame cap.
    ///
    /// # Errors
    ///
    /// Returns [`VideoSettingsError`] when `frames_per_second` is outside
    /// `5..=1_000`.
    pub const fn capped(frames_per_second: u16) -> Result<Self, VideoSettingsError> {
        if frames_per_second < MIN_FRAME_RATE_LIMIT || frames_per_second > MAX_FRAME_RATE_LIMIT {
            return Err(VideoSettingsError::FrameRateOutsideSupportedRange {
                requested: frames_per_second,
                minimum: MIN_FRAME_RATE_LIMIT,
                maximum: MAX_FRAME_RATE_LIMIT,
            });
        }
        match NonZeroU16::new(frames_per_second) {
            Some(nonzero) => Ok(Self::Capped(nonzero)),
            None => Err(VideoSettingsError::FrameRateOutsideSupportedRange {
                requested: frames_per_second,
                minimum: MIN_FRAME_RATE_LIMIT,
                maximum: MAX_FRAME_RATE_LIMIT,
            }),
        }
    }

    /// Parses the canonical settings-catalog representation.
    ///
    /// # Errors
    ///
    /// Returns [`VideoSettingsError`] for an unknown string or an invalid cap.
    pub fn parse(value: &str) -> Result<Self, VideoSettingsError> {
        if value == "unlimited" {
            return Ok(Self::Unlimited);
        }
        let frames_per_second =
            value
                .parse::<u16>()
                .map_err(|_| VideoSettingsError::UnknownFrameRateLimit {
                    value: value.to_owned(),
                })?;
        Self::capped(frames_per_second)
    }

    /// Returns the software cap, or `None` for unlimited rendering.
    #[must_use]
    pub const fn frames_per_second(self) -> Option<u16> {
        match self {
            Self::Unlimited => None,
            Self::Capped(value) => Some(value.get()),
        }
    }

    fn limiter(self) -> Limiter {
        match self {
            Self::Unlimited => Limiter::Off,
            Self::Capped(value) => Limiter::from_framerate(f64::from(value.get())),
        }
    }
}

/// Live video timing policy, independent from the simulation clock.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Resource)]
pub struct VideoRuntimeSettings {
    vsync: bool,
    foreground_limit: FrameRateLimit,
    background_limit: FrameRateLimit,
}

impl VideoRuntimeSettings {
    /// Creates a validated render timing policy.
    #[must_use]
    pub const fn new(
        vsync: bool,
        foreground_limit: FrameRateLimit,
        background_limit: FrameRateLimit,
    ) -> Self {
        Self {
            vsync,
            foreground_limit,
            background_limit,
        }
    }

    /// Returns whether vertical synchronization is requested.
    #[must_use]
    pub const fn vsync(self) -> bool {
        self.vsync
    }

    /// Returns the focused-window software cap.
    #[must_use]
    pub const fn foreground_limit(self) -> FrameRateLimit {
        self.foreground_limit
    }

    /// Returns the unfocused-window software cap.
    #[must_use]
    pub const fn background_limit(self) -> FrameRateLimit {
        self.background_limit
    }

    /// Atomically replaces the live timing policy.
    pub const fn replace(
        &mut self,
        vsync: bool,
        foreground_limit: FrameRateLimit,
        background_limit: FrameRateLimit,
    ) {
        self.vsync = vsync;
        self.foreground_limit = foreground_limit;
        self.background_limit = background_limit;
    }
}

impl Default for VideoRuntimeSettings {
    fn default() -> Self {
        Self::new(
            false,
            FrameRateLimit::Unlimited,
            FrameRateLimit::capped(30).unwrap_or(FrameRateLimit::Unlimited),
        )
    }
}

/// Invalid untrusted video timing value.
#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum VideoSettingsError {
    /// A numeric cap was outside the supported domain.
    #[error("frame-rate limit {requested} FPS is outside {minimum}..={maximum} FPS")]
    FrameRateOutsideSupportedRange {
        /// Untrusted requested cap.
        requested: u16,
        /// Inclusive supported minimum.
        minimum: u16,
        /// Inclusive supported maximum.
        maximum: u16,
    },
    /// The catalog value was neither `unlimited` nor a decimal cap.
    #[error("unknown frame-rate limit `{value}`")]
    UnknownFrameRateLimit {
        /// Untrusted catalog string.
        value: String,
    },
}

/// Installs frame pacing without coupling render updates to fixed simulation.
#[derive(Clone, Copy, Debug, Default)]
pub struct VideoRuntimePlugin;

impl Plugin for VideoRuntimePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<VideoRuntimeSettings>()
            .add_plugins(FramepacePlugin)
            .add_systems(Update, apply_video_runtime_settings);
    }
}

#[allow(clippy::needless_pass_by_value)] // Bevy systems receive SystemParams by value.
fn apply_video_runtime_settings(
    settings: Res<'_, VideoRuntimeSettings>,
    mut framepace: ResMut<'_, FramepaceSettings>,
    mut windows: Query<'_, '_, &mut Window, With<PrimaryWindow>>,
) {
    let Ok(mut window) = windows.single_mut() else {
        return;
    };
    if !settings.is_changed() && !window.is_changed() {
        return;
    }

    let requested_present_mode = if settings.vsync() {
        PresentMode::AutoVsync
    } else {
        PresentMode::AutoNoVsync
    };
    if window.present_mode != requested_present_mode {
        window.present_mode = requested_present_mode;
    }

    let active_limit = if window.focused {
        settings.foreground_limit()
    } else {
        settings.background_limit()
    };
    framepace.limiter = active_limit.limiter();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_limit_accepts_high_refresh_and_unlimited_values() {
        assert_eq!(
            FrameRateLimit::parse("300")
                .ok()
                .and_then(FrameRateLimit::frames_per_second),
            Some(300)
        );
        assert_eq!(
            FrameRateLimit::parse("unlimited"),
            Ok(FrameRateLimit::Unlimited)
        );
    }

    #[test]
    fn frame_limit_rejects_unproved_values() {
        assert!(FrameRateLimit::parse("0").is_err());
        assert!(FrameRateLimit::parse("fast").is_err());
    }
}
