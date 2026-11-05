//! Shared content for the AI tests: five types, a handful of moves and a creature builder.

#![allow(dead_code, clippy::unwrap_used, clippy::expect_used)]

use std::collections::HashMap;

use pkrs_battle::{
    Battle, BattleConfig, BattleCreature, BattleInput, BattlePhase, BattleResources, BattleSetup,
    BattleSideSetup, CreatureStats, MoveSlot, NullBattleHooks, RngPolicy,
};
use pkrs_data::{BattlePack, MoveCategory, MoveDef, MoveEffect, TypeChart};

pub fn resources() -> BattleResources {
    let types = ["normal", "fire", "water", "grass", "ghost"]
        .map(String::from)
        .to_vec();
    let mut eff = HashMap::new();
    for (atk, def, m) in [
        ("fire", "grass", 2.0),
        ("fire", "water", 0.5),
        ("fire", "fire", 0.5),
        ("water", "fire", 2.0),
        ("water", "grass", 0.5),
        ("water", "water", 0.5),
        ("grass", "water", 2.0),
        ("grass", "fire", 0.5),
        ("grass", "grass", 0.5),
        ("normal", "ghost", 0.0),
    ] {
        eff.insert((atk.to_string(), def.to_string()), m);
    }
    let mut pack = BattlePack::new(TypeChart::from_parts(types, eff));
    let mk = |id: &str, ty: &str, category, power, priority| MoveDef {
        id: id.into(),
        type_id: ty.into(),
        category,
        power,
        accuracy: if category == MoveCategory::Status {
            0
        } else {
            100
        },
        priority,
        effects: vec![MoveEffect::Damage],
    };
    for def in [
        mk("tackle", "normal", MoveCategory::Physical, 40, 0),
        mk("quick_attack", "normal", MoveCategory::Physical, 40, 1),
        mk("ember", "fire", MoveCategory::Special, 40, 0),
        mk("water_gun", "water", MoveCategory::Special, 40, 0),
        mk("vine_whip", "grass", MoveCategory::Physical, 45, 0),
        mk("growl", "normal", MoveCategory::Status, 0, 0),
    ] {
        pack.insert_move(def).unwrap();
    }
    BattleResources::new(pack)
}

pub fn creature(name: &str, ty: &str, hp: u32, speed: u32, moves: &[&str]) -> BattleCreature {
    let mut slots: [Option<MoveSlot>; 4] = [None, None, None, None];
    for (i, id) in moves.iter().enumerate() {
        slots[i] = Some(MoveSlot {
            id: (*id).into(),
            priority: 0,
            power: 40,
            pp: 20,
            max_pp: 20,
        });
    }
    BattleCreature {
        species_id: name.into(),
        types: vec![ty.into()],
        nickname: name.into(),
        level: 20,
        stats: CreatureStats::new(hp, 30, 25, speed),
        moves: slots,
    }
}

pub fn side(name: &str, party: Vec<BattleCreature>) -> BattleSideSetup {
    BattleSideSetup {
        name: name.into(),
        party,
        leading: vec![Some(0)],
    }
}

/// A battle advanced to its first `SelectActions`.
pub fn battle(player: Vec<BattleCreature>, opponent: Vec<BattleCreature>, seed: u64) -> Battle {
    let mut hooks = NullBattleHooks;
    let mut b = Battle::new(
        BattleSetup {
            player: side("P", player),
            opponent: side("O", opponent),
        },
        BattleConfig::default(),
        resources(),
        RngPolicy::Fixed { seed },
        &mut hooks,
    )
    .unwrap();
    b.step(BattleInput::default(), &mut hooks).unwrap();
    assert_eq!(b.phase(), BattlePhase::SelectActions);
    b
}
