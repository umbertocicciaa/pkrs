//! Status, weather and field rules (§7.3).

use std::collections::BTreeMap;

/// Major vs volatile classification.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StatusKind {
    /// Only one major status per creature (sleep, burn, …).
    Major,
    /// Timed volatiles (confusion, …).
    Volatile,
}

/// End-of-turn residual behaviour.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResidualKind {
    /// Damage as a percent of max HP (poison/burn).
    MaxHpPercent {
        /// Percent of max HP (e.g. 16 ≈ 1/16 for poison).
        percent: u8,
    },
}

/// Data-defined status behaviour.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StatusDef {
    /// Content id (`burn`, `sleep`, …).
    pub id: String,
    /// Major or volatile.
    pub kind: StatusKind,
    /// Prevents using moves (sleep/freeze).
    pub blocks_move: bool,
    /// Full paralysis roll out of 100 (e.g. 25).
    pub full_paralysis_chance: Option<u8>,
    /// End-of-turn damage or similar.
    pub residual: Option<ResidualKind>,
    /// Default volatile duration in turns (`None` = until cleared).
    pub volatile_turns: Option<u8>,
}

/// Field weather entry.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WeatherDef {
    /// Weather id.
    pub id: String,
    /// Default duration when set without an explicit counter.
    pub default_turns: u8,
    /// Chip damage percent of max HP each end-of-turn (sandstorm/hail).
    pub chip_percent: Option<u8>,
}

/// Catalog of statuses and weather used by battles.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BattleRules {
    statuses: BTreeMap<String, StatusDef>,
    weather: BTreeMap<String, WeatherDef>,
}

impl BattleRules {
    /// Built-in Gen-3-style defaults for tests and early content.
    pub fn gen3_defaults() -> Self {
        let mut statuses = BTreeMap::new();
        for (id, kind, blocks, para, residual, vol_turns) in [
            ("sleep", StatusKind::Major, true, None, None, None),
            (
                "poison",
                StatusKind::Major,
                false,
                None,
                Some(ResidualKind::MaxHpPercent { percent: 12 }),
                None,
            ),
            (
                "burn",
                StatusKind::Major,
                false,
                None,
                Some(ResidualKind::MaxHpPercent { percent: 12 }),
                None,
            ),
            (
                "paralysis",
                StatusKind::Major,
                false,
                Some(25u8),
                None,
                None,
            ),
            ("freeze", StatusKind::Major, true, None, None, None),
            (
                "confusion",
                StatusKind::Volatile,
                false,
                None,
                None,
                Some(2),
            ),
        ] {
            statuses.insert(
                id.into(),
                StatusDef {
                    id: id.into(),
                    kind,
                    blocks_move: blocks,
                    full_paralysis_chance: para,
                    residual,
                    volatile_turns: vol_turns,
                },
            );
        }
        let mut weather = BTreeMap::new();
        for (id, turns, chip) in [
            ("sun", 5, None),
            ("rain", 5, None),
            ("sandstorm", 5, Some(12u8)),
            ("hail", 5, Some(12u8)),
        ] {
            weather.insert(
                id.into(),
                WeatherDef {
                    id: id.into(),
                    default_turns: turns,
                    chip_percent: chip,
                },
            );
        }
        Self { statuses, weather }
    }

    /// Looks up a status definition.
    pub fn status(&self, id: &str) -> Option<&StatusDef> {
        self.statuses.get(id)
    }

    /// Looks up weather metadata.
    pub fn weather(&self, id: &str) -> Option<&WeatherDef> {
        self.weather.get(id)
    }
}
