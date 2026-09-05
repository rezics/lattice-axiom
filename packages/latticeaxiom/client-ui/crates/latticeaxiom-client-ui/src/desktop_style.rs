//! Shared code-drawn desktop style. No raster skin or custom font assets.

use crate::theme::DESKTOP_COLORS;
use bevy::prelude::{BackgroundGradient, Color, ColorStop, LinearGradient, Node, UiRect, Val};

const fn color(index: usize) -> Color {
    Color::srgb_u8(
        DESKTOP_COLORS[index][0],
        DESKTOP_COLORS[index][1],
        DESKTOP_COLORS[index][2],
    )
}

/// Deep slate application background.
pub const CANVAS: Color = color(0);
/// Opaque panel, readable against changing world backgrounds.
pub const SURFACE: Color = color(1);
/// Hovered controls and inset regions.
pub const RAISED: Color = color(7);
/// Primary text.
pub const TEXT: Color = color(2);
/// Secondary copy, still meeting normal-text contrast on panels.
pub const MUTED: Color = color(3);
/// Warm mineral accent used for selected actions and focus outlines.
pub const ACCENT: Color = color(4);
/// Structural divider color.
pub const BORDER: Color = color(5);
/// Error and destructive-action emphasis.
pub const DANGER: Color = color(6);

/// Subtle native gradient shared by full-screen client surfaces.
#[must_use]
pub fn backdrop() -> BackgroundGradient {
    BackgroundGradient(vec![
        LinearGradient::to_top_right(vec![ColorStop::auto(CANVAS), ColorStop::auto(SURFACE)])
            .into(),
    ])
}

/// Standard readable button geometry; callers choose its content and width.
#[must_use]
pub fn button_node() -> Node {
    Node {
        min_height: Val::Px(48.0),
        padding: UiRect::axes(Val::Px(18.0), Val::Px(12.0)),
        border: UiRect::all(Val::Px(2.0)),
        ..Node::default()
    }
}

/// Focus is represented with a visible outline, independently of hover fill.
#[must_use]
pub const fn focus_border(focused: bool) -> Color {
    if focused { ACCENT } else { BORDER }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn luminance(color: Color) -> f32 {
        let c = color.to_linear();
        0.2126 * c.red + 0.7152 * c.green + 0.0722 * c.blue
    }

    #[test]
    fn essential_text_and_focus_keep_accessible_contrast() {
        for background in [CANVAS, SURFACE, RAISED] {
            for foreground in [TEXT, MUTED, ACCENT, DANGER] {
                let ratio = (luminance(foreground) + 0.05) / (luminance(background) + 0.05);
                assert!(ratio >= 4.5, "essential text contrast was {ratio}");
            }
        }
        assert_ne!(focus_border(true), focus_border(false));
    }
}
