//! The `input` module (§10): `input.register_action(name, default_keys?)`.
//!
//! Registers a custom input action (`resonance:toggle`) with default key names
//! (`{ "r", "left_shift" }`). Plugins must use their own `plugin_id:` namespace; the game may
//! use plain `snake_case` names. Built-in action names and duplicates are errors. The engine
//! binds it ([`LuaScriptHost::take_input_actions`](super::LuaScriptHost::take_input_actions)),
//! `pkrs.toml [input]` may rebind it, and a press emits the `action` event
//! (`{ action = name }`).

use mlua::{Lua, Table, Value};
use pkrs_core::BuiltinAction;

use super::facade::{check, fail, name, type_of};
use super::registry::{check_id, with_regs};
use crate::host::ScriptOwner;
use crate::registry::InputActionRequest;

fn register_action(
    lua: &Lua,
    owner: &ScriptOwner,
    (action, keys): (Value, Value),
) -> mlua::Result<String> {
    const FUNC: &str = "input.register_action";
    let action = check(lua, name(FUNC, "action name", &action))?;
    check(lua, check_id(FUNC, "action", &action, owner))?;
    if matches!(owner, ScriptOwner::Plugin(_)) && !action.contains(':') {
        return Err(fail(
            lua,
            format!("{FUNC}: plugin actions must be namespaced (`plugin_id:{action}`)"),
        ));
    }
    if BuiltinAction::parse(&action).is_some() {
        return Err(fail(
            lua,
            format!("{FUNC}: `{action}` is a built-in action"),
        ));
    }
    let keys = match keys {
        Value::Nil => Vec::new(),
        Value::Table(t) => {
            let mut out = Vec::new();
            for (i, v) in t.sequence_values::<Value>().enumerate() {
                out.push(check(lua, name(FUNC, &format!("keys[{}]", i + 1), &v?))?);
            }
            out
        }
        other => {
            return Err(fail(
                lua,
                format!(
                    "{FUNC}: default keys must be a list like {{ \"r\" }} (got {})",
                    type_of(&other)
                ),
            ));
        }
    };
    let duplicate = with_regs(lua, |r| {
        let dup = r
            .input_actions
            .iter()
            .find(|a| a.name == action)
            .map(|a| a.owner.clone());
        if dup.is_none() {
            r.input_actions.push(InputActionRequest {
                owner: owner.clone(),
                name: action.clone(),
                keys,
            });
        }
        dup
    })?;
    match duplicate {
        Some(prev) => Err(fail(
            lua,
            format!("{FUNC}: action `{action}` is already registered by {prev}"),
        )),
        None => Ok(action),
    }
}

/// Installs `input` into `env` for `owner`.
pub(super) fn install(lua: &Lua, env: &Table, owner: &ScriptOwner) -> mlua::Result<()> {
    let input = lua.create_table()?;
    let o = owner.clone();
    input.raw_set(
        "register_action",
        lua.create_function(move |lua, args: (Value, Value)| register_action(lua, &o, args))?,
    )?;
    env.raw_set("input", input)
}
