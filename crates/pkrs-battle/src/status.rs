//! Status, weather, and end-of-turn residuals (§7.3).

use pkrs_core::GameRng;
use pkrs_data::{
    BattleRules, ResidualKind, StatusKind, residual_amount, status_blocked_by_types,
    weather_chip_blocked,
};

use crate::Battle;
use crate::error::Result;
use crate::event::BattleEvent;
use crate::hooks::{BattleHookCtx, BattleHooks, HookResult, StatusApplyRequest};
use crate::model::{ActiveCreature, MajorStatus, SideId, SlotIndex};

/// Applies a status id to a creature using the rules catalog.
pub fn apply_status_to_creature(
    creature: &mut ActiveCreature,
    status_id: &str,
    rules: &BattleRules,
    rng: &mut dyn GameRng,
) -> bool {
    let Some(def) = rules.status(status_id) else {
        return false;
    };
    if status_blocked_by_types(def, &creature.template.types) {
        return false;
    }
    match def.kind {
        StatusKind::Major => {
            if creature.major_status.is_some() {
                return false;
            }
            let counter = sleep_turns_on_apply(def, rng);
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

fn sleep_turns_on_apply(def: &pkrs_data::StatusDef, rng: &mut dyn GameRng) -> u8 {
    let Some((min, max)) = def.sleep_turns else {
        return 0;
    };
    let span = u32::from(max.saturating_sub(min)) + 1;
    min.saturating_add(rng.gen_range_u32(span) as u8)
}

/// Returns `true` when the creature cannot act this turn (events are appended to `out`).
pub fn blocks_move_attempt(
    creature: &mut ActiveCreature,
    side: SideId,
    slot: SlotIndex,
    rules: &BattleRules,
    rng: &mut dyn GameRng,
    out: &mut Vec<BattleEvent>,
) -> bool {
    let Some(major) = creature.major_status.as_ref() else {
        return false;
    };
    let status_id = major.id.clone();
    let Some(def) = rules.status(&status_id) else {
        return false;
    };

    if def.blocks_move {
        if status_id == "freeze" {
            if let Some(chance) = def.thaw_chance {
                if rng.gen_range_u32(100) < u32::from(chance) {
                    creature.major_status = None;
                    out.push(BattleEvent::StatusCured {
                        target_side: side,
                        target_slot: slot,
                        status: status_id,
                    });
                    return false;
                }
            }
        }
        out.push(BattleEvent::Message {
            key: "status:cant_move".into(),
        });
        return true;
    }

    if let Some(chance) = def.full_paralysis_chance {
        if rng.gen_range_u32(100) < u32::from(chance) {
            out.push(BattleEvent::Message {
                key: "status:cant_move".into(),
            });
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
            if let Some(denom) = def.chip_denom {
                for side in [SideId::Player, SideId::Opponent] {
                    for slot in 0..battle.config().side_size {
                        let Some(creature) = battle.creature(side, slot) else {
                            continue;
                        };
                        if creature.fainted() {
                            continue;
                        }
                        if weather_chip_blocked(def, &creature.template.types) {
                            continue;
                        }
                        let amount = residual_amount(
                            creature.template.stats.max_hp,
                            ResidualKind::MaxHpFraction { denom },
                        );
                        apply_residual_damage(
                            battle,
                            side,
                            slot,
                            amount,
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
            let Some(creature) = battle.creature(side, slot) else {
                continue;
            };
            if creature.fainted() {
                continue;
            }
            let amount = battle.creature(side, slot).and_then(|creature| {
                let major = creature.major_status.as_ref()?;
                let def = rules.status(&major.id)?;
                let residual = def.residual?;
                Some(residual_amount(creature.template.stats.max_hp, residual))
            });
            if let Some(amount) = amount {
                apply_residual_damage(battle, side, slot, amount, out, "status:residual")?;
            }
        }
    }

    let ended = battle.tick_weather();
    if ended {
        out.push(BattleEvent::WeatherEnded);
    }

    for side in [SideId::Player, SideId::Opponent] {
        for slot in 0..battle.config().side_size {
            if let Some(creature) = battle.creature_mut(side, slot) {
                tick_sleep(creature, side, slot, out);
                tick_volatiles(creature, side, slot, out);
            }
        }
    }

    Ok(())
}

fn apply_residual_damage(
    battle: &mut Battle,
    side: SideId,
    slot: SlotIndex,
    amount: u32,
    out: &mut Vec<BattleEvent>,
    source: &str,
) -> Result<()> {
    if amount == 0 {
        return Ok(());
    }
    let Some(creature) = battle.creature(side, slot) else {
        return Ok(());
    };
    if creature.fainted() {
        return Ok(());
    }
    out.push(BattleEvent::Message {
        key: format!("residual:{source}"),
    });
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

fn tick_sleep(
    creature: &mut ActiveCreature,
    side: SideId,
    slot: SlotIndex,
    out: &mut Vec<BattleEvent>,
) {
    let Some(major) = &mut creature.major_status else {
        return;
    };
    if major.counter == 0 {
        return;
    }
    major.counter = major.counter.saturating_sub(1);
    if major.counter == 0 {
        let status = major.id.clone();
        creature.major_status = None;
        out.push(BattleEvent::StatusCured {
            target_side: side,
            target_slot: slot,
            status,
        });
    }
}

fn tick_volatiles(
    creature: &mut ActiveCreature,
    side: SideId,
    slot: SlotIndex,
    out: &mut Vec<BattleEvent>,
) {
    let ids: Vec<String> = creature.volatiles.keys().cloned().collect();
    for id in ids {
        let Some(turns) = creature.volatiles.get_mut(&id) else {
            continue;
        };
        if *turns == 0 {
            continue;
        }
        *turns = turns.saturating_sub(1);
        if *turns == 0 {
            creature.volatiles.remove(&id);
            out.push(BattleEvent::VolatileEnded {
                target_side: side,
                target_slot: slot,
                volatile: id,
            });
        }
    }
}

/// Attempts to apply a status with type immunity and hook veto.
#[allow(clippy::too_many_arguments)]
pub fn try_apply_status(
    hooks: &mut dyn BattleHooks,
    ctx: &BattleHookCtx<'_>,
    target_side: SideId,
    target_slot: SlotIndex,
    target: &mut ActiveCreature,
    status_id: &str,
    rules: &BattleRules,
    rng: &mut dyn GameRng,
    out: &mut Vec<BattleEvent>,
) -> Result<bool> {
    let Some(def) = rules.status(status_id) else {
        return Ok(false);
    };
    if status_blocked_by_types(def, &target.template.types) {
        return Ok(false);
    }
    let req = StatusApplyRequest {
        target_side,
        target_slot,
        status_id: status_id.to_string(),
    };
    if hooks.on_status_apply(ctx, &req)? == HookResult::Cancel {
        return Ok(false);
    }
    if apply_status_to_creature(target, status_id, rules, rng) {
        out.push(BattleEvent::StatusApplied {
            target_side,
            target_slot,
            status: status_id.to_string(),
        });
        return Ok(true);
    }
    Ok(false)
}
