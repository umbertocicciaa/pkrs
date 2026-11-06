//! `NullRenderer`: records draw calls for tests and headless runs (§4.5, §15.2).

use std::cell::RefCell;
use std::rc::Rc;

use super::RenderBackend;
use crate::Result;
use crate::color::Color;
use crate::draw::SpriteDraw;
use crate::layer::Layer;
use crate::texture::TextureHandle;
use crate::viewport::ViewportLayout;

/// One recorded sprite draw (for assertions).
#[derive(Clone, Debug, PartialEq)]
pub struct RecordedDraw {
    /// When set, this is a solid `dst` with `tint` (ignore texture/src).
    pub is_filled_rect: bool,
    /// Layer the sprite was drawn on.
    pub layer: Layer,
    /// Texture handle.
    pub texture: TextureHandle,
    /// Source region in texels.
    pub src: crate::rect::Rect,
    /// Destination rectangle on the internal target.
    pub dst: crate::rect::Rect,
    /// Horizontal flip flag.
    pub flip_x: bool,
    /// Tint applied to the sprite.
    pub tint: Color,
}

/// Shared log of recorded draws (clone before moving the backend into a [`Renderer`](crate::Renderer)).
#[derive(Clone, Debug, Default)]
pub struct DrawLog(Rc<RefCell<Vec<RecordedDraw>>>);

impl DrawLog {
    /// Returns the recorded draws in submission order.
    pub fn draws(&self) -> std::cell::Ref<'_, [RecordedDraw]> {
        std::cell::Ref::map(self.0.borrow(), Vec::as_slice)
    }

    /// Clears the log.
    pub fn clear(&self) {
        self.0.borrow_mut().clear();
    }
}

/// Headless backend that records sprite draws and ignores GPU work.
#[derive(Clone, Debug)]
pub struct NullRenderer {
    log: DrawLog,
    internal: (u32, u32),
    last_viewport: ViewportLayout,
    next_texture_id: u32,
}

impl Default for NullRenderer {
    fn default() -> Self {
        Self::new()
    }
}

impl NullRenderer {
    /// Creates an empty recorder.
    pub fn new() -> Self {
        Self {
            log: DrawLog::default(),
            internal: (240, 160),
            last_viewport: ViewportLayout {
                scale: 1.0,
                dest: crate::rect::Rect::new(0.0, 0.0, 240.0, 160.0),
            },
            next_texture_id: 1,
        }
    }

    /// Clones the draw log so tests can inspect it after the backend is boxed.
    pub fn draw_log(&self) -> DrawLog {
        self.log.clone()
    }
}

impl RenderBackend for NullRenderer {
    fn resize_internal(&mut self, width: u32, height: u32) {
        self.internal = (width, height);
    }

    fn begin_frame(&mut self, viewport: &ViewportLayout, _letterbox: Color) {
        self.last_viewport = *viewport;
    }

    fn clear_internal(&mut self, _color: Color) {}

    fn create_texture(&mut self, _width: u32, _height: u32, _rgba: &[u8]) -> Result<TextureHandle> {
        let id = self.next_texture_id;
        self.next_texture_id = self.next_texture_id.saturating_add(1).max(1);
        Ok(TextureHandle(id))
    }

    fn create_texture_from_image(&mut self, encoded: &[u8]) -> Result<(TextureHandle, u32, u32)> {
        let (width, height) = png_size(encoded)?;
        let handle = self.create_texture(width, height, &[])?;
        Ok((handle, width, height))
    }

    fn draw_sprite(&mut self, draw: &SpriteDraw) {
        self.log.0.borrow_mut().push(RecordedDraw {
            is_filled_rect: false,
            layer: draw.layer,
            texture: draw.texture,
            src: draw.src,
            dst: draw.dst,
            flip_x: draw.flip_x,
            tint: draw.tint,
        });
    }

    fn draw_filled_rect(&mut self, dst: crate::rect::Rect, color: Color, layer: Layer) {
        self.log.0.borrow_mut().push(RecordedDraw {
            is_filled_rect: true,
            layer,
            texture: TextureHandle(0),
            src: crate::rect::Rect::new(0.0, 0.0, 0.0, 0.0),
            dst,
            flip_x: false,
            tint: color,
        });
    }

    fn end_frame(&mut self) {}
}

/// Reads the width and height from a PNG header without decoding pixels.
pub(crate) fn png_size(bytes: &[u8]) -> Result<(u32, u32)> {
    const SIGNATURE: &[u8] = b"\x89PNG\r\n\x1a\n";
    let bad = || crate::RenderError::Backend("not a PNG image".into());
    if bytes.len() < 24 || &bytes[..8] != SIGNATURE || &bytes[12..16] != b"IHDR" {
        return Err(bad());
    }
    let read = |at: usize| -> Result<u32> {
        let raw: [u8; 4] = bytes[at..at + 4].try_into().map_err(|_| bad())?;
        Ok(u32::from_be_bytes(raw))
    };
    let (w, h) = (read(16)?, read(20)?);
    if w == 0 || h == 0 {
        return Err(bad());
    }
    Ok((w, h))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::color::Color;
    use crate::rect::Rect;

    #[test]
    fn png_header_gives_the_size() {
        let mut png = b"\x89PNG\r\n\x1a\n\0\0\0\x0dIHDR".to_vec();
        png.extend_from_slice(&64u32.to_be_bytes());
        png.extend_from_slice(&32u32.to_be_bytes());
        let mut backend = NullRenderer::new();
        let (_, w, h) = backend.create_texture_from_image(&png).unwrap();
        assert_eq!((w, h), (64, 32));
        assert!(backend.create_texture_from_image(b"GIF89a").is_err());
        assert!(backend.create_texture_from_image(&png[..20]).is_err());
    }

    #[test]
    fn records_draws_in_submission_order() {
        let mut backend = NullRenderer::new();
        let log = backend.draw_log();
        backend.draw_sprite(&SpriteDraw {
            texture: TextureHandle(1),
            src: Rect::new(0.0, 0.0, 8.0, 8.0),
            dst: Rect::new(1.0, 2.0, 8.0, 8.0),
            flip_x: false,
            tint: Color::WHITE,
            layer: Layer::Entities,
        });
        backend.draw_sprite(&SpriteDraw {
            texture: TextureHandle(1),
            src: Rect::new(0.0, 0.0, 8.0, 8.0),
            dst: Rect::new(3.0, 4.0, 8.0, 8.0),
            flip_x: true,
            tint: Color::WHITE,
            layer: Layer::Ui,
        });
        assert_eq!(log.draws().len(), 2);
        assert_eq!(log.draws()[0].layer, Layer::Entities);
        assert!(log.draws()[1].flip_x);
    }
}
