//! Integration (§9.4 "Hook combination", §9.2): `events.on` listeners from several plugins and
//! the game run as one priority-ordered chain per event (priority, then plugin load order,
//! then registration order), threading the payload and honouring `{ cancel = true }`.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use pkrs_core::Pcg64Rng;
use pkrs_script::{
    DispatchReport, LuaScriptHost, MemoryScripts, Scheduler, ScriptEvent, ScriptHost, ScriptLimits,
    ScriptOwner, ScriptValue, load_plugins, plan_plugins,
};

fn manifest(id: &str) -> String {
    format!("[plugin]\nid = \"{id}\"\nname = \"{id}\"\nversion = \"1.0.0\"\napi = 1\n")
}

/// A host with `files`' plugins loaded in resolved order (`enabled` order breaks ties), plus a
/// game-side `log` table every listener appends to.
fn load(files: &MemoryScripts, enabled: &[&str]) -> LuaScriptHost {
    let enabled: Vec<String> = enabled.iter().map(|s| (*s).to_owned()).collect();
    let plan = plan_plugins(files, &enabled, 1..=1).unwrap();
    let mut host = LuaScriptHost::new(
        Box::new(files.clone()),
        Box::new(Pcg64Rng::from_seed(45)),
        ScriptLimits::default(),
    )
    .unwrap();
    let report = load_plugins(&mut host, files, &plan);
    assert!(report.failed.is_empty(), "{:?}", report.failed);
    host
}

fn plugin(files: MemoryScripts, id: &str, init: &str) -> MemoryScripts {
    files
        .with(format!("plugins/{id}/plugin.toml"), manifest(id))
        .with(format!("plugins/{id}/init.lua"), init.to_owned())
}

/// Emits `event` with `payload` and flushes it; returns the scheduler's report.
fn flush(host: &mut LuaScriptHost, event: &str, payload: ScriptValue) -> DispatchReport {
    host.emit_event(ScriptEvent::new(event, payload));
    Scheduler::new().dispatch_events(host)
}

fn int(host: &mut LuaScriptHost, owner: &ScriptOwner, name: &str) -> i64 {
    host.global(owner, name).unwrap().as_int().unwrap()
}

#[test]
fn listeners_run_by_priority_then_load_order_then_registration() {
    // `late` is loaded second but registers at a lower priority; the game registers first but
    // ranks after every plugin at equal priority.
    let files = plugin(
        plugin(
            MemoryScripts::new(),
            "early",
            "events.on('ping', function(p) p.trail = p.trail .. 'early,' return p end)",
        ),
        "late",
        "events.on('ping', function(p) p.trail = p.trail .. 'late,' return p end, { priority = -1 })
         events.on('ping', function(p) p.trail = p.trail .. 'late2,' return p end)",
    );
    let mut host = load(&files, &["early", "late"]);
    host.run_source(
        &ScriptOwner::Game,
        "scripts/main.lua",
        "events.on('ping', function(p) trail = p.trail .. 'game' end)",
    )
    .unwrap();
    let payload = ScriptValue::Map([("trail".to_owned(), ScriptValue::from(""))].into());
    let report = flush(&mut host, "ping", payload);
    assert!(report.failures.is_empty(), "{:?}", report.failures);
    assert_eq!(
        host.global(&ScriptOwner::Game, "trail").unwrap(),
        ScriptValue::from("late,early,late2,game")
    );
    assert_eq!(
        host.hook_table().to_string(),
        "battle hooks:\n  (none)\nworld events:\n  ping\n      -1  late\n       0  early\n       0  late\n       0  game\n"
    );
}

#[test]
fn returned_values_thread_and_nil_passes_through() {
    let files = plugin(
        MemoryScripts::new(),
        "math",
        "events.on('value', function(n) return n * 2 end, { priority = 10 })
         events.on('value', function(n) return n + 3 end, { priority = -5 })
         events.on('value', function(n) seen_nil = n end)",
    );
    let mut host = load(&files, &["math"]);
    host.run_source(
        &ScriptOwner::Game,
        "scripts/main.lua",
        "events.on('value', function(n) final = n end, { priority = 100 })",
    )
    .unwrap();
    flush(&mut host, "value", ScriptValue::Int(4));
    // +3 (priority -5), nil passthrough (0), ×2 (10), then the game sees the result.
    assert_eq!(int(&mut host, &ScriptOwner::plugin("math"), "seen_nil"), 7);
    assert_eq!(int(&mut host, &ScriptOwner::Game, "final"), 14);
}

#[test]
fn cancel_stops_later_listeners_of_that_event_only() {
    let files = plugin(
        MemoryScripts::new(),
        "guard",
        "before, after, other = 0, 0, 0
         events.on('door', function() before = before + 1 end, { priority = -1 })
         events.on('door', function() return { cancel = true, message = 'locked' } end)
         events.on('door', function() after = after + 1 end, { priority = 1 })
         events.on('bell', function() other = other + 1 end)",
    );
    let mut host = load(&files, &["guard"]);
    host.emit_event(ScriptEvent::new("door", ScriptValue::Nil));
    host.emit_event(ScriptEvent::new("bell", ScriptValue::Nil));
    let report = Scheduler::new().dispatch_events(&mut host);
    let guard = ScriptOwner::plugin("guard");
    assert_eq!(int(&mut host, &guard, "before"), 1);
    assert_eq!(
        int(&mut host, &guard, "after"),
        0,
        "the cancel stopped the chain"
    );
    assert_eq!(
        int(&mut host, &guard, "other"),
        1,
        "other events are unaffected"
    );
    assert_eq!(report.cancelled, ["door"]);
    assert_eq!(report.events, ["door", "bell"]);
}

#[test]
fn yielding_and_failing_listeners_leave_the_payload_unchanged() {
    let files = plugin(
        MemoryScripts::new(),
        "slow",
        "events.on('value', function(n) wait(5); return n * 100 end, { priority = -2 })
         events.on('value', function(n) error('boom') end, { priority = -1 })
         events.on('value', function(n) got = n end)",
    );
    let mut host = load(&files, &["slow"]);
    let report = flush(&mut host, "value", ScriptValue::Int(9));
    assert_eq!(report.failures.len(), 1);
    assert_eq!(int(&mut host, &ScriptOwner::plugin("slow"), "got"), 9);
}

#[test]
fn same_plugins_and_order_give_the_same_chain() {
    let files = plugin(
        plugin(
            MemoryScripts::new(),
            "a",
            "events.on('e', function(s) return s .. 'a' end, { priority = 2 })",
        ),
        "b",
        "events.on('e', function(s) return s .. 'b' end, { priority = 2 })
         events.on('e', function(s) out = s end, { priority = 3 })",
    );
    let run = || {
        let mut host = load(&files, &["a", "b"]);
        flush(&mut host, "e", ScriptValue::from(""));
        (
            host.global(&ScriptOwner::plugin("b"), "out").unwrap(),
            host.hook_table(),
        )
    };
    let first = run();
    assert_eq!(first.0, ScriptValue::from("ab"));
    assert_eq!(first, run());
    let mut swapped = load(&files, &["b", "a"]);
    flush(&mut swapped, "e", ScriptValue::from(""));
    assert_eq!(
        swapped.global(&ScriptOwner::plugin("b"), "out").unwrap(),
        ScriptValue::from("ba"),
        "load order breaks priority ties"
    );
}
