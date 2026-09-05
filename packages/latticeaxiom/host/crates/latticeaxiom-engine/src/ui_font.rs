//! System UI font selection for interactive clients.

use bevy::prelude::TextFont;

/// Installed font family requested by every Lattice Axiom UI text node.
///
/// TODO: Replace the system-family lookup with a game-owned font asset handle
/// when font files become part of the packaged game assets.
const SYSTEM_UI_FONT_FAMILY: &str = "Noto Sans TC";

/// Creates UI text styling that resolves Noto Sans TC from the operating system.
pub(crate) fn ui_text_font(font_size: f32) -> TextFont {
    TextFont::from_font_size(font_size).with_family(SYSTEM_UI_FONT_FAMILY)
}

#[cfg(test)]
mod tests {
    use bevy::text::FontSource;

    use super::*;

    #[test]
    fn ui_text_font_requests_the_installed_noto_sans_tc_family() {
        let font = ui_text_font(16.0);
        assert!(matches!(
            font.font,
            FontSource::Family(family) if family == SYSTEM_UI_FONT_FAMILY
        ));
    }
}
