//! [`DataError`] → [`Diagnostic`] (§8.2 stages 1 and 2 as found while loading).

use pkrs_core::{Diagnostic, SourceSpan};

use super::locate::{find_field_span, find_quoted_span, record_offset};
use crate::error::DataError;

/// The project-relative file an error is about, when it names one.
#[must_use]
pub fn data_error_file(err: &DataError) -> Option<&str> {
    match err {
        DataError::Parse { file, .. }
        | DataError::DuplicateId { file, .. }
        | DataError::Read { file, .. }
        | DataError::InFile { file, .. } => Some(file),
        // Per-file schema checks name the file as their context.
        DataError::InvalidField { context, .. } if is_data_file(context) => Some(context),
        _ => None,
    }
}

/// `err` as a diagnostic. With the `source` of its file the span is RON's parse position,
/// the repeated id of a duplicate, or the field / record a schema error names.
#[must_use]
pub fn data_error_diagnostic(err: &DataError, source: Option<&str>) -> Diagnostic {
    let diagnostic = describe(err, source);
    match data_error_file(err) {
        Some(file) => diagnostic.in_file(file),
        None => diagnostic,
    }
}

fn describe(err: &DataError, source: Option<&str>) -> Diagnostic {
    match err {
        DataError::Parse { message, .. } => match split_position(message) {
            Some((span, rest)) => Diagnostic::error("data::parse", rest.to_owned())
                .at(Some(span))
                .with_label(rest.to_owned()),
            None => Diagnostic::error("data::schema", message.clone()),
        },
        DataError::DuplicateId { id, .. } => {
            let span = source.and_then(|src| second_definition(src, id));
            Diagnostic::error("data::duplicate-id", err.to_string())
                .at(span)
                .with_label("defined again here")
                .with_help("rename one of them, or add `override: true` to the later record")
        }
        DataError::DuplicateContent { .. } | DataError::Duplicate { .. } => {
            Diagnostic::error("data::duplicate-id", err.to_string())
        }
        DataError::Read { message, file } => {
            Diagnostic::error("data::read", format!("cannot read `{file}`: {message}"))
        }
        DataError::InFile { source: inner, .. } => {
            let diagnostic = describe(inner, source);
            if diagnostic.span.is_some() {
                return diagnostic;
            }
            let span = source.and_then(|src| locate_inner(src, inner));
            diagnostic.at(span)
        }
        DataError::InvalidField {
            field,
            context,
            message,
        } => {
            let at = if is_data_file(context) {
                format!("`{context}`")
            } else {
                context.clone()
            };
            let span = source.and_then(|src| locate_inner(src, err));
            Diagnostic::error(
                "data::schema",
                format!("invalid `{field}` in {at}: {message}"),
            )
            .at(span)
            .with_label(message.clone())
        }
        DataError::UnknownReference { kind, .. } => Diagnostic::error(
            format!("data::unknown-{}", kind.replace(' ', "-")),
            err.to_string(),
        ),
        _ => Diagnostic::error("data::invalid", err.to_string()),
    }
}

/// The span of an [`DataError::InvalidField`]: its field inside the record its context
/// names (`species \`a\``), else the first occurrence of the field.
fn locate_inner(source: &str, err: &DataError) -> Option<SourceSpan> {
    let DataError::InvalidField { field, context, .. } = err else {
        return None;
    };
    let from = backticked(context).map_or(0, |id| record_offset(source, id));
    if field == "id" {
        if let Some(id) = backticked(context) {
            return find_quoted_span(source, id, from);
        }
    }
    find_field_span(source, field, from)
}

/// `true` for a content file path (`data/species/a.ron`).
fn is_data_file(text: &str) -> bool {
    text.ends_with(".ron") && !text.contains(' ')
}

/// The text between the first pair of backticks.
fn backticked(text: &str) -> Option<&str> {
    let start = text.find('`')? + 1;
    let len = text[start..].find('`')?;
    Some(&text[start..start + len])
}

/// The second `"<id>"` in `source` (the definition that clashes), else the first.
fn second_definition(source: &str, id: &str) -> Option<SourceSpan> {
    let first = record_offset(source, id);
    let quoted = format!("\"{id}\"");
    SourceSpan::find(source, &quoted, first + quoted.len())
        .or_else(|| find_quoted_span(source, id, first))
}

/// Splits RON's `line:column: message`.
fn split_position(message: &str) -> Option<(SourceSpan, &str)> {
    let (line, rest) = message.split_once(':')?;
    let (column, rest) = rest.split_once(':')?;
    let line: usize = line.trim().parse().ok()?;
    let column: usize = column.trim().parse().ok()?;
    Some((SourceSpan::new(line, column, 1), rest.trim()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ron_positions_become_spans() {
        assert_eq!(
            split_position("3:12: Expected closing `)`"),
            Some((SourceSpan::new(3, 12, 1), "Expected closing `)`"))
        );
        assert_eq!(split_position("party must have 1–6 members"), None);
    }

    #[test]
    fn backticks_name_the_record() {
        assert_eq!(backticked("species `leafkit`"), Some("leafkit"));
        assert_eq!(backticked("no ticks"), None);
    }
}
