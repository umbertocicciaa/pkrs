//! Shared UI resources for overlay states (font atlas + window skin).

use pkrs_render::BitmapFont;
use pkrs_ui::WindowSkin;

/// Font and panel skin wired by the caller (tests, runtime, future asset path).
#[derive(Clone, Debug)]
pub struct UiTheme {
    /// Parsed glyph metrics (same atlas as `font_tex`).
    pub font: BitmapFont,
    /// White atlas uploaded with glyph pixels.
    pub font_tex: pkrs_render::TextureHandle,
    /// Nine-slice panel skin.
    pub skin: WindowSkin,
}

impl UiTheme {
    /// Builds a theme with the given handles (caller loads textures).
    pub fn new(font: BitmapFont, font_tex: pkrs_render::TextureHandle, skin: WindowSkin) -> Self {
        Self {
            font,
            font_tex,
            skin,
        }
    }
}
