//! Stat stage multipliers (-6..=6, §7.3).

/// Which battle stat stages can be modified.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub struct StatStages {
    /// Attack stage.
    pub attack: i8,
    /// Defense stage.
    pub defense: i8,
    /// Speed stage.
    pub speed: i8,
}

impl StatStages {
    /// All stages at 0.
    pub const fn zero() -> Self {
        Self {
            attack: 0,
            defense: 0,
            speed: 0,
        }
    }

    /// Applies a delta, clamped to `[-6, 6]`.
    pub fn apply(&mut self, stat: pkrs_data::StatId, delta: i8) {
        let slot = match stat {
            pkrs_data::StatId::Attack => &mut self.attack,
            pkrs_data::StatId::Defense => &mut self.defense,
            pkrs_data::StatId::Speed => &mut self.speed,
        };
        *slot = (*slot + delta).clamp(-6, 6);
    }

    /// Gen-3-style multiplier for a stage value.
    pub fn multiplier(stage: i8) -> f64 {
        let stage = stage.clamp(-6, 6);
        if stage >= 0 {
            (2 + stage) as f64 / 2.0
        } else {
            2.0 / (2 - stage) as f64
        }
    }

    /// Attack multiplier from current stages.
    pub fn attack_multiplier(&self) -> f64 {
        Self::multiplier(self.attack)
    }

    /// Defense multiplier from current stages.
    pub fn defense_multiplier(&self) -> f64 {
        Self::multiplier(self.defense)
    }

    /// Speed multiplier from current stages.
    pub fn speed_multiplier(&self) -> f64 {
        Self::multiplier(self.speed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pkrs_data::StatId;

    #[test]
    fn stage_multipliers_match_reference_table() {
        assert!((StatStages::multiplier(0) - 1.0).abs() < f64::EPSILON);
        assert!((StatStages::multiplier(1) - 1.5).abs() < f64::EPSILON);
        assert!((StatStages::multiplier(-1) - 2.0 / 3.0).abs() < 1e-6);
        assert!((StatStages::multiplier(6) - 4.0).abs() < f64::EPSILON);
    }

    #[test]
    fn apply_clamps_stages() {
        let mut s = StatStages::zero();
        s.apply(StatId::Attack, 10);
        assert_eq!(s.attack, 6);
    }
}
