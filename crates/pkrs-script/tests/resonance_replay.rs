//! Seeded battle replay with the Resonance plugin hooks (§9.5, issue #48).

#![allow(clippy::unwrap_used, clippy::expect_used)]

use pkrs_battle::{
    Battle, BattleAction, BattleConfig, BattleInput, BattlePhase, RngPolicy, SideId, SideInput,
    SlotRef, battle_testkit,
};
use pkrs_core::Pcg64Rng;
use pkrs_script::{LuaScriptHost, MemoryScripts, ScriptHost, ScriptLimits, ScriptOwner};

const RESONANCE: &str = include_str!("fixtures/resonance.lua");

fn host() -> LuaScriptHost {
    LuaScriptHost::new(
        Box::new(MemoryScripts::new()),
        Box::new(Pcg64Rng::from_seed(48)),
        ScriptLimits::default(),
    )
    .unwrap()
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

#[test]
fn resonance_battle_replay_snapshot() {
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
    let pack = resources.pack.clone();
    let mut hooks = h.battle_hooks().with_moves(&pack);
    let mut battle = Battle::new(
        battle_testkit::wild_1v1(),
        config(),
        resources,
        RngPolicy::Fixed { seed: 48_001 },
        &mut hooks,
    )
    .unwrap();
    let mut log = String::new();
    for e in battle.step(BattleInput::default(), &mut hooks).unwrap() {
        log.push_str(&format!("{e:?}\n"));
    }
    let player = SlotRef::new(SideId::Player, 0);
    let menu = battle.action_menu(player, &mut hooks).unwrap();
    assert!(menu.contains("resonance:resonate"));

    let resonate = BattleAction::Custom {
        id: "resonance:resonate".into(),
        payload: vec![],
    };
    for e in battle.step(turn_with(resonate), &mut hooks).unwrap() {
        log.push_str(&format!("{e:?}\n"));
    }
    for _ in 0..6 {
        if battle.phase() == BattlePhase::End {
            break;
        }
        for e in battle.step(tackle_turn(), &mut hooks).unwrap() {
            log.push_str(&format!("{e:?}\n"));
        }
    }
    if let Some(result) = battle.result() {
        log.push_str(&format!("result: {result:?}\n"));
    }

    insta::assert_snapshot!("resonance_plugin_battle_log", log);
}
