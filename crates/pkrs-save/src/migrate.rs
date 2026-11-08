//! Versioned save migrations (§12): engine steps `v1 → v2 → …` on the [`Value`] tree, then an
//! optional game-level migration (`scripts/migrate.lua`), then typed deserialization.
//!
//! # Format history
//!
//! - **v1**: one game RNG stream in `world.rng` (`{ seed, steps }`); play time in
//!   `header.play_time`.
//! - **v2** (current): named RNG streams in `world.rng` (`{ "battle": { seed, steps }, … }`,
//!   [`RNG_BATTLE`], …); play time in `header.play_time_ticks`. The v1 stream becomes the
//!   battle stream, since battles derived their seeds from the game RNG.

use std::collections::BTreeMap;

use crate::codec::RonCodec;
use crate::error::{Result, SaveError};
use crate::model::{CURRENT_FORMAT_VERSION, RNG_BATTLE, SaveData, SaveHeader};
use crate::serde_value::from_value;
use crate::value::Value;

/// One engine migration step: rewrites a save tree of version `n` into version `n + 1`. The
/// registry updates `header.format_version` after the step.
pub type MigrationFn = fn(Value) -> Result<Value>;

/// Engine migration steps by the version they migrate **from** (§12).
#[derive(Clone, Debug)]
pub struct Migrations {
    steps: BTreeMap<u32, MigrationFn>,
    current: u32,
}

impl Default for Migrations {
    fn default() -> Self {
        Self::engine()
    }
}

impl Migrations {
    /// No steps; saves must already be at version `current`.
    #[must_use]
    pub fn empty(current: u32) -> Self {
        Self {
            steps: BTreeMap::new(),
            current,
        }
    }

    /// The engine's own steps, up to [`CURRENT_FORMAT_VERSION`].
    #[must_use]
    pub fn engine() -> Self {
        let mut migrations = Self::empty(CURRENT_FORMAT_VERSION);
        migrations.register(1, v1_to_v2);
        migrations
    }

    /// Registers the step from version `from` to `from + 1`, replacing any earlier one.
    pub fn register(&mut self, from: u32, step: MigrationFn) {
        self.steps.insert(from, step);
    }

    /// The version saves are migrated to.
    #[must_use]
    pub fn current(&self) -> u32 {
        self.current
    }

    /// Runs every step from the save's `header.format_version` up to
    /// [`current`](Self::current). Returns the migrated tree and the version it started at.
    pub fn migrate(&self, mut save: Value) -> Result<(Value, u32)> {
        let from = format_version(&save)?;
        if from == 0 || from > self.current {
            return Err(SaveError::UnsupportedVersion(from));
        }
        let mut version = from;
        while version < self.current {
            let step = self
                .steps
                .get(&version)
                .ok_or(SaveError::MissingMigration(version))?;
            save = step(save).map_err(|e| match e {
                SaveError::Migration { .. } => e,
                other => SaveError::Migration {
                    from: version,
                    to: version + 1,
                    message: other.to_string(),
                },
            })?;
            version += 1;
            let header = save
                .get_mut("header")
                .and_then(Value::as_map_mut)
                .ok_or_else(|| step_error(version - 1, "the step removed the header"))?;
            header.insert("format_version".into(), Value::Int(version.into()));
            tracing::info!(target: "save", from = version - 1, to = version, "save migrated");
        }
        Ok((save, from))
    }
}

/// `header.format_version` of a save tree.
pub fn format_version(save: &Value) -> Result<u32> {
    save.pointer("header.format_version")
        .and_then(Value::as_int)
        .and_then(|v| u32::try_from(v).ok())
        .ok_or(SaveError::MissingVersion)
}

fn step_error(from: u32, message: impl Into<String>) -> SaveError {
    SaveError::Migration {
        from,
        to: from + 1,
        message: message.into(),
    }
}

/// v1 → v2: one game RNG stream → named streams; `play_time` → `play_time_ticks`.
fn v1_to_v2(mut save: Value) -> Result<Value> {
    let header = save
        .get_mut("header")
        .ok_or_else(|| step_error(1, "no header"))?;
    if let Some(play_time) = header.remove("play_time") {
        header.insert("play_time_ticks", play_time);
    }
    if let Some(world) = save.get_mut("world") {
        if let Some(rng) = world.remove("rng") {
            if rng.get("seed").is_none() {
                return Err(step_error(1, "`world.rng` is not a v1 RNG state"));
            }
            world.insert(
                "rng",
                Value::Map(BTreeMap::from([(RNG_BATTLE.to_owned(), rng)])),
            );
        }
    }
    Ok(save)
}

/// A game-level save migration (§12): `scripts/migrate.lua`'s `migrate(save, from_game_version)`
/// in the v1 runtime (`pkrs_script::ScriptSaveMigration`). It runs after the engine steps, on a
/// tree that is already at [`CURRENT_FORMAT_VERSION`], typically to rename flags and variables.
pub trait GameMigration {
    /// Rewrites `save`, written by game version `from_game_version`.
    fn migrate(&mut self, save: Value, from_game_version: &str) -> Result<Value>;
}

/// Decodes save text: RON → [`Value`] → engine migrations → game migration → [`SaveData`].
pub struct SaveLoader<'a> {
    migrations: Migrations,
    game: Option<(&'a mut dyn GameMigration, String)>,
}

impl Default for SaveLoader<'_> {
    fn default() -> Self {
        Self::new()
    }
}

impl<'a> SaveLoader<'a> {
    /// The engine's migrations, no game migration.
    #[must_use]
    pub fn new() -> Self {
        Self {
            migrations: Migrations::engine(),
            game: None,
        }
    }

    /// Uses `migrations` instead of the engine's.
    #[must_use]
    pub fn with_migrations(mut self, migrations: Migrations) -> Self {
        self.migrations = migrations;
        self
    }

    /// Runs `migration` on saves whose `header.game_version` differs from `game_version` (the
    /// running game's version); the migrated save then carries `game_version`.
    #[must_use]
    pub fn with_game_migration(
        mut self,
        migration: &'a mut dyn GameMigration,
        game_version: impl Into<String>,
    ) -> Self {
        self.game = Some((migration, game_version.into()));
        self
    }

    /// The engine migrations in use.
    #[must_use]
    pub fn migrations(&self) -> &Migrations {
        &self.migrations
    }

    /// Decodes a whole save. `file` names the source in errors.
    pub fn decode(&mut self, text: &str, file: &str) -> Result<SaveData> {
        let value = RonCodec::decode(text, file)?;
        self.load_value(value)
    }

    /// Migrates and decodes a save tree.
    pub fn load_value(&mut self, value: Value) -> Result<SaveData> {
        let (mut value, _) = self.migrations.migrate(value)?;
        if let Some((migration, current)) = self.game.as_mut() {
            let from = value
                .pointer("header.game_version")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned();
            if from != *current {
                value = migration.migrate(value, &from)?;
                if !matches!(value, Value::Map(_)) {
                    return Err(SaveError::GameMigration(format!(
                        "migrate() returned a {}, not a save table",
                        value.kind()
                    )));
                }
                if format_version(&value)? != self.migrations.current() {
                    return Err(SaveError::GameMigration(
                        "migrate() must not change `header.format_version`".into(),
                    ));
                }
                if let Some(header) = value.get_mut("header") {
                    header.insert("game_version", Value::String(current.clone()));
                }
                tracing::info!(target: "save", from = %from, to = %current, "game save migration ran");
            }
        }
        SaveData::from_value(value)
    }

    /// Decodes only the header (after engine migrations), for the slot screen (§12).
    pub fn decode_header(&self, text: &str, file: &str) -> Result<SaveHeader> {
        let value = RonCodec::decode(text, file)?;
        let (mut value, _) = self.migrations.migrate(value)?;
        let header = value.remove("header").ok_or(SaveError::MissingVersion)?;
        from_value(header)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v1_tree() -> Value {
        let text = r#"{
            "header": { "format_version": 1, "game_id": "com.example.t", "game_version": "0.1.0", "play_time": 42 },
            "world": { "money": 5, "rng": { "seed": 9, "steps": 3 } },
            "plugins": {},
        }"#;
        RonCodec::decode(text, "v1").unwrap()
    }

    #[test]
    fn v1_to_v2_names_the_rng_stream_and_renames_play_time() {
        let (v2, from) = Migrations::engine().migrate(v1_tree()).unwrap();
        assert_eq!(from, 1);
        assert_eq!(format_version(&v2).unwrap(), 2);
        assert_eq!(v2.pointer("header.play_time_ticks"), Some(&Value::Int(42)));
        assert_eq!(v2.pointer("header.play_time"), None);
        assert_eq!(v2.pointer("world.rng.battle.seed"), Some(&Value::Int(9)));
        let save = SaveData::from_value(v2).unwrap();
        assert_eq!(save.header.play_time_ticks, 42);
        assert_eq!(save.world.rng[RNG_BATTLE].steps, 3);
        assert_eq!(save.world.money, 5);
    }

    #[test]
    fn v1_to_v2_rejects_a_malformed_rng() {
        let mut tree = v1_tree();
        tree.pointer_mut("world")
            .unwrap()
            .insert("rng", Value::Int(3));
        let err = Migrations::engine().migrate(tree).unwrap_err();
        assert!(
            matches!(err, SaveError::Migration { from: 1, to: 2, .. }),
            "{err}"
        );
    }

    #[test]
    fn current_saves_are_not_migrated() {
        let tree = SaveData::default().to_value().unwrap();
        let (same, from) = Migrations::engine().migrate(tree.clone()).unwrap();
        assert_eq!(from, CURRENT_FORMAT_VERSION);
        assert_eq!(same, tree);
    }

    #[test]
    fn unknown_versions_and_gaps_are_errors() {
        let mut tree = SaveData::default().to_value().unwrap();
        tree.pointer_mut("header")
            .unwrap()
            .insert("format_version", Value::Int(99));
        assert!(matches!(
            Migrations::engine().migrate(tree.clone()),
            Err(SaveError::UnsupportedVersion(99))
        ));
        tree.pointer_mut("header")
            .unwrap()
            .insert("format_version", Value::Int(0));
        assert!(matches!(
            Migrations::engine().migrate(tree.clone()),
            Err(SaveError::UnsupportedVersion(0))
        ));
        tree.pointer_mut("header")
            .unwrap()
            .insert("format_version", Value::Int(1));
        assert!(matches!(
            Migrations::empty(3).migrate(tree.clone()),
            Err(SaveError::MissingMigration(1))
        ));
        tree.remove("header");
        assert!(matches!(
            Migrations::engine().migrate(tree),
            Err(SaveError::MissingVersion)
        ));
    }

    #[test]
    fn steps_run_in_sequence() {
        fn add_a(mut v: Value) -> Result<Value> {
            v.insert("trail", Value::from("a"));
            Ok(v)
        }
        fn add_b(mut v: Value) -> Result<Value> {
            let trail = v
                .get("trail")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_owned();
            v.insert("trail", Value::from(format!("{trail}b")));
            Ok(v)
        }
        let mut migrations = Migrations::empty(3);
        migrations.register(1, add_a);
        migrations.register(2, add_b);
        let mut tree = Value::map();
        tree.insert("header", Value::map());
        tree.pointer_mut("header")
            .unwrap()
            .insert("format_version", Value::Int(1));
        let (out, _) = migrations.migrate(tree).unwrap();
        assert_eq!(out.get("trail"), Some(&Value::from("ab")));
        assert_eq!(format_version(&out).unwrap(), 3);
    }

    struct RenameFlag;

    impl GameMigration for RenameFlag {
        fn migrate(&mut self, mut save: Value, from: &str) -> Result<Value> {
            assert_eq!(from, "0.1.0");
            let flags = save.pointer_mut("world.flags").unwrap();
            let old = flags.remove("old").unwrap();
            flags.insert("new", old);
            Ok(save)
        }
    }

    #[test]
    fn game_migration_runs_after_engine_steps_when_the_game_version_changed() {
        let mut tree = v1_tree();
        tree.pointer_mut("world").unwrap().insert(
            "flags",
            Value::Map(BTreeMap::from([("old".into(), Value::Bool(true))])),
        );
        let mut game = RenameFlag;
        let save = SaveLoader::new()
            .with_game_migration(&mut game, "0.2.0")
            .load_value(tree.clone())
            .unwrap();
        assert!(save.world.flag("new"));
        assert!(!save.world.flag("old"));
        assert_eq!(save.header.game_version, "0.2.0");

        // Same game version: the game migration does not run.
        let mut game = RenameFlag;
        let save = SaveLoader::new()
            .with_game_migration(&mut game, "0.1.0")
            .load_value(tree)
            .unwrap();
        assert!(save.world.flag("old"));
    }

    struct Broken;

    impl GameMigration for Broken {
        fn migrate(&mut self, _save: Value, _from: &str) -> Result<Value> {
            Ok(Value::Int(1))
        }
    }

    #[test]
    fn game_migration_must_return_a_save() {
        let mut game = Broken;
        let err = SaveLoader::new()
            .with_game_migration(&mut game, "9.9.9")
            .load_value(v1_tree())
            .unwrap_err();
        assert!(matches!(err, SaveError::GameMigration(_)), "{err}");
    }

    #[test]
    fn header_only_decoding_migrates_old_headers() {
        let text = RonCodec::encode(&v1_tree()).unwrap();
        let header = SaveLoader::new().decode_header(&text, "v1").unwrap();
        assert_eq!(header.format_version, CURRENT_FORMAT_VERSION);
        assert_eq!(header.play_time_ticks, 42);
        assert_eq!(header.game_id, "com.example.t");
    }
}
