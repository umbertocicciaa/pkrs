//! The `RenderBackend` port and its adapters (§4.5).

#[cfg(feature = "macroquad")]
mod macroquad;
mod null;

pub use null::{DrawLog, NullRenderer, RecordedDraw, RecordedMaterial};

#[cfg(feature = "macroquad")]
pub use self::macroquad::MacroquadRenderer;

use crate::Result;
use crate::color::Color;
use crate::draw::SpriteDraw;
use crate::layer::Layer;
use crate::material::{MaterialBind, MaterialHandle, ShaderProgram};
use crate::rect::Rect;
use crate::texture::TextureHandle;
use crate::viewport::ViewportLayout;

/// Engine-owned port over GPU / 2D drawing (§4.5).
///
/// Policy (layers, viewport mode, letterbox colour) lives in the [`Renderer`](crate::Renderer)
/// facade; adapters implement blits and render targets only.
pub trait RenderBackend {
    /// (Re)allocates the internal render target to `width` × `height`.
    fn resize_internal(&mut self, width: u32, height: u32);

    /// Starts a frame: letterbox clear using `viewport.dest` for the eventual blit.
    fn begin_frame(&mut self, viewport: &ViewportLayout, letterbox: Color);

    /// Clears the internal target (game background).
    fn clear_internal(&mut self, color: Color);

    /// Uploads an RGBA8 texture (`rgba.len()` must be `width * height * 4`).
    fn create_texture(&mut self, width: u32, height: u32, rgba: &[u8]) -> Result<TextureHandle>;

    /// Decodes an encoded image (PNG) and uploads it; returns the handle and its size in pixels.
    /// Malformed images are an error, never a panic.
    fn create_texture_from_image(&mut self, encoded: &[u8]) -> Result<(TextureHandle, u32, u32)>;

    /// Draws one sprite into the internal target (call in layer order).
    fn draw_sprite(&mut self, draw: &SpriteDraw);

    /// Compiles a material's shader program (§5.6). A backend that cannot run it returns an
    /// error; the engine then draws without the material.
    fn create_material(&mut self, program: &ShaderProgram<'_>) -> Result<MaterialHandle>;

    /// Draws one sprite with a material and its resolved uniform values. An unknown handle
    /// draws the sprite plainly.
    fn draw_sprite_with_material(&mut self, draw: &SpriteDraw, bind: &MaterialBind);

    /// Solid filled rectangle in internal pixel space (UI chrome, placeholders).
    fn draw_filled_rect(&mut self, dst: Rect, color: Color, layer: Layer);

    /// Presents the internal target scaled to the window viewport.
    fn end_frame(&mut self);
}
