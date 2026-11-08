//! Wild encounter tables (`data/encounters/*.ron`, §6.2, §8.1).
//!
//! A map's `encounters` object layer holds rects with `table` (an [`EncounterTable`] id) and
//! `rate` (percent chance per completed step). The table lists the creatures that can appear,
//! each with a level range and a weight, and may restrict itself to some terrains so one
//! area can hold separate grass and water tables:
//!
//! ```ron
//! // data/encounters/route1_grass.ron
//! EncounterTable(
//!   id: "route1_grass",
//!   terrains: ["tall_grass"],        // optional; empty = every tile of the rect
//!   slots: [
//!     (species: "sproutling", min_level: 2, max_level: 4, weight: 60),
//!     (species: "leafkit", min_level: 3, max_level: 5, weight: 40),
//!   ],
//! )
//! ```
//!
//! A file holds one table or a list of them. Rolls go through the game RNG
//! ([`GameRng`]), so they replay identically for a fixed seed.

use std::collections::BTreeMap;
use std::collections::btree_map::Entry;

use pkrs_core::GameRng;
use serde::Deserialize;

use crate::content::is_content_id;
use crate::error::{DataError, Result};

/// Highest creature level a slot may produce.
pub const MAX_ENCOUNTER_LEVEL: u8 = 100;

/// One creature an [`EncounterTable`] can produce.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct EncounterSlot {
    /// Species id.
    pub species: String,
    /// Lowest level (1–100).
    pub min_level: u8,
    /// Highest level (`min_level`–100).
    pub max_level: u8,
    /// Relative chance against the table's other slots (at least 1).
    pub weight: u32,
}

/// A wild encounter table.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct EncounterTable {
    /// Table id, referenced by the `table` property of `encounters` rects.
    pub id: String,
    /// Terrain names (`tall_grass`, `water`, a custom terrain, …) the table rolls on. Empty:
    /// every tile of a rect that references it, whatever its terrain.
    #[serde(default)]
    pub terrains: Vec<String>,
    /// The creatures that can appear.
    pub slots: Vec<EncounterSlot>,
}

/// A creature picked from a table: species and level.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EncounterPick {
    /// Species id.
    pub species: String,
    /// Level.
    pub level: u8,
}

impl EncounterTable {
    /// Parses a file holding one `EncounterTable(...)` or a list of them, and validates each
    /// table ([`validate`](Self::validate)).
    pub fn parse_ron(source: &str, file: &str) -> Result<Vec<Self>> {
        let tables: Vec<Self> = crate::codec::RonCodec::decode_one_or_many(source, file)?;
        for table in &tables {
            table.validate()?;
        }
        Ok(tables)
    }

    /// Schema checks (§8.2 stage 2): a content id, at least one slot, levels in `1..=100`
    /// with `min_level <= max_level`, weights of at least 1, non-empty terrain names.
    pub fn validate(&self) -> Result<()> {
        let invalid = |field: &str, message: String| DataError::InvalidField {
            field: field.into(),
            context: format!("data/encounters: {}", self.id),
            message,
        };
        if !is_content_id(&self.id) {
            return Err(invalid(
                "id",
                "must be snake_case or `plugin_id:snake_case`".into(),
            ));
        }
        if self.slots.is_empty() {
            return Err(invalid("slots", "must list at least one creature".into()));
        }
        if self.terrains.iter().any(String::is_empty) {
            return Err(invalid(
                "terrains",
                "terrain names must not be empty".into(),
            ));
        }
        for (i, slot) in self.slots.iter().enumerate() {
            let n = i + 1;
            if slot.species.is_empty() {
                return Err(invalid("slots", format!("slot {n}: species is empty")));
            }
            if slot.min_level == 0 || slot.max_level > MAX_ENCOUNTER_LEVEL {
                return Err(invalid(
                    "slots",
                    format!("slot {n}: levels must be 1..={MAX_ENCOUNTER_LEVEL}"),
                ));
            }
            if slot.min_level > slot.max_level {
                return Err(invalid(
                    "slots",
                    format!(
                        "slot {n}: min_level {} is above max_level {}",
                        slot.min_level, slot.max_level
                    ),
                ));
            }
            if slot.weight == 0 {
                return Err(invalid(
                    "slots",
                    format!("slot {n}: weight must be at least 1"),
                ));
            }
        }
        if self.total_weight().is_none() {
            return Err(invalid("slots", "the weights add up past 2^32 - 1".into()));
        }
        Ok(())
    }

    /// Cross-reference check (§8.2 stage 3): every slot's species is known.
    pub fn check_species(&self, known: impl Fn(&str) -> bool) -> Result<()> {
        match self.slots.iter().find(|s| !known(&s.species)) {
            Some(slot) => Err(DataError::UnknownReference {
                kind: "species".into(),
                id: slot.species.clone(),
            }),
            None => Ok(()),
        }
    }

    /// `true` when the table rolls on a tile whose terrain is `terrain` (`None`: no terrain).
    #[must_use]
    pub fn matches_terrain(&self, terrain: Option<&str>) -> bool {
        self.terrains.is_empty() || terrain.is_some_and(|t| self.terrains.iter().any(|n| n == t))
    }

    /// Sum of the slot weights (`None` on overflow; [`validate`](Self::validate) rejects it).
    #[must_use]
    pub fn total_weight(&self) -> Option<u32> {
        self.slots
            .iter()
            .try_fold(0u32, |sum, s| sum.checked_add(s.weight))
    }

    /// Picks a slot by weight, then a level in its range, drawing from `rng`. `None` for a
    /// table without slots or weight (never the case for a validated table).
    pub fn pick(&self, rng: &mut dyn GameRng) -> Option<EncounterPick> {
        let total = self.total_weight().filter(|&t| t > 0)?;
        let mut roll = rng.gen_range_u32(total);
        let slot = self.slots.iter().find(|s| {
            if roll < s.weight {
                true
            } else {
                roll -= s.weight;
                false
            }
        })?;
        let span = u32::from(slot.max_level.saturating_sub(slot.min_level)) + 1;
        let offset = u8::try_from(rng.gen_range_u32(span)).unwrap_or(0);
        Some(EncounterPick {
            species: slot.species.clone(),
            level: slot.min_level.saturating_add(offset),
        })
    }
}

/// `true` with a chance of `rate` percent (`0` never, `100` or more always). Always draws
/// one value from `rng`, so a step consumes the same randomness whatever the rate.
pub fn roll_encounter_rate(rate: u8, rng: &mut dyn GameRng) -> bool {
    rng.gen_range_u32(100) < u32::from(rate)
}

/// Every encounter table of a project, by id.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct EncounterTables {
    tables: BTreeMap<String, EncounterTable>,
}

impl EncounterTables {
    /// No tables.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds `table` (validated); a second table with the same id is an error.
    pub fn insert(&mut self, table: EncounterTable) -> Result<()> {
        table.validate()?;
        match self.tables.entry(table.id.clone()) {
            Entry::Occupied(_) => Err(DataError::Duplicate {
                kind: "encounter table".into(),
                id: table.id,
            }),
            Entry::Vacant(v) => {
                v.insert(table);
                Ok(())
            }
        }
    }

    /// Parses `source` (`file` names it in errors) with [`EncounterTable::parse_ron`] and
    /// adds every table.
    pub fn insert_ron(&mut self, source: &str, file: &str) -> Result<()> {
        for table in EncounterTable::parse_ron(source, file)? {
            self.insert(table)?;
        }
        Ok(())
    }

    /// The table `id`.
    #[must_use]
    pub fn get(&self, id: &str) -> Option<&EncounterTable> {
        self.tables.get(id)
    }

    /// Number of tables.
    #[must_use]
    pub fn len(&self) -> usize {
        self.tables.len()
    }

    /// `true` without tables.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.tables.is_empty()
    }

    /// Tables in id order.
    pub fn iter(&self) -> impl Iterator<Item = &EncounterTable> {
        self.tables.values()
    }

    /// [`EncounterTable::check_species`] for every table.
    pub fn check_species(&self, known: impl Fn(&str) -> bool) -> Result<()> {
        self.tables
            .values()
            .try_for_each(|table| table.check_species(&known))
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;
    use pkrs_core::Pcg64Rng;

    const TABLE: &str = r#"
// A comment before the record.
EncounterTable(
    id: "route_grass",
    terrains: ["tall_grass"],
    slots: [
        (species: "sproutling", min_level: 2, max_level: 4, weight: 75),
        (species: "leafkit", min_level: 5, max_level: 5, weight: 25),
    ],
)
"#;

    fn table() -> EncounterTable {
        EncounterTable::parse_ron(TABLE, "data/encounters/route_grass.ron")
            .unwrap()
            .remove(0)
    }

    #[test]
    fn parses_one_table_or_a_list() {
        let t = table();
        assert_eq!(t.id, "route_grass");
        assert_eq!(t.terrains, ["tall_grass"]);
        assert_eq!(t.slots.len(), 2);
        assert_eq!(t.total_weight(), Some(100));

        let list = r#"[
            (id: "a", slots: [(species: "x", min_level: 1, max_level: 1, weight: 1)]),
            (id: "b", slots: [(species: "y", min_level: 1, max_level: 1, weight: 1)]),
        ]"#;
        let tables = EncounterTable::parse_ron(list, "list.ron").unwrap();
        assert_eq!(tables.len(), 2);
        assert!(tables[0].terrains.is_empty(), "terrains default to empty");
    }

    #[test]
    fn schema_errors_are_reported() {
        let bad = |slots: &str| {
            let source = format!(r#"(id: "t", slots: [{slots}])"#);
            EncounterTable::parse_ron(&source, "t.ron")
                .unwrap_err()
                .to_string()
        };
        assert_eq!(
            bad(""),
            "invalid slots in `data/encounters: t`: must list at least one creature"
        );
        assert!(
            bad(r#"(species: "x", min_level: 0, max_level: 3, weight: 1)"#).contains("1..=100")
        );
        assert!(
            bad(r#"(species: "x", min_level: 5, max_level: 101, weight: 1)"#).contains("1..=100")
        );
        assert!(
            bad(r#"(species: "x", min_level: 5, max_level: 3, weight: 1)"#)
                .contains("min_level 5 is above max_level 3")
        );
        assert!(bad(r#"(species: "x", min_level: 1, max_level: 3, weight: 0)"#).contains("weight"));
        assert!(bad(r#"(species: "", min_level: 1, max_level: 3, weight: 1)"#).contains("species"));
        let overflow = format!(
            r#"(species: "x", min_level: 1, max_level: 1, weight: {m}), (species: "y", min_level: 1, max_level: 1, weight: {m})"#,
            m = u32::MAX
        );
        assert!(bad(&overflow).contains("2^32"));

        let bad_id = EncounterTable::parse_ron(
            r#"(id: "Route 1", slots: [(species: "x", min_level: 1, max_level: 1, weight: 1)])"#,
            "t.ron",
        );
        assert!(bad_id.unwrap_err().to_string().contains("invalid id"));
        let parse = EncounterTable::parse_ron("(id: 3)", "data/encounters/t.ron");
        assert!(
            parse
                .unwrap_err()
                .to_string()
                .starts_with("failed to parse `data/encounters/t.ron`")
        );
    }

    #[test]
    fn duplicate_tables_and_unknown_species_are_errors() {
        let mut tables = EncounterTables::new();
        tables.insert(table()).unwrap();
        let dup = tables.insert(table()).unwrap_err();
        assert_eq!(dup.to_string(), "duplicate encounter table `route_grass`");
        assert_eq!(tables.len(), 1);

        assert!(tables.check_species(|_| true).is_ok());
        let err = tables.check_species(|s| s == "sproutling").unwrap_err();
        assert_eq!(err.to_string(), "unknown species `leafkit`");
    }

    #[test]
    fn terrain_filter() {
        let t = table();
        assert!(t.matches_terrain(Some("tall_grass")));
        assert!(!t.matches_terrain(Some("water")));
        assert!(!t.matches_terrain(None));
        let any = EncounterTable {
            terrains: Vec::new(),
            ..t
        };
        assert!(any.matches_terrain(None));
        assert!(any.matches_terrain(Some("water")));
    }

    #[test]
    fn selection_follows_the_weights_and_level_ranges() {
        let t = table();
        let mut rng = Pcg64Rng::from_seed(40);
        let n = 40_000;
        let mut sprout = 0u32;
        let mut levels = BTreeMap::new();
        for _ in 0..n {
            let pick = t.pick(&mut rng).unwrap();
            if pick.species == "sproutling" {
                sprout += 1;
                assert!((2..=4).contains(&pick.level), "{pick:?}");
                *levels.entry(pick.level).or_insert(0u32) += 1;
            } else {
                assert_eq!(
                    pick,
                    EncounterPick {
                        species: "leafkit".into(),
                        level: 5
                    }
                );
            }
        }
        // 75 % expected; 4 standard deviations at n = 40 000 is about 0.9 %.
        let share = f64::from(sprout) / f64::from(n);
        assert!((share - 0.75).abs() < 0.01, "sproutling share {share}");
        // Every level of the range shows up about equally often.
        assert_eq!(levels.len(), 3);
        for count in levels.values() {
            let level_share = f64::from(*count) / f64::from(sprout);
            assert!((level_share - 1.0 / 3.0).abs() < 0.02, "{levels:?}");
        }
    }

    #[test]
    fn rate_rolls_match_the_rate_and_replay_for_a_seed() {
        let mut rng = Pcg64Rng::from_seed(7);
        assert!(!(0..1000).any(|_| roll_encounter_rate(0, &mut rng)));
        assert!((0..1000).all(|_| roll_encounter_rate(100, &mut rng)));
        let n = 50_000;
        let hits = (0..n).filter(|_| roll_encounter_rate(20, &mut rng)).count();
        let share = hits as f64 / f64::from(n);
        assert!((share - 0.20).abs() < 0.008, "rate 20 hit {share}");

        let rolls = |seed| {
            let mut rng = Pcg64Rng::from_seed(seed);
            let t = table();
            (0..200)
                .map(|_| (roll_encounter_rate(30, &mut rng), t.pick(&mut rng)))
                .collect::<Vec<_>>()
        };
        assert_eq!(rolls(99), rolls(99));
        assert_ne!(rolls(99), rolls(100));
    }
}
