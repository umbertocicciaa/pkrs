//! Seeded 2v2 battle replay (issue #50).

#![allow(clippy::unwrap_used, clippy::expect_used)]

use insta::assert_snapshot;
use pkrs_battle::{
    Battle, BattleAction, BattleConfig, BattleCreature, BattleInput, BattlePhase, BattleSetup,
    BattleSideSetup, CreatureStats, MoveSlot, NullBattleHooks, RngPolicy, SideId, SideInput,
    battle_testkit,
};
use pkrs_data::{MoveCategory, MoveDef, MoveEffect, MoveTargetScope};

fn creature(name: &str, hp: u32, speed: u32, move_id: &str) -> BattleCreature {
    let mut moves: [Option<MoveSlot>; 4] = Default::default();
    moves[0] = Some(MoveSlot {
        id: move_id.into(),
        priority: 0,
        power: 35,
        pp: 20,
        max_pp: 20,
    });
    BattleCreature {
        species_id: name.into(),
        types: vec!["normal".into()],
        nickname: name.into(),
        level: 10,
        stats: CreatureStats::new(hp, 20, 10, speed),
        moves,
    }
}

fn doubles_setup(spread: bool) -> BattleSetup {
    let id = if spread { "spread_tackle" } else { "tackle" };
    BattleSetup {
        player: BattleSideSetup {
            name: "P".into(),
            party: vec![creature("pa", 50, 12, id), creature("pb", 50, 10, id)],
            leading: vec![Some(0), Some(1)],
        },
        opponent: BattleSideSetup {
            name: "O".into(),
            party: vec![
                creature("oa", 40, 8, "tackle"),
                creature("ob", 40, 6, "tackle"),
            ],
            leading: vec![Some(0), Some(1)],
        },
    }
}

fn fight(target_slot: u8) -> BattleAction {
    BattleAction::Fight {
        move_slot: 0,
        target_side: SideId::Opponent,
        target_slot,
    }
}

fn turn() -> BattleInput {
    BattleInput::both(
        SideInput {
            slots: vec![fight(0), fight(1)],
        },
        SideInput {
            slots: vec![fight(0), fight(1)],
        },
    )
}

#[test]
fn doubles_battle_replay_snapshot() {
    let mut resources = battle_testkit::default_resources();
    resources
        .pack
        .insert_move(MoveDef {
            id: "spread_tackle".into(),
            type_id: "normal".into(),
            category: MoveCategory::Physical,
            power: 20,
            accuracy: 100,
            priority: 0,
            targeting: MoveTargetScope::AllFoes,
            effects: vec![MoveEffect::Damage],
        })
        .expect("spread");
    let mut hooks = NullBattleHooks;
    let config = BattleConfig {
        trace_hooks: false,
        ..BattleConfig::with_side_size(2)
    };
    let mut battle = Battle::new(
        doubles_setup(true),
        config,
        resources,
        RngPolicy::Fixed { seed: 50_002 },
        &mut hooks,
    )
    .unwrap();
    let mut log = String::new();
    for e in battle.step(BattleInput::default(), &mut hooks).unwrap() {
        log.push_str(&format!("{e:?}\n"));
    }
    for _ in 0..6 {
        if battle.phase() == BattlePhase::End {
            break;
        }
        if battle.phase() == BattlePhase::SelectActions {
            for e in battle.step(turn(), &mut hooks).unwrap() {
                log.push_str(&format!("{e:?}\n"));
            }
        } else {
            for e in battle.step(BattleInput::default(), &mut hooks).unwrap() {
                log.push_str(&format!("{e:?}\n"));
            }
        }
    }
    assert_snapshot!("doubles_replay__doubles_battle_log", log);
}
