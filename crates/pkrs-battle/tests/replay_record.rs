//! Seeded replay record + insta snapshot (§15.2).

#![allow(clippy::unwrap_used, clippy::expect_used)]

use pkrs_battle::{
    Battle, BattleAction, BattleConfig, BattleInput, BattleOutcome, BattlePhase, NullBattleHooks,
    RngPolicy, SideId, SideInput, battle_testkit, config_hash, record_battle,
    replay_with_null_hooks,
};

fn turn_input() -> BattleInput {
    BattleInput::both(
        SideInput {
            slots: vec![BattleAction::Fight {
                move_slot: 0,
                target_side: SideId::Opponent,
                target_slot: 0,
            }],
        },
        SideInput {
            slots: vec![BattleAction::Fight {
                move_slot: 0,
                target_side: SideId::Player,
                target_slot: 0,
            }],
        },
    )
}

fn produce_input(battle: &Battle) -> BattleInput {
    match battle.phase() {
        BattlePhase::SelectActions => turn_input(),
        BattlePhase::Replace { .. } => BattleInput::switch(SideId::Player, 0),
        _ => BattleInput::default(),
    }
}

#[test]
fn recorded_wild_1v1_replays_byte_identical() {
    let setup = battle_testkit::wild_1v1();
    let config = battle_testkit::singles_config();
    let resources = battle_testkit::default_resources();
    let policy = RngPolicy::Fixed { seed: 9001 };
    let mut hooks = NullBattleHooks;
    let (record, events, result) = record_battle(
        setup.clone(),
        config,
        resources.clone(),
        policy,
        produce_input,
        &mut hooks,
    )
    .expect("record");

    let (replay_events, replay_result) =
        replay_with_null_hooks(&record, &setup, config, resources).expect("replay");
    assert_eq!(events, replay_events);
    assert_eq!(result, replay_result);
    assert_eq!(result.outcome, BattleOutcome::PlayerWin);
    assert_eq!(record.config_hash, config_hash(config));
}

#[test]
fn wild_1v1_event_log_insta_snapshot() {
    let setup = battle_testkit::wild_1v1();
    // Pin tracing off so the snapshot does not depend on the `hook-trace` feature.
    let config = BattleConfig {
        trace_hooks: false,
        ..battle_testkit::singles_config()
    };
    let resources = battle_testkit::default_resources();
    let policy = RngPolicy::Fixed { seed: 4242 };
    let mut hooks = NullBattleHooks;
    let (_, events, result) =
        record_battle(setup, config, resources, policy, produce_input, &mut hooks).expect("record");

    let mut rendered = String::new();
    for e in &events {
        rendered.push_str(&format!("{e:?}\n"));
    }
    rendered.push_str(&format!("result: {result:?}\n"));
    insta::assert_snapshot!("wild_1v1_replay", rendered);
}
