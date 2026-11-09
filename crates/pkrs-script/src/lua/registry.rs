//! Plugin registrations kept by the Lua host (§9.4): the content database, data effect
//! primitives, terrain rules, field actions, input actions, battle action ids and each
//! plugin's save table, plus the engine-facing [`LuaScriptHost`] API over them.

use std::collections::{BTreeMap, BTreeSet};

use mlua::{Function, Lua, Table, Value};
use pkrs_core::is_action_name;
use pkrs_data::{ContentDb, ContentSource};
use pkrs_world::{TerrainQuery, TerrainRules, TerrainStepVerdict};

use super::{LuaScriptHost, convert, engine};
use crate::chain::ChainKey;
use crate::error::{Result, ScriptError};
use crate::host::{ScriptOwner, ScriptValue, TaskId, TaskStatus};
use crate::registry::{
    FieldActionInfo, FieldActionSource, FieldContext, InputActionRequest, ItemUseContext,
    ItemUseInfo,
};

/// A field action and its functions.
pub(super) struct FieldActionEntry {
    pub key: ChainKey,
    pub info: FieldActionInfo,
    pub available: Option<Function>,
    pub run: Function,
}

/// Item field-use hook.
pub(super) struct ItemUseEntry {
    pub key: ChainKey,
    pub info: ItemUseInfo,
    pub run: Function,
}

/// App data: everything plugins registered besides hooks.
#[derive(Default)]
pub(super) struct Registrations {
    pub content: ContentDb,
    pub effects: BTreeMap<String, Function>,
    pub terrain_rules: BTreeMap<String, (ScriptOwner, Function)>,
    /// Sorted by chain key (priority, load order, registration).
    pub field_actions: Vec<FieldActionEntry>,
    pub item_uses: Vec<ItemUseEntry>,
    pub input_actions: Vec<InputActionRequest>,
    pub input_taken: usize,
    pub battle_actions: BTreeSet<String>,
    pub saves: BTreeMap<String, Table>,
}

fn missing() -> mlua::Error {
    mlua::Error::runtime("plugin registry unavailable")
}

/// Runs `f` on the registrations.
pub(super) fn with_regs<R>(lua: &Lua, f: impl FnOnce(&mut Registrations) -> R) -> mlua::Result<R> {
    let mut regs = lua.app_data_mut::<Registrations>().ok_or_else(missing)?;
    Ok(f(&mut regs))
}

/// Content source for scripts of `owner`: plugins are themselves, everything else is the game.
pub(super) fn content_source(owner: &ScriptOwner) -> ContentSource {
    match owner {
        ScriptOwner::Plugin(id) => ContentSource::Plugin(id.clone()),
        ScriptOwner::Game | ScriptOwner::Map(_) => ContentSource::Game,
    }
}

/// Checks a registration id: `snake_case` or `plugin_id:snake_case`, and a namespace may only
/// be the registering plugin's own.
pub(super) fn check_id(
    func: &str,
    what: &str,
    id: &str,
    owner: &ScriptOwner,
) -> Result<(), String> {
    if !is_action_name(id) {
        return Err(format!(
            "{func}: {what} `{id}` must be snake_case or `plugin_id:snake_case`"
        ));
    }
    match (id.split_once(':'), owner) {
        (None, _) => Ok(()),
        (Some((ns, _)), ScriptOwner::Plugin(p)) if p == ns => Ok(()),
        (Some((ns, _)), _) => Err(format!(
            "{func}: {owner} cannot register in the `{ns}:` namespace"
        )),
    }
}

/// `true` unless `value` is `nil` or `false`.
fn truthy(value: &Value) -> bool {
    !matches!(value, Value::Nil | Value::Boolean(false))
}

impl LuaScriptHost {
    /// Runs `f` on the merged content database (data folders plus `data.register_*`).
    pub fn with_content<R>(&self, f: impl FnOnce(&ContentDb) -> R) -> R {
        match self.lua.app_data_ref::<Registrations>() {
            Some(regs) => f(&regs.content),
            None => f(&ContentDb::default()),
        }
    }

    /// Ids of the data effect primitives that have a Lua implementation, sorted.
    #[must_use]
    pub fn effect_handlers(&self) -> Vec<String> {
        self.lua
            .app_data_ref::<Registrations>()
            .map(|r| r.effects.keys().cloned().collect())
            .unwrap_or_default()
    }

    /// Runs the Lua implementation of effect primitive `id` with `args`.
    pub fn run_effect(&self, id: &str, args: &[ScriptValue]) -> Result<ScriptValue> {
        let func = self
            .lua
            .app_data_ref::<Registrations>()
            .and_then(|r| r.effects.get(id).cloned())
            .ok_or_else(|| ScriptError::NoSuchFunction {
                owner: "data effects".into(),
                name: id.to_owned(),
            })?;
        let args = args
            .iter()
            .map(|a| convert::to_lua(&self.lua, a))
            .collect::<mlua::Result<mlua::MultiValue>>()
            .map_err(engine)?;
        let value = self.protected(&format!("effect:{id}"), |_| func.call::<Value>(args))?;
        convert::from_lua(&value)
    }

    /// Terrain names with a Lua rule (`world.register_terrain`), sorted.
    #[must_use]
    pub fn terrain_rules(&self) -> Vec<String> {
        self.lua
            .app_data_ref::<Registrations>()
            .map(|r| r.terrain_rules.keys().cloned().collect())
            .unwrap_or_default()
    }

    /// Every registered field action, in chain order (priority, load order, registration).
    #[must_use]
    pub fn field_actions(&self) -> Vec<FieldActionInfo> {
        self.lua
            .app_data_ref::<Registrations>()
            .map(|r| r.field_actions.iter().map(|e| e.info.clone()).collect())
            .unwrap_or_default()
    }

    /// The field actions usable in `ctx`, in chain order. From the party menu: actions with
    /// `menu ~= false`; facing a tile: actions bound to that tile's terrain. An action with
    /// a `terrain` always needs the player to face that terrain, and its `available(ctx)`
    /// (if any) must return a truthy value (a failing `available` counts as `false`).
    pub fn available_field_actions(&self, ctx: &FieldContext) -> Vec<FieldActionInfo> {
        let candidates: Vec<(FieldActionInfo, Option<Function>)> = self
            .lua
            .app_data_ref::<Registrations>()
            .map(|r| {
                r.field_actions
                    .iter()
                    .map(|e| (e.info.clone(), e.available.clone()))
                    .collect()
            })
            .unwrap_or_default();
        let ctx_value = ctx.to_value();
        candidates
            .into_iter()
            .filter(|(info, _)| match ctx.source {
                FieldActionSource::PartyMenu => info.menu,
                FieldActionSource::FacingTile => info.terrain.is_some(),
            })
            .filter(|(info, _)| {
                info.terrain
                    .as_ref()
                    .is_none_or(|t| ctx.target_terrain.as_ref() == Some(t))
            })
            .filter(|(info, available)| {
                let Some(available) = available else {
                    return true;
                };
                self.protected(&format!("field_action:{}:available", info.id), |lua| {
                    available.call::<Value>(convert::to_lua(lua, &ctx_value)?)
                })
                .is_ok_and(|v| truthy(&v))
            })
            .map(|(info, _)| info)
            .collect()
    }

    /// Starts field action `id`'s `run(ctx)` as a task. The caller hands the task to the
    /// scheduler (`Scheduler::adopt`, usually as `TaskKind::Interact`). Availability is not
    /// re-checked; use [`available_field_actions`](Self::available_field_actions).
    pub fn start_field_action(
        &mut self,
        id: &str,
        ctx: &FieldContext,
    ) -> Result<(FieldActionInfo, TaskId, TaskStatus)> {
        let found = self.lua.app_data_ref::<Registrations>().and_then(|r| {
            r.field_actions
                .iter()
                .find(|e| e.info.id == id)
                .map(|e| (e.info.clone(), e.run.clone()))
        });
        let Some((info, run)) = found else {
            return Err(ScriptError::NoSuchFunction {
                owner: "field actions".into(),
                name: id.to_owned(),
            });
        };
        let (task, status) = self.start_thread(run, &[ctx.to_value()])?;
        Ok((info, task, status))
    }

    /// `true` when any `world.register_item_use` hook matches `item_id` (exact id or prefix).
    #[must_use]
    pub fn has_item_use_hook(&self, item_id: &str) -> bool {
        self.lua.app_data_ref::<Registrations>().is_some_and(|r| {
            r.item_uses
                .iter()
                .any(|e| item_use_matches(&e.info.id, item_id))
        })
    }

    /// Runs the first matching item-use hook for `ctx`; returns `true` when a hook handled use.
    pub fn run_item_use(&mut self, ctx: &ItemUseContext) -> bool {
        let run = self.lua.app_data_ref::<Registrations>().and_then(|r| {
            r.item_uses
                .iter()
                .find(|e| item_use_matches(&e.info.id, &ctx.item_id))
                .map(|e| e.run.clone())
        });
        let Some(run) = run else {
            return false;
        };
        let ctx_value = ctx.to_value();
        self.protected(&format!("item_use:{}", ctx.item_id), |lua| {
            run.call::<Value>(convert::to_lua(lua, &ctx_value)?)
        })
        .is_ok_and(|v| truthy(&v))
    }

    /// Input actions registered since the last call (`input.register_action`), in order.
    pub fn take_input_actions(&mut self) -> Vec<InputActionRequest> {
        self.lua
            .app_data_mut::<Registrations>()
            .map(|mut r| {
                let start = r.input_taken.min(r.input_actions.len());
                r.input_taken = r.input_actions.len();
                r.input_actions[start..].to_vec()
            })
            .unwrap_or_default()
    }

    /// Every registered input action.
    #[must_use]
    pub fn input_actions(&self) -> Vec<InputActionRequest> {
        self.lua
            .app_data_ref::<Registrations>()
            .map(|r| r.input_actions.clone())
            .unwrap_or_default()
    }

    /// Ids registered with `battle.register_action`, sorted.
    #[must_use]
    pub fn battle_actions(&self) -> Vec<String> {
        self.lua
            .app_data_ref::<Registrations>()
            .map(|r| r.battle_actions.iter().cloned().collect())
            .unwrap_or_default()
    }

    /// Snapshot of plugin `id`'s save namespace (`plugin.save`), or `None` if the plugin is
    /// not bound. Values that cannot be saved (functions, userdata, cycles) are skipped and
    /// logged; the rest is kept (§9.6). Writing it to disk is the save system's job (§12).
    #[must_use]
    pub fn plugin_save(&self, id: &str) -> Option<ScriptValue> {
        let table = self
            .lua
            .app_data_ref::<Registrations>()
            .and_then(|r| r.saves.get(id).cloned())?;
        Some(save_snapshot(id, &table))
    }

    /// [`plugin_save`](Self::plugin_save) for every bound plugin, by plugin id.
    #[must_use]
    pub fn plugin_saves(&self) -> BTreeMap<String, ScriptValue> {
        let tables: Vec<(String, Table)> = self
            .lua
            .app_data_ref::<Registrations>()
            .map(|r| {
                r.saves
                    .iter()
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect()
            })
            .unwrap_or_default();
        tables
            .into_iter()
            .map(|(id, t)| {
                let snap = save_snapshot(&id, &t);
                (id, snap)
            })
            .collect()
    }

    /// Replaces plugin `id`'s save namespace with `value` (a table, or `nil` to clear it),
    /// keeping the table scripts already hold (e.g. after loading a save).
    pub fn restore_plugin_save(&mut self, id: &str, value: &ScriptValue) -> Result<()> {
        let table = self
            .lua
            .app_data_ref::<Registrations>()
            .and_then(|r| r.saves.get(id).cloned())
            .ok_or_else(|| ScriptError::Engine(format!("plugin `{id}` is not bound")))?;
        let entries = match value {
            ScriptValue::Nil => BTreeMap::new(),
            ScriptValue::Map(m) => m.clone(),
            ScriptValue::List(items) => items
                .iter()
                .enumerate()
                .map(|(i, v)| ((i + 1).to_string(), v.clone()))
                .collect(),
            other => {
                return Err(ScriptError::Conversion(format!(
                    "plugin save must be a table, got {other:?}"
                )));
            }
        };
        let fill = || -> mlua::Result<()> {
            table.clear()?;
            for (k, v) in &entries {
                table.raw_set(k.as_str(), convert::to_lua(&self.lua, v)?)?;
            }
            Ok(())
        };
        fill().map_err(engine)
    }
}

/// Keeps only saveable values, logging what was dropped as `plugin.save.<path>`.
fn sanitize(value: ScriptValue, path: &str, plugin: &str) -> Option<ScriptValue> {
    match value {
        ScriptValue::Opaque(kind) => {
            tracing::warn!(target: "script", plugin, key = path, "plugin save: skipped {kind} value");
            None
        }
        ScriptValue::Npc(_) => {
            tracing::warn!(target: "script", plugin, key = path, "plugin save: skipped npc handle");
            None
        }
        ScriptValue::List(items) => Some(ScriptValue::List(
            items
                .into_iter()
                .enumerate()
                .filter_map(|(i, v)| sanitize(v, &format!("{path}[{}]", i + 1), plugin))
                .collect(),
        )),
        ScriptValue::Map(map) => Some(ScriptValue::Map(
            map.into_iter()
                .filter_map(|(k, v)| {
                    let child = format!("{path}.{k}");
                    sanitize(v, &child, plugin).map(|v| (k, v))
                })
                .collect(),
        )),
        other => Some(other),
    }
}

fn save_snapshot(plugin: &str, table: &Table) -> ScriptValue {
    let mut out = BTreeMap::new();
    for pair in table.pairs::<Value, Value>() {
        let Ok((key, value)) = pair else { continue };
        let key = match key {
            Value::String(s) => s.to_string_lossy(),
            Value::Integer(i) => i.to_string(),
            other => {
                tracing::warn!(target: "script", plugin, "plugin save: skipped a {} key", other.type_name());
                continue;
            }
        };
        let path = format!("plugin.save.{key}");
        match convert::from_lua(&value) {
            Ok(v) => {
                if let Some(v) = sanitize(v, &path, plugin) {
                    out.insert(key, v);
                }
            }
            Err(e) => {
                tracing::warn!(target: "script", plugin, key = %path, "plugin save: skipped: {e}");
            }
        }
    }
    ScriptValue::Map(out)
}

impl TerrainRules for LuaScriptHost {
    /// A Lua rule (`world.register_terrain`) decides first: returning `false` or `"block"`
    /// blocks, anything else (including `nil`) allows; a failing rule is logged and allows.
    /// Otherwise a data-defined terrain applies `passable` and `requires_field`. Unknown
    /// custom terrains allow the step.
    fn verdict(&self, query: &TerrainQuery<'_>) -> TerrainStepVerdict {
        let (rule, data) = match self.lua.app_data_ref::<Registrations>() {
            Some(r) => (
                r.terrain_rules.get(query.terrain).map(|(_, f)| f.clone()),
                r.content.terrain(query.terrain).cloned(),
            ),
            None => (None, None),
        };
        if let Some(rule) = rule {
            let ctx = terrain_ctx(query);
            let result = self.protected(&format!("terrain:{}", query.terrain), |lua| {
                rule.call::<Value>(convert::to_lua(lua, &ctx)?)
            });
            return match result {
                Ok(Value::Boolean(false)) => TerrainStepVerdict::Block,
                Ok(Value::String(s)) if s.to_string_lossy() == "block" => TerrainStepVerdict::Block,
                _ => TerrainStepVerdict::Allow,
            };
        }
        match data {
            Some(def) if !def.passable => TerrainStepVerdict::Block,
            Some(def)
                if def
                    .requires_field
                    .as_deref()
                    .is_some_and(|s| !query.field.is_active(s)) =>
            {
                TerrainStepVerdict::Block
            }
            _ => TerrainStepVerdict::Allow,
        }
    }
}

fn item_use_matches(registered: &str, item_id: &str) -> bool {
    registered == item_id || (registered.ends_with(':') && item_id.starts_with(registered))
}

/// `{ terrain, x, y, from = { x, y }, dir, field = { [state] = true } }`.
fn terrain_ctx(q: &TerrainQuery<'_>) -> ScriptValue {
    let int = |n: i32| ScriptValue::Int(i64::from(n));
    let mut from = BTreeMap::new();
    from.insert("x".to_owned(), int(q.from.x));
    from.insert("y".to_owned(), int(q.from.y));
    let field = q
        .field
        .active()
        .into_iter()
        .map(|s| (s.to_owned(), ScriptValue::Bool(true)))
        .collect();
    let mut m = BTreeMap::new();
    m.insert("terrain".to_owned(), ScriptValue::from(q.terrain));
    m.insert("x".to_owned(), int(q.to.x));
    m.insert("y".to_owned(), int(q.to.y));
    m.insert("from".to_owned(), ScriptValue::Map(from));
    m.insert("dir".to_owned(), ScriptValue::String(q.dir.to_string()));
    m.insert("field".to_owned(), ScriptValue::Map(field));
    ScriptValue::Map(m)
}
