//! The active language and text lookup over layered text tables (§5.5, §2.2 `[locale]`).
//!
//! A [`Localizer`] holds the project's languages (`pkrs.toml [locale] default / available`),
//! the active one, and an ordered list of [`TextDb`] layers — normally the game's
//! `data/text/`, then each plugin's, then the engine's own table. A key is looked up in the
//! active language, then the default language, then English (the language engine texts ship
//! in); within each language the layers are tried in order, so a game can override an engine
//! text by defining the same key.
//!
//! Cloning is cheap (the layers are shared), so screens keep their own copy.

use core::fmt;
use std::sync::Arc;

use pkrs_core::manifest::LocaleConfig;

use crate::text::{TextArgs, TextDb, format_text, text_key};

/// Language engine texts are written in: the last fallback of every lookup.
pub const ENGINE_LANGUAGE: &str = "en";

/// [`Localizer::set_language`] was given a language the project does not declare.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UnknownLanguage {
    /// The language asked for.
    pub language: String,
    /// The languages the project declares.
    pub available: Vec<String>,
}

impl fmt::Display for UnknownLanguage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "unknown language `{}` (available: {})",
            self.language,
            self.available.join(", ")
        )
    }
}

impl std::error::Error for UnknownLanguage {}

/// Active language plus layered text tables.
#[derive(Clone, Debug, PartialEq)]
pub struct Localizer {
    layers: Vec<Arc<TextDb>>,
    language: String,
    default: String,
    available: Vec<String>,
}

impl Default for Localizer {
    /// English only, no text tables.
    fn default() -> Self {
        Self::new(ENGINE_LANGUAGE, Vec::new())
    }
}

impl Localizer {
    /// `default` active, `available` the languages a switch may pick (`default` is added
    /// when missing). No text tables yet.
    #[must_use]
    pub fn new(default: impl Into<String>, available: Vec<String>) -> Self {
        let default = default.into();
        let mut available = available;
        if !available.contains(&default) {
            available.insert(0, default.clone());
        }
        Self {
            layers: Vec::new(),
            language: default.clone(),
            default,
            available,
        }
    }

    /// The languages of a project's `[locale]` table, default active.
    #[must_use]
    pub fn from_config(config: &LocaleConfig) -> Self {
        Self::new(config.default.clone(), config.available.clone())
    }

    /// Adds a text table after the existing ones (lower priority), builder style.
    #[must_use]
    pub fn with_layer(mut self, text: Arc<TextDb>) -> Self {
        self.push_layer(text);
        self
    }

    /// Adds a text table after the existing ones (lower priority).
    pub fn push_layer(&mut self, text: Arc<TextDb>) {
        self.layers.push(text);
    }

    /// The text tables, highest priority first.
    #[must_use]
    pub fn layers(&self) -> &[Arc<TextDb>] {
        &self.layers
    }

    /// The active language.
    #[must_use]
    pub fn language(&self) -> &str {
        &self.language
    }

    /// The default language (`[locale] default`), the fallback of every lookup.
    #[must_use]
    pub fn default_language(&self) -> &str {
        &self.default
    }

    /// Languages a switch may pick (`[locale] available`).
    #[must_use]
    pub fn available(&self) -> &[String] {
        &self.available
    }

    /// Switches the active language. Only declared languages are accepted.
    pub fn set_language(&mut self, language: &str) -> Result<(), UnknownLanguage> {
        if !self.available.iter().any(|l| l == language) {
            return Err(UnknownLanguage {
                language: language.to_owned(),
                available: self.available.clone(),
            });
        }
        language.clone_into(&mut self.language);
        Ok(())
    }

    /// Active language, default language, then [`ENGINE_LANGUAGE`], without repeats.
    fn chain(&self) -> impl Iterator<Item = &str> {
        let langs = [
            self.language.as_str(),
            self.default.as_str(),
            ENGINE_LANGUAGE,
        ];
        langs
            .into_iter()
            .enumerate()
            .filter(move |(i, l)| !langs[..*i].contains(l))
            .map(|(_, l)| l)
    }

    /// The text of `key` (no `text:` prefix) through the language chain and the layers.
    #[must_use]
    pub fn lookup(&self, key: &str) -> Option<&str> {
        self.chain()
            .find_map(|lang| self.layers.iter().find_map(|db| db.get(lang, key)))
    }

    /// `true` when some language of the chain defines `key`.
    #[must_use]
    pub fn has_key(&self, key: &str) -> bool {
        self.lookup(key).is_some()
    }

    /// A display value with its arguments filled in: `text:<key>` is looked up, literal
    /// text is used as written. `None` for a `text:` key no table defines.
    #[must_use]
    pub fn resolve(&self, value: &str, args: &TextArgs) -> Option<String> {
        let template = match text_key(value) {
            Some(key) => self.lookup(key)?,
            None => value,
        };
        Some(format_text(template, args))
    }

    /// Like [`resolve`](Self::resolve), showing a missing `text:` key verbatim so the
    /// problem stays visible on screen.
    #[must_use]
    pub fn text(&self, value: &str, args: &TextArgs) -> String {
        self.resolve(value, args)
            .unwrap_or_else(|| value.to_owned())
    }

    /// The text of the engine or game key `key` (no `text:` prefix) with `args`; the key
    /// itself when no table defines it.
    #[must_use]
    pub fn key(&self, key: &str, args: &TextArgs) -> String {
        format_text(self.lookup(key).unwrap_or(key), args)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    fn db(lang: &str, src: &str) -> Arc<TextDb> {
        let mut db = TextDb::new();
        db.insert_ron(lang, src, "t.ron").unwrap();
        Arc::new(db)
    }

    fn localizer() -> Localizer {
        let mut game = TextDb::new();
        game.insert_ron(
            "it",
            r#"{ "hello": "Ciao {name}!", "yes": "Sì!" }"#,
            "it.ron",
        )
        .unwrap();
        game.insert_ron(
            "en",
            r#"{ "hello": "Hello {name}!", "only_en": "Only" }"#,
            "en.ron",
        )
        .unwrap();
        let engine = db("en", r#"{ "yes": "Yes", "no": "No" }"#);
        Localizer::new("it", vec!["it".into(), "en".into()])
            .with_layer(Arc::new(game))
            .with_layer(engine)
    }

    #[test]
    fn resolves_keys_in_the_active_language_with_arguments() {
        let mut loc = localizer();
        let args = TextArgs::new().with("name", "Ash");
        assert_eq!(loc.language(), "it");
        assert_eq!(loc.text("text:hello", &args), "Ciao Ash!");
        assert_eq!(loc.text("Literal {name}", &args), "Literal Ash");
        loc.set_language("en").unwrap();
        assert_eq!(loc.text("text:hello", &args), "Hello Ash!");
        assert_eq!(loc.default_language(), "it");
    }

    #[test]
    fn falls_back_to_the_default_language_then_english() {
        let mut loc = localizer();
        assert_eq!(
            loc.key("only_en", &TextArgs::new()),
            "Only",
            "default -> en"
        );
        assert_eq!(loc.key("yes", &TextArgs::new()), "Sì!", "game layer wins");
        assert_eq!(
            loc.key("no", &TextArgs::new()),
            "No",
            "engine layer, English"
        );
        loc.set_language("en").unwrap();
        assert_eq!(loc.key("yes", &TextArgs::new()), "Yes");
    }

    #[test]
    fn missing_keys_and_unknown_languages() {
        let mut loc = localizer();
        assert_eq!(loc.resolve("text:nope", &TextArgs::new()), None);
        assert_eq!(loc.text("text:nope", &TextArgs::new()), "text:nope");
        assert_eq!(loc.key("nope", &TextArgs::new()), "nope");
        assert!(!loc.has_key("nope"));
        let err = loc.set_language("fr").unwrap_err();
        assert_eq!(err.to_string(), "unknown language `fr` (available: it, en)");
        assert_eq!(loc.language(), "it");
    }

    #[test]
    fn config_and_defaults() {
        let mut config = LocaleConfig::default();
        config.default = "it".into();
        config.available = vec!["en".into(), "it".into()];
        let loc = Localizer::from_config(&config);
        assert_eq!(loc.available(), ["en", "it"]);
        let plain = Localizer::default();
        assert_eq!(plain.language(), "en");
        assert_eq!(plain.available(), ["en"]);
        assert!(plain.layers().is_empty());
        let added = Localizer::new("pt-BR", vec!["en".into()]);
        assert_eq!(added.available(), ["pt-BR", "en"]);
    }
}
