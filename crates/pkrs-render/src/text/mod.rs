//! Bitmap font loading and layout (§5.5).

mod bmfont;
mod font;
mod layout;
mod ron_map;
mod util;

pub use bmfont::parse_bmfont_text;
pub use font::BitmapFont;
pub use layout::{TextMetrics, measure_text, wrap_text};
pub use ron_map::parse_glyph_map_ron;

use crate::color::Color;
use crate::draw::SpriteDraw;
use crate::layer::Layer;
use crate::rect::Rect;
use crate::renderer::Renderer;
use crate::texture::TextureHandle;

/// Style for [`Renderer::draw_text`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TextStyle {
    /// Tint colour (atlas is assumed white).
    pub tint: Color,
    /// Integer scale for crisp pixels (§5.5).
    pub scale: u32,
    /// Draw layer.
    pub layer: Layer,
}

impl Default for TextStyle {
    fn default() -> Self {
        Self {
            tint: Color::WHITE,
            scale: 1,
            layer: Layer::Ui,
        }
    }
}

impl Renderer {
    /// Draws `text` with a bitmap font atlas at integer pixel coordinates.
    ///
    /// `x`/`y` are the top-left of the first line box. Newlines start a new line using
    /// `font.line_height * scale`.
    pub fn draw_text(
        &mut self,
        font: &BitmapFont,
        texture: TextureHandle,
        text: &str,
        x: f32,
        y: f32,
        style: TextStyle,
    ) {
        let scale_f = util::scale_f(style.scale);
        let text = text.replace('\r', "");
        let mut cursor_x = x.floor();
        let mut cursor_y = y.floor();
        for (line_index, line) in text.split('\n').enumerate() {
            if line_index > 0 {
                cursor_y += util::u32_px(font.line_height) * scale_f;
                cursor_x = x.floor();
            }
            let mut prev: Option<char> = None;
            for ch in line.chars() {
                let Some(metrics) = font.resolve_glyph(ch) else {
                    prev = Some(ch);
                    continue;
                };
                let dst_w = metrics.src.width * scale_f;
                let dst_h = metrics.src.height * scale_f;
                let dst_x = cursor_x + util::i32_px(metrics.xoffset) * scale_f;
                let dst_y = cursor_y + util::i32_px(metrics.yoffset) * scale_f;
                self.draw_sprite(SpriteDraw {
                    texture,
                    src: metrics.src,
                    dst: Rect::new(dst_x, dst_y, dst_w, dst_h),
                    flip_x: false,
                    tint: style.tint,
                    layer: style.layer,
                });
                cursor_x += font.advance(prev, ch) * scale_f;
                prev = Some(ch);
            }
        }
    }
}
