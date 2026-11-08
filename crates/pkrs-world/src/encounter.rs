//! Wild encounters on a map (§6.2, §6.3): which `encounters` rect and table apply to a tile,
//! and the per-step roll.
//!
//! - **Lookup** ([`encounter_site`]): the rects covering the tile are tried in map order; the
//!   first one whose [`EncounterTable`] accepts the tile's terrain
//!   ([`EncounterTable::matches_terrain`]) is the site. Overlapping rects can so give one
//!   area a grass table and a water table.
//! - **Roll** ([`roll_step_encounter`]): after a completed step on a site, one draw against
//!   the rect's `rate` (percent per step), then the table picks species and level. Every draw
//!   comes from the caller's [`GameRng`], so a fixed seed replays the same encounters. A tile
//!   without a site draws nothing.

use pkrs_core::{GameRng, GridPos};
use pkrs_data::{DataError, EncounterPick, EncounterTable, EncounterTables, roll_encounter_rate};

use crate::map::{EncounterZone, Map};

/// The rect and table that apply to a tile.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EncounterSite<'a> {
    /// The `encounters` rect.
    pub zone: &'a EncounterZone,
    /// The table it references.
    pub table: &'a EncounterTable,
}

/// A wild creature met on a step.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WildEncounter {
    /// Species id.
    pub species: String,
    /// Level.
    pub level: u8,
    /// Table it came from.
    pub table: String,
    /// Tiled object id of the rect.
    pub zone: u32,
}

/// The encounter site of `pos` on `map`, if any.
///
/// Errors with [`DataError::UnknownReference`] when a rect covering `pos` names a table that
/// `tables` does not have (checked in map order, up to the first matching site).
pub fn encounter_site<'a>(
    map: &'a Map,
    pos: GridPos,
    tables: &'a EncounterTables,
) -> Result<Option<EncounterSite<'a>>, DataError> {
    let terrain = map.grid.terrain(pos).map(|t| t.name());
    for zone in map.encounters_at(pos) {
        let table = tables
            .get(&zone.table)
            .ok_or_else(|| DataError::UnknownReference {
                kind: "encounter table".into(),
                id: zone.table.clone(),
            })?;
        if table.matches_terrain(terrain) {
            return Ok(Some(EncounterSite { zone, table }));
        }
    }
    Ok(None)
}

/// Rolls for a wild encounter after the player completed a step onto `pos` (§6.3 step
/// events): `None` when `pos` has no site or the rate roll misses.
pub fn roll_step_encounter(
    map: &Map,
    pos: GridPos,
    tables: &EncounterTables,
    rng: &mut dyn GameRng,
) -> Result<Option<WildEncounter>, DataError> {
    let Some(site) = encounter_site(map, pos, tables)? else {
        return Ok(None);
    };
    if !roll_encounter_rate(site.zone.rate, rng) {
        return Ok(None);
    }
    Ok(site
        .table
        .pick(rng)
        .map(|EncounterPick { species, level }| WildEncounter {
            species,
            level,
            table: site.table.id.clone(),
            zone: site.zone.id,
        }))
}

/// Table ids referenced by `map`'s `encounters` rects that `tables` does not define, in map
/// order without repeats (for load-time warnings and `pkrs check`).
#[must_use]
pub fn unknown_encounter_tables(map: &Map, tables: &EncounterTables) -> Vec<String> {
    let mut missing: Vec<String> = Vec::new();
    for zone in &map.encounters {
        if tables.get(&zone.table).is_none() && !missing.contains(&zone.table) {
            missing.push(zone.table.clone());
        }
    }
    missing
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;
    use crate::map::{LogicalGrid, Properties, Terrain, TileRect};
    use pkrs_core::Pcg64Rng;

    /// 6x2 map: row 0 is tall grass at x 0..4, water at x 4..6; row 1 has no terrain.
    /// Rect 1 (grass table, rate 25) covers x 0..6 on both rows; rect 2 (water table,
    /// rate 10) covers the same area.
    fn map() -> Map {
        let mut grid = LogicalGrid::new(6, 2);
        for x in 0..6 {
            let terrain = if x < 4 {
                Terrain::TallGrass
            } else {
                Terrain::Water
            };
            grid.cell_mut(GridPos::new(x, 0)).unwrap().terrain = Some(terrain);
        }
        Map {
            id: "route".into(),
            width: 6,
            height: 2,
            tile_width: 16,
            tile_height: 16,
            properties: Properties::new(),
            tilesets: vec![],
            layers: vec![],
            grid,
            objects: vec![],
            encounters: vec![
                EncounterZone {
                    id: 1,
                    rect: TileRect::new(0, 0, 6, 2),
                    table: "grass".into(),
                    rate: 25,
                },
                EncounterZone {
                    id: 2,
                    rect: TileRect::new(0, 0, 6, 2),
                    table: "water".into(),
                    rate: 10,
                },
            ],
        }
    }

    fn tables() -> EncounterTables {
        let mut tables = EncounterTables::new();
        tables
            .insert_ron(
                r#"[
                    (id: "grass", terrains: ["tall_grass"], slots: [
                        (species: "sproutling", min_level: 2, max_level: 4, weight: 3),
                        (species: "leafkit", min_level: 3, max_level: 3, weight: 1),
                    ]),
                    (id: "water", terrains: ["water"], slots: [
                        (species: "bubble", min_level: 10, max_level: 12, weight: 1),
                    ]),
                ]"#,
                "data/encounters/route.ron",
            )
            .unwrap();
        tables
    }

    #[test]
    fn rect_lookup_uses_tile_and_terrain() {
        let (map, tables) = (map(), tables());
        let site = |x, y| {
            encounter_site(&map, GridPos::new(x, y), &tables)
                .unwrap()
                .map(|s| (s.zone.id, s.table.id.as_str()))
        };
        assert_eq!(site(0, 0), Some((1, "grass")));
        assert_eq!(site(3, 0), Some((1, "grass")));
        // Water tiles skip the grass rect and use the overlapping water rect.
        assert_eq!(site(4, 0), Some((2, "water")));
        // Inside both rects but no terrain either table rolls on.
        assert_eq!(site(0, 1), None);
        // Outside every rect.
        assert_eq!(site(6, 0), None);
        assert_eq!(site(-1, 0), None);
    }

    #[test]
    fn unknown_tables_are_reported() {
        let map = map();
        let mut only_water = EncounterTables::new();
        only_water
            .insert_ron(
                r#"(id: "water", slots: [(species: "bubble", min_level: 1, max_level: 1, weight: 1)])"#,
                "w.ron",
            )
            .unwrap();
        let err = encounter_site(&map, GridPos::new(0, 0), &only_water).unwrap_err();
        assert_eq!(err.to_string(), "unknown encounter table `grass`");
        assert_eq!(unknown_encounter_tables(&map, &only_water), ["grass"]);
        assert!(unknown_encounter_tables(&map, &tables()).is_empty());
    }

    #[test]
    fn no_site_draws_nothing_from_the_rng() {
        let (map, tables) = (map(), tables());
        let mut rng = Pcg64Rng::from_seed(5);
        let before = rng.clone();
        for _ in 0..50 {
            let roll = roll_step_encounter(&map, GridPos::new(0, 1), &tables, &mut rng).unwrap();
            assert_eq!(roll, None);
        }
        assert_eq!(rng.next_u64(), before.clone().next_u64());
    }

    #[test]
    fn step_rolls_are_deterministic_for_a_seed() {
        let (map, tables) = (map(), tables());
        let walk = |seed| {
            let mut rng = Pcg64Rng::from_seed(seed);
            (0..300)
                .map(|i| {
                    let pos = GridPos::new(i % 6, 0);
                    roll_step_encounter(&map, pos, &tables, &mut rng).unwrap()
                })
                .collect::<Vec<_>>()
        };
        let a = walk(40);
        assert_eq!(a, walk(40));
        assert_ne!(a, walk(41));
        for hit in a.iter().flatten() {
            match hit.table.as_str() {
                "grass" => assert!(["sproutling", "leafkit"].contains(&hit.species.as_str())),
                "water" => assert!((10..=12).contains(&hit.level) && hit.zone == 2),
                other => panic!("unexpected table {other}"),
            }
        }
    }

    /// Acceptance (#40): over many steps with a fixed seed the share of steps that start an
    /// encounter matches the rect's configured rate.
    #[test]
    fn encounter_rate_matches_the_configured_rate() {
        let (map, tables) = (map(), tables());
        let mut rng = Pcg64Rng::from_seed(0x40);
        let steps = 100_000u32;
        let count = |rng: &mut Pcg64Rng, pos| {
            (0..steps)
                .filter(|_| {
                    roll_step_encounter(&map, pos, &tables, rng)
                        .unwrap()
                        .is_some()
                })
                .count()
        };
        // Rate 25 on grass, rate 10 on water. Four standard deviations at 100 000 steps
        // are below 0.6 percentage points.
        let grass = count(&mut rng, GridPos::new(1, 0)) as f64 / f64::from(steps);
        let water = count(&mut rng, GridPos::new(5, 0)) as f64 / f64::from(steps);
        assert!((grass - 0.25).abs() < 0.006, "grass rate {grass}");
        assert!((water - 0.10).abs() < 0.006, "water rate {water}");
    }
}
