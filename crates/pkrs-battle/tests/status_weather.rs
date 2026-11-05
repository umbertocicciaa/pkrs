//! Status and weather integration.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use pkrs_battle::{
    Battle, BattleAction, BattleConfig, BattleCreature, BattleEvent, BattleInput, BattlePhase,
    BattleResources, BattleSetup, BattleSideSetup, CreatureStats, MoveSlot, NullBattleHooks,
    RngPolicy, SideId, SideInput,
};
use pkrs_data::{BattlePack, BattleRules, MoveCategory, MoveDef, MoveEffect, TypeChart};

fn resources(extra_moves: Vec<MoveDef>) -> BattleResources {
    let chart = TypeChart::from_parts(
        vec![
            "normal".into(),
            "fire".into(),
            "poison".into(),
            "steel".into(),
        ],
        Default::default(),
    );
    let mut pack = BattlePack::new(chart);
    pack.insert_move(MoveDef {
        id: "poison_powder".into(),
        type_id: "normal".into(),
        category: MoveCategory::Status,
        power: 0,
        accuracy: 100,
        priority: 0,
        effects: vec![MoveEffect::ApplyStatus {
            status: "poison".into(),
            chance: 100,
        }],
    })
    .expect("poison_powder");
    pack.insert_move(MoveDef {
        id: "pass".into(),
        type_id: "normal".into(),
        category: MoveCategory::Status,
        power: 0,
        accuracy: 100,
        priority: 0,
        effects: vec![],
    })
    .expect("pass");
    for mv in extra_moves {
        pack.insert_move(mv).expect("move");
    }
    BattleResources::with_rules(pack, BattleRules::gen3_defaults())
}

fn creature(id: &str, types: Vec<&str>, move_id: &str, max_hp: u32) -> BattleCreature {
    BattleCreature {
        species_id: id.into(),
        types: types.into_iter().map(String::from).collect(),
        nickname: id.into(),
        level: 5,
        stats: CreatureStats::new(max_hp, 10, 10, 10),
        moves: [
            Some(MoveSlot {
                id: move_id.into(),
                priority: 0,
                power: 0,
                pp: 10,
                max_pp: 10,
            }),
            None,
            None,
            None,
        ],
    }
}

fn pass_turn() -> BattleInput {
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

fn start_battle(
    res: BattleResources,
    seed: u64,
    player: BattleCreature,
    opponent: BattleCreature,
) -> (Battle, NullBattleHooks) {
    let setup = BattleSetup {
        player: BattleSideSetup {
            name: "P".into(),
            party: vec![player],
            leading: vec![Some(0)],
        },
        opponent: BattleSideSetup {
            name: "O".into(),
            party: vec![opponent],
            leading: vec![Some(0)],
        },
    };
    let mut hooks = NullBattleHooks;
    let mut battle = Battle::new(
        setup,
        BattleConfig::default(),
        res,
        RngPolicy::Fixed { seed },
        &mut hooks,
    )
    .expect("new");
    battle
        .step(BattleInput::default(), &mut hooks)
        .expect("start");
    (battle, hooks)
}

#[test]
fn poison_residual_is_one_eighth_max_hp() {
    let (mut battle, mut hooks) = start_battle(
        resources(vec![]),
        1,
        creature("a", vec!["normal"], "poison_powder", 100),
        creature("b", vec!["normal"], "pass", 100),
    );
    battle.step(pass_turn(), &mut hooks).expect("turn1 poison");
    let t2 = battle.step(pass_turn(), &mut hooks).expect("turn2");
    let residual: u32 = t2
        .iter()
        .filter_map(|e| match e {
            BattleEvent::Damage {
                amount,
                target_side: SideId::Opponent,
                ..
            } => Some(*amount),
            _ => None,
        })
        .sum();
    assert_eq!(residual, 12);
}

#[test]
fn major_status_does_not_stack() {
    let (mut battle, mut hooks) = start_battle(
        resources(vec![]),
        2,
        creature("a", vec!["normal"], "poison_powder", 100),
        creature("b", vec!["normal"], "pass", 100),
    );
    battle
        .step(
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
            ),
            &mut hooks,
        )
        .expect("turn1");
    let t2 = battle
        .step(
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
            ),
            &mut hooks,
        )
        .expect("turn2");
    let applied = t2
        .iter()
        .filter(|e| matches!(e, BattleEvent::StatusApplied { .. }))
        .count();
    assert_eq!(applied, 0);
}

#[test]
fn fire_type_cannot_be_burned() {
    let burn_move = MoveDef {
        id: "will_o_wisp".into(),
        type_id: "fire".into(),
        category: MoveCategory::Status,
        power: 0,
        accuracy: 100,
        priority: 0,
        effects: vec![MoveEffect::ApplyStatus {
            status: "burn".into(),
            chance: 100,
        }],
    };
    let (mut battle, mut hooks) = start_battle(
        resources(vec![burn_move]),
        3,
        creature("a", vec!["normal"], "will_o_wisp", 100),
        creature("b", vec!["fire"], "pass", 100),
    );
    let t1 = battle
        .step(
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
            ),
            &mut hooks,
        )
        .expect("turn1");
    assert!(
        !t1.iter()
            .any(|e| matches!(e, BattleEvent::StatusApplied { status, .. } if status == "burn"))
    );
}

#[test]
fn sleep_wakes_with_status_cured_event() {
    let sleep_move = MoveDef {
        id: "sleep_powder".into(),
        type_id: "normal".into(),
        category: MoveCategory::Status,
        power: 0,
        accuracy: 100,
        priority: 0,
        effects: vec![MoveEffect::ApplyStatus {
            status: "sleep".into(),
            chance: 100,
        }],
    };
    let (mut battle, mut hooks) = start_battle(
        resources(vec![sleep_move]),
        4,
        creature("a", vec!["normal"], "sleep_powder", 100),
        creature("b", vec!["normal"], "pass", 100),
    );
    battle
        .step(
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
            ),
            &mut hooks,
        )
        .expect("apply sleep");
    let mut cured = false;
    for _ in 0..8 {
        let events = battle.step(pass_turn(), &mut hooks).expect("pass turn");
        if events
            .iter()
            .any(|e| matches!(e, BattleEvent::StatusCured { .. }))
        {
            cured = true;
            break;
        }
    }
    assert!(cured, "sleep should end and emit StatusCured");
}

#[test]
fn residual_faint_prompts_replace() {
    let player = creature("a", vec!["normal"], "poison_powder", 100);
    let opponent = creature("b", vec!["normal"], "pass", 1);
    let bench = creature("c", vec!["normal"], "pass", 100);
    let setup = BattleSetup {
        player: BattleSideSetup {
            name: "P".into(),
            party: vec![player],
            leading: vec![Some(0)],
        },
        opponent: BattleSideSetup {
            name: "O".into(),
            party: vec![opponent, bench],
            leading: vec![Some(0)],
        },
    };
    let mut hooks = NullBattleHooks;
    let mut battle = Battle::new(
        setup,
        BattleConfig::default(),
        resources(vec![]),
        RngPolicy::Fixed { seed: 5 },
        &mut hooks,
    )
    .expect("new");
    battle
        .step(BattleInput::default(), &mut hooks)
        .expect("start");
    let events = battle
        .step(
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
            ),
            &mut hooks,
        )
        .expect("poison");
    assert!(
        events
            .iter()
            .any(|e| matches!(e, BattleEvent::Fainted { .. })),
        "expected faint from poison residual, got {events:?}"
    );
    assert!(
        matches!(battle.phase(), BattlePhase::Replace { .. }),
        "opponent should need replace after poison residual KO"
    );
}
