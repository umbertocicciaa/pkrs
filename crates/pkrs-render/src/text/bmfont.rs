//! BMFont text-format parser (subset used by pkrs, §5.5).

use std::collections::BTreeMap;

use crate::error::{RenderError, Result};
use crate::rect::Rect;
use crate::text::font::{BitmapFont, GlyphMetrics, insert_glyph};
use crate::text::util;

/// Parses a BMFont `.fnt` file (text format).
pub fn parse_bmfont_text(source: &str) -> Result<BitmapFont> {
    let mut line_height = 0u32;
    let mut base = 0u32;
    let mut atlas_width = 0u32;
    let mut atlas_height = 0u32;
    let mut page_count = 0u32;
    let mut glyphs = BTreeMap::new();
    let mut kerning = BTreeMap::new();

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
                } else if let Some(v) = token.strip_prefix("pages=") {
                    page_count = parse_u32(v, "pages")?;
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
            let mut page = 0u32;
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
                } else if let Some(v) = token.strip_prefix("page=") {
                    page = parse_u32(v, "page")?;
                }
            }
            let Some(id) = id else {
                continue;
            };
            if page != 0 {
                return Err(RenderError::FontInvalid {
                    detail: format!(
                        "char id={id} uses page {page}, only single-page fonts supported"
                    ),
                });
            }
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
            insert_glyph(&mut glyphs, id, metrics)?;
        } else if let Some(rest) = line.strip_prefix("kerning ") {
            let mut first = None;
            let mut second = None;
            let mut amount = 0i32;
            for token in rest.split_whitespace() {
                if let Some(v) = token.strip_prefix("first=") {
                    first = Some(parse_u32(v, "first")?);
                } else if let Some(v) = token.strip_prefix("second=") {
                    second = Some(parse_u32(v, "second")?);
                } else if let Some(v) = token.strip_prefix("amount=") {
                    amount = parse_i32(v, "amount")?;
                }
            }
            if let (Some(f), Some(s)) = (first, second) {
                kerning.insert((f, s), amount);
            }
        }
    }

    if page_count > 1 {
        return Err(RenderError::FontInvalid {
            detail: format!("pages={page_count}, only single-page fonts supported"),
        });
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
    BitmapFont::from_glyphs(
        line_height,
        base,
        atlas_width,
        atlas_height,
        glyphs,
        kerning,
    )
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

    #[test]
    fn kerning_adjusts_advance() {
        let src = r#"
common lineHeight=12 base=10 scaleW=64 scaleH=32 pages=1
char id=65 x=0 y=0 width=6 height=10 xoffset=0 yoffset=0 xadvance=10 page=0
char id=66 x=8 y=0 width=6 height=10 xoffset=0 yoffset=0 xadvance=10 page=0
kerning first=65 second=66 amount=-4
"#;
        let font = parse_bmfont_text(src).expect("parse");
        assert_eq!(font.advance(Some('A'), 'B'), 6.0);
    }

    #[test]
    fn rejects_glyph_outside_atlas() {
        let src = r#"
common lineHeight=12 base=10 scaleW=64 scaleH=32 pages=1
char id=65 x=900 y=0 width=6 height=10 xoffset=0 yoffset=0 xadvance=7 page=0
"#;
        assert!(parse_bmfont_text(src).is_err());
    }
}
