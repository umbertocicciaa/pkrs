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
//!
//! Texts may contain **argument placeholders** filled by [`format_text`] from [`TextArgs`]:
//! `{name}` takes the argument called `name`, `{0}`, `{1}`, … the positional arguments in
//! order. A placeholder without a matching argument stays as written, so dialogue control
//! codes (`{player}`, `{color:red}`, `{pause:20}`, `{{`, §5.5) pass through untouched.
//!
//! ```ron
//! // data/text/en/battle.ron
//! { "resonance.start": "{name} is resonating!" }
//! ```
//!
//! [`Localizer`](crate::Localizer) layers several databases (game, plugins, engine) and picks
//! the active language with a fallback to the default one.

use std::collections::BTreeMap;

use crate::codec::RonCodec;
use crate::database::{in_file, ron_files, sorted};
use crate::error::{DataError, Result};
use crate::files::DataFiles;

/// The key of a `text:<key>` reference, or `None` for literal text.
#[must_use]
pub fn text_key(value: &str) -> Option<&str> {
    value.strip_prefix(TEXT_PREFIX)
}

/// Arguments of a localized text (`{name}` / `{0}` placeholders), in order.
///
/// Positional arguments are named by their 0-based index (`"0"`, `"1"`, …), so `{0}` and a
/// named `{name}` are looked up the same way.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TextArgs(Vec<(String, String)>);

impl TextArgs {
    /// No arguments.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Positional arguments `{0}`, `{1}`, … in order.
    #[must_use]
    pub fn positional<I, S>(values: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self(
            values
                .into_iter()
                .enumerate()
                .map(|(i, v)| (i.to_string(), v.into()))
                .collect(),
        )
    }

    /// Adds (or replaces) the argument `name`, builder style.
    #[must_use]
    pub fn with(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.set(name, value);
        self
    }

    /// Sets the argument `name` to `value`, replacing an earlier one.
    pub fn set(&mut self, name: impl Into<String>, value: impl Into<String>) {
        let (name, value) = (name.into(), value.into());
        match self.0.iter_mut().find(|(n, _)| *n == name) {
            Some(entry) => entry.1 = value,
            None => self.0.push((name, value)),
        }
    }

    /// The value of `name` (`"0"` for the first positional argument).
    #[must_use]
    pub fn get(&self, name: &str) -> Option<&str> {
        self.0
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.as_str())
    }

    /// `true` without arguments.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// `(name, value)` pairs in order.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &str)> {
        self.0.iter().map(|(n, v)| (n.as_str(), v.as_str()))
    }
}

/// Fills the `{name}` / `{0}` placeholders of `template` from `args`. `{{` and placeholders
/// without a matching argument (dialogue control codes such as `{player}`) are kept as
/// written; argument values are inserted as they are.
#[must_use]
pub fn format_text(template: &str, args: &TextArgs) -> String {
    if args.is_empty() || !template.contains('{') {
        return template.to_owned();
    }
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        if let Some(escaped) = after.strip_prefix('{') {
            out.push_str("{{");
            rest = escaped;
            continue;
        }
        let Some(close) = after.find('}') else {
            out.push_str(&rest[open..]);
            rest = "";
            break;
        };
        let name = &after[..close];
        match args.get(name) {
            Some(value) => out.push_str(value),
            None => {
                out.push('{');
                out.push_str(name);
                out.push('}');
            }
        }
        rest = &after[close + 1..];
    }
    out.push_str(rest);
    out
}

/// What a display value resolves to in a [`TextDb`] ([`TextDb::lookup`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextLookup<'a> {
    /// Not a `text:` reference: the value is literal text.
    Literal(&'a str),
    /// The text of the referenced key.
    Found(&'a str),
    /// A `text:` reference to a key that neither language defines (the key, without the
    /// prefix). This is what a missing-key report lists.
    Missing(&'a str),
}

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

    /// Loads every `<dir>/<lang>/*.ron` file (normally `data/text`), language folders and
    /// files in name order, merging each into its language. Returns the problems found; the
    /// readable files are still merged.
    pub fn load_dir(&mut self, files: &dyn DataFiles, dir: &str) -> Vec<DataError> {
        let mut errors = Vec::new();
        for lang in sorted(files.list_dirs(dir)) {
            let lang_dir = format!("{dir}/{lang}");
            for name in ron_files(files, &lang_dir) {
                let file = format!("{lang_dir}/{name}");
                match files.read_text(&file) {
                    Ok(source) => {
                        if let Err(e) = self.insert_ron(&lang, &source, &file) {
                            errors.push(in_file(&file, e));
                        }
                    }
                    Err(message) => errors.push(DataError::Read { file, message }),
                }
            }
        }
        errors
    }

    /// `true` when `lang` defines `key`.
    #[must_use]
    pub fn contains(&self, lang: &str, key: &str) -> bool {
        self.get(lang, key).is_some()
    }

    /// Like [`resolve`](Self::resolve), telling literal text, a found key and a missing key
    /// apart (missing-key reports use this).
    #[must_use]
    pub fn lookup<'a>(&'a self, lang: &str, fallback: &str, value: &'a str) -> TextLookup<'a> {
        match text_key(value) {
            None => TextLookup::Literal(value),
            Some(key) => self
                .get(lang, key)
                .or_else(|| self.get(fallback, key))
                .map_or(TextLookup::Missing(key), TextLookup::Found),
        }
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
    fn arguments_fill_named_and_positional_placeholders() {
        let args = TextArgs::new().with("name", "Ash").with("n", "3");
        assert_eq!(
            format_text("{name} won {n} times!", &args),
            "Ash won 3 times!"
        );
        let pos = TextArgs::positional(["Ash", "Leafkit"]);
        assert_eq!(
            format_text("{0} sent out {1}!", &pos),
            "Ash sent out Leafkit!"
        );
        assert_eq!(pos.get("1"), Some("Leafkit"));
        // Unknown placeholders, control codes, escapes and unclosed braces stay as written.
        assert_eq!(
            format_text("{color:red}{name}{{x}} {player} {missing} {open", &args),
            "{color:red}Ash{{x}} {player} {missing} {open"
        );
        assert_eq!(format_text("{name}", &TextArgs::new()), "{name}");
        let mut replaced = TextArgs::new().with("a", "1");
        replaced.set("a", "2");
        assert_eq!(replaced.iter().collect::<Vec<_>>(), [("a", "2")]);
        assert!(!replaced.is_empty());
    }

    #[test]
    fn lookup_tells_literal_found_and_missing_apart() {
        let mut db = TextDb::new();
        db.insert_ron("en", r#"{ "a": "A" }"#, "en.ron").unwrap();
        assert_eq!(db.lookup("it", "en", "text:a"), TextLookup::Found("A"));
        assert_eq!(db.lookup("it", "en", "text:b"), TextLookup::Missing("b"));
        assert_eq!(db.lookup("it", "en", "plain"), TextLookup::Literal("plain"));
        assert!(db.contains("en", "a"));
        assert!(!db.contains("it", "a"));
        assert_eq!(text_key("text:a"), Some("a"));
        assert_eq!(text_key("a"), None);
    }

    #[test]
    fn load_dir_reads_every_language_folder() {
        let files = crate::MemoryDataFiles::new()
            .with("data/text/en/a.ron", r#"{ "a": "A" }"#)
            .with("data/text/it/a.ron", r#"{ "a": "Ah" }"#)
            .with("data/text/it/b.ron", "{ ")
            .with("data/text/it/notes.txt", "ignored");
        let mut db = TextDb::new();
        let errors = db.load_dir(&files, "data/text");
        assert_eq!(errors.len(), 1, "{errors:?}");
        assert!(errors[0].to_string().contains("data/text/it/b.ron"));
        assert_eq!(db.get("it", "a"), Some("Ah"));
        assert_eq!(db.languages().collect::<Vec<_>>(), ["en", "it"]);
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
