//! The built-in shader library (`pkrs/<name>`, §5.6): animated tiles, tint, translucency.

use super::{UniformType, UniformValue};

/// Engine vertex stage shared by every material.
pub(crate) const VERTEX: &str = include_str!("../../shaders/vertex.glsl");

/// A shader shipped with the engine, named `pkrs/<name>` in a material descriptor.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BuiltinShader {
    /// `pkrs/animated_tiles`: steps through `frames` frames, `ticks_per_frame` ticks each,
    /// laid out `frame_offset` pixels apart (default: `atlas_region` split into `frames`
    /// columns); optional `tint`.
    AnimatedTiles,
    /// `pkrs/tint`: multiplies by `tint` (default white), blended in by `strength` (default 1).
    Tint,
    /// `pkrs/translucency`: scales alpha by `alpha` (0–1, default 0.5).
    Translucency,
}

/// One parameter a built-in shader accepts in `uniforms`.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Param {
    pub name: &'static str,
    pub ty: UniformType,
    /// Used when the descriptor leaves it out; `None` = required (or derived).
    pub default: Option<UniformValue>,
    /// `false` for parameters the engine reads on the CPU (they must be literal numbers).
    pub global_ok: bool,
}

const WHITE: UniformValue = UniformValue::Vec4([1.0, 1.0, 1.0, 1.0]);

const ANIMATED_PARAMS: &[Param] = &[
    Param {
        name: "frames",
        ty: UniformType::Int,
        default: None,
        global_ok: false,
    },
    Param {
        name: "ticks_per_frame",
        ty: UniformType::Int,
        default: None,
        global_ok: false,
    },
    Param {
        name: "frame_offset",
        ty: UniformType::Vec2,
        default: None,
        global_ok: true,
    },
    Param {
        name: "tint",
        ty: UniformType::Vec4,
        default: Some(WHITE),
        global_ok: true,
    },
];

const TINT_PARAMS: &[Param] = &[
    Param {
        name: "tint",
        ty: UniformType::Vec4,
        default: Some(WHITE),
        global_ok: true,
    },
    Param {
        name: "strength",
        ty: UniformType::Float,
        default: Some(UniformValue::Float(1.0)),
        global_ok: true,
    },
];

const TRANSLUCENCY_PARAMS: &[Param] = &[Param {
    name: "alpha",
    ty: UniformType::Float,
    default: Some(UniformValue::Float(0.5)),
    global_ok: true,
}];

impl BuiltinShader {
    /// Every built-in shader.
    pub const ALL: [Self; 3] = [Self::AnimatedTiles, Self::Tint, Self::Translucency];

    /// The id written in a descriptor (`pkrs/tint`).
    #[must_use]
    pub const fn id(self) -> &'static str {
        match self {
            Self::AnimatedTiles => "pkrs/animated_tiles",
            Self::Tint => "pkrs/tint",
            Self::Translucency => "pkrs/translucency",
        }
    }

    /// Looks up a built-in by id.
    #[must_use]
    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|s| s.id() == id)
    }

    /// GLSL ES 1.00 fragment source.
    pub(crate) const fn fragment(self) -> &'static str {
        match self {
            Self::AnimatedTiles => include_str!("../../shaders/animated_tiles.glsl"),
            Self::Tint => include_str!("../../shaders/tint.glsl"),
            Self::Translucency => include_str!("../../shaders/translucency.glsl"),
        }
    }

    /// Parameters accepted in the descriptor's `uniforms`.
    pub(crate) const fn params(self) -> &'static [Param] {
        match self {
            Self::AnimatedTiles => ANIMATED_PARAMS,
            Self::Tint => TINT_PARAMS,
            Self::Translucency => TRANSLUCENCY_PARAMS,
        }
    }

    /// Uniforms the fragment shader declares (besides `Texture`), in binding order.
    pub(crate) const fn gpu_uniforms(self) -> &'static [(&'static str, UniformType)] {
        match self {
            Self::AnimatedTiles => &[
                ("frame", UniformType::Float),
                ("frame_offset", UniformType::Vec2),
                ("texture_size", UniformType::Vec2),
                ("tint", UniformType::Vec4),
            ],
            Self::Tint => &[
                ("tint", UniformType::Vec4),
                ("strength", UniformType::Float),
            ],
            Self::Translucency => &[("alpha", UniformType::Float)],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::material::glsl;

    #[test]
    fn ids_round_trip() {
        for shader in BuiltinShader::ALL {
            assert_eq!(BuiltinShader::from_id(shader.id()), Some(shader));
        }
        assert_eq!(BuiltinShader::from_id("pkrs/bloom"), None);
    }

    #[test]
    fn declared_uniforms_match_the_glsl() {
        for shader in BuiltinShader::ALL {
            let facts = glsl::scan(shader.fragment());
            assert_eq!(facts.version.as_deref(), Some("100"), "{}", shader.id());
            let declared: Vec<_> = facts
                .uniforms
                .iter()
                .filter(|u| u.ty != "sampler2D")
                .map(|u| (u.name.as_str(), UniformType::from_glsl(&u.ty)))
                .collect();
            let expected: Vec<_> = shader
                .gpu_uniforms()
                .iter()
                .map(|(n, t)| (*n, Some(*t)))
                .collect();
            assert_eq!(declared, expected, "{}", shader.id());
        }
    }
}
