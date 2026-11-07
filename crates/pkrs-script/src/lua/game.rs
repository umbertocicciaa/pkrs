//! The `game` module (§9.1, §6.5): flags, variables, bag and money.
//!
//! `game.flag(name)`, `game.set_flag(name, value = true)`, `game.var(name)`,
//! `game.set_var(name, value)`, `game.give_item(id, count = 1)`, `game.bag()`, `game.money()`.

use mlua::{Lua, Table, Value};

use super::convert;
use super::facade::{check, name, opt_int, storable, with_world};

/// Installs `game` into `env`.
pub(super) fn install(lua: &Lua, env: &Table) -> mlua::Result<()> {
    let game = lua.create_table()?;
    game.raw_set(
        "flag",
        lua.create_function(|lua, flag: Value| {
            let flag = check(lua, name("game.flag", "flag name", &flag))?;
            with_world(lua, "game.flag", |w| w.flag(&flag))
        })?,
    )?;
    game.raw_set(
        "set_flag",
        lua.create_function(|lua, (flag, value): (Value, Value)| {
            let flag = check(lua, name("game.set_flag", "flag name", &flag))?;
            let value = match value {
                Value::Nil => true,
                Value::Boolean(b) => b,
                other => {
                    return Err(super::facade::fail(
                        lua,
                        format!(
                            "game.set_flag: value must be a boolean (got {})",
                            super::facade::type_of(&other)
                        ),
                    ));
                }
            };
            with_world(lua, "game.set_flag", |w| w.set_flag(&flag, value))
        })?,
    )?;
    game.raw_set(
        "var",
        lua.create_function(|lua, var: Value| {
            let var = check(lua, name("game.var", "variable name", &var))?;
            let value = with_world(lua, "game.var", |w| w.var(&var))?;
            convert::to_lua(lua, &value)
        })?,
    )?;
    game.raw_set(
        "set_var",
        lua.create_function(|lua, (var, value): (Value, Value)| {
            let var = check(lua, name("game.set_var", "variable name", &var))?;
            let value = check(lua, storable("game.set_var", &value))?;
            with_world(lua, "game.set_var", |w| w.set_var(&var, value))
        })?,
    )?;
    game.raw_set(
        "give_item",
        lua.create_function(|lua, (item, count): (Value, Value)| {
            let item = check(lua, name("game.give_item", "item id", &item))?;
            let count = check(
                lua,
                opt_int("game.give_item", "count", &count, 1, 1, i64::from(u32::MAX)),
            )?;
            let count = u32::try_from(count).unwrap_or(u32::MAX);
            let result = with_world(lua, "game.give_item", |w| w.give_item(&item, count))?;
            check(lua, result.map_err(|e| format!("game.give_item: {e}")))
        })?,
    )?;
    game.raw_set(
        "bag",
        lua.create_function(|lua, ()| {
            let items = with_world(lua, "game.bag", |w| w.bag())?;
            let t = lua.create_table()?;
            for (item, count) in items {
                t.raw_set(item, count)?;
            }
            Ok(t)
        })?,
    )?;
    game.raw_set(
        "money",
        lua.create_function(|lua, ()| with_world(lua, "game.money", |w| w.money()))?,
    )?;
    env.raw_set("game", game)
}
