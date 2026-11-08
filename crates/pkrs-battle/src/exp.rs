//! Experience math for battle rewards (§7.3, §7.4 `on_battle_end`).
//!
//! The battle core only knows the **default** rewards of a won battle
//! ([`BattleRewards`], set with [`Battle::set_rewards`](crate::Battle::set_rewards)). It
//! starts [`BattleResult`](crate::BattleResult) with them when the player wins, and
//! `on_battle_end` hooks may change them. Distributing the final exp to the party and
//! levelling up happen outside the battle (the runtime's post-battle flow).

/// Default rewards of a won battle, before `on_battle_end` hooks run.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BattleRewards {
    /// Total experience for the player's side.
    pub exp: u32,
    /// Prize money.
    pub money: u32,
}

/// Experience for defeating one creature (Gen-3 style): `base_yield · level / 7`, times 1.5
/// in trainer battles.
#[must_use]
pub fn defeat_exp(base_yield: u32, level: u8, trainer: bool) -> u32 {
    let exp = u64::from(base_yield) * u64::from(level) / 7;
    let exp = if trainer { exp * 3 / 2 } else { exp };
    u32::try_from(exp).unwrap_or(u32::MAX)
}

/// Splits `total` evenly between `participants` creatures; the remainder goes to the first
/// ones (one point each), so the shares always add up to `total`. No participants: no
/// shares.
#[must_use]
pub fn split_exp(total: u32, participants: usize) -> Vec<u32> {
    let Ok(n) = u32::try_from(participants) else {
        return Vec::new();
    };
    if n == 0 {
        return Vec::new();
    }
    let (share, rest) = (total / n, total % n);
    (0..n).map(|i| share + u32::from(i < rest)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defeat_exp_scales_with_level_and_trainer_bonus() {
        assert_eq!(defeat_exp(64, 5, false), 45);
        assert_eq!(defeat_exp(64, 5, true), 67);
        assert_eq!(defeat_exp(100, 1, false), 14);
        assert_eq!(defeat_exp(0, 50, true), 0);
        assert_eq!(defeat_exp(u32::MAX, 100, true), u32::MAX);
    }

    #[test]
    fn split_exp_adds_up_to_the_total() {
        assert_eq!(split_exp(100, 1), [100]);
        assert_eq!(split_exp(100, 3), [34, 33, 33]);
        assert_eq!(split_exp(1, 2), [1, 0]);
        assert!(split_exp(100, 0).is_empty());
        for n in 1..7 {
            assert_eq!(split_exp(1_001, n).iter().sum::<u32>(), 1_001);
        }
    }
}
