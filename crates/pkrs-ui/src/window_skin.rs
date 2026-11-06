//! 9-slice window skin (§5.5).

use pkrs_render::{Color, Layer, Rect, Renderer, SpriteDraw, TextureHandle};

use crate::UiError;

/// Corner size and edge strip thickness in texels (defaults match a 32×32 skin: corners 11, edges 11).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NineSliceSpec {
    /// Corner square size.
    pub corner: u32,
    /// Edge strip thickness (top/bottom height, left/right width).
    pub edge: u32,
}

impl Default for NineSliceSpec {
    fn default() -> Self {
        Self {
            corner: 11,
            edge: 11,
        }
    }
}

/// Loaded window skin: texture + slice geometry.
#[derive(Clone, Debug)]
pub struct WindowSkin {
    /// Atlas texture handle.
    pub texture: TextureHandle,
    /// Atlas width / height in texels.
    pub atlas_size: (u32, u32),
    /// Corner and edge slice sizes in texels.
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
        let c = spec.corner;
        let e = spec.edge;
        if c == 0 || e == 0 {
            return Err(UiError::InvalidSkin(
                "corner and edge must be non-zero".into(),
            ));
        }
        let min_w = c.saturating_mul(2);
        let min_h = c.saturating_mul(2);
        if atlas_w < min_w || atlas_h < min_h {
            return Err(UiError::InvalidSkin(format!(
                "atlas {atlas_w}x{atlas_h} too small for corner {c}"
            )));
        }
        if e.saturating_mul(2) > atlas_w || e.saturating_mul(2) > atlas_h {
            return Err(UiError::InvalidSkin(format!(
                "edge {e} does not fit atlas {atlas_w}x{atlas_h}"
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
        let c = self.spec.corner as f32;
        let e = self.spec.edge as f32;

        let r = SliceSrc {
            tl: Rect::new(0.0, 0.0, c, c),
            tr: Rect::new(wf - c, 0.0, c, c),
            bl: Rect::new(0.0, hf - c, c, c),
            br: Rect::new(wf - c, hf - c, c, c),
            top: Rect::new(c, 0.0, (wf - 2.0 * c).max(0.0), e),
            bottom: Rect::new(c, hf - e, (wf - 2.0 * c).max(0.0), e),
            left: Rect::new(0.0, c, e, (hf - 2.0 * c).max(0.0)),
            right: Rect::new(wf - e, c, e, (hf - 2.0 * c).max(0.0)),
            center: Rect::new(e, e, (wf - 2.0 * e).max(0.0), (hf - 2.0 * e).max(0.0)),
        };

        let dw = dst_outer.width;
        let dh = dst_outer.height;
        let c_dest = c.min(dw * 0.5).min(dh * 0.5);
        let e_h = e
            .min((dh - 2.0 * c_dest).max(0.0) * 0.5 + 0.0)
            .min(dh * 0.5);
        let e_w = e
            .min((dw - 2.0 * c_dest).max(0.0) * 0.5 + 0.0)
            .min(dw * 0.5);

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

        blit(r.top, c_dest, 0.0, (dw - 2.0 * c_dest).max(0.0), e_h);
        blit(
            r.bottom,
            c_dest,
            dh - e_h,
            (dw - 2.0 * c_dest).max(0.0),
            e_h,
        );
        blit(r.left, 0.0, c_dest, e_w, (dh - 2.0 * c_dest).max(0.0));
        blit(r.right, dw - e_w, c_dest, e_w, (dh - 2.0 * c_dest).max(0.0));
        blit(
            r.center,
            e_w,
            e_h,
            (dw - 2.0 * e_w).max(0.0),
            (dh - 2.0 * e_h).max(0.0),
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
