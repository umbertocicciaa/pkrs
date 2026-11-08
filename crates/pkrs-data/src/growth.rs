//! Growth curves: how much total experience a creature needs for each level (§8.1 `growth`).
//!
//! The six curves follow the Gen-3 formulas (`n` = level):
//!
//! | Curve | Total exp at level `n` | Level 100 |
//! |---|---|---|
//! | `Fast` | `4n³/5` | 800 000 |
//! | `MediumFast` | `n³` | 1 000 000 |
//! | `MediumSlow` | `6n³/5 − 15n² + 100n − 140` | 1 059 860 |
//! | `Slow` | `5n³/4` | 1 250 000 |
//! | `Erratic` | piecewise, fastest late | 600 000 |
//! | `Fluctuating` | piecewise, slowest late | 1 640 000 |
//!
//! Level 1 always needs 0 exp (the curves' own value at level 1 is clamped), and every curve
//! is strictly increasing from level 1 to [`MAX_LEVEL`].

use serde::Deserialize;

/// Highest level a creature can reach.
pub const MAX_LEVEL: u8 = 100;

/// A species' growth curve (`growth: MediumFast` in species data).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Deserialize)]
pub enum GrowthRate {
    /// 800 000 exp at level 100.
    Fast,
    /// 1 000 000 exp at level 100 (the default).
    #[default]
    MediumFast,
    /// 1 059 860 exp at level 100; slow early, faster later.
    MediumSlow,
    /// 1 250 000 exp at level 100.
    Slow,
    /// 600 000 exp at level 100; slow early, very fast late.
    Erratic,
    /// 1 640 000 exp at level 100; fast early, very slow late.
    Fluctuating,
}

impl GrowthRate {
    /// Every curve, in declaration order.
    pub const ALL: [Self; 6] = [
        Self::Fast,
        Self::MediumFast,
        Self::MediumSlow,
        Self::Slow,
        Self::Erratic,
        Self::Fluctuating,
    ];

    /// Total experience needed to be at `level` (`0` for level 1 and below; levels above
    /// [`MAX_LEVEL`] count as [`MAX_LEVEL`]).
    #[must_use]
    pub fn exp_for_level(self, level: u8) -> u32 {
        let level = level.min(MAX_LEVEL);
        if level <= 1 {
            return 0;
        }
        let n = i64::from(level);
        let cube = n * n * n;
        let exp = match self {
            Self::Fast => 4 * cube / 5,
            Self::MediumFast => cube,
            Self::MediumSlow => 6 * cube / 5 - 15 * n * n + 100 * n - 140,
            Self::Slow => 5 * cube / 4,
            Self::Erratic => match level {
                0..50 => cube * (100 - n) / 50,
                50..68 => cube * (150 - n) / 100,
                68..98 => cube * ((1911 - 10 * n) / 3) / 500,
                _ => cube * (160 - n) / 100,
            },
            Self::Fluctuating => match level {
                0..15 => cube * ((n + 1) / 3 + 24) / 50,
                15..36 => cube * (n + 14) / 50,
                _ => cube * (n / 2 + 32) / 50,
            },
        };
        u32::try_from(exp.max(0)).unwrap_or(u32::MAX)
    }

    /// The level a creature with `exp` total experience is at (`1..=MAX_LEVEL`).
    #[must_use]
    pub fn level_for_exp(self, exp: u32) -> u8 {
        let mut level = 1;
        while level < MAX_LEVEL && self.exp_for_level(level + 1) <= exp {
            level += 1;
        }
        level
    }

    /// Total experience for every level: index `n` holds the exp needed for level `n`
    /// (index 0 is unused and 0).
    #[must_use]
    pub fn table(self) -> Vec<u32> {
        (0..=MAX_LEVEL).map(|l| self.exp_for_level(l)).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn level_100_totals_match_the_classic_tables() {
        let totals: Vec<u32> = GrowthRate::ALL
            .iter()
            .map(|g| g.exp_for_level(MAX_LEVEL))
            .collect();
        assert_eq!(
            totals,
            [800_000, 1_000_000, 1_059_860, 1_250_000, 600_000, 1_640_000]
        );
    }

    #[test]
    fn sample_levels_match_the_classic_tables() {
        use GrowthRate::*;
        let cases = [
            (Fast, 10, 800),
            (MediumFast, 5, 125),
            (MediumFast, 6, 216),
            (MediumSlow, 2, 9),
            (MediumSlow, 5, 135),
            (MediumSlow, 50, 117_360),
            (Slow, 10, 1_250),
            (Erratic, 10, 1_800),
            (Erratic, 50, 125_000),
            (Erratic, 70, 276_458),
            (Erratic, 98, 583_539),
            (Fluctuating, 10, 540),
            (Fluctuating, 20, 5_440),
            (Fluctuating, 50, 142_500),
        ];
        for (curve, level, exp) in cases {
            assert_eq!(curve.exp_for_level(level), exp, "{curve:?} level {level}");
        }
    }

    #[test]
    fn every_table_starts_at_zero_and_strictly_increases() {
        for curve in GrowthRate::ALL {
            let table = curve.table();
            assert_eq!(table.len(), usize::from(MAX_LEVEL) + 1);
            assert_eq!((table[0], table[1]), (0, 0), "{curve:?}");
            for level in 2..=usize::from(MAX_LEVEL) {
                assert!(table[level] > table[level - 1], "{curve:?} level {level}");
            }
        }
    }

    #[test]
    fn level_for_exp_inverts_the_table() {
        for curve in GrowthRate::ALL {
            for level in 1..=MAX_LEVEL {
                let exp = curve.exp_for_level(level);
                assert_eq!(curve.level_for_exp(exp), level, "{curve:?} at {exp}");
                if level > 1 {
                    assert_eq!(curve.level_for_exp(exp - 1), level - 1);
                }
            }
            assert_eq!(curve.level_for_exp(u32::MAX), MAX_LEVEL);
            assert_eq!(curve.exp_for_level(200), curve.exp_for_level(MAX_LEVEL));
        }
    }

    #[test]
    fn parses_from_ron() {
        let g: GrowthRate = crate::RonCodec::decode("MediumSlow", "g.ron").unwrap();
        assert_eq!(g, GrowthRate::MediumSlow);
        assert_eq!(GrowthRate::default(), GrowthRate::MediumFast);
        assert!(crate::RonCodec::decode::<GrowthRate>("Sluggish", "g.ron").is_err());
    }
}
