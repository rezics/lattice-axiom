//! Plain v1 theme tokens shared by shell, HUD, and settings surfaces.

use serde::{Deserialize, Serialize};
use thiserror::Error;

pub(crate) const DESKTOP_COLORS: [[u8; 3]; 8] = [
    [12, 20, 29],
    [23, 35, 47],
    [240, 239, 233],
    [177, 189, 199],
    [229, 188, 125],
    [67, 87, 103],
    [248, 158, 146],
    [35, 51, 65],
];

fn desktop_color(index: usize) -> Result<LinearRgba, ThemeTokenError> {
    let channels = DESKTOP_COLORS[index].map(|byte| {
        let value = f32::from(byte) / 255.0;
        if value <= 0.04045 {
            value / 12.92
        } else {
            ((value + 0.055) / 1.055).powf(2.4)
        }
    });
    LinearRgba::new(channels[0], channels[1], channels[2], 1.0)
}

/// First-version UI scale factors required by the accessibility gate.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum UiScale {
    /// 100% logical scale.
    One,
    /// 200% logical scale.
    Two,
}

impl UiScale {
    /// Returns the integer scale factor applied to logical pixels.
    #[must_use]
    pub const fn factor(self) -> u32 {
        match self {
            Self::One => 1,
            Self::Two => 2,
        }
    }
}

/// Linear RGBA color in the inclusive `0..=1` range.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct LinearRgba {
    /// Red channel.
    pub red: f32,
    /// Green channel.
    pub green: f32,
    /// Blue channel.
    pub blue: f32,
    /// Alpha channel.
    pub alpha: f32,
}

impl LinearRgba {
    /// Creates a finite color in the inclusive `0..=1` range.
    ///
    /// # Errors
    ///
    /// Returns [`ThemeTokenError::InvalidColorChannel`] when a channel is
    /// non-finite or outside `0..=1`.
    pub fn new(red: f32, green: f32, blue: f32, alpha: f32) -> Result<Self, ThemeTokenError> {
        for (name, channel) in [
            ("red", red),
            ("green", green),
            ("blue", blue),
            ("alpha", alpha),
        ] {
            if !channel.is_finite() || !(0.0..=1.0).contains(&channel) {
                return Err(ThemeTokenError::InvalidColorChannel { name });
            }
        }
        Ok(Self {
            red,
            green,
            blue,
            alpha,
        })
    }

    /// Returns the four channels in stable RGBA order.
    #[must_use]
    pub const fn channels(self) -> [f32; 4] {
        [self.red, self.green, self.blue, self.alpha]
    }
}

/// Type scale in unscaled logical pixels.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TypeScale {
    /// Caption and helper text.
    pub caption: u32,
    /// Default body copy.
    pub body: u32,
    /// Surface titles.
    pub title: u32,
}

impl TypeScale {
    /// Scales every size by [`UiScale`].
    #[must_use]
    pub const fn scaled(self, scale: UiScale) -> Self {
        let factor = scale.factor();
        Self {
            caption: self.caption.saturating_mul(factor),
            body: self.body.saturating_mul(factor),
            title: self.title.saturating_mul(factor),
        }
    }
}

/// Spacing scale in unscaled logical pixels.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SpacingScale {
    /// Extra-small inset.
    pub xs: u32,
    /// Small inset.
    pub sm: u32,
    /// Default inset.
    pub md: u32,
    /// Large inset.
    pub lg: u32,
}

impl SpacingScale {
    /// Scales every token by [`UiScale`].
    #[must_use]
    pub const fn scaled(self, scale: UiScale) -> Self {
        let factor = scale.factor();
        Self {
            xs: self.xs.saturating_mul(factor),
            sm: self.sm.saturating_mul(factor),
            md: self.md.saturating_mul(factor),
            lg: self.lg.saturating_mul(factor),
        }
    }
}

/// Focus-ring token applied to the unique focus owner.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FocusRing {
    /// Unscaled ring width in logical pixels.
    pub width: u32,
    /// Ring color.
    pub color: LinearRgba,
}

impl FocusRing {
    /// Scales the ring width by [`UiScale`].
    #[must_use]
    pub const fn scaled(self, scale: UiScale) -> Self {
        Self {
            width: self.width.saturating_mul(scale.factor()),
            color: self.color,
        }
    }
}

/// Chrome heights used by the 800×600 scroll contract.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ChromeMetrics {
    /// Unscaled header height.
    pub header: u32,
    /// Unscaled footer height.
    pub footer: u32,
    /// Unscaled focusable row height.
    pub row: u32,
}

impl ChromeMetrics {
    /// Scales chrome by [`UiScale`].
    #[must_use]
    pub const fn scaled(self, scale: UiScale) -> Self {
        let factor = scale.factor();
        Self {
            header: self.header.saturating_mul(factor),
            footer: self.footer.saturating_mul(factor),
            row: self.row.saturating_mul(factor),
        }
    }
}

/// Shared palette for blocking surfaces and high-frequency HUD.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ThemePalette {
    /// Application canvas.
    pub canvas: LinearRgba,
    /// Raised panel.
    pub surface: LinearRgba,
    /// Primary copy.
    pub text: LinearRgba,
    /// Secondary copy.
    pub muted: LinearRgba,
    /// Accent and selected state.
    pub accent: LinearRgba,
    /// Destructive or blocking alert.
    pub danger: LinearRgba,
}

/// Required CJK fallback font identity. The host adapter supplies the asset.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(transparent)]
pub struct CjkFallbackFontId(String);

impl CjkFallbackFontId {
    /// First-version CJK fallback font registration identity.
    #[must_use]
    pub fn v1() -> Self {
        Self("latticeaxiom:asset/font/cjk-fallback@1".to_owned())
    }

    /// Returns the stable asset identity.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Plain v1 theme tokens. Visual polish may change later; ownership may not.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ThemeTokens {
    /// Shared palette.
    pub palette: ThemePalette,
    /// Type scale.
    pub type_scale: TypeScale,
    /// Spacing scale.
    pub spacing: SpacingScale,
    /// Focus ring.
    pub focus_ring: FocusRing,
    /// Header, footer, and row metrics.
    pub chrome: ChromeMetrics,
    /// Required CJK fallback font identity.
    pub cjk_fallback_font: CjkFallbackFontId,
}

impl ThemeTokens {
    /// Intentionally plain v2 tokens used until V8 visual work.
    ///
    /// # Errors
    ///
    /// Returns [`ThemeTokenError`] if a channel constant is invalid. The
    /// published constants are finite and in range, so this is unreachable for
    /// [`Self::plain_v2`].
    fn try_plain_v2() -> Result<Self, ThemeTokenError> {
        let canvas = desktop_color(0)?;
        let surface = desktop_color(1)?;
        let text = desktop_color(2)?;
        let muted = desktop_color(3)?;
        let accent = desktop_color(4)?;
        let danger = desktop_color(6)?;
        Ok(Self {
            palette: ThemePalette {
                canvas,
                surface,
                text,
                muted,
                accent,
                danger,
            },
            type_scale: TypeScale {
                caption: 12,
                body: 16,
                title: 22,
            },
            spacing: SpacingScale {
                xs: 4,
                sm: 8,
                md: 16,
                lg: 24,
            },
            focus_ring: FocusRing {
                width: 2,
                color: accent,
            },
            chrome: ChromeMetrics {
                header: 72,
                footer: 64,
                row: 44,
            },
            cjk_fallback_font: CjkFallbackFontId::v1(),
        })
    }

    /// Returns the published plain v2 token set.
    #[must_use]
    pub fn plain_v2() -> Self {
        match Self::try_plain_v2() {
            Ok(tokens) => tokens,
            Err(error) => {
                unreachable!("plain v2 theme constants are finite 0..=1 colors: {error}")
            }
        }
    }

    /// Returns tokens scaled for a supported UI scale.
    #[must_use]
    pub fn scaled(&self, scale: UiScale) -> Self {
        Self {
            palette: self.palette,
            type_scale: self.type_scale.scaled(scale),
            spacing: self.spacing.scaled(scale),
            focus_ring: self.focus_ring.scaled(scale),
            chrome: self.chrome.scaled(scale),
            cjk_fallback_font: self.cjk_fallback_font.clone(),
        }
    }
}

/// Invalid theme token construction.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum ThemeTokenError {
    /// A color channel is non-finite or outside `0..=1`.
    #[error("theme color channel `{name}` must be a finite value in 0..=1")]
    InvalidColorChannel {
        /// Channel name.
        name: &'static str,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_v2_tokens_scale_without_overflowing_minimum_chrome() {
        let tokens = ThemeTokens::plain_v2();
        let scaled = tokens.scaled(UiScale::Two);
        assert_eq!(scaled.type_scale.body, 32);
        assert_eq!(scaled.chrome.row, 88);
        assert_eq!(scaled.focus_ring.width, 4);
        assert_eq!(
            scaled.cjk_fallback_font.as_str(),
            "latticeaxiom:asset/font/cjk-fallback@1"
        );
    }

    #[test]
    fn color_constructor_rejects_non_finite_and_out_of_range_channels() {
        assert_eq!(
            LinearRgba::new(f32::NAN, 0.0, 0.0, 1.0),
            Err(ThemeTokenError::InvalidColorChannel { name: "red" })
        );
        assert_eq!(
            LinearRgba::new(0.0, 1.1, 0.0, 1.0),
            Err(ThemeTokenError::InvalidColorChannel { name: "green" })
        );
    }
}
