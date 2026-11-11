//! Stage 3 (§8.2): cross-references between the records of a [`GameData`], plus the stage-2
//! schema rules of [`super::schema`].

use pkrs_core::Diagnostic;

use super::locate::{find_field_span, find_quoted_span, record_offset};
use super::schema;
use crate::battle_rules::BattleRules;
use crate::database::GameData;
use crate::files::DataFiles;
use crate::intern::{Keyed, Table};
use crate::move_def::MoveEffect;
use crate::text::{TEXT_PREFIX, TextDb, is_text_key};
use crate::trainer::BUILTIN_PROFILES;

/// The languages `text:` keys are checked against (`pkrs.toml [locale]`).
#[derive(Clone, Copy, Debug)]
pub struct TextLocale<'a> {
    /// `locale.default`: every key must exist here.
    pub default: &'a str,
    /// `locale.available`: a key missing in one of these is a warning (the default
    /// language is shown instead).
    pub available: &'a [String],
}

/// What [`check_game_data`] needs besides the data.
pub struct RefContext<'a> {
    /// The project files: sources for spans, and the existence of sprites and scripts.
    pub files: &'a dyn DataFiles,
    /// Languages for `text:` keys.
    pub locale: TextLocale<'a>,
    /// `true` for ids that plugin scripts may register at run time (`data.register_*`,
    /// §9.4), which are therefore not reported as unknown.
    pub registered_elsewhere: &'a dyn Fn(&str) -> bool,
}

/// The problems of `value` as a display field: nothing for literal text; for `text:<key>`
/// an error when the key is malformed or missing in the default language
/// (`text::missing-key`), and one warning listing the available languages that lack it
/// (`text::missing-translation`). The diagnostics have no file yet.
#[must_use]
pub fn check_text_ref(text: &TextDb, locale: &TextLocale<'_>, value: &str) -> Vec<Diagnostic> {
    let Some(key) = value.strip_prefix(TEXT_PREFIX) else {
        return Vec::new();
    };
    if !is_text_key(key) {
        return vec![
            Diagnostic::error(
                "text::invalid-key",
                format!("`{key}` is not a valid text key"),
            )
            .with_label("invalid text key")
            .with_help("text keys use lowercase letters, digits, `_`, `-`, `.` and `:`"),
        ];
    }
    if text.get(locale.default, key).is_none() {
        return vec![
            Diagnostic::error(
                "text::missing-key",
                format!(
                    "text key `{key}` is not defined for the default language `{}`",
                    locale.default
                ),
            )
            .with_label("unknown text key")
            .with_help(format!(
                "add `\"{key}\": \"…\"` to a file in data/text/{}/",
                locale.default
            )),
        ];
    }
    let missing: Vec<&str> = locale
        .available
        .iter()
        .map(String::as_str)
        .filter(|lang| *lang != locale.default && text.get(lang, key).is_none())
        .collect();
    if missing.is_empty() {
        return Vec::new();
    }
    vec![
        Diagnostic::warning(
            "text::missing-translation",
            format!(
                "text key `{key}` has no translation for {}",
                quoted_list(&missing)
            ),
        )
        .with_label("untranslated")
        .with_help(format!(
            "players of those languages see the `{}` text",
            locale.default
        )),
    ]
}

fn quoted_list(items: &[&str]) -> String {
    items
        .iter()
        .map(|s| format!("`{s}`"))
        .collect::<Vec<_>>()
        .join(", ")
}

/// `true` when the asset `path` exists: as written, under `assets/`, or for a namespaced
/// `plugin_id:path` under `plugins/<plugin_id>/assets/` (§9.4, §13).
#[must_use]
pub fn asset_exists(files: &dyn DataFiles, path: &str) -> bool {
    if let Some((plugin, rest)) = path.split_once(':') {
        return files.exists(&format!("plugins/{plugin}/assets/{rest}"));
    }
    files.exists(path) || files.exists(&format!("assets/{path}"))
}

/// Stage-2 rules that need the loaded records and every stage-3 cross-reference of `data`
/// (see the module docs of [`super`]). Each diagnostic names the record's file and, when the
/// file can be read, the span of the offending value.
#[must_use]
pub fn check_game_data(data: &GameData, ctx: &RefContext<'_>) -> Vec<Diagnostic> {
    let mut checker = Checker {
        data,
        ctx,
        rules: BattleRules::gen3_defaults(),
        out: Vec::new(),
    };
    checker.species();
    checker.moves();
    checker.trainers();
    checker.ai_profiles();
    checker.encounters();
    checker.display_text();
    checker.out
}

struct Checker<'a> {
    data: &'a GameData,
    ctx: &'a RefContext<'a>,
    rules: BattleRules,
    out: Vec<Diagnostic>,
}

/// Where a value sits: the record's file and id, and the text to point at (a quoted value,
/// or a field name prefixed with [`schema::FIELD`]).
struct At<'a> {
    file: &'a str,
    record: &'a str,
    needle: &'a str,
}

impl Checker<'_> {
    fn push(&mut self, at: &At<'_>, diagnostic: Diagnostic) {
        let span = self.ctx.files.read_text(at.file).ok().and_then(|src| {
            let from = record_offset(&src, at.record);
            match at.needle.strip_prefix(schema::FIELD) {
                Some(field) => find_field_span(&src, field, from),
                None => find_quoted_span(&src, at.needle, from),
            }
        });
        self.out.push(diagnostic.in_file(at.file).at(span));
    }

    fn unknown(&mut self, at: &At<'_>, kind: &str, owner: String, help: String) {
        if (self.ctx.registered_elsewhere)(at.needle) {
            return;
        }
        let diagnostic = Diagnostic::error(
            format!("data::unknown-{kind}"),
            format!("{owner} references unknown {kind} `{}`", at.needle),
        )
        .with_label(format!("no {kind} with this id"))
        .with_help(help);
        self.push(at, diagnostic);
    }

    fn type_known(&self, id: &str) -> bool {
        let types = self.data.type_chart().types();
        types.is_empty() || types.iter().any(|t| t == id)
    }

    fn species(&mut self) {
        let data = self.data;
        for (handle, sp) in data.species().iter() {
            let file = data.species().file(handle).unwrap_or_default();
            let owner = format!("species `{}`", sp.id);
            for (needle, d) in schema::species(sp) {
                self.push(
                    &At {
                        file,
                        record: &sp.id,
                        needle: &needle,
                    },
                    d,
                );
            }
            for t in &sp.types {
                if !self.type_known(t) {
                    let at = At {
                        file,
                        record: &sp.id,
                        needle: t,
                    };
                    self.unknown(
                        &at,
                        "type",
                        owner.clone(),
                        "types are defined in data/types.ron".into(),
                    );
                }
            }
            for a in &sp.abilities {
                if !data.abilities().contains(a) {
                    let at = At {
                        file,
                        record: &sp.id,
                        needle: a,
                    };
                    self.unknown(
                        &at,
                        "ability",
                        owner.clone(),
                        "define it in data/abilities/".into(),
                    );
                }
            }
            for entry in &sp.learnset {
                if !data.moves().contains(&entry.move_id) {
                    let at = At {
                        file,
                        record: &sp.id,
                        needle: &entry.move_id,
                    };
                    self.unknown(
                        &at,
                        "move",
                        owner.clone(),
                        "define it in data/moves/".into(),
                    );
                }
            }
            for evo in &sp.evolutions {
                if !data.species().contains(&evo.into) {
                    let at = At {
                        file,
                        record: &sp.id,
                        needle: &evo.into,
                    };
                    self.unknown(
                        &at,
                        "species",
                        owner.clone(),
                        "define it in data/species/".into(),
                    );
                }
            }
            let sprites = [&sp.sprites.front, &sp.sprites.back, &sp.sprites.icon];
            for path in sprites.into_iter().flatten() {
                if !asset_exists(self.ctx.files, path) {
                    let diagnostic = Diagnostic::error(
                        "data::missing-sprite",
                        format!("{owner} uses sprite `{path}`, which does not exist"),
                    )
                    .with_label("file not found")
                    .with_help("sprite paths are relative to the project (or its assets/ folder)");
                    self.push(
                        &At {
                            file,
                            record: &sp.id,
                            needle: path,
                        },
                        diagnostic,
                    );
                }
            }
        }
    }

    fn moves(&mut self) {
        let data = self.data;
        for (handle, mv) in data.moves().iter() {
            let file = data.moves().file(handle).unwrap_or_default();
            let owner = format!("move `{}`", mv.id);
            for (needle, d) in schema::move_def(mv) {
                self.push(
                    &At {
                        file,
                        record: &mv.id,
                        needle: &needle,
                    },
                    d,
                );
            }
            if !self.type_known(&mv.type_id) {
                let at = At {
                    file,
                    record: &mv.id,
                    needle: &mv.type_id,
                };
                self.unknown(
                    &at,
                    "type",
                    owner.clone(),
                    "types are defined in data/types.ron".into(),
                );
            }
            for effect in &mv.effects {
                if let MoveEffect::ApplyStatus { status, .. } = effect
                    && self.rules.status(status).is_none()
                    && !data.statuses().contains(status)
                {
                    let at = At {
                        file,
                        record: &mv.id,
                        needle: status,
                    };
                    self.unknown(
                            &at,
                            "status",
                            owner.clone(),
                            "built-in statuses are sleep, poison, burn, paralysis and freeze; define others in data/statuses/".into(),
                        );
                }
            }
        }
    }

    fn trainers(&mut self) {
        let data = self.data;
        for (handle, tr) in data.trainers().iter() {
            let file = data.trainers().file(handle).unwrap_or_default();
            let owner = format!("trainer `{}`", tr.id);
            for member in &tr.party {
                if !data.species().contains(&member.species) {
                    let at = At {
                        file,
                        record: &tr.id,
                        needle: &member.species,
                    };
                    self.unknown(
                        &at,
                        "species",
                        owner.clone(),
                        "define it in data/species/".into(),
                    );
                }
                for mv in &member.moves {
                    if !data.moves().contains(mv) {
                        let at = At {
                            file,
                            record: &tr.id,
                            needle: mv,
                        };
                        self.unknown(
                            &at,
                            "move",
                            owner.clone(),
                            "define it in data/moves/".into(),
                        );
                    }
                }
            }
            for item in &tr.items {
                if !data.items().contains(item) {
                    let at = At {
                        file,
                        record: &tr.id,
                        needle: item,
                    };
                    self.unknown(
                        &at,
                        "item",
                        owner.clone(),
                        "define it in data/items/".into(),
                    );
                }
            }
            let profile = tr.ai_profile_id();
            let builtin = BUILTIN_PROFILES.iter().any(|(name, _)| *name == profile);
            if !builtin && !data.ai_profiles().contains(profile) {
                let at = At {
                    file,
                    record: &tr.id,
                    needle: &tr.ai_profile,
                };
                self.unknown(
                    &at,
                    "ai-profile",
                    owner,
                    "use `random`, `basic` or `smart`, or define a profile in data/trainers/profiles/".into(),
                );
            }
        }
    }

    fn ai_profiles(&mut self) {
        let data = self.data;
        for (handle, profile) in data.ai_profiles().iter() {
            let Some(script) = profile.script.as_deref() else {
                continue;
            };
            if script.contains(':') && !script.ends_with(".lua") {
                // `plugin_id:function` names a function a plugin registers at run time.
                continue;
            }
            if !self.ctx.files.exists(script) {
                let file = data.ai_profiles().file(handle).unwrap_or_default();
                let diagnostic = Diagnostic::error(
                    "data::missing-script",
                    format!(
                        "AI profile `{}` uses script `{script}`, which does not exist",
                        profile.id
                    ),
                )
                .with_label("file not found");
                self.push(
                    &At {
                        file,
                        record: &profile.id,
                        needle: script,
                    },
                    diagnostic,
                );
            }
        }
    }

    fn encounters(&mut self) {
        let data = self.data;
        for (handle, table) in data.encounters().iter() {
            let file = data.encounters().file(handle).unwrap_or_default();
            let owner = format!("encounter table `{}`", table.id);
            for slot in &table.slots {
                if !data.species().contains(&slot.species) {
                    let at = At {
                        file,
                        record: &table.id,
                        needle: &slot.species,
                    };
                    self.unknown(
                        &at,
                        "species",
                        owner.clone(),
                        "define it in data/species/".into(),
                    );
                }
            }
        }
    }

    /// `text:` keys of every display field.
    fn display_text(&mut self) {
        let data = self.data;
        let mut fields: Vec<(String, String, String)> = Vec::new();
        collect(data.species(), &mut fields, |s| vec![s.name.as_str()]);
        collect(data.moves(), &mut fields, |m| vec![m.name.as_str()]);
        collect(data.abilities(), &mut fields, |a| {
            vec![a.name.as_str(), a.description.as_str()]
        });
        collect(data.items(), &mut fields, |i| {
            vec![i.name.as_str(), i.description.as_str()]
        });
        collect(data.effects(), &mut fields, |e| {
            vec![e.description.as_str()]
        });
        collect(data.trainers(), &mut fields, |t| {
            t.name.as_deref().into_iter().collect()
        });
        for (file, record, value) in fields {
            for d in check_text_ref(data.text(), &self.ctx.locale, &value) {
                self.push(
                    &At {
                        file: &file,
                        record: &record,
                        needle: &value,
                    },
                    d,
                );
            }
        }
    }
}

/// `(file, id, value)` for every `text:` value `fields` returns.
fn collect<T: Keyed>(
    table: &Table<T>,
    out: &mut Vec<(String, String, String)>,
    fields: impl Fn(&T) -> Vec<&str>,
) {
    for (handle, record) in table.iter() {
        for value in fields(record) {
            if value.starts_with(TEXT_PREFIX) {
                out.push((
                    table.file(handle).unwrap_or_default().to_owned(),
                    record.key().to_owned(),
                    value.to_owned(),
                ));
            }
        }
    }
}
