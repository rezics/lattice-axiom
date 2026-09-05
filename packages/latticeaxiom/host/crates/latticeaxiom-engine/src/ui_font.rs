//! System UI font selection for interactive clients.

use bevy::{prelude::TextFont, text::FontSource};

/// Uses the operating system's UI sans-serif family and native glyph fallback.
pub(crate) fn ui_text_font(font_size: f32) -> TextFont {
    TextFont {
        font: FontSource::UiSansSerif,
        ..TextFont::from_font_size(font_size)
    }
}

#[cfg(test)]
mod tests {
    use bevy::text::FontSource;

    use super::*;

    #[test]
    fn ui_text_font_uses_the_system_ui_family_without_bundled_assets() {
        let font = ui_text_font(16.0);
        assert!(matches!(font.font, FontSource::UiSansSerif));
    }
}
