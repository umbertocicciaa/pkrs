//! The `time` module (§9.1): `time.ticks()` and `time.wait(n)` (also the global `wait`), plus
//! parsing of the engine wait conditions a task may yield (§9.3).

use mlua::{Function, Lua, Table, Value};

use crate::error::{Result, ScriptError};
use crate::host::WaitRequest;

/// App data: the simulation tick reported by `time.ticks()`.
pub(super) struct Clock(pub u64);

/// `time.wait(n)`: validate `n`, then yield `{ wait = "ticks", n = n }` and return whatever the
/// engine resumes the task with (the current tick).
const WAIT_FN: &str = r#"
local yield, isyieldable, tointeger = ...
return function(n)
  if n == nil then n = 1 end
  local ticks = type(n) == "number" and tointeger(n) or nil
  if ticks == nil or ticks < 0 or ticks > 4294967295 then
    error("time.wait: n must be a whole number of ticks between 0 and 4294967295", 2)
  end
  if not isyieldable() then
    error("time.wait must be called from a script task (coroutine)", 2)
  end
  return yield({ wait = "ticks", n = ticks })
end
"#;

/// Registers the [`Clock`] app data (tick 0).
pub(super) fn init(lua: &Lua) {
    lua.set_app_data(Clock(0));
}

/// Sets the tick returned by `time.ticks()`.
pub(super) fn set_tick(lua: &Lua, tick: u64) {
    match lua.app_data_mut::<Clock>() {
        Some(mut clock) => clock.0 = tick,
        None => {
            lua.set_app_data(Clock(tick));
        }
    }
}

/// Installs `time` and the global `wait` alias into `env`.
pub(super) fn install(lua: &Lua, env: &Table) -> mlua::Result<()> {
    let time = lua.create_table()?;
    let ticks = lua.create_function(|lua, ()| {
        let tick = lua.app_data_ref::<Clock>().map_or(0, |c| c.0);
        Ok(i64::try_from(tick).unwrap_or(i64::MAX))
    })?;
    let co: Table = env.raw_get("coroutine")?;
    let math: Table = env.raw_get("math")?;
    let wait: Function = lua.load(WAIT_FN).set_name("=pkrs-time").call((
        co.raw_get::<Function>("yield")?,
        co.raw_get::<Function>("isyieldable")?,
        math.raw_get::<Function>("tointeger")?,
    ))?;
    time.raw_set("ticks", ticks)?;
    time.raw_set("wait", wait.clone())?;
    env.raw_set("time", time)?;
    env.raw_set("wait", wait)
}

/// Engine wait conditions other than battles: `{ wait = "ticks", n }`, `{ wait = "dialogue" }`
/// and `{ wait = "path", npc }`. `None` for anything else.
pub(super) fn parse_wait(table: &Table) -> Result<Option<WaitRequest>> {
    let conv = |e: mlua::Error| ScriptError::Conversion(e.to_string());
    let kind = match table.raw_get::<Value>("wait").map_err(conv)? {
        Value::String(s) => s.to_string_lossy(),
        _ => return Ok(None),
    };
    let request = match kind.as_str() {
        "ticks" => {
            let n: i64 = table.raw_get("n").map_err(conv)?;
            let n = u32::try_from(n).map_err(|_| {
                ScriptError::Conversion(format!("wait: tick count out of range ({n})"))
            })?;
            WaitRequest::Ticks(n)
        }
        "dialogue" => WaitRequest::DialogueClosed,
        "path" => WaitRequest::PathFinished {
            npc: table.raw_get("npc").map_err(conv)?,
        },
        _ => return Ok(None),
    };
    Ok(Some(request))
}
