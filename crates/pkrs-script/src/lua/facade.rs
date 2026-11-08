//! Shared plumbing for the API v0 bindings (§9.1): access to the [`ScriptWorld`] port and
//! argument validation with `file:line`-located, function-prefixed error messages.

use mlua::{Function, Lua, Table, Value};
use pkrs_core::{Direction, GridPos};
use pkrs_data::TextArgs;

use super::convert;
use super::sandbox::located_error;
use crate::host::ScriptValue;
use crate::world::ScriptWorld;

/// App data: the world the bindings talk to (`None` until
/// [`LuaScriptHost::set_world`](super::LuaScriptHost::set_world)).
#[derive(Default)]
pub(super) struct WorldSlot(pub Option<Box<dyn ScriptWorld>>);

/// Message for a binding used while no world is attached.
fn no_world(func: &str) -> String {
    format!("{func}: no game world is attached to the script host")
}

/// Runs `f` on the attached world, or raises a located error naming `func`.
pub(super) fn with_world<R>(
    lua: &Lua,
    func: &str,
    f: impl FnOnce(&mut dyn ScriptWorld) -> R,
) -> mlua::Result<R> {
    let attached = lua
        .app_data_mut::<WorldSlot>()
        .and_then(|mut slot| slot.0.as_mut().map(|world| f(world.as_mut())));
    attached.ok_or_else(|| located_error(lua, no_world(func)))
}

/// Like [`with_world`], but returns the problem as a message (for the Lua-side wrappers that
/// raise it themselves with `error(msg, 2)`).
pub(super) fn try_world<R>(
    lua: &Lua,
    func: &str,
    f: impl FnOnce(&mut dyn ScriptWorld) -> Result<R, String>,
) -> Result<R, String> {
    match lua.app_data_mut::<WorldSlot>() {
        Some(mut slot) => match slot.0.as_mut() {
            Some(world) => f(world.as_mut()),
            None => Err(no_world(func)),
        },
        None => Err(no_world(func)),
    }
}

/// Raises `message` at the caller's `file:line`.
pub(super) fn fail(lua: &Lua, message: impl AsRef<str>) -> mlua::Error {
    located_error(lua, message)
}

/// Lua type name of `v` for error messages.
pub(super) fn type_of(v: &Value) -> &'static str {
    match v {
        Value::Nil => "nil",
        Value::Boolean(_) => "boolean",
        Value::Integer(_) | Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Table(_) => "table",
        Value::Function(_) => "function",
        _ => "userdata",
    }
}

/// A non-empty string argument `what` of `func`.
pub(super) fn name(func: &str, what: &str, v: &Value) -> Result<String, String> {
    match v {
        Value::String(s) => {
            let s = s.to_string_lossy();
            if s.is_empty() {
                Err(format!("{func}: {what} must not be empty"))
            } else {
                Ok(s)
            }
        }
        other => Err(format!(
            "{func}: {what} must be a string (got {})",
            type_of(other)
        )),
    }
}

/// A string argument (may be empty).
pub(super) fn text(func: &str, what: &str, v: &Value) -> Result<String, String> {
    match v {
        Value::String(s) => Ok(s.to_string_lossy()),
        other => Err(format!(
            "{func}: {what} must be a string (got {})",
            type_of(other)
        )),
    }
}

/// A whole number in `min..=max` (floats without a fraction are accepted).
pub(super) fn int(func: &str, what: &str, v: &Value, min: i64, max: i64) -> Result<i64, String> {
    let n = match v {
        Value::Integer(i) => Some(*i),
        Value::Number(n) if n.fract() == 0.0 && n.is_finite() => Some(*n as i64),
        _ => None,
    };
    match n {
        Some(n) if (min..=max).contains(&n) => Ok(n),
        Some(n) => Err(format!(
            "{func}: {what} must be between {min} and {max} (got {n})"
        )),
        None => Err(format!(
            "{func}: {what} must be a whole number (got {})",
            type_of(v)
        )),
    }
}

/// Optional text arguments (§5.5): `nil`, or a table whose list part fills `{0}`, `{1}`, …
/// and whose string keys fill `{name}`. Values are strings, numbers or booleans.
pub(super) fn text_args(func: &str, v: &Value) -> Result<TextArgs, String> {
    let table = match v {
        Value::Nil => return Ok(TextArgs::new()),
        Value::Table(t) => t,
        other => {
            return Err(format!(
                "{func}: args must be a table (got {})",
                type_of(other)
            ));
        }
    };
    let value = |key: &str, v: &Value| -> Result<String, String> {
        match v {
            Value::String(s) => Ok(s.to_string_lossy()),
            Value::Integer(i) => Ok(i.to_string()),
            Value::Number(n) if n.fract() == 0.0 && n.is_finite() => Ok(format!("{n:.0}")),
            Value::Number(n) => Ok(n.to_string()),
            Value::Boolean(b) => Ok(b.to_string()),
            other => Err(format!(
                "{func}: args.{key} must be a string, number or boolean (got {})",
                type_of(other)
            )),
        }
    };
    let mut named = Vec::new();
    let mut positional = Vec::new();
    for pair in table.clone().pairs::<Value, Value>() {
        let (k, v) = pair.map_err(|e| format!("{func}: {e}"))?;
        match k {
            Value::String(s) => {
                let key = s.to_string_lossy();
                let v = value(&key, &v)?;
                named.push((key, v));
            }
            Value::Integer(i) if i >= 1 => {
                let v = value(&i.to_string(), &v)?;
                positional.push((i, v));
            }
            other => {
                return Err(format!(
                    "{func}: args keys must be names or list positions (got {})",
                    type_of(&other)
                ));
            }
        }
    }
    positional.sort();
    named.sort();
    let mut args = TextArgs::new();
    for (i, v) in positional {
        args.set((i - 1).to_string(), v);
    }
    for (k, v) in named {
        args.set(k, v);
    }
    Ok(args)
}

/// Like [`int`], `default` when `nil`.
pub(super) fn opt_int(
    func: &str,
    what: &str,
    v: &Value,
    default: i64,
    min: i64,
    max: i64,
) -> Result<i64, String> {
    if v.is_nil() {
        Ok(default)
    } else {
        int(func, what, v, min, max)
    }
}

/// A tick count (`0..=u32::MAX`), `default` when `nil`.
pub(super) fn ticks(func: &str, v: &Value, default: u32) -> Result<u32, String> {
    let n = opt_int(func, "ticks", v, i64::from(default), 0, i64::from(u32::MAX))?;
    u32::try_from(n).map_err(|_| format!("{func}: ticks out of range"))
}

/// Tile coordinates `x, y`.
pub(super) fn pos(func: &str, x: &Value, y: &Value) -> Result<GridPos, String> {
    let lim = i64::from(i32::MAX);
    let x = int(func, "x", x, -lim, lim)?;
    let y = int(func, "y", y, -lim, lim)?;
    Ok(GridPos::new(
        i32::try_from(x).unwrap_or(i32::MAX),
        i32::try_from(y).unwrap_or(i32::MAX),
    ))
}

/// Direction name `up` / `down` / `left` / `right`.
pub(super) fn direction(func: &str, what: &str, v: &Value) -> Result<Direction, String> {
    let bad = |got: String| {
        format!("{func}: {what} must be \"up\", \"down\", \"left\" or \"right\" (got {got})")
    };
    match v {
        Value::String(s) => {
            let raw = s.to_string_lossy();
            raw.parse::<Direction>()
                .map_err(|_| bad(format!("\"{raw}\"")))
        }
        other => Err(bad(type_of(other).to_owned())),
    }
}

/// Optional direction (`nil` → `None`).
pub(super) fn opt_direction(
    func: &str,
    what: &str,
    v: &Value,
) -> Result<Option<Direction>, String> {
    if v.is_nil() {
        Ok(None)
    } else {
        direction(func, what, v).map(Some)
    }
}

/// A value scripts may store in a flag/variable: `nil`, booleans, numbers, strings and tables
/// of those (functions, threads and userdata are refused, they cannot be saved, §9.6).
pub(super) fn storable(func: &str, v: &Value) -> Result<ScriptValue, String> {
    let value = convert::from_lua(v).map_err(|e| format!("{func}: {e}"))?;
    fn ok(v: &ScriptValue) -> bool {
        match v {
            ScriptValue::Opaque(_) | ScriptValue::Npc(_) => false,
            ScriptValue::List(items) => items.iter().all(ok),
            ScriptValue::Map(map) => map.values().all(ok),
            _ => true,
        }
    }
    if ok(&value) {
        Ok(value)
    } else {
        Err(format!(
            "{func}: value must be nil, a boolean, a number, a string or a table of those"
        ))
    }
}

/// Raises a `Result<_, String>` error at the caller's location.
pub(super) fn check<T>(lua: &Lua, r: Result<T, String>) -> mlua::Result<T> {
    r.map_err(|msg| fail(lua, msg))
}

/// `nil` or the error message, for Lua-side wrappers.
pub(super) fn report(lua: &Lua, r: Result<(), String>) -> mlua::Result<Value> {
    match r {
        Ok(()) => Ok(Value::Nil),
        Err(msg) => Ok(Value::String(lua.create_string(msg)?)),
    }
}

/// `coroutine.yield` and `coroutine.isyieldable` from the real globals (the sandbox copies
/// hold the same functions).
pub(super) fn coroutine_fns(lua: &Lua) -> mlua::Result<(Function, Function)> {
    let co: Table = lua.globals().raw_get("coroutine")?;
    Ok((co.raw_get("yield")?, co.raw_get("isyieldable")?))
}

/// Error for a blocking call made outside a task.
pub(super) fn needs_task(func: &str) -> String {
    format!("{func} must be called from a script task (coroutine)")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validators_name_the_function_and_argument() {
        assert_eq!(
            name("game.flag", "flag name", &Value::Nil).unwrap_err(),
            "game.flag: flag name must be a string (got nil)"
        );
        assert_eq!(
            int("game.give_item", "count", &Value::Integer(0), 1, 99).unwrap_err(),
            "game.give_item: count must be between 1 and 99 (got 0)"
        );
        assert_eq!(
            int("x", "n", &Value::Number(1.5), 0, 9).unwrap_err(),
            "x: n must be a whole number (got number)"
        );
        assert_eq!(opt_int("x", "n", &Value::Nil, 7, 0, 9).unwrap(), 7);
        assert_eq!(ticks("camera.fade_in", &Value::Nil, 20).unwrap(), 20);
    }
}
