//! Accuracy checks (§7.3).

use pkrs_core::GameRng;

use crate::stat_stage::StatStages;

/// Context for [`BattleHooks::on_accuracy`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AccuracyCalc {
    /// Move accuracy 0–100 (`0` = always hits).
    pub move_accuracy: u8,
    /// Effective accuracy after stages (0–100 scale).
    pub effective: f64,
    /// Whether the hit succeeds.
    pub hits: bool,
}

/// Rolls whether a move hits (Gen-3-style stage modifiers on acc/evasion).
pub fn roll_accuracy(
    move_accuracy: u8,
    attacker: &StatStages,
    defender: &StatStages,
    rng: &mut dyn GameRng,
) -> AccuracyCalc {
    if move_accuracy == 0 {
        return AccuracyCalc {
            move_accuracy,
            effective: 100.0,
            hits: true,
        };
    }
    let acc_mult = StatStages::multiplier(attacker.attack);
    let eva_mult = StatStages::multiplier(defender.defense);
    let effective = (f64::from(move_accuracy) * acc_mult / eva_mult).clamp(1.0, 100.0);
    let roll = rng.gen_range_u32(100) + 1;
    let hits = f64::from(roll) <= effective;
    AccuracyCalc {
        move_accuracy,
        effective,
        hits,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pkrs_core::Pcg64Rng;

    #[test]
    fn zero_accuracy_always_hits() {
        let mut rng = Pcg64Rng::from_seed(0);
        let calc = roll_accuracy(0, &StatStages::zero(), &StatStages::zero(), &mut rng);
        assert!(calc.hits);
    }

    #[test]
    fn full_accuracy_usually_hits() {
        let mut rng = Pcg64Rng::from_seed(1);
        let calc = roll_accuracy(100, &StatStages::zero(), &StatStages::zero(), &mut rng);
        assert!(calc.hits);
    }
}
