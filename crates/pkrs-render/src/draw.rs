//! Draw commands passed to [`RenderBackend`](crate::backend::RenderBackend).

use crate::layer::Layer;
use crate::rect::Rect;
use crate::texture::TextureHandle;

/// One sprite blit in internal pixel coordinates.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SpriteDraw {
    /// Source texture.
    pub texture: TextureHandle,
    /// Source region in texels.
    pub src: Rect,
    /// Destination on the internal render target.
    pub dst: Rect,
    /// Mirror horizontally (e.g. `walk_left` reused for right).
    pub flip_x: bool,
    /// Draw order band.
    pub layer: Layer,
}
