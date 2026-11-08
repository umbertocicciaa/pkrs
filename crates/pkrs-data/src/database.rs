//! The immutable game content database (§8.1): every schema of a project's `data/` folder,
//! loaded through a [`DataFiles`] port, merged per folder, interned into [`Handle`]s and
//! frozen into a [`GameData`].
//!
//! | Kind | Location | Battle data |
//! |---|---|---|
//! | type chart | `data/types.ron` | yes |
//! | species | `data/species/*.ron` | yes |
//! | moves | `data/moves/*.ron` | yes |
//! | abilities | `data/abilities/*.ron` | yes |
//! | statuses | `data/statuses/*.ron` | yes |
//! | effect primitives | `data/effects/*.ron` | yes |
//! | trainers | `data/trainers/*.ron` | yes |
//! | AI profiles | `data/trainers/profiles/*.ron` | yes |
//! | encounter tables | `data/encounters/*.ron` | yes |
//! | items | `data/items/*.ron` | no |
//! | terrains | `data/terrains/*.ron` | no |
//! | text | `data/text/<lang>/*.ron` | no |
//!
//! - **Files** hold one record or a list ([`RonCodec::decode_records`]); a folder's files
//!   are read in name order, so loading is deterministic.
//! - **Duplicates** within the game's folders are an error naming both files
//!   ([`DataError::DuplicateId`]) unless the later record declares `override: true`.
//! - **Battle data** is skipped when [`LoadOptions::battles`] is `false`
//!   (`features.battles = false` in `pkrs.toml`), so story-only projects need none of it.
//! - **Plugins** add their records through the mutable [`ContentDb`] first (§9.4); a
//!   [`GameDataBuilder`] merges that database in before the content is frozen.
//! - Problems are collected, not fail-fast ([`DataErrors`]).
//!
//! Cross-reference checks (§8.2 stage 3) are not part of loading; see [`crate::validate`].

use core::fmt;
use std::collections::BTreeMap;

use pkrs_core::manifest::Features;
use serde::de::DeserializeOwned;

use crate::battle_pack::BattlePack;
use crate::battle_rules::StatusDef;
use crate::codec::RonCodec;
use crate::content::{
    AbilityDef, ContentDb, ContentDef, ContentKind, EffectDef, ItemDef, TerrainDef, is_content_id,
    parse_content,
};
use crate::encounter::{EncounterTable, EncounterTables};
use crate::error::DataError;
use crate::files::DataFiles;
use crate::intern::{Keyed, Table};
use crate::move_def::MoveDef;
use crate::species::SpeciesDef;
use crate::text::TextDb;
use crate::trainer::{AiProfile, TrainerDef};
use crate::type_chart::TypeChart;

/// The game's content folder.
pub const DATA_ROOT: &str = "data";

/// What to load.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LoadOptions {
    /// Load battle data (types, species, moves, abilities, statuses, effects, trainers, AI
    /// profiles, encounters). Default `true`.
    pub battles: bool,
}

impl Default for LoadOptions {
    fn default() -> Self {
        Self { battles: true }
    }
}

impl LoadOptions {
    /// Options for a project's `[features]` (`features.battles`).
    #[must_use]
    pub fn from_features(features: &Features) -> Self {
        Self {
            battles: features.battles,
        }
    }
}

/// The kinds of data files.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum DataKind {
    /// `types.ron`.
    TypeChart,
    /// `species/`.
    Species,
    /// `moves/`.
    Move,
    /// `abilities/`.
    Ability,
    /// `items/`.
    Item,
    /// `statuses/`.
    Status,
    /// `terrains/`.
    Terrain,
    /// `effects/`.
    Effect,
    /// `trainers/`.
    Trainer,
    /// `trainers/profiles/`.
    AiProfile,
    /// `encounters/`.
    Encounter,
    /// `text/<lang>/`.
    Text,
}

impl DataKind {
    /// Every kind, in load order.
    pub const ALL: &'static [DataKind] = &[
        Self::TypeChart,
        Self::Species,
        Self::Move,
        Self::Ability,
        Self::Item,
        Self::Status,
        Self::Terrain,
        Self::Effect,
        Self::Trainer,
        Self::AiProfile,
        Self::Encounter,
        Self::Text,
    ];

    /// Location below the data root: a file (`types.ron`) or a folder (`species`).
    #[must_use]
    pub fn path(self) -> &'static str {
        match self {
            Self::TypeChart => "types.ron",
            Self::Species => "species",
            Self::Move => "moves",
            Self::Ability => "abilities",
            Self::Item => "items",
            Self::Status => "statuses",
            Self::Terrain => "terrains",
            Self::Effect => "effects",
            Self::Trainer => "trainers",
            Self::AiProfile => "trainers/profiles",
            Self::Encounter => "encounters",
            Self::Text => "text",
        }
    }

    /// `true` for battle data (skipped when battles are off).
    #[must_use]
    pub fn is_battle(self) -> bool {
        !matches!(self, Self::Item | Self::Terrain | Self::Text)
    }

    /// The content-database kind for the kinds plugins can register (§9.4).
    #[must_use]
    pub fn content_kind(self) -> Option<ContentKind> {
        Some(match self {
            Self::Move => ContentKind::Move,
            Self::Ability => ContentKind::Ability,
            Self::Item => ContentKind::Item,
            Self::Status => ContentKind::Status,
            Self::Terrain => ContentKind::Terrain,
            Self::Effect => ContentKind::Effect,
            _ => return None,
        })
    }
}

/// Every problem found while loading, in the order found.
#[derive(Debug, Default)]
pub struct DataErrors(pub Vec<DataError>);

impl DataErrors {
    /// The problems.
    #[must_use]
    pub fn errors(&self) -> &[DataError] {
        &self.0
    }

    /// Number of problems.
    #[must_use]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// `true` when there is none.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl fmt::Display for DataErrors {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (i, err) in self.0.iter().enumerate() {
            if i > 0 {
                f.write_str("\n")?;
            }
            write!(f, "{err}")?;
        }
        Ok(())
    }
}

impl std::error::Error for DataErrors {}

/// The frozen content of a project. Build it with [`GameData::load`] or a
/// [`GameDataBuilder`]; it cannot change afterwards.
#[derive(Clone, Debug, PartialEq)]
pub struct GameData {
    options: LoadOptions,
    type_chart: TypeChart,
    species: Table<SpeciesDef>,
    moves: Table<MoveDef>,
    abilities: Table<AbilityDef>,
    items: Table<ItemDef>,
    statuses: Table<StatusDef>,
    terrains: Table<TerrainDef>,
    effects: Table<EffectDef>,
    trainers: Table<TrainerDef>,
    ai_profiles: Table<AiProfile>,
    encounters: Table<EncounterTable>,
    encounter_tables: EncounterTables,
    text: TextDb,
}

impl GameData {
    /// Nothing loaded.
    #[must_use]
    pub fn empty(options: LoadOptions) -> Self {
        Self {
            options,
            type_chart: TypeChart::from_parts(Vec::new(), Default::default()),
            species: Table::new(),
            moves: Table::new(),
            abilities: Table::new(),
            items: Table::new(),
            statuses: Table::new(),
            terrains: Table::new(),
            effects: Table::new(),
            trainers: Table::new(),
            ai_profiles: Table::new(),
            encounters: Table::new(),
            encounter_tables: EncounterTables::new(),
            text: TextDb::new(),
        }
    }

    /// Loads the project's `data/` folder from `files`.
    pub fn load(files: &dyn DataFiles, options: LoadOptions) -> Result<Self, DataErrors> {
        let mut builder = GameDataBuilder::new(options);
        builder.load_dir(files, DATA_ROOT);
        builder.build()
    }

    /// The options it was loaded with.
    #[must_use]
    pub fn options(&self) -> LoadOptions {
        self.options
    }

    /// `true` when battle data was loaded.
    #[must_use]
    pub fn battles_enabled(&self) -> bool {
        self.options.battles
    }

    /// Type chart (empty without `data/types.ron`).
    #[must_use]
    pub fn type_chart(&self) -> &TypeChart {
        &self.type_chart
    }

    /// Species.
    #[must_use]
    pub fn species(&self) -> &Table<SpeciesDef> {
        &self.species
    }

    /// Moves.
    #[must_use]
    pub fn moves(&self) -> &Table<MoveDef> {
        &self.moves
    }

    /// Abilities.
    #[must_use]
    pub fn abilities(&self) -> &Table<AbilityDef> {
        &self.abilities
    }

    /// Items.
    #[must_use]
    pub fn items(&self) -> &Table<ItemDef> {
        &self.items
    }

    /// Statuses defined in data (the built-in Gen-3 ones live in `BattleRules`).
    #[must_use]
    pub fn statuses(&self) -> &Table<StatusDef> {
        &self.statuses
    }

    /// Custom terrains.
    #[must_use]
    pub fn terrains(&self) -> &Table<TerrainDef> {
        &self.terrains
    }

    /// Data effect primitives.
    #[must_use]
    pub fn effects(&self) -> &Table<EffectDef> {
        &self.effects
    }

    /// Trainers.
    #[must_use]
    pub fn trainers(&self) -> &Table<TrainerDef> {
        &self.trainers
    }

    /// AI profiles from data (trainers may also name the built-in ones).
    #[must_use]
    pub fn ai_profiles(&self) -> &Table<AiProfile> {
        &self.ai_profiles
    }

    /// Encounter tables.
    #[must_use]
    pub fn encounters(&self) -> &Table<EncounterTable> {
        &self.encounters
    }

    /// Encounter tables in the form the world's encounter rolls use.
    #[must_use]
    pub fn encounter_tables(&self) -> &EncounterTables {
        &self.encounter_tables
    }

    /// Localized text.
    #[must_use]
    pub fn text(&self) -> &TextDb {
        &self.text
    }

    /// Number of records per kind (type chart: types; text: keys over all languages).
    #[must_use]
    pub fn record_counts(&self) -> BTreeMap<DataKind, usize> {
        BTreeMap::from([
            (DataKind::TypeChart, self.type_chart.types().len()),
            (DataKind::Species, self.species.len()),
            (DataKind::Move, self.moves.len()),
            (DataKind::Ability, self.abilities.len()),
            (DataKind::Item, self.items.len()),
            (DataKind::Status, self.statuses.len()),
            (DataKind::Terrain, self.terrains.len()),
            (DataKind::Effect, self.effects.len()),
            (DataKind::Trainer, self.trainers.len()),
            (DataKind::AiProfile, self.ai_profiles.len()),
            (DataKind::Encounter, self.encounters.len()),
            (
                DataKind::Text,
                self.text.languages().map(|l| self.text.len(l)).sum(),
            ),
        ])
    }

    /// Type chart, moves and species as the battle core's content bundle.
    #[must_use]
    pub fn battle_pack(&self) -> BattlePack {
        let mut pack = BattlePack::new(self.type_chart.clone());
        pack.moves = self
            .moves
            .records()
            .iter()
            .map(|m| (m.id.clone(), m.clone()))
            .collect();
        pack.species = self
            .species
            .records()
            .iter()
            .map(|s| (s.id.clone(), s.clone()))
            .collect();
        pack
    }
}

/// Builds a [`GameData`]: load folders, merge the plugin content database, then
/// [`build`](Self::build).
#[derive(Debug)]
pub struct GameDataBuilder {
    data: GameData,
    errors: Vec<DataError>,
}

impl GameDataBuilder {
    /// Nothing loaded yet.
    #[must_use]
    pub fn new(options: LoadOptions) -> Self {
        Self {
            data: GameData::empty(options),
            errors: Vec::new(),
        }
    }

    /// Loads every kind below `root` (normally [`DATA_ROOT`]). Later folders may replace
    /// records only with `override: true`.
    pub fn load_dir(&mut self, files: &dyn DataFiles, root: &str) -> &mut Self {
        for &kind in DataKind::ALL {
            if kind.is_battle() && !self.data.options.battles {
                continue;
            }
            let path = format!("{root}/{}", kind.path());
            match kind {
                DataKind::TypeChart => {
                    if files.exists(&path) {
                        self.read_and_add(files, kind, &path);
                    }
                }
                DataKind::Text => {
                    for lang in sorted(files.list_dirs(&path)) {
                        let dir = format!("{path}/{lang}");
                        for name in ron_files(files, &dir) {
                            let file = format!("{dir}/{name}");
                            match files.read_text(&file) {
                                Ok(source) => {
                                    self.add_text(&lang, &file, &source);
                                }
                                Err(message) => self.errors.push(DataError::Read { file, message }),
                            }
                        }
                    }
                }
                _ => {
                    for name in ron_files(files, &path) {
                        self.read_and_add(files, kind, &format!("{path}/{name}"));
                    }
                }
            }
        }
        self
    }

    fn read_and_add(&mut self, files: &dyn DataFiles, kind: DataKind, file: &str) {
        match files.read_text(file) {
            Ok(source) => {
                self.add_file(kind, file, &source);
            }
            Err(message) => self.errors.push(DataError::Read {
                file: file.into(),
                message,
            }),
        }
    }

    /// Parses one file of `kind` (`file` names it in errors) and adds its records. Battle
    /// kinds are ignored when battles are off. Use [`add_text`](Self::add_text) for text.
    pub fn add_file(&mut self, kind: DataKind, file: &str, source: &str) -> &mut Self {
        if kind.is_battle() && !self.data.options.battles {
            return self;
        }
        let data = &mut self.data;
        let errors = &mut self.errors;
        match kind {
            DataKind::TypeChart => match TypeChart::parse_ron(source, file) {
                Ok(chart) => data.type_chart = chart,
                Err(e) => errors.push(e),
            },
            DataKind::Species => add_records(&mut data.species, source, file, errors, |s| {
                s.validate(file)
            }),
            DataKind::Trainer => add_records(&mut data.trainers, source, file, errors, |t| {
                t.validate(file)
            }),
            DataKind::AiProfile => add_records(&mut data.ai_profiles, source, file, errors, |p| {
                p.validate(file)
            }),
            DataKind::Encounter => {
                add_records(
                    &mut data.encounters,
                    source,
                    file,
                    errors,
                    EncounterTable::validate,
                );
            }
            DataKind::Text => {
                let lang = file.rsplit('/').nth(1).unwrap_or_default().to_owned();
                self.add_text(&lang, file, source);
            }
            DataKind::Move
            | DataKind::Ability
            | DataKind::Item
            | DataKind::Status
            | DataKind::Terrain
            | DataKind::Effect => {
                let Some(content_kind) = kind.content_kind() else {
                    return self;
                };
                match parse_content(content_kind, source, file) {
                    Ok(records) => {
                        for record in records {
                            let replace = record.override_existing;
                            if let Err(e) = data.insert_content(record.def, file, replace) {
                                errors.push(in_file(file, e));
                            }
                        }
                    }
                    Err(e) => errors.push(e),
                }
            }
        }
        self
    }

    /// Parses one text file of language `lang` and merges it.
    pub fn add_text(&mut self, lang: &str, file: &str, source: &str) -> &mut Self {
        if let Err(e) = self.data.text.insert_ron(lang, source, file) {
            self.errors.push(in_file(file, e));
        }
        self
    }

    /// Merges the content database the plugins registered into (moves, abilities, items,
    /// statuses, terrains, effects; §9.4): its records are added, and replace loaded ones
    /// with the same id (the database already resolved duplicates and overrides).
    pub fn merge_content(&mut self, db: &ContentDb) -> &mut Self {
        for &kind in ContentKind::ALL {
            for id in db.ids(kind) {
                let Some(entry) = db.get(kind, id) else {
                    continue;
                };
                let label = entry.source.to_string();
                if let Err(e) = self.data.merge_content_def(&entry.def, &label) {
                    self.errors.push(e);
                }
            }
        }
        self
    }

    /// Freezes the content, or returns every problem found.
    pub fn build(self) -> Result<GameData, DataErrors> {
        let (data, errors) = self.finish();
        if errors.is_empty() {
            Ok(data)
        } else {
            Err(DataErrors(errors))
        }
    }

    /// Freezes whatever loaded and returns it together with every problem found. Records
    /// that failed are left out. Validation (§8.2) uses it to check the cross-references of
    /// the rest of the content in the same run.
    #[must_use]
    pub fn finish(self) -> (GameData, Vec<DataError>) {
        let Self {
            mut data,
            mut errors,
        } = self;
        let mut tables = EncounterTables::new();
        for table in data.encounters.records() {
            if let Err(e) = tables.insert(table.clone()) {
                errors.push(e);
            }
        }
        data.encounter_tables = tables;
        (data, errors)
    }
}

impl GameData {
    fn insert_content(&mut self, def: ContentDef, file: &str, replace: bool) -> crate::Result<()> {
        if !is_content_id(def.id()) {
            return Err(DataError::InvalidField {
                field: "id".into(),
                context: format!("{} `{}`", def.kind(), def.id()),
                message: "must be snake_case or `plugin_id:snake_case`".into(),
            });
        }
        def.validate()?;
        let battles = self.options.battles;
        match def {
            ContentDef::Move(d) if battles => self.moves.insert(d, file, replace).map(drop),
            ContentDef::Ability(d) if battles => self.abilities.insert(d, file, replace).map(drop),
            ContentDef::Status(d) if battles => self.statuses.insert(d, file, replace).map(drop),
            ContentDef::Effect(d) if battles => self.effects.insert(d, file, replace).map(drop),
            ContentDef::Item(d) => self.items.insert(d, file, replace).map(drop),
            ContentDef::Terrain(d) => self.terrains.insert(d, file, replace).map(drop),
            ContentDef::Move(_)
            | ContentDef::Ability(_)
            | ContentDef::Status(_)
            | ContentDef::Effect(_) => Ok(()),
        }
    }

    /// Adds or replaces `def` unless the same record is already there.
    fn merge_content_def(&mut self, def: &ContentDef, label: &str) -> crate::Result<()> {
        let unchanged = match def {
            ContentDef::Move(d) => self.moves.by_id(&d.id) == Some(d),
            ContentDef::Ability(d) => self.abilities.by_id(&d.id) == Some(d),
            ContentDef::Item(d) => self.items.by_id(&d.id) == Some(d),
            ContentDef::Status(d) => self.statuses.by_id(&d.id) == Some(d),
            ContentDef::Terrain(d) => self.terrains.by_id(&d.id) == Some(d),
            ContentDef::Effect(d) => self.effects.by_id(&d.id) == Some(d),
        };
        if unchanged {
            return Ok(());
        }
        self.insert_content(def.clone(), label, true)
    }
}

/// Decodes the records of `file` into `table`, running `check` on each.
fn add_records<T: Keyed + DeserializeOwned>(
    table: &mut Table<T>,
    source: &str,
    file: &str,
    errors: &mut Vec<DataError>,
    check: impl Fn(&T) -> crate::Result<()>,
) {
    let records = match RonCodec::decode_records::<T>(source, file) {
        Ok(records) => records,
        Err(e) => {
            errors.push(e);
            return;
        }
    };
    for record in records {
        let id = record.value.key();
        if !is_content_id(id) {
            errors.push(in_file(
                file,
                DataError::InvalidField {
                    field: "id".into(),
                    context: format!("{} `{id}`", T::KIND),
                    message: "must be snake_case or `plugin_id:snake_case`".into(),
                },
            ));
            continue;
        }
        if let Err(e) = check(&record.value) {
            errors.push(in_file(file, e));
            continue;
        }
        if let Err(e) = table.insert(record.value, file, record.override_existing) {
            errors.push(e);
        }
    }
}

/// Adds the file to an error that does not name it yet.
fn in_file(file: &str, err: DataError) -> DataError {
    if err.to_string().contains(file) {
        err
    } else {
        DataError::InFile {
            file: file.into(),
            source: Box::new(err),
        }
    }
}

fn sorted(mut names: Vec<String>) -> Vec<String> {
    names.sort();
    names.dedup();
    names
}

/// `*.ron` files directly in `dir`, in name order.
fn ron_files(files: &dyn DataFiles, dir: &str) -> Vec<String> {
    sorted(
        files
            .list_files(dir)
            .into_iter()
            .filter(|n| n.ends_with(".ron"))
            .collect(),
    )
}

#[cfg(test)]
mod tests;
