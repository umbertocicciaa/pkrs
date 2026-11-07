//! `LuaScriptHost`: the `mlua` adapter for the [`ScriptHost`] port (§4.5, §9.6).
//!
//! This folder is the only place in the workspace that names `mlua` types. Lua 5.4 is vendored
//! (`mlua` features `lua54` + `vendored`), so no system Lua is needed on any OS (§0.4).

mod ai;
pub(crate) mod battle;
mod camera;
mod convert;
mod dialogue;
mod events;
mod facade;
mod game;
mod hook_order;
mod log;
mod map;
mod npc;
mod player;
mod plugin;
mod sandbox;
mod time;
mod ui;

pub use ai::{DEFAULT_AI_MAX_FAILURES, LuaAi};
pub use battle::{DEFAULT_MAX_FAILURES, HOOK_NAMES, LuaBattleHooks};
pub use ui::GfxOp;

use std::cell::Cell;
use std::collections::BTreeMap;
use std::rc::Rc;

use mlua::{
    ChunkMode, HookTriggers, Lua, LuaOptions, MultiValue, StdLib, Table, Thread, ThreadStatus,
    Value, VmState,
};
use pkrs_core::GameRng;
use pkrs_core::InputBuiltinPressed;

use crate::error::{Result, ScriptError, split_location};
use crate::events::{ListenerId, ListenerInfo, ScriptEvent};
use crate::host::{
    BattleRequest, ScriptFiles, ScriptHost, ScriptLimits, ScriptOwner, ScriptValue, TaskId,
    TaskStatus, WaitRequest,
};
use crate::input_bridge::{LastScriptInput, ScriptInputSnapshot};
use crate::world::ScriptWorld;

/// Instructions between two budget checks.
const HOOK_STRIDE: u32 = 1_000;

/// Shared with the instruction hook.
struct Budget {
    limit: u64,
    used: Cell<u64>,
    tripped: Cell<bool>,
}

/// Sandboxed Lua 5.4 state implementing [`ScriptHost`].
pub struct LuaScriptHost {
    lua: Lua,
    limits: ScriptLimits,
    budget: Rc<Budget>,
    guard: mlua::Function,
    envs: BTreeMap<ScriptOwner, Table>,
    tasks: BTreeMap<TaskId, Thread>,
    next_task: TaskId,
}

impl LuaScriptHost {
    /// Creates a sandboxed state reading scripts from `files`, with `math.random` bound to `rng`.
    pub fn new(
        files: Box<dyn ScriptFiles>,
        rng: Box<dyn GameRng>,
        limits: ScriptLimits,
    ) -> Result<Self> {
        let libs = StdLib::COROUTINE | StdLib::TABLE | StdLib::STRING | StdLib::UTF8 | StdLib::MATH;
        let lua = Lua::new_with(libs, LuaOptions::default()).map_err(engine)?;
        sandbox::harden_globals(&lua).map_err(engine)?;
        if limits.memory_limit > 0 {
            lua.set_memory_limit(limits.memory_limit).map_err(engine)?;
        }
        lua.set_app_data(sandbox::Files(files));
        lua.set_app_data(sandbox::Rng(rng));
        lua.set_app_data(battle::Registry::default());
        lua.set_app_data(events::EventRegistry::default());
        lua.set_app_data(facade::WorldSlot::default());
        time::init(&lua);
        npc::init(&lua).map_err(engine)?;

        let budget = Rc::new(Budget {
            limit: limits.instruction_budget,
            used: Cell::new(0),
            tripped: Cell::new(false),
        });
        let hook_budget = Rc::clone(&budget);
        // Global hook: coroutines created by scripts inherit it, so they are budgeted too.
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

        let guard_budget = Rc::clone(&budget);
        let guard = lua
            .create_function(move |_, args: MultiValue| {
                if guard_budget.tripped.get() {
                    Err(mlua::Error::runtime("instruction budget exceeded"))
                } else {
                    Ok(args)
                }
            })
            .map_err(engine)?;

        Ok(Self {
            lua,
            limits,
            budget,
            guard,
            envs: BTreeMap::new(),
            tasks: BTreeMap::new(),
            next_task: 1,
        })
    }

    /// The limits this host enforces.
    #[must_use]
    pub fn limits(&self) -> ScriptLimits {
        self.limits
    }

    /// Attaches the engine world the API v0 bindings (`game`, `map`, `npc`, `player`, `ui`,
    /// `camera`) talk to (§9.1). Until then those calls raise a script error.
    pub fn set_world(&mut self, world: Box<dyn ScriptWorld>) {
        self.lua.set_app_data(facade::WorldSlot(Some(world)));
    }

    /// Detaches and returns the world set with [`set_world`](Self::set_world).
    pub fn take_world(&mut self) -> Option<Box<dyn ScriptWorld>> {
        self.lua
            .app_data_mut::<facade::WorldSlot>()
            .and_then(|mut slot| slot.0.take())
    }

    /// Unloads `owner` (e.g. the previous map's script on map change, §6.4): its event
    /// listeners are removed and its `_ENV` is dropped, so the next chunk run for `owner`
    /// starts from a fresh environment. Tasks it already started keep running until they
    /// finish (§9.8). Returns the number of listeners removed.
    pub fn unload_owner(&mut self, owner: &ScriptOwner) -> usize {
        self.envs.remove(owner);
        events::remove_owner(&self.lua, owner)
    }

    /// `true` if the project has a script file at `path` (e.g. `scripts/maps/route_1.lua`).
    #[must_use]
    pub fn script_exists(&self, path: &str) -> bool {
        self.lua
            .app_data_ref::<sandbox::Files>()
            .is_some_and(|files| files.0.exists(path))
    }

    /// `true` if `owner` currently has an environment (a chunk ran for it and it was not
    /// unloaded).
    #[must_use]
    pub fn is_loaded(&self, owner: &ScriptOwner) -> bool {
        self.envs.contains_key(owner)
    }

    fn start_thread(
        &mut self,
        function: mlua::Function,
        args: &[ScriptValue],
    ) -> Result<(TaskId, TaskStatus)> {
        let thread = self.lua.create_thread(function).map_err(engine)?;
        let args = args
            .iter()
            .map(|a| convert::to_lua(&self.lua, a))
            .collect::<mlua::Result<MultiValue>>()
            .map_err(engine)?;
        let id = self.next_task;
        self.next_task = self.next_task.wrapping_add(1).max(1);
        let status = self.run_task(id, thread, args)?;
        Ok((id, status))
    }

    fn env(&mut self, owner: &ScriptOwner) -> Result<Table> {
        if let Some(env) = self.envs.get(owner) {
            return Ok(env.clone());
        }
        let env = sandbox::build_env(&self.lua, owner, &self.guard).map_err(engine)?;
        self.envs.insert(owner.clone(), env.clone());
        Ok(env)
    }

    /// Runs `f` as one protected top-level call with a fresh instruction budget; errors are
    /// mapped to [`ScriptError`] but not logged (see [`protected`](Self::protected)).
    pub(crate) fn call_protected<R>(
        &self,
        chunk: &str,
        f: impl FnOnce(&Lua) -> mlua::Result<R>,
    ) -> Result<R> {
        self.budget.used.set(0);
        self.budget.tripped.set(false);
        let result = f(&self.lua);
        let tripped = self.budget.tripped.replace(false);
        result.map_err(|err| {
            if tripped {
                ScriptError::InstructionBudget {
                    budget: self.limits.instruction_budget,
                }
            } else {
                map_error(err, chunk, self.limits)
            }
        })
    }

    /// [`call_protected`](Self::call_protected), logging failures to `tracing` (target
    /// `script`) with location and traceback.
    fn protected<R>(&self, chunk: &str, f: impl FnOnce(&Lua) -> mlua::Result<R>) -> Result<R> {
        self.call_protected(chunk, f).map_err(|err| {
            match err.location() {
                Some((file, line)) => tracing::error!(
                    target: "script",
                    file,
                    line,
                    traceback = err.traceback().unwrap_or(""),
                    "{err}"
                ),
                None => tracing::error!(target: "script", chunk, "{err}"),
            }
            err
        })
    }

    /// Battle hooks dispatching to the handlers registered with `battle.on` (§7.4).
    #[must_use]
    pub fn battle_hooks(&self) -> LuaBattleHooks<'_> {
        LuaBattleHooks::new(self)
    }

    /// Number of live tasks (yielded and waiting).
    #[must_use]
    pub fn task_count(&self) -> usize {
        self.tasks.len()
    }

    fn run_task(&mut self, id: TaskId, thread: Thread, args: MultiValue) -> Result<TaskStatus> {
        let resumed = self.protected("task", |_| thread.resume::<MultiValue>(args));
        let values = match resumed {
            Ok(v) => v,
            Err(err) => {
                self.tasks.remove(&id);
                return Err(err);
            }
        };
        let first = values.into_iter().next().unwrap_or(Value::Nil);
        if thread.status() == ThreadStatus::Resumable {
            let request = wait_request(&first)?;
            self.tasks.insert(id, thread);
            Ok(TaskStatus::Waiting(request))
        } else {
            self.tasks.remove(&id);
            Ok(TaskStatus::Finished(convert::from_lua(&first)?))
        }
    }

    fn function(&mut self, owner: &ScriptOwner, name: &str) -> Result<Option<mlua::Function>> {
        let env = self.env(owner)?;
        match env.raw_get::<Value>(name).map_err(engine)? {
            Value::Function(f) => Ok(Some(f)),
            _ => Ok(None),
        }
    }

    /// Updates `__pkrs_input.pressed` for Lua UI (`input.pressed("cancel")`, §9.5 §10).
    pub fn sync_script_input(
        &mut self,
        owner: &ScriptOwner,
        input: &impl InputBuiltinPressed,
    ) -> Result<()> {
        self.lua
            .set_app_data(LastScriptInput(ScriptInputSnapshot::capture(input)));
        let env = self.env(owner)?;
        ui::ensure_input_table(&self.lua, &env).map_err(engine)?;
        Ok(())
    }

    /// Returns the handler table registered with `ui.register_screen`, if any.
    pub fn get_ui_screen_table(
        &mut self,
        owner: &ScriptOwner,
        screen_id: &str,
    ) -> Result<Option<Table>> {
        let env = self.env(owner)?;
        let screens: Value = env.raw_get("__pkrs_ui_screens").map_err(engine)?;
        let Value::Table(screens) = screens else {
            return Ok(None);
        };
        let t: Value = screens.raw_get(screen_id).map_err(engine)?;
        match t {
            Value::Table(tab) => Ok(Some(tab)),
            _ => Ok(None),
        }
    }

    /// Invokes a Lua UI screen handler: `on_enter`, `update`, or `render` (§9.5).
    ///
    /// Missing optional methods return [`ScriptValue::Nil`]. For `render`, recorded ops are read
    /// with [`take_ui_gfx_ops`](Self::take_ui_gfx_ops).
    pub fn ui_invoke(
        &mut self,
        owner: &ScriptOwner,
        screen_id: &str,
        method: UiScreenMethod,
        screen_table: &Table,
        arg: Option<ScriptValue>,
    ) -> Result<ScriptValue> {
        let method_name = method.as_str();
        let method_fn: Value = screen_table.raw_get(method_name).map_err(engine)?;
        let f = match method_fn {
            Value::Function(f) => f,
            Value::Nil => return Ok(ScriptValue::Nil),
            _ => {
                return Err(ScriptError::Conversion(format!(
                    "ui screen `{screen_id}`: `{method_name}` must be a function or nil"
                )));
            }
        };
        let env = self.env(owner)?;
        ui::ensure_input_table(&self.lua, &env).map_err(engine)?;
        let instance = screen_table.clone();
        let args = match method {
            UiScreenMethod::OnEnter => {
                let a = match arg {
                    Some(v) => convert::to_lua(&self.lua, &v).map_err(engine)?,
                    None => Value::Nil,
                };
                MultiValue::from_vec(vec![Value::Table(instance), a])
            }
            UiScreenMethod::Update => {
                let input: Table = env.raw_get("__pkrs_input").map_err(engine)?;
                MultiValue::from_vec(vec![Value::Table(instance), Value::Table(input)])
            }
            UiScreenMethod::Render => {
                ui::begin_gfx_recording(&self.lua);
                let gfx = ui::gfx_api_table(&self.lua)?;
                MultiValue::from_vec(vec![Value::Table(instance), Value::Table(gfx)])
            }
        };
        let results = self.protected(&format!("ui:{screen_id}:{method_name}"), |_| {
            f.call::<MultiValue>(args)
        })?;
        let first = results.into_iter().next().unwrap_or(Value::Nil);
        convert::from_lua(&first)
    }

    /// Draw commands recorded during the last [`ui_invoke`](Self::ui_invoke) with [`UiScreenMethod::Render`].
    #[must_use]
    pub fn take_ui_gfx_ops(&mut self) -> Vec<ui::GfxOp> {
        ui::take_gfx_ops(&self.lua)
    }

    /// Runs the screen's `on_enter(self, args?)` if registered.
    pub fn ui_screen_on_enter(
        &mut self,
        owner: &ScriptOwner,
        screen_id: &str,
        args: ScriptValue,
    ) -> Result<()> {
        let Some(table) = self.get_ui_screen_table(owner, screen_id)? else {
            return Err(ScriptError::Engine(format!(
                "ui screen `{screen_id}` is not registered for {owner}"
            )));
        };
        self.ui_invoke(
            owner,
            screen_id,
            UiScreenMethod::OnEnter,
            &table,
            Some(args),
        )?;
        Ok(())
    }

    /// Runs `update(self, input)`; missing `update` returns [`ScriptValue::Nil`].
    pub fn ui_screen_update(
        &mut self,
        owner: &ScriptOwner,
        screen_id: &str,
    ) -> Result<ScriptValue> {
        let Some(table) = self.get_ui_screen_table(owner, screen_id)? else {
            return Err(ScriptError::Engine(format!(
                "ui screen `{screen_id}` is not registered for {owner}"
            )));
        };
        self.ui_invoke(owner, screen_id, UiScreenMethod::Update, &table, None)
    }

    /// Runs `render(self, gfx)` and returns recorded draw commands.
    pub fn ui_screen_render_ops(
        &mut self,
        owner: &ScriptOwner,
        screen_id: &str,
    ) -> Result<Vec<ui::GfxOp>> {
        let Some(table) = self.get_ui_screen_table(owner, screen_id)? else {
            return Err(ScriptError::Engine(format!(
                "ui screen `{screen_id}` is not registered for {owner}"
            )));
        };
        self.ui_invoke(owner, screen_id, UiScreenMethod::Render, &table, None)?;
        Ok(self.take_ui_gfx_ops())
    }
}

/// Lua screen lifecycle method (§9.5).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UiScreenMethod {
    /// `on_enter(self, args?)`.
    OnEnter,
    /// `update(self, input)`.
    Update,
    /// `render(self, gfx)`.
    Render,
}

impl UiScreenMethod {
    fn as_str(self) -> &'static str {
        match self {
            Self::OnEnter => "on_enter",
            Self::Update => "update",
            Self::Render => "render",
        }
    }
}

impl ScriptHost for LuaScriptHost {
    fn run_file(&mut self, owner: &ScriptOwner, path: &str) -> Result<ScriptValue> {
        let source = {
            let files = self
                .lua
                .app_data_ref::<sandbox::Files>()
                .ok_or_else(|| ScriptError::Engine("script files unavailable".into()))?;
            files.0.read_script(path)?
        };
        self.run_source(owner, path, &source)
    }

    fn run_source(
        &mut self,
        owner: &ScriptOwner,
        chunk_name: &str,
        source: &str,
    ) -> Result<ScriptValue> {
        let env = self.env(owner)?;
        let value = self.protected(chunk_name, |lua| {
            lua.load(source)
                .set_name(format!("@{chunk_name}"))
                .set_mode(ChunkMode::Text)
                .set_environment(env)
                .eval::<Value>()
        })?;
        convert::from_lua(&value)
    }

    fn call_function(
        &mut self,
        owner: &ScriptOwner,
        name: &str,
        args: &[ScriptValue],
    ) -> Result<ScriptValue> {
        let Some(function) = self.function(owner, name)? else {
            return Err(ScriptError::NoSuchFunction {
                owner: owner.to_string(),
                name: name.to_owned(),
            });
        };
        let args = args
            .iter()
            .map(|a| convert::to_lua(&self.lua, a))
            .collect::<mlua::Result<MultiValue>>()
            .map_err(engine)?;
        let results = self.protected(name, |_| function.call::<MultiValue>(args))?;
        match results.into_iter().next() {
            Some(first) => convert::from_lua(&first),
            None => Ok(ScriptValue::Nil),
        }
    }

    fn has_function(&mut self, owner: &ScriptOwner, name: &str) -> bool {
        matches!(self.function(owner, name), Ok(Some(_)))
    }

    fn global(&mut self, owner: &ScriptOwner, name: &str) -> Result<ScriptValue> {
        let env = self.env(owner)?;
        let value = env.raw_get::<Value>(name).map_err(engine)?;
        convert::from_lua(&value)
    }

    fn memory_used(&self) -> usize {
        self.lua.used_memory()
    }

    fn start_task(
        &mut self,
        owner: &ScriptOwner,
        name: &str,
        args: &[ScriptValue],
    ) -> Result<(TaskId, TaskStatus)> {
        let Some(function) = self.function(owner, name)? else {
            return Err(ScriptError::NoSuchFunction {
                owner: owner.to_string(),
                name: name.to_owned(),
            });
        };
        self.start_thread(function, args)
    }

    fn resume_task(&mut self, task: TaskId, answer: ScriptValue) -> Result<TaskStatus> {
        let thread = self
            .tasks
            .remove(&task)
            .ok_or_else(|| ScriptError::Engine(format!("no waiting task with id {task}")))?;
        let answer = convert::to_lua(&self.lua, &answer).map_err(engine)?;
        self.run_task(task, thread, MultiValue::from_vec(vec![answer]))
    }

    fn cancel_task(&mut self, task: TaskId) -> bool {
        self.tasks.remove(&task).is_some()
    }

    fn set_tick(&mut self, tick: u64) {
        time::set_tick(&self.lua, tick);
    }

    fn emit_event(&mut self, event: ScriptEvent) {
        events::emit(&self.lua, event);
    }

    fn take_events(&mut self) -> Vec<ScriptEvent> {
        events::take(&self.lua)
    }

    fn listeners(&mut self, event: &str) -> Vec<ListenerInfo> {
        events::listeners(&self.lua, event)
    }

    fn start_listener(
        &mut self,
        listener: ListenerId,
        args: &[ScriptValue],
    ) -> Result<(TaskId, TaskStatus)> {
        let function = events::function(&self.lua, listener)
            .ok_or_else(|| ScriptError::Engine(format!("no event listener with id {listener}")))?;
        self.start_thread(function, args)
    }
}

/// Installs the API v0 tables (§9.1) into a new environment. `time` and the `ui` screen
/// functions are installed by their own modules first.
fn install_api_v0(lua: &Lua, env: &Table, owner: &ScriptOwner) -> mlua::Result<()> {
    game::install(lua, env)?;
    map::install(lua, env)?;
    player::install(lua, env)?;
    dialogue::install(lua, env)?;
    camera::install(lua, env)?;
    log::install(lua, env, owner)?;
    events::install(lua, env, owner)
}

/// Reads what a yielded task waits for; `battle.start_*` yields a `{ wait = "battle" }` table,
/// `time.wait` a `{ wait = "ticks" }` one.
fn wait_request(value: &Value) -> Result<WaitRequest> {
    let conv = |e: mlua::Error| ScriptError::Conversion(e.to_string());
    if let Value::Table(t) = value {
        if let Some(request) = time::parse_wait(t)? {
            return Ok(request);
        }
        if let Some(request) = ui::parse_ui_push(t)? {
            return Ok(request);
        }
        if t.raw_get::<Option<String>>("wait")
            .map_err(conv)?
            .as_deref()
            == Some("battle")
        {
            let kind: String = t.raw_get("kind").map_err(conv)?;
            let request = match kind.as_str() {
                "wild" => {
                    let species: String = t.raw_get("species").map_err(conv)?;
                    let level: i64 = t.raw_get("level").map_err(conv)?;
                    let level = u8::try_from(level)
                        .ok()
                        .filter(|l| (1..=100).contains(l))
                        .ok_or_else(|| {
                            ScriptError::Conversion(format!(
                                "battle.start_wild: level must be 1-100 (got {level})"
                            ))
                        })?;
                    BattleRequest::Wild { species, level }
                }
                "trainer" => BattleRequest::Trainer {
                    id: t.raw_get("id").map_err(conv)?,
                },
                other => {
                    return Err(ScriptError::Conversion(format!(
                        "unknown battle request kind `{other}`"
                    )));
                }
            };
            return Ok(WaitRequest::Battle(request));
        }
    }
    Ok(WaitRequest::Other(convert::from_lua(value)?))
}

fn engine(err: mlua::Error) -> ScriptError {
    ScriptError::Engine(err.to_string())
}

/// Maps an `mlua` error to [`ScriptError`], keeping `file:line` and the traceback.
fn map_error(err: mlua::Error, chunk: &str, limits: ScriptLimits) -> ScriptError {
    match err {
        mlua::Error::MemoryError(_) => ScriptError::MemoryLimit {
            limit: limits.memory_limit,
        },
        mlua::Error::SyntaxError { message, .. } => ScriptError::Syntax { message },
        mlua::Error::RuntimeError(text) => runtime_error(&text, chunk),
        mlua::Error::CallbackError { traceback, cause } => {
            match map_error((*cause).clone(), chunk, limits) {
                ScriptError::Located {
                    file,
                    line,
                    message,
                    traceback: inner,
                } if inner.is_empty() => ScriptError::Located {
                    file,
                    line,
                    message,
                    traceback,
                },
                other => other,
            }
        }
        other => ScriptError::Runtime {
            chunk: chunk.to_owned(),
            message: other.to_string(),
        },
    }
}

fn runtime_error(text: &str, chunk: &str) -> ScriptError {
    let (message, traceback) = match text.split_once("\nstack traceback:") {
        Some((msg, tb)) => (msg, format!("stack traceback:{tb}")),
        None => (text, String::new()),
    };
    match split_location(message) {
        Some((file, line, rest)) => ScriptError::Located {
            file: file.to_owned(),
            line,
            message: rest.to_owned(),
            traceback,
        },
        None => ScriptError::Runtime {
            chunk: chunk.to_owned(),
            message: message.to_owned(),
        },
    }
}

#[cfg(test)]
mod tests;
