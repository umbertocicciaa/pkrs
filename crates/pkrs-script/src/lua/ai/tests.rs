//! Lua AI unit tests: view marshalling per visibility, read-only view, result parsing,
//! failure handling and the AI RNG stream.

use pkrs_battle::{
    AiMemory, AiView, Battle, BattleInput, BattleSetup, NullBattleHooks, battle_testkit,
};
use pkrs_core::Pcg64Rng;

use super::*;
use crate::host::{MemoryScripts, ScriptHost, ScriptLimits, ScriptValue};

const POLICY: RngPolicy = RngPolicy::Fixed { seed: 77 };

fn host_with(files: MemoryScripts) -> LuaScriptHost {
    LuaScriptHost::new(
        Box::new(files),
        Box::new(Pcg64Rng::from_seed(3)),
        ScriptLimits::default(),
    )
    .unwrap()
}

/// A 2-member party on each side (second member benched), at the first `SelectActions`.
fn battle() -> Battle {
    let mut setup: BattleSetup = battle_testkit::wild_1v1();
    let mut bench = setup.player.party[0].clone();
    bench.nickname = "reserve".into();
    setup.player.party.push(bench);
    let mut foe_bench = setup.opponent.party[0].clone();
    foe_bench.nickname = "secret".into();
    setup.opponent.party.push(foe_bench);
    let mut b = Battle::new(
        setup,
        battle_testkit::singles_config(),
        battle_testkit::default_resources(),
        POLICY,
        &mut NullBattleHooks,
    )
    .unwrap();
    b.step(BattleInput::default(), &mut NullBattleHooks)
        .unwrap();
    b
}

fn script_view(b: &Battle, visibility: AiVisibility) -> AiScriptView {
    let snapshot = b.view();
    let view = AiView {
        battle: &snapshot,
        side: SideId::Player,
        pack: &b.resources().pack,
    };
    let mut memory = AiMemory::default();
    memory.refresh(view.foe());
    AiScriptView::build(
        &view,
        AiRequest::Action { slot: 0 },
        visibility,
        &memory,
        &["potion".to_owned()],
    )
}

/// Evaluates `expr` with `view` bound to the frozen Lua view.
fn eval(h: &LuaScriptHost, view: &AiScriptView, expr: &str) -> Value {
    let lua = &h.lua;
    let f: Function = lua
        .load(format!("return function(view) return {expr} end"))
        .eval()
        .unwrap();
    let t = freeze(lua, view_table(lua, view).unwrap()).unwrap();
    f.call(t).unwrap()
}

fn int(v: Value) -> i64 {
    match v {
        Value::Integer(n) => n,
        other => panic!("expected an integer, got {other:?}"),
    }
}

#[test]
fn fair_view_hides_the_foes_secrets() {
    let h = host_with(MemoryScripts::new());
    let b = battle();
    let v = script_view(&b, AiVisibility::Fair);
    assert_eq!(eval(&h, &v, "view.visibility").to_string().unwrap(), "fair");
    // Own side: everything, including the bench and PP.
    assert_eq!(int(eval(&h, &v, "#view.own.party")), 2);
    assert_eq!(int(eval(&h, &v, "view.me.hp")), 40);
    assert_eq!(int(eval(&h, &v, "view.me.moves[1].pp")), 35);
    assert_eq!(int(eval(&h, &v, "view.own.party[2].stats.attack")), 20);
    assert_eq!(eval(&h, &v, "view.items[1]").to_string().unwrap(), "potion");
    // Foe: only the active creature; no exact HP, stats or unrevealed moves.
    assert_eq!(int(eval(&h, &v, "#view.foe.party")), 1);
    assert_eq!(int(eval(&h, &v, "view.foe.party_size")), 2);
    assert_eq!(int(eval(&h, &v, "view.foe.active[1].hp_percent")), 100);
    assert_eq!(int(eval(&h, &v, "view.foe.active[1].level")), 5);
    for hidden in ["hp", "max_hp", "stats"] {
        assert!(
            eval(&h, &v, &format!("view.foe.active[1].{hidden}")).is_nil(),
            "{hidden} must be hidden"
        );
    }
    assert_eq!(int(eval(&h, &v, "#view.foe.active[1].moves")), 0);
}

#[test]
fn full_view_shows_everything() {
    let h = host_with(MemoryScripts::new());
    let b = battle();
    let v = script_view(&b, AiVisibility::Full);
    assert_eq!(int(eval(&h, &v, "#view.foe.party")), 2);
    assert_eq!(
        eval(&h, &v, "view.foe.party[2].name").to_string().unwrap(),
        "secret"
    );
    assert!(eval(&h, &v, "view.foe.party[2].slot").is_nil(), "benched");
    assert_eq!(int(eval(&h, &v, "view.foe.active[1].hp")), 30);
    assert_eq!(int(eval(&h, &v, "view.foe.active[1].stats.speed")), 10);
    assert_eq!(int(eval(&h, &v, "view.foe.active[1].moves[1].pp")), 35);
    assert_eq!(
        eval(&h, &v, "view.foe.active[1].moves[1].category")
            .to_string()
            .unwrap(),
        "physical"
    );
}

fn ai_from(source: &str) -> LuaScriptHost {
    let files = MemoryScripts::new().with("scripts/ai/test.lua", source);
    let mut h = host_with(files);
    h.load_ai_script(&ScriptOwner::Game, "scripts/ai/test.lua")
        .unwrap();
    h
}

fn scripted_profile() -> AiProfile {
    AiProfile {
        script: Some("scripts/ai/test.lua".into()),
        ..AiProfile::builtin("test", AiTier::Basic)
    }
}

fn choose(h: &LuaScriptHost, view: &AiScriptView) -> std::result::Result<AiChoice, String> {
    let mut ai = h
        .lua_ai(&scripted_profile(), &POLICY, SideId::Player)
        .unwrap();
    ai.choose(view)
}

#[test]
fn the_view_is_read_only() {
    let b = battle();
    let v = script_view(&b, AiVisibility::Fair);
    for write in ["view.turn = 99", "view.me.hp = 1", "view.foe.party[1] = {}"] {
        let h = ai_from(&format!("function choose_action(view) {write} end"));
        let err = choose(&h, &v).unwrap_err();
        assert!(err.contains("read-only"), "{write}: {err}");
    }
    // Reading through pairs / ipairs / # still works on the proxies.
    let h = ai_from(
        "function choose_action(view)
           local n = 0
           for _, m in ipairs(view.me.moves) do n = n + 1 end
           for k in pairs(view.own) do n = n + 1 end
           return { move = view.me.moves[#view.me.moves].id, target = n }
         end",
    );
    let err = choose(&h, &v).unwrap();
    assert_eq!(
        err,
        AiChoice::Move {
            mv: AiMoveRef::Id("tackle".into()),
            target: Some(SlotRef::new(SideId::Opponent, 6)),
        },
        "1 move + 5 side fields"
    );
}

#[test]
fn results_parse_into_choices() {
    let b = battle();
    let v = script_view(&b, AiVisibility::Fair);
    let cases: &[(&str, AiChoice)] = &[
        ("nil", AiChoice::Default),
        (
            "{ move = 0 }",
            AiChoice::Move {
                mv: AiMoveRef::Slot(0),
                target: None,
            },
        ),
        (
            "{ move = 'tackle', target = view.foe.active[1] }",
            AiChoice::Move {
                mv: AiMoveRef::Id("tackle".into()),
                target: Some(SlotRef::new(SideId::Opponent, 0)),
            },
        ),
        (
            "{ move = 0, target = 0, target_side = 'player' }",
            AiChoice::Move {
                mv: AiMoveRef::Slot(0),
                target: Some(SlotRef::new(SideId::Player, 0)),
            },
        ),
        ("{ switch = 1 }", AiChoice::Switch { to_party: 1 }),
        (
            "{ switch = view.own.party[2] }",
            AiChoice::Switch { to_party: 1 },
        ),
        (
            "{ item = view.items[1], target = view.me }",
            AiChoice::Item {
                item_id: "potion".into(),
                target_slot: Some(0),
            },
        ),
    ];
    for (ret, expected) in cases {
        let h = ai_from(&format!("function choose_action(view) return {ret} end"));
        assert_eq!(&choose(&h, &v).unwrap(), expected, "{ret}");
    }
}

#[test]
fn malformed_results_are_errors() {
    let b = battle();
    let v = script_view(&b, AiVisibility::Fair);
    for ret in [
        "'tackle'",
        "42",
        "{}",
        "{ move = 0, switch = 1 }",
        "{ move = 0.5 }",
        "{ move = -1 }",
        "{ switch = 'reserve' }",
        "{ move = 0, target_side = 'left' }",
        "{ item = 3 }",
        "{ item = 'potion', target = view.foe.active[1] }",
        "{ move = 0, target = view.foe.party[1].moves }",
    ] {
        let h = ai_from(&format!("function choose_action(view) return {ret} end"));
        assert!(choose(&h, &v).is_err(), "{ret} must be rejected");
    }
}

#[test]
fn errors_yields_and_budget_overruns_are_failures() {
    let b = battle();
    let v = script_view(&b, AiVisibility::Fair);
    for body in [
        "error('boom')",
        "coroutine.yield({ wait = 'frames', n = 1 })",
        "while true do end",
        "return view.nope.field",
    ] {
        let h = ai_from(&format!("function choose_action(view) {body} end"));
        let err = choose(&h, &v).unwrap_err();
        assert!(!err.is_empty(), "{body}");
    }
    let h = ai_from("function choose_action(view) coroutine.yield() end");
    assert!(choose(&h, &v).unwrap_err().contains("yielded"));
}

#[test]
fn a_failing_script_is_disabled_after_max_failures() {
    let b = battle();
    let v = script_view(&b, AiVisibility::Fair);
    let h = ai_from("calls = 0 function choose_action(view) calls = calls + 1 error('x') end");
    let mut ai = h
        .lua_ai(&scripted_profile(), &POLICY, SideId::Player)
        .unwrap()
        .with_max_failures(2);
    for _ in 0..5 {
        assert!(ai.choose(&v).is_err());
    }
    assert!(ai.disabled());
    assert_eq!(ai.failures(), 2);
}

#[test]
fn a_profile_script_must_define_choose_action() {
    let files = MemoryScripts::new().with("scripts/ai/empty.lua", "local x = 1");
    let mut h = host_with(files);
    let err = h
        .load_ai_script(&ScriptOwner::Game, "scripts/ai/empty.lua")
        .unwrap_err();
    assert!(matches!(err, ScriptError::NoSuchFunction { .. }), "{err}");
    assert!(
        h.load_ai_script(&ScriptOwner::Game, "scripts/ai/missing.lua")
            .is_err()
    );
}

#[test]
fn profile_scripts_do_not_share_globals() {
    let files = MemoryScripts::new()
        .with(
            "scripts/ai/a.lua",
            "function choose_action(view) return { move = 0 } end",
        )
        .with(
            "scripts/ai/b.lua",
            "function choose_action(view) return { switch = 1 } end",
        );
    let mut h = host_with(files);
    h.load_ai_script(&ScriptOwner::Game, "scripts/ai/a.lua")
        .unwrap();
    h.load_ai_script(&ScriptOwner::Game, "scripts/ai/b.lua")
        .unwrap();
    let b = battle();
    let v = script_view(&b, AiVisibility::Fair);
    let profile = |path: &str| AiProfile {
        script: Some(path.into()),
        ..AiProfile::builtin(path, AiTier::Basic)
    };
    let mut a = h
        .lua_ai(&profile("scripts/ai/a.lua"), &POLICY, SideId::Player)
        .unwrap();
    let mut bb = h
        .lua_ai(&profile("scripts/ai/b.lua"), &POLICY, SideId::Player)
        .unwrap();
    assert!(matches!(a.choose(&v).unwrap(), AiChoice::Move { .. }));
    assert!(matches!(bb.choose(&v).unwrap(), AiChoice::Switch { .. }));
    assert!(
        h.lua_ai(
            &AiProfile::builtin("smart", AiTier::Smart),
            &POLICY,
            SideId::Player
        )
        .is_none(),
        "a purely built-in profile has no Lua AI"
    );
}

#[test]
fn math_random_uses_the_ai_stream_only() {
    let src = "function choose_action(view) return { move = math.random(0, 3) } end";
    let picks = |h: &LuaScriptHost| -> Vec<AiChoice> {
        let b = battle();
        let v = script_view(&b, AiVisibility::Fair);
        let mut ai = h
            .lua_ai(&scripted_profile(), &POLICY, SideId::Player)
            .unwrap();
        (0..12).map(|_| ai.choose(&v).unwrap()).collect()
    };
    let mut h1 = ai_from(src);
    let first = picks(&h1);
    // A second AI from the same policy, on a host whose shared RNG has advanced, decides
    // identically: the script draws from the AI's own stream.
    let mut h2 = ai_from(src);
    h2.run_source(
        &ScriptOwner::Game,
        "x.lua",
        "for i = 1, 10 do math.random() end",
    )
    .unwrap();
    assert_eq!(picks(&h2), first);
    assert!(
        first.iter().any(|c| c != &first[0]),
        "the stream actually varies"
    );
    // ...and the shared script RNG is untouched by AI calls.
    let roll = "return math.random(1, 1000000)";
    let mut fresh = ai_from(src);
    assert_eq!(
        h1.run_source(&ScriptOwner::Game, "r.lua", roll).unwrap(),
        fresh.run_source(&ScriptOwner::Game, "r.lua", roll).unwrap()
    );
}

#[test]
fn plugins_register_and_replace_profiles() {
    let mut h = host_with(MemoryScripts::new());
    h.run_source(
        &ScriptOwner::plugin("gauntlet"),
        "plugins/gauntlet/main.lua",
        "ai.register_profile('gauntlet:boss', function(view) return { move = 0 } end,
           { tier = 'smart', visibility = 'full', allow_items = true })
         ai.register_profile('test', function(view) return { switch = 1 } end)",
    )
    .unwrap();
    assert_eq!(h.ai_profile_ids(), ["gauntlet:boss", "test"]);
    let boss = h.registered_ai_profile("gauntlet:boss").unwrap();
    assert_eq!(boss.tier, AiTier::Smart);
    assert_eq!(boss.visibility, AiVisibility::Full);
    assert!(boss.allow_items);
    assert!(h.registered_ai_profile("nope").is_none());

    let b = battle();
    let v = script_view(&b, AiVisibility::Fair);
    // The registered `test` replaces the data profile's script (which is not even loaded).
    let mut ai = h
        .lua_ai(&scripted_profile(), &POLICY, SideId::Player)
        .unwrap();
    assert_eq!(ai.choose(&v).unwrap(), AiChoice::Switch { to_party: 1 });

    for bad in [
        "ai.register_profile('', function() end)",
        "ai.register_profile('x', 3)",
        "ai.register_profile('x', function() end, { tier = 'genius' })",
        "ai.register_profile('x', function() end, { visibility = 'xray' })",
        "ai.register_profile('x', function() end, { heal_below_percent = 101 })",
    ] {
        assert!(
            h.run_source(&ScriptOwner::Game, "bad.lua", bad).is_err(),
            "{bad}"
        );
    }
    assert_eq!(
        h.run_source(
            &ScriptOwner::Game,
            "t.lua",
            "return type(ai.register_profile)"
        )
        .unwrap(),
        ScriptValue::String("function".into())
    );
}
