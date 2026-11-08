//! Vendor-free diagnostics (§8.2): one problem found while validating a project, with its
//! file, line/column and a label, shared by `pkrs check` and the runtime's start-up check.
//!
//! Library crates build [`Diagnostic`]s; only the CLI renders them with `miette` (in its
//! report adapter). [`Diagnostic`]'s `Display` is a plain, one-block text form used for logs.
//!
//! Positions are 1-based lines and columns counted in characters, so they read the same as an
//! editor's; [`SourceSpan::byte_range`] turns them back into byte offsets for renderers.

use core::fmt;
use core::ops::Range;

/// How bad a [`Diagnostic`] is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Severity {
    /// Content is invalid: `pkrs check` fails and release builds refuse to start.
    Error,
    /// Suspicious but valid content (`pkrs check --deny-warnings` turns these into failures).
    Warning,
}

impl fmt::Display for Severity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Error => "error",
            Self::Warning => "warning",
        })
    }
}

/// A region of a source file: 1-based line and column (in characters) and a length in
/// characters (at least 1 when rendered).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SourceSpan {
    /// 1-based line.
    pub line: usize,
    /// 1-based column, in characters.
    pub column: usize,
    /// Length in characters.
    pub len: usize,
}

impl SourceSpan {
    /// A span at `line:column` covering `len` characters.
    #[must_use]
    pub const fn new(line: usize, column: usize, len: usize) -> Self {
        Self { line, column, len }
    }

    /// The span of the bytes `range` of `source` (clamped to the source and snapped to char
    /// boundaries).
    #[must_use]
    pub fn from_byte_range(source: &str, range: Range<usize>) -> Self {
        let start = floor_boundary(source, range.start);
        let end = floor_boundary(source, range.end.max(start));
        let before = &source[..start];
        let line = before.matches('\n').count() + 1;
        let line_start = before.rfind('\n').map_or(0, |i| i + 1);
        let column = before[line_start..].chars().count() + 1;
        let len = source[start..end].chars().count();
        Self { line, column, len }
    }

    /// The span of the first occurrence of `needle` in `source` at or after byte `from`.
    #[must_use]
    pub fn find(source: &str, needle: &str, from: usize) -> Option<Self> {
        find_from(source, needle, from)
            .map(|start| Self::from_byte_range(source, start..start + needle.len()))
    }

    /// The byte range of this span in `source`, if the line exists (the column and length
    /// are clamped to the line).
    #[must_use]
    pub fn byte_range(&self, source: &str) -> Option<Range<usize>> {
        let line_start = if self.line <= 1 {
            0
        } else {
            source
                .match_indices('\n')
                .nth(self.line - 2)
                .map(|(i, _)| i + 1)?
        };
        let line_text = source[line_start..].split('\n').next().unwrap_or_default();
        let line_text = line_text.strip_suffix('\r').unwrap_or(line_text);
        let char_offset = |n: usize| {
            line_text
                .char_indices()
                .nth(n)
                .map_or(line_text.len(), |(i, _)| i)
        };
        let col = self.column.saturating_sub(1);
        let start = char_offset(col);
        let end = char_offset(col + self.len.max(1)).max(start);
        Some(line_start + start..line_start + end)
    }
}

impl fmt::Display for SourceSpan {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.line, self.column)
    }
}

/// Byte offset of `needle` in `source` at or after `from` (clamped, char-boundary safe).
#[must_use]
pub fn find_from(source: &str, needle: &str, from: usize) -> Option<usize> {
    let from = floor_boundary(source, from);
    source[from..].find(needle).map(|i| from + i)
}

fn floor_boundary(source: &str, offset: usize) -> usize {
    let offset = offset.min(source.len());
    (0..=offset)
        .rev()
        .find(|&i| source.is_char_boundary(i))
        .unwrap_or(0)
}

/// One problem found while validating a project (§8.2).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Diagnostic {
    /// Error or warning.
    pub severity: Severity,
    /// Stable rule code (`data::unknown-move`, `map::convention`, …), documented for CI.
    pub code: String,
    /// What is wrong.
    pub message: String,
    /// Project-relative file (`/`-separated), when the problem is in one.
    pub file: Option<String>,
    /// Where in [`file`](Self::file), when known.
    pub span: Option<SourceSpan>,
    /// Short text shown at the span (defaults to the message).
    pub label: Option<String>,
    /// How to fix it.
    pub help: Option<String>,
}

impl Diagnostic {
    /// A diagnostic of `severity` with `code` and `message`.
    #[must_use]
    pub fn new(severity: Severity, code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            severity,
            code: code.into(),
            message: message.into(),
            file: None,
            span: None,
            label: None,
            help: None,
        }
    }

    /// An error.
    #[must_use]
    pub fn error(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self::new(Severity::Error, code, message)
    }

    /// A warning.
    #[must_use]
    pub fn warning(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self::new(Severity::Warning, code, message)
    }

    /// Sets the file.
    #[must_use]
    pub fn in_file(mut self, file: impl Into<String>) -> Self {
        self.file = Some(file.into());
        self
    }

    /// Sets the span (keeps the current one when `span` is `None`).
    #[must_use]
    pub fn at(mut self, span: Option<SourceSpan>) -> Self {
        if span.is_some() {
            self.span = span;
        }
        self
    }

    /// Sets the label shown at the span.
    #[must_use]
    pub fn with_label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// Sets the help text.
    #[must_use]
    pub fn with_help(mut self, help: impl Into<String>) -> Self {
        self.help = Some(help.into());
        self
    }

    /// `true` for errors.
    #[must_use]
    pub fn is_error(&self) -> bool {
        self.severity == Severity::Error
    }
}

/// Plain-text form for logs:
///
/// ```text
/// error[data::unknown-move]: species `leafkit` learns unknown move `vine`
///   --> data/species/leafkit.ron:4:30
///   = help: define it in data/moves/
/// ```
impl fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}[{}]: {}", self.severity, self.code, self.message)?;
        if let Some(file) = &self.file {
            write!(f, "\n  --> {file}")?;
            if let Some(span) = self.span {
                write!(f, ":{span}")?;
            }
        }
        if let Some(help) = &self.help {
            write!(f, "\n  = help: {help}")?;
        }
        Ok(())
    }
}

/// Every diagnostic of a validation run, in the order found (errors are never fail-fast).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Diagnostics(Vec<Diagnostic>);

impl Diagnostics {
    /// None yet.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds one.
    pub fn push(&mut self, diagnostic: Diagnostic) {
        self.0.push(diagnostic);
    }

    /// All of them.
    #[must_use]
    pub fn items(&self) -> &[Diagnostic] {
        &self.0
    }

    /// Number of errors.
    #[must_use]
    pub fn error_count(&self) -> usize {
        self.0.iter().filter(|d| d.is_error()).count()
    }

    /// Number of warnings.
    #[must_use]
    pub fn warning_count(&self) -> usize {
        self.0.len() - self.error_count()
    }

    /// `true` when at least one is an error.
    #[must_use]
    pub fn has_errors(&self) -> bool {
        self.0.iter().any(Diagnostic::is_error)
    }

    /// `true` when there are none.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Number of diagnostics.
    #[must_use]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Iterates in order.
    pub fn iter(&self) -> core::slice::Iter<'_, Diagnostic> {
        self.0.iter()
    }

    /// Removes exact duplicates (same severity, code, message, file and span), keeping the
    /// first.
    pub fn dedup(&mut self) {
        let mut seen = Vec::with_capacity(self.0.len());
        self.0.retain(|d| {
            if seen.contains(d) {
                false
            } else {
                seen.push(d.clone());
                true
            }
        });
    }

    /// The diagnostics, consuming the collection.
    #[must_use]
    pub fn into_vec(self) -> Vec<Diagnostic> {
        self.0
    }
}

impl Extend<Diagnostic> for Diagnostics {
    fn extend<I: IntoIterator<Item = Diagnostic>>(&mut self, iter: I) {
        self.0.extend(iter);
    }
}

impl FromIterator<Diagnostic> for Diagnostics {
    fn from_iter<I: IntoIterator<Item = Diagnostic>>(iter: I) -> Self {
        Self(iter.into_iter().collect())
    }
}

impl IntoIterator for Diagnostics {
    type Item = Diagnostic;
    type IntoIter = std::vec::IntoIter<Diagnostic>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()
    }
}

impl<'a> IntoIterator for &'a Diagnostics {
    type Item = &'a Diagnostic;
    type IntoIter = core::slice::Iter<'a, Diagnostic>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.iter()
    }
}

/// Every diagnostic in plain text, separated by blank lines.
impl fmt::Display for Diagnostics {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (i, d) in self.0.iter().enumerate() {
            if i > 0 {
                f.write_str("\n\n")?;
            }
            write!(f, "{d}")?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spans_round_trip_through_byte_ranges() {
        let src = "a = 1\nbé = \"x\"\n";
        let start = src.find("\"x\"").unwrap();
        let span = SourceSpan::from_byte_range(src, start..start + 3);
        assert_eq!(span, SourceSpan::new(2, 6, 3));
        assert_eq!(span.byte_range(src), Some(start..start + 3));
        assert_eq!(
            SourceSpan::find(src, "1", 0),
            Some(SourceSpan::new(1, 5, 1))
        );
        assert_eq!(SourceSpan::find(src, "nope", 0), None);
        // A line past the end has no range; a column past the end clamps.
        assert_eq!(SourceSpan::new(9, 1, 1).byte_range(src), None);
        assert_eq!(SourceSpan::new(1, 40, 2).byte_range(src), Some(5..5));
    }

    #[test]
    fn crlf_lines_do_not_include_the_carriage_return() {
        let src = "ab\r\ncd\r\n";
        assert_eq!(SourceSpan::new(1, 1, 9).byte_range(src), Some(0..2));
        assert_eq!(SourceSpan::new(2, 2, 1).byte_range(src), Some(5..6));
    }

    #[test]
    fn display_is_one_block_per_diagnostic() {
        let d = Diagnostic::error("data::unknown-move", "unknown move `vine`")
            .in_file("data/species/a.ron")
            .at(Some(SourceSpan::new(4, 30, 6)))
            .with_help("define it in data/moves/");
        assert_eq!(
            d.to_string(),
            "error[data::unknown-move]: unknown move `vine`\n  --> data/species/a.ron:4:30\n  = help: define it in data/moves/"
        );
        let w = Diagnostic::warning("script::tick", "tick listener");
        assert_eq!(w.to_string(), "warning[script::tick]: tick listener");
        let mut all: Diagnostics = [d.clone(), w, d].into_iter().collect();
        assert_eq!(all.error_count(), 2);
        assert_eq!(all.warning_count(), 1);
        all.dedup();
        assert_eq!(all.len(), 2);
        assert!(all.has_errors());
        assert!(all.to_string().contains("\n\nwarning[script::tick]"));
    }
}
