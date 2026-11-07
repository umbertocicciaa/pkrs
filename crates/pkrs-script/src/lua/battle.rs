//! `LuaBattleHooks`: implements the battle [`BattleHooks`] port by dispatching to Lua handlers
//! registered with `battle.on(...)` (§7.4, §9.1), plus `battle.register_action` (custom
//! fight-menu actions, §9.4) and `battle.start_wild` / `start_trainer`.
//!
//! **Calling convention** (mirrors §7.4's Lua hook list): every handler gets `ctx` first, then
//! hook-specific arguments. Returning `nil` leaves the value unchanged; returning a value
//! replaces it; cancellable hooks return `{ cancel = true, message = "text:…" }`, which also
//! stops the chain. Handlers run in ascending `priority` (default 0), then plugin load order,
//! then registration order (the [`crate::chain`] engine, §9.4).
//!
//! **Failure policy** (§9.6): a handler that errors (or returns a malformed value) is skipped
//! for that call — the value passes through unchanged and any `ctx` changes it made are rolled
//! back — a warning is logged, and after `max_failures` consecutive failures it is disabled.

use std::cell::RefCell;
use std::collections::BTreeMap;

use mlua::{Function, Lua, MultiValue, Table, Value};
use pkrs_battle::{
    AccuracyCalc, ActionMenu, ActionOrder, BattleHookCtx, BattleHooks, BattleOutcome, BattleResult,
    BattleValue, BattleView, CreatureView, CustomAction, DamageCalc, DamageTaken, FaintEvent,
    HookResult, MenuOption, MoveRequest, MoveResult, SideId, SlotRef, StatusApply, SwitchIn,
    SwitchOut,
};
use pkrs_data::BattlePack;

use super::facade::{check, fail, name, type_of};
use super::registry::{check_id, with_regs};
use super::{LuaScriptHost, hook_order};
use crate::chain::{HookChain, Step, run_chain};
use crate::host::ScriptOwner;

/// Hook names accepted by `battle.on` (API level 1, §7.4).
pub const HOOK_NAMES: &[&str] = &[
    "on_battle_start",
    "on_turn_start",
    "on_pre_turn",
    "on_action_order",
    "on_before_move",
    "on_accuracy",
    "on_damage_calc",
    "on_damage_taken",
    "on_after_move",
    "on_status_apply",
    "on_switch_in",
    "on_switch_out",
    "on_faint",
    "on_end_of_turn",
    "on_battle_end",
    "on_action_menu",
    "on_custom_action",
];

/// Default number of consecutive failures before a handler is disabled.
pub const DEFAULT_MAX_FAILURES: u32 = 3;

pub(super) struct Handler {
    id: u64,
    owner: ScriptOwner,
    func: Function,
    failures: u32,
    disabled: bool,
}

impl Handler {
    pub(super) fn owner(&self) -> &ScriptOwner {
        &self.owner
    }

    pub(super) fn is_disabled(&self) -> bool {
        self.disabled
    }
}

/// App data: every registered battle handler, one priority-ordered chain per hook.
#[derive(Default)]
pub(super) struct Registry {
    chains: BTreeMap<&'static str, HookChain<Handler>>,
}

impl Registry {
    /// Hook name → chain, sorted by hook name.
    pub(super) fn chains(&self) -> impl Iterator<Item = (&'static str, &HookChain<Handler>)> {
        self.chains.iter().map(|(k, c)| (*k, c))
    }
}

/// `battle.start_wild` / `battle.start_trainer`: yield a battle request to the engine and
/// return the result it resumes the task with.
const START_FNS: &str = r#"
local yield, isyieldable = ...
local function need_task(name)
  if not isyieldable() then
    error("battle." .. name .. " must be called from a script task (coroutine)", 3)
  end
end
local function start_wild(species, level)
  if type(species) ~= "string" then error("battle.start_wild: species must be a string", 2) end
  need_task("start_wild")
  return yield({ wait = "battle", kind = "wild", species = species, level = level or 5 })
end
local function start_trainer(id)
  if type(id) ~= "string" then error("battle.start_trainer: trainer id must be a string", 2) end
  need_task("start_trainer")
  return yield({ wait = "battle", kind = "trainer", id = id })
end
return start_wild, start_trainer
"#;

/// Adds `func` to `hook`'s chain for `owner`; returns the handler id.
fn add_handler(
    lua: &Lua,
    owner: &ScriptOwner,
    hook: &'static str,
    func: Function,
    priority: i64,
) -> mlua::Result<u64> {
    let key = hook_order::chain_key(lua, owner, priority);
    let mut reg = lua
        .app_data_mut::<Registry>()
        .ok_or_else(|| mlua::Error::runtime("battle registry unavailable"))?;
    reg.chains.entry(hook).or_default().insert(
        key,
        Handler {
            id: key.seq,
            owner: owner.clone(),
            func,
            failures: 0,
            disabled: false,
        },
    );
    Ok(key.seq)
}

/// Builds the `on_action_menu` / `on_custom_action` pair behind `battle.register_action`.
const ACTION_FNS: &str = r#"
local id, label, available, run = ...
local function menu(ctx, side, options)
  if available == nil or available(ctx, side) then
    options[#options + 1] = { id = id, label = label }
  end
  return options
end
local function exec(ctx, action_id, payload)
  if action_id ~= id then return nil end
  local handled = run(ctx, payload)
  if handled == nil then return true end
  return handled
end
return menu, exec
"#;

/// `battle.register_action(id, { label?, available?(ctx, side), run(ctx, payload),
/// priority? })`: a new fight-menu entry and its execution, as sugar over `on_action_menu`
/// and `on_custom_action` (§9.4). `run` returning `nil` counts as handled.
fn register_action(
    lua: &Lua,
    owner: &ScriptOwner,
    (id, def): (Value, Value),
) -> mlua::Result<String> {
    const FUNC: &str = "battle.register_action";
    let id = check(lua, name(FUNC, "action id", &id))?;
    check(lua, check_id(FUNC, "action id", &id, owner))?;
    if ActionMenu::builtin(SlotRef::new(SideId::Player, 0)).contains(&id) {
        return Err(fail(
            lua,
            format!("{FUNC}: `{id}` is a built-in menu entry"),
        ));
    }
    let Value::Table(def) = def else {
        return Err(fail(
            lua,
            format!("{FUNC}: definition must be a table (got {})", type_of(&def)),
        ));
    };
    let run = match def.raw_get::<Value>("run")? {
        Value::Function(f) => f,
        other => {
            return Err(fail(
                lua,
                format!("{FUNC}: `run` must be a function (got {})", type_of(&other)),
            ));
        }
    };
    let available = match def.raw_get::<Value>("available")? {
        Value::Nil => None,
        Value::Function(f) => Some(f),
        other => {
            return Err(fail(
                lua,
                format!(
                    "{FUNC}: `available` must be a function (got {})",
                    type_of(&other)
                ),
            ));
        }
    };
    let label = match def.raw_get::<Value>("label")? {
        Value::Nil => id.clone(),
        v => check(lua, name(FUNC, "`label`", &v))?,
    };
    let priority = hook_order::parse_priority(Some(&def))
        .map_err(|_| fail(lua, format!("{FUNC}: `priority` must be an integer")))?;
    let fresh = with_regs(lua, |r| r.battle_actions.insert(id.clone()))?;
    if !fresh {
        return Err(fail(
            lua,
            format!("{FUNC}: battle action `{id}` is already registered"),
        ));
    }
    let (menu, exec): (Function, Function) = lua
        .load(ACTION_FNS)
        .set_name("=pkrs-battle-action")
        .call((id.as_str(), label, available, run))?;
    add_handler(lua, owner, "on_action_menu", menu, priority)?;
    add_handler(lua, owner, "on_custom_action", exec, priority)?;
    Ok(id)
}

/// Installs the `battle` module into `env`.
pub(super) fn install(lua: &Lua, env: &Table, owner: &ScriptOwner) -> mlua::Result<()> {
    let battle = lua.create_table()?;
    let owner = owner.clone();
    let on_owner = owner.clone();
    let on = lua.create_function(
        move |lua, (name, func, opts): (String, Function, Option<Table>)| {
            let Some(hook) = HOOK_NAMES.iter().find(|h| **h == name).copied() else {
                return Err(mlua::Error::runtime(format!(
                    "battle.on: unknown hook '{name}'"
                )));
            };
            let priority = hook_order::parse_priority(opts.as_ref())?;
            add_handler(lua, &on_owner, hook, func, priority)
        },
    )?;
    battle.raw_set("on", on)?;
    let action_owner = owner.clone();
    battle.raw_set(
        "register_action",
        lua.create_function(move |lua, args: (Value, Value)| {
            register_action(lua, &action_owner, args)
        })?,
    )?;
    let co: Table = env.raw_get("coroutine")?;
    let (wild, trainer): (Function, Function) =
        lua.load(START_FNS).set_name("=pkrs-battle").call((
            co.raw_get::<Function>("yield")?,
            co.raw_get::<Function>("isyieldable")?,
        ))?;
    battle.raw_set("start_wild", wild)?;
    battle.raw_set("start_trainer", trainer)?;
    env.raw_set("battle", battle)
}

// --- value marshalling -------------------------------------------------------------------

fn side_name(side: SideId) -> &'static str {
    match side {
        SideId::Player => "player",
        SideId::Opponent => "opponent",
    }
}

fn parse_side(v: &Value) -> mlua::Result<SideId> {
    let name = match v {
        Value::String(s) => s.to_str()?.to_owned(),
        Value::Table(t) => t.raw_get::<String>("side")?,
        other => {
            return Err(mlua::Error::runtime(format!(
                "expected a side ('player' / 'opponent' or a side table), got {}",
                other.type_name()
            )));
        }
    };
    match name.as_str() {
        "player" => Ok(SideId::Player),
        "opponent" => Ok(SideId::Opponent),
        other => Err(mlua::Error::runtime(format!("unknown side '{other}'"))),
    }
}

fn parse_slot(v: &Value) -> mlua::Result<SlotRef> {
    let Value::Table(t) = v else {
        return Err(mlua::Error::runtime(format!(
            "expected a creature table, got {}",
            v.type_name()
        )));
    };
    let side = parse_side(&Value::String(t.raw_get("side")?))?;
    let slot: i64 = t.raw_get("slot")?;
    let slot = u8::try_from(slot).map_err(|_| mlua::Error::runtime("slot out of range"))?;
    Ok(SlotRef::new(side, slot))
}

/// [`BattleValue`] → Lua.
pub(super) fn value_to_lua(lua: &Lua, v: &BattleValue) -> mlua::Result<Value> {
    Ok(match v {
        BattleValue::Bool(b) => Value::Boolean(*b),
        BattleValue::Int(i) => Value::Integer(*i),
        BattleValue::Str(s) => Value::String(lua.create_string(s)?),
        BattleValue::List(items) => {
            let t = lua.create_table()?;
            for (i, item) in items.iter().enumerate() {
                t.raw_set(i + 1, value_to_lua(lua, item)?)?;
            }
            Value::Table(t)
        }
        BattleValue::Map(map) => {
            let t = lua.create_table()?;
            for (k, item) in map {
                t.raw_set(k.as_str(), value_to_lua(lua, item)?)?;
            }
            Value::Table(t)
        }
    })
}

/// Lua → [`BattleValue`]. Battle state stays exact for replays, so non-integer numbers,
/// functions and userdata are rejected.
pub(super) fn value_from_lua(v: &Value) -> mlua::Result<BattleValue> {
    value_from_lua_depth(v, 0)
}

fn value_from_lua_depth(v: &Value, depth: usize) -> mlua::Result<BattleValue> {
    if depth > 16 {
        return Err(mlua::Error::runtime(
            "battle value nested too deeply (or cyclic)",
        ));
    }
    Ok(match v {
        Value::Boolean(b) => BattleValue::Bool(*b),
        Value::Integer(i) => BattleValue::Int(*i),
        Value::Number(n) if n.fract() == 0.0 && n.is_finite() => BattleValue::Int(*n as i64),
        Value::Number(n) => {
            return Err(mlua::Error::runtime(format!(
                "battle state only stores integers (got {n})"
            )));
        }
        Value::String(s) => BattleValue::Str(s.to_str()?.to_owned()),
        Value::Table(t) => {
            let len = t.raw_len();
            let mut pairs = Vec::new();
            for pair in t.clone().pairs::<Value, Value>() {
                pairs.push(pair?);
            }
            if len > 0 && pairs.len() == len {
                let mut list = Vec::with_capacity(len);
                for i in 1..=len {
                    list.push(value_from_lua_depth(&t.raw_get::<Value>(i)?, depth + 1)?);
                }
                BattleValue::List(list)
            } else {
                let mut map = BTreeMap::new();
                for (k, item) in pairs {
                    let Value::String(k) = k else {
                        return Err(mlua::Error::runtime("battle state tables need string keys"));
                    };
                    map.insert(
                        k.to_str()?.to_owned(),
                        value_from_lua_depth(&item, depth + 1)?,
                    );
                }
                BattleValue::Map(map)
            }
        }
        Value::Nil => {
            return Err(mlua::Error::runtime(
                "nil cannot be stored; use clear_* instead",
            ));
        }
        other => {
            return Err(mlua::Error::runtime(format!(
                "a {} cannot be stored in battle state",
                other.type_name()
            )));
        }
    })
}

fn creature_table(lua: &Lua, slot: SlotRef, c: &CreatureView) -> mlua::Result<Table> {
    let t = lua.create_table()?;
    t.raw_set("side", side_name(slot.side))?;
    t.raw_set("slot", slot.slot)?;
    t.raw_set("is_player", slot.side == SideId::Player)?;
    t.raw_set("party_index", c.party_index)?;
    t.raw_set("species", c.species_id.as_str())?;
    t.raw_set("name", c.nickname.as_str())?;
    t.raw_set("level", c.level)?;
    t.raw_set("hp", c.hp)?;
    t.raw_set("max_hp", c.stats.max_hp)?;
    let types = lua.create_table()?;
    for (i, ty) in c.types.iter().enumerate() {
        types.raw_set(i + 1, ty.as_str())?;
    }
    t.raw_set("types", types)?;
    let stats = lua.create_table()?;
    stats.raw_set("attack", c.stats.attack)?;
    stats.raw_set("defense", c.stats.defense)?;
    stats.raw_set("speed", c.stats.speed)?;
    t.raw_set("stats", stats)?;
    Ok(t)
}

fn creature_at(lua: &Lua, view: &BattleView, slot: SlotRef) -> mlua::Result<Value> {
    match view.creature(slot) {
        Some(c) => Ok(Value::Table(creature_table(lua, slot, c)?)),
        None => Ok(Value::Nil),
    }
}

fn move_table(lua: &Lua, pack: Option<&BattlePack>, id: &str) -> mlua::Result<Table> {
    let t = lua.create_table()?;
    t.raw_set("id", id)?;
    if let Some(def) = pack.and_then(|p| p.move_by_id(id)) {
        t.raw_set("type", def.type_id.as_str())?;
        t.raw_set(
            "category",
            match def.category {
                pkrs_data::MoveCategory::Physical => "physical",
                pkrs_data::MoveCategory::Special => "special",
                pkrs_data::MoveCategory::Status => "status",
            },
        )?;
        t.raw_set("power", def.power)?;
        t.raw_set("accuracy", def.accuracy)?;
        t.raw_set("priority", def.priority)?;
    }
    Ok(t)
}

/// `(user, move, target)` for the move being executed, as Lua values.
fn move_args(
    lua: &Lua,
    view: &BattleView,
    pack: Option<&BattlePack>,
) -> mlua::Result<(Value, Value, Value)> {
    let Some(cur) = view.current_move.clone() else {
        return Ok((Value::Nil, Value::Nil, Value::Nil));
    };
    Ok((
        creature_at(lua, view, cur.user)?,
        Value::Table(move_table(lua, pack, &cur.move_id)?),
        creature_at(lua, view, cur.target)?,
    ))
}

/// `{ cancel = true, message = … }` → `Some(Cancel)`.
fn cancel_of(v: &Value) -> mlua::Result<Option<HookResult>> {
    let Value::Table(t) = v else { return Ok(None) };
    if t.raw_get::<Option<bool>>("cancel")?.unwrap_or(false) {
        return Ok(Some(HookResult::Cancel {
            reason: t.raw_get::<Option<String>>("message")?,
        }));
    }
    Ok(None)
}

/// Strips a leading `ctx` table so both `ctx.f(x)` and `ctx:f(x)` work.
fn args_without_self(ctx_tbl: &Table, args: MultiValue) -> Vec<Value> {
    let mut v: Vec<Value> = args.into_iter().collect();
    if let Some(Value::Table(first)) = v.first() {
        if first == ctx_tbl {
            v.remove(0);
        }
    }
    v
}

fn arg(args: &[Value], i: usize) -> Value {
    args.get(i).cloned().unwrap_or(Value::Nil)
}

/// Fills `t` with the hook `ctx` API bound to `cell` for the duration of `scope`.
fn install_ctx<'scope, 'env: 'scope, 'c: 'env>(
    scope: &'scope mlua::Scope<'scope, 'env>,
    t: &Table,
    cell: &'env RefCell<&'env mut BattleHookCtx<'c>>,
) -> mlua::Result<()> {
    {
        let ctx = cell.borrow();
        t.raw_set("turn", ctx.turn())?;
        t.raw_set("phase", format!("{:?}", ctx.phase()))?;
        t.raw_set("side_size", ctx.side_size())?;
    }
    let me = t.clone();
    t.raw_set(
        "random",
        scope.create_function(move |_, args: MultiValue| {
            let args = args_without_self(&me, args);
            let n = match arg(&args, 0) {
                Value::Integer(n) if n > 0 => n,
                Value::Number(n) if n >= 1.0 => n as i64,
                _ => return Err(mlua::Error::runtime("ctx:random(n) needs n >= 1")),
            };
            let n =
                u32::try_from(n).map_err(|_| mlua::Error::runtime("ctx:random: n too large"))?;
            Ok(i64::from(cell.borrow_mut().random(n)) + 1)
        })?,
    )?;
    let me = t.clone();
    t.raw_set(
        "message",
        scope.create_function(move |_, args: MultiValue| {
            let args = args_without_self(&me, args);
            let Value::String(key) = arg(&args, 0) else {
                return Err(mlua::Error::runtime("ctx.message(key) needs a string key"));
            };
            cell.borrow_mut().message(key.to_str()?.to_owned());
            Ok(())
        })?,
    )?;
    let me = t.clone();
    t.raw_set(
        "play_fx",
        scope.create_function(move |_, args: MultiValue| {
            let args = args_without_self(&me, args);
            let Value::String(id) = arg(&args, 0) else {
                return Err(mlua::Error::runtime(
                    "ctx.play_fx(id, creature) needs a string id",
                ));
            };
            cell.borrow_mut().message(format!("fx:{}", id.to_str()?));
            Ok(())
        })?,
    )?;
    let me = t.clone();
    t.raw_set(
        "volatile",
        scope.create_function(move |lua, args: MultiValue| {
            let args = args_without_self(&me, args);
            let slot = parse_slot(&arg(&args, 0))?;
            let key: String = lua.unpack(arg(&args, 1))?;
            match cell.borrow().volatile(slot, &key) {
                Some(v) => value_to_lua(lua, &v),
                None => Ok(Value::Nil),
            }
        })?,
    )?;
    let me = t.clone();
    t.raw_set(
        "set_volatile",
        scope.create_function(move |lua, args: MultiValue| {
            let args = args_without_self(&me, args);
            let slot = parse_slot(&arg(&args, 0))?;
            let key: String = lua.unpack(arg(&args, 1))?;
            let value = value_from_lua(&arg(&args, 2))?;
            cell.borrow_mut().set_volatile(slot, key, value);
            Ok(())
        })?,
    )?;
    let me = t.clone();
    t.raw_set(
        "clear_volatile",
        scope.create_function(move |lua, args: MultiValue| {
            let args = args_without_self(&me, args);
            let slot = parse_slot(&arg(&args, 0))?;
            let key: String = lua.unpack(arg(&args, 1))?;
            cell.borrow_mut().clear_volatile(slot, key);
            Ok(())
        })?,
    )?;
    let me = t.clone();
    t.raw_set(
        "side_state",
        scope.create_function(move |lua, args: MultiValue| {
            let args = args_without_self(&me, args);
            let side = parse_side(&arg(&args, 0))?;
            let key: String = lua.unpack(arg(&args, 1))?;
            match cell.borrow().side_state(side, &key) {
                Some(v) => value_to_lua(lua, &v),
                None => Ok(Value::Nil),
            }
        })?,
    )?;
    let me = t.clone();
    t.raw_set(
        "set_side_state",
        scope.create_function(move |lua, args: MultiValue| {
            let args = args_without_self(&me, args);
            let side = parse_side(&arg(&args, 0))?;
            let key: String = lua.unpack(arg(&args, 1))?;
            let value = value_from_lua(&arg(&args, 2))?;
            cell.borrow_mut().set_side_state(side, key, value);
            Ok(())
        })?,
    )?;
    let me = t.clone();
    t.raw_set(
        "active_creatures",
        scope.create_function(move |lua, args: MultiValue| {
            let _ = args_without_self(&me, args);
            let ctx = cell.borrow();
            let list = lua.create_table()?;
            for (i, (slot, c)) in ctx.view().active_creatures().into_iter().enumerate() {
                list.raw_set(i + 1, creature_table(lua, slot, c)?)?;
            }
            Ok(list)
        })?,
    )?;
    let me = t.clone();
    t.raw_set(
        "creature",
        scope.create_function(move |lua, args: MultiValue| {
            let args = args_without_self(&me, args);
            let side = parse_side(&arg(&args, 0))?;
            let slot: u8 = lua.unpack(arg(&args, 1)).unwrap_or(0);
            creature_at(lua, cell.borrow().view(), SlotRef::new(side, slot))
        })?,
    )?;
    Ok(())
}

/// What a handler did with the chain.
enum Flow {
    Continue,
    Stop,
}

/// [`BattleHooks`] backed by Lua handlers registered with `battle.on` (§7.4).
pub struct LuaBattleHooks<'h> {
    host: &'h LuaScriptHost,
    pack: Option<&'h BattlePack>,
    max_failures: u32,
}

impl<'h> LuaBattleHooks<'h> {
    /// Hooks dispatching into `host`'s registered handlers.
    #[must_use]
    pub fn new(host: &'h LuaScriptHost) -> Self {
        Self {
            host,
            pack: None,
            max_failures: DEFAULT_MAX_FAILURES,
        }
    }

    /// Gives handlers full move tables (`type`, `power`, …) from `pack`.
    #[must_use]
    pub fn with_moves(mut self, pack: &'h BattlePack) -> Self {
        self.pack = Some(pack);
        self
    }

    /// Consecutive failures before a handler is disabled (default
    /// [`DEFAULT_MAX_FAILURES`]; `0` never disables).
    #[must_use]
    pub fn with_max_failures(mut self, n: u32) -> Self {
        self.max_failures = n;
        self
    }

    /// Enabled handlers of `hook`, in chain order. Copied out so no registry borrow is held
    /// while Lua runs (a handler may call `battle.on`).
    fn handlers(&self, hook: &str) -> Vec<(u64, Function, String)> {
        let Some(reg) = self.host.lua.app_data_ref::<Registry>() else {
            return Vec::new();
        };
        let Some(chain) = reg.chains.get(hook) else {
            return Vec::new();
        };
        chain
            .iter()
            .map(|(_, h)| h)
            .filter(|h| !h.disabled)
            .map(|h| (h.id, h.func.clone(), h.owner.to_string()))
            .collect()
    }

    fn record(&self, id: u64, hook: &str, owner: &str, error: Option<&crate::ScriptError>) {
        let Some(mut reg) = self.host.lua.app_data_mut::<Registry>() else {
            return;
        };
        let Some(h) = reg
            .chains
            .values_mut()
            .flat_map(HookChain::handlers_mut)
            .find(|h| h.id == id)
        else {
            return;
        };
        let Some(err) = error else {
            h.failures = 0;
            return;
        };
        h.failures = h.failures.saturating_add(1);
        tracing::warn!(
            target: "script",
            hook,
            owner,
            failures = h.failures,
            "battle hook handler failed and was skipped (value unchanged): {err}"
        );
        if self.max_failures > 0 && h.failures >= self.max_failures {
            h.disabled = true;
            tracing::warn!(
                target: "script",
                hook,
                owner,
                "battle hook handler disabled after {} consecutive failures",
                h.failures
            );
        }
    }

    /// Runs the chain of `hook` ([`run_chain`]). `call` invokes one handler with the ctx table
    /// and applies its result to the payload in place (so the value threads through the chain;
    /// a `nil` return leaves it unchanged); returning [`Flow::Stop`] cancels the rest of the
    /// chain. A failing handler passes through with its `ctx` changes rolled back.
    fn dispatch(
        &mut self,
        hook: &'static str,
        ctx: &mut BattleHookCtx<'_>,
        mut call: impl FnMut(&Lua, &Function, Table, &BattleView) -> mlua::Result<Flow>,
    ) {
        let handlers = self.handlers(hook);
        if handlers.is_empty() {
            return;
        }
        // Arguments are built from a snapshot so no `ctx` borrow is held while Lua runs
        // (handlers call back into `ctx.set_*`).
        let view = ctx.view().clone();
        // The payload is mutated in place by `call`, so the threaded chain value is `()`.
        let _ = run_chain(handlers, (), |(id, func, owner), ()| {
            let checkpoint = ctx.checkpoint();
            let result = {
                let cell = RefCell::new(&mut *ctx);
                self.host.call_protected(hook, |lua| {
                    lua.scope(|scope| {
                        let tbl = lua.create_table()?;
                        install_ctx(scope, &tbl, &cell)?;
                        call(lua, &func, tbl, &view)
                    })
                })
            };
            match result {
                Ok(flow) => {
                    self.record(id, hook, &owner, None);
                    match flow {
                        Flow::Continue => Step::Pass,
                        Flow::Stop => Step::Cancel(None),
                    }
                }
                Err(err) => {
                    ctx.rollback(checkpoint);
                    self.record(id, hook, &owner, Some(&err));
                    Step::Pass
                }
            }
        });
    }

    fn simple(&mut self, hook: &'static str, ctx: &mut BattleHookCtx<'_>) {
        self.dispatch(hook, ctx, |_, f, tbl, _| {
            f.call::<Value>(tbl)?;
            Ok(Flow::Continue)
        });
    }
}

/// Each hook builds its Lua arguments from the view snapshot, calls the handler, then parses
/// the returned value before touching the payload, so a malformed return leaves it unchanged.
impl BattleHooks for LuaBattleHooks<'_> {
    fn on_battle_start(&mut self, ctx: &mut BattleHookCtx<'_>) -> pkrs_battle::Result<()> {
        self.simple("on_battle_start", ctx);
        Ok(())
    }

    fn on_turn_start(&mut self, ctx: &mut BattleHookCtx<'_>) -> pkrs_battle::Result<()> {
        self.simple("on_turn_start", ctx);
        Ok(())
    }

    fn on_pre_turn(&mut self, ctx: &mut BattleHookCtx<'_>) -> pkrs_battle::Result<()> {
        self.simple("on_pre_turn", ctx);
        Ok(())
    }

    fn on_end_of_turn(&mut self, ctx: &mut BattleHookCtx<'_>) -> pkrs_battle::Result<()> {
        self.simple("on_end_of_turn", ctx);
        Ok(())
    }

    fn on_action_order(
        &mut self,
        ctx: &mut BattleHookCtx<'_>,
        order: &mut ActionOrder,
    ) -> pkrs_battle::Result<()> {
        self.dispatch("on_action_order", ctx, |lua, f, tbl, _| {
            let list = lua.create_table()?;
            for (i, (side, slot)) in order.entries.iter().enumerate() {
                let e = lua.create_table()?;
                e.raw_set("side", side_name(*side))?;
                e.raw_set("slot", *slot)?;
                list.raw_set(i + 1, e)?;
            }
            let ret: Value = f.call((tbl, list))?;
            if let Value::Table(t) = ret {
                let mut entries = Vec::new();
                for v in t.sequence_values::<Value>() {
                    let s = parse_slot(&v?)?;
                    entries.push((s.side, s.slot));
                }
                if entries.len() != order.entries.len() {
                    return Err(mlua::Error::runtime(
                        "on_action_order must return every action exactly once",
                    ));
                }
                order.entries = entries;
            }
            Ok(Flow::Continue)
        });
        Ok(())
    }

    fn on_before_move(
        &mut self,
        ctx: &mut BattleHookCtx<'_>,
        req: &mut MoveRequest,
    ) -> pkrs_battle::Result<HookResult> {
        let pack = self.pack;
        let mut verdict = HookResult::Continue;
        self.dispatch("on_before_move", ctx, |lua, f, tbl, view| {
            let user = creature_at(lua, view, req.user)?;
            let target = creature_at(lua, view, req.target)?;
            let mv = move_table(lua, pack, &req.move_id)?;
            mv.raw_set("slot", req.move_slot)?;
            let ret: Value = f.call((tbl, user, mv, target))?;
            if let Some(cancel) = cancel_of(&ret)? {
                verdict = cancel;
                return Ok(Flow::Stop);
            }
            if let Value::Table(t) = ret {
                let slot = t.raw_get::<Option<u8>>("move_slot")?;
                let target = match t.raw_get::<Value>("target")? {
                    Value::Nil => None,
                    v => Some(parse_slot(&v)?),
                };
                if let Some(s) = slot {
                    req.move_slot = s;
                }
                if let Some(t) = target {
                    req.target = t;
                }
            }
            Ok(Flow::Continue)
        });
        Ok(verdict)
    }

    fn on_accuracy(
        &mut self,
        ctx: &mut BattleHookCtx<'_>,
        acc: &mut AccuracyCalc,
    ) -> pkrs_battle::Result<()> {
        let pack = self.pack;
        self.dispatch("on_accuracy", ctx, |lua, f, tbl, view| {
            let (user, mv, target) = move_args(lua, view, pack)?;
            let a = lua.create_table()?;
            a.raw_set("hits", acc.hits)?;
            a.raw_set("accuracy", acc.move_accuracy)?;
            let ret: Value = f.call((tbl, user, mv, target, a))?;
            if let Value::Table(t) = ret {
                if let Some(hits) = t.raw_get::<Option<bool>>("hits")? {
                    acc.hits = hits;
                }
            }
            Ok(Flow::Continue)
        });
        Ok(())
    }

    fn on_damage_calc(
        &mut self,
        ctx: &mut BattleHookCtx<'_>,
        dmg: &mut DamageCalc,
    ) -> pkrs_battle::Result<()> {
        let pack = self.pack;
        self.dispatch("on_damage_calc", ctx, |lua, f, tbl, view| {
            let (user, mv, target) = move_args(lua, view, pack)?;
            let d = damage_table(lua, dmg)?;
            let ret: Value = f.call((tbl, user, mv, target, d.clone()))?;
            let result = match ret {
                Value::Table(t) => t,
                _ => d,
            };
            let mods: Table = result.raw_get("modifiers")?;
            let mut pushed = Vec::new();
            for entry in mods.sequence_values::<Table>() {
                let entry = entry?;
                let name: String = entry.raw_get(1)?;
                let mult: f64 = entry.raw_get(2)?;
                pushed.push((name, mult));
            }
            let final_override = result.raw_get::<Option<u32>>("final")?;
            for (name, mult) in pushed {
                dmg.push_modifier(name, mult);
            }
            if let Some(v) = final_override {
                dmg.set_final(v);
            }
            Ok(Flow::Continue)
        });
        Ok(())
    }

    fn on_damage_taken(
        &mut self,
        ctx: &mut BattleHookCtx<'_>,
        hit: &mut DamageTaken,
    ) -> pkrs_battle::Result<HookResult> {
        let mut verdict = HookResult::Continue;
        self.dispatch("on_damage_taken", ctx, |lua, f, tbl, view| {
            let target = creature_at(lua, view, hit.target)?;
            let source = match hit.source {
                Some(s) => creature_at(lua, view, s)?,
                None => Value::Nil,
            };
            let ret: Value = f.call((tbl, target, hit.amount, source))?;
            if let Some(cancel) = cancel_of(&ret)? {
                verdict = cancel;
                return Ok(Flow::Stop);
            }
            match ret {
                Value::Integer(n) => hit.amount = u32::try_from(n.max(0)).unwrap_or(u32::MAX),
                Value::Number(n) if n.is_finite() => hit.amount = n.max(0.0) as u32,
                Value::Table(t) => {
                    if let Some(n) = t.raw_get::<Option<u32>>("amount")? {
                        hit.amount = n;
                    }
                }
                Value::Nil => {}
                other => {
                    return Err(mlua::Error::runtime(format!(
                        "on_damage_taken must return a number, a table or nil (got {})",
                        other.type_name()
                    )));
                }
            }
            Ok(Flow::Continue)
        });
        Ok(verdict)
    }

    fn on_after_move(
        &mut self,
        ctx: &mut BattleHookCtx<'_>,
        res: &MoveResult,
    ) -> pkrs_battle::Result<()> {
        let pack = self.pack;
        self.dispatch("on_after_move", ctx, |lua, f, tbl, view| {
            let user = creature_at(lua, view, res.user)?;
            let target = creature_at(lua, view, res.target)?;
            let mv = move_table(lua, pack, &res.move_id)?;
            let r = lua.create_table()?;
            r.raw_set("hit", res.hit)?;
            r.raw_set("damage", res.damage)?;
            r.raw_set("target_fainted", res.target_fainted)?;
            f.call::<Value>((tbl, user, mv, target, r))?;
            Ok(Flow::Continue)
        });
        Ok(())
    }

    fn on_status_apply(
        &mut self,
        ctx: &mut BattleHookCtx<'_>,
        req: &mut StatusApply,
    ) -> pkrs_battle::Result<HookResult> {
        let mut verdict = HookResult::Continue;
        self.dispatch("on_status_apply", ctx, |lua, f, tbl, view| {
            let target = creature_at(lua, view, req.target)?;
            let ret: Value = f.call((tbl, target, req.status.as_str()))?;
            if let Some(cancel) = cancel_of(&ret)? {
                verdict = cancel;
                return Ok(Flow::Stop);
            }
            match ret {
                Value::String(s) => req.status = s.to_str()?.to_owned(),
                Value::Nil => {}
                other => {
                    return Err(mlua::Error::runtime(format!(
                        "on_status_apply must return a status id, a cancel table or nil (got {})",
                        other.type_name()
                    )));
                }
            }
            Ok(Flow::Continue)
        });
        Ok(verdict)
    }

    fn on_switch_in(
        &mut self,
        ctx: &mut BattleHookCtx<'_>,
        sw: &mut SwitchIn,
    ) -> pkrs_battle::Result<HookResult> {
        let mut verdict = HookResult::Continue;
        self.dispatch("on_switch_in", ctx, |lua, f, tbl, view| {
            let side = view.side(sw.slot.side);
            let incoming = match side.party.get(usize::from(sw.party_index)) {
                Some(c) => Value::Table(creature_table(lua, sw.slot, c)?),
                None => Value::Nil,
            };
            let ret: Value = f.call((tbl, incoming, sw.forced))?;
            if let Some(cancel) = cancel_of(&ret)? {
                verdict = cancel;
                return Ok(Flow::Stop);
            }
            Ok(Flow::Continue)
        });
        Ok(verdict)
    }

    fn on_switch_out(
        &mut self,
        ctx: &mut BattleHookCtx<'_>,
        sw: &mut SwitchOut,
    ) -> pkrs_battle::Result<HookResult> {
        let mut verdict = HookResult::Continue;
        self.dispatch("on_switch_out", ctx, |lua, f, tbl, view| {
            let leaving = creature_at(lua, view, sw.slot)?;
            let ret: Value = f.call((tbl, leaving, sw.forced))?;
            if let Some(cancel) = cancel_of(&ret)? {
                verdict = cancel;
                return Ok(Flow::Stop);
            }
            Ok(Flow::Continue)
        });
        Ok(verdict)
    }

    fn on_faint(
        &mut self,
        ctx: &mut BattleHookCtx<'_>,
        ev: &FaintEvent,
    ) -> pkrs_battle::Result<()> {
        self.dispatch("on_faint", ctx, |lua, f, tbl, view| {
            let c = creature_at(lua, view, ev.slot)?;
            f.call::<Value>((tbl, c))?;
            Ok(Flow::Continue)
        });
        Ok(())
    }

    fn on_battle_end(
        &mut self,
        ctx: &mut BattleHookCtx<'_>,
        result: &mut BattleResult,
    ) -> pkrs_battle::Result<()> {
        self.dispatch("on_battle_end", ctx, |lua, f, tbl, _| {
            let r = lua.create_table()?;
            r.raw_set(
                "outcome",
                match result.outcome {
                    BattleOutcome::PlayerWin => "win",
                    BattleOutcome::OpponentWin => "loss",
                    BattleOutcome::Fled { .. } => "fled",
                },
            )?;
            r.raw_set("turns", result.turns)?;
            r.raw_set("exp", result.exp)?;
            r.raw_set("money", result.money)?;
            let ret: Value = f.call((tbl, r.clone()))?;
            let t = match ret {
                Value::Table(t) => t,
                _ => r,
            };
            let exp = t.raw_get::<Option<u32>>("exp")?;
            let money = t.raw_get::<Option<u32>>("money")?;
            if let Some(exp) = exp {
                result.exp = exp;
            }
            if let Some(money) = money {
                result.money = money;
            }
            Ok(Flow::Continue)
        });
        Ok(())
    }

    fn on_action_menu(
        &mut self,
        ctx: &mut BattleHookCtx<'_>,
        menu: &mut ActionMenu,
    ) -> pkrs_battle::Result<()> {
        self.dispatch("on_action_menu", ctx, |lua, f, tbl, _| {
            let side = lua.create_table()?;
            side.raw_set("side", side_name(menu.slot.side))?;
            side.raw_set("slot", menu.slot.slot)?;
            side.raw_set("is_player", menu.slot.side == SideId::Player)?;
            let options = lua.create_table()?;
            for (i, o) in menu.options.iter().enumerate() {
                let e = lua.create_table()?;
                e.raw_set("id", o.id.as_str())?;
                e.raw_set("label", o.label.as_str())?;
                options.raw_set(i + 1, e)?;
            }
            let ret: Value = f.call((tbl, side, options.clone()))?;
            let list = match ret {
                Value::Table(t) => t,
                _ => options,
            };
            let mut parsed = Vec::new();
            for e in list.sequence_values::<Table>() {
                let e = e?;
                let id: String = e.raw_get("id")?;
                let label = e
                    .raw_get::<Option<String>>("label")?
                    .unwrap_or_else(|| id.clone());
                parsed.push(MenuOption { id, label });
            }
            menu.options = parsed;
            Ok(Flow::Continue)
        });
        Ok(())
    }

    fn on_custom_action(
        &mut self,
        ctx: &mut BattleHookCtx<'_>,
        action: &mut CustomAction,
    ) -> pkrs_battle::Result<()> {
        // A handler "handles" the action if it returns true or changes battle state
        // (`ctx.set_*`, `ctx.message`, …) — the §9.5 Resonance example returns nothing.
        let mut handled = action.handled;
        let before = ctx.checkpoint();
        self.dispatch("on_custom_action", ctx, |lua, f, tbl, view| {
            let payload = lua.create_table()?;
            payload.raw_set("creature", creature_at(lua, view, action.actor)?)?;
            payload.raw_set("bytes", lua.create_string(&action.payload)?)?;
            let ret: Value = f.call((tbl, action.id.as_str(), payload))?;
            if matches!(ret, Value::Boolean(true)) {
                handled = true;
            }
            Ok(Flow::Continue)
        });
        if ctx.checkpoint() > before {
            handled = true;
        }
        action.handled = handled;
        Ok(())
    }
}

/// The `dmg` table: `{ base, value, critical, modifiers = { push = fn(self, name, mult) } }`.
fn damage_table(lua: &Lua, dmg: &DamageCalc) -> mlua::Result<Table> {
    let d = lua.create_table()?;
    let base = dmg.trace.first().map_or(0, |(_, v)| *v);
    d.raw_set("base", base)?;
    d.raw_set("value", dmg.current())?;
    d.raw_set("immune", dmg.is_immune())?;
    let critical = dmg
        .trace
        .iter()
        .any(|(stage, v)| *stage == pkrs_battle::DamageModifierStage::Critical && *v > base);
    d.raw_set("critical", critical)?;
    let mods: Table = lua
        .load(
            "local m = {} function m:push(name, mult) self[#self + 1] = { name, mult } end return m",
        )
        .set_name("=pkrs-battle")
        .eval()?;
    d.raw_set("modifiers", mods)?;
    Ok(d)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn round_trip(v: &BattleValue) -> mlua::Result<BattleValue> {
        let lua = Lua::new();
        value_from_lua(&value_to_lua(&lua, v)?)
    }

    #[test]
    fn battle_values_round_trip() {
        let map: BTreeMap<String, BattleValue> = [
            ("turns".to_string(), BattleValue::Int(3)),
            ("owner".to_string(), BattleValue::Str("resonance".into())),
            (
                "flags".to_string(),
                BattleValue::List(vec![BattleValue::Bool(true), BattleValue::Int(-1)]),
            ),
        ]
        .into_iter()
        .collect();
        for v in [
            BattleValue::Bool(false),
            BattleValue::Int(i64::MIN),
            BattleValue::Str("é".into()),
            BattleValue::List(vec![BattleValue::Int(1), BattleValue::Int(2)]),
            BattleValue::Map(map),
        ] {
            assert_eq!(round_trip(&v).unwrap(), v);
        }
    }

    #[test]
    fn inexact_or_opaque_values_are_rejected() {
        let lua = Lua::new();
        assert_eq!(
            value_from_lua(&Value::Number(4.0)).unwrap(),
            BattleValue::Int(4)
        );
        assert!(value_from_lua(&Value::Number(1.5)).is_err());
        assert!(value_from_lua(&Value::Nil).is_err());
        let f = lua.create_function(|_, ()| Ok(())).unwrap();
        assert!(value_from_lua(&Value::Function(f)).is_err());
        let t: Table = lua.load("return { [true] = 1 }").eval().unwrap();
        assert!(value_from_lua(&Value::Table(t)).is_err());
    }

    #[test]
    fn sides_and_slots_parse_from_strings_and_tables() {
        let lua = Lua::new();
        assert_eq!(
            parse_side(&Value::String(lua.create_string("player").unwrap())).unwrap(),
            SideId::Player
        );
        let t: Table = lua
            .load("return { side = 'opponent', slot = 1 }")
            .eval()
            .unwrap();
        assert_eq!(
            parse_slot(&Value::Table(t.clone())).unwrap(),
            SlotRef::new(SideId::Opponent, 1)
        );
        assert_eq!(parse_side(&Value::Table(t)).unwrap(), SideId::Opponent);
        assert!(parse_side(&Value::String(lua.create_string("left").unwrap())).is_err());
    }

    #[test]
    fn cancel_tables_are_recognised() {
        let lua = Lua::new();
        let t: Value = lua
            .load("return { cancel = true, message = 'text:x' }")
            .eval()
            .unwrap();
        assert_eq!(cancel_of(&t).unwrap(), Some(HookResult::cancel("text:x")));
        let t: Value = lua.load("return { cancel = false }").eval().unwrap();
        assert_eq!(cancel_of(&t).unwrap(), None);
        assert_eq!(cancel_of(&Value::Nil).unwrap(), None);
    }
}
