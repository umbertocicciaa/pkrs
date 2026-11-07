//! `npc` objects (§9.1): handles to the NPCs of the active map.
//!
//! An `npc` is a table `{ id = "keeper" }` sharing one sealed metatable whose methods call the
//! [`ScriptWorld`](crate::ScriptWorld) port: `npc:move(path)` (blocks until the walk ends),
//! `npc:face(dir)`, `npc:face_player()`, `npc:set_sprite(id)`, `npc:hide()`, `npc:show()` and
//! `npc:pos()` (→ `x, y, facing`). Crossing into Rust it becomes
//! [`ScriptValue::Npc`](crate::ScriptValue::Npc).

use mlua::{Function, Lua, Table, Value};
use pkrs_core::Direction;

use super::facade::{
    check, coroutine_fns, direction, name, needs_task, try_world, type_of, with_world,
};
use crate::world::direction_toward;

/// Registry key of the shared metatable.
const METATABLE: &str = "pkrs.npc";

/// `npc:move(path)`: validates and starts the walk in Rust, then yields `{ wait = "path" }`
/// from Lua (a Rust callback cannot yield).
const MOVE_FN: &str = r#"
local yield, isyieldable, request = ...
return function(self, path)
  local err, id = request(self, path, isyieldable())
  if err then error(err, 2) end
  if id then yield({ wait = "path", npc = id }) end
end
"#;

/// The id of an `npc` object, or `None` if `t` is not one.
pub(super) fn handle_id(t: &Table) -> Option<String> {
    let mt = t.metatable()?;
    match mt.raw_get::<Value>("__name").ok()? {
        Value::String(s) if s.to_string_lossy() == METATABLE => {}
        _ => return None,
    }
    match t.raw_get::<Value>("id").ok()? {
        Value::String(s) => Some(s.to_string_lossy()),
        _ => None,
    }
}

/// A new `npc` object for `id`.
pub(super) fn handle(lua: &Lua, id: &str) -> mlua::Result<Table> {
    let mt: Table = lua.named_registry_value(METATABLE)?;
    let t = lua.create_table()?;
    t.raw_set("id", id)?;
    t.set_metatable(Some(mt))?;
    Ok(t)
}

/// The id behind `self` for method `func`.
fn self_id(func: &str, this: &Value) -> Result<String, String> {
    if let Value::Table(t) = this {
        if let Some(id) = handle_id(t) {
            return Ok(id);
        }
    }
    Err(format!(
        "{func}: expected an npc object as `self` (got {}); call it with ':'",
        type_of(this)
    ))
}

fn api(func: &'static str, r: crate::world::ApiResult<()>) -> Result<(), String> {
    r.map_err(|e| format!("{func}: {e}"))
}

/// Builds the shared metatable. Run once at start-up.
pub(super) fn init(lua: &Lua) -> mlua::Result<()> {
    let methods = lua.create_table()?;
    methods.raw_set(
        "face",
        lua.create_function(|lua, (this, dir): (Value, Value)| {
            let id = check(lua, self_id("npc:face", &this))?;
            let dir = check(lua, direction("npc:face", "direction", &dir))?;
            let r = with_world(lua, "npc:face", |w| w.npc_face(&id, dir))?;
            check(lua, api("npc:face", r))
        })?,
    )?;
    methods.raw_set(
        "face_player",
        lua.create_function(|lua, this: Value| {
            let id = check(lua, self_id("npc:face_player", &this))?;
            let r = with_world(lua, "npc:face_player", |w| {
                let (npc_pos, _) = w.npc_pos(&id)?;
                let (player_pos, _) = w.player_pos();
                match direction_toward(npc_pos, player_pos) {
                    Some(dir) => w.npc_face(&id, dir),
                    None => Ok(()),
                }
            })?;
            check(lua, api("npc:face_player", r))
        })?,
    )?;
    methods.raw_set(
        "set_sprite",
        lua.create_function(|lua, (this, sprite): (Value, Value)| {
            let id = check(lua, self_id("npc:set_sprite", &this))?;
            let sprite = check(lua, name("npc:set_sprite", "sprite id", &sprite))?;
            let r = with_world(lua, "npc:set_sprite", |w| w.npc_set_sprite(&id, &sprite))?;
            check(lua, api("npc:set_sprite", r))
        })?,
    )?;
    for (method, visible) in [("hide", false), ("show", true)] {
        let func: &'static str = if visible { "npc:show" } else { "npc:hide" };
        methods.raw_set(
            method,
            lua.create_function(move |lua, this: Value| {
                let id = check(lua, self_id(func, &this))?;
                let r = with_world(lua, func, |w| w.npc_set_visible(&id, visible))?;
                check(lua, api(func, r))
            })?,
        )?;
    }
    methods.raw_set(
        "pos",
        lua.create_function(|lua, this: Value| {
            let id = check(lua, self_id("npc:pos", &this))?;
            let r = with_world(lua, "npc:pos", |w| w.npc_pos(&id))?;
            let (pos, facing) = check(lua, r.map_err(|e| format!("npc:pos: {e}")))?;
            Ok((pos.x, pos.y, facing.to_string()))
        })?,
    )?;
    let request = lua.create_function(|lua, (this, path, yieldable): (Value, Value, bool)| {
        let r = request_move(lua, &this, &path, yieldable);
        match r {
            Ok(Some(id)) => Ok((Value::Nil, Value::String(lua.create_string(id)?))),
            Ok(None) => Ok((Value::Nil, Value::Nil)),
            Err(msg) => Ok((Value::String(lua.create_string(msg)?), Value::Nil)),
        }
    })?;
    let (yield_fn, isyieldable) = coroutine_fns(lua)?;
    let move_fn: Function =
        lua.load(MOVE_FN)
            .set_name("=pkrs-npc")
            .call((yield_fn, isyieldable, request))?;
    methods.raw_set("move", move_fn)?;

    let mt = lua.create_table()?;
    mt.raw_set("__index", methods)?;
    mt.raw_set("__name", METATABLE)?;
    mt.raw_set("__metatable", false)?;
    mt.raw_set(
        "__tostring",
        lua.create_function(|_, this: Table| {
            Ok(format!(
                "npc({})",
                handle_id(&this).unwrap_or_else(|| "?".into())
            ))
        })?,
    )?;
    lua.set_named_registry_value(METATABLE, mt)
}

/// Validates `npc:move(path)` and starts the walk; `Ok(None)` for an empty path (nothing to
/// wait for).
fn request_move(
    lua: &Lua,
    this: &Value,
    path: &Value,
    yieldable: bool,
) -> Result<Option<String>, String> {
    const FUNC: &str = "npc:move";
    let id = self_id(FUNC, this)?;
    let Value::Table(list) = path else {
        return Err(format!(
            "{FUNC}: path must be a list of directions (got {})",
            type_of(path)
        ));
    };
    let mut steps: Vec<Direction> = Vec::new();
    for (i, entry) in list.clone().sequence_values::<Value>().enumerate() {
        let entry = entry.map_err(|e| format!("{FUNC}: {e}"))?;
        steps.push(direction(FUNC, &format!("path[{}]", i + 1), &entry)?);
    }
    if steps.len() != list.raw_len() || list.clone().pairs::<Value, Value>().count() != steps.len()
    {
        return Err(format!(
            "{FUNC}: path must be a list of directions like {{ \"up\", \"left\" }}"
        ));
    }
    if !yieldable {
        return Err(needs_task(FUNC));
    }
    if steps.is_empty() {
        return Ok(None);
    }
    try_world(lua, FUNC, |w| {
        w.npc_move(&id, &steps).map_err(|e| format!("{FUNC}: {e}"))
    })?;
    Ok(Some(id))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn handles_round_trip_their_id() {
        let lua = Lua::new();
        init(&lua).unwrap();
        let t = handle(&lua, "keeper").unwrap();
        assert_eq!(handle_id(&t).as_deref(), Some("keeper"));
        assert_eq!(handle_id(&lua.create_table().unwrap()), None);
    }
}
