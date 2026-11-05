//! Integration: battle resolves moves loaded from RON fixtures.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use pkrs_battle::{
    Battle, BattleAction, BattleConfig, BattleCreature, BattleEvent, BattleInput, BattleResources,
    BattleSetup, BattleSideSetup, CreatureStats, MoveSlot, NullBattleHooks, RngPolicy, SideId,
    SideInput,
};
use pkrs_data::{BattlePack, MoveDef, SpeciesDef, TypeChart};

fn fixture_pack() -> BattlePack {
    let chart = TypeChart::parse_ron(
        include_str!("fixtures/types.ron"),
        "tests/fixtures/types.ron",
    )
    .expect("types");
    let mut pack = BattlePack::new(chart);
    pack.insert_move(
        MoveDef::parse_ron(include_str!("fixtures/ember.ron"), "ember.ron").expect("ember"),
    )
    .expect("insert ember");
    pack.insert_move(
        MoveDef::parse_ron(include_str!("fixtures/tackle.ron"), "tackle.ron").expect("tackle"),
    )
    .expect("insert tackle");
    pack.insert_species(
        SpeciesDef::parse_ron(include_str!("fixtures/splash.ron"), "splash.ron").expect("splash"),
    )
    .expect("insert splash");
    pack.insert_species(
        SpeciesDef::parse_ron(include_str!("fixtures/leafkit.ron"), "leafkit.ron")
            .expect("leafkit"),
    )
    .expect("insert leafkit");
    pack
}

fn setup_from_pack(pack: &BattlePack) -> BattleSetup {
    let splash = pack.species.get("splash").expect("splash");
    let leafkit = pack.species.get("leafkit").expect("leafkit");
    BattleSetup {
        player: BattleSideSetup {
            name: "Player".into(),
            party: vec![BattleCreature {
                species_id: splash.id.clone(),
                types: splash.types.clone(),
                nickname: "Hero".into(),
                level: 10,
                stats: CreatureStats::new(splash.hp, splash.attack, splash.defense, splash.speed),
                moves: [
                    Some(MoveSlot {
                        id: "ember".into(),
                        priority: 0,
                        power: 40,
                        pp: 25,
                        max_pp: 25,
                    }),
                    None,
                    None,
                    None,
                ],
            }],
            leading: vec![Some(0)],
        },
        opponent: BattleSideSetup {
            name: "Wild".into(),
            party: vec![BattleCreature {
                species_id: leafkit.id.clone(),
                types: leafkit.types.clone(),
                nickname: "Foe".into(),
                level: 10,
                stats: CreatureStats::new(
                    leafkit.hp,
                    leafkit.attack,
                    leafkit.defense,
                    leafkit.speed,
                ),
                moves: [
                    Some(MoveSlot {
                        id: "tackle".into(),
                        priority: 0,
                        power: 35,
                        pp: 35,
                        max_pp: 35,
                    }),
                    None,
                    None,
                    None,
                ],
            }],
            leading: vec![Some(0)],
        },
    }
}

#[test]
fn ron_loaded_ember_produces_damage_event() {
    let pack = fixture_pack();
    let setup = setup_from_pack(&pack);
    let resources = BattleResources::new(pack);
    let mut hooks = NullBattleHooks;
    let mut battle = Battle::new(
        setup,
        BattleConfig::default(),
        resources,
        RngPolicy::Fixed { seed: 42 },
        &mut hooks,
    )
    .expect("new");

    let empty = BattleInput::default();
    let _ = battle.step(empty.clone(), &mut hooks).expect("start");

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
    let events = battle.step(turn, &mut hooks).expect("turn");
    let ember_damage: Vec<u32> = events
        .iter()
        .filter_map(|e| match e {
            BattleEvent::Damage {
                target_side: SideId::Opponent,
                amount,
                ..
            } => Some(*amount),
            _ => None,
        })
        .collect();
    assert_eq!(ember_damage.first().copied(), Some(23), "pinned seed 42 ember vs grass");
    assert!(
        events
            .iter()
            .any(|e| matches!(e, BattleEvent::MoveUsed { move_id, .. } if move_id == "ember"))
    );
    assert!(
        events
            .iter()
            .any(|e| matches!(e, BattleEvent::Damage { .. }))
    );
}
