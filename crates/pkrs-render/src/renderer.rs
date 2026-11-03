//! The `Renderer` facade (§4.5, §5.1).

use pkrs_platform::DisplaySettings;

use crate::backend::RenderBackend;
use crate::camera::Camera;
use crate::color::Color;
use crate::draw::SpriteDraw;
use crate::viewport::{ViewportLayout, ViewportMode, compute_viewport};

/// Engine policy applied on top of [`RenderBackend`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RendererConfig {
    /// Letterbox / pillarbox bars around the integer-scaled image.
    pub letterbox: Color,
    /// Clear colour on the internal render target each frame.
    pub clear: Color,
    /// Integer vs fit scaling (§5.1).
    pub viewport_mode: ViewportMode,
}

impl Default for RendererConfig {
    fn default() -> Self {
        Self {
            letterbox: Color::BLACK,
            clear: Color::BLACK,
            viewport_mode: ViewportMode::Integer,
        }
    }
}

/// Game-facing renderer: internal target, viewport, layer ordering.
pub struct Renderer {
    backend: Box<dyn RenderBackend>,
    display: DisplaySettings,
    config: RendererConfig,
    camera: Camera,
    pending: Vec<SpriteDraw>,
}

impl Renderer {
    /// Wraps `backend` with project display settings and policy.
    pub fn new(
        backend: Box<dyn RenderBackend>,
        display: DisplaySettings,
        config: RendererConfig,
    ) -> Self {
        Self {
            backend,
            display,
            config,
            camera: Camera::default(),
            pending: Vec::new(),
        }
    }

    /// Internal resolution from `pkrs.toml [display]`.
    pub fn display(&self) -> &DisplaySettings {
        &self.display
    }

    /// Facade configuration (letterbox, scale mode, …).
    pub fn config(&self) -> &RendererConfig {
        &self.config
    }

    /// The active camera (interpolated); use [`Camera::snapped`] when drawing.
    pub fn camera(&self) -> &Camera {
        &self.camera
    }

    /// Replaces the camera (e.g. after simulation step + interpolation).
    pub fn set_camera(&mut self, camera: Camera) {
        self.camera = camera;
    }

    /// Computes viewport layout for the current window size.
    pub fn viewport_for_window(&self, window_width: u32, window_height: u32) -> ViewportLayout {
        compute_viewport(
            self.display.width,
            self.display.height,
            window_width,
            window_height,
            self.config.viewport_mode,
        )
    }

    /// Starts a frame: resize internal target, letterbox, clear.
    pub fn begin_frame(&mut self, window_width: u32, window_height: u32) {
        self.backend
            .resize_internal(self.display.width, self.display.height);
        let viewport = self.viewport_for_window(window_width, window_height);
        self.backend.begin_frame(
            window_width,
            window_height,
            &viewport,
            self.config.letterbox,
        );
        self.backend.clear_internal(self.config.clear);
        self.pending.clear();
    }

    /// Queues a sprite draw (flushed in layer order in [`end_frame`](Self::end_frame)).
    pub fn draw_sprite(&mut self, draw: SpriteDraw) {
        self.pending.push(draw);
    }

    /// Submits queued draws and presents to the window.
    pub fn end_frame(&mut self) {
        self.pending.sort_by_key(|d| d.layer);
        for draw in &self.pending {
            self.backend.draw_sprite(draw);
        }
        self.pending.clear();
        self.backend.end_frame();
    }

    /// Uploads an RGBA8 sprite sheet through the backend.
    pub fn create_texture(
        &mut self,
        width: u32,
        height: u32,
        rgba: &[u8],
    ) -> crate::Result<crate::texture::TextureHandle> {
        self.backend.create_texture(width, height, rgba)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::NullRenderer;
    use crate::layer::Layer;
    use crate::rect::Rect;
    use crate::texture::TextureHandle;

    #[test]
    fn flushes_draws_sorted_by_layer() {
        let null = NullRenderer::new();
        let log = null.draw_log();
        let mut renderer = Renderer::new(
            Box::new(null),
            DisplaySettings::default(),
            RendererConfig::default(),
        );
        renderer.begin_frame(720, 480);
        renderer.draw_sprite(SpriteDraw {
            texture: TextureHandle(1),
            src: Rect::new(0.0, 0.0, 8.0, 8.0),
            dst: Rect::new(0.0, 0.0, 8.0, 8.0),
            flip_x: false,
            layer: Layer::Ui,
        });
        renderer.draw_sprite(SpriteDraw {
            texture: TextureHandle(1),
            src: Rect::new(0.0, 0.0, 8.0, 8.0),
            dst: Rect::new(0.0, 0.0, 8.0, 8.0),
            flip_x: false,
            layer: Layer::Ground,
        });
        renderer.end_frame();
        let layers: Vec<_> = log.draws().iter().map(|d| d.layer).collect();
        assert_eq!(layers, vec![Layer::Ground, Layer::Ui]);
    }
}
