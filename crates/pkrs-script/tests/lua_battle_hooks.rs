//! Integration (§7.4, §9.1, §9.6): Lua battle handlers through `LuaBattleHooks`.
//!
//! Regenerate the snapshot with `PKRS_UPDATE_SNAPSHOTS=1 cargo test -p pkrs-script --test lua_battle_hooks`.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::Path;

use pkrs_battle::{
    Battle, BattleAction, BattleConfig, BattleCreature, BattleEvent, BattleHooks, BattleInput,
    BattleOutcome, BattlePhase, BattleResult, BattleSetup, BattleSideSetup, BattleValue,
    NullBattleHooks, RngPolicy, SideId, SideInput, SlotRef, battle_testkit,
};
use pkrs_core::Pcg64Rng;
use pkrs_script::{
    BattleRequest, LuaScriptHost, MemoryScripts, ScriptHost, ScriptLimits, ScriptOwner,
    ScriptValue, TaskStatus, WaitRequest, battle_result_value,
};

const FIXTURE: &str = include_str!("fixtures/battle_hooks.lua");
const RESONANCE: &str = include_str!("fixtures/resonance.lua");

fn host() -> LuaScriptHost {
    LuaScriptHost::new(
        Box::new(MemoryScripts::new()),
        Box::new(Pcg64Rng::from_seed(5)),
        ScriptLimits::default(),
    )
    .unwrap()
}

fn host_with(source: &str) -> LuaScriptHost {
    let mut h = host();
    h.run_source(&ScriptOwner::Game, "scripts/battle_test.lua", source)
        .unwrap();
    h
}

fn config() -> BattleConfig {
    BattleConfig {
        trace_hooks: false,
        ..BattleConfig::default()
    }
}

fn fight(side: SideId) -> BattleAction {
    BattleAction::Fight {
        move_slot: 0,
        target_side: side,
        target_slot: 0,
    }
}

fn turn_with(player: BattleAction) -> BattleInput {
    BattleInput::both(
        SideInput {
            slots: vec![player],
        },
        SideInput {
            slots: vec![fight(SideId::Player)],
        },
    )
}

fn tackle_turn() -> BattleInput {
    turn_with(fight(SideId::Opponent))
}

/// Runs the testkit 1v1 to the end with `hooks`.
fn run_battle(hooks: &mut dyn BattleHooks) -> (Vec<BattleEvent>, BattleResult) {
    let mut battle = Battle::new(
        battle_testkit::wild_1v1(),
        config(),
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
    (events, *battle.result().unwrap())
}

#[test]
fn fixture_modifies_damage_and_cancels_a_move() {
    let h = host_with(FIXTURE);
    let resources = battle_testkit::default_resources();
    let mut hooks = h.battle_hooks().with_moves(&resources.pack);
    let (events, result) = run_battle(&mut hooks);

    assert!(events.contains(&BattleEvent::MoveCancelled {
        user_side: SideId::Opponent,
        user_slot: 0,
        move_id: "tackle".into(),
        reason: Some("text:fixture.flinch".into()),
    }));
    assert!(events.contains(&BattleEvent::Message {
        key: "text:fixture.turn_1".into()
    }));
    assert_eq!(result.money, 77);
    assert_eq!(result.outcome, BattleOutcome::PlayerWin);

    // Same seed without hooks: the player's hits are exactly doubled by the Lua modifier.
    let (plain, _) = run_battle(&mut NullBattleHooks);
    let first_hit = |evs: &[BattleEvent]| {
        evs.iter().find_map(|e| match e {
            BattleEvent::Damage {
                target_side: SideId::Opponent,
                amount,
                ..
            } => Some(*amount),
            _ => None,
        })
    };
    assert_eq!(first_hit(&events).unwrap(), first_hit(&plain).unwrap() * 2);

    let mut rendered: String = events.iter().map(|e| format!("{e:?}\n")).collect();
    rendered.push_str(&format!("result: {result:?}\n"));
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/snapshots/lua_hooks.snap");
    if std::env::var_os("PKRS_UPDATE_SNAPSHOTS").is_some() {
        std::fs::write(&path, &rendered).unwrap();
    }
    let expected = std::fs::read_to_string(&path).unwrap_or_else(|_| {
        panic!(
            "missing snapshot {}; run with PKRS_UPDATE_SNAPSHOTS=1",
            path.display()
        )
    });
    assert_eq!(rendered, expected.replace("\r\n", "\n"));
}

#[test]
fn a_throwing_hook_never_crashes_the_battle() {
    let mut source = String::from("calls = 0\n");
    for hook in pkrs_script::HOOK_NAMES {
        source.push_str(&format!(
            "battle.on('{hook}', function() calls = calls + 1; error('kaboom in {hook}') end)\n"
        ));
    }
    let mut h = host_with(&source);
    let (events, result) = {
        let mut hooks = h.battle_hooks().with_max_failures(0);
        run_battle(&mut hooks)
    };
    let (plain, plain_result) = run_battle(&mut NullBattleHooks);
    assert_eq!(
        events, plain,
        "failed handlers pass every value through unchanged"
    );
    assert_eq!(result, plain_result);
    assert!(
        h.global(&ScriptOwner::Game, "calls")
            .unwrap()
            .as_int()
            .unwrap()
            > 10
    );
}

#[test]
fn failing_handlers_are_disabled_after_n_consecutive_failures() {
    let mut h = host_with(
        "always, flaky = 0, 0
         battle.on('on_accuracy', function() always = always + 1; error('nope') end)
         battle.on('on_accuracy', function()
           flaky = flaky + 1
           if flaky % 2 == 1 then error('every other call') end
         end)",
    );
    {
        let mut hooks = h.battle_hooks();
        run_battle(&mut hooks);
    }
    assert_eq!(
        h.global(&ScriptOwner::Game, "always").unwrap(),
        ScriptValue::Int(3),
        "disabled after 3 consecutive failures"
    );
    let flaky = h
        .global(&ScriptOwner::Game, "flaky")
        .unwrap()
        .as_int()
        .unwrap();
    assert!(flaky > 3, "a success resets the counter: {flaky}");
}

#[test]
fn a_failed_handler_leaves_no_state_changes() {
    let h = host_with(
        "battle.on('on_turn_start', function(ctx)
           ctx.set_side_state('player', 'half_done', true)
           ctx.message('text:never')
           error('fails after writing')
         end)",
    );
    let mut hooks = h.battle_hooks();
    let (events, _) = run_battle(&mut hooks);
    assert!(!events.iter().any(|e| matches!(
        e,
        BattleEvent::SideStateChanged { .. } | BattleEvent::Message { .. }
    )));
}

#[test]
fn handlers_run_in_priority_then_registration_order() {
    let mut h = host_with(
        "order = {}
         battle.on('on_battle_start', function() order[#order + 1] = 'b' end)
         battle.on('on_battle_start', function() order[#order + 1] = 'late' end, { priority = 5 })
         battle.on('on_battle_start', function() order[#order + 1] = 'early' end, { priority = -5 })
         battle.on('on_battle_start', function() order[#order + 1] = 'c' end)",
    );
    {
        let mut hooks = h.battle_hooks();
        run_battle(&mut hooks);
    }
    assert_eq!(
        h.global(&ScriptOwner::Game, "order").unwrap(),
        ScriptValue::List(["early", "b", "c", "late"].map(ScriptValue::from).to_vec())
    );
}

#[test]
fn hook_values_marshal_both_ways() {
    let mut h = host_with(
        "seen = {}
         battle.on('on_damage_taken', function(ctx, target, amount, source)
           seen.target = target.side .. ':' .. target.slot
           seen.source = source.name
           seen.types = target.types[1]
           return 1
         end)
         battle.on('on_status_apply', function(ctx, target, status) return 'renamed' end)
         battle.on('on_turn_start', function(ctx)
           ctx.set_volatile(ctx.creature('player', 0), 'mark', { n = 2, tags = { 'a', 'b' } })
           local back = ctx.volatile(ctx.creature('player', 0), 'mark')
           seen.round_trip = back.n == 2 and back.tags[2] == 'b'
           seen.roll = ctx:random(6)
         end)",
    );
    let events = {
        let mut hooks = h.battle_hooks();
        run_battle(&mut hooks).0
    };
    assert!(events.iter().all(|e| !matches!(
        e,
        BattleEvent::Damage { amount, .. } if *amount != 1
    )));
    let ScriptValue::Map(seen) = h.global(&ScriptOwner::Game, "seen").unwrap() else {
        panic!("seen table")
    };
    assert_eq!(seen["round_trip"], ScriptValue::Bool(true));
    assert!((1..=6).contains(&seen["roll"].as_int().unwrap()));
    assert_eq!(seen["types"], ScriptValue::from("normal"));
    assert!(events.iter().any(|e| matches!(
        e,
        BattleEvent::VolatileChanged { key, value: Some(BattleValue::Map(_)), .. } if key == "mark"
    )));
}

#[test]
fn unknown_hook_names_are_rejected() {
    let mut h = host();
    let err = h
        .run_source(
            &ScriptOwner::Game,
            "scripts/x.lua",
            "battle.on('on_lunch', function() end)",
        )
        .unwrap_err();
    assert!(err.to_string().contains("unknown hook 'on_lunch'"), "{err}");
}

#[test]
fn start_wild_yields_until_the_battle_result() {
    let mut h = host_with(
        "function encounter()
           local result = battle.start_wild('emberkit', 7)
           return result.outcome .. ':' .. result.turns
         end
         function gauntlet()
           return battle.start_trainer('champion_aria').money
         end",
    );
    let (task, status) = h.start_task(&ScriptOwner::Game, "encounter", &[]).unwrap();
    assert_eq!(
        status,
        TaskStatus::Waiting(WaitRequest::Battle(BattleRequest::Wild {
            species: "emberkit".into(),
            level: 7,
        }))
    );
    assert_eq!(h.task_count(), 1);
    let result = BattleResult {
        outcome: BattleOutcome::PlayerWin,
        turns: 4,
        exp: 10,
        money: 0,
    };
    assert_eq!(
        h.resume_task(task, battle_result_value(&result)).unwrap(),
        TaskStatus::Finished(ScriptValue::from("win:4"))
    );
    assert_eq!(h.task_count(), 0);

    let (task, status) = h.start_task(&ScriptOwner::Game, "gauntlet", &[]).unwrap();
    assert_eq!(
        status,
        TaskStatus::Waiting(WaitRequest::Battle(BattleRequest::Trainer {
            id: "champion_aria".into()
        }))
    );
    let paid = BattleResult {
        money: 500,
        ..result
    };
    assert_eq!(
        h.resume_task(task, battle_result_value(&paid)).unwrap(),
        TaskStatus::Finished(ScriptValue::Int(500))
    );
}

#[test]
fn start_wild_outside_a_task_is_an_error() {
    let mut h = host();
    let err = h
        .run_source(
            &ScriptOwner::Game,
            "scripts/x.lua",
            "battle.start_wild('emberkit', 5)",
        )
        .unwrap_err();
    assert!(
        err.to_string()
            .contains("must be called from a script task"),
        "{err}"
    );
    h.run_source(
        &ScriptOwner::Game,
        "scripts/y.lua",
        "function bad() return battle.start_wild('x', 300) end",
    )
    .unwrap();
    assert!(
        h.start_task(&ScriptOwner::Game, "bad", &[]).is_err(),
        "level out of range"
    );
    assert_eq!(h.task_count(), 0, "a failed task is dropped");
}

#[test]
fn resonance_plugin_runs_on_the_battle_api() {
    let h = {
        let mut h = host();
        h.run_source(
            &ScriptOwner::plugin("resonance"),
            "plugins/resonance/init.lua",
            RESONANCE,
        )
        .unwrap();
        h
    };
    let resources = battle_testkit::default_resources();
    let mut hooks = h.battle_hooks().with_moves(&resources.pack);
    let mut battle = Battle::new(
        battle_testkit::wild_1v1(),
        config(),
        battle_testkit::default_resources(),
        RngPolicy::Fixed { seed: 11 },
        &mut hooks,
    )
    .unwrap();
    battle.step(BattleInput::default(), &mut hooks).unwrap();
    let player = SlotRef::new(SideId::Player, 0);

    let menu = battle.action_menu(player, &mut hooks).unwrap();
    assert!(menu.contains("resonance:resonate"));

    let resonate = BattleAction::Custom {
        id: "resonance:resonate".into(),
        payload: vec![],
    };
    let events = battle.step(turn_with(resonate), &mut hooks).unwrap();
    for expected in [
        BattleEvent::SideStateChanged {
            side: SideId::Player,
            key: "resonance_used".into(),
            value: Some(BattleValue::Bool(true)),
        },
        BattleEvent::Message {
            key: "text:resonance.start".into(),
        },
        BattleEvent::Fx {
            side: SideId::Player,
            slot: 0,
            id: Some("resonance:aura".into()),
        },
        BattleEvent::CustomActionUsed {
            side: SideId::Player,
            slot: 0,
            id: "resonance:resonate".into(),
            handled: true,
        },
    ] {
        assert!(
            events.contains(&expected),
            "missing {expected:?} in {events:#?}"
        );
    }
    assert!(
        !battle
            .action_menu(player, &mut hooks)
            .unwrap()
            .contains("resonance:resonate")
    );

    // The end-of-turn countdown wrote the volatile back with one turn less.
    let turns = |b: &Battle| match b
        .view()
        .creature(player)
        .unwrap()
        .volatiles
        .get("resonating")
    {
        Some(BattleValue::Map(m)) => m.get("turns").cloned(),
        _ => None,
    };
    assert_eq!(turns(&battle), Some(BattleValue::Int(2)));

    // Next turn the boosted tackle (normal move, normal user) carries the 1.5x modifier.
    let events = battle.step(tackle_turn(), &mut hooks).unwrap();
    assert!(events.iter().any(|e| matches!(
        e,
        BattleEvent::Damage {
            target_side: SideId::Opponent,
            ..
        }
    )));
}

#[test]
fn resonance_plugin_clears_its_aura_when_the_volatile_ends() {
    let h = {
        let mut h = host();
        h.run_source(
            &ScriptOwner::plugin("resonance"),
            "plugins/resonance/init.lua",
            RESONANCE,
        )
        .unwrap();
        h
    };
    // Sturdy creatures so the volatile runs out before the battle ends.
    let mut setup = battle_testkit::wild_1v1();
    setup.opponent.party[0].stats.max_hp = 500;
    setup.player.party[0].stats.max_hp = 500;
    let resources = battle_testkit::default_resources();
    let mut hooks = h.battle_hooks().with_moves(&resources.pack);
    let mut battle = Battle::new(
        setup,
        config(),
        battle_testkit::default_resources(),
        RngPolicy::Fixed { seed: 11 },
        &mut hooks,
    )
    .unwrap();
    let mut events = battle.step(BattleInput::default(), &mut hooks).unwrap();
    let resonate = BattleAction::Custom {
        id: "resonance:resonate".into(),
        payload: vec![],
    };
    events.extend(battle.step(turn_with(resonate), &mut hooks).unwrap());
    for _ in 0..3 {
        events.extend(battle.step(tackle_turn(), &mut hooks).unwrap());
    }
    let fx: Vec<_> = events
        .iter()
        .filter(|e| matches!(e, BattleEvent::Fx { .. }))
        .cloned()
        .collect();
    assert_eq!(
        fx,
        [
            BattleEvent::Fx {
                side: SideId::Player,
                slot: 0,
                id: Some("resonance:aura".into()),
            },
            BattleEvent::Fx {
                side: SideId::Player,
                slot: 0,
                id: None,
            },
        ]
    );
    // The clear follows the volatile removal and the end text, in the same hook.
    let cleared = events
        .iter()
        .position(|e| {
            matches!(e, BattleEvent::VolatileChanged { key, value: None, .. } if key == "resonating")
        })
        .expect("resonating cleared");
    assert!(
        matches!(
            &events[cleared + 1..],
            [
                BattleEvent::Message { .. },
                BattleEvent::Fx { id: None, .. },
                ..
            ]
        ),
        "{:#?}",
        &events[cleared..]
    );
}

fn doubles_creature(name: &str) -> BattleCreature {
    let mut c = battle_testkit::wild_1v1().player.party[0].clone();
    c.nickname = name.into();
    c.stats.max_hp = 500;
    c
}

fn doubles_setup() -> BattleSetup {
    BattleSetup {
        player: BattleSideSetup {
            name: "P".into(),
            party: vec![doubles_creature("pa"), doubles_creature("pb")],
            leading: vec![Some(0), Some(1)],
        },
        opponent: BattleSideSetup {
            name: "O".into(),
            party: vec![doubles_creature("oa"), doubles_creature("ob")],
            leading: vec![Some(0), Some(1)],
        },
    }
}

#[test]
fn play_fx_and_clear_fx_target_the_creatures_side_and_slot() {
    let h = host_with(
        "battle.on('on_pre_turn', function(ctx)
           ctx.play_fx('test:glow', ctx.creature('opponent', 1))
           ctx:play_fx('test:spark', ctx.creature('player', 0))
         end)
         battle.on('on_end_of_turn', function(ctx)
           ctx:clear_fx(ctx.creature('opponent', 1))
         end)",
    );
    let resources = battle_testkit::default_resources();
    let mut hooks = h.battle_hooks().with_moves(&resources.pack);
    let mut battle = Battle::new(
        doubles_setup(),
        BattleConfig {
            trace_hooks: false,
            ..BattleConfig::with_side_size(2)
        },
        battle_testkit::default_resources(),
        RngPolicy::Fixed { seed: 11 },
        &mut hooks,
    )
    .unwrap();
    battle.step(BattleInput::default(), &mut hooks).unwrap();
    let side = |target_side| SideInput {
        slots: vec![
            BattleAction::Fight {
                move_slot: 0,
                target_side,
                target_slot: 0,
            };
            2
        ],
    };
    let events = battle
        .step(
            BattleInput::both(side(SideId::Opponent), side(SideId::Player)),
            &mut hooks,
        )
        .unwrap();
    let fx: Vec<_> = events
        .iter()
        .take_while(|e| !matches!(e, BattleEvent::TurnEnded { .. }))
        .filter(|e| matches!(e, BattleEvent::Fx { .. }))
        .cloned()
        .collect();
    assert_eq!(
        fx,
        [
            BattleEvent::Fx {
                side: SideId::Opponent,
                slot: 1,
                id: Some("test:glow".into()),
            },
            BattleEvent::Fx {
                side: SideId::Player,
                slot: 0,
                id: Some("test:spark".into()),
            },
            BattleEvent::Fx {
                side: SideId::Opponent,
                slot: 1,
                id: None,
            },
        ]
    );
}

#[test]
fn play_fx_and_clear_fx_reject_bad_arguments() {
    let mut h = host_with(
        "errs = {}
         local function err(f, ...)
           local ok, e = pcall(f, ...)
           if ok then return 'no error' end
           return tostring(e)
         end
         battle.on('on_battle_start', function(ctx)
           local foe = ctx.creature('opponent', 0)
           errs.no_id = err(ctx.play_fx, nil, foe)
           errs.empty_id = err(ctx.play_fx, '', foe)
           errs.no_creature = err(ctx.play_fx, 'test:glow')
           errs.bad_creature = err(ctx.play_fx, 'test:glow', { side = 'left', slot = 0 })
           errs.clear_number = err(ctx.clear_fx, 42)
           errs.clear_bad_slot = err(ctx.clear_fx, { side = 'opponent', slot = -1 })
         end)",
    );
    let events = {
        let mut hooks = h.battle_hooks();
        run_battle(&mut hooks).0
    };
    assert!(
        !events.iter().any(|e| matches!(e, BattleEvent::Fx { .. })),
        "failed calls emit nothing"
    );
    let ScriptValue::Map(errs) = h.global(&ScriptOwner::Game, "errs").unwrap() else {
        panic!("errs is not a table");
    };
    for (key, needle) in [
        (
            "no_id",
            "ctx.play_fx(id, creature) needs a non-empty string id",
        ),
        (
            "empty_id",
            "ctx.play_fx(id, creature) needs a non-empty string id",
        ),
        (
            "no_creature",
            "ctx.play_fx(id, creature): expected a creature table, got nil",
        ),
        (
            "bad_creature",
            "ctx.play_fx(id, creature): unknown side 'left'",
        ),
        (
            "clear_number",
            "ctx.clear_fx(creature): expected a creature table, got integer",
        ),
        (
            "clear_bad_slot",
            "ctx.clear_fx(creature): slot out of range",
        ),
    ] {
        let Some(ScriptValue::String(value)) = errs.get(key) else {
            panic!("{key}: {:?}", errs.get(key));
        };
        assert!(value.contains(needle), "{key}: {value}");
    }
}
