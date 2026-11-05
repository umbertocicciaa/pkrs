//! Sandbox and safety tests for [`LuaScriptHost`] (§9.6, §15.2).

use pkrs_core::Pcg64Rng;

use super::*;
use crate::host::MemoryScripts;

fn host_with(files: MemoryScripts) -> LuaScriptHost {
    LuaScriptHost::new(
        Box::new(files),
        Box::new(Pcg64Rng::from_seed(42)),
        ScriptLimits::default(),
    )
    .unwrap()
}

fn host() -> LuaScriptHost {
    host_with(MemoryScripts::new())
}

fn eval(host: &mut LuaScriptHost, src: &str) -> Result<ScriptValue> {
    host.run_source(&ScriptOwner::Game, "scripts/test.lua", src)
}

#[test]
fn forbidden_globals_are_absent() {
    let mut h = host();
    for name in [
        "io",
        "os",
        "package",
        "debug",
        "dofile",
        "loadfile",
        "warn",
        "module",
        "loadstring",
        "setfenv",
        "getfenv",
    ] {
        assert_eq!(
            eval(&mut h, &format!("return {name} == nil")).unwrap(),
            ScriptValue::Bool(true),
            "{name} must not be reachable"
        );
    }
    assert_eq!(
        eval(&mut h, "return string.dump == nil").unwrap(),
        ScriptValue::Bool(true)
    );
}

#[test]
fn whitelisted_libraries_work() {
    let mut h = host();
    let v = eval(
        &mut h,
        "return string.upper('a') .. table.concat({1,2}, ',') .. math.max(1, 3) .. utf8.char(65) .. tostring(coroutine.isyieldable())",
    )
    .unwrap();
    assert_eq!(v, ScriptValue::String("A1,23Afalse".into()));
}

#[test]
fn string_metatable_is_sealed() {
    let mut h = host();
    assert_eq!(
        eval(&mut h, "return getmetatable('')").unwrap(),
        ScriptValue::Bool(false)
    );
    // Method calls still work.
    assert_eq!(
        eval(&mut h, "return ('abc'):upper()").unwrap(),
        ScriptValue::String("ABC".into())
    );
}

#[test]
fn collectgarbage_only_allows_count() {
    let mut h = host();
    assert!(matches!(
        eval(&mut h, "return collectgarbage('count')").unwrap(),
        ScriptValue::Number(kb) if kb > 0.0
    ));
    let err = eval(&mut h, "collectgarbage('collect')").unwrap_err();
    assert!(err.to_string().contains("not available"), "{err}");
    assert!(eval(&mut h, "collectgarbage()").is_err());
}

#[test]
fn load_is_text_only_and_uses_the_callers_env() {
    let mut h = host();
    assert_eq!(
        eval(&mut h, "secret = 7; return load('return secret')()").unwrap(),
        ScriptValue::Int(7)
    );
    assert_eq!(
        eval(
            &mut h,
            "local f, err = load('\\27Lua', 'x', 'b'); return err"
        )
        .unwrap(),
        ScriptValue::String("load: binary chunks are not allowed".into())
    );
    assert_eq!(
        eval(&mut h, "local f, err = load('\\27LuaT'); return err").unwrap(),
        ScriptValue::String("load: binary chunks are not allowed".into())
    );
    assert_eq!(
        eval(
            &mut h,
            "local f, err = load(function() return nil end); return err"
        )
        .unwrap(),
        ScriptValue::String("load: only string chunks are allowed in the sandbox".into())
    );
    // A chunk loaded without an env cannot see the real global table.
    assert_eq!(
        eval(&mut h, "return load('return io == nil and os == nil')()").unwrap(),
        ScriptValue::Bool(true)
    );
}

#[test]
fn require_loads_from_scripts_and_caches() {
    let files = MemoryScripts::new()
        .with(
            "scripts/lib/counter.lua",
            "loads = (loads or 0) + 1\nreturn { value = 41 }",
        )
        .with("scripts/lib/pkg/init.lua", "return 'init'");
    let mut h = host_with(files);
    let v = eval(
        &mut h,
        "local a = require('lib.counter'); local b = require('lib.counter'); return a.value + (a == b and 1 or 0)",
    )
    .unwrap();
    assert_eq!(v, ScriptValue::Int(42));
    assert_eq!(
        h.global(&ScriptOwner::Game, "loads").unwrap(),
        ScriptValue::Int(1),
        "module body runs once"
    );
    assert_eq!(
        eval(&mut h, "return require('lib.pkg')").unwrap(),
        ScriptValue::String("init".into())
    );
}

#[test]
fn require_is_restricted_to_allowed_folders() {
    let files = MemoryScripts::new()
        .with("plugins/alpha/helper.lua", "return 'alpha helper'")
        .with("plugins/beta/secret.lua", "return 'beta secret'")
        .with("scripts/shared.lua", "return 'shared'");
    let mut h = host_with(files);
    let alpha = ScriptOwner::plugin("alpha");

    // Own folder and scripts/ are allowed for a plugin.
    assert_eq!(
        h.run_source(&alpha, "plugins/alpha/init.lua", "return require('helper')")
            .unwrap(),
        ScriptValue::String("alpha helper".into())
    );
    assert_eq!(
        h.run_source(&alpha, "plugins/alpha/init.lua", "return require('shared')")
            .unwrap(),
        ScriptValue::String("shared".into())
    );
    // Another plugin's folder is not reachable, by name or by path.
    for module in [
        "secret",
        "beta.secret",
        "plugins.beta.secret",
        "../beta/secret",
    ] {
        let err = h
            .run_source(
                &alpha,
                "plugins/alpha/init.lua",
                &format!("return require('{module}')"),
            )
            .unwrap_err();
        assert!(
            err.to_string().contains("not found") || err.to_string().contains("invalid module"),
            "{module}: {err}"
        );
    }
    // The game cannot reach into plugin folders.
    assert!(eval(&mut h, "return require('helper')").is_err());
    // Path-like names are rejected outright.
    for bad in ["/etc/passwd", "..", "a/b", "c:x"] {
        let err = eval(&mut h, &format!("return require('{bad}')")).unwrap_err();
        assert!(
            err.to_string().contains("invalid module name"),
            "{bad}: {err}"
        );
    }
}

#[test]
fn circular_require_is_an_error() {
    let files = MemoryScripts::new()
        .with("scripts/a.lua", "return require('b')")
        .with("scripts/b.lua", "return require('a')");
    let mut h = host_with(files);
    let err = eval(&mut h, "return require('a')").unwrap_err();
    assert!(err.to_string().contains("circular require of 'a'"), "{err}");
}

#[test]
fn owners_have_isolated_environments() {
    let mut h = host();
    let a = ScriptOwner::plugin("a");
    let b = ScriptOwner::plugin("b");
    eval(&mut h, "shared_name = 'game'").unwrap();
    h.run_source(
        &a,
        "plugins/a/init.lua",
        "shared_name = 'a'; string.upper = nil",
    )
    .unwrap();
    assert_eq!(
        h.global(&ScriptOwner::Game, "shared_name").unwrap(),
        ScriptValue::String("game".into())
    );
    assert_eq!(h.global(&b, "shared_name").unwrap(), ScriptValue::Nil);
    // Plugin `a` replacing its `string.upper` does not leak into `b` or the game.
    assert_eq!(
        h.run_source(&b, "plugins/b/init.lua", "return string.upper('x')")
            .unwrap(),
        ScriptValue::String("X".into())
    );
    // `_G` is the owner's own environment.
    assert_eq!(
        h.run_source(&b, "plugins/b/init.lua", "return _G.shared_name == nil")
            .unwrap(),
        ScriptValue::Bool(true)
    );
}

#[test]
fn runaway_loop_is_aborted_and_host_survives() {
    let mut h = LuaScriptHost::new(
        Box::new(MemoryScripts::new()),
        Box::new(Pcg64Rng::from_seed(1)),
        ScriptLimits {
            instruction_budget: 100_000,
            ..ScriptLimits::default()
        },
    )
    .unwrap();
    let err = eval(&mut h, "while true do end").unwrap_err();
    assert!(
        matches!(err, ScriptError::InstructionBudget { budget: 100_000 }),
        "{err:?}"
    );
    // Inside a coroutine too: `coroutine.resume` cannot swallow the budget error.
    let err = eval(
        &mut h,
        "local co = coroutine.create(function() while true do end end); return coroutine.resume(co)",
    )
    .unwrap_err();
    assert!(
        matches!(err, ScriptError::InstructionBudget { .. }),
        "{err:?}"
    );
    // Neither can `pcall` / `xpcall` in a loop.
    for src in [
        "while true do pcall(function() while true do end end) end",
        "while true do xpcall(function() while true do end end, function(e) return e end) end",
    ] {
        let err = eval(&mut h, src).unwrap_err();
        assert!(
            matches!(err, ScriptError::InstructionBudget { .. }),
            "{src}: {err:?}"
        );
    }
    // Budget is per call: a normal call afterwards succeeds.
    assert_eq!(eval(&mut h, "return 1 + 1").unwrap(), ScriptValue::Int(2));
}

#[test]
fn pcall_still_catches_ordinary_errors_and_coroutines_yield_through_it() {
    let mut h = host();
    assert_eq!(
        eval(&mut h, "local ok, e = pcall(error, 'x'); return ok").unwrap(),
        ScriptValue::Bool(false)
    );
    let v = eval(
        &mut h,
        "local co = coroutine.create(function() pcall(function() coroutine.yield(5) end) return 9 end)
         local _, a = coroutine.resume(co); local _, b = coroutine.resume(co); return a + b",
    )
    .unwrap();
    assert_eq!(v, ScriptValue::Int(14));
}

#[test]
fn memory_limit_is_enforced_and_host_survives() {
    let mut h = LuaScriptHost::new(
        Box::new(MemoryScripts::new()),
        Box::new(Pcg64Rng::from_seed(1)),
        ScriptLimits {
            memory_limit: 4 * 1024 * 1024,
            ..ScriptLimits::default()
        },
    )
    .unwrap();
    let err = eval(
        &mut h,
        "local t = {} for i = 1, 10000000 do t[i] = string.rep('x', 64) .. i end",
    )
    .unwrap_err();
    assert!(matches!(err, ScriptError::MemoryLimit { .. }), "{err:?}");
    assert_eq!(
        eval(&mut h, "return 'ok'").unwrap(),
        ScriptValue::String("ok".into())
    );
}

#[test]
fn runtime_errors_carry_file_line_and_traceback() {
    let mut h = host();
    let err = h
        .run_source(
            &ScriptOwner::Game,
            "scripts/main.lua",
            "local x = 1\n\nerror('boom')\n",
        )
        .unwrap_err();
    assert_eq!(err.location(), Some(("scripts/main.lua", 3)), "{err:?}");
    assert_eq!(err.to_string(), "scripts/main.lua:3: boom");
    assert!(
        err.traceback()
            .is_some_and(|t| t.contains("stack traceback")),
        "{err:?}"
    );
}

#[test]
fn errors_in_called_functions_point_at_their_line() {
    let mut h = host();
    h.run_source(
        &ScriptOwner::Game,
        "scripts/main.lua",
        "function explode(n)\n  return n.field\nend",
    )
    .unwrap();
    let err = h
        .call_function(&ScriptOwner::Game, "explode", &[ScriptValue::Int(1)])
        .unwrap_err();
    assert_eq!(err.location(), Some(("scripts/main.lua", 2)), "{err:?}");
}

#[test]
fn errors_from_required_modules_and_callbacks_are_located() {
    let files = MemoryScripts::new().with("scripts/bad.lua", "\nlocal y = nil + 1");
    let mut h = host_with(files);
    let err = eval(&mut h, "require('bad')").unwrap_err();
    assert_eq!(err.location(), Some(("scripts/bad.lua", 2)), "{err:?}");

    let err = eval(&mut h, "\n\nrequire('missing.module')").unwrap_err();
    assert_eq!(err.location(), Some(("scripts/test.lua", 3)), "{err:?}");
}

#[test]
fn syntax_errors_are_reported() {
    let mut h = host();
    let err = eval(&mut h, "function (").unwrap_err();
    assert!(
        matches!(&err, ScriptError::Syntax { message } if message.contains("scripts/test.lua:1")),
        "{err:?}"
    );
}

#[test]
fn math_random_uses_the_engine_rng_deterministically() {
    let script = "local t = {} for i = 1, 8 do t[i] = math.random(1, 6) end; t[9] = math.random(3); local f = math.random(); t[10] = (f >= 0 and f < 1); return t";
    let a = eval(&mut host(), script).unwrap();
    let b = eval(&mut host(), script).unwrap();
    assert_eq!(a, b, "same engine seed, same rolls");
    let ScriptValue::List(items) = a else {
        panic!("list expected")
    };
    for roll in &items[..8] {
        let r = roll.as_int().unwrap();
        assert!((1..=6).contains(&r), "{r}");
    }
    assert!((1..=3).contains(&items[8].as_int().unwrap()));
    assert_eq!(items[9], ScriptValue::Bool(true));

    let mut h = host();
    assert!(eval(&mut h, "math.random(5, 1)").is_err());
    // randomseed cannot reseed the engine stream.
    let before = eval(&mut host(), "return math.random(1, 1000000)").unwrap();
    let after = eval(
        &mut host(),
        "math.randomseed(1); return math.random(1, 1000000)",
    )
    .unwrap();
    assert_eq!(before, after);
}

#[test]
fn values_round_trip_through_calls() {
    let mut h = host();
    eval(&mut h, "function echo(...) return ... end").unwrap();
    let map: BTreeMap<String, ScriptValue> = [
        ("name".to_string(), ScriptValue::from("emberkit")),
        ("level".to_string(), ScriptValue::Int(5)),
    ]
    .into_iter()
    .collect();
    for v in [
        ScriptValue::Nil,
        ScriptValue::Bool(true),
        ScriptValue::Int(-3),
        ScriptValue::Number(1.5),
        ScriptValue::from("hi"),
        ScriptValue::List(vec![ScriptValue::Int(1), ScriptValue::from("two")]),
        ScriptValue::Map(map),
    ] {
        assert_eq!(
            h.call_function(&ScriptOwner::Game, "echo", std::slice::from_ref(&v))
                .unwrap(),
            v
        );
    }
    assert_eq!(
        eval(&mut h, "return function() end").unwrap(),
        ScriptValue::Opaque("function")
    );
    assert!(matches!(
        h.call_function(&ScriptOwner::Game, "nope", &[]),
        Err(ScriptError::NoSuchFunction { .. })
    ));
    assert!(h.has_function(&ScriptOwner::Game, "echo"));
    assert!(!h.has_function(&ScriptOwner::plugin("x"), "echo"));
    assert!(h.memory_used() > 0);
}

#[test]
fn file_reads_go_through_script_files() {
    let files = MemoryScripts::new().with("scripts/main.lua", "return 'from files'");
    let mut h = host_with(files);
    assert_eq!(
        h.run_file(&ScriptOwner::Game, "scripts/main.lua").unwrap(),
        ScriptValue::String("from files".into())
    );
    assert!(matches!(
        h.run_file(&ScriptOwner::Game, "scripts/none.lua"),
        Err(ScriptError::Load { .. })
    ));
}
