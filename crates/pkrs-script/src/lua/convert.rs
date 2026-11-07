//! [`ScriptValue`] ⇄ Lua value conversion.

use std::collections::BTreeMap;

use mlua::{Lua, Table, Value};

use crate::error::{Result, ScriptError};
use crate::host::ScriptValue;

/// Tables nested deeper than this are rejected (also guards against cycles).
const MAX_DEPTH: usize = 32;

/// Lua → engine value.
pub(super) fn from_lua(value: &Value) -> Result<ScriptValue> {
    from_lua_depth(value, 0)
}

fn from_lua_depth(value: &Value, depth: usize) -> Result<ScriptValue> {
    Ok(match value {
        Value::Nil => ScriptValue::Nil,
        Value::Boolean(b) => ScriptValue::Bool(*b),
        Value::Integer(i) => ScriptValue::Int(*i),
        Value::Number(n) => ScriptValue::Number(*n),
        Value::String(s) => ScriptValue::String(s.to_string_lossy()),
        Value::Table(t) => table_from_lua(t, depth)?,
        other => ScriptValue::Opaque(other.type_name()),
    })
}

fn table_from_lua(table: &Table, depth: usize) -> Result<ScriptValue> {
    if let Some(id) = super::npc::handle_id(table) {
        return Ok(ScriptValue::Npc(id));
    }
    if depth >= MAX_DEPTH {
        return Err(ScriptError::Conversion(format!(
            "table nesting deeper than {MAX_DEPTH} (or a cycle)"
        )));
    }
    let len = table.raw_len();
    let mut entries = Vec::new();
    for pair in table.clone().pairs::<Value, Value>() {
        let (k, v) = pair.map_err(|e| ScriptError::Conversion(e.to_string()))?;
        entries.push((k, v));
    }
    if len > 0 && entries.len() == len {
        let mut list = Vec::with_capacity(len);
        for i in 1..=len {
            let v: Value = table
                .raw_get(i)
                .map_err(|e| ScriptError::Conversion(e.to_string()))?;
            list.push(from_lua_depth(&v, depth + 1)?);
        }
        return Ok(ScriptValue::List(list));
    }
    let mut map = BTreeMap::new();
    for (k, v) in entries {
        let key = match &k {
            Value::String(s) => s.to_string_lossy(),
            Value::Integer(i) => i.to_string(),
            Value::Number(n) => n.to_string(),
            Value::Boolean(b) => b.to_string(),
            other => {
                return Err(ScriptError::Conversion(format!(
                    "table key of type {} cannot be converted",
                    other.type_name()
                )));
            }
        };
        map.insert(key, from_lua_depth(&v, depth + 1)?);
    }
    Ok(ScriptValue::Map(map))
}

/// Engine → Lua value.
pub(super) fn to_lua(lua: &Lua, value: &ScriptValue) -> mlua::Result<Value> {
    Ok(match value {
        ScriptValue::Nil | ScriptValue::Opaque(_) => Value::Nil,
        ScriptValue::Bool(b) => Value::Boolean(*b),
        ScriptValue::Int(i) => Value::Integer(*i),
        ScriptValue::Number(n) => Value::Number(*n),
        ScriptValue::String(s) => Value::String(lua.create_string(s)?),
        ScriptValue::Npc(id) => Value::Table(super::npc::handle(lua, id)?),
        ScriptValue::List(items) => {
            let t = lua.create_table_with_capacity(items.len(), 0)?;
            for (i, item) in items.iter().enumerate() {
                t.raw_set(i + 1, to_lua(lua, item)?)?;
            }
            Value::Table(t)
        }
        ScriptValue::Map(map) => {
            let t = lua.create_table_with_capacity(0, map.len())?;
            for (k, v) in map {
                t.raw_set(k.as_str(), to_lua(lua, v)?)?;
            }
            Value::Table(t)
        }
    })
}
