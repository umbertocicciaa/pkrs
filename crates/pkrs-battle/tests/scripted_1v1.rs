//! Full 1v1 battle through the public API only.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use pkrs_battle::{
    Battle, BattleAction, BattleConfig, BattleInput, BattleOutcome, BattlePhase, NullBattleHooks,
    RngPolicy, SideId, SideInput, battle_testkit,
};

fn fight_opp() -> BattleAction {
    BattleAction::Fight {
        move_slot: 0,
        target_side: SideId::Opponent,
        target_slot: 0,
    }
}

fn fight_player() -> BattleAction {
    BattleAction::Fight {
        move_slot: 0,
        target_side: SideId::Player,
        target_slot: 0,
    }
}

fn turn_input() -> BattleInput {
    BattleInput::both(
        SideInput {
            slots: vec![fight_opp()],
        },
        SideInput {
            slots: vec![fight_player()],
        },
    )
}

#[test]
fn scripted_1v1_runs_to_player_win() {
    let setup = battle_testkit::wild_1v1();
    let config = battle_testkit::singles_config();
    let mut hooks = NullBattleHooks;
    let mut battle =
        Battle::new(setup, config, RngPolicy::Fixed { seed: 1 }, &mut hooks).expect("new");

    let empty = BattleInput::default();
    while battle.phase() != BattlePhase::End {
        let input = if battle.phase() == BattlePhase::SelectActions {
            turn_input()
        } else if matches!(battle.phase(), BattlePhase::Replace { .. }) {
            BattleInput::switch(SideId::Player, 0)
        } else {
            empty.clone()
        };
        let _events = battle.step(input, &mut hooks).expect("step");
    }

    let result = battle.result().expect("result");
    assert_eq!(result.outcome, BattleOutcome::PlayerWin);
}

#[test]
fn run_ends_battle_as_fled() {
    let setup = battle_testkit::wild_1v1();
    let mut hooks = NullBattleHooks;
    let mut battle = Battle::new(
        setup,
        BattleConfig::default(),
        RngPolicy::Fixed { seed: 2 },
        &mut hooks,
    )
    .expect("new");

    let empty = BattleInput::default();
    let _ = battle.step(empty.clone(), &mut hooks).expect("start");
    let run = BattleInput::both(
        SideInput {
            slots: vec![BattleAction::Run],
        },
        SideInput {
            slots: vec![fight_player()],
        },
    );
    let _ = battle.step(run, &mut hooks).expect("run turn");
    while battle.phase() != BattlePhase::End {
        let _ = battle.step(empty.clone(), &mut hooks).expect("finish");
    }
    assert!(matches!(
        battle.result().expect("result").outcome,
        BattleOutcome::Fled {
            side: SideId::Player
        }
    ));
}
