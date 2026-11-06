//! Helpers to build UI [`UiTheme`](crate::ui_theme::UiTheme) from fixtures (tests / overlays).

use pkrs_render::BitmapFont;
use pkrs_ui::WindowSkin;

use crate::ui_theme::UiTheme;

const FONT_RON: &str = include_str!("../../pkrs-render/tests/fixtures/minimal.ron");

/// Solid white atlas: fixture glyphs sample visible pixels (§5.5 smoke / tests).
fn font_atlas_rgba(font: &BitmapFont) -> Vec<u8> {
    let w = font.atlas_width as usize;
    let h = font.atlas_height as usize;
    vec![255u8; w * h * 4]
}

fn skin_atlas_rgba() -> Vec<u8> {
    let w = 32u32;
    let h = 32u32;
    let mut rgba = vec![0u8; (w * h * 4) as usize];
    for y in 0..h {
        for x in 0..w {
            let i = ((y * w + x) * 4) as usize;
            let c = if (x + y) % 2 == 0 { 40u8 } else { 25u8 };
            rgba[i] = c;
            rgba[i + 1] = c.saturating_add(30);
            rgba[i + 2] = c.saturating_add(50);
            rgba[i + 3] = 255;
        }
    }
    rgba
}

/// Builds font + skin textures on `renderer` (fixture font + generated skin).
pub fn build_test_ui_theme(
    renderer: &mut pkrs_render::Renderer,
) -> Result<UiTheme, pkrs_ui::UiError> {
    let font: BitmapFont = pkrs_render::parse_glyph_map_ron(FONT_RON)?;
    let atlas = font_atlas_rgba(&font);
    let font_tex = renderer.create_texture(font.atlas_width, font.atlas_height, &atlas)?;

    let tw = 32u32;
    let th = 32u32;
    let skin_px = skin_atlas_rgba();
    let tex_skin = renderer.create_texture(tw, th, &skin_px)?;
    let skin = WindowSkin::new(tex_skin, tw, th, pkrs_ui::NineSliceSpec::default())?;

    Ok(UiTheme::new(font, font_tex, skin))
}
