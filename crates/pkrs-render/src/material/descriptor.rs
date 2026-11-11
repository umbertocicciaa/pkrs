//! Material descriptor RON schema and validation (§5.6). [`analyze`] collects every problem
//! (nothing is fail-fast) so `pkrs check` can report them all; the runtime only accepts a
//! descriptor without errors.

use core::ops::Range;
use std::collections::BTreeMap;

use pkrs_core::Severity;
use serde::Deserialize;

use super::builtin::{BuiltinShader, Param};
use super::glsl;
use super::{
    BACKEND_UNIFORMS, BUILTIN_PREFIX, CapabilityProfile, ENGINE_UNIFORMS, Material, SHADER_ROOT,
    ShaderRef, UniformSource, UniformType, UniformValue, is_engine_uniform,
};
use crate::color::Color;
use crate::rect::Rect;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawMaterial {
    shader: String,
    #[serde(default)]
    uniforms: BTreeMap<String, RawUniform>,
    #[serde(default)]
    atlas_region: Option<RawRegion>,
    #[serde(default)]
    profile: Option<CapabilityProfile>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawRegion {
    x: u32,
    y: u32,
    width: u32,
    height: u32,
}

#[derive(Clone, Debug, Deserialize)]
enum RawUniform {
    Float(f32),
    Int(i32),
    Vec2(f32, f32),
    Vec3(f32, f32, f32),
    Vec4(f32, f32, f32, f32),
    Color(u8, u8, u8, u8),
    Global {
        name: String,
        #[serde(default)]
        default: Option<Box<RawUniform>>,
    },
}

impl RawUniform {
    fn value(&self) -> Option<UniformValue> {
        Some(match *self {
            Self::Float(f) => UniformValue::Float(f),
            Self::Int(i) => UniformValue::Int(i),
            Self::Vec2(x, y) => UniformValue::Vec2([x, y]),
            Self::Vec3(x, y, z) => UniformValue::Vec3([x, y, z]),
            Self::Vec4(x, y, z, w) => UniformValue::Vec4([x, y, z, w]),
            Self::Color(r, g, b, a) => UniformValue::from_color(Color { r, g, b, a }),
            Self::Global { .. } => return None,
        })
    }
}

/// Where an [`Issue`] points.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum At {
    /// Nowhere in particular.
    Nothing,
    /// RON's `line:column` (parse errors).
    Position(usize, usize),
    /// The first occurrence of this text in the descriptor.
    Material(String),
    /// A byte range of the project shader.
    Shader(Range<usize>),
}

/// One problem with a descriptor.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Issue {
    pub severity: Severity,
    pub code: &'static str,
    pub message: String,
    pub at: At,
    pub label: Option<String>,
    pub help: Option<String>,
}

impl Issue {
    fn error(code: &'static str, message: impl Into<String>, at: At) -> Self {
        Self {
            severity: Severity::Error,
            code,
            message: message.into(),
            at,
            label: None,
            help: None,
        }
    }

    fn warning(code: &'static str, message: impl Into<String>, at: At) -> Self {
        Self {
            severity: Severity::Warning,
            ..Self::error(code, message, at)
        }
    }

    fn label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }

    fn help(mut self, help: impl Into<String>) -> Self {
        self.help = Some(help.into());
        self
    }

    pub(crate) fn is_error(&self) -> bool {
        self.severity == Severity::Error
    }
}

/// Everything [`analyze`] found.
#[derive(Debug, Default)]
pub(crate) struct Analysis {
    /// The material, when there is no error.
    pub material: Option<Material>,
    pub issues: Vec<Issue>,
    /// Project-relative path and source of the project shader, when it was read.
    pub shader_file: Option<(String, String)>,
}

fn key(name: &str) -> At {
    At::Material(format!("\"{name}\""))
}

/// Parses and validates `source`; `read_shader` reads a project-relative file.
pub(crate) fn analyze(source: &str, read_shader: &dyn Fn(&str) -> Option<String>) -> Analysis {
    let mut out = Analysis::default();
    let options =
        ron::Options::default().with_default_extension(ron::extensions::Extensions::IMPLICIT_SOME);
    let raw: RawMaterial = match options.from_str(source) {
        Ok(raw) => raw,
        Err(err) => {
            out.issues.push(
                Issue::error(
                    "material::parse",
                    format!("material descriptor parse error: {}", err.code),
                    At::Position(err.position.line, err.position.col),
                )
                .label(err.code.to_string()),
            );
            return out;
        }
    };

    let atlas_region = raw.atlas_region.as_ref().and_then(|r| {
        if r.width == 0 || r.height == 0 {
            out.issues.push(
                Issue::error(
                    "material::invalid-value",
                    "atlas_region must have a non-zero width and height",
                    At::Material("atlas_region".into()),
                )
                .label("empty region"),
            );
            return None;
        }
        Some(Rect::new(
            r.x as f32,
            r.y as f32,
            r.width as f32,
            r.height as f32,
        ))
    });

    let mut params = BTreeMap::new();
    for (name, raw_value) in &raw.uniforms {
        if is_engine_uniform(name) {
            out.issues.push(
                Issue::error(
                    "material::reserved-uniform",
                    format!("uniform `{name}` is provided by the engine and cannot be set"),
                    key(name),
                )
                .label("engine uniform")
                .help("remove it; the engine fills in time_ticks, map_origin and texture_size"),
            );
            continue;
        }
        if let Some(source) = convert(name, raw_value, &mut out.issues) {
            params.insert(name.clone(), source);
        }
    }

    let shader = if let Some(id) = raw.shader.strip_prefix(BUILTIN_PREFIX) {
        match BuiltinShader::from_id(&raw.shader) {
            Some(builtin) => Some(ShaderRef::Builtin(builtin)),
            None => {
                out.issues.push(
                    Issue::error(
                        "material::unknown-shader",
                        format!("`{}` is not a built-in shader", raw.shader),
                        At::Material(format!("\"{}\"", raw.shader)),
                    )
                    .label(format!("no built-in shader `{id}`"))
                    .help(format!(
                        "built-in shaders: {}",
                        BuiltinShader::ALL.map(BuiltinShader::id).join(", ")
                    )),
                );
                None
            }
        }
    } else if raw.shader.ends_with(".glsl") && valid_relative_path(&raw.shader) {
        Some(ShaderRef::Project(raw.shader.clone()))
    } else {
        out.issues.push(
            Issue::error(
                "material::unknown-shader",
                format!(
                    "shader `{}` must be a built-in (`pkrs/<name>`) or a `.glsl` file under `{SHADER_ROOT}/`",
                    raw.shader
                ),
                At::Material(format!("\"{}\"", raw.shader)),
            )
            .label("not a shader reference"),
        );
        None
    };

    let built = match shader {
        Some(ShaderRef::Builtin(builtin)) => {
            check_builtin(builtin, &raw, &mut params, atlas_region, &mut out.issues)
        }
        Some(ShaderRef::Project(ref path)) => {
            check_project(path, &raw, &params, read_shader, &mut out)
        }
        None => None,
    };

    if out.issues.iter().any(Issue::is_error) {
        return out;
    }
    if let (Some(shader), Some(built)) = (shader, built) {
        out.material = Some(Material {
            shader,
            fragment: built.fragment,
            uniforms: built.uniforms,
            params,
            atlas_region,
            profile: built.profile,
            animation: built.animation,
        });
    }
    out
}

/// What the shader-specific checks produce for the [`Material`].
struct Built {
    fragment: String,
    uniforms: Vec<(String, UniformType)>,
    profile: CapabilityProfile,
    animation: Option<(u32, u32)>,
}

fn valid_relative_path(path: &str) -> bool {
    !path.is_empty()
        && !path.starts_with('/')
        && !path.contains('\\')
        && !path.contains(':')
        && path
            .split('/')
            .all(|seg| !seg.is_empty() && seg != "." && seg != "..")
}

fn convert(name: &str, raw: &RawUniform, issues: &mut Vec<Issue>) -> Option<UniformSource> {
    match raw {
        RawUniform::Global {
            name: global,
            default,
        } => {
            if global.trim().is_empty() {
                issues.push(
                    Issue::error(
                        "material::invalid-value",
                        format!("uniform `{name}`: the global name must not be empty"),
                        key(name),
                    )
                    .label("empty global name"),
                );
                return None;
            }
            let default = match default.as_deref() {
                None => None,
                Some(inner) => match inner.value() {
                    Some(v) => Some(v),
                    None => {
                        issues.push(
                            Issue::error(
                                "material::invalid-value",
                                format!(
                                    "uniform `{name}`: the default of a global must be a value, not another global"
                                ),
                                key(name),
                            )
                            .label("nested global"),
                        );
                        return None;
                    }
                },
            };
            Some(UniformSource::Global {
                name: global.clone(),
                default,
            })
        }
        other => other.value().map(UniformSource::Value),
    }
}

/// The type a source provides, for type checks (`None` for a global without default).
fn source_type(source: &UniformSource) -> Option<UniformType> {
    match source {
        UniformSource::Value(v) => Some(v.ty()),
        UniformSource::Global { default, .. } => default.map(|d| d.ty()),
    }
}

fn type_matches(source: &UniformSource, ty: UniformType) -> bool {
    match source {
        UniformSource::Value(v) => v.coerce(ty).is_some(),
        UniformSource::Global { default, .. } => default.is_none_or(|d| d.coerce(ty).is_some()),
    }
}

fn type_issue(name: &str, expected: UniformType, source: &UniformSource) -> Issue {
    let found = source_type(source).map_or("?", UniformType::glsl_name);
    Issue::error(
        "material::uniform-type",
        format!(
            "uniform `{name}` is a `{}`, but the descriptor gives a `{found}`",
            expected.glsl_name()
        ),
        key(name),
    )
    .label(format!("expected `{}`", expected.glsl_name()))
}

fn check_builtin(
    builtin: BuiltinShader,
    raw: &RawMaterial,
    params: &mut BTreeMap<String, UniformSource>,
    atlas_region: Option<Rect>,
    issues: &mut Vec<Issue>,
) -> Option<Built> {
    if raw.profile.is_some() {
        issues.push(
            Issue::warning(
                "material::builtin-profile",
                format!(
                    "`{}` is a built-in shader; its capability profile is the engine's",
                    builtin.id()
                ),
                At::Material("profile".into()),
            )
            .label("ignored")
            .help("profiles are only needed for project shaders"),
        );
    }
    let spec = builtin.params();
    for (name, source) in params.iter() {
        let Some(param) = spec.iter().find(|p| p.name == name) else {
            issues.push(
                Issue::error(
                    "material::unknown-uniform",
                    format!("`{}` has no uniform `{name}`", builtin.id()),
                    key(name),
                )
                .label("unknown uniform")
                .help(format!(
                    "`{}` accepts: {}",
                    builtin.id(),
                    spec.iter().map(|p| p.name).collect::<Vec<_>>().join(", ")
                )),
            );
            continue;
        };
        if !type_matches(source, param.ty) {
            issues.push(type_issue(name, param.ty, source));
            continue;
        }
        if !param.global_ok && matches!(source, UniformSource::Global { .. }) {
            issues.push(
                Issue::error(
                    "material::invalid-value",
                    format!("uniform `{name}` must be a constant, not a global"),
                    key(name),
                )
                .label("read by the engine, not the shader"),
            );
        }
    }

    let mut animation = None;
    if builtin == BuiltinShader::AnimatedTiles {
        let positive = |name: &str, issues: &mut Vec<Issue>| -> Option<u32> {
            match params.get(name) {
                Some(UniformSource::Value(UniformValue::Int(n))) if *n >= 1 => {
                    u32::try_from(*n).ok()
                }
                Some(UniformSource::Value(UniformValue::Int(_))) => {
                    issues.push(
                        Issue::error(
                            "material::invalid-value",
                            format!("uniform `{name}` must be at least 1"),
                            key(name),
                        )
                        .label("must be at least 1"),
                    );
                    None
                }
                Some(_) => None,
                None => {
                    issues.push(
                        Issue::error(
                            "material::missing-uniform",
                            format!("`{}` needs uniform `{name}`", builtin.id()),
                            At::Material("uniforms".into()),
                        )
                        .label(format!("add \"{name}\": Int(…)")),
                    );
                    None
                }
            }
        };
        let frames = positive("frames", issues);
        let ticks = positive("ticks_per_frame", issues);
        if let (Some(frames), Some(ticks)) = (frames, ticks) {
            animation = Some((frames, ticks));
            if !params.contains_key("frame_offset") {
                match atlas_region {
                    Some(region) => {
                        params.insert(
                            "frame_offset".into(),
                            UniformSource::Value(UniformValue::Vec2([
                                region.width / frames as f32,
                                0.0,
                            ])),
                        );
                    }
                    None => issues.push(
                        Issue::error(
                            "material::missing-uniform",
                            format!(
                                "`{}` needs `frame_offset` or an `atlas_region` to split into frames",
                                builtin.id()
                            ),
                            At::Material("uniforms".into()),
                        )
                        .label("add \"frame_offset\": Vec2(…) or atlas_region"),
                    ),
                }
            }
        }
    }
    if builtin == BuiltinShader::Translucency
        && let Some(UniformSource::Value(UniformValue::Float(a))) = params.get("alpha")
        && !(0.0..=1.0).contains(a)
    {
        issues.push(
            Issue::error(
                "material::invalid-value",
                format!("uniform `alpha` must be between 0 and 1 (got {a})"),
                key("alpha"),
            )
            .label("out of range"),
        );
    }

    // Defaults: missing parameters, and globals without their own default.
    for param in spec {
        apply_default(params, param);
    }

    Some(Built {
        fragment: builtin.fragment().to_owned(),
        uniforms: builtin
            .gpu_uniforms()
            .iter()
            .map(|(n, t)| ((*n).to_owned(), *t))
            .collect(),
        profile: CapabilityProfile::engine(),
        animation,
    })
}

fn apply_default(params: &mut BTreeMap<String, UniformSource>, param: &Param) {
    let Some(default) = param.default else {
        return;
    };
    match params.get_mut(param.name) {
        None => {
            params.insert(param.name.to_owned(), UniformSource::Value(default));
        }
        Some(UniformSource::Global { default: d, .. }) if d.is_none() => *d = Some(default),
        Some(_) => {}
    }
}

fn check_project(
    path: &str,
    raw: &RawMaterial,
    params: &BTreeMap<String, UniformSource>,
    read_shader: &dyn Fn(&str) -> Option<String>,
    out: &mut Analysis,
) -> Option<Built> {
    let issues = &mut out.issues;
    let profile = match &raw.profile {
        Some(profile) => {
            for (field, problem) in profile.unsupported() {
                issues.push(
                    Issue::error(
                        "material::unsupported-profile",
                        format!("capability profile: {problem}"),
                        At::Material(format!("{field}:")),
                    )
                    .label("not supported by the engine")
                    .help(format!(
                        "the v1 engine supports glsl \"{}\", up to {} uniforms, {} texture slot and the types float, int, vec2, vec3, vec4",
                        CapabilityProfile::ENGINE_GLSL,
                        CapabilityProfile::ENGINE_MAX_UNIFORMS,
                        CapabilityProfile::ENGINE_TEXTURE_SLOTS
                    )),
                );
            }
            profile.clone()
        }
        None => {
            issues.push(
                Issue::error(
                    "material::missing-profile",
                    format!("project shader `{path}` needs a capability profile"),
                    At::Material("shader".into()),
                )
                .label("no `profile`")
                .help("add `profile: (glsl: \"100\", max_uniforms: 4, texture_slots: 1)`"),
            );
            CapabilityProfile::engine()
        }
    };

    let file = format!("{SHADER_ROOT}/{path}");
    let Some(glsl_source) = read_shader(&file) else {
        issues.push(
            Issue::error(
                "material::missing-shader",
                format!("shader `{file}` does not exist"),
                At::Material(format!("\"{path}\"")),
            )
            .label("file not found"),
        );
        return None;
    };
    let facts = glsl::scan(&glsl_source);
    if facts.version.as_deref() != Some(profile.glsl.as_str()) {
        let found = facts
            .version
            .as_deref()
            .map_or("no `#version` line".to_owned(), |v| {
                format!("`#version {v}`")
            });
        issues.push(
            Issue::error(
                "material::glsl-version",
                format!(
                    "shader `{file}` has {found}, but its profile declares glsl \"{}\"",
                    profile.glsl
                ),
                At::Material("glsl:".into()),
            )
            .label("version mismatch")
            .help(format!("start the shader with `#version {}`", profile.glsl)),
        );
    }

    let mut uniforms = Vec::new();
    let mut samplers = Vec::new();
    let mut rejected = Vec::new();
    for u in &facts.uniforms {
        if BACKEND_UNIFORMS.contains(&u.name.as_str()) {
            continue;
        }
        if u.ty == "sampler2D" {
            samplers.push(u.range.clone());
            continue;
        }
        let ty = UniformType::from_glsl(&u.ty);
        let allowed = ty.filter(|t| profile.uniform_types.contains(t) && !u.array);
        let Some(ty) = allowed else {
            let what = if u.array {
                format!("`{}` is an array; arrays are not supported", u.name)
            } else {
                format!(
                    "`{}` has type `{}`, which the capability profile does not allow",
                    u.name, u.ty
                )
            };
            issues.push(
                Issue::error(
                    "material::uniform-type-not-allowed",
                    format!("shader `{file}`: {what}"),
                    At::Shader(u.range.clone()),
                )
                .label("type not allowed")
                .help(format!(
                    "allowed types: {}",
                    profile
                        .uniform_types
                        .iter()
                        .map(|t| t.glsl_name())
                        .collect::<Vec<_>>()
                        .join(", ")
                )),
            );
            rejected.push(u.name.as_str());
            continue;
        };
        if let Some((_, expected)) = ENGINE_UNIFORMS.iter().find(|(n, _)| *n == u.name)
            && *expected != ty
        {
            issues.push(
                Issue::error(
                    "material::uniform-type",
                    format!(
                        "shader `{file}`: engine uniform `{}` is a `{}`",
                        u.name,
                        expected.glsl_name()
                    ),
                    At::Shader(u.range.clone()),
                )
                .label(format!("declare it as `{}`", expected.glsl_name())),
            );
            continue;
        }
        uniforms.push((u.name.clone(), ty, u.range.clone()));
    }

    let slots = u32::try_from(samplers.len()).unwrap_or(u32::MAX);
    if slots > profile.texture_slots {
        let at = samplers.last().cloned().map_or(At::Nothing, At::Shader);
        issues.push(
            Issue::error(
                "material::too-many-textures",
                format!(
                    "shader `{file}` declares {slots} textures, but its profile allows {}",
                    profile.texture_slots
                ),
                at,
            )
            .label("texture slot over the limit"),
        );
    }
    let count = u32::try_from(uniforms.len()).unwrap_or(u32::MAX);
    if count > profile.max_uniforms {
        let at = uniforms
            .last()
            .map_or(At::Nothing, |(_, _, r)| At::Shader(r.clone()));
        issues.push(
            Issue::error(
                "material::too-many-uniforms",
                format!(
                    "shader `{file}` declares {count} uniforms, but its profile allows {}",
                    profile.max_uniforms
                ),
                at,
            )
            .label("uniform over the limit"),
        );
    }

    for (name, source) in params {
        match uniforms.iter().find(|(n, _, _)| n == name) {
            // Already reported with its shader declaration.
            None if rejected.contains(&name.as_str()) => {}
            None => issues.push(
                Issue::error(
                    "material::unknown-uniform",
                    format!("shader `{file}` declares no uniform `{name}`"),
                    key(name),
                )
                .label("not declared in the shader"),
            ),
            Some((_, ty, _)) if !type_matches(source, *ty) => {
                issues.push(type_issue(name, *ty, source));
            }
            Some(_) => {}
        }
    }
    for (name, _, range) in &uniforms {
        if !is_engine_uniform(name) && !params.contains_key(name) {
            issues.push(
                Issue::warning(
                    "material::unset-uniform",
                    format!("shader `{file}`: uniform `{name}` is never set and stays zero"),
                    At::Shader(range.clone()),
                )
                .label("not set by the material")
                .help("give it a value in the material's `uniforms`"),
            );
        }
    }

    out.shader_file = Some((file, glsl_source.clone()));
    Some(Built {
        fragment: glsl_source,
        uniforms: uniforms.into_iter().map(|(n, t, _)| (n, t)).collect(),
        profile,
        animation: None,
    })
}
