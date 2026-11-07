//! The `data` module (§9.1, §9.4): content registration and read-only queries.
//!
//! - `data.register_move(def)`, `register_ability`, `register_item`, `register_status`,
//!   `register_terrain` and `register_effect(def, fn?)` add a record to the content database
//!   and return its id. `def` uses the same fields as the RON files (enum values as strings,
//!   e.g. `category = "special"`); `override = true` replaces an existing id (logged), and a
//!   duplicate without it is an error. Plugins may use plain ids or their own `id:` namespace.
//!   `register_effect`'s function implements the effect primitive.
//! - `data.has(kind, id)` and `data.ids(kind)` query it (`kind` = `"move"`, `"item"`, …).

use std::collections::BTreeMap;

use mlua::{Lua, Table, Value};
use pkrs_data::{ContentDef, ContentKind, ContentValue};

use super::convert;
use super::facade::{check, fail, name, type_of};
use super::registry::{content_source, with_regs};
use crate::host::{ScriptOwner, ScriptValue};
use crate::plugin::{check_custom_terrain, log_override};

/// Engine value → content value; `None` for values that cannot be content.
fn to_content(value: &ScriptValue) -> Option<ContentValue> {
    Some(match value {
        ScriptValue::Nil => ContentValue::Unit,
        ScriptValue::Bool(b) => ContentValue::Bool(*b),
        ScriptValue::Int(i) => ContentValue::Int(*i),
        ScriptValue::Number(n) if n.fract() == 0.0 && n.is_finite() => ContentValue::Int(*n as i64),
        ScriptValue::Number(n) => ContentValue::Float(*n),
        ScriptValue::String(s) => ContentValue::Str(s.clone()),
        ScriptValue::List(items) => {
            ContentValue::List(items.iter().map(to_content).collect::<Option<Vec<_>>>()?)
        }
        ScriptValue::Map(map) => ContentValue::Map(
            map.iter()
                .map(|(k, v)| Some((k.clone(), to_content(v)?)))
                .collect::<Option<BTreeMap<_, _>>>()?,
        ),
        ScriptValue::Opaque(_) | ScriptValue::Npc(_) => return None,
    })
}

fn kind_arg(func: &str, v: &Value) -> Result<ContentKind, String> {
    let raw = name(func, "kind", v)?;
    ContentKind::parse(&raw).ok_or_else(|| {
        let names: Vec<&str> = ContentKind::ALL.iter().map(|k| k.name()).collect();
        format!("{func}: unknown kind `{raw}` (use {})", names.join(", "))
    })
}

fn register(
    lua: &Lua,
    owner: &ScriptOwner,
    kind: ContentKind,
    def: &Value,
    extra: Value,
) -> mlua::Result<String> {
    let func = format!("data.register_{}", kind.name());
    if !matches!(def, Value::Table(_)) {
        return Err(fail(
            lua,
            format!("{func}: definition must be a table (got {})", type_of(def)),
        ));
    }
    let value = convert::from_lua(def).map_err(|e| fail(lua, format!("{func}: {e}")))?;
    let value = to_content(&value).ok_or_else(|| {
        fail(
            lua,
            format!("{func}: definitions may only hold booleans, numbers, strings and tables"),
        )
    })?;
    let record = ContentDef::from_value(kind, value, "definition")
        .map_err(|e| fail(lua, format!("{func}: {e}")))?;
    let id = record.def.id().to_owned();
    if kind == ContentKind::Terrain {
        check(
            lua,
            check_custom_terrain(&id).map_err(|e| format!("{func}: {e}")),
        )?;
    }
    let handler = match (kind, extra) {
        (ContentKind::Effect, Value::Function(f)) => Some(f),
        (_, Value::Nil) => None,
        (ContentKind::Effect, other) => {
            return Err(fail(
                lua,
                format!(
                    "{func}: implementation must be a function (got {})",
                    type_of(&other)
                ),
            ));
        }
        (_, _) => {
            return Err(fail(lua, format!("{func}: takes one definition table")));
        }
    };
    let source = content_source(owner);
    let inserted = with_regs(lua, |r| {
        let result = r.content.insert(record, source);
        if result.is_ok() {
            if let Some(f) = handler {
                r.effects.insert(id.clone(), f);
            }
        }
        result
    })?;
    match inserted {
        Ok(Some(ov)) => log_override(&ov, &func),
        Ok(None) => {}
        Err(e) => return Err(fail(lua, format!("{func}: {e}"))),
    }
    Ok(id)
}

/// Installs `data` into `env` for `owner`.
pub(super) fn install(lua: &Lua, env: &Table, owner: &ScriptOwner) -> mlua::Result<()> {
    let data = lua.create_table()?;
    for kind in ContentKind::ALL {
        let kind = *kind;
        let owner = owner.clone();
        data.raw_set(
            format!("register_{}", kind.name()),
            lua.create_function(move |lua, (def, extra): (Value, Value)| {
                register(lua, &owner, kind, &def, extra)
            })?,
        )?;
    }
    data.raw_set(
        "has",
        lua.create_function(|lua, (kind, id): (Value, Value)| {
            let kind = check(lua, kind_arg("data.has", &kind))?;
            let id = check(lua, name("data.has", "id", &id))?;
            with_regs(lua, |r| r.content.contains(kind, &id))
        })?,
    )?;
    data.raw_set(
        "ids",
        lua.create_function(|lua, kind: Value| {
            let kind = check(lua, kind_arg("data.ids", &kind))?;
            let ids: Vec<ScriptValue> = with_regs(lua, |r| {
                r.content
                    .ids(kind)
                    .into_iter()
                    .map(ScriptValue::from)
                    .collect()
            })?;
            convert::to_lua(lua, &ScriptValue::List(ids))
        })?,
    )?;
    env.raw_set("data", data)
}
