//! BMFont text-format parser (subset used by pkrs, §5.5).

use std::collections::BTreeMap;

use crate::error::{RenderError, Result};
use crate::rect::Rect;
use crate::text::font::{BitmapFont, GlyphMetrics};
use crate::text::util;

/// Parses a BMFont `.fnt` file (text format).
pub fn parse_bmfont_text(source: &str) -> Result<BitmapFont> {
    let mut line_height = 0u32;
    let mut base = 0u32;
    let mut atlas_width = 0u32;
    let mut atlas_height = 0u32;
    let mut glyphs = BTreeMap::new();

    for line in source.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Some(rest) = line.strip_prefix("common ") {
            for token in rest.split_whitespace() {
                if let Some(v) = token.strip_prefix("lineHeight=") {
                    line_height = parse_u32(v, "lineHeight")?;
                } else if let Some(v) = token.strip_prefix("base=") {
                    base = parse_u32(v, "base")?;
                } else if let Some(v) = token.strip_prefix("scaleW=") {
                    atlas_width = parse_u32(v, "scaleW")?;
                } else if let Some(v) = token.strip_prefix("scaleH=") {
                    atlas_height = parse_u32(v, "scaleH")?;
                }
            }
        } else if let Some(rest) = line.strip_prefix("char ") {
            let mut id = None;
            let mut x = 0u32;
            let mut y = 0u32;
            let mut width = 0u32;
            let mut height = 0u32;
            let mut xoffset = 0i32;
            let mut yoffset = 0i32;
            let mut xadvance = 0u32;
            for token in rest.split_whitespace() {
                if let Some(v) = token.strip_prefix("id=") {
                    id = Some(parse_u32(v, "id")?);
                } else if let Some(v) = token.strip_prefix("x=") {
                    x = parse_u32(v, "x")?;
                } else if let Some(v) = token.strip_prefix("y=") {
                    y = parse_u32(v, "y")?;
                } else if let Some(v) = token.strip_prefix("width=") {
                    width = parse_u32(v, "width")?;
                } else if let Some(v) = token.strip_prefix("height=") {
                    height = parse_u32(v, "height")?;
                } else if let Some(v) = token.strip_prefix("xoffset=") {
                    xoffset = parse_i32(v, "xoffset")?;
                } else if let Some(v) = token.strip_prefix("yoffset=") {
                    yoffset = parse_i32(v, "yoffset")?;
                } else if let Some(v) = token.strip_prefix("xadvance=") {
                    xadvance = parse_u32(v, "xadvance")?;
                }
            }
            let Some(id) = id else {
                continue;
            };
            let metrics = GlyphMetrics {
                src: Rect::new(
                    util::u32_px(x),
                    util::u32_px(y),
                    util::u32_px(width),
                    util::u32_px(height),
                ),
                xoffset,
                yoffset,
                xadvance,
            };
            glyphs.insert(id, metrics);
        }
    }

    if line_height == 0 || atlas_width == 0 || atlas_height == 0 {
        return Err(RenderError::FontInvalid {
            detail: "missing common lineHeight/scaleW/scaleH".into(),
        });
    }
    if glyphs.is_empty() {
        return Err(RenderError::FontInvalid {
            detail: "no char entries".into(),
        });
    }
    Ok(BitmapFont::from_glyphs(
        line_height,
        base,
        atlas_width,
        atlas_height,
        glyphs,
    ))
}

fn parse_u32(raw: &str, field: &str) -> Result<u32> {
    raw.parse::<u32>().map_err(|e| RenderError::FontParse {
        detail: format!("{field}: {e}"),
    })
}

fn parse_i32(raw: &str, field: &str) -> Result<i32> {
    raw.parse::<i32>().map_err(|e| RenderError::FontParse {
        detail: format!("{field}: {e}"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_minimal_bmfont() {
        let src = r#"
info face="Test" size=12
common lineHeight=12 base=10 scaleW=64 scaleH=32 pages=1
page id=0 file="font.png"
chars count=2
char id=32 x=0 y=0 width=3 height=1 xoffset=0 yoffset=11 xadvance=4 page=0
char id=65 x=4 y=0 width=6 height=10 xoffset=0 yoffset=0 xadvance=7 page=0
"#;
        let font = parse_bmfont_text(src).expect("parse");
        assert_eq!(font.line_height, 12);
        assert!(font.glyph('A'.into()).is_some());
        assert_eq!(font.glyph('A'.into()).unwrap().xadvance, 7);
    }
}
