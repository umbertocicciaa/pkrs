//! `TexturePackerAdapter`: the [`AtlasPacker`] port on `texture_packer` (skyline packing)
//! and `image` (PNG decode/encode). The only module that uses either crate (§4.5).

use std::io::Cursor;

use image::{DynamicImage, GenericImageView as _, ImageFormat};
use texture_packer::exporter::ImageExporter;
use texture_packer::importer::ImageImporter;
use texture_packer::{MultiTexturePacker, TexturePackerConfig};

use crate::atlas::{
    AtlasError, AtlasLimits, AtlasPacker, AtlasSource, PackedAtlas, PackedPage, PackedRect,
    Unpacked,
};

/// Skyline atlas packer (`texture_packer`).
#[derive(Clone, Copy, Debug, Default)]
pub struct TexturePackerAdapter;

impl AtlasPacker for TexturePackerAdapter {
    fn pack(
        &self,
        sources: &[AtlasSource],
        limits: AtlasLimits,
    ) -> Result<PackedAtlas, AtlasError> {
        let config = TexturePackerConfig {
            max_width: limits.max_width,
            max_height: limits.max_height,
            allow_rotation: false,
            force_max_dimensions: false,
            border_padding: 0,
            texture_padding: limits.padding,
            texture_extrusion: 0,
            trim: false,
            texture_outlines: false,
        };
        let mut packer: MultiTexturePacker<'_, DynamicImage, String> =
            MultiTexturePacker::new_skyline(config);
        let mut out = PackedAtlas::default();
        for source in sources {
            let image = match ImageImporter::import_from_memory(&source.png) {
                Ok(image) => image,
                Err(err) => {
                    out.unpacked.push(Unpacked {
                        key: source.key.clone(),
                        reason: format!("cannot decode the image: {err}"),
                    });
                    continue;
                }
            };
            let (w, h) = image.dimensions();
            if w > limits.max_width || h > limits.max_height {
                out.unpacked.push(Unpacked {
                    key: source.key.clone(),
                    reason: format!(
                        "{w}x{h} is larger than an atlas page ({}x{})",
                        limits.max_width, limits.max_height
                    ),
                });
                continue;
            }
            let rgba = DynamicImage::ImageRgba8(image.to_rgba8());
            // texture_packer needs a pixel of slack below a page's edge, so an image as
            // large as a page is refused here: it stays a file of its own.
            if let Err(err) = packer.pack_own(source.key.clone(), rgba) {
                out.unpacked.push(Unpacked {
                    key: source.key.clone(),
                    reason: format!("does not fit on an atlas page ({err:?})"),
                });
            }
        }
        for (number, page) in packer.get_pages().iter().enumerate() {
            let image = ImageExporter::export(page, None).map_err(AtlasError)?;
            let (width, height) = image.dimensions();
            let mut png = Vec::new();
            image
                .write_to(&mut Cursor::new(&mut png), ImageFormat::Png)
                .map_err(|e| AtlasError(format!("cannot encode atlas page {number}: {e}")))?;
            for frame in page.get_frames().values() {
                out.rects.insert(
                    frame.key.clone(),
                    PackedRect {
                        page: number,
                        x: frame.frame.x,
                        y: frame.frame.y,
                        width: frame.frame.w,
                        height: frame.frame.h,
                    },
                );
            }
            out.pages.push(PackedPage { width, height, png });
        }
        for source in sources {
            let placed = out.rects.contains_key(&source.key)
                || out.unpacked.iter().any(|u| u.key == source.key);
            if !placed {
                out.unpacked.push(Unpacked {
                    key: source.key.clone(),
                    reason: "the packer found no place for it".into(),
                });
            }
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    /// A `w`×`h` PNG where pixel (x, y) is `(x, y, seed, 255)`.
    pub(crate) fn png(w: u32, h: u32, seed: u8) -> Vec<u8> {
        let img =
            image::RgbaImage::from_fn(w, h, |x, y| image::Rgba([x as u8, y as u8, seed, 255]));
        let mut out = Vec::new();
        DynamicImage::ImageRgba8(img)
            .write_to(&mut Cursor::new(&mut out), ImageFormat::Png)
            .unwrap();
        out
    }

    fn decode(png: &[u8]) -> image::RgbaImage {
        image::load_from_memory(png).unwrap().to_rgba8()
    }

    fn source(key: &str, png: Vec<u8>) -> AtlasSource {
        AtlasSource {
            key: key.into(),
            png,
        }
    }

    #[test]
    fn packed_images_keep_their_exact_pixels() {
        let sources = [
            source("a.png", png(16, 8, 1)),
            source("b.png", png(8, 24, 2)),
            source("c.png", png(5, 3, 3)),
        ];
        let atlas = TexturePackerAdapter
            .pack(&sources, AtlasLimits::default())
            .unwrap();
        assert_eq!(atlas.pages.len(), 1);
        assert!(atlas.unpacked.is_empty());
        let page = decode(&atlas.pages[0].png);
        assert_eq!(
            (page.width(), page.height()),
            (atlas.pages[0].width, atlas.pages[0].height)
        );
        for src in &sources {
            let r = atlas.rects[&src.key];
            let original = decode(&src.png);
            assert_eq!((r.width, r.height), original.dimensions(), "{}", src.key);
            for (x, y, px) in original.enumerate_pixels() {
                assert_eq!(
                    page.get_pixel(r.x + x, r.y + y),
                    px,
                    "{} at {x},{y}",
                    src.key
                );
            }
        }
    }

    #[test]
    fn rects_do_not_overlap_and_respect_padding() {
        let sources: Vec<_> = (0..6)
            .map(|i| source(&format!("{i}.png"), png(10, 10, i)))
            .collect();
        let atlas = TexturePackerAdapter
            .pack(&sources, AtlasLimits::default())
            .unwrap();
        let rects: Vec<_> = atlas.rects.values().collect();
        for (i, a) in rects.iter().enumerate() {
            for b in &rects[i + 1..] {
                let apart = a.x + a.width < b.x
                    || b.x + b.width < a.x
                    || a.y + a.height < b.y
                    || b.y + b.height < a.y;
                assert!(apart, "{a:?} touches {b:?}");
            }
        }
    }

    #[test]
    fn overflow_goes_to_new_pages_and_oversized_images_stay_unpacked() {
        let limits = AtlasLimits {
            max_width: 40,
            max_height: 40,
            padding: 0,
        };
        let sources = [
            source("a.png", png(32, 32, 1)),
            source("b.png", png(32, 32, 2)),
            source("huge.png", png(48, 8, 3)),
            source("broken.png", b"not a png".to_vec()),
            source("full.png", png(40, 40, 4)),
        ];
        let atlas = TexturePackerAdapter.pack(&sources, limits).unwrap();
        assert_eq!(atlas.pages.len(), 2);
        assert_ne!(atlas.rects["a.png"].page, atlas.rects["b.png"].page);
        let unpacked: Vec<_> = atlas.unpacked.iter().map(|u| u.key.as_str()).collect();
        assert_eq!(unpacked, ["huge.png", "broken.png", "full.png"]);
        assert!(
            atlas.unpacked[0]
                .reason
                .contains("larger than an atlas page")
        );
        assert!(!atlas.rects.contains_key("full.png"));
    }

    #[test]
    fn packing_is_deterministic() {
        let sources: Vec<_> = (0..5)
            .map(|i| source(&format!("{i}.png"), png(4 + u32::from(i), 6, i)))
            .collect();
        let a = TexturePackerAdapter
            .pack(&sources, AtlasLimits::default())
            .unwrap();
        let b = TexturePackerAdapter
            .pack(&sources, AtlasLimits::default())
            .unwrap();
        assert_eq!(a, b);
    }
}
