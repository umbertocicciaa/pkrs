//! Lua scripting: runtime, sandbox, API bindings, coroutine scheduler and plugin loader.
//!
//! - **Ports:** [`ScriptHost`] (run chunks, call functions) and [`ScriptFiles`] (read project
//!   scripts by path; the runtime implements it over the assets VFS). Both use engine types
//!   only ([`ScriptValue`], [`ScriptOwner`], [`ScriptError`]).
//! - **Adapter:** [`LuaScriptHost`] embeds vendored Lua 5.4 through `mlua` with the §9.6
//!   sandbox: whitelisted libraries, per-owner `_ENV`, restricted `require`, engine-RNG
//!   `math.random`, instruction budget, memory limit, and errors with `file:line` + traceback
//!   routed to `tracing` (target `script`).
//! - **Battle hooks:** [`LuaBattleHooks`] implements `pkrs_battle::BattleHooks` by dispatching
//!   to handlers registered with `battle.on(...)`; `battle.start_wild` / `start_trainer` yield
//!   a [`WaitRequest::Battle`] from a task ([`ScriptHost::start_task`]).
//! - **Plugins:** [`plugin`] discovers `plugins/*/plugin.toml`, validates the enabled set and
//!   sorts it ([`plan_plugins`]), then runs each `init.lua` in its own `_ENV`
//!   ([`load_plugins`]) with `plugin.define()` bound to the manifest (§9.4, §9.7).

mod error;
mod host;
mod lua;
pub mod plugin;

pub use error::{Result, ScriptError, split_location};
pub use host::{
    BattleRequest, MemoryScripts, ScriptFiles, ScriptHost, ScriptLimits, ScriptOwner, ScriptValue,
    TaskId, TaskStatus, WaitRequest, battle_result_value,
};
pub use lua::{DEFAULT_MAX_FAILURES, HOOK_NAMES, LuaBattleHooks, LuaScriptHost};
pub use plugin::{
    PluginError, PluginErrors, PluginHost, PluginInitError, PluginLoadReport, PluginPlan,
    load_plugins, plan_plugins,
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_display_is_human_readable() {
        let err: ScriptError = ScriptError::Runtime {
            chunk: "main.lua".into(),
            message: "boom".into(),
        };
        assert_eq!(err.to_string(), "script error in `main.lua`: boom");
    }

    #[test]
    fn result_alias_defaults_to_crate_error() {
        let ok: Result<u8> = Ok(1);
        assert!(matches!(ok, Ok(1)));
    }
}
