//! `MacroquadRenderer`: the `RenderBackend` adapter over macroquad (§4.5).
//!
//! This is the **only** module of `pkrs-render` that imports `macroquad`.

use std::collections::HashMap;

use ::macroquad::prelude::{
    Camera2D, DrawTextureParams, FilterMode, RenderTarget, Texture2D, draw_texture_ex,
    render_target, set_camera, set_default_camera,
};

use super::RenderBackend;
use crate::color::Color;
use crate::draw::SpriteDraw;
use crate::texture::TextureHandle;
use crate::viewport::ViewportLayout;
use crate::{RenderError, Result};

fn to_mq(color: Color) -> ::macroquad::prelude::Color {
    ::macroquad::prelude::Color::new(
        color.r as f32 / 255.0,
        color.g as f32 / 255.0,
        color.b as f32 / 255.0,
        color.a as f32 / 255.0,
    )
}

/// [`RenderBackend`] backed by macroquad render targets and batched textures.
#[derive(Debug)]
pub struct MacroquadRenderer {
    internal: Option<RenderTarget>,
    internal_size: (u32, u32),
    textures: HashMap<u32, Texture2D>,
    next_id: u32,
    viewport: ViewportLayout,
    letterbox: Color,
}

impl Default for MacroquadRenderer {
    fn default() -> Self {
        Self::new()
    }
}

impl MacroquadRenderer {
    /// Creates a renderer; the internal target is allocated on first `resize_internal`.
    pub fn new() -> Self {
        Self {
            internal: None,
            internal_size: (0, 0),
            textures: HashMap::new(),
            next_id: 1,
            viewport: ViewportLayout {
                scale: 1.0,
                dest: crate::rect::Rect::new(0.0, 0.0, 0.0, 0.0),
            },
            letterbox: Color::BLACK,
        }
    }

    fn ensure_internal(&mut self, width: u32, height: u32) {
        if self.internal_size == (width, height) && self.internal.is_some() {
            return;
        }
        let target = render_target(width, height);
        target.texture.set_filter(FilterMode::Nearest);
        self.internal = Some(target);
        self.internal_size = (width, height);
    }

    fn internal_camera(width: u32, height: u32, target: RenderTarget) -> Camera2D {
        Camera2D {
            render_target: Some(target),
            zoom: ::macroquad::prelude::vec2(2.0 / width as f32, 2.0 / height as f32),
            target: ::macroquad::prelude::vec2(width as f32 / 2.0, height as f32 / 2.0),
            ..Default::default()
        }
    }
}

impl RenderBackend for MacroquadRenderer {
    fn resize_internal(&mut self, width: u32, height: u32) {
        if width > 0 && height > 0 {
            self.ensure_internal(width, height);
        }
    }

    fn begin_frame(
        &mut self,
        window_width: u32,
        window_height: u32,
        viewport: &ViewportLayout,
        letterbox: Color,
    ) {
        self.viewport = *viewport;
        self.letterbox = letterbox;
        let _ = (window_width, window_height);
        set_default_camera();
        ::macroquad::prelude::clear_background(to_mq(letterbox));
    }

    fn clear_internal(&mut self, color: Color) {
        let Some(target) = self.internal.clone() else {
            return;
        };
        let (w, h) = self.internal_size;
        set_camera(&Self::internal_camera(w, h, target));
        ::macroquad::prelude::clear_background(to_mq(color));
        set_default_camera();
    }

    fn create_texture(&mut self, width: u32, height: u32, rgba: &[u8]) -> Result<TextureHandle> {
        let expected = (width as usize)
            .checked_mul(height as usize)
            .and_then(|n| n.checked_mul(4))
            .ok_or_else(|| RenderError::Backend("texture dimensions overflow".into()))?;
        if rgba.len() != expected {
            return Err(RenderError::Backend(format!(
                "expected {expected} rgba bytes, got {}",
                rgba.len()
            )));
        }
        let width_u16 = u16::try_from(width)
            .map_err(|_| RenderError::Backend("texture width exceeds u16".into()))?;
        let height_u16 = u16::try_from(height)
            .map_err(|_| RenderError::Backend("texture height exceeds u16".into()))?;
        let texture = Texture2D::from_rgba8(width_u16, height_u16, rgba);
        texture.set_filter(FilterMode::Nearest);
        let id = self.next_id;
        self.next_id = self.next_id.saturating_add(1).max(1);
        self.textures.insert(id, texture);
        Ok(TextureHandle(id))
    }

    fn draw_sprite(&mut self, draw: &SpriteDraw) {
        let Some(target) = self.internal.clone() else {
            return;
        };
        let Some(texture) = self.textures.get(&draw.texture.0) else {
            return;
        };
        let (w, h) = self.internal_size;
        set_camera(&Self::internal_camera(w, h, target));
        draw_texture_ex(
            texture,
            draw.dst.x,
            draw.dst.y,
            ::macroquad::prelude::WHITE,
            DrawTextureParams {
                source: Some(::macroquad::prelude::Rect::new(
                    draw.src.x,
                    draw.src.y,
                    draw.src.width,
                    draw.src.height,
                )),
                dest_size: Some(::macroquad::prelude::vec2(draw.dst.width, draw.dst.height)),
                flip_x: draw.flip_x,
                ..Default::default()
            },
        );
        set_default_camera();
    }

    fn end_frame(&mut self) {
        let Some(target) = self.internal.clone() else {
            return;
        };
        set_default_camera();
        draw_texture_ex(
            &target.texture,
            self.viewport.dest.x,
            self.viewport.dest.y,
            ::macroquad::prelude::WHITE,
            DrawTextureParams {
                dest_size: Some(::macroquad::prelude::vec2(
                    self.viewport.dest.width,
                    self.viewport.dest.height,
                )),
                ..Default::default()
            },
        );
    }
}
