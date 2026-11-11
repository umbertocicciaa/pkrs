//! Localization in the runtime (§5.5, §2.2 `[locale]`): the engine's own text table, the
//! project's [`Localizer`] and the names battle texts use.
//!
//! - **Engine texts** (`crates/pkrs-runtime/text/<lang>/engine.ron`, shipped in English and
//!   Italian) hold every string the engine shows itself: `ui.yes` / `ui.no`, battle menus and
//!   battle messages. They are the last layer of every lookup, so a game overrides any of them
//!   by defining the same key in its own `data/text/<lang>/` table.
//! - **[`project_localizer`]** layers the game's `data/text/`, each plugin's
//!   `plugins/<id>/data/text/` (load order) and the engine table, with the project's
//!   `[locale] default` active.
//! - **[`BattleText`]** wraps a localizer for the battle screen: engine keys with arguments
//!   and localized content names (`name` fields, else a `<kind>.<id>` key such as
//!   `moves.ember`, else a readable form of the id).

use std::collections::BTreeMap;
use std::sync::{Arc, OnceLock};

use pkrs_assets::Vfs;
use pkrs_data::{DATA_ROOT, GameData, Localizer, TextArgs, TextDb};

use crate::battle::content::display_name;
use crate::data::VfsDataFiles;

/// The engine's text tables, one `(language, source)` per shipped language.
const ENGINE_TEXT: &[(&str, &str)] = &[
    ("en", include_str!("../text/en/engine.ron")),
    ("it", include_str!("../text/it/engine.ron")),
];

/// The engine's text table (parsed once).
#[must_use]
pub fn engine_text() -> Arc<TextDb> {
    static TEXT: OnceLock<Arc<TextDb>> = OnceLock::new();
    TEXT.get_or_init(|| {
        let mut db = TextDb::new();
        for (lang, source) in ENGINE_TEXT {
            let file = format!("engine/text/{lang}/engine.ron");
            if let Err(err) = db.insert_ron(lang, source, &file) {
                tracing::error!(%err, "engine text table");
            }
        }
        Arc::new(db)
    })
    .clone()
}

/// English, with only the engine's texts (tests and screens built without a project).
#[must_use]
pub fn engine_localizer() -> Localizer {
    Localizer::default().with_layer(engine_text())
}

/// The project's localizer: `locale` languages (default active) over the game's
/// `data/text/`, each plugin's `plugins/<id>/data/text/` in `plugins` order, then the
/// engine texts. Broken text files are logged and skipped (`pkrs check` reports them).
#[must_use]
pub fn project_localizer(
    vfs: &dyn Vfs,
    locale: &pkrs_core::manifest::LocaleConfig,
    plugins: &[String],
) -> Localizer {
    let files = VfsDataFiles(vfs);
    let mut localizer = Localizer::from_config(locale);
    let roots = std::iter::once(format!("{DATA_ROOT}/text"))
        .chain(plugins.iter().map(|id| format!("plugins/{id}/data/text")));
    for root in roots {
        let mut db = TextDb::new();
        for err in db.load_dir(&files, &root) {
            tracing::warn!(%err, "text table");
        }
        if !db.is_empty() {
            localizer.push_layer(Arc::new(db));
        }
    }
    for lang in localizer.available() {
        if localizer.layers().iter().all(|db| db.len(lang) == 0) {
            tracing::debug!(%lang, "no text table for a declared language; texts fall back");
        }
    }
    localizer.with_layer(engine_text())
}

/// Text for the battle screens: engine keys with arguments and content names in the active
/// language. Cheap to clone.
#[derive(Clone, Debug)]
pub struct BattleText {
    localizer: Localizer,
    /// Display field (`name`) per `<kind>.<id>` (`moves.ember` → `text:moves.ember`).
    names: Arc<BTreeMap<String, String>>,
}

impl Default for BattleText {
    /// English engine texts, names derived from ids.
    fn default() -> Self {
        Self::new(engine_localizer(), None)
    }
}

impl BattleText {
    /// Texts from `localizer`, with the `name` fields of `data`'s species, moves, items and
    /// abilities.
    #[must_use]
    pub fn new(localizer: Localizer, data: Option<&GameData>) -> Self {
        let mut names = BTreeMap::new();
        if let Some(data) = data {
            let mut add = |kind: &str, id: &str, name: &str| {
                if !name.is_empty() {
                    names.insert(format!("{kind}.{id}"), name.to_owned());
                }
            };
            for s in data.species().records() {
                add("species", &s.id, &s.name);
            }
            for m in data.moves().records() {
                add("moves", &m.id, &m.name);
            }
            for i in data.items().records() {
                add("items", &i.id, &i.name);
            }
            for a in data.abilities().records() {
                add("abilities", &a.id, &a.name);
            }
        }
        Self {
            localizer,
            names: Arc::new(names),
        }
    }

    /// The localizer.
    #[must_use]
    pub fn localizer(&self) -> &Localizer {
        &self.localizer
    }

    /// The engine (or game) text `key` with named `args`.
    #[must_use]
    pub fn say(&self, key: &str, args: &[(&str, &str)]) -> String {
        let mut text_args = TextArgs::new();
        for (name, value) in args {
            text_args.set(*name, *value);
        }
        self.localizer.key(key, &text_args)
    }

    /// A display value (`text:` key or literal) with `args`; a missing key is shown readable
    /// (without the `text:` prefix).
    #[must_use]
    pub fn value(&self, value: &str, args: &TextArgs) -> String {
        self.localizer
            .resolve(value, args)
            .unwrap_or_else(|| pkrs_data::text_key(value).unwrap_or(value).to_owned())
    }

    /// Localized name of content `id` of `kind` (`species`, `moves`, `items`, `abilities`,
    /// `statuses`): its `name` field, else the `<kind>.<id>` key, else `None`.
    fn lookup_name(&self, kind: &str, id: &str) -> Option<String> {
        let key = format!("{kind}.{id}");
        if let Some(field) = self.names.get(&key)
            && let Some(name) = self.localizer.resolve(field, &TextArgs::new())
        {
            return Some(name);
        }
        self.localizer.lookup(&key).map(str::to_owned)
    }

    /// Localized species name.
    #[must_use]
    pub fn species(&self, id: &str) -> String {
        self.lookup_name("species", id)
            .unwrap_or_else(|| display_name(id))
    }

    /// Localized move name.
    #[must_use]
    pub fn move_name(&self, id: &str) -> String {
        self.lookup_name("moves", id)
            .unwrap_or_else(|| display_name(id))
    }

    /// Localized item name.
    #[must_use]
    pub fn item(&self, id: &str) -> String {
        self.lookup_name("items", id)
            .unwrap_or_else(|| display_name(id))
    }

    /// Localized status, volatile or weather name as used inside a sentence (`statuses.<id>`,
    /// else the lowercase readable id).
    #[must_use]
    pub fn status(&self, id: &str) -> String {
        self.lookup_name("statuses", id)
            .unwrap_or_else(|| display_name(id).to_lowercase())
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;
    use pkrs_assets::MemoryVfs;
    use pkrs_core::manifest::LocaleConfig;

    #[test]
    fn engine_tables_parse_and_cover_the_same_keys() {
        let db = engine_text();
        assert_eq!(db.languages().collect::<Vec<_>>(), ["en", "it"]);
        let en: Vec<_> = db.keys("en").collect();
        let it: Vec<_> = db.keys("it").collect();
        assert_eq!(en, it, "every engine key is translated");
        assert_eq!(db.get("en", "ui.yes"), Some("Yes"));
    }

    #[test]
    fn project_texts_override_engine_texts_and_follow_the_language() {
        let mut vfs = MemoryVfs::new();
        vfs.insert(
            "data/text/it/ui.ron",
            br#"{ "hello": "Ciao!", "ui.yes": "Certo" }"#.to_vec(),
        )
        .unwrap();
        vfs.insert("data/text/en/ui.ron", br#"{ "hello": "Hello!" }"#.to_vec())
            .unwrap();
        vfs.insert(
            "plugins/res/data/text/en/res.ron",
            br#"{ "res.start": "{name} resonates!", "hello": "ignored" }"#.to_vec(),
        )
        .unwrap();
        let mut locale = LocaleConfig::default();
        locale.default = "it".into();
        locale.available = vec!["it".into(), "en".into()];
        let mut loc = project_localizer(&vfs, &locale, &["res".to_owned()]);
        let none = TextArgs::new();
        assert_eq!(loc.text("text:hello", &none), "Ciao!");
        assert_eq!(loc.key("ui.yes", &none), "Certo", "game overrides engine");
        assert_eq!(loc.key("ui.no", &none), "No");
        loc.set_language("en").unwrap();
        assert_eq!(
            loc.text("text:hello", &none),
            "Hello!",
            "game before plugin"
        );
        assert_eq!(loc.key("ui.yes", &none), "Yes");
        assert_eq!(
            loc.text("text:res.start", &TextArgs::new().with("name", "Ash")),
            "Ash resonates!"
        );
    }

    #[test]
    fn battle_text_names_and_messages() {
        let text = BattleText::default();
        assert_eq!(
            text.say("battle.go", &[("name", "Leafkit")]),
            "Go! Leafkit!"
        );
        assert_eq!(text.move_name("vine_whip"), "Vine whip");
        assert_eq!(text.status("burn"), "burn");
        assert_eq!(text.value("text:x.y", &TextArgs::new()), "x.y");
        let mut it = engine_localizer();
        it = Localizer::new("en", vec!["it".into()]).with_layer(it.layers()[0].clone());
        it.set_language("it").unwrap();
        let text = BattleText::new(it, None);
        assert_eq!(
            text.say("battle.go", &[("name", "Leafkit")]),
            "Vai! Leafkit!"
        );
    }
}
