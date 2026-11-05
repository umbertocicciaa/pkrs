//! Switching clears per-slot stat stages (forced replace path).

#![allow(clippy::expect_used, clippy::unwrap_used)]

use pkrs_battle::{
    Battle, BattleAction, BattleConfig, BattleCreature, BattleEvent, BattleInput, BattlePhase,
    BattleResources, BattleSetup, BattleSideSetup, CreatureStats, MoveSlot, NullBattleHooks,
    RngPolicy, SideId, SideInput,
};
use pkrs_data::{BattlePack, MoveCategory, MoveDef, MoveEffect, TypeChart};

fn resources() -> BattleResources {
    let chart = TypeChart::from_parts(vec!["normal".into()], std::collections::HashMap::new());
    let mut pack = BattlePack::new(chart);
    pack.insert_move(MoveDef {
        id: "growth".into(),
        type_id: "normal".into(),
        category: MoveCategory::Status,
        power: 0,
        accuracy: 0,
        priority: 0,
        effects: vec![MoveEffect::StatStage {
            stat: pkrs_data::StatId::Attack,
            stages: 1,
            target: pkrs_data::EffectTarget::User,
            chance: 100,
        }],
    })
    .expect("growth");
    pack.insert_move(MoveDef {
        id: "tackle".into(),
        type_id: "normal".into(),
        category: MoveCategory::Physical,
        power: 200,
        accuracy: 100,
        priority: 0,
        effects: vec![MoveEffect::Damage],
    })
    .expect("tackle");
    BattleResources::new(pack)
}

fn growth_stage(events: &[BattleEvent]) -> Option<i8> {
    events.iter().rev().find_map(|e| match e {
        BattleEvent::StatChanged {
            side: SideId::Player,
            stages,
            ..
        } => Some(*stages),
        _ => None,
    })
}

#[test]
fn replace_switch_resets_slot_stat_stages() {
    let player = BattleSideSetup {
        name: "P".into(),
        party: vec![
            BattleCreature {
                species_id: "a".into(),
                types: vec!["normal".into()],
                nickname: "a".into(),
                level: 5,
                stats: CreatureStats::new(1, 20, 10, 30),
                moves: [
                    Some(MoveSlot {
                        id: "growth".into(),
                        priority: 0,
                        power: 0,
                        pp: 10,
                        max_pp: 10,
                    }),
                    None,
                    None,
                    None,
                ],
            },
            BattleCreature {
                species_id: "b".into(),
                types: vec!["normal".into()],
                nickname: "b".into(),
                level: 5,
                // Enough HP to survive the faster foe's hit and act (fainted creatures drop
                // their queued action).
                stats: CreatureStats::new(400, 20, 10, 5),
                moves: [
                    Some(MoveSlot {
                        id: "growth".into(),
                        priority: 0,
                        power: 0,
                        pp: 10,
                        max_pp: 10,
                    }),
                    None,
                    None,
                    None,
                ],
            },
        ],
        leading: vec![Some(0)],
    };
    let opponent = BattleSideSetup {
        name: "O".into(),
        party: vec![BattleCreature {
            species_id: "o".into(),
            types: vec!["normal".into()],
            nickname: "o".into(),
            level: 5,
            stats: CreatureStats::new(40, 20, 10, 20),
            moves: [
                Some(MoveSlot {
                    id: "tackle".into(),
                    priority: 0,
                    power: 200,
                    pp: 10,
                    max_pp: 10,
                }),
                None,
                None,
                None,
            ],
        }],
        leading: vec![Some(0)],
    };
    let mut hooks = NullBattleHooks;
    let mut battle = Battle::new(
        BattleSetup { player, opponent },
        BattleConfig::default(),
        resources(),
        RngPolicy::Fixed { seed: 0 },
        &mut hooks,
    )
    .expect("new");
    battle
        .step(BattleInput::default(), &mut hooks)
        .expect("start");

    let turn1 = BattleInput::both(
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
    );
    let t1 = battle.step(turn1, &mut hooks).expect("turn1");
    assert_eq!(growth_stage(&t1), Some(1));
    assert!(matches!(battle.phase(), BattlePhase::Replace { .. }));

    let replace = BattleInput::switch(SideId::Player, 1);
    let t2 = battle.step(replace, &mut hooks).expect("replace");
    assert!(t2.iter().any(|e| matches!(e, BattleEvent::Switched { .. })));

    let turn2 = BattleInput::both(
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
    );
    let t3 = battle.step(turn2, &mut hooks).expect("turn2");
    assert_eq!(growth_stage(&t3), Some(1));
}
