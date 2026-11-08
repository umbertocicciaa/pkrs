//! Compile-only Lua syntax check (§2.3 `pkrs check`, §8.2): the chunk is compiled by Lua 5.4
//! and never run, so no game code executes.

use mlua::{ChunkMode, Lua};

use crate::error::split_location;

/// A Lua syntax error: the 1-based line (when Lua reports one) and the compiler message
/// without the `file:line:` prefix.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LuaSyntaxError {
    /// 1-based line.
    pub line: Option<u32>,
    /// Compiler message (`'end' expected near <eof>`).
    pub message: String,
}

/// Compiles `source` (named `path` in messages) without running it.
pub fn check_lua_syntax(path: &str, source: &str) -> Result<(), LuaSyntaxError> {
    let lua = Lua::new();
    let compiled = lua
        .load(source)
        .set_name(format!("@{path}"))
        .set_mode(ChunkMode::Text)
        .into_function();
    match compiled {
        Ok(_) => Ok(()),
        Err(mlua::Error::SyntaxError { message, .. }) => Err(parse_message(&message)),
        Err(other) => Err(LuaSyntaxError {
            line: None,
            message: other.to_string(),
        }),
    }
}

fn parse_message(message: &str) -> LuaSyntaxError {
    let first = message.lines().next().unwrap_or(message);
    match split_location(first) {
        Some((_, line, rest)) => LuaSyntaxError {
            line: Some(line),
            message: rest.to_owned(),
        },
        None => LuaSyntaxError {
            line: None,
            message: first.to_owned(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_chunks_compile_without_running() {
        // Running this would fail: `error` is never called by a compile-only check.
        assert_eq!(
            check_lua_syntax("scripts/a.lua", "error('boom')\nlocal x = 1"),
            Ok(())
        );
    }

    #[test]
    fn syntax_errors_carry_the_line() {
        let err =
            check_lua_syntax("scripts/a.lua", "local x = 1\nfunction f()\n  return\n").unwrap_err();
        assert_eq!(err.line, Some(4));
        assert!(err.message.contains("'end' expected"), "{}", err.message);
        let err = check_lua_syntax("scripts/b.lua", "x = = 1").unwrap_err();
        assert_eq!(err.line, Some(1));
    }
}
