//! Move definitions and built-in effect primitives (§7.3).

use serde::Deserialize;

use crate::codec::RonCodec;
use crate::error::Result;

/// Physical / special / status category (`physical` or §8.1's `Physical`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MoveCategory {
    /// Uses attack and defense (default).
    #[default]
    #[serde(alias = "Physical")]
    Physical,
    /// Uses special attack and special defense (SpA/SpD stubbed to Atk/Def in M1).
    #[serde(alias = "Special")]
    Special,
    /// No damage calc.
    #[serde(alias = "Status")]
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

/// Which foes a move can hit in multi-active battles (§7.3 formats). §8.1's
/// `SingleOpponent` / `AllOpponents` are accepted as aliases.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum MoveTargetScope {
    /// One chosen foe (default).
    #[default]
    #[serde(alias = "SingleOpponent", alias = "single_opponent")]
    SingleFoe,
    /// Every non-fainted foe (`Spread` moves in doubles).
    #[serde(alias = "AllOpponents", alias = "all_opponents")]
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

/// One move (§7.3, §8.1).
///
/// ```ron
/// // data/moves/ember.ron
/// Move(
///   id: "ember", name: "text:moves.ember", type: "fire", category: Special,
///   power: 40, accuracy: 100, pp: 25, priority: 0, target: SingleOpponent,
///   effects: [ (effect: "damage"), (effect: "apply_status", status: "burn", chance: 10) ],
/// )
/// ```
///
/// The `Move` name is optional; `name`, `pp`, `target`, `flags` and `script` have defaults.
/// Effects are written as tagged records (`(effect: "apply_status", …)`), the same shape
/// `data.register_move` takes from Lua.
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(rename = "Move")]
pub struct MoveDef {
    /// Content id.
    pub id: String,
    /// Display name: literal text or a `text:` key (empty: derived from the id).
    #[serde(default)]
    pub name: String,
    /// Element type id.
    #[serde(rename = "type")]
    pub type_id: String,
    /// Move category.
    pub category: MoveCategory,
    /// Base power (0 for pure status).
    pub power: u16,
    /// Accuracy 0–100 (`0` = always hits for status).
    pub accuracy: u8,
    /// Power points; `None` uses the engine default.
    #[serde(default)]
    pub pp: Option<u8>,
    /// Priority for turn order.
    pub priority: i8,
    /// Who the move hits in multi-active battles (`targeting` or §8.1's `target`).
    #[serde(default, alias = "target")]
    pub targeting: MoveTargetScope,
    /// Free-form flags (`contact`, `sound`, …) for hooks and plugins.
    #[serde(default)]
    pub flags: Vec<String>,
    /// Bespoke logic: `plugin_id:function` (§7.3).
    #[serde(default)]
    pub script: Option<String>,
    /// Effect pipeline executed after a successful hit check.
    pub effects: Vec<MoveEffect>,
}

impl MoveDef {
    /// Parses a single move RON document.
    pub fn parse_ron(source: &str, file: &str) -> Result<Self> {
        RonCodec::decode(source, file)
    }
}
