//! `MacroquadRenderer`: the `RenderBackend` adapter over macroquad (§4.5).
//!
//! This is the **only** module of `pkrs-render` that imports `macroquad`.

use std::collections::HashMap;

use ::macroquad::miniquad::{
    BlendFactor, BlendState, BlendValue, Equation, PipelineParams, ShaderSource, UniformDesc,
    UniformType as MqUniformType,
};
use ::macroquad::prelude::{
    Camera2D, DrawTextureParams, FilterMode, Material as MqMaterial, MaterialParams, RenderTarget,
    Texture2D, draw_rectangle, draw_texture_ex, gl_use_default_material, gl_use_material,
    load_material, render_target, set_camera, set_default_camera,
};

use super::RenderBackend;
use crate::color::Color;
use crate::draw::SpriteDraw;
use crate::material::{MaterialBind, MaterialHandle, ShaderProgram, UniformType, UniformValue};
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
    materials: HashMap<u32, MqMaterial>,
    next_material_id: u32,
}

fn to_mq_uniform(ty: UniformType) -> Option<MqUniformType> {
    Some(match ty {
        UniformType::Float => MqUniformType::Float1,
        UniformType::Int => MqUniformType::Int1,
        UniformType::Vec2 => MqUniformType::Float2,
        UniformType::Vec3 => MqUniformType::Float3,
        UniformType::Vec4 => MqUniformType::Float4,
        UniformType::Mat4 => return None,
    })
}

fn set_uniform(material: &MqMaterial, name: &str, value: UniformValue) {
    match value {
        UniformValue::Float(v) => material.set_uniform(name, v),
        UniformValue::Int(v) => material.set_uniform(name, v),
        UniformValue::Vec2(v) => material.set_uniform(name, v),
        UniformValue::Vec3(v) => material.set_uniform(name, v),
        UniformValue::Vec4(v) => material.set_uniform(name, v),
    }
}

/// Straight alpha blending, as macroquad's default sprite pipeline.
fn alpha_blend() -> PipelineParams {
    PipelineParams {
        color_blend: Some(BlendState::new(
            Equation::Add,
            BlendFactor::Value(BlendValue::SourceAlpha),
            BlendFactor::OneMinusValue(BlendValue::SourceAlpha),
        )),
        alpha_blend: Some(BlendState::new(
            Equation::Add,
            BlendFactor::Zero,
            BlendFactor::One,
        )),
        ..Default::default()
    }
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
            materials: HashMap::new(),
            next_material_id: 1,
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

    fn begin_frame(&mut self, viewport: &ViewportLayout, letterbox: Color) {
        self.viewport = *viewport;
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
    }

    fn create_texture_from_image(&mut self, encoded: &[u8]) -> Result<(TextureHandle, u32, u32)> {
        let image = ::macroquad::texture::Image::from_file_with_format(encoded, None)
            .map_err(|e| RenderError::Backend(format!("cannot decode image: {e}")))?;
        let (width, height) = (u32::from(image.width), u32::from(image.height));
        let handle = self.create_texture(width, height, &image.bytes)?;
        Ok((handle, width, height))
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
        self.next_id = self
            .next_id
            .checked_add(1)
            .ok_or_else(|| RenderError::Backend("texture handle id overflow".into()))?;
        self.textures.insert(id, texture);
        Ok(TextureHandle(id))
    }

    fn draw_sprite(&mut self, draw: &SpriteDraw) {
        let Some(texture) = self.textures.get(&draw.texture.0) else {
            return;
        };
        blit(texture, draw);
    }

    fn create_material(&mut self, program: &ShaderProgram<'_>) -> Result<MaterialHandle> {
        let mut uniforms = Vec::with_capacity(program.uniforms.len());
        for (name, ty) in program.uniforms {
            let ty = to_mq_uniform(*ty).ok_or_else(|| {
                RenderError::Backend(format!(
                    "material `{}`: uniform `{name}` has a type the backend cannot bind",
                    program.name
                ))
            })?;
            uniforms.push(UniformDesc::new(name, ty));
        }
        let material = load_material(
            ShaderSource::Glsl {
                vertex: program.vertex,
                fragment: program.fragment,
            },
            MaterialParams {
                pipeline_params: alpha_blend(),
                uniforms,
                textures: Vec::new(),
            },
        )
        .map_err(|e| {
            RenderError::Backend(format!("material `{}` does not compile: {e}", program.name))
        })?;
        let id = self.next_material_id;
        self.next_material_id = self
            .next_material_id
            .checked_add(1)
            .ok_or_else(|| RenderError::Backend("material handle id overflow".into()))?;
        self.materials.insert(id, material);
        Ok(MaterialHandle(id))
    }

    fn draw_sprite_with_material(&mut self, draw: &SpriteDraw, bind: &MaterialBind) {
        let Some(texture) = self.textures.get(&draw.texture.0) else {
            return;
        };
        let Some(material) = self.materials.get(&bind.material.0) else {
            blit(texture, draw);
            return;
        };
        for (name, value) in &bind.uniforms {
            set_uniform(material, name, *value);
        }
        gl_use_material(material);
        blit(texture, draw);
        gl_use_default_material();
    }

    fn draw_filled_rect(
        &mut self,
        dst: crate::rect::Rect,
        color: Color,
        _layer: crate::layer::Layer,
    ) {
        let Some(target) = self.internal.clone() else {
            return;
        };
        let (w, h) = self.internal_size;
        set_camera(&Self::internal_camera(w, h, target));
        draw_rectangle(dst.x, dst.y, dst.width, dst.height, to_mq(color));
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

/// Draws `draw` from `texture` with the current material.
fn blit(texture: &Texture2D, draw: &SpriteDraw) {
    draw_texture_ex(
        texture,
        draw.dst.x,
        draw.dst.y,
        ::macroquad::prelude::Color::new(
            f32::from(draw.tint.r) / 255.0,
            f32::from(draw.tint.g) / 255.0,
            f32::from(draw.tint.b) / 255.0,
            f32::from(draw.tint.a) / 255.0,
        ),
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
}
