//! Species schema for battles and creature progression (§8.1; full data in M6).
//!
//! ```ron
//! // data/species/sproutling.ron
//! (
//!   id: "sproutling", types: ["grass"],
//!   hp: 40, attack: 30, defense: 30, speed: 25,
//!   exp_yield: 64, growth: MediumSlow,
//!   learnset: [ (level: 1, move: "tackle"), (level: 6, move: "vine_whip") ],
//!   evolutions: [ (into: "bloomling", trigger: Level(16)) ],
//! )
//! ```
//!
//! `exp_yield`, `growth`, `learnset` and `evolutions` are optional (`0`, `MediumFast`, none,
//! none), so species written before they existed still load.

use std::collections::BTreeMap;

use serde::Deserialize;

use crate::error::{DataError, Result};
use crate::growth::{GrowthRate, MAX_LEVEL};
use crate::move_def::MoveDef;

/// A move a species learns at a level.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct LearnsetEntry {
    /// Level the move is learned at (`1..=100`; `1` = known from the start).
    pub level: u8,
    /// Move id.
    #[serde(rename = "move")]
    pub move_id: String,
}

/// What makes a species evolve.
///
/// Only level-based evolution is built in; other triggers (items, trades, …) are plugin
/// territory (§9.4).
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[non_exhaustive]
pub enum EvolutionTrigger {
    /// Evolves after a battle in which it reached at least this level.
    Level(u8),
}

/// One evolution of a species.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct EvolutionDef {
    /// Species id it evolves into.
    pub into: String,
    /// When it evolves.
    pub trigger: EvolutionTrigger,
}

/// Species record used when building battle parties from content data.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct SpeciesDef {
    /// Content id.
    pub id: String,
    /// One or two type ids.
    pub types: Vec<String>,
    /// Base HP.
    pub hp: u32,
    /// Base attack.
    pub attack: u32,
    /// Base defense.
    pub defense: u32,
    /// Base speed.
    pub speed: u32,
    /// Base experience yield when defeated (`0`: defeating it gives no exp).
    #[serde(default)]
    pub exp_yield: u32,
    /// Growth curve (total exp per level).
    #[serde(default)]
    pub growth: GrowthRate,
    /// Moves learned by level, in the order they are learned.
    #[serde(default)]
    pub learnset: Vec<LearnsetEntry>,
    /// Evolutions (the first one whose trigger is met wins).
    #[serde(default)]
    pub evolutions: Vec<EvolutionDef>,
}

impl SpeciesDef {
    /// Parses a species RON document and checks its schema ([`validate`](Self::validate)).
    pub fn parse_ron(source: &str, file: &str) -> Result<Self> {
        let species: Self = ron::from_str(source).map_err(|e| DataError::Parse {
            file: file.into(),
            message: e.to_string(),
        })?;
        species.validate(file)?;
        Ok(species)
    }

    /// Schema checks: learnset and evolution levels in `1..=100`, learnset levels in
    /// ascending order, no species evolving into itself.
    pub fn validate(&self, file: &str) -> Result<()> {
        let bad = |field: &str, message: String| {
            Err(DataError::InvalidField {
                field: field.into(),
                context: file.into(),
                message,
            })
        };
        let mut last = 0;
        for (i, entry) in self.learnset.iter().enumerate() {
            if !(1..=MAX_LEVEL).contains(&entry.level) {
                return bad(
                    "learnset",
                    format!("learnset[{i}].level must be 1–100 (got {})", entry.level),
                );
            }
            if entry.level < last {
                return bad(
                    "learnset",
                    format!(
                        "learnset[{i}] (level {}) is out of level order",
                        entry.level
                    ),
                );
            }
            last = entry.level;
        }
        for (i, evo) in self.evolutions.iter().enumerate() {
            let EvolutionTrigger::Level(level) = evo.trigger;
            if !(1..=MAX_LEVEL).contains(&level) {
                return bad(
                    "evolutions",
                    format!("evolutions[{i}] level must be 1–100 (got {level})"),
                );
            }
            if evo.into == self.id {
                return bad(
                    "evolutions",
                    format!("evolutions[{i}]: `{}` cannot evolve into itself", self.id),
                );
            }
        }
        Ok(())
    }

    /// Moves learned exactly at `level`, in learnset order.
    pub fn moves_learned_at(&self, level: u8) -> impl Iterator<Item = &str> {
        self.learnset
            .iter()
            .filter(move |e| e.level == level)
            .map(|e| e.move_id.as_str())
    }

    /// The moves a creature of this species knows at `level` when nothing else is said:
    /// the last four distinct moves of the learnset learned at or below `level`, oldest
    /// first.
    #[must_use]
    pub fn moves_at_level(&self, level: u8) -> Vec<String> {
        let mut known: Vec<String> = Vec::new();
        for entry in self.learnset.iter().filter(|e| e.level <= level) {
            if known.contains(&entry.move_id) {
                continue;
            }
            if known.len() == 4 {
                known.remove(0);
            }
            known.push(entry.move_id.clone());
        }
        known
    }

    /// The species this one evolves into at `level` (the first level evolution whose level
    /// is reached).
    #[must_use]
    pub fn evolution_at(&self, level: u8) -> Option<&str> {
        self.evolutions.iter().find_map(|evo| match evo.trigger {
            EvolutionTrigger::Level(at) if level >= at => Some(evo.into.as_str()),
            EvolutionTrigger::Level(_) => None,
        })
    }

    /// Total experience a creature of this species needs for `level`.
    #[must_use]
    pub fn exp_for_level(&self, level: u8) -> u32 {
        self.growth.exp_for_level(level)
    }
}

/// Cross-reference check (§8.2 stage 3): every learnset move and every evolution target
/// exists. Returns every failure, not just the first.
#[must_use]
pub fn check_species_refs(
    species: &BTreeMap<String, SpeciesDef>,
    moves: &BTreeMap<String, MoveDef>,
) -> Vec<DataError> {
    let mut errors = Vec::new();
    for def in species.values() {
        for entry in &def.learnset {
            if !moves.contains_key(&entry.move_id) {
                errors.push(DataError::UnknownReference {
                    kind: "move".into(),
                    id: entry.move_id.clone(),
                });
            }
        }
        for evo in &def.evolutions {
            if !species.contains_key(&evo.into) {
                errors.push(DataError::UnknownReference {
                    kind: "species".into(),
                    id: evo.into.clone(),
                });
            }
        }
    }
    errors
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    const SPROUTLING: &str = r#"(
        id: "sproutling", types: ["grass"],
        hp: 40, attack: 30, defense: 30, speed: 25,
        exp_yield: 64, growth: MediumSlow,
        learnset: [
            (level: 1, move: "tackle"), (level: 1, move: "scratch"), (level: 3, move: "pound"),
            (level: 5, move: "leafage"), (level: 6, move: "vine_whip"), (level: 9, move: "tackle"),
        ],
        evolutions: [ (into: "bloomling", trigger: Level(6)) ],
    )"#;

    fn sproutling() -> SpeciesDef {
        SpeciesDef::parse_ron(SPROUTLING, "data/species/sproutling.ron").unwrap()
    }

    #[test]
    fn progression_fields_parse_and_default() {
        let s = sproutling();
        assert_eq!((s.exp_yield, s.growth), (64, GrowthRate::MediumSlow));
        assert_eq!(s.learnset[4].move_id, "vine_whip");
        assert_eq!(s.evolutions[0].trigger, EvolutionTrigger::Level(6));
        assert_eq!(s.exp_for_level(5), 135);
        let old = SpeciesDef::parse_ron(
            r#"(id: "a", types: ["normal"], hp: 1, attack: 1, defense: 1, speed: 1)"#,
            "a.ron",
        )
        .unwrap();
        assert_eq!((old.exp_yield, old.growth), (0, GrowthRate::MediumFast));
        assert!(old.learnset.is_empty() && old.evolutions.is_empty());
        assert!(old.moves_at_level(50).is_empty());
        assert_eq!(old.evolution_at(100), None);
    }

    #[test]
    fn learnset_selection() {
        let s = sproutling();
        assert_eq!(s.moves_at_level(1), ["tackle", "scratch"]);
        assert_eq!(s.moves_at_level(4), ["tackle", "scratch", "pound"]);
        assert_eq!(
            s.moves_at_level(5),
            ["tackle", "scratch", "pound", "leafage"]
        );
        // A fifth move pushes out the oldest one.
        assert_eq!(
            s.moves_at_level(6),
            ["scratch", "pound", "leafage", "vine_whip"]
        );
        // A move pushed out earlier comes back when it is listed again.
        assert_eq!(
            s.moves_at_level(9),
            ["pound", "leafage", "vine_whip", "tackle"]
        );
        // A move listed twice while still known stays once.
        let mut twice = s.clone();
        twice.learnset.truncate(2);
        twice.learnset.push(LearnsetEntry {
            level: 2,
            move_id: "tackle".into(),
        });
        assert_eq!(twice.moves_at_level(2), ["tackle", "scratch"]);
        assert_eq!(
            s.moves_learned_at(1).collect::<Vec<_>>(),
            ["tackle", "scratch"]
        );
        assert_eq!(s.moves_learned_at(6).collect::<Vec<_>>(), ["vine_whip"]);
        assert_eq!(s.moves_learned_at(7).count(), 0);
    }

    #[test]
    fn evolution_by_level() {
        let s = sproutling();
        assert_eq!(s.evolution_at(5), None);
        assert_eq!(s.evolution_at(6), Some("bloomling"));
        assert_eq!(s.evolution_at(40), Some("bloomling"));
    }

    #[test]
    fn schema_errors() {
        let bad = |extra: &str| {
            SpeciesDef::parse_ron(
                &format!(
                    r#"(id: "a", types: ["normal"], hp: 1, attack: 1, defense: 1, speed: 1, {extra})"#
                ),
                "a.ron",
            )
        };
        assert!(bad(r#"learnset: [(level: 0, move: "x")]"#).is_err());
        assert!(bad(r#"learnset: [(level: 101, move: "x")]"#).is_err());
        assert!(bad(r#"learnset: [(level: 5, move: "x"), (level: 2, move: "y")]"#).is_err());
        assert!(bad(r#"evolutions: [(into: "b", trigger: Level(0))]"#).is_err());
        assert!(bad(r#"evolutions: [(into: "a", trigger: Level(5))]"#).is_err());
        assert!(bad(r#"evolutions: [(into: "b", trigger: Stone("moon"))]"#).is_err());
        assert!(bad(r#"growth: Glacial"#).is_err());
        assert!(bad(r#"evolutions: [(into: "b", trigger: Level(5))]"#).is_ok());
    }

    #[test]
    fn cross_references_are_all_reported() {
        let s = sproutling();
        let species = BTreeMap::from([(s.id.clone(), s)]);
        let moves = BTreeMap::new();
        let msgs: Vec<String> = check_species_refs(&species, &moves)
            .iter()
            .map(ToString::to_string)
            .collect();
        assert_eq!(msgs.len(), 7, "{msgs:?}");
        assert_eq!(msgs[0], "unknown move `tackle`");
        assert_eq!(msgs[6], "unknown species `bloomling`");
    }
}
