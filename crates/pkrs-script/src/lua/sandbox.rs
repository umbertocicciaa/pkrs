//! Sandboxed per-owner environments (§9.6).
//!
//! Only `base` (minus `dofile`, `loadfile`, binary `load`, and `collectgarbage` options other than
//! `count`), `string`, `table`, `math`, `utf8` and `coroutine` are exposed. `io`, `os`, `package`
//! and `debug` are never opened. Each owner gets its own `_ENV` with **copies** of the library
//! tables, so one plugin replacing `string.format` cannot affect another.

use mlua::{ChunkMode, Lua, MultiValue, Table, Value};
use pkrs_core::GameRng;

use crate::host::{ScriptFiles, ScriptOwner};

/// Base-library functions copied into every environment.
const SAFE_BASE: &[&str] = &[
    "assert",
    "error",
    "getmetatable",
    "ipairs",
    "next",
    "pairs",
    "pcall",
    "rawequal",
    "rawget",
    "rawlen",
    "rawset",
    "select",
    "setmetatable",
    "tonumber",
    "tostring",
    "type",
    "xpcall",
    "_VERSION",
];

/// Library tables copied (shallowly) into every environment.
const SAFE_LIBS: &[&str] = &["string", "table", "math", "utf8", "coroutine"];

/// Globals removed from the real global table as defence in depth.
const STRIPPED_GLOBALS: &[&str] = &[
    "dofile",
    "loadfile",
    "load",
    "require",
    "collectgarbage",
    "print",
    "warn",
];

/// App data: project script files.
pub(super) struct Files(pub Box<dyn ScriptFiles>);

/// App data: the engine RNG behind `math.random`.
pub(super) struct Rng(pub Box<dyn GameRng>);

/// Strips the real globals and seals the shared string metatable. Run once at start-up.
pub(super) fn harden_globals(lua: &Lua) -> mlua::Result<()> {
    // `getmetatable("")` would expose the shared `string` table used by `s:method()` calls.
    lua.load("local mt = getmetatable(''); mt.__metatable = false; string.dump = nil")
        .set_name("=pkrs-sandbox")
        .exec()?;
    let globals = lua.globals();
    for name in STRIPPED_GLOBALS {
        globals.raw_set(*name, Value::Nil)?;
    }
    Ok(())
}

/// `"<file>:<line>: "` for the Lua function that called the current Rust callback.
fn caller_location(lua: &Lua) -> String {
    lua.inspect_stack(1, |d| {
        let src = d.source().short_src.map(|s| s.into_owned());
        match (src, d.current_line()) {
            (Some(src), Some(line)) => format!("{src}:{line}: "),
            _ => String::new(),
        }
    })
    .unwrap_or_default()
}

fn located_error(lua: &Lua, message: impl AsRef<str>) -> mlua::Error {
    mlua::Error::runtime(format!("{}{}", caller_location(lua), message.as_ref()))
}

fn copy_table(lua: &Lua, source: &Table) -> mlua::Result<Table> {
    let copy = lua.create_table()?;
    for pair in source.pairs::<Value, Value>() {
        let (k, v) = pair?;
        copy.raw_set(k, v)?;
    }
    Ok(copy)
}

/// Wraps `pcall`, `xpcall` and `coroutine.resume` so a tripped instruction budget cannot be
/// swallowed: after they return, `guard` re-raises the budget error if it fired. The wrappers
/// are Lua functions, so coroutines may still yield through `pcall`.
const GUARDED_CATCHERS: &str = r#"
local raw_pcall, raw_xpcall, raw_resume, guard = ...
return function(...) return guard(raw_pcall(...)) end,
       function(...) return guard(raw_xpcall(...)) end,
       function(...) return guard(raw_resume(...)) end
"#;

/// Builds the `_ENV` table for `owner`. `guard` returns its arguments unchanged, or raises the
/// budget error once the instruction budget has tripped.
pub(super) fn build_env(
    lua: &Lua,
    owner: &ScriptOwner,
    guard: &mlua::Function,
) -> mlua::Result<Table> {
    let globals = lua.globals();
    let env = lua.create_table()?;
    for name in SAFE_BASE {
        env.raw_set(*name, globals.raw_get::<Value>(*name)?)?;
    }
    for name in SAFE_LIBS {
        let lib: Table = globals.raw_get(*name)?;
        env.raw_set(*name, copy_table(lua, &lib)?)?;
    }
    env.raw_set("_G", env.clone())?;

    let coroutine: Table = env.raw_get("coroutine")?;
    let (pcall, xpcall, resume): (mlua::Function, mlua::Function, mlua::Function) =
        lua.load(GUARDED_CATCHERS).set_name("=pkrs-sandbox").call((
            globals.raw_get::<mlua::Function>("pcall")?,
            globals.raw_get::<mlua::Function>("xpcall")?,
            coroutine.raw_get::<mlua::Function>("resume")?,
            guard.clone(),
        ))?;
    env.raw_set("pcall", pcall)?;
    env.raw_set("xpcall", xpcall)?;
    coroutine.raw_set("resume", resume)?;

    let math: Table = env.raw_get("math")?;
    math.raw_set("random", random_fn(lua)?)?;
    math.raw_set(
        "randomseed",
        lua.create_function(|_, _: MultiValue| Ok(()))?,
    )?;

    env.raw_set("print", print_fn(lua, owner)?)?;
    env.raw_set("collectgarbage", collectgarbage_fn(lua)?)?;
    env.raw_set("load", load_fn(lua, &env)?)?;
    env.raw_set("require", require_fn(lua, owner, &env)?)?;
    super::battle::install(lua, &env, owner)?;
    super::ai::install(lua, &env, owner)?;
    super::time::install(lua, &env)?;
    Ok(env)
}

/// `math.random` on the engine [`GameRng`] (Lua 5.4 semantics: `()` → float in `[0,1)`,
/// `(m)` → `[1,m]`, `(m,n)` → `[m,n]`).
fn random_fn(lua: &Lua) -> mlua::Result<mlua::Function> {
    lua.create_function(|lua, (m, n): (Option<i64>, Option<i64>)| {
        let mut rng = lua
            .app_data_mut::<Rng>()
            .ok_or_else(|| mlua::Error::runtime("engine RNG unavailable"))?;
        let (lo, hi) = match (m, n) {
            (None, None) => {
                let bits = rng.0.next_u64() >> 11;
                return Ok(Value::Number(bits as f64 / (1u64 << 53) as f64));
            }
            (Some(hi), None) => (1, hi),
            (Some(lo), Some(hi)) => (lo, hi),
            (None, Some(_)) => {
                return Err(mlua::Error::runtime(
                    "bad argument #1 to 'random' (number expected)",
                ));
            }
        };
        if lo > hi {
            return Err(mlua::Error::runtime(
                "bad argument to 'random' (interval is empty)",
            ));
        }
        let span = (i128::from(hi) - i128::from(lo) + 1) as u128;
        let offset = if span <= u128::from(u32::MAX) {
            u128::from(rng.0.gen_range_u32(span as u32))
        } else {
            u128::from(rng.0.next_u64()) % span
        };
        Ok(Value::Integer((i128::from(lo) + offset as i128) as i64))
    })
}

fn print_fn(lua: &Lua, owner: &ScriptOwner) -> mlua::Result<mlua::Function> {
    let owner = owner.to_string();
    lua.create_function(move |_, args: MultiValue| {
        let parts: Vec<String> = args
            .iter()
            .map(|v| v.to_string().unwrap_or_else(|_| v.type_name().to_owned()))
            .collect();
        tracing::info!(target: "script", owner = %owner, "{}", parts.join("\t"));
        Ok(())
    })
}

fn collectgarbage_fn(lua: &Lua) -> mlua::Result<mlua::Function> {
    lua.create_function(|lua, opt: Option<String>| match opt.as_deref() {
        Some("count") => Ok(lua.used_memory() as f64 / 1024.0),
        other => Err(located_error(
            lua,
            format!(
                "collectgarbage('{}') is not available in the sandbox (only 'count')",
                other.unwrap_or("collect")
            ),
        )),
    })
}

/// Text-only `load`; the default environment is the caller's own `_ENV`, never the real globals.
fn load_fn(lua: &Lua, env: &Table) -> mlua::Result<mlua::Function> {
    let owner_env = env.clone();
    lua.create_function(
        move |lua,
              (chunk, name, mode, custom_env): (
            Value,
            Option<String>,
            Option<String>,
            Option<Table>,
        )| {
            let fail = |msg: &str| -> mlua::Result<(Value, Value)> {
                Ok((Value::Nil, Value::String(lua.create_string(msg)?)))
            };
            let Value::String(src) = chunk else {
                return fail("load: only string chunks are allowed in the sandbox");
            };
            if mode.as_deref().is_some_and(|m| !m.contains('t')) {
                return fail("load: binary chunks are not allowed");
            }
            let bytes = src.as_bytes().to_vec();
            if bytes.first() == Some(&0x1b) {
                return fail("load: binary chunks are not allowed");
            }
            let compiled = lua
                .load(bytes)
                .set_name(name.unwrap_or_else(|| "=(load)".into()))
                .set_mode(ChunkMode::Text)
                .set_environment(custom_env.unwrap_or_else(|| owner_env.clone()))
                .into_function();
            match compiled {
                Ok(f) => Ok((Value::Function(f), Value::Nil)),
                Err(e) => fail(&e.to_string()),
            }
        },
    )
}

/// `true` if `name` is a dotted module name (`lib.util`): ASCII letters, digits, `_`, `-`.
pub(super) fn valid_module_name(name: &str) -> bool {
    !name.is_empty()
        && name.split('.').all(|seg| {
            !seg.is_empty()
                && seg
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
        })
}

/// Project-relative `require` limited to `scripts/` and the plugin's own folder (§9.6).
/// Modules are cached per owner; a module that requires itself (directly or not) is an error.
fn require_fn(lua: &Lua, owner: &ScriptOwner, env: &Table) -> mlua::Result<mlua::Function> {
    let roots = owner.require_roots();
    let owner_env = env.clone();
    let loaded = lua.create_table()?;
    let loading = lua.create_table()?;
    lua.create_function(move |lua, name: String| {
        if !valid_module_name(&name) {
            return Err(located_error(
                lua,
                format!("invalid module name '{name}' (use dotted names like 'lib.util')"),
            ));
        }
        let cached: Value = loaded.raw_get(name.as_str())?;
        if !cached.is_nil() {
            return Ok(cached);
        }
        if loading.raw_get::<bool>(name.as_str()).unwrap_or(false) {
            return Err(located_error(lua, format!("circular require of '{name}'")));
        }
        let rel = name.replace('.', "/");
        let candidates: Vec<String> = roots
            .iter()
            .flat_map(|root| {
                [
                    format!("{root}/{rel}.lua"),
                    format!("{root}/{rel}/init.lua"),
                ]
            })
            .collect();
        let source = {
            let files = lua
                .app_data_ref::<Files>()
                .ok_or_else(|| mlua::Error::runtime("script files unavailable"))?;
            candidates
                .iter()
                .find(|p| files.0.exists(p))
                .map(|p| files.0.read_script(p).map(|src| (p.clone(), src)))
        };
        let (path, src) = match source {
            None => {
                return Err(located_error(
                    lua,
                    format!(
                        "module '{name}' not found (searched {})",
                        candidates.join(", ")
                    ),
                ));
            }
            Some(Err(e)) => return Err(located_error(lua, e.to_string())),
            Some(Ok(found)) => found,
        };
        loading.raw_set(name.as_str(), true)?;
        let result = lua
            .load(src)
            .set_name(format!("@{path}"))
            .set_mode(ChunkMode::Text)
            .set_environment(owner_env.clone())
            .call::<Value>(name.as_str());
        loading.raw_set(name.as_str(), Value::Nil)?;
        let value = match result? {
            Value::Nil => Value::Boolean(true),
            v => v,
        };
        loaded.raw_set(name.as_str(), value.clone())?;
        Ok(value)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn module_names_are_dotted_identifiers() {
        for ok in ["util", "lib.util", "ai.champion-hard", "a_b.c1"] {
            assert!(valid_module_name(ok), "{ok}");
        }
        for bad in ["", "..x", "a..b", "../x", "/x", "a/b", "a\\b", "c:x", "x."] {
            assert!(!valid_module_name(bad), "{bad}");
        }
    }
}
