//! Move effect primitive execution (§7.3).

use pkrs_core::GameRng;
use pkrs_data::{MoveEffect, StatId};

use crate::event::BattleEvent;
use crate::model::SideId;
use crate::stat_stage::StatStages;

/// Applies non-damage effects after a successful hit.
#[allow(clippy::too_many_arguments)]
pub fn apply_effects(
    effects: &[MoveEffect],
    user_side: SideId,
    user_slot: u8,
    target_side: SideId,
    target_slot: u8,
    damage_dealt: u32,
    _user_stages: &mut StatStages,
    target_stages: &mut StatStages,
    rng: &mut dyn GameRng,
    out: &mut Vec<BattleEvent>,
) {
    for effect in effects {
        match effect {
            MoveEffect::Damage => {}
            MoveEffect::ApplyStatus { status, chance } => {
                let roll = rng.gen_range_u32(100);
                if roll < u32::from(*chance) {
                    out.push(BattleEvent::StatusApplied {
                        target_side,
                        target_slot,
                        status: status.clone(),
                    });
                }
            }
            MoveEffect::StatStage { stat, stages } => {
                target_stages.apply(*stat, *stages);
                out.push(BattleEvent::StatChanged {
                    side: target_side,
                    slot: target_slot,
                    stat: *stat,
                    stages: target_stages.stage_value(*stat),
                });
            }
            MoveEffect::Heal { percent } => {
                out.push(BattleEvent::Heal {
                    side: user_side,
                    slot: user_slot,
                    amount: u32::from(*percent),
                });
            }
            MoveEffect::Recoil { percent } => {
                let amount = damage_dealt.saturating_mul(u32::from(*percent)) / 100;
                if amount > 0 {
                    out.push(BattleEvent::Recoil {
                        side: user_side,
                        slot: user_slot,
                        amount,
                    });
                }
            }
            MoveEffect::Drain { percent } => {
                let amount = damage_dealt.saturating_mul(u32::from(*percent)) / 100;
                if amount > 0 {
                    out.push(BattleEvent::Heal {
                        side: user_side,
                        slot: user_slot,
                        amount,
                    });
                }
            }
            MoveEffect::MultiHit { min, max } => {
                let span = u32::from(max.saturating_sub(*min)) + 1;
                let extra = rng.gen_range_u32(span) + u32::from(*min);
                out.push(BattleEvent::Message {
                    key: format!("multi_hit:{extra}"),
                });
            }
            MoveEffect::Weather { weather } => {
                out.push(BattleEvent::WeatherSet {
                    weather: weather.clone(),
                });
            }
            MoveEffect::Protect => {
                out.push(BattleEvent::Message {
                    key: "protect".into(),
                });
            }
            MoveEffect::SwitchOut => {
                out.push(BattleEvent::Message {
                    key: "switch_out".into(),
                });
            }
        }
    }
}

impl StatStages {
    /// Current stage for `stat`.
    pub fn stage_value(&self, stat: StatId) -> i8 {
        match stat {
            StatId::Attack => self.attack,
            StatId::Defense => self.defense,
            StatId::Speed => self.speed,
        }
    }
}
