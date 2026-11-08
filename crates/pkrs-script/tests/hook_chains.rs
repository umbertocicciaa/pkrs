//! Integration (§9.4 "Hook combination"): `battle.on` handlers from several plugins run as one
//! priority-ordered chain (priority, then plugin load order, then registration order).
//!
//! Review snapshot changes with `cargo insta review` (or `INSTA_UPDATE=always`).

#![allow(clippy::unwrap_used, clippy::expect_used)]

use pkrs_battle::{
    Battle, BattleAction, BattleConfig, BattleEvent, BattleHooks, BattleInput, BattlePhase,
    BattleResult, NullBattleHooks, RngPolicy, SideId, SideInput, battle_testkit,
};
use pkrs_core::Pcg64Rng;
use pkrs_script::{
    LuaScriptHost, MemoryScripts, ScriptHost, ScriptLimits, ScriptOwner, load_plugins, plan_plugins,
};

fn manifest(id: &str) -> String {
    format!("[plugin]\nid = \"{id}\"\nname = \"{id}\"\nversion = \"1.0.0\"\napi = 1\n")
}

/// `crit-boost` doubles damage at `boost_priority`; `shield` adds 3 at `shield_priority`.
/// `crit-boost` is enabled (and so loaded) first.
fn damage_plugins(boost_priority: i64, shield_priority: i64) -> MemoryScripts {
    MemoryScripts::new()
        .with("plugins/crit-boost/plugin.toml", manifest("crit-boost"))
        .with(
            "plugins/crit-boost/init.lua",
            format!(
                "battle.on('on_damage_taken', function(ctx, target, amount)
                   return amount * 2
                 end, {{ priority = {boost_priority} }})"
            ),
        )
        .with("plugins/shield/plugin.toml", manifest("shield"))
        .with(
            "plugins/shield/init.lua",
            format!(
                "battle.on('on_damage_taken', function(ctx, target, amount)
                   return amount + 3
                 end, {{ priority = {shield_priority} }})"
            ),
        )
}

/// A host with `files`' plugins loaded in resolved order (`enabled` order breaks ties).
fn load(files: &MemoryScripts, enabled: &[&str]) -> LuaScriptHost {
    let enabled: Vec<String> = enabled.iter().map(|s| (*s).to_owned()).collect();
    let plan = plan_plugins(files, &enabled, 1..=1).unwrap();
    let mut host = LuaScriptHost::new(
        Box::new(files.clone()),
        Box::new(Pcg64Rng::from_seed(5)),
        ScriptLimits::default(),
    )
    .unwrap();
    let report = load_plugins(&mut host, files, &plan);
    assert!(report.failed.is_empty(), "{:?}", report.failed);
    host
}

fn tackle_turn() -> BattleInput {
    let fight = |side| BattleAction::Fight {
        move_slot: 0,
        target_side: side,
        target_slot: 0,
    };
    BattleInput::both(
        SideInput {
            slots: vec![fight(SideId::Opponent)],
        },
        SideInput {
            slots: vec![fight(SideId::Player)],
        },
    )
}

/// Runs the testkit 1v1 to the end with `hooks`; hook tracing is pinned off so the snapshot
/// does not depend on the `hook-trace` feature.
/// The result is `None` if the battle did not end within 100 turns.
fn run_battle(hooks: &mut dyn BattleHooks) -> (Vec<BattleEvent>, Option<BattleResult>) {
    let config = BattleConfig {
        trace_hooks: false,
        ..BattleConfig::default()
    };
    let mut battle = Battle::new(
        battle_testkit::wild_1v1(),
        config,
        battle_testkit::default_resources(),
        RngPolicy::Fixed { seed: 11 },
        hooks,
    )
    .unwrap();
    let mut events = battle.step(BattleInput::default(), hooks).unwrap();
    for _ in 0..100 {
        if battle.phase() == BattlePhase::End {
            break;
        }
        events.extend(battle.step(tackle_turn(), hooks).unwrap());
    }
    (events, battle.result().copied())
}

fn run_with(host: &LuaScriptHost) -> (Vec<BattleEvent>, Option<BattleResult>) {
    let resources = battle_testkit::default_resources();
    let mut hooks = host.battle_hooks().with_moves(&resources.pack);
    run_battle(&mut hooks)
}

fn first_hit_on_opponent(events: &[BattleEvent]) -> u32 {
    events
        .iter()
        .find_map(|e| match e {
            BattleEvent::Damage {
                target_side: SideId::Opponent,
                amount,
                ..
            } => Some(*amount),
            _ => None,
        })
        .unwrap()
}

#[test]
fn two_plugins_modify_damage_in_priority_order() {
    // `shield` (priority -5) runs before `crit-boost` (priority 10) although it loads later.
    let host = load(&damage_plugins(10, -5), &["crit-boost", "shield"]);
    let (events, result) = run_with(&host);
    let (plain, _) = run_battle(&mut NullBattleHooks);
    let base = first_hit_on_opponent(&plain);
    assert_eq!(first_hit_on_opponent(&events), (base + 3) * 2);

    let table = host.hook_table();
    let mut rendered = format!("hook table:\n{table}\nevents:\n");
    for e in &events {
        rendered.push_str(&format!("{e:?}\n"));
    }
    rendered.push_str(&format!("result: {result:?}\n"));
    insta::assert_snapshot!("two_plugins_damage_chain", rendered);
}

#[test]
fn swapping_priorities_swaps_the_composition() {
    let host = load(&damage_plugins(-5, 10), &["crit-boost", "shield"]);
    let (events, _) = run_with(&host);
    let (plain, _) = run_battle(&mut NullBattleHooks);
    assert_eq!(
        first_hit_on_opponent(&events),
        first_hit_on_opponent(&plain) * 2 + 3
    );
}

#[test]
fn same_plugins_and_order_give_the_same_battle() {
    let files = damage_plugins(0, 0);
    let a = run_with(&load(&files, &["crit-boost", "shield"]));
    let b = run_with(&load(&files, &["crit-boost", "shield"]));
    assert_eq!(a, b);
    // Equal priorities: load order decides, so changing it changes the result.
    let (plain, _) = run_battle(&mut NullBattleHooks);
    let base = first_hit_on_opponent(&plain);
    assert_eq!(first_hit_on_opponent(&a.0), base * 2 + 3);
    let swapped = run_with(&load(&files, &["shield", "crit-boost"]));
    assert_eq!(first_hit_on_opponent(&swapped.0), (base + 3) * 2);
}

#[test]
fn equal_priorities_follow_load_order_not_registration_order() {
    // Each plugin exposes `register()`; calling them in reverse load order (and the game
    // first) must not change the chain: plugins in load order, then the game.
    let files = MemoryScripts::new()
        .with("plugins/alpha/plugin.toml", manifest("alpha"))
        .with(
            "plugins/alpha/init.lua",
            "function register()
               battle.on('on_battle_start', function(ctx) ctx.message('text:alpha') end)
             end",
        )
        .with("plugins/beta/plugin.toml", manifest("beta"))
        .with(
            "plugins/beta/init.lua",
            "function register()
               battle.on('on_battle_start', function(ctx) ctx.message('text:beta') end)
             end",
        );
    let mut host = load(&files, &["alpha", "beta"]);
    host.run_source(
        &ScriptOwner::Game,
        "scripts/main.lua",
        "battle.on('on_battle_start', function(ctx) ctx.message('text:game') end)",
    )
    .unwrap();
    for id in ["beta", "alpha"] {
        host.call_function(&ScriptOwner::plugin(id), "register", &[])
            .unwrap();
    }
    let (events, _) = run_with(&host);
    let messages: Vec<&str> = events
        .iter()
        .filter_map(|e| match e {
            BattleEvent::Message { key, .. } => Some(key.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(messages[..3], ["text:alpha", "text:beta", "text:game"]);
    assert_eq!(
        host.hook_table().to_string(),
        "battle hooks:\n  on_battle_start\n       0  alpha\n       0  beta\n       0  game\n\
         world events:\n  (none)\n"
    );
}

#[test]
fn cancel_stops_the_chain_and_nil_passes_through() {
    let files = MemoryScripts::new()
        .with("plugins/guard/plugin.toml", manifest("guard"))
        .with(
            "plugins/guard/init.lua",
            "seen = 0
             battle.on('on_damage_taken', function() seen = seen + 1 end, { priority = -10 })
             battle.on('on_damage_taken', function()
               return { cancel = true, message = 'text:guarded' }
             end)
             battle.on('on_damage_taken', function() error('never runs') end, { priority = 10 })",
        );
    let mut host = load(&files, &["guard"]);
    let (events, _) = run_with(&host);
    let (plain, _) = run_battle(&mut NullBattleHooks);
    assert_ne!(events, plain, "the cancel took effect");
    let seen = host
        .global(&ScriptOwner::plugin("guard"), "seen")
        .unwrap()
        .as_int()
        .unwrap();
    assert!(seen > 0, "the nil-returning handler ran first");
    // The erroring handler after the cancel never ran, so it was never disabled.
    let table = host.hook_table();
    let entries = &table.battle["on_damage_taken"];
    assert_eq!(entries.len(), 3);
    assert!(entries.iter().all(|e| !e.disabled), "{table}");
    assert_eq!(
        entries.iter().map(|e| e.priority).collect::<Vec<_>>(),
        [-10, 0, 10]
    );
}
