//! The `log` module (§9.1, §15.1): `log.debug/info/warn/error(...)`, routed to `tracing` with
//! target `script`. Arguments are joined with tabs like `print`; every record carries the
//! owner (`the game`, `plugin `x``, `map script `y``) and the calling `file:line`.

use mlua::{Lua, MultiValue, Table};

use super::sandbox::caller_location;
use crate::host::ScriptOwner;

#[derive(Clone, Copy)]
enum Level {
    Debug,
    Info,
    Warn,
    Error,
}

fn message(args: &MultiValue) -> String {
    args.iter()
        .map(|v| v.to_string().unwrap_or_else(|_| v.type_name().to_owned()))
        .collect::<Vec<_>>()
        .join("\t")
}

/// Installs `log` into `env`.
pub(super) fn install(lua: &Lua, env: &Table, owner: &ScriptOwner) -> mlua::Result<()> {
    let log = lua.create_table()?;
    for (name, level) in [
        ("debug", Level::Debug),
        ("info", Level::Info),
        ("warn", Level::Warn),
        ("error", Level::Error),
    ] {
        let owner = owner.to_string();
        let f = lua.create_function(move |lua, args: MultiValue| {
            let text = message(&args);
            let at = caller_location(lua);
            let at = at.trim_end_matches(": ");
            match level {
                Level::Debug => tracing::debug!(target: "script", owner = %owner, at, "{text}"),
                Level::Info => tracing::info!(target: "script", owner = %owner, at, "{text}"),
                Level::Warn => tracing::warn!(target: "script", owner = %owner, at, "{text}"),
                Level::Error => tracing::error!(target: "script", owner = %owner, at, "{text}"),
            }
            Ok(())
        })?;
        log.raw_set(name, f)?;
    }
    env.raw_set("log", log)
}
