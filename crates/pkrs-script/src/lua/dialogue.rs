//! Blocking `ui` calls (§9.1, §9.3): `ui.say(text)`, `ui.choose(options)`, `ui.yes_no(text)`,
//! plus the non-blocking `ui.notify(text)`. Added to the `ui` table next to
//! `register_screen` / `push_screen` (see `ui.rs`).
//!
//! The blocking calls open the box through the [`ScriptWorld`](crate::ScriptWorld) port, then
//! yield `{ wait = "dialogue" }` from Lua; the engine resumes the task with
//! [`WakeSignal::DialogueClosed`](crate::WakeSignal::DialogueClosed) once the box closes.
//! `ui.choose` returns the 1-based index picked (or `nil` when cancelled); `ui.yes_no`
//! returns `true` for "Yes".

use mlua::{Function, Lua, Table, Value};

use super::facade::{coroutine_fns, needs_task, report, text, try_world, type_of, with_world};

/// Labels of the `ui.yes_no` options.
pub(crate) const YES_NO: [&str; 2] = ["Yes", "No"];

/// Lua wrappers: Rust validates and opens the box, Lua yields.
const DIALOGUE_FNS: &str = r#"
local yield, isyieldable, req_say, req_choose = ...
local function say(text)
  local err = req_say(text, isyieldable())
  if err then error(err, 2) end
  yield({ wait = "dialogue" })
end
local function choose(options)
  local err = req_choose("ui.choose", nil, options, isyieldable())
  if err then error(err, 2) end
  return yield({ wait = "dialogue" })
end
local function yes_no(text)
  local err = req_choose("ui.yes_no", text, nil, isyieldable())
  if err then error(err, 2) end
  return yield({ wait = "dialogue" }) == 1
end
return say, choose, yes_no
"#;

fn options(func: &str, v: &Value) -> Result<Vec<String>, String> {
    let Value::Table(list) = v else {
        return Err(format!(
            "{func}: options must be a list of strings (got {})",
            type_of(v)
        ));
    };
    let mut out = Vec::new();
    for (i, entry) in list.clone().sequence_values::<Value>().enumerate() {
        let entry = entry.map_err(|e| format!("{func}: {e}"))?;
        out.push(text(func, &format!("option {}", i + 1), &entry)?);
    }
    if out.len() != list.clone().pairs::<Value, Value>().count() {
        return Err(format!(
            "{func}: options must be a list of strings like {{ \"Yes\", \"No\" }}"
        ));
    }
    if out.is_empty() {
        return Err(format!("{func}: options must not be empty"));
    }
    Ok(out)
}

/// Adds `say`, `choose`, `yes_no` and `notify` to `env.ui`.
pub(super) fn install(lua: &Lua, env: &Table) -> mlua::Result<()> {
    let ui: Table = env.raw_get("ui")?;
    let req_say = lua.create_function(|lua, (msg, yieldable): (Value, bool)| {
        const FUNC: &str = "ui.say";
        let r = text(FUNC, "text", &msg).and_then(|msg| {
            if !yieldable {
                return Err(needs_task(FUNC));
            }
            try_world(lua, FUNC, |w| {
                w.say(&msg).map_err(|e| format!("{FUNC}: {e}"))
            })
        });
        report(lua, r)
    })?;
    let req_choose = lua.create_function(
        |lua, (func, prompt, opts, yieldable): (String, Value, Value, bool)| {
            let r = (|| {
                let (prompt, opts) = if func == "ui.yes_no" {
                    let prompt = text(&func, "text", &prompt)?;
                    (Some(prompt), YES_NO.map(str::to_owned).to_vec())
                } else {
                    (None, options(&func, &opts)?)
                };
                if !yieldable {
                    return Err(needs_task(&func));
                }
                try_world(lua, &func, |w| {
                    w.choose(prompt.as_deref(), &opts)
                        .map_err(|e| format!("{func}: {e}"))
                })
            })();
            report(lua, r)
        },
    )?;
    let (yield_fn, isyieldable) = coroutine_fns(lua)?;
    let (say, choose, yes_no): (Function, Function, Function) = lua
        .load(DIALOGUE_FNS)
        .set_name("=pkrs-ui")
        .call((yield_fn, isyieldable, req_say, req_choose))?;
    ui.raw_set("say", say)?;
    ui.raw_set("choose", choose)?;
    ui.raw_set("yes_no", yes_no)?;
    ui.raw_set(
        "notify",
        lua.create_function(|lua, msg: Value| {
            let msg = super::facade::check(lua, text("ui.notify", "text", &msg))?;
            with_world(lua, "ui.notify", |w| w.notify(&msg))
        })?,
    )?;
    Ok(())
}
