//! Bitmap font metrics and glyph lookup (§5.5).

use std::collections::BTreeMap;

use crate::rect::Rect;

/// Metrics for one glyph in the atlas.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GlyphMetrics {
    /// Source rectangle in the font atlas (texels).
    pub src: Rect,
    /// Left bearing from cursor (pixels).
    pub xoffset: i32,
    /// Top bearing from baseline (pixels).
    pub yoffset: i32,
    /// Horizontal advance after this glyph.
    pub xadvance: u32,
}

/// Variable-width bitmap font backed by a single atlas texture.
#[derive(Clone, Debug, PartialEq)]
pub struct BitmapFont {
    /// Distance between baselines.
    pub line_height: u32,
    /// Baseline offset from top of line box.
    pub base: u32,
    /// Atlas width in texels.
    pub atlas_width: u32,
    /// Atlas height in texels.
    pub atlas_height: u32,
    /// Glyphs keyed by Unicode scalar value.
    glyphs: BTreeMap<u32, GlyphMetrics>,
}

impl BitmapFont {
    /// Creates a font from validated parts (used by parsers).
    pub fn from_glyphs(
        line_height: u32,
        base: u32,
        atlas_width: u32,
        atlas_height: u32,
        glyphs: BTreeMap<u32, GlyphMetrics>,
    ) -> Self {
        Self {
            line_height,
            base,
            atlas_width,
            atlas_height,
            glyphs,
        }
    }

    /// Looks up a glyph by Unicode scalar.
    pub fn glyph(&self, codepoint: u32) -> Option<&GlyphMetrics> {
        self.glyphs.get(&codepoint)
    }

    /// Default glyph used for unknown code points (space if present, else first glyph).
    pub fn fallback_glyph(&self) -> Option<&GlyphMetrics> {
        self.glyph(' '.into())
            .or_else(|| self.glyphs.values().next())
    }
}
