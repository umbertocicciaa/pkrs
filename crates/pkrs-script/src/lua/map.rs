//! The `map` module (§9.1, §6.4, §6.5).
//!
//! `map.current()`, `map.warp(map, x, y, facing?)`, `map.tile(x, y)` (→ `{ solid, terrain }`
//! or `nil` off the map), `map.npc(id)` (→ an `npc` object or `nil`), and the map-local,
//! persisted `map.var(name)` / `map.set_var(name, value)`.

use mlua::{Lua, Table, Value};

use super::facade::{check, name, opt_direction, pos, storable, with_world};
use super::{convert, npc};

/// Installs `map` into `env`.
pub(super) fn install(lua: &Lua, env: &Table) -> mlua::Result<()> {
    let map = lua.create_table()?;
    map.raw_set(
        "current",
        lua.create_function(|lua, ()| with_world(lua, "map.current", |w| w.current_map()))?,
    )?;
    map.raw_set(
        "warp",
        lua.create_function(
            |lua, (target, x, y, facing): (Value, Value, Value, Value)| {
                let target = check(lua, name("map.warp", "map id", &target))?;
                let to = check(lua, pos("map.warp", &x, &y))?;
                let facing = check(lua, opt_direction("map.warp", "facing", &facing))?;
                let result = with_world(lua, "map.warp", |w| w.warp(&target, to, facing))?;
                check(lua, result.map_err(|e| format!("map.warp: {e}")))
            },
        )?,
    )?;
    map.raw_set(
        "tile",
        lua.create_function(|lua, (x, y): (Value, Value)| {
            let at = check(lua, pos("map.tile", &x, &y))?;
            let Some(info) = with_world(lua, "map.tile", |w| w.tile(at))? else {
                return Ok(Value::Nil);
            };
            let t = lua.create_table()?;
            t.raw_set("solid", info.solid)?;
            t.raw_set("terrain", info.terrain)?;
            Ok(Value::Table(t))
        })?,
    )?;
    map.raw_set(
        "npc",
        lua.create_function(|lua, id: Value| {
            let id = check(lua, name("map.npc", "npc id", &id))?;
            if with_world(lua, "map.npc", |w| w.npc_exists(&id))? {
                Ok(Value::Table(npc::handle(lua, &id)?))
            } else {
                Ok(Value::Nil)
            }
        })?,
    )?;
    map.raw_set(
        "var",
        lua.create_function(|lua, var: Value| {
            let var = check(lua, name("map.var", "variable name", &var))?;
            let value = with_world(lua, "map.var", |w| w.map_var(&var))?;
            convert::to_lua(lua, &value)
        })?,
    )?;
    map.raw_set(
        "set_var",
        lua.create_function(|lua, (var, value): (Value, Value)| {
            let var = check(lua, name("map.set_var", "variable name", &var))?;
            let value = check(lua, storable("map.set_var", &value))?;
            with_world(lua, "map.set_var", |w| w.set_map_var(&var, value))
        })?,
    )?;
    env.raw_set("map", map)
}
