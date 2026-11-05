//! Renderer facade and `RenderBackend` port: render target, sprite batch, atlases, layers, camera, text and materials.
//!
//! See `docs/design.md` §4.5 and §5.1 for crate boundaries.

mod animation;
mod backend;
mod camera;
mod color;
mod draw;
mod error;
mod layer;
mod rect;
mod renderer;
mod text;
mod texture;
mod tilemap;
mod viewport;

pub use animation::{
    AnimationClip, ClipPlayer, DirectionalClip, SpriteSheetAnimation, clip_for_direction,
    parse_animation_descriptor,
};
#[cfg(feature = "macroquad")]
pub use backend::MacroquadRenderer;
pub use backend::{DrawLog, NullRenderer, RecordedDraw, RenderBackend};
pub use camera::{Camera, CameraBounds, CameraController, CameraTarget, snap};
pub use color::Color;
pub use draw::SpriteDraw;
pub use error::{RenderError, Result};
pub use layer::Layer;
pub use rect::Rect;
pub use renderer::{Renderer, RendererConfig};
pub use text::{
    BitmapFont, TextMetrics, TextStyle, measure_text, parse_bmfont_text, parse_glyph_map_ron,
    wrap_text,
};
pub use texture::TextureHandle;
pub use tilemap::{
    EntitySprite, TileGrid, TileInstance, TileSheet, tile_layer_draws, visible_tiles, y_sort,
};
pub use viewport::{ViewportLayout, ViewportMode, compute_viewport};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_display_is_human_readable() {
        let err: RenderError =
            RenderError::Assets(pkrs_assets::AssetsError::NotFound("atlas.png".into()));
        assert_eq!(err.to_string(), "asset not found: atlas.png");
    }

    #[test]
    fn result_alias_defaults_to_crate_error() {
        let ok: Result<u8> = Ok(1);
        assert!(matches!(ok, Ok(1)));
    }
}
