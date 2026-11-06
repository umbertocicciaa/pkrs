//! `ui.register_screen`, `ui.push_screen` and a minimal `gfx` drawing API (§9.5).

use mlua::{Function, Lua, Table, Value};

use super::convert;
use super::engine;
use crate::error::{Result, ScriptError};
use crate::host::{ScriptOwner, WaitRequest};
use crate::input_bridge::LastScriptInput;

/// Built-in stack ids plugins must not override (§9.5).
const RESERVED_SCREEN_IDS: &[&str] = &["title", "dialogue", "choice", "transition"];

/// Per-environment table `__pkrs_ui_screens` (id → handler table).
fn screens_table(lua: &Lua, env: &Table) -> Result<Table> {
    let key = "__pkrs_ui_screens";
    let t: Value = env.raw_get(key).map_err(engine)?;
    match t {
        Value::Table(tab) => Ok(tab),
        Value::Nil => {
            let tab = lua.create_table().map_err(engine)?;
            env.raw_set(key, tab.clone()).map_err(engine)?;
            Ok(tab)
        }
        _ => Err(ScriptError::Engine(
            "ui: internal __pkrs_ui_screens is not a table".into(),
        )),
    }
}

fn expect_string(_lua: &Lua, v: Value, what: &str) -> Result<String> {
    match v {
        Value::String(s) => Ok(s.to_string_lossy()),
        _ => Err(ScriptError::Conversion(format!(
            "ui.{what}: string expected, got {}",
            v.type_name()
        ))),
    }
}

fn expect_function_or_nil(_lua: &Lua, t: &Table, key: &str) -> Result<Option<Function>> {
    let v: Value = t.raw_get(key).map_err(engine)?;
    match v {
        Value::Nil => Ok(None),
        Value::Function(f) => Ok(Some(f)),
        _ => Err(ScriptError::Conversion(format!(
            "ui.register_screen: `{key}` must be a function or nil"
        ))),
    }
}

/// One draw command recorded for tests / host bridge.
#[derive(Clone, Debug, PartialEq)]
pub enum GfxOp {
    /// Filled rectangle (window chrome).
    Window {
        /// Left in internal pixels.
        x: f64,
        /// Top in internal pixels.
        y: f64,
        /// Width in internal pixels.
        w: f64,
        /// Height in internal pixels.
        h: f64,
    },
    /// Text placeholder (content key or literal).
    Text {
        /// X in internal pixels.
        x: f64,
        /// Y in internal pixels.
        y: f64,
        /// Raw string from Lua.
        text: String,
    },
}

/// Commands queued during the last `gfx` calls for the current screen.
#[derive(Debug, Default)]
pub struct GfxFrame(pub Vec<GfxOp>);

fn push_gfx_op(lua: &Lua, op: GfxOp) -> Result<()> {
    let mut frame = lua
        .app_data_mut::<GfxFrame>()
        .ok_or_else(|| ScriptError::Engine("gfx: app data missing".into()))?;
    frame.0.push(op);
    Ok(())
}

/// Clears recorded gfx ops (before `render`).
pub(crate) fn begin_gfx_recording(lua: &Lua) {
    lua.set_app_data(GfxFrame::default());
}

/// Takes and clears the last frame's ops.
#[must_use]
pub fn take_gfx_ops(lua: &Lua) -> Vec<GfxOp> {
    lua.app_data_mut::<GfxFrame>()
        .map(|mut f| std::mem::take(&mut f.0))
        .unwrap_or_default()
}

/// `ui.register_screen(id, handlers)`.
fn register_screen(lua: &Lua, owner: ScriptOwner, env: &Table) -> Result<Function> {
    let env = env.clone();
    let owner_for_log = owner.to_string();
    lua.create_function(move |lua, (id, handlers): (Value, Value)| {
        let id = expect_string(lua, id, "register_screen").map_err(mlua::Error::external)?;
        if id.is_empty() {
            return Err(mlua::Error::runtime(
                "ui.register_screen: id must not be empty",
            ));
        }
        if RESERVED_SCREEN_IDS.contains(&id.as_str()) {
            return Err(mlua::Error::runtime(format!(
                "ui.register_screen: id `{id}` is reserved for built-in states"
            )));
        }
        let Value::Table(tab) = handlers else {
            return Err(mlua::Error::runtime(
                "ui.register_screen: second argument must be a table",
            ));
        };
        expect_function_or_nil(lua, &tab, "on_enter").map_err(mlua::Error::external)?;
        expect_function_or_nil(lua, &tab, "update").map_err(mlua::Error::external)?;
        expect_function_or_nil(lua, &tab, "render").map_err(mlua::Error::external)?;

        let scr = screens_table(lua, &env).map_err(mlua::Error::external)?;
        let existing: Value = scr.raw_get(id.as_str()).map_err(mlua::Error::external)?;
        if !existing.is_nil() {
            return Err(mlua::Error::runtime(format!(
                "ui.register_screen: screen `{id}` is already registered"
            )));
        }
        scr.raw_set(id.as_str(), tab)
            .map_err(mlua::Error::external)?;
        tracing::info!(
            target: "script",
            owner = %owner_for_log,
            screen = %id,
            "registered Lua UI screen"
        );
        Ok(())
    })
    .map_err(engine)
}

/// `ui.push_screen(id, args?)` — must be called from a script task; yields until popped.
///
/// Implemented as a Lua chunk (like `time.wait`) so `coroutine.yield` runs in Lua, not across an
/// `mlua` C callback (`attempt to yield across a C-call boundary`).
const PUSH_SCREEN_FN: &str = r#"
local env, yield, isyieldable = ...
return function(id, args)
  if type(id) ~= "string" or id == "" then
    error("ui.push_screen: id must be a non-empty string", 2)
  end
  if not isyieldable() then
    error("ui.push_screen must be called from a script task (coroutine)", 2)
  end
  local scr = env.__pkrs_ui_screens
  if scr == nil then
    error("ui.push_screen: no screens registered", 2)
  end
  if scr[id] == nil then
    error(("ui.push_screen: unknown screen `" .. id .. "` (call ui.register_screen first)"), 2)
  end
  return yield({ wait = "ui_push", screen = id, args = args })
end
"#;

fn push_screen(lua: &Lua, env: &Table) -> Result<Function> {
    let env = env.clone();
    let co: Table = env.raw_get("coroutine").map_err(engine)?;
    let yield_fn: Function = co.raw_get("yield").map_err(engine)?;
    let isyieldable: Function = co.raw_get("isyieldable").map_err(engine)?;
    lua.load(PUSH_SCREEN_FN)
        .set_name("=pkrs-ui-push_screen")
        .call((env, yield_fn, isyieldable))
        .map_err(engine)
}

/// Builds the `gfx` table passed to `render(self, gfx)`.
fn make_gfx_table(lua: &Lua) -> Result<Table> {
    let gfx = lua.create_table().map_err(engine)?;
    let window_fn = lua
        .create_function(move |lua, (x, y, w, h): (f64, f64, f64, f64)| {
            push_gfx_op(lua, GfxOp::Window { x, y, w, h }).map_err(mlua::Error::external)?;
            Ok(())
        })
        .map_err(engine)?;
    let text_fn = lua
        .create_function(move |lua, (x, y, text): (f64, f64, String)| {
            push_gfx_op(lua, GfxOp::Text { x, y, text }).map_err(mlua::Error::external)?;
            Ok(())
        })
        .map_err(engine)?;
    let sprite_stub = lua
        .create_function(|_, (_tex, _x, _y): (String, f64, f64)| Ok(()))
        .map_err(engine)?;
    gfx.raw_set("window", window_fn).map_err(engine)?;
    gfx.raw_set("text", text_fn).map_err(engine)?;
    gfx.raw_set("sprite", sprite_stub).map_err(engine)?;
    Ok(gfx)
}

/// `wait = ui_push` from `ui.push_screen`.
pub(super) fn parse_ui_push(table: &Table) -> Result<Option<WaitRequest>> {
    let conv = |e: mlua::Error| ScriptError::Conversion(e.to_string());
    let kind = match table.raw_get::<Value>("wait").map_err(conv)? {
        Value::String(s) => s.to_string_lossy(),
        _ => return Ok(None),
    };
    if kind != "ui_push" {
        return Ok(None);
    }
    let screen: String = table.raw_get("screen").map_err(conv)?;
    if screen.is_empty() {
        return Err(ScriptError::Conversion(
            "ui.push_screen: `screen` must be a non-empty string".into(),
        ));
    }
    let args_val: Value = table.raw_get("args").map_err(conv)?;
    let args_sv = convert::from_lua(&args_val)?;
    Ok(Some(WaitRequest::PushUiScreen {
        screen,
        args: args_sv,
    }))
}

/// `input.pressed("cancel")` for UI scripts; reads [`LastScriptInput`] app data.
fn install_input(lua: &Lua, env: &Table) -> Result<()> {
    let input = lua.create_table().map_err(engine)?;
    let pressed = lua
        .create_function(|lua, name: String| {
            let snap = lua
                .app_data_ref::<LastScriptInput>()
                .map(|l| l.0.clone())
                .unwrap_or_default();
            Ok(snap.pressed_named(name.trim()))
        })
        .map_err(engine)?;
    input.raw_set("pressed", pressed).map_err(engine)?;
    env.raw_set("__pkrs_input", input).map_err(engine)?;
    Ok(())
}

/// Ensures `__pkrs_input` exists (older cached envs before this change).
pub(super) fn ensure_input_table(lua: &Lua, env: &Table) -> mlua::Result<()> {
    let v: Value = env.raw_get("__pkrs_input")?;
    if v.is_nil() {
        install_input(lua, env).map_err(mlua::Error::external)?;
    }
    Ok(())
}

/// `ui` module: `register_screen`, `push_screen`; `gfx` prototype lives in [`make_gfx_table`].
pub(super) fn install(lua: &Lua, env: &Table, owner: &ScriptOwner) -> Result<()> {
    lua.set_app_data(GfxFrame::default());
    install_input(lua, env)?;
    let ui = lua.create_table().map_err(engine)?;
    ui.raw_set("register_screen", register_screen(lua, owner.clone(), env)?)
        .map_err(engine)?;
    ui.raw_set("push_screen", push_screen(lua, env)?)
        .map_err(engine)?;
    env.raw_set("ui", ui).map_err(engine)?;
    Ok(())
}

/// Table with `window` / `text` / `sprite` for one `render` call.
pub fn gfx_api_table(lua: &Lua) -> Result<Table> {
    make_gfx_table(lua)
}
