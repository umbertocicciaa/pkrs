//! Move target resolution for multi-active battles (§7.3 formats).

use pkrs_data::MoveTargetScope;

use crate::Battle;
use crate::{SideId, SlotRef};

/// Resolves which slots a fight action hits.
#[must_use]
pub fn fight_targets(
    battle: &Battle,
    user: SlotRef,
    move_id: &str,
    chosen: SlotRef,
) -> Vec<SlotRef> {
    let scope = battle
        .resources()
        .pack
        .move_by_id(move_id)
        .map(|m| m.targeting)
        .unwrap_or(MoveTargetScope::SingleFoe);
    match scope {
        MoveTargetScope::SingleFoe => vec![chosen],
        MoveTargetScope::AllFoes => {
            let view = battle.view();
            let foe = user.side.other();
            (0..battle.config().side_size)
                .filter_map(|slot| {
                    let slot_ref = SlotRef::new(foe, slot);
                    view.creature(slot_ref)
                        .filter(|c| c.hp > 0)
                        .map(|_| slot_ref)
                })
                .collect()
        }
    }
}

/// `true` when the player must pick a foe slot (`side_size > 1`, single-target move, >1 foe).
#[must_use]
pub fn needs_foe_target_ui(battle: &Battle, move_id: &str) -> bool {
    if battle.config().side_size <= 1 {
        return false;
    }
    let scope = battle
        .resources()
        .pack
        .move_by_id(move_id)
        .map(|m| m.targeting)
        .unwrap_or(MoveTargetScope::SingleFoe);
    if !matches!(scope, MoveTargetScope::SingleFoe) {
        return false;
    }
    let view = battle.view();
    let foe = SideId::Opponent;
    (0..battle.config().side_size)
        .filter(|&slot| {
            view.creature(SlotRef::new(foe, slot))
                .is_some_and(|c| c.hp > 0)
        })
        .count()
        > 1
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        BattleConfig, BattleCreature, BattleSetup, BattleSideSetup, CreatureStats, NullBattleHooks,
        RngPolicy, battle_testkit,
    };

    fn creature(name: &str, speed: u32) -> BattleCreature {
        BattleCreature {
            species_id: name.into(),
            types: vec!["normal".into()],
            nickname: name.into(),
            level: 5,
            stats: CreatureStats::new(40, 10, 10, speed),
            moves: Default::default(),
        }
    }

    fn doubles_setup() -> BattleSetup {
        BattleSetup {
            player: BattleSideSetup {
                name: "P".into(),
                party: vec![creature("a", 20), creature("b", 5)],
                leading: vec![Some(0), Some(1)],
            },
            opponent: BattleSideSetup {
                name: "O".into(),
                party: vec![creature("x", 8), creature("y", 6)],
                leading: vec![Some(0), Some(1)],
            },
        }
    }

    #[test]
    fn single_target_keeps_chosen_slot() {
        let mut hooks = NullBattleHooks;
        let battle = crate::Battle::new(
            doubles_setup(),
            BattleConfig::with_side_size(2),
            battle_testkit::default_resources(),
            RngPolicy::Fixed { seed: 0 },
            &mut hooks,
        )
        .unwrap();
        let user = SlotRef::new(SideId::Player, 0);
        let chosen = SlotRef::new(SideId::Opponent, 1);
        let id = "tackle";
        let targets = fight_targets(&battle, user, id, chosen);
        assert_eq!(targets, vec![chosen]);
    }

    #[test]
    fn spread_hits_both_foes() {
        let mut resources = battle_testkit::default_resources();
        resources
            .pack
            .moves
            .get_mut("tackle")
            .expect("tackle")
            .targeting = MoveTargetScope::AllFoes;
        let mut hooks = NullBattleHooks;
        let battle = crate::Battle::new(
            doubles_setup(),
            BattleConfig::with_side_size(2),
            resources,
            RngPolicy::Fixed { seed: 0 },
            &mut hooks,
        )
        .unwrap();
        let user = SlotRef::new(SideId::Player, 0);
        let chosen = SlotRef::new(SideId::Opponent, 0);
        let targets = fight_targets(&battle, user, "tackle", chosen);
        assert_eq!(
            targets,
            vec![
                SlotRef::new(SideId::Opponent, 0),
                SlotRef::new(SideId::Opponent, 1),
            ]
        );
    }
}
