//! 9-slice window skin (§5.5).

use pkrs_render::{Color, Layer, Rect, Renderer, SpriteDraw, TextureHandle};

use crate::UiError;

/// Border width in texels: corners are `border×border`, center is the remainder (defaults match a 32×32 skin with 11px frame).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NineSliceSpec {
    /// Edge and corner size in texels (must fit twice in the atlas).
    pub border: u32,
}

impl Default for NineSliceSpec {
    fn default() -> Self {
        Self { border: 11 }
    }
}

/// Loaded window skin: texture + slice geometry.
#[derive(Clone, Debug)]
pub struct WindowSkin {
    /// Atlas texture handle.
    pub texture: TextureHandle,
    /// Atlas width / height in texels.
    pub atlas_size: (u32, u32),
    /// Border size in texels.
    pub spec: NineSliceSpec,
}

impl WindowSkin {
    /// Builds a skin from an uploaded texture; validates dimensions against `spec`.
    pub fn new(
        texture: TextureHandle,
        atlas_w: u32,
        atlas_h: u32,
        spec: NineSliceSpec,
    ) -> Result<Self, UiError> {
        let b = spec.border;
        if b == 0 {
            return Err(UiError::InvalidSkin("border must be non-zero".into()));
        }
        let min_w = b.saturating_mul(2);
        let min_h = b.saturating_mul(2);
        if atlas_w < min_w || atlas_h < min_h {
            return Err(UiError::InvalidSkin(format!(
                "atlas {atlas_w}x{atlas_h} too small for border {b}"
            )));
        }
        Ok(Self {
            texture,
            atlas_size: (atlas_w, atlas_h),
            spec,
        })
    }

    /// Draws the skin into `dst_outer` on the UI layer.
    pub fn draw(&self, renderer: &mut Renderer, dst_outer: Rect, tint: Color) {
        let (w, h) = self.atlas_size;
        let wf = w as f32;
        let hf = h as f32;
        let b = self.spec.border as f32;

        let r = SliceSrc {
            tl: Rect::new(0.0, 0.0, b, b),
            tr: Rect::new(wf - b, 0.0, b, b),
            bl: Rect::new(0.0, hf - b, b, b),
            br: Rect::new(wf - b, hf - b, b, b),
            top: Rect::new(b, 0.0, (wf - 2.0 * b).max(0.0), b),
            bottom: Rect::new(b, hf - b, (wf - 2.0 * b).max(0.0), b),
            left: Rect::new(0.0, b, b, (hf - 2.0 * b).max(0.0)),
            right: Rect::new(wf - b, b, b, (hf - 2.0 * b).max(0.0)),
            center: Rect::new(b, b, (wf - 2.0 * b).max(0.0), (hf - 2.0 * b).max(0.0)),
        };

        let dw = dst_outer.width;
        let dh = dst_outer.height;
        let c_dest = b.min((dw * 0.5).floor()).min((dh * 0.5).floor());

        let mut blit = |src: Rect, dx: f32, dy: f32, rw: f32, rh: f32| {
            if rw <= 0.0 || rh <= 0.0 {
                return;
            }
            renderer.draw_sprite(SpriteDraw {
                texture: self.texture,
                src,
                dst: Rect::new(dst_outer.x + dx, dst_outer.y + dy, rw, rh),
                flip_x: false,
                tint,
                layer: Layer::Ui,
            });
        };

        blit(r.tl, 0.0, 0.0, c_dest, c_dest);
        blit(r.tr, dw - c_dest, 0.0, c_dest, c_dest);
        blit(r.bl, 0.0, dh - c_dest, c_dest, c_dest);
        blit(r.br, dw - c_dest, dh - c_dest, c_dest, c_dest);

        blit(r.top, c_dest, 0.0, (dw - 2.0 * c_dest).max(0.0), c_dest);
        blit(
            r.bottom,
            c_dest,
            dh - c_dest,
            (dw - 2.0 * c_dest).max(0.0),
            c_dest,
        );
        blit(r.left, 0.0, c_dest, c_dest, (dh - 2.0 * c_dest).max(0.0));
        blit(
            r.right,
            dw - c_dest,
            c_dest,
            c_dest,
            (dh - 2.0 * c_dest).max(0.0),
        );
        blit(
            r.center,
            c_dest,
            c_dest,
            (dw - 2.0 * c_dest).max(0.0),
            (dh - 2.0 * c_dest).max(0.0),
        );
    }

    /// Single stretched quad (1×1 src) for tests without a skin atlas.
    pub fn draw_fallback_panel(
        renderer: &mut Renderer,
        texture: TextureHandle,
        dst: Rect,
        tint: Color,
    ) {
        renderer.draw_sprite(SpriteDraw {
            texture,
            src: Rect::new(0.0, 0.0, 1.0, 1.0),
            dst,
            flip_x: false,
            tint,
            layer: Layer::Ui,
        });
    }
}

/// Small "more" indicator (three blips) for dialogue paging (§5.5).
pub fn draw_more_arrow(
    renderer: &mut Renderer,
    texture: TextureHandle,
    x: f32,
    y: f32,
    tint: Color,
) {
    for (dx, dy) in [(0.0, 4.0), (4.0, 0.0), (8.0, 4.0)] {
        renderer.draw_sprite(SpriteDraw {
            texture,
            src: Rect::new(0.0, 0.0, 1.0, 1.0),
            dst: Rect::new(x + dx, y + dy, 2.0, 2.0),
            flip_x: false,
            tint,
            layer: Layer::Ui,
        });
    }
}

struct SliceSrc {
    tl: Rect,
    tr: Rect,
    bl: Rect,
    br: Rect,
    top: Rect,
    bottom: Rect,
    left: Rect,
    right: Rect,
    center: Rect,
}
