//! Integration (§7.4): a full battle with a recording hook asserts the exact hook call
//! sequence, and hook traces match it one-to-one.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use pkrs_battle::{
    Battle, BattleAction, BattleConfig, BattleEvent, BattleInput, BattleOutcome, BattlePhase,
    RecordingBattleHooks, RngPolicy, SideId, SideInput, battle_testkit,
};

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

fn run(config: BattleConfig) -> (Vec<&'static str>, Vec<BattleEvent>, BattleOutcome) {
    let mut hooks = RecordingBattleHooks::default();
    let mut battle = Battle::new(
        battle_testkit::wild_1v1(),
        config,
        battle_testkit::default_resources(),
        RngPolicy::Fixed { seed: 1 },
        &mut hooks,
    )
    .unwrap();
    let mut events = battle.step(BattleInput::default(), &mut hooks).unwrap();
    while battle.phase() != BattlePhase::End {
        events.extend(battle.step(tackle_turn(), &mut hooks).unwrap());
    }
    let outcome = battle.result().unwrap().outcome;
    (hooks.log, events, outcome)
}

#[test]
fn recording_hook_sees_the_exact_call_sequence() {
    let (log, events, outcome) = run(BattleConfig::default());
    assert_eq!(outcome, BattleOutcome::PlayerWin);

    // Per full turn where both sides hit: start/order hooks, then for each move
    // before_move → accuracy → damage_calc → damage_taken → after_move.
    let mv = [
        "on_before_move",
        "on_accuracy",
        "on_damage_calc",
        "on_damage_taken",
        "on_after_move",
    ];
    let mut expected = vec!["on_battle_start", "on_turn_start"];
    let turns = events
        .iter()
        .filter(|e| matches!(e, BattleEvent::TurnEnded { .. }))
        .count();
    for _ in 0..turns {
        expected.extend(["on_pre_turn", "on_action_order"]);
        expected.extend(mv);
        expected.extend(mv);
        expected.extend(["on_end_of_turn", "on_turn_start"]);
    }
    // Final turn: the player (faster) knocks the foe out; the foe's action is dropped.
    expected.extend(["on_pre_turn", "on_action_order"]);
    expected.extend(&mv[..4]);
    expected.extend(["on_faint", "on_after_move", "on_battle_end"]);
    assert_eq!(log, expected);
}

#[test]
fn hook_traces_mirror_the_recorded_calls() {
    let (log, events, _) = run(BattleConfig {
        trace_hooks: true,
        ..BattleConfig::default()
    });
    let traces: Vec<_> = events
        .iter()
        .filter_map(|e| match e {
            BattleEvent::HookTrace { hook } => Some(hook.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(traces, log);
}

#[test]
fn hooks_do_not_change_the_untraced_event_stream() {
    let (_, traced, _) = run(BattleConfig {
        trace_hooks: true,
        ..BattleConfig::default()
    });
    let (_, plain, _) = run(BattleConfig {
        trace_hooks: false,
        ..BattleConfig::default()
    });
    let stripped: Vec<_> = traced
        .into_iter()
        .filter(|e| !matches!(e, BattleEvent::HookTrace { .. }))
        .collect();
    assert_eq!(stripped, plain, "tracing is purely additive");
}
