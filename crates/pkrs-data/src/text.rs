//! Localized text (§8.1): `data/text/<lang>/*.ron`.
//!
//! Each file is a RON map from text keys to strings; every file of a language folder is
//! merged into that language's table, and a key defined twice is an error.
//!
//! ```ron
//! // data/text/en/species.ron
//! {
//!     "species.emberkit": "Emberkit",
//!     "moves.ember": "Ember",
//! }
//! ```
//!
//! Content fields that hold display text (`name`, `description`) either contain literal
//! text or a reference `text:<key>` ([`TEXT_PREFIX`]) resolved with [`TextDb::resolve`].

use std::collections::BTreeMap;

use crate::codec::RonCodec;
use crate::error::{DataError, Result};

/// Prefix of a text reference in content fields (`text:species.emberkit`).
pub const TEXT_PREFIX: &str = "text:";

/// `true` for a valid text key: non-empty ASCII lowercase letters, digits, `_`, `-`, `.`
/// and `:`.
#[must_use]
pub fn is_text_key(key: &str) -> bool {
    !key.is_empty()
        && key.bytes().all(|b| {
            b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'_' | b'-' | b'.' | b':')
        })
}

/// One text entry and the file that defined it.
#[derive(Clone, Debug, PartialEq, Eq)]
struct TextEntry {
    text: String,
    file: String,
}

/// Every language's text table.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TextDb {
    langs: BTreeMap<String, BTreeMap<String, TextEntry>>,
}

impl TextDb {
    /// No text.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Parses one text file (a map of keys to strings) and checks its keys.
    pub fn parse_ron(source: &str, file: &str) -> Result<BTreeMap<String, String>> {
        let map: BTreeMap<String, String> = RonCodec::decode(source, file)?;
        if let Some(bad) = map.keys().find(|k| !is_text_key(k)) {
            return Err(DataError::InvalidField {
                field: "key".into(),
                context: file.into(),
                message: format!(
                    "`{bad}` is not a text key (lowercase letters, digits, `_`, `-`, `.`, `:`)"
                ),
            });
        }
        Ok(map)
    }

    /// Parses `source` (from `file`) and merges it into language `lang`. A key that `lang`
    /// already has is an error ([`DataError::DuplicateId`]); the other keys of the file are
    /// still added.
    pub fn insert_ron(&mut self, lang: &str, source: &str, file: &str) -> Result<()> {
        let map = Self::parse_ron(source, file)?;
        let table = self.langs.entry(lang.to_owned()).or_default();
        let mut duplicate = None;
        for (key, text) in map {
            if let Some(existing) = table.get(&key) {
                duplicate.get_or_insert_with(|| DataError::DuplicateId {
                    kind: format!("text key ({lang})"),
                    id: key.clone(),
                    file: file.into(),
                    first: existing.file.clone(),
                });
                continue;
            }
            table.insert(
                key,
                TextEntry {
                    text,
                    file: file.into(),
                },
            );
        }
        duplicate.map_or(Ok(()), Err)
    }

    /// The text of `key` in `lang`.
    #[must_use]
    pub fn get(&self, lang: &str, key: &str) -> Option<&str> {
        self.langs.get(lang)?.get(key).map(|e| e.text.as_str())
    }

    /// Resolves a display field: `text:<key>` is looked up in `lang`, then in `fallback`;
    /// anything else is literal text and returned as is. `None` for an unknown key.
    #[must_use]
    pub fn resolve<'a>(&'a self, lang: &str, fallback: &str, value: &'a str) -> Option<&'a str> {
        match value.strip_prefix(TEXT_PREFIX) {
            Some(key) => self.get(lang, key).or_else(|| self.get(fallback, key)),
            None => Some(value),
        }
    }

    /// Languages with at least one file, sorted.
    pub fn languages(&self) -> impl Iterator<Item = &str> {
        self.langs.keys().map(String::as_str)
    }

    /// Keys of `lang`, sorted.
    pub fn keys(&self, lang: &str) -> impl Iterator<Item = &str> {
        self.langs
            .get(lang)
            .into_iter()
            .flat_map(|t| t.keys().map(String::as_str))
    }

    /// Number of keys in `lang`.
    #[must_use]
    pub fn len(&self, lang: &str) -> usize {
        self.langs.get(lang).map_or(0, BTreeMap::len)
    }

    /// `true` when no language has any text.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.langs.values().all(BTreeMap::is_empty)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    #[test]
    fn files_merge_per_language_and_resolve() {
        let mut db = TextDb::new();
        db.insert_ron(
            "en",
            r#"{ "species.emberkit": "Emberkit" }"#,
            "data/text/en/a.ron",
        )
        .unwrap();
        db.insert_ron("en", r#"{ "moves.ember": "Ember" }"#, "data/text/en/b.ron")
            .unwrap();
        db.insert_ron("it", r#"{ "moves.ember": "Brace" }"#, "data/text/it/a.ron")
            .unwrap();
        assert_eq!(db.languages().collect::<Vec<_>>(), ["en", "it"]);
        assert_eq!(db.len("en"), 2);
        assert_eq!(db.get("it", "moves.ember"), Some("Brace"));
        assert_eq!(db.resolve("it", "en", "text:moves.ember"), Some("Brace"));
        assert_eq!(
            db.resolve("it", "en", "text:species.emberkit"),
            Some("Emberkit"),
            "falls back"
        );
        assert_eq!(db.resolve("it", "en", "Literal"), Some("Literal"));
        assert_eq!(db.resolve("en", "en", "text:missing"), None);
    }

    #[test]
    fn duplicate_and_bad_keys_are_errors() {
        let mut db = TextDb::new();
        db.insert_ron("en", r#"{ "a": "x" }"#, "data/text/en/a.ron")
            .unwrap();
        let err = db
            .insert_ron("en", r#"{ "a": "y", "b": "z" }"#, "data/text/en/b.ron")
            .unwrap_err();
        assert_eq!(
            err.to_string(),
            "duplicate text key (en) `a` in `data/text/en/b.ron` (already defined in `data/text/en/a.ron`)"
        );
        assert_eq!(db.get("en", "a"), Some("x"));
        assert_eq!(db.get("en", "b"), Some("z"));
        let bad = TextDb::parse_ron(r#"{ "Bad Key": "x" }"#, "t.ron").unwrap_err();
        assert!(bad.to_string().contains("not a text key"), "{bad}");
        assert!(TextDb::parse_ron("(a: 1)", "t.ron").is_err());
    }
}
