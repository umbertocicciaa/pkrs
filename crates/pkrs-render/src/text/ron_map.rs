//! RON glyph map parser (§5.5).

use std::collections::BTreeMap;

use serde::Deserialize;

use crate::error::{RenderError, Result};
use crate::rect::Rect;
use crate::text::font::{BitmapFont, GlyphMetrics, insert_glyph};
use crate::text::util;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawGlyphMap {
    line_height: u32,
    base: u32,
    atlas: RawAtlas,
    glyphs: BTreeMap<String, RawGlyph>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawAtlas {
    width: u32,
    height: u32,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawGlyph {
    x: u32,
    y: u32,
    w: u32,
    h: u32,
    #[serde(default)]
    xoff: i32,
    #[serde(default)]
    yoff: i32,
    xadv: u32,
}

/// Parses a pkrs RON glyph map.
pub fn parse_glyph_map_ron(source: &str) -> Result<BitmapFont> {
    let raw: RawGlyphMap = ron::from_str(source).map_err(|e| RenderError::FontParse {
        detail: e.to_string(),
    })?;
    if raw.line_height == 0 || raw.atlas.width == 0 || raw.atlas.height == 0 {
        return Err(RenderError::FontInvalid {
            detail: "line_height and atlas size must be non-zero".into(),
        });
    }
    if raw.glyphs.is_empty() {
        return Err(RenderError::FontInvalid {
            detail: "glyphs must not be empty".into(),
        });
    }
    let mut glyphs = BTreeMap::new();
    for (key, g) in raw.glyphs {
        let codepoint = decode_glyph_key(&key)?;
        if g.w == 0 || g.h == 0 {
            return Err(RenderError::FontInvalid {
                detail: format!("glyph `{key}` has zero size"),
            });
        }
        insert_glyph(
            &mut glyphs,
            codepoint,
            GlyphMetrics {
                src: Rect::new(
                    util::u32_px(g.x),
                    util::u32_px(g.y),
                    util::u32_px(g.w),
                    util::u32_px(g.h),
                ),
                xoffset: g.xoff,
                yoffset: g.yoff,
                xadvance: g.xadv,
            },
        )?;
    }
    BitmapFont::from_glyphs(
        raw.line_height,
        raw.base,
        raw.atlas.width,
        raw.atlas.height,
        glyphs,
        BTreeMap::new(),
    )
}

fn decode_glyph_key(key: &str) -> Result<u32> {
    if key.starts_with("U+") || key.starts_with("u+") {
        let hex = &key[2..];
        if hex.starts_with('+') {
            return Err(RenderError::FontParse {
                detail: format!("glyph key `{key}`: invalid U+ form"),
            });
        }
        let cp = u32::from_str_radix(hex, 16).map_err(|e| RenderError::FontParse {
            detail: format!("glyph key `{key}`: {e}"),
        })?;
        validate_codepoint(cp, key)?;
        return Ok(cp);
    }
    let mut chars = key.chars();
    let ch = chars.next().ok_or_else(|| RenderError::FontParse {
        detail: "empty glyph key".into(),
    })?;
    if chars.next().is_some() {
        return Err(RenderError::FontParse {
            detail: format!("glyph key `{key}` must be one character or U+XXXX"),
        });
    }
    Ok(u32::from(ch))
}

fn validate_codepoint(cp: u32, key: &str) -> Result<()> {
    if cp > 0x10_FFFF || (0xD800..=0xDFFF).contains(&cp) {
        return Err(RenderError::FontParse {
            detail: format!("glyph key `{key}`: not a Unicode scalar value"),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_ron_glyph_map() {
        let src = r#"(
            line_height: 10,
            base: 8,
            atlas: (width: 32, height: 16),
            glyphs: {
                "A": (x: 0, y: 0, w: 6, h: 8, xadv: 7),
                "U+0020": (x: 8, y: 0, w: 4, h: 1, xadv: 4),
            },
        )"#;
        let font = parse_glyph_map_ron(src).expect("parse");
        assert!(font.glyph('A'.into()).is_some());
        assert!(font.glyph(' '.into()).is_some());
    }

    #[test]
    fn rejects_out_of_atlas_glyph() {
        let src = r#"(
            line_height: 10,
            base: 8,
            atlas: (width: 32, height: 16),
            glyphs: {
                "A": (x: 4000, y: 0, w: 6, h: 8, xadv: 7),
            },
        )"#;
        assert!(parse_glyph_map_ron(src).is_err());
    }
}
