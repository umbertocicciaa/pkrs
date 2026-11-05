//! Deterministic action ordering (§7.1).

use crate::action::BattleAction;
use crate::error::{BattleError, Result};
use crate::model::{ActiveCreature, SideId, SlotIndex};

/// One action waiting in the turn queue.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct QueuedAction {
    pub side: SideId,
    pub slot: SlotIndex,
    pub action: BattleAction,
    pub priority: i8,
    pub speed: u32,
}

/// Sort key: higher priority first, then speed, then stable tiebreak.
#[allow(dead_code)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct OrderKey {
    priority: i8,
    speed: u32,
    side: SideId,
    slot: SlotIndex,
}

impl OrderKey {
    fn for_action(
        side: SideId,
        slot: SlotIndex,
        action: &BattleAction,
        creature: &ActiveCreature,
    ) -> Self {
        let (priority, speed) = match action {
            BattleAction::Fight { move_slot, .. } => {
                let priority = creature
                    .template
                    .moves
                    .get(*move_slot as usize)
                    .and_then(|m| m.as_ref())
                    .map(|m| m.priority)
                    .unwrap_or(0);
                (priority, creature.speed())
            }
            BattleAction::Switch { .. } => (6, creature.speed()),
            BattleAction::Run => (6, u32::MAX),
            BattleAction::Item { .. } | BattleAction::Custom { .. } => (0, creature.speed()),
        };
        Self {
            priority,
            speed,
            side,
            slot,
        }
    }
}

/// Builds a priority/speed sorted queue from committed actions.
pub(crate) fn resolve_order<'a, F>(
    side_size: u8,
    player: &[(SlotIndex, BattleAction)],
    opponent: &[(SlotIndex, BattleAction)],
    active: F,
) -> Result<Vec<QueuedAction>>
where
    F: Fn(SideId, SlotIndex) -> Option<&'a ActiveCreature>,
{
    let mut entries = Vec::new();
    for (slot, action) in player {
        let creature = active(SideId::Player, *slot).ok_or_else(|| {
            BattleError::InvalidAction(format!("no active creature on player slot {slot}"))
        })?;
        if creature.fainted() {
            return Err(BattleError::InvalidAction(
                "fainted creature cannot act".into(),
            ));
        }
        let key = OrderKey::for_action(SideId::Player, *slot, action, creature);
        entries.push(QueuedAction {
            side: SideId::Player,
            slot: *slot,
            priority: key.priority,
            speed: key.speed,
            action: action.clone(),
        });
    }
    for (slot, action) in opponent {
        let creature = active(SideId::Opponent, *slot).ok_or_else(|| {
            BattleError::InvalidAction(format!("no active creature on opponent slot {slot}"))
        })?;
        if creature.fainted() {
            return Err(BattleError::InvalidAction(
                "fainted creature cannot act".into(),
            ));
        }
        let key = OrderKey::for_action(SideId::Opponent, *slot, action, creature);
        entries.push(QueuedAction {
            side: SideId::Opponent,
            slot: *slot,
            priority: key.priority,
            speed: key.speed,
            action: action.clone(),
        });
    }
    let _ = side_size;
    entries.sort_by(|a, b| {
        b.priority
            .cmp(&a.priority)
            .then(b.speed.cmp(&a.speed))
            .then(a.side.cmp(&b.side))
            .then(a.slot.cmp(&b.slot))
    });
    Ok(entries)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{BattleCreature, CreatureStats, MoveSlot};

    fn creature(speed: u32, priority: i8) -> ActiveCreature {
        ActiveCreature::from_template(BattleCreature {
            species_id: "a".into(),
            types: vec!["normal".into()],
            nickname: "A".into(),
            level: 5,
            stats: CreatureStats::new(20, 10, 10, speed),
            moves: [
                Some(MoveSlot {
                    id: "hit".into(),
                    priority,
                    power: 40,
                    pp: 10,
                    max_pp: 10,
                }),
                None,
                None,
                None,
            ],
        })
    }

    #[test]
    fn higher_priority_goes_first() {
        let fast = creature(10, 0);
        let slow_prio = creature(1, 1);
        let order = resolve_order(
            1,
            &[(
                0,
                BattleAction::Fight {
                    move_slot: 0,
                    target_side: SideId::Opponent,
                    target_slot: 0,
                },
            )],
            &[(
                0,
                BattleAction::Fight {
                    move_slot: 0,
                    target_side: SideId::Player,
                    target_slot: 0,
                },
            )],
            |side, slot| match (side, slot) {
                (SideId::Player, 0) => Some(&fast),
                (SideId::Opponent, 0) => Some(&slow_prio),
                _ => None,
            },
        )
        .expect("order");
        assert_eq!(order[0].side, SideId::Opponent);
    }

    #[test]
    fn speed_tiebreak_favors_player_side() {
        let p = creature(50, 0);
        let o = creature(50, 0);
        let order = resolve_order(
            1,
            &[(
                0,
                BattleAction::Fight {
                    move_slot: 0,
                    target_side: SideId::Opponent,
                    target_slot: 0,
                },
            )],
            &[(
                0,
                BattleAction::Fight {
                    move_slot: 0,
                    target_side: SideId::Player,
                    target_slot: 0,
                },
            )],
            |side, slot| match (side, slot) {
                (SideId::Player, 0) => Some(&p),
                (SideId::Opponent, 0) => Some(&o),
                _ => None,
            },
        )
        .expect("order");
        assert_eq!(order[0].side, SideId::Player);
    }
}
