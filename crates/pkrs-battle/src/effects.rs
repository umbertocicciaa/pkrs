//! Move effect primitive execution (§7.3).

use pkrs_core::GameRng;
use pkrs_data::{BattleRules, EffectTarget, MoveEffect};

use crate::event::BattleEvent;
use crate::model::{ActiveCreature, SideId};
use crate::stat_stage::StatStages;
use crate::status::apply_status_to_creature;

/// Applies non-damage effects after a successful hit.
#[allow(clippy::too_many_arguments)]
pub fn apply_effects(
    effects: &[MoveEffect],
    user_side: SideId,
    user_slot: u8,
    target_side: SideId,
    target_slot: u8,
    damage_dealt: u32,
    target_fainted: bool,
    user_creature: &mut ActiveCreature,
    target_creature: &mut ActiveCreature,
    user_stages: &mut StatStages,
    target_stages: &mut StatStages,
    rules: &BattleRules,
    weather: &mut Option<(String, u8)>,
    rng: &mut dyn GameRng,
    out: &mut Vec<BattleEvent>,
) {
    for effect in effects {
        match effect {
            MoveEffect::Damage => {}
            MoveEffect::ApplyStatus { status, chance } => {
                if target_fainted {
                    continue;
                }
                let roll = rng.gen_range_u32(100);
                if roll < u32::from(*chance)
                    && apply_status_to_creature(target_creature, status, rules)
                {
                    out.push(BattleEvent::StatusApplied {
                        target_side,
                        target_slot,
                        status: status.clone(),
                    });
                }
            }
            MoveEffect::StatStage {
                stat,
                stages,
                target,
                chance,
            } => {
                let roll = rng.gen_range_u32(100);
                if roll >= u32::from(*chance) {
                    continue;
                }
                if *target == EffectTarget::User {
                    user_stages.apply(*stat, *stages);
                    out.push(BattleEvent::StatChanged {
                        side: user_side,
                        slot: user_slot,
                        stat: *stat,
                        stages: user_stages.stage_value(*stat),
                    });
                } else if !target_fainted {
                    target_stages.apply(*stat, *stages);
                    out.push(BattleEvent::StatChanged {
                        side: target_side,
                        slot: target_slot,
                        stat: *stat,
                        stages: target_stages.stage_value(*stat),
                    });
                }
            }
            MoveEffect::Heal { percent } => {
                let amount = heal_amount(user_creature, *percent);
                if amount > 0 {
                    out.push(BattleEvent::Heal {
                        side: user_side,
                        slot: user_slot,
                        amount,
                    });
                }
            }
            MoveEffect::Recoil { percent } => {
                let amount = damage_dealt.saturating_mul(u32::from(*percent)) / 100;
                if amount > 0 {
                    apply_damage_to(user_side, user_slot, user_creature, amount, out);
                }
            }
            MoveEffect::Drain { percent } => {
                let amount = damage_dealt.saturating_mul(u32::from(*percent)) / 100;
                if amount > 0 {
                    let healed = heal_amount_by(user_creature, amount);
                    if healed > 0 {
                        out.push(BattleEvent::Heal {
                            side: user_side,
                            slot: user_slot,
                            amount: healed,
                        });
                    }
                }
            }
            MoveEffect::MultiHit { min, max } => {
                let span = u32::from(max.saturating_sub(*min)) + 1;
                let extra = rng.gen_range_u32(span) + u32::from(*min);
                out.push(BattleEvent::Message {
                    key: format!("multi_hit:{extra}"),
                });
            }
            MoveEffect::Weather { weather: id } => {
                if let Some(def) = rules.weather(id) {
                    *weather = Some((id.clone(), def.default_turns));
                }
                out.push(BattleEvent::WeatherSet {
                    weather: id.clone(),
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

fn heal_amount(creature: &mut ActiveCreature, percent: u8) -> u32 {
    heal_amount_by(
        creature,
        creature.template.stats.max_hp * u32::from(percent) / 100,
    )
}

fn heal_amount_by(creature: &mut ActiveCreature, delta: u32) -> u32 {
    let before = creature.current_hp;
    creature.current_hp = creature
        .current_hp
        .saturating_add(delta)
        .min(creature.template.stats.max_hp);
    creature.current_hp.saturating_sub(before)
}

fn apply_damage_to(
    side: SideId,
    slot: u8,
    creature: &mut ActiveCreature,
    amount: u32,
    out: &mut Vec<BattleEvent>,
) {
    creature.current_hp = creature.current_hp.saturating_sub(amount);
    out.push(BattleEvent::Recoil { side, slot, amount });
}

#[cfg(test)]
mod tests {
    use super::*;
    use pkrs_core::Pcg64Rng;
    use pkrs_data::{BattleRules, MoveEffect, StatId};

    fn creature(hp: u32) -> ActiveCreature {
        use crate::model::{BattleCreature, CreatureStats};
        ActiveCreature::from_template(BattleCreature {
            species_id: "t".into(),
            types: vec!["normal".into()],
            nickname: "T".into(),
            level: 5,
            stats: CreatureStats::new(hp, 10, 10, 10),
            moves: [None, None, None, None],
        })
    }

    #[test]
    fn stat_stage_on_user_boosts_self() {
        let mut user = creature(40);
        let mut target = creature(40);
        let mut user_stages = StatStages::zero();
        let mut target_stages = StatStages::zero();
        let mut rng = Pcg64Rng::from_seed(0);
        let mut out = Vec::new();
        let rules = BattleRules::gen3_defaults();
        let mut weather = None;
        apply_effects(
            &[MoveEffect::StatStage {
                stat: StatId::Attack,
                stages: 1,
                target: EffectTarget::User,
                chance: 100,
            }],
            SideId::Player,
            0,
            SideId::Opponent,
            0,
            0,
            false,
            &mut user,
            &mut target,
            &mut user_stages,
            &mut target_stages,
            &rules,
            &mut weather,
            &mut rng,
            &mut out,
        );
        assert_eq!(user_stages.attack, 1);
        assert_eq!(target_stages.attack, 0);
        assert!(matches!(
            out[0],
            BattleEvent::StatChanged {
                side: SideId::Player,
                ..
            }
        ));
    }

    #[test]
    fn heal_restores_hp() {
        let mut user = creature(100);
        user.current_hp = 50;
        let mut target = creature(100);
        let mut user_stages = StatStages::zero();
        let mut target_stages = StatStages::zero();
        let mut rng = Pcg64Rng::from_seed(0);
        let mut out = Vec::new();
        let rules = BattleRules::gen3_defaults();
        let mut weather = None;
        apply_effects(
            &[MoveEffect::Heal { percent: 50 }],
            SideId::Player,
            0,
            SideId::Opponent,
            0,
            0,
            false,
            &mut user,
            &mut target,
            &mut user_stages,
            &mut target_stages,
            &rules,
            &mut weather,
            &mut rng,
            &mut out,
        );
        assert_eq!(user.current_hp, 100);
        assert_eq!(
            out[0],
            BattleEvent::Heal {
                side: SideId::Player,
                slot: 0,
                amount: 50,
            }
        );
    }
}
