//! Data-driven materials for tiles and sprites (§5.6).
//!
//! A material descriptor (`assets/materials/<id>.ron`) names a shader — a built-in one
//! (`pkrs/animated_tiles`, `pkrs/tint`, `pkrs/translucency`) or a project GLSL fragment shader
//! under `assets/` — and sets its `uniforms`, optionally restricted to an `atlas_region` of
//! the sheet:
//!
//! ```ron
//! (
//!     shader: "pkrs/animated_tiles",
//!     uniforms: {
//!         "frames": Int(4),
//!         "ticks_per_frame": Int(15),
//!         "tint": Global(name: "day_tint", default: Color(255, 255, 255, 255)),
//!     },
//!     atlas_region: (x: 0, y: 16, width: 64, height: 16),
//! )
//! ```
//!
//! Uniform values are `Float(f)`, `Int(i)`, `Vec2(x, y)`, `Vec3(x, y, z)`, `Vec4(x, y, z, w)`,
//! `Color(r, g, b, a)` (0–255, sent as a `vec4` in 0–1) or `Global(name: "…", default: …)`,
//! which reads a global uniform set with [`Renderer::set_global_uniform`](crate::Renderer)
//! (`render.set_global_uniform` in Lua). The engine also provides `time_ticks` (`float`),
//! `map_origin` (`vec2`, the map's top-left on the internal target) and `texture_size`
//! (`vec2`, pixels) to every shader that declares them.
//!
//! Project shaders must declare a [`CapabilityProfile`] (`profile: (glsl: "100",
//! max_uniforms: 4, texture_slots: 1, uniform_types: [Float, Vec4])`); [`check_material`]
//! (run by `pkrs check`) rejects profiles the engine cannot honour and shaders that exceed
//! their own profile. No GPU code reaches Lua: scripts only set uniform values.

mod builtin;
mod check;
mod descriptor;
mod glsl;

use std::collections::BTreeMap;

use serde::Deserialize;

pub use builtin::BuiltinShader;
pub use check::check_material;

use crate::color::Color;
use crate::error::{RenderError, Result};
use crate::rect::Rect;

/// Folder of material descriptors (`assets/materials/<id>.ron`).
pub const MATERIALS_DIR: &str = "assets/materials";

/// Folder project shader paths are relative to (`shaders/water.glsl` → `assets/shaders/water.glsl`).
pub const SHADER_ROOT: &str = "assets";

/// Prefix of built-in shader ids.
pub const BUILTIN_PREFIX: &str = "pkrs/";

/// Uniforms the engine fills in for every shader that declares them.
pub const ENGINE_UNIFORMS: &[(&str, UniformType)] = &[
    ("time_ticks", UniformType::Float),
    ("map_origin", UniformType::Vec2),
    ("texture_size", UniformType::Vec2),
];

/// Uniforms the backend itself provides (never counted, never set by descriptors).
pub(crate) const BACKEND_UNIFORMS: &[&str] = &["Model", "Projection", "_Time"];

/// The descriptor path of material `id` (`water` → `assets/materials/water.ron`).
#[must_use]
pub fn material_path(id: &str) -> String {
    format!("{MATERIALS_DIR}/{id}.ron")
}

/// `true` when `name` is filled in by the engine ([`ENGINE_UNIFORMS`]).
#[must_use]
pub fn is_engine_uniform(name: &str) -> bool {
    ENGINE_UNIFORMS.iter().any(|(n, _)| *n == name)
}

/// GLSL type of a uniform.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Deserialize)]
pub enum UniformType {
    /// `float`.
    Float,
    /// `int`.
    Int,
    /// `vec2`.
    Vec2,
    /// `vec3`.
    Vec3,
    /// `vec4`.
    Vec4,
    /// `mat4`: recognised, but no v1 backend binds it (profiles that ask for it are rejected).
    Mat4,
}

impl UniformType {
    /// The types the v1 engine can bind.
    pub const SUPPORTED: [Self; 5] = [Self::Float, Self::Int, Self::Vec2, Self::Vec3, Self::Vec4];

    /// GLSL spelling (`vec4`).
    #[must_use]
    pub const fn glsl_name(self) -> &'static str {
        match self {
            Self::Float => "float",
            Self::Int => "int",
            Self::Vec2 => "vec2",
            Self::Vec3 => "vec3",
            Self::Vec4 => "vec4",
            Self::Mat4 => "mat4",
        }
    }

    /// Parses a GLSL type name; `None` for anything else (`bool`, `ivec2`, `mat3`, …).
    #[must_use]
    pub fn from_glsl(name: &str) -> Option<Self> {
        [
            Self::Float,
            Self::Int,
            Self::Vec2,
            Self::Vec3,
            Self::Vec4,
            Self::Mat4,
        ]
        .into_iter()
        .find(|t| t.glsl_name() == name)
    }
}

/// A uniform value in engine vocabulary.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum UniformValue {
    /// `float`.
    Float(f32),
    /// `int`.
    Int(i32),
    /// `vec2`.
    Vec2([f32; 2]),
    /// `vec3`.
    Vec3([f32; 3]),
    /// `vec4` (colours are `vec4` in 0–1).
    Vec4([f32; 4]),
}

impl UniformValue {
    /// The GLSL type of this value.
    #[must_use]
    pub const fn ty(&self) -> UniformType {
        match self {
            Self::Float(_) => UniformType::Float,
            Self::Int(_) => UniformType::Int,
            Self::Vec2(_) => UniformType::Vec2,
            Self::Vec3(_) => UniformType::Vec3,
            Self::Vec4(_) => UniformType::Vec4,
        }
    }

    /// A colour as a `vec4` in 0–1.
    #[must_use]
    pub fn from_color(color: Color) -> Self {
        let c = |v: u8| f32::from(v) / 255.0;
        Self::Vec4([c(color.r), c(color.g), c(color.b), c(color.a)])
    }

    /// The zero value of `ty` (`None` for types the engine cannot bind).
    #[must_use]
    pub const fn zero(ty: UniformType) -> Option<Self> {
        Some(match ty {
            UniformType::Float => Self::Float(0.0),
            UniformType::Int => Self::Int(0),
            UniformType::Vec2 => Self::Vec2([0.0; 2]),
            UniformType::Vec3 => Self::Vec3([0.0; 3]),
            UniformType::Vec4 => Self::Vec4([0.0; 4]),
            UniformType::Mat4 => return None,
        })
    }

    /// This value as `ty`: the same type, or an `int` widened to a `float`.
    #[must_use]
    pub fn coerce(self, ty: UniformType) -> Option<Self> {
        match (self, ty) {
            (v, t) if v.ty() == t => Some(v),
            (Self::Int(i), UniformType::Float) => Some(Self::Float(i as f32)),
            _ => None,
        }
    }
}

/// Where a material uniform gets its value.
#[derive(Clone, Debug, PartialEq)]
pub enum UniformSource {
    /// A constant from the descriptor.
    Value(UniformValue),
    /// The global uniform `name` (set by Rust hooks or `render.set_global_uniform`), or
    /// `default` while it is unset or has the wrong type.
    Global {
        /// Global uniform name.
        name: String,
        /// Fallback value.
        default: Option<UniformValue>,
    },
}

/// The shader a material runs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ShaderRef {
    /// One of the engine's shaders.
    Builtin(BuiltinShader),
    /// A project fragment shader, relative to `assets/` (`shaders/water.glsl`).
    Project(String),
}

/// What a project shader needs from the GPU, declared in its material descriptor so
/// `pkrs check` can reject combinations the backend cannot run (§5.6).
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CapabilityProfile {
    /// GLSL version of the fragment shader (`"100"`: GLSL ES 1.00).
    pub glsl: String,
    /// Most uniforms the shader declares (engine uniforms included, `Texture` excluded).
    pub max_uniforms: u32,
    /// Most `sampler2D` uniforms (the sprite's `Texture` counts as one).
    pub texture_slots: u32,
    /// Uniform types the shader may declare.
    #[serde(default = "supported_types")]
    pub uniform_types: Vec<UniformType>,
}

fn supported_types() -> Vec<UniformType> {
    UniformType::SUPPORTED.to_vec()
}

impl CapabilityProfile {
    /// Most uniforms any v1 material may declare.
    pub const ENGINE_MAX_UNIFORMS: u32 = 16;
    /// Texture slots of the v1 backend: the sprite texture only.
    pub const ENGINE_TEXTURE_SLOTS: u32 = 1;
    /// GLSL version of the v1 backend (macroquad's OpenGL ES 2 / WebGL 1 pipeline).
    pub const ENGINE_GLSL: &'static str = "100";

    /// What the v1 engine supports.
    #[must_use]
    pub fn engine() -> Self {
        Self {
            glsl: Self::ENGINE_GLSL.to_owned(),
            max_uniforms: Self::ENGINE_MAX_UNIFORMS,
            texture_slots: Self::ENGINE_TEXTURE_SLOTS,
            uniform_types: supported_types(),
        }
    }

    /// Why the engine cannot honour this profile (empty when it can): `(field, problem)`.
    #[must_use]
    pub fn unsupported(&self) -> Vec<(&'static str, String)> {
        let mut out = Vec::new();
        if self.glsl != Self::ENGINE_GLSL {
            out.push((
                "glsl",
                format!(
                    "GLSL version `{}` is not supported (the engine runs GLSL ES 1.00, `\"{}\"`)",
                    self.glsl,
                    Self::ENGINE_GLSL
                ),
            ));
        }
        if self.max_uniforms > Self::ENGINE_MAX_UNIFORMS {
            out.push((
                "max_uniforms",
                format!(
                    "max_uniforms {} exceeds the engine limit of {}",
                    self.max_uniforms,
                    Self::ENGINE_MAX_UNIFORMS
                ),
            ));
        }
        if self.texture_slots > Self::ENGINE_TEXTURE_SLOTS {
            out.push((
                "texture_slots",
                format!(
                    "texture_slots {} exceeds the engine limit of {} (the sprite texture)",
                    self.texture_slots,
                    Self::ENGINE_TEXTURE_SLOTS
                ),
            ));
        }
        for ty in &self.uniform_types {
            if !UniformType::SUPPORTED.contains(ty) {
                out.push((
                    "uniform_types",
                    format!("uniform type `{}` is not supported", ty.glsl_name()),
                ));
            }
        }
        out
    }
}

/// Per-frame values the engine provides to shaders ([`ENGINE_UNIFORMS`]).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct EngineUniforms {
    /// Simulation ticks since the game started (60 Hz, §3.1).
    pub time_ticks: u64,
    /// The map's top-left corner on the internal target, in pixels.
    pub map_origin: (f32, f32),
    /// Size of the drawn texture in pixels.
    pub texture_size: (u32, u32),
}

/// Global uniforms set by Rust hooks and plugins (`render.set_global_uniform`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct GlobalUniforms(BTreeMap<String, UniformValue>);

impl GlobalUniforms {
    /// Sets `name` (replacing any previous value).
    pub fn set(&mut self, name: &str, value: UniformValue) {
        self.0.insert(name.to_owned(), value);
    }

    /// The value of `name`, if set.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<UniformValue> {
        self.0.get(name).copied()
    }

    /// Removes `name`; returns whether it was set.
    pub fn remove(&mut self, name: &str) -> bool {
        self.0.remove(name).is_some()
    }
}

/// Handle of a material created through the [`RenderBackend`](crate::RenderBackend).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct MaterialHandle(pub u32);

/// A material bound for one draw, with its resolved uniform values.
#[derive(Clone, Debug, PartialEq)]
pub struct MaterialBind {
    /// The material.
    pub material: MaterialHandle,
    /// Every uniform the shader declares, in declaration order.
    pub uniforms: Vec<(String, UniformValue)>,
}

impl MaterialBind {
    /// The value bound to `name`.
    #[must_use]
    pub fn uniform(&self, name: &str) -> Option<UniformValue> {
        self.uniforms
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| *v)
    }
}

/// What a backend compiles in [`RenderBackend::create_material`](crate::RenderBackend::create_material).
#[derive(Clone, Copy, Debug)]
pub struct ShaderProgram<'a> {
    /// Material id (for logs).
    pub name: &'a str,
    /// GLSL ES 1.00 vertex source (the engine's).
    pub vertex: &'a str,
    /// GLSL ES 1.00 fragment source.
    pub fragment: &'a str,
    /// Uniforms to bind, in order.
    pub uniforms: &'a [(String, UniformType)],
}

/// A validated material descriptor (§5.6). Backends receive it as a [`ShaderProgram`]; the
/// [`Renderer`](crate::Renderer) resolves its uniforms for every draw.
#[derive(Clone, Debug, PartialEq)]
pub struct Material {
    shader: ShaderRef,
    fragment: String,
    uniforms: Vec<(String, UniformType)>,
    params: BTreeMap<String, UniformSource>,
    atlas_region: Option<Rect>,
    profile: CapabilityProfile,
    animation: Option<(u32, u32)>,
}

impl Material {
    /// Parses and validates a descriptor. `read_shader` reads a project file by its
    /// project-relative path (`assets/shaders/water.glsl`). Every problem [`check_material`]
    /// reports as an error is an error here too.
    pub fn from_ron(source: &str, read_shader: &dyn Fn(&str) -> Option<String>) -> Result<Self> {
        let analysis = descriptor::analyze(source, read_shader);
        match analysis.material {
            Some(material) => Ok(material),
            None => Err(RenderError::MaterialInvalid {
                detail: analysis
                    .issues
                    .iter()
                    .filter(|i| i.is_error())
                    .map(|i| i.message.as_str())
                    .collect::<Vec<_>>()
                    .join("; "),
            }),
        }
    }

    /// The shader.
    #[must_use]
    pub fn shader(&self) -> &ShaderRef {
        &self.shader
    }

    /// The capability profile (the engine's for built-in shaders).
    #[must_use]
    pub fn profile(&self) -> &CapabilityProfile {
        &self.profile
    }

    /// The sheet region the material applies to (`None` = everything drawn with it).
    #[must_use]
    pub fn atlas_region(&self) -> Option<Rect> {
        self.atlas_region
    }

    /// Uniforms the fragment shader declares, in binding order.
    #[must_use]
    pub fn uniform_decls(&self) -> &[(String, UniformType)] {
        &self.uniforms
    }

    /// Where each descriptor uniform gets its value.
    #[must_use]
    pub fn params(&self) -> &BTreeMap<String, UniformSource> {
        &self.params
    }

    /// The program a backend compiles for this material.
    #[must_use]
    pub fn program<'a>(&'a self, name: &'a str) -> ShaderProgram<'a> {
        ShaderProgram {
            name,
            vertex: builtin::VERTEX,
            fragment: &self.fragment,
            uniforms: &self.uniforms,
        }
    }

    /// `true` when a draw from `src` (texels) uses this material: always without an
    /// `atlas_region`, else when `src` lies inside it.
    #[must_use]
    pub fn applies_to(&self, src: Rect) -> bool {
        self.atlas_region.is_none_or(|r| {
            src.x >= r.x
                && src.y >= r.y
                && src.x + src.width <= r.x + r.width
                && src.y + src.height <= r.y + r.height
        })
    }

    /// The animation frame shown at `time_ticks` (`pkrs/animated_tiles`; `0` otherwise).
    #[must_use]
    pub fn frame_at(&self, time_ticks: u64) -> u32 {
        match self.animation {
            Some((frames, ticks_per_frame)) if frames > 0 && ticks_per_frame > 0 => {
                let frame = (time_ticks / u64::from(ticks_per_frame)) % u64::from(frames);
                u32::try_from(frame).unwrap_or(0)
            }
            _ => 0,
        }
    }

    /// Resolves every declared uniform: engine uniforms from `engine`, `Global` sources from
    /// `globals` (falling back to their default, then the shader default, then zero) and
    /// constants as written.
    #[must_use]
    pub fn resolve_uniforms(
        &self,
        engine: &EngineUniforms,
        globals: &GlobalUniforms,
    ) -> Vec<(String, UniformValue)> {
        self.uniforms
            .iter()
            .filter_map(|(name, ty)| {
                let value = self.resolve_one(name, *ty, engine, globals)?;
                Some((name.clone(), value))
            })
            .collect()
    }

    fn resolve_one(
        &self,
        name: &str,
        ty: UniformType,
        engine: &EngineUniforms,
        globals: &GlobalUniforms,
    ) -> Option<UniformValue> {
        let engine_value = match name {
            "time_ticks" => Some(UniformValue::Float(engine.time_ticks as f32)),
            "map_origin" => Some(UniformValue::Vec2([
                engine.map_origin.0,
                engine.map_origin.1,
            ])),
            "texture_size" => Some(UniformValue::Vec2([
                engine.texture_size.0 as f32,
                engine.texture_size.1 as f32,
            ])),
            "frame" if self.shader == ShaderRef::Builtin(BuiltinShader::AnimatedTiles) => {
                Some(UniformValue::Float(self.frame_at(engine.time_ticks) as f32))
            }
            _ => None,
        };
        if let Some(value) = engine_value.and_then(|v| v.coerce(ty)) {
            return Some(value);
        }
        let value = match self.params.get(name) {
            Some(UniformSource::Value(v)) => v.coerce(ty),
            Some(UniformSource::Global { name, default }) => globals
                .get(name)
                .and_then(|v| v.coerce(ty))
                .or_else(|| default.and_then(|d| d.coerce(ty))),
            None => None,
        };
        value.or_else(|| UniformValue::zero(ty))
    }
}

#[cfg(test)]
mod tests;
