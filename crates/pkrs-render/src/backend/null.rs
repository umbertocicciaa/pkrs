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
    /// Layer the sprite was drawn on.
    pub layer: Layer,
    /// Texture handle.
    pub texture: TextureHandle,
    /// Destination rectangle on the internal target.
    pub dst: crate::rect::Rect,
    /// Horizontal flip flag.
    pub flip_x: bool,
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

    fn begin_frame(
        &mut self,
        _window_width: u32,
        _window_height: u32,
        viewport: &ViewportLayout,
        _letterbox: Color,
    ) {
        self.last_viewport = *viewport;
    }

    fn clear_internal(&mut self, _color: Color) {}

    fn create_texture(&mut self, _width: u32, _height: u32, _rgba: &[u8]) -> Result<TextureHandle> {
        Ok(TextureHandle(1))
    }

    fn draw_sprite(&mut self, draw: &SpriteDraw) {
        self.log.0.borrow_mut().push(RecordedDraw {
            layer: draw.layer,
            texture: draw.texture,
            dst: draw.dst,
            flip_x: draw.flip_x,
        });
    }

    fn end_frame(&mut self) {}
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rect::Rect;

    #[test]
    fn records_draws_in_submission_order() {
        let mut backend = NullRenderer::new();
        let log = backend.draw_log();
        backend.draw_sprite(&SpriteDraw {
            texture: TextureHandle(1),
            src: Rect::new(0.0, 0.0, 8.0, 8.0),
            dst: Rect::new(1.0, 2.0, 8.0, 8.0),
            flip_x: false,
            layer: Layer::Entities,
        });
        backend.draw_sprite(&SpriteDraw {
            texture: TextureHandle(1),
            src: Rect::new(0.0, 0.0, 8.0, 8.0),
            dst: Rect::new(3.0, 4.0, 8.0, 8.0),
            flip_x: true,
            layer: Layer::Ui,
        });
        assert_eq!(log.draws().len(), 2);
        assert_eq!(log.draws()[0].layer, Layer::Entities);
        assert!(log.draws()[1].flip_x);
    }
}
