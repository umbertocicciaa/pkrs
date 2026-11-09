//! Descriptor validation and uniform resolution (§5.6).

use super::*;

fn no_files(_: &str) -> Option<String> {
    None
}

fn codes(source: &str, read: &dyn Fn(&str) -> Option<String>) -> Vec<String> {
    check_material("assets/materials/m.ron", source, read)
        .iter()
        .map(|d| d.code.clone())
        .collect()
}

const WATER: &str = r#"(
    shader: "pkrs/animated_tiles",
    uniforms: {
        "frames": Int(4),
        "ticks_per_frame": Int(15),
        "tint": Global(name: "day_tint", default: Color(255, 255, 255, 255)),
    },
    atlas_region: (x: 0, y: 16, width: 64, height: 16),
)"#;

#[test]
fn builtin_descriptor_parses_with_defaults() {
    let m = Material::from_ron(WATER, &no_files).unwrap();
    assert_eq!(
        m.shader(),
        &ShaderRef::Builtin(BuiltinShader::AnimatedTiles)
    );
    assert_eq!(m.atlas_region(), Some(Rect::new(0.0, 16.0, 64.0, 16.0)));
    // frame_offset defaults to one quarter of the region.
    assert_eq!(
        m.params().get("frame_offset"),
        Some(&UniformSource::Value(UniformValue::Vec2([16.0, 0.0])))
    );
    assert_eq!(m.profile(), &CapabilityProfile::engine());
    assert!(check_material("m.ron", WATER, &no_files).is_empty());
}

#[test]
fn explicit_some_is_accepted_too() {
    let src = r#"(shader: "pkrs/tint", atlas_region: Some((x: 0, y: 0, width: 8, height: 8)))"#;
    assert!(Material::from_ron(src, &no_files).is_ok());
}

#[test]
fn applies_only_inside_the_atlas_region() {
    let m = Material::from_ron(WATER, &no_files).unwrap();
    assert!(m.applies_to(Rect::new(0.0, 16.0, 16.0, 16.0)));
    assert!(m.applies_to(Rect::new(48.0, 16.0, 16.0, 16.0)));
    assert!(!m.applies_to(Rect::new(0.0, 0.0, 16.0, 16.0)));
    assert!(!m.applies_to(Rect::new(56.0, 16.0, 16.0, 16.0)));
    let tint = Material::from_ron(r#"(shader: "pkrs/tint")"#, &no_files).unwrap();
    assert!(tint.applies_to(Rect::new(500.0, 0.0, 16.0, 16.0)));
}

#[test]
fn uniforms_resolve_from_engine_globals_and_defaults() {
    let m = Material::from_ron(WATER, &no_files).unwrap();
    let mut globals = GlobalUniforms::default();
    let engine = EngineUniforms {
        time_ticks: 31,
        map_origin: (-72.0, -48.0),
        texture_size: (64, 32),
    };
    let resolved = m.resolve_uniforms(&engine, &globals);
    assert_eq!(
        resolved,
        vec![
            ("frame".to_owned(), UniformValue::Float(2.0)),
            ("frame_offset".to_owned(), UniformValue::Vec2([16.0, 0.0])),
            ("texture_size".to_owned(), UniformValue::Vec2([64.0, 32.0])),
            ("tint".to_owned(), UniformValue::Vec4([1.0; 4])),
        ]
    );
    // The global wins once set; a wrong type falls back to the default.
    globals.set("day_tint", UniformValue::Vec4([0.5, 0.5, 0.8, 1.0]));
    let tint = |g: &GlobalUniforms| {
        m.resolve_uniforms(&engine, g)
            .into_iter()
            .find(|(n, _)| n == "tint")
            .map(|(_, v)| v)
    };
    assert_eq!(
        tint(&globals),
        Some(UniformValue::Vec4([0.5, 0.5, 0.8, 1.0]))
    );
    globals.set("day_tint", UniformValue::Float(1.0));
    assert_eq!(tint(&globals), Some(UniformValue::Vec4([1.0; 4])));
}

#[test]
fn animation_frames_cycle_over_ticks() {
    let m = Material::from_ron(WATER, &no_files).unwrap();
    let frames: Vec<u32> = [0, 14, 15, 30, 45, 59, 60].map(|t| m.frame_at(t)).to_vec();
    assert_eq!(frames, [0, 0, 1, 2, 3, 3, 0]);
}

#[test]
fn engine_uniforms_reach_project_shaders() {
    let glsl = "#version 100\nprecision mediump float;\nvarying mediump vec2 uv;\n\
                uniform sampler2D Texture;\nuniform float time_ticks;\nuniform vec2 map_origin;\n\
                uniform float speed;\nvoid main() { gl_FragColor = texture2D(Texture, uv); }\n";
    let read = |p: &str| (p == "assets/shaders/wave.glsl").then(|| glsl.to_owned());
    let src = r#"(
        shader: "shaders/wave.glsl",
        profile: (glsl: "100", max_uniforms: 3, texture_slots: 1, uniform_types: [Float, Vec2]),
        uniforms: { "speed": Int(2) },
    )"#;
    assert!(check_material("m.ron", src, &read).is_empty());
    let m = Material::from_ron(src, &read).unwrap();
    let engine = EngineUniforms {
        time_ticks: 120,
        map_origin: (8.0, -16.0),
        texture_size: (16, 16),
    };
    assert_eq!(
        m.resolve_uniforms(&engine, &GlobalUniforms::default()),
        vec![
            ("time_ticks".to_owned(), UniformValue::Float(120.0)),
            ("map_origin".to_owned(), UniformValue::Vec2([8.0, -16.0])),
            ("speed".to_owned(), UniformValue::Float(2.0)),
        ],
        "an int constant widens to a float uniform"
    );
    assert_eq!(m.program("wave").fragment, glsl);
}

#[test]
fn builtin_descriptor_errors() {
    assert_eq!(codes("(shader: )", &no_files), ["material::parse"]);
    assert_eq!(
        codes(r#"(shader: "pkrs/bloom")"#, &no_files),
        ["material::unknown-shader"]
    );
    assert_eq!(
        codes(r#"(shader: "water.png")"#, &no_files),
        ["material::unknown-shader"]
    );
    assert_eq!(
        codes(r#"(shader: "../outside.glsl")"#, &no_files),
        ["material::unknown-shader"]
    );
    assert_eq!(
        codes(
            r#"(shader: "pkrs/tint", uniforms: { "glow": Float(1.0), "tint": Float(1.0) })"#,
            &no_files
        ),
        ["material::unknown-uniform", "material::uniform-type"]
    );
    assert_eq!(
        codes(
            r#"(shader: "pkrs/tint", uniforms: { "time_ticks": Float(1.0) })"#,
            &no_files
        ),
        ["material::reserved-uniform"]
    );
    assert_eq!(
        codes(r#"(shader: "pkrs/animated_tiles")"#, &no_files),
        ["material::missing-uniform", "material::missing-uniform"]
    );
    assert_eq!(
        codes(
            r#"(shader: "pkrs/animated_tiles", uniforms: { "frames": Int(0), "ticks_per_frame": Global(name: "x") })"#,
            &no_files
        ),
        ["material::invalid-value", "material::invalid-value"]
    );
    assert_eq!(
        codes(
            r#"(shader: "pkrs/animated_tiles", uniforms: { "frames": Int(2), "ticks_per_frame": Int(8) })"#,
            &no_files
        ),
        ["material::missing-uniform"],
        "no frame_offset and no atlas_region"
    );
    assert_eq!(
        codes(
            r#"(shader: "pkrs/translucency", uniforms: { "alpha": Float(1.5) }, atlas_region: (x: 0, y: 0, width: 0, height: 4))"#,
            &no_files
        ),
        ["material::invalid-value", "material::invalid-value"]
    );
    assert_eq!(
        codes(
            r#"(shader: "pkrs/tint", uniforms: { "tint": Global(name: "a", default: Global(name: "b")) })"#,
            &no_files
        ),
        ["material::invalid-value"]
    );
    assert_eq!(
        codes(
            r#"(shader: "pkrs/tint", profile: (glsl: "100", max_uniforms: 2, texture_slots: 1))"#,
            &no_files
        ),
        ["material::builtin-profile"]
    );
    let err = Material::from_ron(r#"(shader: "pkrs/bloom")"#, &no_files).unwrap_err();
    assert!(err.to_string().contains("not a built-in shader"), "{err}");
}

#[test]
fn project_shader_profile_errors() {
    let glsl = "#version 330\nuniform sampler2D Texture;\nuniform sampler2D Noise;\n\
                uniform mat4 warp;\nuniform vec4 glow;\nuniform vec4 rim;\nuniform bool on;\n\
                uniform vec2 time_ticks;\nuniform float w[2];\nvoid main() {}\n";
    let read = |p: &str| (p == "assets/shaders/fx.glsl").then(|| glsl.to_owned());
    let src = r#"(
        shader: "shaders/fx.glsl",
        profile: (glsl: "300", max_uniforms: 32, texture_slots: 2, uniform_types: [Vec2, Vec4, Mat4]),
        uniforms: { "glow": Vec4(1.0, 1.0, 1.0, 1.0), "rim": Float(1.0), "missing": Float(0.0) },
    )"#;
    assert_eq!(
        codes(src, &read),
        [
            "material::unsupported-profile", // glsl 300
            "material::unsupported-profile", // max_uniforms 32
            "material::unsupported-profile", // texture_slots 2
            "material::unsupported-profile", // Mat4
            "material::glsl-version",
            "material::uniform-type-not-allowed", // bool
            "material::uniform-type",             // time_ticks is a float
            "material::uniform-type-not-allowed", // array
            "material::unknown-uniform",          // missing
            "material::uniform-type",             // rim
            "material::unset-uniform",            // warp
        ]
    );
    // Shader problems point into the shader file.
    let diags = check_material("assets/materials/fx.ron", src, &read);
    let bool_uniform = diags.iter().find(|d| d.message.contains("`on`")).unwrap();
    assert_eq!(bool_uniform.file.as_deref(), Some("assets/shaders/fx.glsl"));
    assert_eq!(bool_uniform.span.map(|s| s.line), Some(7));
}

#[test]
fn project_shader_limits_and_missing_pieces() {
    let glsl = "#version 100\nuniform sampler2D Texture;\nuniform sampler2D Noise;\n\
                uniform float a;\nuniform float b;\nuniform float c;\nvoid main() {}\n";
    let read = |p: &str| (p == "assets/shaders/fx.glsl").then(|| glsl.to_owned());
    let src = r#"(
        shader: "shaders/fx.glsl",
        profile: (glsl: "100", max_uniforms: 2, texture_slots: 1),
        uniforms: { "a": Float(1.0), "b": Float(1.0) },
    )"#;
    assert_eq!(
        codes(src, &read),
        [
            "material::too-many-textures",
            "material::too-many-uniforms",
            "material::unset-uniform"
        ]
    );
    assert_eq!(
        codes(r#"(shader: "shaders/fx.glsl")"#, &read),
        [
            "material::missing-profile",
            "material::too-many-textures",
            "material::unset-uniform",
            "material::unset-uniform",
            "material::unset-uniform"
        ]
    );
    assert_eq!(
        codes(
            r#"(shader: "shaders/none.glsl", profile: (glsl: "100", max_uniforms: 2, texture_slots: 1))"#,
            &read
        ),
        ["material::missing-shader"]
    );
}

#[test]
fn profile_engine_limits() {
    assert!(CapabilityProfile::engine().unsupported().is_empty());
    let p = CapabilityProfile {
        glsl: "100".into(),
        max_uniforms: 17,
        texture_slots: 1,
        uniform_types: vec![UniformType::Float],
    };
    assert_eq!(p.unsupported().len(), 1);
}

#[test]
fn value_coercion() {
    assert_eq!(
        UniformValue::Int(3).coerce(UniformType::Float),
        Some(UniformValue::Float(3.0))
    );
    assert_eq!(UniformValue::Float(3.0).coerce(UniformType::Int), None);
    assert_eq!(UniformValue::zero(UniformType::Mat4), None);
    assert_eq!(
        UniformValue::from_color(Color::WHITE),
        UniformValue::Vec4([1.0; 4])
    );
    assert_eq!(material_path("water"), "assets/materials/water.ron");
}
