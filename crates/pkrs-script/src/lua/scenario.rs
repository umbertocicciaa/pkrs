//! `LuaScenario`: the `mlua` adapter that runs a `tests/*.lua` scenario (§15.2).
//!
//! Each scenario gets its **own** Lua state, separate from the game's
//! [`LuaScriptHost`](super::LuaScriptHost): the safe standard library (§9.6 whitelist, minus `math.random`,
//! `require` and `load`) plus the test API below. Game scripts never see it, and it never
//! sees the game's `game` / `map` / `ui` tables: it observes the game only through the
//! [`ScenarioSnapshot`] the runtime hands to [`LuaScenario::resume`].
//!
//! The script body runs as a coroutine. Every call that needs time to pass (`wait_ticks`,
//! `press`, `walk`, …) yields a tick count back to the runtime, which runs those ticks with the
//! held actions and resumes the script with a fresh snapshot.
//!
//! # Test API
//!
//! Input: `press(action [, ticks])`, `hold(action)`, `release(action)`, `release_all()`,
//! `held(action)`, `wait_ticks(n)` (alias `wait`).
//!
//! Waiting: `wait_until(fn [, max_ticks [, what]])`, `wait_for_state(name [, max_ticks])`.
//!
//! Helpers: `advance_dialogue([max_ticks])`, `choose(index)` (choice lists, battle menus and
//! the move-forget menu after a battle or an evolution), `answer(yes)`,
//! `walk(direction [, tiles])`, `face(direction)`, `advance_battle([max_ticks])` (confirms
//! battle messages until a battle menu opens or the battle ends).
//!
//! Queries: `tick()`, `state()`, `map()`, `player()` (x, y, facing), `idle()`, `locked()`,
//! `npc(id)` (x, y, facing or nil), `flag(name)`, `var(name)`, `item(id)`, `dialogue()`,
//! `dialogues_opened()`, `choices()`, `cursor()` (1-based), `battle()` (`nil` outside a battle,
//! else `{ menu, turn, player = { name, level, hp, max_hp }, opponent = { … } }`), `party()`
//! (the player's party, 1-based: `{ species, name, level, exp, moves = { ids }, max_hp,
//! attack, defense, speed }`; empty before the first battle).
//!
//! Expectations: `expect(cond [, message])`, `expect_eq(actual, expected [, message])`,
//! `expect_state(name)`, `expect_map(id)`, `expect_player_at(x, y [, facing])`,
//! `expect_npc_at(id, x, y [, facing])`, `expect_flag(name [, value])`,
//! `expect_var(name, value)`, `expect_item(id, count)`, `expect_dialogue(text)`,
//! `expect_choices(options)`, `fail(message)`.
//!
//! A failed expectation records a [`ScenarioFailure`] with the scenario's `file:line` (the
//! first frame of the scenario file on the stack, so helpers report their caller) and stops
//! the script; it cannot be swallowed by `pcall`.

use std::cell::Cell;
use std::collections::BTreeSet;
use std::rc::Rc;

use mlua::{
    ChunkMode, Function, HookTriggers, Lua, LuaOptions, MultiValue, StdLib, Table, Thread,
    ThreadStatus, Value, VmState,
};
use pkrs_core::{Direction, GridPos};

use super::{convert, map_error, sandbox};
use crate::error::{Result, ScriptError};
use crate::host::{ScriptLimits, ScriptValue};
use crate::scenario::{
    BattleSnapshot, BattlerSnapshot, DEFAULT_WAIT_TICKS, PartyMemberSnapshot, ScenarioFailure,
    ScenarioSnapshot, ScenarioStep, SourceLocation, format_placement, format_value, values_equal,
};

/// Every global the test API adds to a scenario's environment.
pub const SCENARIO_API: &[&str] = &[
    "press",
    "hold",
    "release",
    "release_all",
    "held",
    "wait_ticks",
    "wait",
    "wait_until",
    "wait_for_state",
    "advance_dialogue",
    "advance_battle",
    "choose",
    "answer",
    "walk",
    "face",
    "tick",
    "state",
    "map",
    "player",
    "idle",
    "locked",
    "npc",
    "flag",
    "var",
    "item",
    "dialogue",
    "dialogues_opened",
    "choices",
    "cursor",
    "battle",
    "party",
    "expect",
    "expect_eq",
    "expect_state",
    "expect_map",
    "expect_player_at",
    "expect_npc_at",
    "expect_flag",
    "expect_var",
    "expect_item",
    "expect_dialogue",
    "expect_choices",
    "fail",
];

/// Chunk name of the Lua-side helpers; frames from it are skipped when locating failures.
const PRELUDE_NAME: &str = "=pkrs-scenario";

/// Instructions between two budget checks.
const HOOK_STRIDE: u32 = 1_000;

/// Deepest stack level searched for the scenario file's frame.
const MAX_STACK_SCAN: usize = 64;

/// Lua-side helpers built on the Rust primitives. Receives `coroutine.yield`, the default wait
/// and the environment; defines the helper globals in it.
const PRELUDE: &str = r#"
local yield, default_wait, env = ...
local tointeger = math.tointeger
local _ENV = env

local function whole(n, what, min)
  local t = type(n) == "number" and tointeger(n) or nil
  if t == nil or t < min then
    __usage(what, "expected a whole number >= " .. min .. " (got " .. tostring(n) .. ")")
  end
  return t
end

function wait_ticks(n)
  if n == nil then n = 1 end
  local t = whole(n, "wait_ticks", 0)
  if t > 0 then yield(t) end
end
wait = wait_ticks

function press(action, ticks)
  local t = whole(ticks or 1, "press", 1)
  if held(action) then
    __usage("press", "`" .. tostring(action) .. "` is already held (release it first)")
  end
  hold(action)
  wait_ticks(t)
  release(action)
  wait_ticks(1)
end

function wait_until(pred, max_ticks, what)
  if type(pred) ~= "function" then
    __usage("wait_until", "expected a function (got " .. type(pred) .. ")")
  end
  local limit = whole(max_ticks or default_wait, "wait_until", 0)
  local start = tick()
  while not pred() do
    if tick() - start >= limit then
      __fail("wait_until", what or "condition to become true",
        "still false after " .. limit .. " ticks")
    end
    wait_ticks(1)
  end
end

function wait_for_state(name, max_ticks)
  local limit = whole(max_ticks or default_wait, "wait_for_state", 0)
  local start = tick()
  while state() ~= name do
    if tick() - start >= limit then
      __fail("wait_for_state", tostring(name), state() .. " (after " .. limit .. " ticks)")
    end
    wait_ticks(1)
  end
end

function advance_dialogue(max_ticks)
  if state() ~= "Dialogue" then __fail("advance_dialogue", "Dialogue", state()) end
  local limit = whole(max_ticks or default_wait, "advance_dialogue", 1)
  local opened = dialogues_opened()
  local start = tick()
  repeat
    if tick() - start >= limit then
      __fail("advance_dialogue", "the text box to close",
        "still open after " .. limit .. " ticks")
    end
    press("confirm")
  until state() ~= "Dialogue" or dialogues_opened() ~= opened
end

function choose(index)
  local st = state()
  if st ~= "Choice" and not ((st == "Battle" or st == "Evolution") and choices() ~= nil) then
    __fail("choose", "Choice", st)
  end
  local options = choices()
  local i = whole(index, "choose", 1)
  if i > #options then
    __usage("choose", "option " .. i .. " does not exist (" .. #options .. " options)")
  end
  local guard = 0
  while cursor() ~= i do
    guard = guard + 1
    if guard > 2 * #options + 2 then
      __fail("choose", "cursor on option " .. i, "cursor on option " .. tostring(cursor()))
    end
    if cursor() < i then press("down") else press("up") end
  end
  press("confirm")
end

function advance_battle(max_ticks)
  if state() ~= "Battle" then __fail("advance_battle", "Battle", state()) end
  local limit = whole(max_ticks or default_wait, "advance_battle", 1)
  local start = tick()
  while state() == "Battle" and choices() == nil do
    if tick() - start >= limit then
      __fail("advance_battle", "a battle menu or the end of the battle",
        "still playing after " .. limit .. " ticks")
    end
    if dialogue() ~= nil then press("confirm") else wait_ticks(1) end
  end
end

function answer(yes)
  if yes then choose(1) else choose(2) end
end

local deltas = { up = { 0, -1 }, down = { 0, 1 }, left = { -1, 0 }, right = { 1, 0 } }

local function direction(dir, what)
  local d = deltas[dir]
  if d == nil then
    __usage(what, "expected up, down, left or right (got " .. tostring(dir) .. ")")
  end
  return d
end

function walk(dir, tiles)
  local d = direction(dir, "walk")
  local n = whole(tiles or 1, "walk", 1)
  local x, y = player()
  local tx, ty = x + d[1] * n, y + d[2] * n
  local limit = 40 * n + 40
  local start = tick()
  hold(dir)
  while true do
    local px, py = player()
    if px == tx and py == ty then break end
    if tick() - start >= limit then
      release(dir)
      __fail("walk", "player at (" .. tx .. ", " .. ty .. ")",
        "(" .. px .. ", " .. py .. ") after " .. limit .. " ticks")
    end
    wait_ticks(1)
  end
  release(dir)
  wait_until(idle, limit, "the player to finish the step")
end

function face(dir)
  direction(dir, "face")
  local _, _, facing = player()
  if facing ~= dir then press(dir) end
  wait_until(idle, default_wait, "the player to stand still")
end
"#;

/// Shared with the instruction hook.
struct Budget {
    limit: u64,
    used: Cell<u64>,
    tripped: Cell<bool>,
}

/// App data: what the test API reads and records.
#[derive(Default)]
struct ScenarioState {
    snapshot: ScenarioSnapshot,
    held: BTreeSet<String>,
    failure: Option<ScenarioFailure>,
}

/// A loaded scenario script, resumed once per [`ScenarioStep::Advance`].
pub struct LuaScenario {
    lua: Lua,
    name: String,
    thread: Option<Thread>,
    budget: Rc<Budget>,
    limits: ScriptLimits,
    tick: u64,
}

impl LuaScenario {
    /// Compiles `source` (named `name`, e.g. `tests/smoke.lua`, in failure messages) in a fresh
    /// sandboxed Lua state with the test API. Nothing runs until the first
    /// [`resume`](Self::resume).
    pub fn load(name: &str, source: &str, limits: ScriptLimits) -> Result<Self> {
        let libs = StdLib::COROUTINE | StdLib::TABLE | StdLib::STRING | StdLib::UTF8 | StdLib::MATH;
        let lua = Lua::new_with(libs, LuaOptions::default()).map_err(engine)?;
        sandbox::harden_globals(&lua).map_err(engine)?;
        if limits.memory_limit > 0 {
            lua.set_memory_limit(limits.memory_limit).map_err(engine)?;
        }
        lua.set_app_data(ScenarioState::default());

        let budget = Rc::new(Budget {
            limit: limits.instruction_budget,
            used: Cell::new(0),
            tripped: Cell::new(false),
        });
        let hook_budget = Rc::clone(&budget);
        lua.set_global_hook(
            HookTriggers::new().every_nth_instruction(HOOK_STRIDE),
            move |_, _| {
                let used = hook_budget
                    .used
                    .get()
                    .saturating_add(u64::from(HOOK_STRIDE));
                hook_budget.used.set(used);
                if used > hook_budget.limit {
                    hook_budget.tripped.set(true);
                    Err(mlua::Error::runtime("instruction budget exceeded"))
                } else {
                    Ok(VmState::Continue)
                }
            },
        )
        .map_err(engine)?;

        let env = build_env(&lua, name).map_err(engine)?;
        let body = lua
            .load(source)
            .set_name(format!("@{name}"))
            .set_mode(ChunkMode::Text)
            .set_environment(env)
            .into_function()
            .map_err(|e| map_error(e, name, limits))?;
        let thread = lua.create_thread(body).map_err(engine)?;
        Ok(Self {
            lua,
            name: name.to_owned(),
            thread: Some(thread),
            budget,
            limits,
            tick: 0,
        })
    }

    /// The scenario's name (its path).
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Actions currently held with `hold` (sorted).
    #[must_use]
    pub fn held(&self) -> Vec<String> {
        self.lua
            .app_data_ref::<ScenarioState>()
            .map(|s| s.held.iter().cloned().collect())
            .unwrap_or_default()
    }

    /// Runs the script with `snapshot` as the game state until it needs ticks to pass,
    /// returns, or fails. After [`ScenarioStep::Finished`] or [`ScenarioStep::Failed`] every
    /// later call returns [`ScenarioStep::Finished`].
    pub fn resume(&mut self, snapshot: ScenarioSnapshot) -> ScenarioStep {
        let Some(thread) = self.thread.take() else {
            return ScenarioStep::Finished;
        };
        self.tick = snapshot.tick;
        if let Some(mut state) = self.lua.app_data_mut::<ScenarioState>() {
            state.snapshot = snapshot;
        }
        self.budget.used.set(0);
        self.budget.tripped.set(false);
        let result = thread.resume::<MultiValue>(());
        let tripped = self.budget.tripped.replace(false);
        if let Some(failure) = self.take_failure() {
            return ScenarioStep::Failed(failure);
        }
        if tripped {
            return ScenarioStep::Failed(
                ScenarioFailure::new("error", self.tick).message(
                    ScriptError::InstructionBudget {
                        budget: self.limits.instruction_budget,
                    }
                    .to_string(),
                ),
            );
        }
        let values = match result {
            Ok(values) => values,
            Err(err) => return ScenarioStep::Failed(self.error_failure(err)),
        };
        if thread.status() != ThreadStatus::Resumable {
            return ScenarioStep::Finished;
        }
        let ticks = match values.into_iter().next() {
            Some(Value::Integer(n)) => u32::try_from(n).ok().filter(|n| *n > 0),
            _ => None,
        };
        match ticks {
            Some(ticks) => {
                self.thread = Some(thread);
                ScenarioStep::Advance {
                    ticks,
                    held: self.held(),
                }
            }
            None => ScenarioStep::Failed(ScenarioFailure::new("error", self.tick).message(
                "the scenario yielded outside the test API (use wait_ticks, not coroutine.yield)",
            )),
        }
    }

    fn take_failure(&mut self) -> Option<ScenarioFailure> {
        self.lua
            .app_data_mut::<ScenarioState>()
            .and_then(|mut s| s.failure.take())
    }

    fn error_failure(&self, err: mlua::Error) -> ScenarioFailure {
        let failure = ScenarioFailure::new("error", self.tick);
        match map_error(err, &self.name, self.limits) {
            ScriptError::Located {
                file,
                line,
                message,
                ..
            } => failure
                .at(Some(SourceLocation { file, line }))
                .message(message),
            other => failure.message(other.to_string()),
        }
    }
}

fn engine(err: mlua::Error) -> ScriptError {
    ScriptError::Engine(err.to_string())
}

/// The first frame on the stack that belongs to a scenario file (chunk names `@path`).
fn scenario_location(lua: &Lua) -> Option<SourceLocation> {
    (0..MAX_STACK_SCAN)
        .map_while(|level| {
            lua.inspect_stack(level, |d| {
                let source = d.source().source.map(|s| s.into_owned());
                (source, d.current_line())
            })
        })
        .find_map(|(source, line)| {
            let file = source?.strip_prefix('@')?.to_owned();
            let line = u32::try_from(line?).ok()?;
            Some(SourceLocation { file, line })
        })
}

fn with_state<R>(lua: &Lua, f: impl FnOnce(&mut ScenarioState) -> R) -> mlua::Result<R> {
    let mut state = lua
        .app_data_mut::<ScenarioState>()
        .ok_or_else(|| mlua::Error::runtime("scenario state unavailable"))?;
    Ok(f(&mut state))
}

fn snapshot<R>(lua: &Lua, f: impl FnOnce(&ScenarioSnapshot) -> R) -> mlua::Result<R> {
    with_state(lua, |s| f(&s.snapshot))
}

/// Records `failure` (located at the scenario's current line) and returns the error that
/// stops the script. The first failure wins.
fn raise(lua: &Lua, failure: ScenarioFailure) -> mlua::Error {
    let failure = failure.at(scenario_location(lua));
    let text = failure.to_string();
    let _ = with_state(lua, |s| {
        if s.failure.is_none() {
            s.failure = Some(failure);
        }
    });
    mlua::Error::runtime(text)
}

fn current_tick(lua: &Lua) -> u64 {
    snapshot(lua, |s| s.tick).unwrap_or(0)
}

/// A failed `check` with `expected` / `actual`.
fn mismatch(lua: &Lua, check: &str, expected: String, actual: String) -> mlua::Error {
    raise(
        lua,
        ScenarioFailure::new(check, current_tick(lua)).values(expected, actual),
    )
}

/// Wrong arguments to a test API function.
fn usage(lua: &Lua, check: &str, message: impl Into<String>) -> mlua::Error {
    raise(
        lua,
        ScenarioFailure::new(check, current_tick(lua)).message(message),
    )
}

fn type_name(value: &Value) -> &'static str {
    match value {
        Value::Integer(_) | Value::Number(_) => "number",
        other => other.type_name(),
    }
}

fn string_arg(lua: &Lua, check: &str, what: &str, value: &Value) -> mlua::Result<String> {
    match value {
        Value::String(s) => Ok(s.to_string_lossy()),
        other => Err(usage(
            lua,
            check,
            format!("{what} must be a string (got {})", type_name(other)),
        )),
    }
}

fn int_arg(lua: &Lua, check: &str, what: &str, value: &Value) -> mlua::Result<i64> {
    let n = match value {
        Value::Integer(i) => Some(*i),
        Value::Number(n) if n.fract() == 0.0 && n.is_finite() => Some(*n as i64),
        _ => None,
    };
    n.ok_or_else(|| {
        usage(
            lua,
            check,
            format!("{what} must be a whole number (got {})", type_name(value)),
        )
    })
}

fn coord_arg(lua: &Lua, check: &str, what: &str, value: &Value) -> mlua::Result<i32> {
    let n = int_arg(lua, check, what, value)?;
    i32::try_from(n).map_err(|_| usage(lua, check, format!("{what} is out of range ({n})")))
}

fn facing_arg(lua: &Lua, check: &str, value: &Value) -> mlua::Result<Option<Direction>> {
    match value {
        Value::Nil => Ok(None),
        other => {
            let text = string_arg(lua, check, "facing", other)?;
            text.parse::<Direction>().map(Some).map_err(|_| {
                usage(
                    lua,
                    check,
                    format!("facing must be up, down, left or right (got {text:?})"),
                )
            })
        }
    }
}

fn truthy(value: &Value) -> bool {
    !matches!(value, Value::Nil | Value::Boolean(false))
}

/// Engine value → Lua value for queries; NPC handles become their id (the scenario state has
/// no `npc` objects).
fn to_lua(lua: &Lua, value: &ScriptValue) -> mlua::Result<Value> {
    convert::to_lua(lua, &plain(value))
}

fn plain(value: &ScriptValue) -> ScriptValue {
    match value {
        ScriptValue::Npc(id) => ScriptValue::String(id.clone()),
        ScriptValue::List(items) => ScriptValue::List(items.iter().map(plain).collect()),
        ScriptValue::Map(map) => {
            ScriptValue::Map(map.iter().map(|(k, v)| (k.clone(), plain(v))).collect())
        }
        other => other.clone(),
    }
}

fn from_lua(lua: &Lua, check: &str, value: &Value) -> mlua::Result<ScriptValue> {
    convert::from_lua(value).map_err(|e| usage(lua, check, e.to_string()))
}

fn placement_values(lua: &Lua, pos: GridPos, facing: Direction) -> mlua::Result<MultiValue> {
    Ok(MultiValue::from_vec(vec![
        Value::Integer(i64::from(pos.x)),
        Value::Integer(i64::from(pos.y)),
        Value::String(lua.create_string(facing.to_string())?),
    ]))
}

/// Builds the scenario's `_ENV`: safe libraries, `print`, the Rust primitives and the Lua
/// helpers.
fn build_env(lua: &Lua, name: &str) -> mlua::Result<Table> {
    let globals = lua.globals();
    let env = lua.create_table()?;
    for base in sandbox::SAFE_BASE {
        env.raw_set(*base, globals.raw_get::<Value>(*base)?)?;
    }
    for lib in sandbox::SAFE_LIBS {
        let table: Table = globals.raw_get(*lib)?;
        env.raw_set(*lib, sandbox::copy_table(lua, &table)?)?;
    }
    env.raw_set("_G", env.clone())?;
    // Scenarios must be deterministic: no unseeded randomness.
    let math: Table = env.raw_get("math")?;
    math.raw_set("random", Value::Nil)?;
    math.raw_set("randomseed", Value::Nil)?;

    let scenario = name.to_owned();
    env.raw_set(
        "print",
        lua.create_function(move |_, args: MultiValue| {
            let parts: Vec<String> = args
                .iter()
                .map(|v| v.to_string().unwrap_or_else(|_| v.type_name().to_owned()))
                .collect();
            tracing::info!(target: "scenario", scenario = %scenario, "{}", parts.join("\t"));
            Ok(())
        })?,
    )?;

    install_input(lua, &env)?;
    install_queries(lua, &env)?;
    install_expectations(lua, &env)?;

    let coroutine: Table = env.raw_get("coroutine")?;
    let yield_fn: Function = coroutine.raw_get("yield")?;
    lua.load(PRELUDE)
        .set_name(PRELUDE_NAME)
        .set_mode(ChunkMode::Text)
        .call::<()>((yield_fn, DEFAULT_WAIT_TICKS, env.clone()))?;
    Ok(env)
}

fn install_input(lua: &Lua, env: &Table) -> mlua::Result<()> {
    fn action(lua: &Lua, check: &str, value: &Value) -> mlua::Result<String> {
        let name = string_arg(lua, check, "action", value)?;
        let known = snapshot(lua, |s| s.actions.contains(&name))?;
        if known {
            Ok(name)
        } else {
            let all = snapshot(lua, |s| {
                s.actions.iter().cloned().collect::<Vec<_>>().join(", ")
            })?;
            Err(usage(
                lua,
                check,
                format!("unknown action {name:?} (known: {all})"),
            ))
        }
    }
    env.raw_set(
        "hold",
        lua.create_function(|lua, value: Value| {
            let name = action(lua, "hold", &value)?;
            with_state(lua, |s| {
                s.held.insert(name);
            })
        })?,
    )?;
    env.raw_set(
        "release",
        lua.create_function(|lua, value: Value| {
            let name = action(lua, "release", &value)?;
            with_state(lua, |s| {
                s.held.remove(&name);
            })
        })?,
    )?;
    env.raw_set(
        "release_all",
        lua.create_function(|lua, ()| with_state(lua, |s| s.held.clear()))?,
    )?;
    env.raw_set(
        "held",
        lua.create_function(|lua, value: Value| {
            let name = string_arg(lua, "held", "action", &value)?;
            with_state(lua, |s| s.held.contains(&name))
        })?,
    )?;
    // Internal: wrong arguments / failed checks raised by the Lua helpers.
    env.raw_set(
        "__usage",
        lua.create_function(|lua, (check, message): (String, String)| {
            Err::<(), _>(usage(lua, &check, message))
        })?,
    )?;
    env.raw_set(
        "__fail",
        lua.create_function(|lua, (check, expected, actual): (String, String, String)| {
            Err::<(), _>(mismatch(lua, &check, expected, actual))
        })?,
    )?;
    Ok(())
}

fn install_queries(lua: &Lua, env: &Table) -> mlua::Result<()> {
    env.raw_set(
        "tick",
        lua.create_function(|lua, ()| {
            snapshot(lua, |s| i64::try_from(s.tick).unwrap_or(i64::MAX))
        })?,
    )?;
    env.raw_set(
        "state",
        lua.create_function(|lua, ()| snapshot(lua, |s| s.state.clone()))?,
    )?;
    env.raw_set(
        "map",
        lua.create_function(|lua, ()| snapshot(lua, |s| s.map.clone()))?,
    )?;
    env.raw_set(
        "player",
        lua.create_function(|lua, ()| match snapshot(lua, |s| s.player)? {
            Some(p) => placement_values(lua, p.pos, p.facing),
            None => Ok(MultiValue::new()),
        })?,
    )?;
    env.raw_set(
        "idle",
        lua.create_function(|lua, ()| snapshot(lua, |s| s.player_idle))?,
    )?;
    env.raw_set(
        "locked",
        lua.create_function(|lua, ()| snapshot(lua, |s| s.player_locked))?,
    )?;
    env.raw_set(
        "npc",
        lua.create_function(|lua, id: Value| {
            let id = string_arg(lua, "npc", "npc id", &id)?;
            match snapshot(lua, |s| s.npcs.get(&id).copied())? {
                Some(p) => placement_values(lua, p.pos, p.facing),
                None => Ok(MultiValue::new()),
            }
        })?,
    )?;
    env.raw_set(
        "flag",
        lua.create_function(|lua, name: Value| {
            let name = string_arg(lua, "flag", "flag name", &name)?;
            snapshot(lua, |s| s.flags.get(&name).copied().unwrap_or(false))
        })?,
    )?;
    env.raw_set(
        "var",
        lua.create_function(|lua, name: Value| {
            let name = string_arg(lua, "var", "variable name", &name)?;
            let value = snapshot(lua, |s| s.vars.get(&name).cloned())?;
            to_lua(lua, &value.unwrap_or(ScriptValue::Nil))
        })?,
    )?;
    env.raw_set(
        "item",
        lua.create_function(|lua, id: Value| {
            let id = string_arg(lua, "item", "item id", &id)?;
            snapshot(lua, |s| s.items.get(&id).copied().unwrap_or(0))
        })?,
    )?;
    env.raw_set(
        "dialogue",
        lua.create_function(|lua, ()| snapshot(lua, |s| s.dialogue.clone()))?,
    )?;
    env.raw_set(
        "dialogues_opened",
        lua.create_function(|lua, ()| {
            snapshot(lua, |s| {
                i64::try_from(s.dialogues_opened).unwrap_or(i64::MAX)
            })
        })?,
    )?;
    env.raw_set(
        "choices",
        lua.create_function(|lua, ()| {
            let options = snapshot(lua, |s| s.choices.clone())?;
            if options.is_empty() {
                return Ok(Value::Nil);
            }
            let list = ScriptValue::List(options.into_iter().map(ScriptValue::String).collect());
            to_lua(lua, &list)
        })?,
    )?;
    env.raw_set(
        "cursor",
        lua.create_function(|lua, ()| snapshot(lua, |s| s.cursor.map(|c| c + 1)))?,
    )?;
    env.raw_set(
        "battle",
        lua.create_function(|lua, ()| {
            let battle = snapshot(lua, |s| s.battle.clone())?;
            to_lua(lua, &battle.map_or(ScriptValue::Nil, |b| battle_value(&b)))
        })?,
    )?;
    env.raw_set(
        "party",
        lua.create_function(|lua, ()| {
            let party = snapshot(lua, |s| s.party.clone())?;
            to_lua(
                lua,
                &ScriptValue::List(party.iter().map(party_member_value).collect()),
            )
        })?,
    )?;
    Ok(())
}

/// One `party()` entry: `{ species, name, level, exp, moves, max_hp, attack, defense, speed }`.
fn party_member_value(m: &PartyMemberSnapshot) -> ScriptValue {
    let mut map = std::collections::BTreeMap::new();
    map.insert("species".into(), ScriptValue::String(m.species.clone()));
    map.insert("name".into(), ScriptValue::String(m.name.clone()));
    map.insert("level".into(), ScriptValue::Int(i64::from(m.level)));
    map.insert("exp".into(), ScriptValue::Int(i64::from(m.exp)));
    map.insert(
        "moves".into(),
        ScriptValue::List(m.moves.iter().cloned().map(ScriptValue::String).collect()),
    );
    for (key, value) in [
        ("max_hp", m.max_hp),
        ("attack", m.attack),
        ("defense", m.defense),
        ("speed", m.speed),
    ] {
        map.insert(key.into(), ScriptValue::Int(i64::from(value)));
    }
    ScriptValue::Map(map)
}

/// `battle()`: `{ menu, turn, player = { name, level, hp, max_hp }, opponent = { … } }`.
fn battle_value(battle: &BattleSnapshot) -> ScriptValue {
    let battler = |b: &BattlerSnapshot| {
        let mut map = std::collections::BTreeMap::new();
        map.insert("name".into(), ScriptValue::String(b.name.clone()));
        map.insert("level".into(), ScriptValue::Int(i64::from(b.level)));
        map.insert("hp".into(), ScriptValue::Int(i64::from(b.hp)));
        map.insert("max_hp".into(), ScriptValue::Int(i64::from(b.max_hp)));
        ScriptValue::Map(map)
    };
    let mut map = std::collections::BTreeMap::new();
    if let Some(menu) = &battle.menu {
        map.insert("menu".into(), ScriptValue::String(menu.clone()));
    }
    map.insert("turn".into(), ScriptValue::Int(i64::from(battle.turn)));
    if let Some(p) = &battle.player {
        map.insert("player".into(), battler(p));
    }
    if let Some(o) = &battle.opponent {
        map.insert("opponent".into(), battler(o));
    }
    ScriptValue::Map(map)
}

fn install_expectations(lua: &Lua, env: &Table) -> mlua::Result<()> {
    env.raw_set(
        "expect",
        lua.create_function(|lua, (cond, message): (Value, Option<String>)| {
            if truthy(&cond) {
                Ok(())
            } else {
                Err(usage(
                    lua,
                    "expect",
                    message.unwrap_or_else(|| "condition is false".into()),
                ))
            }
        })?,
    )?;
    env.raw_set(
        "expect_eq",
        lua.create_function(
            |lua, (actual, expected, message): (Value, Value, Option<String>)| {
                let actual = from_lua(lua, "expect_eq", &actual)?;
                let expected = from_lua(lua, "expect_eq", &expected)?;
                if values_equal(&actual, &expected) {
                    return Ok(());
                }
                let mut failure = ScenarioFailure::new("expect_eq", current_tick(lua))
                    .values(format_value(&expected), format_value(&actual));
                if let Some(message) = message {
                    failure = failure.message(message);
                }
                Err(raise(lua, failure))
            },
        )?,
    )?;
    env.raw_set(
        "expect_state",
        lua.create_function(|lua, name: Value| {
            let name = string_arg(lua, "expect_state", "state name", &name)?;
            let actual = snapshot(lua, |s| s.state.clone())?;
            if actual == name {
                Ok(())
            } else {
                Err(mismatch(lua, "expect_state", name, actual))
            }
        })?,
    )?;
    env.raw_set(
        "expect_map",
        lua.create_function(|lua, id: Value| {
            let id = string_arg(lua, "expect_map", "map id", &id)?;
            let actual = snapshot(lua, |s| s.map.clone())?;
            if actual.as_deref() == Some(id.as_str()) {
                Ok(())
            } else {
                Err(mismatch(
                    lua,
                    "expect_map",
                    id,
                    actual.unwrap_or_else(|| "no map".into()),
                ))
            }
        })?,
    )?;
    env.raw_set(
        "expect_player_at",
        lua.create_function(|lua, (x, y, facing): (Value, Value, Value)| {
            let check = "expect_player_at";
            let pos = GridPos::new(
                coord_arg(lua, check, "x", &x)?,
                coord_arg(lua, check, "y", &y)?,
            );
            let facing = facing_arg(lua, check, &facing)?;
            let actual = snapshot(lua, |s| s.player)?;
            expect_placement(lua, check, pos, facing, actual)
        })?,
    )?;
    env.raw_set(
        "expect_npc_at",
        lua.create_function(|lua, (id, x, y, facing): (Value, Value, Value, Value)| {
            let check = "expect_npc_at";
            let id = string_arg(lua, check, "npc id", &id)?;
            let pos = GridPos::new(
                coord_arg(lua, check, "x", &x)?,
                coord_arg(lua, check, "y", &y)?,
            );
            let facing = facing_arg(lua, check, &facing)?;
            let actual = snapshot(lua, |s| s.npcs.get(&id).copied())?;
            expect_placement(lua, check, pos, facing, actual)
        })?,
    )?;
    env.raw_set(
        "expect_flag",
        lua.create_function(|lua, (name, value): (Value, Value)| {
            let name = string_arg(lua, "expect_flag", "flag name", &name)?;
            let expected = match value {
                Value::Nil => true,
                Value::Boolean(b) => b,
                other => {
                    return Err(usage(
                        lua,
                        "expect_flag",
                        format!("value must be a boolean (got {})", type_name(&other)),
                    ));
                }
            };
            let actual = snapshot(lua, |s| s.flags.get(&name).copied().unwrap_or(false))?;
            if actual == expected {
                Ok(())
            } else {
                Err(mismatch(
                    lua,
                    "expect_flag",
                    format!("{name} = {expected}"),
                    format!("{name} = {actual}"),
                ))
            }
        })?,
    )?;
    env.raw_set(
        "expect_var",
        lua.create_function(|lua, (name, value): (Value, Value)| {
            let name = string_arg(lua, "expect_var", "variable name", &name)?;
            let expected = from_lua(lua, "expect_var", &value)?;
            let actual = snapshot(lua, |s| s.vars.get(&name).cloned())?.unwrap_or(ScriptValue::Nil);
            if values_equal(&actual, &expected) {
                Ok(())
            } else {
                Err(mismatch(
                    lua,
                    "expect_var",
                    format!("{name} = {}", format_value(&expected)),
                    format!("{name} = {}", format_value(&actual)),
                ))
            }
        })?,
    )?;
    env.raw_set(
        "expect_item",
        lua.create_function(|lua, (id, count): (Value, Value)| {
            let id = string_arg(lua, "expect_item", "item id", &id)?;
            let expected = int_arg(lua, "expect_item", "count", &count)?;
            let actual = snapshot(lua, |s| s.items.get(&id).copied().unwrap_or(0))?;
            if i64::from(actual) == expected {
                Ok(())
            } else {
                Err(mismatch(
                    lua,
                    "expect_item",
                    format!("{id} x{expected}"),
                    format!("{id} x{actual}"),
                ))
            }
        })?,
    )?;
    env.raw_set(
        "expect_dialogue",
        lua.create_function(|lua, text: Value| {
            let text = string_arg(lua, "expect_dialogue", "text", &text)?;
            let actual = snapshot(lua, |s| s.dialogue.clone())?;
            if actual.as_deref() == Some(text.as_str()) {
                Ok(())
            } else {
                Err(mismatch(
                    lua,
                    "expect_dialogue",
                    format!("{text:?}"),
                    actual.map_or_else(|| "no dialogue".into(), |t| format!("{t:?}")),
                ))
            }
        })?,
    )?;
    env.raw_set(
        "expect_choices",
        lua.create_function(|lua, options: Value| {
            let expected = from_lua(lua, "expect_choices", &options)?;
            let actual = snapshot(lua, |s| s.choices.clone())?;
            let actual = ScriptValue::List(actual.into_iter().map(ScriptValue::String).collect());
            let expected = match expected {
                ScriptValue::Map(m) if m.is_empty() => ScriptValue::List(Vec::new()),
                other => other,
            };
            if values_equal(&actual, &expected) {
                Ok(())
            } else {
                Err(mismatch(
                    lua,
                    "expect_choices",
                    format_value(&expected),
                    format_value(&actual),
                ))
            }
        })?,
    )?;
    env.raw_set(
        "fail",
        lua.create_function(|lua, message: Option<String>| {
            Err::<(), _>(usage(
                lua,
                "fail",
                message.unwrap_or_else(|| "failed".into()),
            ))
        })?,
    )?;
    Ok(())
}

fn expect_placement(
    lua: &Lua,
    check: &str,
    pos: GridPos,
    facing: Option<Direction>,
    actual: Option<crate::scenario::Placement>,
) -> mlua::Result<()> {
    let Some(actual) = actual else {
        return Err(mismatch(
            lua,
            check,
            format_placement(pos, facing),
            "nothing there".into(),
        ));
    };
    let facing_ok = facing.is_none_or(|f| f == actual.facing);
    if actual.pos == pos && facing_ok {
        Ok(())
    } else {
        Err(mismatch(
            lua,
            check,
            format_placement(pos, facing),
            format_placement(actual.pos, facing.map(|_| actual.facing)),
        ))
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use std::collections::BTreeMap;

    use super::*;
    use crate::scenario::Placement;

    fn snap(tick: u64) -> ScenarioSnapshot {
        ScenarioSnapshot {
            tick,
            state: "Overworld".into(),
            map: Some("town".into()),
            player: Some(Placement {
                pos: GridPos::new(5, 7),
                facing: Direction::Down,
            }),
            player_idle: true,
            flags: BTreeMap::from([("met".to_owned(), true)]),
            vars: BTreeMap::from([("count".to_owned(), ScriptValue::Int(3))]),
            items: BTreeMap::from([("potion".to_owned(), 2)]),
            npcs: BTreeMap::from([(
                "guide".to_owned(),
                Placement {
                    pos: GridPos::new(5, 5),
                    facing: Direction::Down,
                },
            )]),
            actions: ["confirm", "cancel", "up", "down", "left", "right"]
                .into_iter()
                .map(String::from)
                .collect(),
            ..ScenarioSnapshot::default()
        }
    }

    fn scenario(source: &str) -> LuaScenario {
        LuaScenario::load("tests/unit.lua", source, ScriptLimits::default()).unwrap()
    }

    fn run(source: &str) -> ScenarioStep {
        scenario(source).resume(snap(10))
    }

    fn failure(source: &str) -> ScenarioFailure {
        match run(source) {
            ScenarioStep::Failed(f) => f,
            other => panic!("expected a failure, got {other:?}"),
        }
    }

    fn battle_snap(tick: u64) -> ScenarioSnapshot {
        ScenarioSnapshot {
            state: "Battle".into(),
            choices: vec!["FIGHT".into(), "BAG".into()],
            cursor: Some(0),
            battle: Some(BattleSnapshot {
                menu: Some("action".into()),
                turn: 2,
                player: Some(BattlerSnapshot {
                    name: "Hero".into(),
                    level: 5,
                    hp: 12,
                    max_hp: 20,
                }),
                opponent: None,
            }),
            ..snap(tick)
        }
    }

    #[test]
    fn battle_query_and_choose_in_a_battle_menu() {
        let mut sc = scenario(
            r#"
            expect(battle().menu == "action" and battle().turn == 2)
            expect(battle().player.name == "Hero" and battle().player.hp == 12)
            expect_eq(battle().player.max_hp, 20)
            expect(battle().opponent == nil)
            choose(1)
        "#,
        );
        let step = sc.resume(battle_snap(10));
        assert_eq!(
            step,
            ScenarioStep::Advance {
                ticks: 1,
                held: vec!["confirm".into()]
            }
        );
        let f = failure("advance_battle()");
        assert_eq!(f.check, "advance_battle");
        assert_eq!(f.expected.as_deref(), Some("Battle"));
        let none = run("expect(battle() == nil)");
        assert_eq!(none, ScenarioStep::Finished);
    }

    #[test]
    fn party_query_and_choose_in_the_evolution_scene() {
        let mut sc = scenario(
            r#"
            expect_eq(#party(), 1)
            local m = party()[1]
            expect(m.species == "bloomling" and m.name == "Bloomling")
            expect(m.level == 6 and m.exp == 229 and m.max_hp == 24)
            expect(m.attack == 9 and m.defense == 8 and m.speed == 7)
            expect_eq(m.moves, { "tackle", "vine_whip" })
            expect_state("Evolution")
            choose(2)
        "#,
        );
        let snap = ScenarioSnapshot {
            state: "Evolution".into(),
            choices: vec!["Tackle".into(), "Don't learn".into()],
            cursor: Some(1),
            party: vec![PartyMemberSnapshot {
                species: "bloomling".into(),
                name: "Bloomling".into(),
                level: 6,
                exp: 229,
                moves: vec!["tackle".into(), "vine_whip".into()],
                max_hp: 24,
                attack: 9,
                defense: 8,
                speed: 7,
            }],
            ..snap(4)
        };
        assert_eq!(
            sc.resume(snap),
            ScenarioStep::Advance {
                ticks: 1,
                held: vec!["confirm".into()]
            }
        );
        assert_eq!(run("expect_eq(#party(), 0)"), ScenarioStep::Finished);
    }

    #[test]
    fn passing_expectations_finish() {
        let step = run(r#"
            expect_state("Overworld")
            expect_map("town")
            expect_player_at(5, 7)
            expect_player_at(5, 7, "down")
            expect_npc_at("guide", 5, 5, "down")
            expect_flag("met")
            expect_flag("missing", false)
            expect_var("count", 3.0)
            expect_var("nothing", nil)
            expect_item("potion", 2)
            expect_eq(tick(), 10)
            expect_eq({1, 2}, {1, 2})
            expect(idle() and not locked())
            local x, y, f = player()
            expect(x == 5 and y == 7 and f == "down")
            expect(npc("nobody") == nil)
            expect(dialogue() == nil and choices() == nil and cursor() == nil)
        "#);
        assert_eq!(step, ScenarioStep::Finished);
    }

    #[test]
    fn failed_expectation_reports_line_tick_and_values() {
        let f = failure("expect_map('town')\n\nexpect_state('Dialogue')\n");
        assert_eq!(f.check, "expect_state");
        assert_eq!(
            f.location,
            Some(SourceLocation {
                file: "tests/unit.lua".into(),
                line: 3
            })
        );
        assert_eq!(f.tick, 10);
        assert_eq!(f.expected.as_deref(), Some("Dialogue"));
        assert_eq!(f.actual.as_deref(), Some("Overworld"));
        assert_eq!(
            f.to_string(),
            "tests/unit.lua:3: expect_state failed at tick 10\n  expected: Dialogue\n    actual: Overworld"
        );
    }

    #[test]
    fn each_expectation_formats_its_mismatch() {
        let cases: &[(&str, &str, &str)] = &[
            ("expect_flag('met', false)", "met = false", "met = true"),
            ("expect_var('count', 'x')", "count = \"x\"", "count = 3"),
            ("expect_item('potion', 1)", "potion x1", "potion x2"),
            ("expect_player_at(1, 2)", "(1, 2)", "(5, 7)"),
            (
                "expect_player_at(5, 7, 'up')",
                "(5, 7) facing up",
                "(5, 7) facing down",
            ),
            ("expect_npc_at('ghost', 1, 1)", "(1, 1)", "nothing there"),
            ("expect_map('cave')", "cave", "town"),
            ("expect_dialogue('hi')", "\"hi\"", "no dialogue"),
            ("expect_choices({'Yes', 'No'})", "{\"Yes\", \"No\"}", "{}"),
            ("expect_eq(1, 2)", "2", "1"),
        ];
        for (source, expected, actual) in cases {
            let f = failure(source);
            assert_eq!(f.expected.as_deref(), Some(*expected), "{source}");
            assert_eq!(f.actual.as_deref(), Some(*actual), "{source}");
            assert_eq!(f.location.as_ref().map(|l| l.line), Some(1), "{source}");
        }
    }

    #[test]
    fn expect_and_fail_carry_messages() {
        let f = failure("expect(false, 'door is open')");
        assert_eq!(
            (f.check.as_str(), f.message.as_deref()),
            ("expect", Some("door is open"))
        );
        let f = failure("fail('nope')");
        assert_eq!(
            (f.check.as_str(), f.message.as_deref()),
            ("fail", Some("nope"))
        );
    }

    #[test]
    fn pcall_cannot_swallow_a_failure() {
        let f = failure("pcall(expect_state, 'Battle')\nexpect(true)");
        assert_eq!(f.check, "expect_state");
    }

    #[test]
    fn press_holds_then_releases() {
        let mut s = scenario("press('confirm')\nhold('up')\nwait_ticks(3)\nrelease_all()");
        assert_eq!(
            s.resume(snap(0)),
            ScenarioStep::Advance {
                ticks: 1,
                held: vec!["confirm".into()]
            }
        );
        assert_eq!(
            s.resume(snap(1)),
            ScenarioStep::Advance {
                ticks: 1,
                held: vec![]
            }
        );
        assert_eq!(
            s.resume(snap(2)),
            ScenarioStep::Advance {
                ticks: 3,
                held: vec!["up".into()]
            }
        );
        assert_eq!(s.resume(snap(5)), ScenarioStep::Finished);
        assert_eq!(s.resume(snap(5)), ScenarioStep::Finished);
    }

    #[test]
    fn unknown_action_is_a_located_usage_error() {
        let f = failure("\npress('jump')");
        assert_eq!(f.check, "hold");
        assert_eq!(f.location.as_ref().map(|l| l.line), Some(2));
        assert!(f.message.unwrap().contains("unknown action \"jump\""));
    }

    #[test]
    fn helpers_report_the_scenario_line() {
        let f = failure("\n\nwait_for_state('Battle', 0)");
        assert_eq!(f.check, "wait_for_state");
        assert_eq!(f.location.as_ref().map(|l| l.line), Some(3));
        assert_eq!(f.expected.as_deref(), Some("Battle"));
        let f = failure("choose(1)");
        assert_eq!(
            (f.expected.as_deref(), f.actual.as_deref()),
            (Some("Choice"), Some("Overworld"))
        );
        let f = failure("wait_ticks(-1)");
        assert_eq!(f.check, "wait_ticks");
    }

    #[test]
    fn wait_until_times_out() {
        let mut s = scenario("wait_until(function() return flag('never') end, 2, 'never')");
        assert!(matches!(
            s.resume(snap(0)),
            ScenarioStep::Advance { ticks: 1, .. }
        ));
        assert!(matches!(
            s.resume(snap(1)),
            ScenarioStep::Advance { ticks: 1, .. }
        ));
        match s.resume(snap(2)) {
            ScenarioStep::Failed(f) => {
                assert_eq!(f.check, "wait_until");
                assert_eq!(f.expected.as_deref(), Some("never"));
                assert_eq!(f.tick, 2);
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn script_errors_are_located_failures() {
        let f = failure("local x = nil\nlocal y = x.field");
        assert_eq!(f.check, "error");
        assert_eq!(f.location.as_ref().map(|l| l.line), Some(2));
    }

    #[test]
    fn syntax_errors_fail_to_load() {
        assert!(matches!(
            LuaScenario::load("tests/bad.lua", "expect(", ScriptLimits::default()),
            Err(ScriptError::Syntax { .. })
        ));
    }

    #[test]
    fn raw_yield_is_rejected() {
        let f = failure("coroutine.yield('x')");
        assert!(f.message.unwrap().contains("outside the test API"));
    }

    #[test]
    fn runaway_loop_hits_the_budget() {
        let mut s = LuaScenario::load(
            "tests/loop.lua",
            "while true do end",
            ScriptLimits {
                instruction_budget: 100_000,
                ..ScriptLimits::default()
            },
        )
        .unwrap();
        match s.resume(snap(0)) {
            ScenarioStep::Failed(f) => assert!(f.message.unwrap().contains("budget")),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn sandbox_has_only_the_test_api() {
        let step = run(r#"
            expect(io == nil and os == nil and require == nil and load == nil and debug == nil)
            expect(game == nil and ui == nil and events == nil)
            expect(math.random == nil)
            expect(type(string.format) == "function")
        "#);
        assert_eq!(step, ScenarioStep::Finished);
        let s = scenario("");
        // The API lives only in the scenario's _ENV, never in the real globals.
        let global: bool = s.lua.load("return press == nil").eval().unwrap();
        assert!(global);
        for name in SCENARIO_API {
            let ok: bool = s
                .lua
                .load(format!("return {name} ~= nil"))
                .set_environment(build_env(&s.lua, "tests/env.lua").unwrap())
                .eval()
                .unwrap();
            assert!(ok, "{name} missing");
        }
    }
}
