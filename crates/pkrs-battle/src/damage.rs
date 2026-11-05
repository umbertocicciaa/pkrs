//! Gen-3 damage pipeline with named modifier stages (§7.3).

use pkrs_core::GameRng;
use pkrs_data::{MoveCategory, TypeChart};

use crate::stat_stage::StatStages;

/// One modifier stage in the damage pipeline (hook / debug surface).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DamageModifierStage {
    /// Base damage before multipliers.
    Base,
    /// Critical hit multiplier.
    Critical,
    /// Same-type attack bonus.
    Stab,
    /// Type chart product.
    TypeEffectiveness,
    /// Random 85–100% roll (Gen 3).
    Random,
    /// A multiplier pushed by a hook ([`DamageCalc::push_modifier`]).
    Hook,
    /// A final value set by a hook ([`DamageCalc::set_final`]).
    Override,
}

/// Inputs to the physical/special damage formula.
#[derive(Clone, Debug, PartialEq)]
pub struct DamageInputs<'a> {
    /// Attacker level.
    pub level: u8,
    /// Move base power.
    pub power: u16,
    /// Move category.
    pub category: MoveCategory,
    /// Attacker offensive stat (Atk or SpA).
    pub offense: u32,
    /// Defender defensive stat (Def or SpD).
    pub defense: u32,
    /// Move elemental type id.
    pub move_type: &'a str,
    /// Attacker types for STAB.
    pub user_types: &'a [String],
    /// Defender types for effectiveness.
    pub target_types: &'a [String],
    /// Whether this hit is a critical.
    pub critical: bool,
}

/// Mutable damage calculation trace (§7.4 `on_damage_calc`).
#[derive(Clone, Debug, PartialEq)]
pub struct DamageCalc {
    /// Value after each named stage (last element is final before min-1 clamp).
    pub trace: Vec<(DamageModifierStage, u32)>,
    /// Names and multipliers of hook modifiers, in the order of their
    /// [`DamageModifierStage::Hook`] trace entries.
    pub hook_modifiers: Vec<(String, f64)>,
}

impl DamageCalc {
    /// Runs the default Gen-3 pipeline.
    pub fn compute(
        inputs: DamageInputs<'_>,
        type_chart: &TypeChart,
        rng: &mut dyn GameRng,
    ) -> Self {
        let mut trace = Vec::with_capacity(5);
        let base = gen3_base_damage(inputs.level, inputs.power, inputs.offense, inputs.defense);
        trace.push((DamageModifierStage::Base, base));

        let mut value = base;
        let crit_mult = if inputs.critical { 2.0 } else { 1.0 };
        value = apply_multiplier(value, crit_mult);
        trace.push((DamageModifierStage::Critical, value));

        let stab = if inputs.user_types.iter().any(|t| t == inputs.move_type) {
            1.5
        } else {
            1.0
        };
        value = apply_multiplier(value, stab);
        trace.push((DamageModifierStage::Stab, value));

        let type_mult = type_chart.combined_multiplier(inputs.move_type, inputs.target_types);
        if type_mult == 0.0 {
            trace.push((DamageModifierStage::TypeEffectiveness, 0));
            return Self {
                trace,
                hook_modifiers: Vec::new(),
            };
        }
        value = apply_multiplier(value, type_mult);
        trace.push((DamageModifierStage::TypeEffectiveness, value));

        let roll = rng.gen_range_u32(16) + 85;
        value = apply_multiplier(value, f64::from(roll) / 100.0);
        trace.push((DamageModifierStage::Random, value));

        Self {
            trace,
            hook_modifiers: Vec::new(),
        }
    }

    /// Current value (last stage), before the min-1 clamp.
    #[must_use]
    pub fn current(&self) -> u32 {
        self.trace.last().map_or(0, |(_, v)| *v)
    }

    /// `true` when the type chart made the target immune.
    #[must_use]
    pub fn is_immune(&self) -> bool {
        self.trace
            .iter()
            .any(|(stage, v)| *stage == DamageModifierStage::TypeEffectiveness && *v == 0)
    }

    /// Applies a named multiplier (e.g. `("resonance", 1.5)`) to the current value (§9.5).
    /// Negative multipliers count as `0`; non-finite ones are ignored (`1`).
    pub fn push_modifier(&mut self, name: impl Into<String>, multiplier: f64) {
        let mult = if multiplier.is_finite() {
            multiplier.max(0.0)
        } else {
            1.0
        };
        let value = apply_multiplier(self.current(), mult);
        self.hook_modifiers.push((name.into(), mult));
        self.trace.push((DamageModifierStage::Hook, value));
    }

    /// Replaces the final damage with `value`.
    pub fn set_final(&mut self, value: u32) {
        self.trace.push((DamageModifierStage::Override, value));
    }

    /// Final damage (0 on immunity; otherwise minimum 1 when non-zero).
    pub fn final_damage(&self) -> u32 {
        let raw = self.trace.last().map(|(_, v)| *v).unwrap_or(0);
        if raw == 0 { 0 } else { raw.max(1) }
    }
}

pub(crate) fn gen3_base_damage(level: u8, power: u16, offense: u32, defense: u32) -> u32 {
    let level = u64::from(level);
    let power = u64::from(power);
    let offense = u64::from(offense.max(1));
    let defense = u64::from(defense.max(1));
    let inner = (2 * level / 5 + 2)
        .saturating_mul(power)
        .saturating_mul(offense)
        / defense
        / 50
        + 2;
    u32::try_from(inner).unwrap_or(u32::MAX)
}

fn apply_multiplier(value: u32, mult: f64) -> u32 {
    (f64::from(value) * mult).floor() as u32
}

/// Applies stat stages to offensive stats used in damage.
pub fn effective_offense(base: u32, stages: &StatStages, category: MoveCategory) -> u32 {
    if matches!(category, MoveCategory::Status) {
        return base.max(1);
    }
    scale_stat(base, stages.attack_multiplier())
}

/// Applies stat stages to defensive stats used in damage.
pub fn effective_defense(base: u32, stages: &StatStages, category: MoveCategory) -> u32 {
    if matches!(category, MoveCategory::Status) {
        return base.max(1);
    }
    scale_stat(base, stages.defense_multiplier())
}

fn scale_stat(base: u32, mult: f64) -> u32 {
    let scaled = (f64::from(base.max(1)) * mult).floor();
    u32::try_from(scaled as u64).unwrap_or(u32::MAX).max(1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pkrs_core::Pcg64Rng;
    use pkrs_data::TypeChart;

    fn chart() -> TypeChart {
        TypeChart::parse_ron(
            r#"(
                types: ["normal", "fire", "water", "ghost"],
                chart: {
                    ("fire", "water"): 0.5,
                    ("water", "fire"): 2.0,
                    ("normal", "ghost"): 0.0,
                },
            )"#,
            "types.ron",
        )
        .expect("chart")
    }

    #[test]
    fn immunity_deals_zero_damage() {
        let user_types = [String::from("normal")];
        let target_types = [String::from("ghost")];
        let inputs = DamageInputs {
            level: 50,
            power: 80,
            category: MoveCategory::Physical,
            offense: 100,
            defense: 100,
            move_type: "normal",
            user_types: &user_types,
            target_types: &target_types,
            critical: false,
        };
        let mut rng = Pcg64Rng::from_seed(0);
        let calc = DamageCalc::compute(inputs, &chart(), &mut rng);
        assert_eq!(calc.trace[3].1, 0);
        assert_eq!(calc.final_damage(), 0);
    }

    #[test]
    fn reference_table_with_pinned_random_roll() {
        struct Row {
            level: u8,
            power: u16,
            offense: u32,
            defense: u32,
            move_type: &'static str,
            user_types: &'static [&'static str],
            target_types: &'static [&'static str],
            critical: bool,
            seed: u64,
            expected: u32,
        }
        let rows = [
            Row {
                level: 50,
                power: 95,
                offense: 100,
                defense: 100,
                move_type: "normal",
                user_types: &["normal"],
                target_types: &["normal"],
                critical: false,
                seed: 0,
                expected: 56,
            },
            Row {
                level: 5,
                power: 35,
                offense: 20,
                defense: 8,
                move_type: "normal",
                user_types: &["normal"],
                target_types: &["normal"],
                critical: false,
                seed: 99,
                expected: 13,
            },
        ];
        for row in rows {
            let user: Vec<String> = row.user_types.iter().map(|s| (*s).to_string()).collect();
            let target: Vec<String> = row.target_types.iter().map(|s| (*s).to_string()).collect();
            let inputs = DamageInputs {
                level: row.level,
                power: row.power,
                category: MoveCategory::Physical,
                offense: row.offense,
                defense: row.defense,
                move_type: row.move_type,
                user_types: &user,
                target_types: &target,
                critical: row.critical,
            };
            let mut rng = Pcg64Rng::from_seed(row.seed);
            let calc = DamageCalc::compute(inputs, &chart(), &mut rng);
            assert_eq!(calc.final_damage(), row.expected, "seed {}", row.seed);
        }
    }
}
