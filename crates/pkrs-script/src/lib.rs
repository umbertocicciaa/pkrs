//! Lua scripting: runtime, sandbox, API bindings, coroutine scheduler and plugin loader.
//!
//! - **Ports:** [`ScriptHost`] (run chunks, call functions) and [`ScriptFiles`] (read project
//!   scripts by path; the runtime implements it over the assets VFS). Both use engine types
//!   only ([`ScriptValue`], [`ScriptOwner`], [`ScriptError`]).
//! - **Adapter:** [`LuaScriptHost`] embeds vendored Lua 5.4 through `mlua` with the §9.6
//!   sandbox: whitelisted libraries, per-owner `_ENV`, restricted `require`, engine-RNG
//!   `math.random`, instruction budget, memory limit, and errors with `file:line` + traceback
//!   routed to `tracing` (target `script`).

mod error;
mod host;
mod lua;

pub use error::{Result, ScriptError, split_location};
pub use host::{MemoryScripts, ScriptFiles, ScriptHost, ScriptLimits, ScriptOwner, ScriptValue};
pub use lua::LuaScriptHost;

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
