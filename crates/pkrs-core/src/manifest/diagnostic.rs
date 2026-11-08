//! Manifest problems as [`Diagnostic`]s (§8.2): every [`ManifestIssue`] gets the file and,
//! when the source is at hand, the span of the field its dotted path names.

use crate::diagnostic::{Diagnostic, SourceSpan};

use super::error::{ManifestError, ManifestIssue, PluginManifestError};

/// Code of manifest parse errors (TOML syntax, wrong types, unknown keys).
pub const CODE_PARSE: &str = "manifest::parse";
/// Code of manifest validation errors (ranges, id formats, semver, engine requirement).
pub const CODE_INVALID: &str = "manifest::invalid";

impl ManifestIssue {
    /// This issue as an error in `file`. With the `source` text the span is the parse
    /// position, or the value of the field the dotted path names.
    #[must_use]
    pub fn to_diagnostic(&self, code: &str, file: &str, source: Option<&str>) -> Diagnostic {
        let message = if self.path.is_empty() {
            self.message.clone()
        } else {
            format!("`{}`: {}", self.path, self.message)
        };
        let span = match (self.location, source) {
            (Some(loc), _) => Some(SourceSpan::new(loc.line, loc.column, 1)),
            (None, Some(src)) => locate_toml_path(src, &self.path),
            (None, None) => None,
        };
        Diagnostic::error(code, message)
            .in_file(file)
            .at(span)
            .with_label(self.message.clone())
    }
}

impl ManifestError {
    /// Every problem as a diagnostic in `file` (`pkrs.toml`).
    #[must_use]
    pub fn diagnostics(&self, file: &str, source: Option<&str>) -> Vec<Diagnostic> {
        match self {
            Self::Io { source: err, .. } => {
                vec![
                    Diagnostic::error("manifest::read", format!("cannot read {file}: {err}"))
                        .in_file(file),
                ]
            }
            Self::Parse(issue) => vec![issue.to_diagnostic(CODE_PARSE, file, source)],
            Self::Invalid(issues) => issues
                .iter()
                .map(|i| i.to_diagnostic(CODE_INVALID, file, source))
                .collect(),
        }
    }
}

impl PluginManifestError {
    /// Every problem as a diagnostic in `file` (`plugins/<id>/plugin.toml`).
    #[must_use]
    pub fn diagnostics(&self, file: &str, source: Option<&str>) -> Vec<Diagnostic> {
        let (code, issues) = match self {
            Self::Parse(issue) => ("plugin::manifest-parse", core::slice::from_ref(issue)),
            Self::Invalid(issues) => ("plugin::manifest-invalid", issues.as_slice()),
        };
        issues
            .iter()
            .map(|i| i.to_diagnostic(code, file, source))
            .collect()
    }
}

/// The span of the value of the TOML field `path` (`display.scale`, `plugins.enabled[1]`,
/// `dependencies.core`) in `source`: the element for an indexed path, the whole value for a
/// key, the table header when the key is absent. `None` when the table is not in the file.
///
/// A line-based search: it understands `[table]` headers and `key = value` lines (also
/// quoted keys), which is what pkrs manifests use.
#[must_use]
pub fn locate_toml_path(source: &str, path: &str) -> Option<SourceSpan> {
    if path.is_empty() {
        return None;
    }
    let (table, rest) = match path.split_once('.') {
        Some((table, rest)) => (table, Some(rest)),
        None => (path, None),
    };
    let mut offset = 0;
    let mut header: Option<(usize, usize)> = None;
    let mut in_table = false;
    for line in source.split_inclusive('\n') {
        let start = offset;
        offset += line.len();
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            if in_table {
                break;
            }
            if trimmed == format!("[{table}]") {
                let lead = line.len() - line.trim_start().len();
                header = Some((start + lead, trimmed.len()));
                in_table = true;
            }
            continue;
        }
        let Some(rest) = rest.filter(|_| in_table) else {
            continue;
        };
        let (key, index) = split_index(rest);
        let Some(value) = key_value(line, key) else {
            continue;
        };
        let value_start = start + value.0;
        let value_text = &line[value.0..value.0 + value.1];
        let range = match index.and_then(|n| nth_element(value_text, n)) {
            Some((s, len)) => value_start + s..value_start + s + len,
            None => value_start..value_start + value_text.len(),
        };
        return Some(SourceSpan::from_byte_range(source, range));
    }
    header.map(|(s, len)| SourceSpan::from_byte_range(source, s..s + len))
}

/// `enabled[1]` → (`enabled`, Some(1)); keys may be quoted.
fn split_index(rest: &str) -> (&str, Option<usize>) {
    let (key, index) = match rest.strip_suffix(']').and_then(|r| r.rsplit_once('[')) {
        Some((key, n)) => (key, n.parse().ok()),
        None => (rest, None),
    };
    (key.trim_matches('"'), index)
}

/// `(offset, len)` of the value of `key = value` on `line`.
fn key_value(line: &str, key: &str) -> Option<(usize, usize)> {
    let lead = line.len() - line.trim_start().len();
    let body = &line[lead..];
    let after = body
        .strip_prefix(key)
        .or_else(|| body.strip_prefix(&format!("\"{key}\"")))?;
    let after_key = after.trim_start();
    let after_eq = after_key.strip_prefix('=')?;
    let value = after_eq.trim();
    if value.is_empty() {
        return None;
    }
    let value_offset = line.len() - after_eq.trim_start().len();
    Some((value_offset, value.len()))
}

/// `(offset, len)` of the `n`-th (0-based) element of a one-line array value.
fn nth_element(value: &str, n: usize) -> Option<(usize, usize)> {
    let inner = value.strip_prefix('[')?;
    let mut offset = 1;
    for (i, part) in inner.split(',').enumerate() {
        let part_trimmed = part.trim().trim_end_matches(']').trim();
        if i == n && !part_trimmed.is_empty() {
            let lead = part.len() - part.trim_start().len();
            return Some((offset + lead, part_trimmed.len()));
        }
        offset += part.len() + 1;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::{Location, ProjectManifest};

    const SRC: &str = "[game]\nname = \"T\"\nid = \"Bad Id\"\n\n[display]\nscale = 99\n\n[plugins]\nenabled = [\"a\", \"ghost\"]\n";

    #[test]
    fn locates_keys_elements_and_headers() {
        assert_eq!(
            locate_toml_path(SRC, "display.scale"),
            Some(SourceSpan::new(6, 9, 2))
        );
        assert_eq!(
            locate_toml_path(SRC, "game.id"),
            Some(SourceSpan::new(3, 6, 8))
        );
        assert_eq!(
            locate_toml_path(SRC, "plugins.enabled[1]"),
            Some(SourceSpan::new(9, 17, 7))
        );
        // The key is missing: the table header; the table is missing: nothing.
        assert_eq!(
            locate_toml_path(SRC, "display.width"),
            Some(SourceSpan::new(5, 1, 9))
        );
        assert_eq!(locate_toml_path(SRC, "start.map"), None);
        assert_eq!(locate_toml_path(SRC, ""), None);
    }

    #[test]
    fn invalid_manifests_become_located_diagnostics() {
        let err = ManifestError::Invalid(vec![ManifestIssue::new(
            "display.scale",
            "must be between 1 and 16",
        )]);
        let diags = err.diagnostics("pkrs.toml", Some(SRC));
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].code, CODE_INVALID);
        assert_eq!(
            diags[0].message,
            "`display.scale`: must be between 1 and 16"
        );
        assert_eq!(diags[0].span, Some(SourceSpan::new(6, 9, 2)));

        let parse = ManifestError::Parse(ManifestIssue {
            path: String::new(),
            message: "expected `=`".into(),
            location: Some(Location { line: 2, column: 3 }),
        });
        let diags = parse.diagnostics("pkrs.toml", None);
        assert_eq!(diags[0].code, CODE_PARSE);
        assert_eq!(diags[0].span, Some(SourceSpan::new(2, 3, 1)));
    }

    #[test]
    fn real_validation_errors_are_located() {
        let src = "[game]\nname = \"T\"\nid = \"com.example.t\"\n[engine]\nversion = \"0.0\"\napi = 1\n\
                   [display]\nscale = 40\n[start]\nmap = \"m\"\nposition = [0, 0]\n";
        let err = ProjectManifest::from_toml_str(src).unwrap_err();
        let diags = err.diagnostics("pkrs.toml", Some(src));
        let scale = diags
            .iter()
            .find(|d| d.message.contains("display.scale"))
            .unwrap();
        assert_eq!(scale.span, Some(SourceSpan::new(8, 9, 2)));
    }
}
