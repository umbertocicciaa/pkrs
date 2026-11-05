//! Error type for `pkrs-script` (§9.6, §15.4).

use thiserror::Error;

/// Convenience alias for results whose error is [`ScriptError`].
pub type Result<T, E = ScriptError> = std::result::Result<T, E>;

/// Errors produced by `pkrs-script`.
///
/// Script failures never panic the engine: every Lua call goes through a protected boundary and
/// comes back as one of these variants (§9.6).
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum ScriptError {
    /// A script raised an error while running.
    #[error("script error in `{chunk}`: {message}")]
    Runtime {
        /// Script chunk name (usually its path).
        chunk: String,
        /// Error message, starting with `file:line:` when Lua knows the location.
        message: String,
    },
    /// A script raised an error; location and traceback are known.
    #[error("{file}:{line}: {message}")]
    Located {
        /// Script file (`scripts/main.lua`).
        file: String,
        /// 1-based line.
        line: u32,
        /// Error message without the location prefix.
        message: String,
        /// Lua stack traceback (may be empty).
        traceback: String,
    },
    /// A chunk failed to compile.
    #[error("syntax error: {message}")]
    Syntax {
        /// Compiler message (includes `file:line:`).
        message: String,
    },
    /// A single call ran past its instruction budget (runaway loop).
    #[error("script exceeded its instruction budget of {budget} instructions")]
    InstructionBudget {
        /// The configured budget.
        budget: u64,
    },
    /// The Lua state ran past its memory limit.
    #[error("script exceeded the memory limit of {limit} bytes")]
    MemoryLimit {
        /// The configured limit in bytes.
        limit: usize,
    },
    /// `require` or a file load named a script that does not exist or may not be loaded.
    #[error("cannot load script `{path}`: {reason}")]
    Load {
        /// Requested path or module name.
        path: String,
        /// Why it failed.
        reason: String,
    },
    /// A function was called that the owner does not define.
    #[error("`{name}` is not a function defined by {owner}")]
    NoSuchFunction {
        /// Owner description.
        owner: String,
        /// Function name.
        name: String,
    },
    /// A value could not cross the Rust/Lua boundary.
    #[error("cannot convert script value: {0}")]
    Conversion(String),
    /// The scripting backend failed in a way not covered above.
    #[error("script engine error: {0}")]
    Engine(String),
    /// Error from `pkrs-world`.
    #[error(transparent)]
    World(#[from] pkrs_world::WorldError),
    /// Error from `pkrs-battle`.
    #[error(transparent)]
    Battle(#[from] pkrs_battle::BattleError),
    /// Error from `pkrs-data`.
    #[error(transparent)]
    Data(#[from] pkrs_data::DataError),
    /// Error from `pkrs-save`.
    #[error(transparent)]
    Save(#[from] pkrs_save::SaveError),
    /// Error from `pkrs-ui`.
    #[error(transparent)]
    Ui(#[from] pkrs_ui::UiError),
    /// Error from `pkrs-audio`.
    #[error(transparent)]
    Audio(#[from] pkrs_audio::AudioError),
}

impl ScriptError {
    /// `(file, line)` if the error points at a script location.
    #[must_use]
    pub fn location(&self) -> Option<(&str, u32)> {
        match self {
            Self::Located { file, line, .. } => Some((file, *line)),
            _ => None,
        }
    }

    /// The Lua traceback, if one was captured.
    #[must_use]
    pub fn traceback(&self) -> Option<&str> {
        match self {
            Self::Located { traceback, .. } if !traceback.is_empty() => Some(traceback),
            _ => None,
        }
    }
}

/// Splits `file:line: message` (Lua's error format). Returns `None` without a location.
#[must_use]
pub fn split_location(text: &str) -> Option<(&str, u32, &str)> {
    // The file part may itself contain ':' (never in VFS paths, but be lenient): scan for the
    // first ":<digits>:" separator.
    let bytes = text.as_bytes();
    let mut i = 0;
    while let Some(off) = text[i..].find(':') {
        let colon = i + off;
        let digits_start = colon + 1;
        let digits_len = bytes[digits_start..]
            .iter()
            .take_while(|b| b.is_ascii_digit())
            .count();
        let after = digits_start + digits_len;
        if digits_len > 0 && bytes.get(after) == Some(&b':') && colon > 0 {
            let line: u32 = text[digits_start..after].parse().ok()?;
            let message = text[after + 1..].trim_start();
            return Some((&text[..colon], line, message));
        }
        i = colon + 1;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_location_parses_lua_errors() {
        assert_eq!(
            split_location("scripts/main.lua:12: attempt to call a nil value"),
            Some(("scripts/main.lua", 12, "attempt to call a nil value"))
        );
        assert_eq!(
            split_location("plugins/x/init.lua:3: bad: thing"),
            Some(("plugins/x/init.lua", 3, "bad: thing"))
        );
        assert_eq!(split_location("no location here"), None);
        assert_eq!(split_location(":3: missing file"), None);
    }

    #[test]
    fn located_error_formats_like_lua() {
        let err = ScriptError::Located {
            file: "scripts/main.lua".into(),
            line: 4,
            message: "boom".into(),
            traceback: "stack traceback:\n\t[C]: in ?".into(),
        };
        assert_eq!(err.to_string(), "scripts/main.lua:4: boom");
        assert_eq!(err.location(), Some(("scripts/main.lua", 4)));
        assert!(err.traceback().is_some());
    }
}
