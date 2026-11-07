//! The `events` module (§9.1, §9.2): `events.on(name, fn, opts?)`, `events.off(id)` and
//! `events.emit(name, payload?)`.
//!
//! Listeners of one event form a priority-ordered hook chain (§9.4): ascending
//! `opts.priority` (default `0`), then plugin load order, then registration order, with keys
//! built by [`hook_order::chain_key`] exactly like `battle.on`. `events.emit` only **queues**
//! the event (see [`crate::events`]): the engine flushes the queue in the events phase and
//! starts each listener, in chain order, as a scheduler task.

use std::collections::BTreeMap;

use mlua::{Function, Lua, Table, Value};

use super::convert;
use super::facade::{check, fail, name, type_of};
use super::hook_order;
use crate::chain::{ChainKey, HookChain};
use crate::events::{ListenerId, ListenerInfo, ScriptEvent};
use crate::host::ScriptOwner;

struct Listener {
    id: ListenerId,
    event: String,
    owner: ScriptOwner,
    key: ChainKey,
    func: Function,
}

/// App data: listeners and the pending event queue.
#[derive(Default)]
pub(super) struct EventRegistry {
    listeners: Vec<Listener>,
    queue: Vec<ScriptEvent>,
    next_id: ListenerId,
}

fn registry_error() -> mlua::Error {
    mlua::Error::runtime("event registry unavailable")
}

/// Queues `event`.
pub(super) fn emit(lua: &Lua, event: ScriptEvent) {
    if let Some(mut reg) = lua.app_data_mut::<EventRegistry>() {
        reg.queue.push(event);
    }
}

/// Takes the queued events, in emission order.
pub(super) fn take(lua: &Lua) -> Vec<ScriptEvent> {
    lua.app_data_mut::<EventRegistry>()
        .map(|mut reg| std::mem::take(&mut reg.queue))
        .unwrap_or_default()
}

/// Listeners of `event`, in chain order (priority, load order, registration).
pub(super) fn listeners(lua: &Lua, event: &str) -> Vec<ListenerInfo> {
    let Some(reg) = lua.app_data_ref::<EventRegistry>() else {
        return Vec::new();
    };
    let mut found: Vec<&Listener> = reg.listeners.iter().filter(|l| l.event == event).collect();
    found.sort_by_key(|l| l.key);
    found
        .into_iter()
        .map(|l| ListenerInfo {
            id: l.id,
            owner: l.owner.clone(),
        })
        .collect()
}

/// Every event's listeners as a chain of `(owner, priority)`, by event name (for the resolved
/// hook table).
pub(super) fn chains(lua: &Lua) -> BTreeMap<String, HookChain<(ScriptOwner, i64)>> {
    let mut chains: BTreeMap<String, HookChain<(ScriptOwner, i64)>> = BTreeMap::new();
    if let Some(reg) = lua.app_data_ref::<EventRegistry>() {
        for l in &reg.listeners {
            chains
                .entry(l.event.clone())
                .or_default()
                .insert(l.key, (l.owner.clone(), l.key.priority));
        }
    }
    chains
}

/// The function of listener `id`.
pub(super) fn function(lua: &Lua, id: ListenerId) -> Option<Function> {
    let reg = lua.app_data_ref::<EventRegistry>()?;
    reg.listeners
        .iter()
        .find(|l| l.id == id)
        .map(|l| l.func.clone())
}

/// Drops every listener registered by `owner`; returns how many.
pub(super) fn remove_owner(lua: &Lua, owner: &ScriptOwner) -> usize {
    let Some(mut reg) = lua.app_data_mut::<EventRegistry>() else {
        return 0;
    };
    let before = reg.listeners.len();
    reg.listeners.retain(|l| &l.owner != owner);
    before - reg.listeners.len()
}

/// Installs `events` into `env` for `owner`.
pub(super) fn install(lua: &Lua, env: &Table, owner: &ScriptOwner) -> mlua::Result<()> {
    let events = lua.create_table()?;
    let owner = owner.clone();
    events.raw_set(
        "on",
        lua.create_function(move |lua, (event, func, opts): (Value, Value, Value)| {
            let event = check(lua, name("events.on", "event name", &event))?;
            let Value::Function(func) = func else {
                return Err(fail(
                    lua,
                    format!(
                        "events.on: handler must be a function (got {})",
                        type_of(&func)
                    ),
                ));
            };
            let opts = match opts {
                Value::Nil => None,
                Value::Table(t) => Some(t),
                other => {
                    return Err(fail(
                        lua,
                        format!(
                            "events.on: options must be a table like {{ priority = 0 }} (got {})",
                            type_of(&other)
                        ),
                    ));
                }
            };
            let priority = hook_order::parse_priority(opts.as_ref())
                .map_err(|_| fail(lua, "events.on: options.priority must be an integer"))?;
            let key = hook_order::chain_key(lua, &owner, priority);
            let mut reg = lua
                .app_data_mut::<EventRegistry>()
                .ok_or_else(registry_error)?;
            reg.next_id += 1;
            let id = reg.next_id;
            reg.listeners.push(Listener {
                id,
                event,
                owner: owner.clone(),
                key,
                func,
            });
            Ok(id)
        })?,
    )?;
    events.raw_set(
        "off",
        lua.create_function(|lua, id: Value| {
            let id = match id {
                Value::Integer(i) => ListenerId::try_from(i).ok(),
                _ => None,
            };
            let Some(id) = id else {
                return Err(fail(
                    lua,
                    "events.off: expected the id returned by events.on",
                ));
            };
            let mut reg = lua
                .app_data_mut::<EventRegistry>()
                .ok_or_else(registry_error)?;
            let before = reg.listeners.len();
            reg.listeners.retain(|l| l.id != id);
            Ok(reg.listeners.len() != before)
        })?,
    )?;
    events.raw_set(
        "emit",
        lua.create_function(|lua, (event, payload): (Value, Value)| {
            let event = check(lua, name("events.emit", "event name", &event))?;
            let payload = convert::from_lua(&payload)
                .map_err(|e| fail(lua, format!("events.emit: payload {e}")))?;
            emit(lua, ScriptEvent::new(event, payload));
            Ok(())
        })?,
    )?;
    env.raw_set("events", events)
}
