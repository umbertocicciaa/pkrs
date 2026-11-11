//! The `AtlasPacker` port (§4.5, §5.2): packs images into texture atlas pages for release
//! builds. The adapter is [`TexturePackerAdapter`](crate::backend::TexturePackerAdapter).
//!
//! Packing must not change how an image looks: no trimming, no rotation, and each image
//! keeps its exact pixels and size on the page.

use std::collections::BTreeMap;

/// One image to pack.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AtlasSource {
    /// Key reported back in [`PackedAtlas::rects`] (the image's project path).
    pub key: String,
    /// The encoded image (PNG).
    pub png: Vec<u8>,
}

/// Page size and spacing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AtlasLimits {
    /// Largest page width in pixels.
    pub max_width: u32,
    /// Largest page height in pixels.
    pub max_height: u32,
    /// Transparent pixels between images.
    pub padding: u32,
}

impl Default for AtlasLimits {
    /// 2048×2048 pages (safe on every GPU the runtime targets) with 1 pixel of padding.
    fn default() -> Self {
        Self {
            max_width: 2048,
            max_height: 2048,
            padding: 1,
        }
    }
}

/// One atlas page image.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PackedPage {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// The page, PNG-encoded (RGBA8).
    pub png: Vec<u8>,
}

/// Where an image ended up.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PackedRect {
    /// Index into [`PackedAtlas::pages`].
    pub page: usize,
    /// Left edge on the page.
    pub x: u32,
    /// Top edge on the page.
    pub y: u32,
    /// Width (the image's width).
    pub width: u32,
    /// Height (the image's height).
    pub height: u32,
}

/// An image that was left out of the atlas (it stays a file of its own).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Unpacked {
    /// The image's key.
    pub key: String,
    /// Why it was not packed.
    pub reason: String,
}

/// The result of packing.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PackedAtlas {
    /// Page images.
    pub pages: Vec<PackedPage>,
    /// Packed images by key.
    pub rects: BTreeMap<String, PackedRect>,
    /// Images that were not packed (too large for a page, or not decodable).
    pub unpacked: Vec<Unpacked>,
}

/// Packing failed as a whole.
#[derive(Debug, thiserror::Error)]
#[error("atlas packing failed: {0}")]
pub struct AtlasError(pub String);

/// Engine-owned port for atlas packing.
pub trait AtlasPacker {
    /// Packs `sources` (in order) onto as many pages as needed. Images that cannot be packed
    /// are reported in [`PackedAtlas::unpacked`], not as errors.
    fn pack(&self, sources: &[AtlasSource], limits: AtlasLimits)
    -> Result<PackedAtlas, AtlasError>;
}
