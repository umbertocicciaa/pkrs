//! [`ScriptSaveMigration`]: the game-level save migration `scripts/migrate.lua` (§12).
//!
//! The script defines a global `migrate(save, from_game_version)`. It receives the whole save
//! as a table (already migrated to the current engine format) and the `game.version` that wrote
//! it, and returns the migrated table, typically after renaming flags and variables:
//!
//! ```lua
//! function migrate(save, from)
//!   local flags = save.world.flags
//!   if flags.old_name ~= nil then
//!     flags.new_name, flags.old_name = flags.old_name, nil
//!   end
//!   return save
//! end
//! ```
//!
//! It runs in the normal sandbox (instruction budget, memory limit, no I/O, §9.6). Returning
//! `nil` keeps the save unchanged.

use pkrs_save::{GameMigration, SaveError, Value};

use crate::host::{ScriptHost, ScriptOwner, ScriptValue};

/// Path of the game's save migration script.
pub const MIGRATE_SCRIPT: &str = "scripts/migrate.lua";

/// Name of the migration function `scripts/migrate.lua` defines.
pub const MIGRATE_FUNCTION: &str = "migrate";

/// [`GameMigration`] that calls `migrate(save, from_game_version)` from
/// [`MIGRATE_SCRIPT`] in its own script host (one dedicated to migrations, so the game's
/// environment is not touched).
pub struct ScriptSaveMigration<H: ScriptHost> {
    host: H,
    loaded: bool,
}

impl<H: ScriptHost> ScriptSaveMigration<H> {
    /// Uses `host`; [`MIGRATE_SCRIPT`] runs on the first migration.
    pub fn new(host: H) -> Self {
        Self {
            host,
            loaded: false,
        }
    }

    /// The script host.
    pub fn host(&self) -> &H {
        &self.host
    }
}

fn failed(err: impl std::fmt::Display) -> SaveError {
    SaveError::GameMigration(format!("{MIGRATE_SCRIPT}: {err}"))
}

impl<H: ScriptHost> GameMigration for ScriptSaveMigration<H> {
    fn migrate(&mut self, save: Value, from_game_version: &str) -> pkrs_save::Result<Value> {
        let owner = ScriptOwner::Game;
        if !self.loaded {
            self.host.run_file(&owner, MIGRATE_SCRIPT).map_err(failed)?;
            self.loaded = true;
        }
        if !self.host.has_function(&owner, MIGRATE_FUNCTION) {
            return Err(failed(format!(
                "does not define `{MIGRATE_FUNCTION}(save, from_game_version)`"
            )));
        }
        let args = [
            ScriptValue::from_save_value(Some(&save)),
            ScriptValue::String(from_game_version.to_owned()),
        ];
        let result = self
            .host
            .call_function(&owner, MIGRATE_FUNCTION, &args)
            .map_err(failed)?;
        match result {
            ScriptValue::Nil => Ok(save),
            other @ ScriptValue::Map(_) => other
                .to_save_value()
                .ok_or_else(|| failed("migrate() returned a value with no saved form")),
            other => Err(failed(format!(
                "migrate() must return the save table, got {other:?}"
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use pkrs_core::{Pcg64Rng, RngState};
    use pkrs_save::{RNG_BATTLE, SaveData, SaveLoader};

    use super::*;
    use crate::host::{MemoryScripts, ScriptLimits};
    use crate::lua::LuaScriptHost;

    fn host(source: &str) -> LuaScriptHost {
        let files = MemoryScripts::new().with(MIGRATE_SCRIPT, source);
        LuaScriptHost::new(
            Box::new(files),
            Box::new(Pcg64Rng::from_seed(5)),
            ScriptLimits {
                instruction_budget: 100_000,
                ..ScriptLimits::default()
            },
        )
        .unwrap()
    }

    fn old_save() -> Value {
        let mut save = SaveData::default();
        save.header.game_version = "0.1.0".into();
        save.world.set_flag("old_name", true);
        save.world.set_var("score", Some(Value::Float(1.5)));
        save.world.map_mut("town").fire_once("intro");
        save.world.rng.insert(
            RNG_BATTLE.into(),
            RngState {
                seed: u64::MAX,
                steps: 2,
            },
        );
        save.plugins.insert(
            "p".into(),
            Value::Map(BTreeMap::from([("list".into(), Value::List(Vec::new()))])),
        );
        save.to_value().unwrap()
    }

    #[test]
    fn migrate_lua_renames_flags_and_keeps_everything_else() {
        let mut lua = ScriptSaveMigration::new(host(
            r#"
            function migrate(save, from)
              assert(from == "0.1.0")
              local flags = save.world.flags
              flags.new_name, flags.old_name = flags.old_name, nil
              save.world.vars.migrated_from = from
              return save
            end
            "#,
        ));
        let save = SaveLoader::new()
            .with_game_migration(&mut lua, "0.2.0")
            .load_value(old_save())
            .unwrap();
        assert!(save.world.flag("new_name"));
        assert!(!save.world.flags().contains_key("old_name"));
        assert_eq!(save.world.var("migrated_from"), Some(&Value::from("0.1.0")));
        assert_eq!(save.world.var("score"), Some(&Value::Float(1.5)));
        assert!(save.world.map("town").unwrap().has_fired("intro"));
        assert_eq!(save.world.rng[RNG_BATTLE].seed, u64::MAX);
        assert_eq!(save.header.game_version, "0.2.0");
    }

    #[test]
    fn returning_nil_keeps_the_save() {
        let mut lua = ScriptSaveMigration::new(host("function migrate(save, from) end"));
        let out = lua.migrate(old_save(), "0.1.0").unwrap();
        assert_eq!(out, old_save());
    }

    #[test]
    fn script_errors_and_bad_results_are_migration_errors() {
        for source in [
            "function migrate(save, from) error('boom') end",
            "function migrate(save, from) return 3 end",
            "local x = 1",
            "this is not lua",
            "function migrate(save, from) while true do end end",
        ] {
            let mut lua = ScriptSaveMigration::new(host(source));
            let err = lua.migrate(old_save(), "0.1.0").unwrap_err();
            assert!(
                matches!(err, SaveError::GameMigration(_)),
                "{source}: {err}"
            );
        }
    }

    #[test]
    fn script_rng_state_round_trips() {
        let mut host = host("");
        assert_eq!(host.rng_state(), Some(RngState { seed: 5, steps: 0 }));
        host.restore_rng_state(RngState { seed: 9, steps: 4 });
        assert_eq!(host.rng_state(), Some(RngState { seed: 9, steps: 4 }));
    }
}
