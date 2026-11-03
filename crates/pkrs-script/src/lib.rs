//! Lua scripting: runtime, sandbox, API bindings, coroutine scheduler and plugin loader.
//!
//! This crate is currently a skeleton; see `docs/design.md` §4.3 for its role
//! and dependency boundaries.

mod error;

pub use error::{Result, ScriptError};

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
