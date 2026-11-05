//! Bitmap font metrics and glyph lookup (§5.5).

use std::collections::BTreeMap;

use crate::error::{RenderError, Result};
use crate::rect::Rect;
use crate::text::util;

/// Metrics for one glyph in the atlas.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GlyphMetrics {
    /// Source rectangle in the font atlas (texels).
    pub src: Rect,
    /// Left bearing from cursor (pixels).
    pub xoffset: i32,
    /// Offset from the top of the line box to the top of the glyph bitmap (pixels).
    pub yoffset: i32,
    /// Horizontal advance after this glyph.
    pub xadvance: u32,
}

/// Variable-width bitmap font backed by a single atlas texture.
#[derive(Clone, Debug, PartialEq)]
pub struct BitmapFont {
    /// Distance between baselines (line box height).
    pub line_height: u32,
    /// Baseline distance from the top of the line box (BMFont `base`).
    pub base: u32,
    /// Atlas width in texels.
    pub atlas_width: u32,
    /// Atlas height in texels.
    pub atlas_height: u32,
    /// Glyphs keyed by Unicode scalar value.
    glyphs: BTreeMap<u32, GlyphMetrics>,
    /// Pair kerning adjustments (first, second) → delta in pixels.
    kerning: BTreeMap<(u32, u32), i32>,
}

impl BitmapFont {
    /// Creates a font from parsed glyph data (atlas bounds validated).
    pub(crate) fn from_glyphs(
        line_height: u32,
        base: u32,
        atlas_width: u32,
        atlas_height: u32,
        glyphs: BTreeMap<u32, GlyphMetrics>,
        kerning: BTreeMap<(u32, u32), i32>,
    ) -> Result<Self> {
        for metrics in glyphs.values() {
            validate_glyph_in_atlas(atlas_width, atlas_height, metrics)?;
        }
        Ok(Self {
            line_height,
            base,
            atlas_width,
            atlas_height,
            glyphs,
            kerning,
        })
    }

    /// Looks up a glyph by Unicode scalar.
    pub fn glyph(&self, codepoint: u32) -> Option<&GlyphMetrics> {
        self.glyphs.get(&codepoint)
    }

    /// Missing code points use `?` when present.
    pub fn resolve_glyph(&self, ch: char) -> Option<&GlyphMetrics> {
        self.glyph(u32::from(ch))
            .or_else(|| self.glyph(u32::from('?')))
    }

    /// Horizontal advance in pixels (includes kerning when `previous` is set).
    pub fn advance(&self, previous: Option<char>, ch: char) -> f32 {
        let Some(metrics) = self.resolve_glyph(ch) else {
            return 0.0;
        };
        let mut adv = i32::try_from(metrics.xadvance).unwrap_or(i32::MAX);
        if let Some(prev) = previous {
            if let Some(delta) = self.kerning.get(&(u32::from(prev), u32::from(ch))) {
                adv = adv.saturating_add(*delta);
            }
        }
        adv.max(0) as f32
    }
}

fn validate_glyph_in_atlas(
    atlas_width: u32,
    atlas_height: u32,
    metrics: &GlyphMetrics,
) -> Result<()> {
    let aw = util::u32_px(atlas_width);
    let ah = util::u32_px(atlas_height);
    let right = metrics.src.x + metrics.src.width;
    let bottom = metrics.src.y + metrics.src.height;
    if metrics.src.x < 0.0
        || metrics.src.y < 0.0
        || right > aw
        || bottom > ah
        || metrics.src.width <= 0.0
        || metrics.src.height <= 0.0
    {
        return Err(RenderError::FontInvalid {
            detail: format!(
                "glyph rect ({},{},{},{}) outside {atlas_width}x{atlas_height} atlas",
                metrics.src.x, metrics.src.y, metrics.src.width, metrics.src.height
            ),
        });
    }
    Ok(())
}

pub(crate) fn insert_glyph(
    glyphs: &mut BTreeMap<u32, GlyphMetrics>,
    codepoint: u32,
    metrics: GlyphMetrics,
) -> Result<()> {
    if glyphs.insert(codepoint, metrics).is_some() {
        return Err(RenderError::FontInvalid {
            detail: format!("duplicate glyph id {codepoint}"),
        });
    }
    Ok(())
}
