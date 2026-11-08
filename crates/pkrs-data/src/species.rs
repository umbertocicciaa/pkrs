//! Species schema (§8.1).
//!
//! ```ron
//! // data/species/emberkit.ron
//! Species(
//!   id: "emberkit",
//!   name: "text:species.emberkit",
//!   types: ["fire"],
//!   base_stats: (hp: 45, atk: 52, def: 40, spa: 60, spd: 50, spe: 65),
//!   abilities: ["kindle"],
//!   learnset: [ (level: 1, move: "scratch"), (level: 7, move: "ember") ],
//!   evolutions: [ (into: "blazecat", trigger: Level(16)) ],
//!   sprites: (front: "sprites/creatures/emberkit_front.png", icon: "sprites/icons/emberkit.png"),
//!   catch_rate: 45, exp_yield: 62, growth: MediumFast,
//! )
//! ```
//!
//! The `Species` name is optional. Everything except `id`, `types` and the base stats has a
//! default (`name` none, no abilities, no learnset, no evolutions, no sprites, `catch_rate`
//! 45, `exp_yield` 0, `growth` `MediumFast`).
//!
//! **Legacy base stats.** Instead of `base_stats`, a species may give the flat M1 fields
//! `hp`, `attack`, `defense` and `speed` (special attack/defense then equal attack/defense),
//! so species written before §8.1's nested form still load. Giving both forms, or only part
//! of the flat one, is an error.

use std::collections::BTreeMap;

use serde::Deserialize;

use crate::codec::RonCodec;
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

/// Base stats (§7.3): HP, attack, defense, special attack, special defense, speed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize)]
pub struct BaseStats {
    /// Hit points.
    pub hp: u32,
    /// Attack.
    pub atk: u32,
    /// Defense.
    pub def: u32,
    /// Special attack.
    pub spa: u32,
    /// Special defense.
    pub spd: u32,
    /// Speed.
    pub spe: u32,
}

/// Sprite paths of a species (project-relative; all optional).
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct SpeciesSprites {
    /// Front sprite (seen on the opponent's side).
    pub front: Option<String>,
    /// Back sprite (seen on the player's side).
    pub back: Option<String>,
    /// Party/menu icon.
    pub icon: Option<String>,
}

/// Default catch rate (out of 255).
pub const DEFAULT_CATCH_RATE: u8 = 45;

/// A species (§8.1).
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(try_from = "RawSpecies")]
pub struct SpeciesDef {
    /// Content id.
    pub id: String,
    /// Display name: literal text or a `text:` key (empty: derived from the id).
    pub name: String,
    /// One or two type ids.
    pub types: Vec<String>,
    /// Base stats.
    pub base_stats: BaseStats,
    /// Ability ids.
    pub abilities: Vec<String>,
    /// Base experience yield when defeated (`0`: defeating it gives no exp).
    pub exp_yield: u32,
    /// Growth curve (total exp per level).
    pub growth: GrowthRate,
    /// Moves learned by level, in the order they are learned.
    pub learnset: Vec<LearnsetEntry>,
    /// Evolutions (the first one whose trigger is met wins).
    pub evolutions: Vec<EvolutionDef>,
    /// Sprite paths.
    pub sprites: SpeciesSprites,
    /// Catch rate (out of 255).
    pub catch_rate: u8,
}

fn default_catch_rate() -> u8 {
    DEFAULT_CATCH_RATE
}

/// The file shape of a species: both base-stat forms are accepted.
#[derive(Deserialize)]
#[serde(rename = "Species")]
struct RawSpecies {
    id: String,
    #[serde(default)]
    name: String,
    types: Vec<String>,
    #[serde(default)]
    base_stats: Option<BaseStats>,
    #[serde(default)]
    hp: Option<u32>,
    #[serde(default)]
    attack: Option<u32>,
    #[serde(default)]
    defense: Option<u32>,
    #[serde(default)]
    speed: Option<u32>,
    #[serde(default)]
    abilities: Vec<String>,
    #[serde(default)]
    exp_yield: u32,
    #[serde(default)]
    growth: GrowthRate,
    #[serde(default)]
    learnset: Vec<LearnsetEntry>,
    #[serde(default)]
    evolutions: Vec<EvolutionDef>,
    #[serde(default)]
    sprites: SpeciesSprites,
    #[serde(default = "default_catch_rate")]
    catch_rate: u8,
}

impl TryFrom<RawSpecies> for SpeciesDef {
    type Error = String;

    fn try_from(raw: RawSpecies) -> core::result::Result<Self, String> {
        let flat = [raw.hp, raw.attack, raw.defense, raw.speed];
        let base_stats = match (raw.base_stats, flat) {
            (Some(stats), [None, None, None, None]) => stats,
            (Some(_), _) => {
                return Err(format!(
                    "species `{}`: give either `base_stats` or the flat `hp`/`attack`/`defense`/`speed`, not both",
                    raw.id
                ));
            }
            (None, [Some(hp), Some(atk), Some(def), Some(spe)]) => BaseStats {
                hp,
                atk,
                def,
                spa: atk,
                spd: def,
                spe,
            },
            (None, _) => {
                return Err(format!(
                    "species `{}`: missing `base_stats` (or all of `hp`, `attack`, `defense`, `speed`)",
                    raw.id
                ));
            }
        };
        Ok(Self {
            id: raw.id,
            name: raw.name,
            types: raw.types,
            base_stats,
            abilities: raw.abilities,
            exp_yield: raw.exp_yield,
            growth: raw.growth,
            learnset: raw.learnset,
            evolutions: raw.evolutions,
            sprites: raw.sprites,
            catch_rate: raw.catch_rate,
        })
    }
}

impl SpeciesDef {
    /// Parses a species RON document and checks its schema ([`validate`](Self::validate)).
    pub fn parse_ron(source: &str, file: &str) -> Result<Self> {
        let species: Self = RonCodec::decode(source, file)?;
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
    fn design_doc_species_and_both_base_stat_forms() {
        let s = SpeciesDef::parse_ron(
            r#"Species(
              id: "emberkit", name: "text:species.emberkit", types: ["fire"],
              base_stats: (hp: 45, atk: 52, def: 40, spa: 60, spd: 50, spe: 65),
              abilities: ["kindle"],
              sprites: (front: "f.png", icon: "i.png"),
              catch_rate: 45, exp_yield: 62, growth: MediumFast,
            )"#,
            "emberkit.ron",
        )
        .unwrap();
        assert_eq!(s.base_stats.spa, 60);
        assert_eq!(s.sprites.back, None);
        assert_eq!(s.sprites.icon.as_deref(), Some("i.png"));
        let flat = sproutling();
        assert_eq!(
            flat.base_stats,
            BaseStats {
                hp: 40,
                atk: 30,
                def: 30,
                spa: 30,
                spd: 30,
                spe: 25
            }
        );
        assert_eq!(
            (flat.name.as_str(), flat.catch_rate),
            ("", DEFAULT_CATCH_RATE)
        );
        let both = SpeciesDef::parse_ron(
            r#"(id: "a", types: [], hp: 1, attack: 1, defense: 1, speed: 1,
                base_stats: (hp: 1, atk: 1, def: 1, spa: 1, spd: 1, spe: 1))"#,
            "a.ron",
        );
        assert!(both.unwrap_err().to_string().contains("not both"));
        let partial = SpeciesDef::parse_ron(r#"(id: "a", types: [], hp: 1, attack: 1)"#, "a.ron");
        assert!(
            partial
                .unwrap_err()
                .to_string()
                .contains("missing `base_stats`")
        );
        let wrong_name = SpeciesDef::parse_ron(
            r#"Move(id: "a", types: [], hp: 1, attack: 1, defense: 1, speed: 1)"#,
            "a.ron",
        );
        assert!(wrong_name.is_err());
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
