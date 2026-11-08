//! Finding records and values in RON source text, for diagnostic spans.
//!
//! The searches are textual (RON's parser does not keep positions after decoding): a record
//! is found by its quoted id, a value by its quoted text after that record.

use pkrs_core::SourceSpan;
use pkrs_core::diagnostic::find_from;

/// Byte offset of the record with `id` in `source`: the first `"<id>"` written after an
/// `id:` key, else the first `"<id>"` anywhere, else 0.
#[must_use]
pub fn record_offset(source: &str, id: &str) -> usize {
    let quoted = format!("\"{id}\"");
    let mut from = 0;
    let mut first = None;
    while let Some(at) = find_from(source, &quoted, from) {
        first.get_or_insert(at);
        let before = source[..at].trim_end();
        if before.ends_with("id:") && !before.ends_with("_id:") {
            return at;
        }
        from = at + quoted.len();
    }
    first.unwrap_or(0)
}

/// The span of `"<value>"` (quotes included) at or after byte `from`, or of the bare
/// `value` when it is not quoted there.
#[must_use]
pub fn find_quoted_span(source: &str, value: &str, from: usize) -> Option<SourceSpan> {
    SourceSpan::find(source, &format!("\"{value}\""), from)
        .or_else(|| SourceSpan::find(source, value, from))
}

/// The span of the field name `field` (followed by `:`) at or after byte `from`.
#[must_use]
pub fn find_field_span(source: &str, field: &str, from: usize) -> Option<SourceSpan> {
    let mut at = from;
    while let Some(start) = find_from(source, field, at) {
        let rest = source[start + field.len()..].trim_start();
        let boundary = source[..start]
            .chars()
            .next_back()
            .is_none_or(|c| !(c.is_ascii_alphanumeric() || c == '_'));
        if boundary && rest.starts_with(':') {
            return Some(SourceSpan::from_byte_range(
                source,
                start..start + field.len(),
            ));
        }
        at = start + field.len();
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    const SRC: &str = "[\n  (id: \"a\", learnset: [(level: 1, move: \"b\")]),\n  (id: \"b\", type_id: \"a\"),\n]\n";

    #[test]
    fn records_are_found_by_their_id_key() {
        let at = record_offset(SRC, "b");
        assert_eq!(&SRC[at..at + 3], "\"b\"");
        assert_eq!(SourceSpan::from_byte_range(SRC, at..at + 3).line, 3);
        assert_eq!(record_offset(SRC, "zzz"), 0);
        assert_eq!(
            find_quoted_span(SRC, "b", 0),
            Some(SourceSpan::new(2, 41, 3))
        );
        assert_eq!(
            find_field_span(SRC, "learnset", 0),
            Some(SourceSpan::new(2, 13, 8))
        );
        assert_eq!(find_field_span(SRC, "id", record_offset(SRC, "b")), None);
    }
}
