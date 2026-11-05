//! `LuaScriptHost`: the `mlua` adapter for the [`ScriptHost`] port (§4.5, §9.6).
//!
//! This folder is the only place in the workspace that names `mlua` types. Lua 5.4 is vendored
//! (`mlua` features `lua54` + `vendored`), so no system Lua is needed on any OS (§0.4).

mod convert;
mod sandbox;

use std::cell::Cell;
use std::collections::BTreeMap;
use std::rc::Rc;

use mlua::{ChunkMode, HookTriggers, Lua, LuaOptions, MultiValue, StdLib, Table, Value, VmState};
use pkrs_core::GameRng;

use crate::error::{Result, ScriptError, split_location};
use crate::host::{ScriptFiles, ScriptHost, ScriptLimits, ScriptOwner, ScriptValue};

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
        })
    }

    /// The limits this host enforces.
    #[must_use]
    pub fn limits(&self) -> ScriptLimits {
        self.limits
    }

    fn env(&mut self, owner: &ScriptOwner) -> Result<Table> {
        if let Some(env) = self.envs.get(owner) {
            return Ok(env.clone());
        }
        let env = sandbox::build_env(&self.lua, owner, &self.guard).map_err(engine)?;
        self.envs.insert(owner.clone(), env.clone());
        Ok(env)
    }

    /// Runs `f` as one protected top-level call: fresh instruction budget, errors mapped to
    /// [`ScriptError`] and logged to `tracing` (target `script`) with location and traceback.
    fn protected<R>(&self, chunk: &str, f: impl FnOnce(&Lua) -> mlua::Result<R>) -> Result<R> {
        self.budget.used.set(0);
        self.budget.tripped.set(false);
        let result = f(&self.lua);
        let tripped = self.budget.tripped.replace(false);
        result.map_err(|err| {
            let err = if tripped {
                ScriptError::InstructionBudget {
                    budget: self.limits.instruction_budget,
                }
            } else {
                map_error(err, chunk, self.limits)
            };
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

    fn function(&mut self, owner: &ScriptOwner, name: &str) -> Result<Option<mlua::Function>> {
        let env = self.env(owner)?;
        match env.raw_get::<Value>(name).map_err(engine)? {
            Value::Function(f) => Ok(Some(f)),
            _ => Ok(None),
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
