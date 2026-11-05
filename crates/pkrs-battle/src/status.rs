//! Status, weather, and end-of-turn residuals (§7.3).

use pkrs_data::{BattleRules, StatusKind};

use crate::Battle;
use crate::error::Result;
use crate::event::BattleEvent;
use crate::hooks::BattleHooks;
use crate::model::{ActiveCreature, MajorStatus, SideId, SlotIndex};

/// Applies a status id to a creature using the rules catalog.
pub fn apply_status_to_creature(
    creature: &mut ActiveCreature,
    status_id: &str,
    rules: &BattleRules,
) -> bool {
    let Some(def) = rules.status(status_id) else {
        return false;
    };
    match def.kind {
        StatusKind::Major => {
            if creature.major_status.is_some() {
                return false;
            }
            let counter = if status_id == "sleep" { 2 } else { 0 };
            creature.major_status = Some(MajorStatus {
                id: status_id.to_string(),
                counter,
            });
            true
        }
        StatusKind::Volatile => {
            let turns = def.volatile_turns.unwrap_or(0);
            creature.volatiles.insert(status_id.to_string(), turns);
            true
        }
    }
}

/// Returns `true` when the creature cannot act (sleep/freeze/full paralysis).
pub fn blocks_move(creature: &ActiveCreature, rules: &BattleRules, para_roll: u32) -> bool {
    let Some(major) = &creature.major_status else {
        return false;
    };
    let Some(def) = rules.status(&major.id) else {
        return false;
    };
    if def.blocks_move {
        return true;
    }
    if let Some(chance) = def.full_paralysis_chance {
        if para_roll < u32::from(chance) {
            return true;
        }
    }
    false
}

/// Runs end-of-turn residuals in a stable order: weather chip → status residuals → timers.
pub fn process_end_of_turn(
    battle: &mut Battle,
    hooks: &mut dyn BattleHooks,
    out: &mut Vec<BattleEvent>,
) -> Result<()> {
    hooks.on_end_of_turn(&battle.hook_ctx(out))?;

    let rules = battle.rules().clone();
    let weather = battle.weather_snapshot();
    if let Some((weather_id, _)) = &weather {
        if let Some(def) = rules.weather(weather_id) {
            if let Some(percent) = def.chip_percent {
                for side in [SideId::Player, SideId::Opponent] {
                    for slot in 0..battle.config().side_size {
                        apply_chip(
                            battle,
                            side,
                            slot,
                            percent,
                            out,
                            &format!("weather:{weather_id}"),
                        )?;
                    }
                }
            }
        }
    }

    for side in [SideId::Player, SideId::Opponent] {
        for slot in 0..battle.config().side_size {
            let residual = battle.creature(side, slot).and_then(|creature| {
                let major = creature.major_status.as_ref()?;
                let def = rules.status(&major.id)?;
                match def.residual? {
                    pkrs_data::ResidualKind::MaxHpPercent { percent } => Some(
                        creature
                            .template
                            .stats
                            .max_hp
                            .saturating_mul(u32::from(percent))
                            / 100,
                    ),
                }
            });
            if let Some(amount) = residual {
                if amount > 0 {
                    let remaining = battle.apply_damage(side, slot, amount)?;
                    out.push(BattleEvent::Damage {
                        target_side: side,
                        target_slot: slot,
                        amount,
                        remaining_hp: remaining,
                    });
                    if remaining == 0 {
                        faint_if_needed(battle, side, slot, out);
                    }
                }
            }
        }
    }

    battle.tick_weather();
    for side in [SideId::Player, SideId::Opponent] {
        for slot in 0..battle.config().side_size {
            if let Some(creature) = battle.creature_mut(side, slot) {
                tick_sleep(creature);
                tick_volatiles(creature);
            }
        }
    }

    Ok(())
}

fn apply_chip(
    battle: &mut Battle,
    side: SideId,
    slot: SlotIndex,
    percent: u8,
    out: &mut Vec<BattleEvent>,
    _key: &str,
) -> Result<()> {
    let Some(creature) = battle.creature(side, slot) else {
        return Ok(());
    };
    if creature.fainted() {
        return Ok(());
    }
    let mut amount = creature
        .template
        .stats
        .max_hp
        .saturating_mul(u32::from(percent))
        / 100;
    if amount == 0 {
        amount = 1;
    }
    let remaining = battle.apply_damage(side, slot, amount)?;
    out.push(BattleEvent::Damage {
        target_side: side,
        target_slot: slot,
        amount,
        remaining_hp: remaining,
    });
    if remaining == 0 {
        faint_if_needed(battle, side, slot, out);
    }
    Ok(())
}

fn faint_if_needed(battle: &Battle, side: SideId, slot: SlotIndex, out: &mut Vec<BattleEvent>) {
    if let Some(party_index) = battle.party_index(side, slot) {
        out.push(BattleEvent::Fainted {
            side,
            slot,
            party_index,
        });
    }
}

fn tick_sleep(creature: &mut ActiveCreature) {
    let Some(major) = &mut creature.major_status else {
        return;
    };
    if major.id != "sleep" || major.counter == 0 {
        return;
    }
    major.counter = major.counter.saturating_sub(1);
    if major.counter == 0 {
        creature.major_status = None;
    }
}

fn tick_volatiles(creature: &mut ActiveCreature) {
    creature.volatiles.retain(|_, turns| {
        if *turns == 0 {
            return true;
        }
        *turns = turns.saturating_sub(1);
        *turns > 0
    });
}
