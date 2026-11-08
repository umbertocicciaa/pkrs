//! The `world` module (§6.3, §9.4): custom terrain rules and field actions.
//!
//! - `world.register_terrain(name, rule, opts?)` (also `map.register_terrain`): `rule(ctx)`
//!   runs when the player steps onto a tile whose `terrain=` is `name`, with
//!   `ctx = { terrain, x, y, from = { x, y }, dir, field = { [state] = true } }`; returning
//!   `false` or `"block"` blocks the step. Built-in terrains cannot be ruled. A second rule for
//!   the same name is an error unless `opts.override = true` (logged).
//! - `world.register_field_action(id, def)`: `def = { label?, terrain?, menu? = true,
//!   available?(ctx), run(ctx), priority? = 0, override? }`. The action is offered in the party
//!   menu (`menu ~= false`) and, when it names a `terrain`, by pressing Confirm while facing
//!   such a tile (and only then). `run(ctx)` runs as an interact task, so it may `ui.say`,
//!   `wait`, … `ctx = { source = "party"|"tile", x, y, facing, target = { x, y, terrain } }`.

use mlua::{Function, Lua, Table, Value};

use super::facade::{check, fail, name, type_of};
use super::hook_order;
use super::registry::{FieldActionEntry, ItemUseEntry, check_id, with_regs};
use crate::host::ScriptOwner;
use crate::plugin::check_custom_terrain;
use crate::registry::{FieldActionInfo, ItemUseInfo};

fn opt_bool(lua: &Lua, func: &str, t: &Table, key: &str, default: bool) -> mlua::Result<bool> {
    match t.raw_get::<Value>(key)? {
        Value::Nil => Ok(default),
        Value::Boolean(b) => Ok(b),
        other => Err(fail(
            lua,
            format!(
                "{func}: `{key}` must be a boolean (got {})",
                type_of(&other)
            ),
        )),
    }
}

fn opt_string(lua: &Lua, func: &str, t: &Table, key: &str) -> mlua::Result<Option<String>> {
    match t.raw_get::<Value>(key)? {
        Value::Nil => Ok(None),
        v => check(lua, name(func, &format!("`{key}`"), &v)).map(Some),
    }
}

fn opt_function(lua: &Lua, func: &str, t: &Table, key: &str) -> mlua::Result<Option<Function>> {
    match t.raw_get::<Value>(key)? {
        Value::Nil => Ok(None),
        Value::Function(f) => Ok(Some(f)),
        other => Err(fail(
            lua,
            format!(
                "{func}: `{key}` must be a function (got {})",
                type_of(&other)
            ),
        )),
    }
}

fn register_terrain(
    lua: &Lua,
    owner: &ScriptOwner,
    (terrain, rule, opts): (Value, Value, Value),
) -> mlua::Result<String> {
    const FUNC: &str = "world.register_terrain";
    let terrain = check(lua, name(FUNC, "terrain name", &terrain))?;
    check(
        lua,
        check_custom_terrain(&terrain).map_err(|e| format!("{FUNC}: {e}")),
    )?;
    check(lua, check_id(FUNC, "terrain", &terrain, owner))?;
    let Value::Function(rule) = rule else {
        return Err(fail(
            lua,
            format!("{FUNC}: rule must be a function (got {})", type_of(&rule)),
        ));
    };
    let override_existing = match opts {
        Value::Nil => false,
        Value::Table(t) => opt_bool(lua, FUNC, &t, "override", false)?,
        other => {
            return Err(fail(
                lua,
                format!(
                    "{FUNC}: options must be a table like {{ override = true }} (got {})",
                    type_of(&other)
                ),
            ));
        }
    };
    let previous = with_regs(lua, |r| {
        let previous = r.terrain_rules.get(&terrain).map(|(o, _)| o.clone());
        if previous.is_none() || override_existing {
            r.terrain_rules
                .insert(terrain.clone(), (owner.clone(), rule));
        }
        previous
    })?;
    match previous {
        Some(prev) if !override_existing => Err(fail(
            lua,
            format!(
                "{FUNC}: terrain `{terrain}` already has a rule from {prev}; pass {{ override = true }} to replace it"
            ),
        )),
        Some(prev) => {
            tracing::info!(target: "data", terrain = %terrain, previous = %prev, by = %owner, "terrain rule overridden");
            Ok(terrain)
        }
        None => Ok(terrain),
    }
}

fn register_field_action(
    lua: &Lua,
    owner: &ScriptOwner,
    (id, def): (Value, Value),
) -> mlua::Result<String> {
    const FUNC: &str = "world.register_field_action";
    let id = check(lua, name(FUNC, "action id", &id))?;
    check(lua, check_id(FUNC, "action id", &id, owner))?;
    let Value::Table(def) = def else {
        return Err(fail(
            lua,
            format!("{FUNC}: definition must be a table (got {})", type_of(&def)),
        ));
    };
    let Some(run) = opt_function(lua, FUNC, &def, "run")? else {
        return Err(fail(lua, format!("{FUNC}: `run` must be a function")));
    };
    let available = opt_function(lua, FUNC, &def, "available")?;
    let label = opt_string(lua, FUNC, &def, "label")?.unwrap_or_else(|| id.clone());
    let terrain = opt_string(lua, FUNC, &def, "terrain")?;
    let menu = opt_bool(lua, FUNC, &def, "menu", true)?;
    let override_existing = opt_bool(lua, FUNC, &def, "override", false)?;
    let priority = hook_order::parse_priority(Some(&def))
        .map_err(|_| fail(lua, format!("{FUNC}: `priority` must be an integer")))?;
    let key = hook_order::chain_key(lua, owner, priority);
    let entry = FieldActionEntry {
        key,
        info: FieldActionInfo {
            id: id.clone(),
            label,
            owner: owner.clone(),
            terrain,
            menu,
        },
        available,
        run,
    };
    let previous = with_regs(lua, |r| {
        let previous = r
            .field_actions
            .iter()
            .position(|e| e.info.id == id)
            .map(|i| (i, r.field_actions[i].info.owner.clone()));
        match previous {
            Some(_) if !override_existing => {}
            Some((i, _)) => {
                r.field_actions.remove(i);
                r.field_actions.push(entry);
            }
            None => r.field_actions.push(entry),
        }
        r.field_actions.sort_by_key(|e| e.key);
        previous.map(|(_, o)| o)
    })?;
    match previous {
        Some(prev) if !override_existing => Err(fail(
            lua,
            format!(
                "{FUNC}: field action `{id}` is already registered by {prev}; set `override = true` to replace it"
            ),
        )),
        Some(prev) => {
            tracing::info!(target: "data", action = %id, previous = %prev, by = %owner, "field action overridden");
            Ok(id)
        }
        None => Ok(id),
    }
}

fn register_item_use(
    lua: &Lua,
    owner: &ScriptOwner,
    (id, def): (Value, Value),
) -> mlua::Result<String> {
    const FUNC: &str = "world.register_item_use";
    let id = check(lua, name(FUNC, "item id", &id))?;
    check(lua, check_id(FUNC, "item id", &id, owner))?;
    let Value::Table(def) = def else {
        return Err(fail(
            lua,
            format!("{FUNC}: definition must be a table (got {})", type_of(&def)),
        ));
    };
    let Some(run) = opt_function(lua, FUNC, &def, "use")? else {
        return Err(fail(lua, format!("{FUNC}: `use` must be a function")));
    };
    let override_existing = opt_bool(lua, FUNC, &def, "override", false)?;
    let priority = hook_order::parse_priority(Some(&def))
        .map_err(|_| fail(lua, format!("{FUNC}: `priority` must be an integer")))?;
    let key = hook_order::chain_key(lua, owner, priority);
    let entry = ItemUseEntry {
        key,
        info: ItemUseInfo {
            id: id.clone(),
            owner: owner.clone(),
        },
        run,
    };
    let previous = with_regs(lua, |r| {
        let previous = r
            .item_uses
            .iter()
            .position(|e| e.info.id == id)
            .map(|i| (i, r.item_uses[i].info.owner.clone()));
        match previous {
            Some(_) if !override_existing => {}
            Some((i, _)) => {
                r.item_uses.remove(i);
                r.item_uses.push(entry);
            }
            None => r.item_uses.push(entry),
        }
        r.item_uses.sort_by_key(|e| e.key);
        previous.map(|(_, o)| o)
    })?;
    match previous {
        Some(prev) if !override_existing => Err(fail(
            lua,
            format!(
                "{FUNC}: item use `{id}` is already registered by {prev}; set `override = true` to replace it"
            ),
        )),
        Some(prev) => {
            tracing::info!(target: "data", item = %id, previous = %prev, by = %owner, "item use overridden");
            Ok(id)
        }
        None => Ok(id),
    }
}

/// Installs `world` into `env` for `owner`, and `map.register_terrain` as an alias.
pub(super) fn install(lua: &Lua, env: &Table, owner: &ScriptOwner) -> mlua::Result<()> {
    let world = lua.create_table()?;
    let o = owner.clone();
    let terrain = lua
        .create_function(move |lua, args: (Value, Value, Value)| register_terrain(lua, &o, args))?;
    world.raw_set("register_terrain", terrain.clone())?;
    let o = owner.clone();
    world.raw_set(
        "register_field_action",
        lua.create_function(move |lua, args: (Value, Value)| register_field_action(lua, &o, args))?,
    )?;
    let o = owner.clone();
    world.raw_set(
        "register_item_use",
        lua.create_function(move |lua, args: (Value, Value)| register_item_use(lua, &o, args))?,
    )?;
    if let Value::Table(map) = env.raw_get::<Value>("map")? {
        map.raw_set("register_terrain", terrain)?;
    }
    env.raw_set("world", world)
}
