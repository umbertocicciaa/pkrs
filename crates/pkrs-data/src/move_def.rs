//! Move definitions and built-in effect primitives (§7.3).

use serde::Deserialize;

use crate::error::{DataError, Result};

/// Physical / special / status category.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MoveCategory {
    /// Uses attack and defense.
    Physical,
    /// Uses special attack and special defense (SpA/SpD stubbed to Atk/Def in M1).
    Special,
    /// No damage calc.
    Status,
}

/// Which stat a stage effect modifies.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StatId {
    /// Attack stage.
    Attack,
    /// Defense stage.
    Defense,
    /// Speed stage.
    Speed,
    /// Accuracy stage.
    Accuracy,
    /// Evasion stage.
    Evasion,
}

/// Which foes a move can hit in multi-active battles (§7.3 formats).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum MoveTargetScope {
    /// One chosen foe (default).
    #[default]
    SingleFoe,
    /// Every non-fainted foe (`Spread` moves in doubles).
    AllFoes,
}

/// Who receives a stat-stage or status effect.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum EffectTarget {
    /// The move user (default for self-boosts like Growth).
    #[default]
    User,
    /// The move target.
    Target,
}

fn default_hundred() -> u8 {
    100
}

/// Built-in move effect primitives (data-driven, §7.3).
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(tag = "effect", rename_all = "snake_case")]
pub enum MoveEffect {
    /// Standard damage using the Gen-3 pipeline.
    Damage,
    /// Applies a major status if the roll succeeds.
    ApplyStatus {
        /// Status id (e.g. `burn`).
        status: String,
        /// Chance out of 100.
        chance: u8,
    },
    /// Raises or lowers a stat stage.
    StatStage {
        /// Which stat.
        stat: StatId,
        /// Delta stages (-6..=6 clamped at apply time).
        stages: i8,
        /// Who receives the stage change.
        #[serde(default)]
        target: EffectTarget,
        /// Chance out of 100 (secondary drops like Acid).
        #[serde(default = "default_hundred")]
        chance: u8,
    },
    /// Heals a fraction of max HP.
    Heal {
        /// Percent of max HP (1–100).
        percent: u8,
    },
    /// Recoil damage to the user.
    Recoil {
        /// Percent of damage dealt.
        percent: u8,
    },
    /// Drain heal to the user.
    Drain {
        /// Percent of damage dealt.
        percent: u8,
    },
    /// Multiple hits in one turn.
    MultiHit {
        /// Minimum hits.
        min: u8,
        /// Maximum hits.
        max: u8,
    },
    /// Sets field weather.
    Weather {
        /// Weather id.
        weather: String,
    },
    /// Protect-like end-of-turn block (stub: emits message).
    Protect,
    /// Forces the target to switch (stub: message).
    SwitchOut,
}

/// One move loaded from RON.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct MoveDef {
    /// Content id.
    pub id: String,
    /// Element type id.
    #[serde(rename = "type")]
    pub type_id: String,
    /// Move category.
    pub category: MoveCategory,
    /// Base power (0 for pure status).
    pub power: u16,
    /// Accuracy 0–100 (`0` = always hits for status).
    pub accuracy: u8,
    /// Priority for turn order.
    pub priority: i8,
    /// Who the move hits in multi-active battles.
    #[serde(default)]
    pub targeting: MoveTargetScope,
    /// Effect pipeline executed after a successful hit check.
    pub effects: Vec<MoveEffect>,
}

impl MoveDef {
    /// Parses a single move RON document.
    pub fn parse_ron(source: &str, file: &str) -> Result<Self> {
        ron::from_str(source).map_err(|e| DataError::Parse {
            file: file.into(),
            message: e.to_string(),
        })
    }
}
