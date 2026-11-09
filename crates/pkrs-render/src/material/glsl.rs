//! A tiny GLSL scanner for material capability checks (§5.6): the `#version` line and the
//! `uniform` declarations of a fragment shader. It does not compile GLSL; the backend does
//! that when the material is created.

use core::ops::Range;

/// One `uniform <type> <name>;` declaration.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct GlslUniform {
    /// Type as written (`float`, `vec4`, `sampler2D`, `mat4`, …).
    pub ty: String,
    /// Uniform name.
    pub name: String,
    /// `true` for an array (`uniform float weights[4];`), which v1 does not bind.
    pub array: bool,
    /// Byte range of the name in the source.
    pub range: Range<usize>,
}

/// What [`scan`] found.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct GlslFacts {
    /// The `#version` value (`100`), if present.
    pub version: Option<String>,
    /// Uniform declarations in source order.
    pub uniforms: Vec<GlslUniform>,
}

const PRECISIONS: &[&str] = &["lowp", "mediump", "highp"];

/// Scans `source` (comments are skipped).
pub(crate) fn scan(source: &str) -> GlslFacts {
    let code = blank_comments(source);
    let mut facts = GlslFacts::default();
    for line in code.lines() {
        let trimmed = line.trim_start();
        if let Some(rest) = trimmed.strip_prefix("#version") {
            if facts.version.is_none() {
                facts.version = rest.split_whitespace().next().map(str::to_owned);
            }
        }
    }
    // Statements end with `;`; a uniform declaration starts with the `uniform` keyword.
    let mut start = 0;
    for (end, ch) in code.char_indices() {
        if ch != ';' {
            continue;
        }
        let mut statement = &code[start..end];
        let mut offset = start;
        start = end + 1;
        // A declaration after a function body starts after its closing brace.
        if let Some(brace) = statement.rfind(['{', '}']) {
            offset += brace + 1;
            statement = &statement[brace + 1..];
        }
        let lead = statement.len() - statement.trim_start().len();
        let body = statement.trim_start();
        // Skip preprocessor lines that precede the declaration.
        let (body, lead) = skip_directives(body, offset + lead);
        let Some(rest) = body.strip_prefix("uniform") else {
            continue;
        };
        if !rest.starts_with(char::is_whitespace) {
            continue;
        }
        let rest_at = lead + "uniform".len();
        let mut tokens = tokens(rest, rest_at).into_iter().peekable();
        while tokens
            .peek()
            .is_some_and(|(t, _)| PRECISIONS.contains(&t.as_str()))
        {
            tokens.next();
        }
        let Some((ty, _)) = tokens.next() else {
            continue;
        };
        // `uniform vec4 a, b;` declares two uniforms.
        for (name, range) in tokens {
            let name = name.trim_matches(',');
            if name.is_empty() {
                continue;
            }
            let (name, array) = match name.find('[') {
                Some(i) => (&name[..i], true),
                None => (name, false),
            };
            facts.uniforms.push(GlslUniform {
                ty: ty.clone(),
                name: name.to_owned(),
                array,
                range: range.start..range.start + name.len(),
            });
        }
    }
    facts
}

/// Drops leading `#…` lines from a statement, returning the rest and its byte offset.
fn skip_directives(mut body: &str, mut at: usize) -> (&str, usize) {
    while body.starts_with('#') {
        let line_end = body.find('\n').map_or(body.len(), |i| i + 1);
        let rest = &body[line_end..];
        let trimmed = rest.trim_start();
        at += line_end + (rest.len() - trimmed.len());
        body = trimmed;
    }
    (body, at)
}

/// Whitespace-separated tokens of `text` (commas split too) with their byte ranges, where
/// `text` starts at byte `at` of the source.
fn tokens(text: &str, at: usize) -> Vec<(String, Range<usize>)> {
    let mut out = Vec::new();
    let mut current: Option<usize> = None;
    for (i, ch) in text.char_indices() {
        let sep = ch.is_whitespace() || ch == ',';
        match (sep, current) {
            (true, Some(s)) => {
                out.push((text[s..i].to_owned(), at + s..at + i));
                current = None;
            }
            (false, None) => current = Some(i),
            _ => {}
        }
    }
    if let Some(s) = current {
        out.push((text[s..].to_owned(), at + s..at + text.len()));
    }
    out
}

/// `source` with `//` and `/* */` comments replaced by spaces (byte offsets are kept).
fn blank_comments(source: &str) -> String {
    let bytes = source.as_bytes();
    let mut out = String::with_capacity(source.len());
    let mut i = 0;
    for (at, ch) in source.char_indices() {
        if at < i {
            continue;
        }
        if ch == '/' && bytes.get(at + 1) == Some(&b'/') {
            let end = source[at..].find('\n').map_or(source.len(), |n| at + n);
            out.extend(std::iter::repeat_n(' ', end - at));
            i = end;
            continue;
        }
        if ch == '/' && bytes.get(at + 1) == Some(&b'*') {
            let end = source[at + 2..]
                .find("*/")
                .map_or(source.len(), |n| at + 2 + n + 2);
            for c in source[at..end].chars() {
                if c == '\n' {
                    out.push('\n');
                } else {
                    out.extend(std::iter::repeat_n(' ', c.len_utf8()));
                }
            }
            i = end;
            continue;
        }
        out.push(ch);
        i = at + ch.len_utf8();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_version_and_uniforms() {
        let src = "#version 100\n// uniform float ignored;\nprecision mediump float;\n\
                   uniform sampler2D Texture;\nuniform lowp vec4 tint, glow;\n\
                   /* uniform mat4 hidden; */\nuniform float weights[4];\nvoid main() {}\n";
        let facts = scan(src);
        assert_eq!(facts.version.as_deref(), Some("100"));
        let found: Vec<_> = facts
            .uniforms
            .iter()
            .map(|u| (u.ty.as_str(), u.name.as_str(), u.array))
            .collect();
        assert_eq!(
            found,
            [
                ("sampler2D", "Texture", false),
                ("vec4", "tint", false),
                ("vec4", "glow", false),
                ("float", "weights", true),
            ]
        );
        let tint = &facts.uniforms[1];
        assert_eq!(&src[tint.range.clone()], "tint");
    }

    #[test]
    fn no_version_line() {
        assert_eq!(scan("uniform float a;").version, None);
        assert_eq!(scan("uniform float a;").uniforms.len(), 1);
    }
}
