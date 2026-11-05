//! Status and weather integration.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use pkrs_battle::{
    Battle, BattleAction, BattleConfig, BattleCreature, BattleEvent, BattleInput, BattleResources,
    BattleSetup, BattleSideSetup, CreatureStats, MoveSlot, NullBattleHooks, RngPolicy, SideId,
    SideInput,
};
use pkrs_data::{BattlePack, BattleRules, MoveCategory, MoveDef, MoveEffect, TypeChart};

fn resources() -> BattleResources {
    let chart = TypeChart::from_parts(vec!["normal".into()], Default::default());
    let mut pack = BattlePack::new(chart);
    pack.insert_move(MoveDef {
        id: "toxic".into(),
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
    .expect("toxic");
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
    BattleResources::with_rules(pack, BattleRules::gen3_defaults())
}

fn creature(id: &str, move_id: &str) -> BattleCreature {
    BattleCreature {
        species_id: id.into(),
        types: vec!["normal".into()],
        nickname: id.into(),
        level: 5,
        stats: CreatureStats::new(100, 10, 10, 10),
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

#[test]
fn poison_residual_damage_after_turn_end() {
    let setup = BattleSetup {
        player: BattleSideSetup {
            name: "P".into(),
            party: vec![creature("a", "toxic")],
            leading: vec![Some(0)],
        },
        opponent: BattleSideSetup {
            name: "O".into(),
            party: vec![creature("b", "pass")],
            leading: vec![Some(0)],
        },
    };
    let mut hooks = NullBattleHooks;
    let mut battle = Battle::new(
        setup,
        BattleConfig::default(),
        resources(),
        RngPolicy::Fixed { seed: 1 },
        &mut hooks,
    )
    .expect("new");
    battle
        .step(BattleInput::default(), &mut hooks)
        .expect("start");

    let turn = BattleInput::both(
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
    let t1 = battle.step(turn, &mut hooks).expect("turn1");
    assert!(
        t1.iter()
            .any(|e| matches!(e, BattleEvent::StatusApplied { status, .. } if status == "poison"))
    );

    let pass_turn = BattleInput::both(
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
    let t2 = battle.step(pass_turn, &mut hooks).expect("turn2");
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
    assert!(
        residual >= 12,
        "poison should chip at end of turn, got {residual}"
    );
}
