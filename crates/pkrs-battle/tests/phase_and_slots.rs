//! Phase transitions, ordering, and multi-slot setup.

use pkrs_battle::{
    Battle, BattleAction, BattleConfig, BattleCreature, BattleEvent, BattleInput, BattlePhase,
    BattleSetup, BattleSideSetup, CreatureStats, MoveSlot, NullBattleHooks, RngPolicy, SideId,
    SideInput,
};

fn creature(name: &str, hp: u32, spe: u32, priority: i8) -> BattleCreature {
    BattleCreature {
        species_id: name.into(),
        types: vec!["normal".into()],
        nickname: name.into(),
        level: 5,
        stats: CreatureStats::new(hp, 10, 10, spe),
        moves: [
            Some(MoveSlot {
                id: "tackle".into(),
                priority,
                power: 40,
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
fn start_reaches_select_actions() {
    let setup = pkrs_battle::battle_testkit::wild_1v1();
    let mut hooks = NullBattleHooks;
    let mut battle = Battle::new(
        setup,
        BattleConfig::default(),
        pkrs_battle::battle_testkit::default_resources(),
        RngPolicy::Fixed { seed: 0 },
        &mut hooks,
    )
    .expect("new");
    assert_eq!(battle.phase(), BattlePhase::Start);
    let events = battle
        .step(BattleInput::default(), &mut hooks)
        .expect("step");
    assert!(
        events
            .iter()
            .any(|e| matches!(e, BattleEvent::BattleStarted))
    );
    assert_eq!(battle.phase(), BattlePhase::SelectActions);
}

#[test]
fn side_size_two_accepts_two_actions_per_side() {
    let setup = BattleSetup {
        player: BattleSideSetup {
            name: "P".into(),
            party: vec![creature("a", 20, 10, 0), creature("b", 20, 5, 0)],
            leading: vec![Some(0), Some(1)],
        },
        opponent: BattleSideSetup {
            name: "O".into(),
            party: vec![creature("x", 20, 8, 0), creature("y", 20, 6, 0)],
            leading: vec![Some(0), Some(1)],
        },
    };
    let config = BattleConfig { side_size: 2 };
    let mut hooks = NullBattleHooks;
    let mut battle = Battle::new(
        setup,
        config,
        pkrs_battle::battle_testkit::default_resources(),
        RngPolicy::Fixed { seed: 0 },
        &mut hooks,
    )
    .expect("new");
    battle
        .step(BattleInput::default(), &mut hooks)
        .expect("start");
    let input = BattleInput::both(
        SideInput {
            slots: vec![
                BattleAction::Fight {
                    move_slot: 0,
                    target_side: SideId::Opponent,
                    target_slot: 0,
                },
                BattleAction::Fight {
                    move_slot: 0,
                    target_side: SideId::Opponent,
                    target_slot: 1,
                },
            ],
        },
        SideInput {
            slots: vec![
                BattleAction::Fight {
                    move_slot: 0,
                    target_side: SideId::Player,
                    target_slot: 0,
                },
                BattleAction::Fight {
                    move_slot: 0,
                    target_side: SideId::Player,
                    target_slot: 1,
                },
            ],
        },
    );
    let events = battle.step(input, &mut hooks).expect("turn");
    let move_uses = events
        .iter()
        .filter(|e| matches!(e, BattleEvent::MoveUsed { .. }))
        .count();
    assert_eq!(move_uses, 4);
    assert_eq!(battle.turn(), 2);
}
