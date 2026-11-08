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
//! - **Lua AI:** [`LuaAi`] implements `pkrs_battle::AiScript` by calling a profile script's
//!   `choose_action(view)` (loaded with [`LuaScriptHost::load_ai_script`]) or a plugin's
//!   `ai.register_profile(id, fn)` function (§7.5).
//! - **Scheduler:** [`Scheduler`] steps tasks once per tick in creation order, resuming each
//!   when its [`WaitCondition`] is met (`time.wait(n)`, dialogue closed, path finished, battle
//!   ended) and locking the player for interact/trigger scripts (§9.3).
//! - **Lua API v0 (§9.1):** namespaced `game`, `map`, `npc` objects, `player`, `ui`, `camera`,
//!   `time`, `log` and `events` tables in every environment. They reach the engine only through
//!   the [`ScriptWorld`] port ([`LuaScriptHost::set_world`]); blocking-looking calls
//!   (`ui.say`, `ui.choose`, `ui.yes_no`, `npc:move`, `wait`) yield to the [`Scheduler`].
//! - **Events (§9.2):** `events.on` / `events.emit` and the engine events ([`ScriptEvent`]) are
//!   queued and flushed once per tick with [`Scheduler::dispatch_events`], in emission order.
//! - **Plugins:** [`plugin`] discovers `plugins/*/plugin.toml`, validates the enabled set and
//!   sorts it ([`plan_plugins`]), then runs each `init.lua` in its own `_ENV`
//!   ([`load_plugins`]) with `plugin.define()` bound to the manifest (§9.4, §9.7).
//! - **Plugin registration (§9.4):** `data.register_*` adds content to the merged
//!   [`ContentDb`](pkrs_data::ContentDb) (data folders are merged first, [`merge_content`]),
//!   `world.register_terrain` / `world.register_field_action`, `battle.register_action` and
//!   `input.register_action` register terrain rules and field, battle and input actions, and
//!   `plugin.save` is the plugin's save namespace. The engine reads them through
//!   [`LuaScriptHost`] ([`FieldContext`], [`InputActionRequest`]); the host is also a
//!   `pkrs_world::TerrainRules`.
//! - **Hook chains:** [`chain`] orders handlers by priority, then plugin load order, then
//!   registration order, threads the value through them and stops on cancel (§9.4).
//!   [`LuaScriptHost::hook_table`] returns the resolved [`HookTable`].
//! - **Validation (§8.2, §2.3):** [`validate::validate_project`] runs every static check of
//!   `pkrs check` and the runtime's start-up check (manifest, plugin graph, content stages 1–3,
//!   compile-only Lua syntax via [`check_lua_syntax`], maps, tick-listener and `once`-trigger
//!   warnings) and returns vendor-free diagnostics.
//! - **Test scenarios (§15.2):** [`LuaScenario`] runs a `tests/*.lua` scenario in its own
//!   sandboxed Lua state with the scripted-input test API; [`scenario`] holds the engine-side
//!   snapshot, step and failure types the runtime's harness uses.

pub mod chain;
mod error;
pub mod events;
mod host;
pub mod input_bridge;
mod lua;
pub mod plugin;
pub mod registry;
pub mod scenario;
mod scheduler;
pub mod validate;
mod world;

pub use chain::{
    ChainKey, ChainOutcome, HookChain, HookKind, HookTable, HookTableEntry, LoadOrder, Step,
    run_chain,
};
pub use error::{Result, ScriptError, split_location};
pub use events::{DispatchReport, ENGINE_EVENTS, ListenerId, ListenerInfo, ScriptEvent};
pub use host::{
    BattleRequest, MemoryScripts, ScriptFiles, ScriptHost, ScriptLimits, ScriptOwner, ScriptValue,
    TaskId, TaskStatus, WaitRequest, battle_result_value,
};
pub use input_bridge::ScriptInputSnapshot;
pub use lua::{
    DEFAULT_AI_MAX_FAILURES, DEFAULT_MAX_FAILURES, GfxOp, HOOK_NAMES, LuaAi, LuaBattleHooks,
    LuaScenario, LuaScriptHost, LuaSyntaxError, SCENARIO_API, UiScreenMethod, check_lua_syntax,
};
pub use plugin::{
    ContentError, PluginError, PluginErrors, PluginHost, PluginInitError, PluginLoadReport,
    PluginPlan, load_plugins, merge_content, plan_plugins,
};
pub use registry::{FieldActionInfo, FieldActionSource, FieldContext, InputActionRequest};
pub use scheduler::{
    PendingUiPush, Scheduler, StepReport, TaskKind, TaskOutcome, WaitCondition, WakeSignal,
};
pub use world::{
    ApiError, ApiResult, CameraCommand, CameraTarget, ScriptWorld, TileInfo, direction_toward,
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
