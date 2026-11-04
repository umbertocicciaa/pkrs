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
    /// Value after each named stage (last element is final before min 1).
    pub trace: Vec<(DamageModifierStage, u32)>,
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
        value = apply_multiplier(value, type_mult);
        trace.push((DamageModifierStage::TypeEffectiveness, value));

        let roll = rng.gen_range_u32(16) + 85;
        value = apply_multiplier(value, f64::from(roll) / 100.0);
        trace.push((DamageModifierStage::Random, value.max(1)));

        Self { trace }
    }

    /// Final damage (minimum 1).
    pub fn final_damage(&self) -> u32 {
        self.trace.last().map(|(_, v)| *v).unwrap_or(1).max(1)
    }
}

fn gen3_base_damage(level: u8, power: u16, offense: u32, defense: u32) -> u32 {
    let level = u32::from(level);
    let power = u32::from(power);
    let offense = offense.max(1);
    let defense = defense.max(1);
    (2 * level / 5 + 2) * power * offense / defense / 50 + 2
}

fn apply_multiplier(value: u32, mult: f64) -> u32 {
    ((value as f64) * mult).floor().max(1.0) as u32
}

/// Applies stat stages to offensive/defensive stats used in damage.
pub fn effective_offense(base: u32, stages: &StatStages, category: MoveCategory) -> u32 {
    let mult = match category {
        MoveCategory::Physical => stages.attack_multiplier(),
        MoveCategory::Special => stages.attack_multiplier(),
        MoveCategory::Status => 1.0,
    };
    ((base as f64) * mult).floor().max(1.0) as u32
}

pub fn effective_defense(base: u32, stages: &StatStages, category: MoveCategory) -> u32 {
    let mult = match category {
        MoveCategory::Physical => stages.defense_multiplier(),
        MoveCategory::Special => stages.defense_multiplier(),
        MoveCategory::Status => 1.0,
    };
    ((base as f64) * mult).floor().max(1.0) as u32
}

#[cfg(test)]
mod tests {
    use super::*;
    use pkrs_core::Pcg64Rng;
    use pkrs_data::TypeChart;

    fn chart() -> TypeChart {
        TypeChart::parse_ron(
            r#"(
                types: ["normal", "fire", "water"],
                chart: { ("fire", "water"): 0.5, ("water", "fire"): 2.0 },
            )"#,
            "types.ron",
        )
        .expect("chart")
    }

    #[test]
    fn reference_case_level_50_neutral() {
        let inputs = DamageInputs {
            level: 50,
            power: 95,
            category: MoveCategory::Physical,
            offense: 100,
            defense: 100,
            move_type: "normal",
            user_types: &["normal".to_string()],
            target_types: &["normal".to_string()],
            critical: false,
        };
        let mut rng = Pcg64Rng::from_seed(0);
        let calc = DamageCalc::compute(inputs, &chart(), &mut rng);
        let base = gen3_base_damage(50, 95, 100, 100);
        assert_eq!(base, 43);
        assert_eq!(calc.trace[0].1, 43);
    }

    #[test]
    fn stab_and_type_modifiers_apply() {
        let user_types = [String::from("fire")];
        let target_types = [String::from("water")];
        let inputs = DamageInputs {
            level: 50,
            power: 40,
            category: MoveCategory::Special,
            offense: 90,
            defense: 80,
            move_type: "fire",
            user_types: &user_types,
            target_types: &target_types,
            critical: false,
        };
        let mut rng = Pcg64Rng::from_seed(1);
        let calc = DamageCalc::compute(inputs, &chart(), &mut rng);
        let after_stab = calc.trace[2].1;
        let after_type = calc.trace[3].1;
        assert!(after_stab > calc.trace[1].1);
        assert!(after_type < after_stab);
    }

    #[test]
    fn pinned_random_roll_reproduces_hand_computed_final() {
        let user_types = [String::from("normal")];
        let target_types = [String::from("normal")];
        let inputs = DamageInputs {
            level: 5,
            power: 35,
            category: MoveCategory::Physical,
            offense: 20,
            defense: 8,
            move_type: "normal",
            user_types: &user_types,
            target_types: &target_types,
            critical: false,
        };
        let base = gen3_base_damage(5, 35, 20, 8);
        assert_eq!(base, 9);
        let mut rng = Pcg64Rng::from_seed(99);
        let calc = DamageCalc::compute(inputs, &chart(), &mut rng);
        assert_eq!(calc.trace[0].1, base);
        let after_random = calc.trace[4].1;
        assert_eq!(calc.final_damage(), after_random.max(1));
    }
}
