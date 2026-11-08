//! `miette` adapter (§4.5, §8.2): renders the engine's vendor-free [`Diagnostic`]s with
//! source snippets for `pkrs check`. This is the only module that uses `miette`.
//!
//! Output is deterministic: sources are read with `\r\n` turned into `\n` and file names are
//! the project-relative `/` paths of the diagnostics, so the text is identical on every OS.

use std::fmt::Write as _;

use miette::{
    GraphicalReportHandler, GraphicalTheme, LabeledSpan, MietteDiagnostic, NamedSource, Report,
    Severity as MietteSeverity,
};
use pkrs_core::{Diagnostic, Diagnostics, Severity};

/// How diagnostics are drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReportStyle {
    /// ANSI colours (off for pipes, `NO_COLOR` and tests).
    pub color: bool,
    /// Wrap width in columns.
    pub width: usize,
}

impl Default for ReportStyle {
    fn default() -> Self {
        Self {
            color: false,
            width: 100,
        }
    }
}

/// Renders every diagnostic (with a snippet of its file when `source` returns its text)
/// followed by a summary line. `source` maps a project-relative path to the file's text.
#[must_use]
pub fn render_diagnostics(
    diagnostics: &Diagnostics,
    source: &dyn Fn(&str) -> Option<String>,
    style: ReportStyle,
) -> String {
    let theme = if style.color {
        GraphicalTheme::unicode()
    } else {
        GraphicalTheme::unicode_nocolor()
    };
    let handler = GraphicalReportHandler::new_themed(theme)
        .with_width(style.width)
        .with_links(false)
        .with_context_lines(1);
    let mut out = String::new();
    for d in diagnostics {
        let report = to_report(d, source);
        let mut text = String::new();
        if handler.render_report(&mut text, report.as_ref()).is_err() {
            text = format!("{d}\n");
        }
        out.push_str(&text);
        if !text.ends_with('\n') {
            out.push('\n');
        }
        out.push('\n');
    }
    let _ = writeln!(out, "{}", summary(diagnostics));
    out
}

/// `2 errors, 1 warning` (or `no problems found`).
#[must_use]
pub fn summary(diagnostics: &Diagnostics) -> String {
    let plural = |n: usize, word: &str| format!("{n} {word}{}", if n == 1 { "" } else { "s" });
    match (diagnostics.error_count(), diagnostics.warning_count()) {
        (0, 0) => "no problems found".to_owned(),
        (e, w) => format!("{}, {}", plural(e, "error"), plural(w, "warning")),
    }
}

fn to_report(d: &Diagnostic, source: &dyn Fn(&str) -> Option<String>) -> Report {
    let severity = match d.severity {
        Severity::Error => MietteSeverity::Error,
        Severity::Warning => MietteSeverity::Warning,
    };
    let text = d
        .file
        .as_deref()
        .and_then(source)
        .map(|s| s.replace("\r\n", "\n"));
    let range = match (&text, d.span) {
        (Some(src), Some(span)) => span.byte_range(src),
        _ => None,
    };
    let message = match (&d.file, &range) {
        (Some(file), None) => format!("{file}: {}", d.message),
        _ => d.message.clone(),
    };
    let mut diagnostic = MietteDiagnostic::new(message)
        .with_code(d.code.clone())
        .with_severity(severity);
    if let Some(help) = &d.help {
        diagnostic = diagnostic.with_help(help.clone());
    }
    match (d.file.as_deref(), text, range) {
        (Some(file), Some(src), Some(range)) => {
            let len = range.end.saturating_sub(range.start);
            diagnostic = diagnostic.with_label(LabeledSpan::new(d.label.clone(), range.start, len));
            Report::new(diagnostic).with_source_code(NamedSource::new(file, src))
        }
        _ => Report::new(diagnostic),
    }
}

#[cfg(test)]
mod tests {
    use pkrs_core::SourceSpan;

    use super::*;

    #[test]
    fn renders_snippets_and_a_summary() {
        let diags: Diagnostics = [
            Diagnostic::error(
                "data::unknown-move",
                "species `a` references unknown move `vine`",
            )
            .in_file("data/species/a.ron")
            .at(Some(SourceSpan::new(2, 9, 6)))
            .with_label("no move with this id")
            .with_help("define it in data/moves/"),
            Diagnostic::warning("script::tick-listener", "tick").in_file("scripts/x.lua"),
        ]
        .into_iter()
        .collect();
        let source = |path: &str| {
            (path == "data/species/a.ron")
                .then(|| "(id: \"a\",\r\n  move: \"vine\")\r\n".to_owned())
        };
        let text = render_diagnostics(&diags, &source, ReportStyle::default());
        assert!(text.contains("data::unknown-move"), "{text}");
        assert!(text.contains("[data/species/a.ron:2:9]"), "{text}");
        assert!(text.contains("no move with this id"), "{text}");
        assert!(text.contains("scripts/x.lua: tick"), "{text}");
        assert!(!text.contains('\r'));
        assert!(text.ends_with("1 error, 1 warning\n"), "{text}");
        assert_eq!(summary(&Diagnostics::new()), "no problems found");
    }
}
