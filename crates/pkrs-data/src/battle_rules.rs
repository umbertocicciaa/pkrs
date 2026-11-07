//! Status, weather and field rules (§7.3).

use std::collections::BTreeMap;

use serde::Deserialize;

use crate::error::{DataError, Result};

/// Major vs volatile classification.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StatusKind {
    /// Only one major status per creature (sleep, burn, …).
    Major,
    /// Timed volatiles (confusion, …).
    Volatile,
}

/// End-of-turn residual behaviour.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResidualKind {
    /// Damage as max_hp / denom (Gen 3 poison/burn = 8, weather chip = 16).
    MaxHpFraction {
        /// Divisor (must be > 0).
        denom: u8,
    },
}

/// Data-defined status behaviour.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct StatusDef {
    /// Content id (`burn`, `sleep`, …).
    pub id: String,
    /// Major or volatile.
    pub kind: StatusKind,
    /// Prevents using moves (sleep/freeze).
    #[serde(default)]
    pub blocks_move: bool,
    /// Full paralysis roll out of 100 (e.g. 25).
    pub full_paralysis_chance: Option<u8>,
    /// End-of-turn damage.
    pub residual: Option<ResidualKind>,
    /// Default volatile duration in turns (`None` = until cleared).
    pub volatile_turns: Option<u8>,
    /// Inclusive sleep duration range rolled on apply (turns remaining).
    pub sleep_turns: Option<(u8, u8)>,
    /// Thaw chance out of 100 when attempting a move while frozen.
    pub thaw_chance: Option<u8>,
    /// Types that cannot receive this status.
    #[serde(default)]
    pub immune_types: Vec<String>,
}

impl StatusDef {
    /// Schema checks: chances are `0..=100`, sleep ranges are non-empty and start at 1.
    pub fn validate(&self) -> Result<()> {
        if self.full_paralysis_chance.is_some_and(|c| c > 100) {
            return Err(DataError::InvalidField {
                field: "full_paralysis_chance".into(),
                context: self.id.clone(),
                message: "must be 0..=100".into(),
            });
        }
        if self.thaw_chance.is_some_and(|c| c > 100) {
            return Err(DataError::InvalidField {
                field: "thaw_chance".into(),
                context: self.id.clone(),
                message: "must be 0..=100".into(),
            });
        }
        if let Some((min, max)) = self.sleep_turns {
            if min == 0 || max < min {
                return Err(DataError::InvalidField {
                    field: "sleep_turns".into(),
                    context: self.id.clone(),
                    message: "invalid range".into(),
                });
            }
        }
        Ok(())
    }
}

/// Field weather entry.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct WeatherDef {
    /// Weather id.
    pub id: String,
    /// Default duration when set (`0` = indefinite until replaced).
    #[serde(default = "default_weather_turns")]
    pub default_turns: u8,
    /// Chip damage as max_hp / denom each end-of-turn (sand/hail = 16).
    pub chip_denom: Option<u8>,
    /// Types immune to weather chip.
    #[serde(default)]
    pub immune_types: Vec<String>,
}

fn default_weather_turns() -> u8 {
    5
}

/// Catalog of statuses and weather used by battles.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct BattleRules {
    statuses: BTreeMap<String, StatusDef>,
    weather: BTreeMap<String, WeatherDef>,
}

impl BattleRules {
    /// Built-in Gen-3-style defaults for tests and early content.
    #[allow(clippy::expect_used)]
    pub fn gen3_defaults() -> Self {
        let mut rules = Self::default();
        for def in [
            StatusDef {
                id: "sleep".into(),
                kind: StatusKind::Major,
                blocks_move: true,
                full_paralysis_chance: None,
                residual: None,
                volatile_turns: None,
                sleep_turns: Some((2, 5)),
                thaw_chance: None,
                immune_types: vec![],
            },
            StatusDef {
                id: "poison".into(),
                kind: StatusKind::Major,
                blocks_move: false,
                full_paralysis_chance: None,
                residual: Some(ResidualKind::MaxHpFraction { denom: 8 }),
                volatile_turns: None,
                sleep_turns: None,
                thaw_chance: None,
                immune_types: vec!["poison".into(), "steel".into()],
            },
            StatusDef {
                id: "burn".into(),
                kind: StatusKind::Major,
                blocks_move: false,
                full_paralysis_chance: None,
                residual: Some(ResidualKind::MaxHpFraction { denom: 8 }),
                volatile_turns: None,
                sleep_turns: None,
                thaw_chance: None,
                immune_types: vec!["fire".into()],
            },
            StatusDef {
                id: "paralysis".into(),
                kind: StatusKind::Major,
                blocks_move: false,
                full_paralysis_chance: Some(25),
                residual: None,
                volatile_turns: None,
                sleep_turns: None,
                thaw_chance: None,
                immune_types: vec![],
            },
            StatusDef {
                id: "freeze".into(),
                kind: StatusKind::Major,
                blocks_move: true,
                full_paralysis_chance: None,
                residual: None,
                volatile_turns: None,
                sleep_turns: None,
                thaw_chance: Some(20),
                immune_types: vec![],
            },
        ] {
            rules.insert_status(def).expect("builtin status");
        }
        for def in [
            WeatherDef {
                id: "sun".into(),
                default_turns: 5,
                chip_denom: None,
                immune_types: vec![],
            },
            WeatherDef {
                id: "rain".into(),
                default_turns: 5,
                chip_denom: None,
                immune_types: vec![],
            },
            WeatherDef {
                id: "sandstorm".into(),
                default_turns: 5,
                chip_denom: Some(16),
                immune_types: vec!["rock".into(), "ground".into(), "steel".into()],
            },
            WeatherDef {
                id: "hail".into(),
                default_turns: 5,
                chip_denom: Some(16),
                immune_types: vec!["ice".into()],
            },
        ] {
            rules.insert_weather(def).expect("builtin weather");
        }
        rules
    }

    /// Inserts a status (errors on duplicate id).
    pub fn insert_status(&mut self, def: StatusDef) -> Result<()> {
        def.validate()?;
        match self.statuses.entry(def.id.clone()) {
            std::collections::btree_map::Entry::Occupied(_) => Err(DataError::Duplicate {
                kind: "status".into(),
                id: def.id,
            }),
            std::collections::btree_map::Entry::Vacant(v) => {
                v.insert(def);
                Ok(())
            }
        }
    }

    /// Inserts weather metadata.
    pub fn insert_weather(&mut self, def: WeatherDef) -> Result<()> {
        match self.weather.entry(def.id.clone()) {
            std::collections::btree_map::Entry::Occupied(_) => Err(DataError::Duplicate {
                kind: "weather".into(),
                id: def.id,
            }),
            std::collections::btree_map::Entry::Vacant(v) => {
                v.insert(def);
                Ok(())
            }
        }
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

/// Returns true when `types` includes an immunity from `def`.
pub fn status_blocked_by_types(def: &StatusDef, types: &[String]) -> bool {
    def.immune_types
        .iter()
        .any(|imm| types.iter().any(|t| t == imm))
}

/// Weather chip immunity for a creature's types.
pub fn weather_chip_blocked(def: &WeatherDef, types: &[String]) -> bool {
    def.immune_types
        .iter()
        .any(|imm| types.iter().any(|t| t == imm))
}

/// Residual damage amount from max HP.
pub fn residual_amount(max_hp: u32, kind: ResidualKind) -> u32 {
    match kind {
        ResidualKind::MaxHpFraction { denom } if denom > 0 => (max_hp / u32::from(denom)).max(1),
        ResidualKind::MaxHpFraction { .. } => 1,
    }
}
