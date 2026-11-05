//! Text measurement and wrapping (§5.5).

use crate::text::font::BitmapFont;
use crate::text::util;

/// Pixel size of a string at the given integer scale.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TextMetrics {
    /// Total width in pixels.
    pub width: f32,
    /// Total height in pixels.
    pub height: f32,
}

/// Measures `text` without wrapping.
pub fn measure_text(font: &BitmapFont, text: &str, scale: u32) -> TextMetrics {
    let scale_f = util::scale_f(scale);
    if text.is_empty() {
        return TextMetrics {
            width: 0.0,
            height: util::u32_px(font.line_height) * scale_f,
        };
    }
    let lines: Vec<&str> = text.split('\n').collect();
    let width = lines
        .iter()
        .map(|line| line_width_for(font, line, scale))
        .fold(0.0f32, f32::max);
    let height = util::u32_px(font.line_height * u32::try_from(lines.len()).unwrap_or(1)) * scale_f;
    TextMetrics { width, height }
}

fn line_width_for(font: &BitmapFont, line: &str, scale: u32) -> f32 {
    let scale_f = util::scale_f(scale);
    let mut width = 0.0f32;
    for ch in line.chars() {
        width += glyph_advance(font, ch) * scale_f;
    }
    width
}

fn glyph_advance(font: &BitmapFont, ch: char) -> f32 {
    font.glyph(u32::from(ch))
        .or_else(|| font.fallback_glyph())
        .map(|g| util::u32_px(g.xadvance))
        .unwrap_or(0.0)
}

/// Wraps `text` to at most `max_width` pixels (scale applied). Preserves explicit `\n`.
pub fn wrap_text(font: &BitmapFont, text: &str, max_width: f32, scale: u32) -> Vec<String> {
    if max_width <= 0.0 {
        return vec![text.to_string()];
    }
    let mut out = Vec::new();
    for paragraph in text.split('\n') {
        if paragraph.is_empty() {
            out.push(String::new());
            continue;
        }
        wrap_paragraph(font, paragraph, max_width, scale, &mut out);
    }
    if out.is_empty() {
        out.push(String::new());
    }
    out
}

fn wrap_paragraph(
    font: &BitmapFont,
    text: &str,
    max_width: f32,
    scale: u32,
    out: &mut Vec<String>,
) {
    let scale_f = util::scale_f(scale);
    let mut line = String::new();
    let mut line_width = 0.0f32;
    let mut word = String::new();
    let mut word_width = 0.0f32;

    let flush_word = |line: &mut String,
                      line_width: &mut f32,
                      word: &mut String,
                      word_width: &mut f32,
                      out: &mut Vec<String>| {
        if word.is_empty() {
            return;
        }
        if *line_width + *word_width > max_width && !line.is_empty() {
            out.push(std::mem::take(line));
            *line_width = 0.0;
        }
        if *word_width > max_width && line.is_empty() {
            for ch in word.chars() {
                let adv = glyph_advance(font, ch) * scale_f;
                if *line_width + adv > max_width && !line.is_empty() {
                    out.push(std::mem::take(line));
                    *line_width = 0.0;
                }
                line.push(ch);
                *line_width += adv;
            }
            word.clear();
            *word_width = 0.0;
            return;
        }
        if !line.is_empty() {
            line.push(' ');
            *line_width += glyph_advance(font, ' ') * scale_f;
        }
        line.push_str(word);
        *line_width += *word_width;
        word.clear();
        *word_width = 0.0;
    };

    for ch in text.chars() {
        if ch.is_whitespace() {
            flush_word(&mut line, &mut line_width, &mut word, &mut word_width, out);
            continue;
        }
        let adv = glyph_advance(font, ch) * scale_f;
        word.push(ch);
        word_width += adv;
    }
    flush_word(&mut line, &mut line_width, &mut word, &mut word_width, out);
    if !line.is_empty() || out.is_empty() {
        out.push(line);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rect::Rect;
    use crate::text::font::GlyphMetrics;
    use std::collections::BTreeMap;

    fn test_font() -> BitmapFont {
        let mut glyphs = BTreeMap::new();
        let mk = |w, adv| GlyphMetrics {
            src: Rect::new(0.0, 0.0, util::u32_px(w), 8.0),
            xoffset: 0,
            yoffset: 0,
            xadvance: adv,
        };
        glyphs.insert(' '.into(), mk(4, 4));
        glyphs.insert('a'.into(), mk(6, 6));
        glyphs.insert('b'.into(), mk(6, 6));
        BitmapFont::from_glyphs(10, 8, 64, 32, glyphs)
    }

    #[test]
    fn empty_string_has_zero_width() {
        let m = measure_text(&test_font(), "", 1);
        assert_eq!(m.width, 0.0);
        assert_eq!(m.height, 10.0);
    }

    #[test]
    fn wrap_breaks_long_word() {
        let font = test_font();
        let lines = wrap_text(&font, "aaaa", 12.0, 1);
        assert!(lines.len() >= 2);
    }

    #[test]
    fn utf8_multibyte_characters_measured() {
        let mut glyphs = BTreeMap::new();
        glyphs.insert(
            '🙂'.into(),
            GlyphMetrics {
                src: Rect::new(0.0, 0.0, 8.0, 8.0),
                xoffset: 0,
                yoffset: 0,
                xadvance: 8,
            },
        );
        let font = BitmapFont::from_glyphs(10, 8, 16, 16, glyphs);
        let m = measure_text(&font, "🙂", 1);
        assert_eq!(m.width, 8.0);
    }
}
