//! Coloured dialogue layout and drawing (§5.5).

use pkrs_render::{BitmapFont, Color, Layer, Rect, Renderer, SpriteDraw};

use crate::codes::DialogueSegment;

#[inline]
fn scale_f(scale: u32) -> f32 {
    scale.max(1) as f32
}

#[inline]
fn u32_px(v: u32) -> f32 {
    v as f32
}

#[inline]
fn i32_px(v: i32) -> f32 {
    v as f32
}

/// Flatten segments into a `(char, color)` stream; explicit `\n` preserved.
pub fn flatten_segments(segments: &[DialogueSegment]) -> Vec<(char, Color)> {
    let mut out = Vec::new();
    for seg in segments {
        for ch in seg.text.chars() {
            out.push((ch, seg.tint));
        }
    }
    out
}

fn char_advance(font: &BitmapFont, prev: Option<char>, ch: char, scale: u32) -> f32 {
    font.advance(prev, ch) * scale_f(scale)
}

fn word_width(font: &BitmapFont, word: &[(char, Color)], scale: u32) -> f32 {
    let mut w = 0.0f32;
    let mut prev: Option<char> = None;
    for &(ch, _) in word {
        w += char_advance(font, prev, ch, scale);
        prev = Some(ch);
    }
    w
}

/// Hard breaks on `\n`, soft-wrap words to `max_width` (pixels).
pub fn wrap_colored_lines(
    font: &BitmapFont,
    chars: &[(char, Color)],
    max_width: f32,
    scale: u32,
) -> Vec<Vec<(char, Color)>> {
    if max_width <= 0.0 {
        return vec![chars.to_vec()];
    }
    let mut out = Vec::new();
    let mut para_start = 0usize;
    for i in 0..=chars.len() {
        let at_break = i == chars.len() || chars.get(i).is_some_and(|&(ch, _)| ch == '\n');
        if !at_break {
            continue;
        }
        out.extend(wrap_paragraph(
            font,
            &chars[para_start..i],
            max_width,
            scale,
        ));
        para_start = i + 1;
    }
    if out.is_empty() {
        out.push(Vec::new());
    }
    out
}

fn wrap_paragraph(
    font: &BitmapFont,
    chars: &[(char, Color)],
    max_width: f32,
    scale: u32,
) -> Vec<Vec<(char, Color)>> {
    let mut lines: Vec<Vec<(char, Color)>> = Vec::new();
    let mut line: Vec<(char, Color)> = Vec::new();
    let mut line_width = 0.0f32;
    let mut prev_in_line: Option<char> = None;

    let flush_line = |line: &mut Vec<(char, Color)>,
                      lines: &mut Vec<Vec<(char, Color)>>,
                      lw: &mut f32,
                      prev: &mut Option<char>| {
        if !line.is_empty() {
            lines.push(std::mem::take(line));
            *lw = 0.0;
            *prev = None;
        }
    };

    let mut i = 0usize;
    while i < chars.len() {
        let ch = chars[i].0;
        if ch == '\n' {
            flush_line(&mut line, &mut lines, &mut line_width, &mut prev_in_line);
            i += 1;
            continue;
        }
        while i < chars.len() && chars[i].0.is_whitespace() && chars[i].0 != '\n' {
            i += 1;
        }
        if i >= chars.len() {
            break;
        }
        let w_start = i;
        while i < chars.len() && !chars[i].0.is_whitespace() {
            i += 1;
        }
        let word = &chars[w_start..i];
        if word.is_empty() {
            continue;
        }
        let ww = word_width(font, word, scale);
        let gap = if line.is_empty() {
            0.0
        } else {
            char_advance(font, prev_in_line, ' ', scale)
        };
        if !line.is_empty() && line_width + gap + ww > max_width {
            flush_line(&mut line, &mut lines, &mut line_width, &mut prev_in_line);
        }
        if ww > max_width {
            for &(ch, tint) in word {
                let adv = char_advance(font, prev_in_line, ch, scale);
                if !line.is_empty() && line_width + adv > max_width {
                    flush_line(&mut line, &mut lines, &mut line_width, &mut prev_in_line);
                }
                line_width += adv;
                line.push((ch, tint));
                prev_in_line = Some(ch);
            }
            continue;
        }
        if !line.is_empty() {
            line_width += gap;
            line.push((' ', word[0].1));
            prev_in_line = Some(' ');
        }
        for &(ch, tint) in word {
            line_width += char_advance(font, prev_in_line, ch, scale);
            line.push((ch, tint));
            prev_in_line = Some(ch);
        }
    }
    if !line.is_empty() {
        lines.push(line);
    }
    if lines.is_empty() {
        lines.push(Vec::new());
    }
    lines
}

/// Draws the first `visible` glyphs from wrapped lines (including spaces).
#[allow(clippy::too_many_arguments)] // render helper: atlas, font, layout, and style in one call
pub fn draw_colored_lines(
    renderer: &mut Renderer,
    font: &BitmapFont,
    texture: pkrs_render::TextureHandle,
    lines: &[Vec<(char, Color)>],
    mut visible: usize,
    x: f32,
    y: f32,
    scale: u32,
    layer: Layer,
) {
    let scale_f = scale_f(scale);
    let lh = u32_px(font.line_height) * scale_f;
    let mut line_y = y;
    for row in lines {
        let mut cx = x;
        let mut prev: Option<char> = None;
        for &(ch, tint) in row {
            if visible == 0 {
                return;
            }
            visible -= 1;
            if ch == ' ' {
                cx += char_advance(font, prev, ch, scale);
                prev = Some(ch);
                continue;
            }
            let Some(metrics) = font.resolve_glyph(ch) else {
                prev = Some(ch);
                continue;
            };
            let dst_w = metrics.src.width * scale_f;
            let dst_h = metrics.src.height * scale_f;
            let dst_x = cx + i32_px(metrics.xoffset) * scale_f;
            let dst_y = line_y + i32_px(metrics.yoffset) * scale_f;
            renderer.draw_sprite(SpriteDraw {
                texture,
                src: metrics.src,
                dst: Rect::new(dst_x, dst_y, dst_w, dst_h),
                flip_x: false,
                tint,
                layer,
            });
            cx += font.advance(prev, ch) * scale_f;
            prev = Some(ch);
        }
        line_y += lh;
    }
}

/// Total glyphs that would be drawn (including spaces).
#[must_use]
pub fn colored_line_char_count(lines: &[Vec<(char, Color)>]) -> usize {
    lines.iter().map(|row| row.len()).sum()
}
