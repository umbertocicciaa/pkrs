//! The `player` module (§9.1): `player.pos()` (→ `x, y, facing`), `player.face(dir)`,
//! `player.lock()`, `player.unlock()` and `player.set_field_state(name, value)`.
//!
//! `lock`/`unlock` drive a script-held lock; interact and trigger tasks additionally hold the
//! scheduler's automatic lock while they run (§9.3).

use mlua::{Lua, Table, Value};

use super::facade::{check, direction, fail, name, type_of, with_world};

/// Installs `player` into `env`.
pub(super) fn install(lua: &Lua, env: &Table) -> mlua::Result<()> {
    let player = lua.create_table()?;
    player.raw_set(
        "pos",
        lua.create_function(|lua, ()| {
            let (pos, facing) = with_world(lua, "player.pos", |w| w.player_pos())?;
            Ok((pos.x, pos.y, facing.to_string()))
        })?,
    )?;
    player.raw_set(
        "face",
        lua.create_function(|lua, dir: Value| {
            let dir = check(lua, direction("player.face", "direction", &dir))?;
            with_world(lua, "player.face", |w| w.player_face(dir))
        })?,
    )?;
    player.raw_set(
        "lock",
        lua.create_function(|lua, ()| {
            with_world(lua, "player.lock", |w| w.set_player_locked(true))
        })?,
    )?;
    player.raw_set(
        "unlock",
        lua.create_function(|lua, ()| {
            with_world(lua, "player.unlock", |w| w.set_player_locked(false))
        })?,
    )?;
    player.raw_set(
        "set_field_state",
        lua.create_function(|lua, (state, value): (Value, Value)| {
            const FUNC: &str = "player.set_field_state";
            let state = check(lua, name(FUNC, "field state name", &state))?;
            let Value::Boolean(value) = value else {
                return Err(fail(
                    lua,
                    format!("{FUNC}: value must be a boolean (got {})", type_of(&value)),
                ));
            };
            let r = with_world(lua, FUNC, |w| w.set_field_state(&state, value))?;
            check(lua, r.map_err(|e| format!("{FUNC}: {e}")))
        })?,
    )?;
    env.raw_set("player", player)
}
