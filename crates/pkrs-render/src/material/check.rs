//! Material descriptors as `pkrs check` diagnostics (§5.6, §8.2).

use pkrs_core::{Diagnostic, Diagnostics, SourceSpan};

use super::descriptor::{At, analyze};

/// Checks one material descriptor (`file` is its project-relative path, `source` its text):
/// RON schema, shader reference, built-in parameters, capability profile against the
/// engine, and the project shader against its own profile. `read_shader` reads a
/// project-relative file (`assets/shaders/water.glsl`).
///
/// Rule codes: `material::parse`, `material::unknown-shader`, `material::missing-shader`,
/// `material::missing-profile`, `material::unsupported-profile`, `material::glsl-version`,
/// `material::too-many-uniforms`, `material::too-many-textures`,
/// `material::uniform-type-not-allowed`, `material::unknown-uniform`,
/// `material::uniform-type`, `material::reserved-uniform`, `material::missing-uniform`,
/// `material::invalid-value`; warnings `material::unset-uniform`, `material::builtin-profile`.
#[must_use]
pub fn check_material(
    file: &str,
    source: &str,
    read_shader: &dyn Fn(&str) -> Option<String>,
) -> Diagnostics {
    let analysis = analyze(source, read_shader);
    let mut out = Diagnostics::new();
    for issue in analysis.issues {
        let (in_file, span) = match &issue.at {
            At::Nothing => (file.to_owned(), None),
            At::Position(line, col) => (file.to_owned(), Some(SourceSpan::new(*line, *col, 1))),
            At::Material(needle) => (file.to_owned(), SourceSpan::find(source, needle, 0)),
            At::Shader(range) => match &analysis.shader_file {
                Some((shader, glsl)) => (
                    shader.clone(),
                    Some(SourceSpan::from_byte_range(glsl, range.clone())),
                ),
                None => (file.to_owned(), None),
            },
        };
        let mut d = Diagnostic::new(issue.severity, issue.code, issue.message)
            .in_file(in_file)
            .at(span);
        if let Some(label) = issue.label {
            d = d.with_label(label);
        }
        if let Some(help) = issue.help {
            d = d.with_help(help);
        }
        out.push(d);
    }
    out
}
