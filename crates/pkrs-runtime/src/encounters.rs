//! Wild encounters in the overworld (§6.2, §6.3, §7.2): after each completed player step on
//! an `encounters` rect, roll against the rect's rate with the game RNG and, on a hit, start
//! a wild battle through the same path as `battle.start_wild`.
//!
//! Tables are read from `data/encounters/*.ron` ([`EncounterTables`]) the first time the
//! player steps onto an encounter rect, validated, and checked against the project's species.
//! A project whose tables fail to load logs the error once and has no encounters.

use pkrs_assets::{EntryKind, Vfs, VfsPath};
use pkrs_core::{GridPos, Pcg64Rng};
use pkrs_data::{DataError, EncounterTables};
use pkrs_script::BattleRequest;
use pkrs_world::{Map, WildEncounter, roll_step_encounter, unknown_encounter_tables};

/// Folder holding the encounter tables.
pub const ENCOUNTERS_DIR: &str = "data/encounters";

/// Seed of the encounter roll stream until the save owns the game RNG (§12).
pub const DEFAULT_ENCOUNTER_SEED: u64 = 0x0065_6e63_6f75_6e74;

/// Why the encounter tables could not be loaded.
#[derive(Debug, thiserror::Error)]
pub enum EncounterLoadError {
    /// A table file could not be listed or read.
    #[error(transparent)]
    Assets(#[from] pkrs_assets::AssetsError),
    /// A table is invalid, duplicated or names an unknown species.
    #[error(transparent)]
    Data(#[from] DataError),
    /// A table file is not UTF-8.
    #[error("`{0}` is not valid UTF-8")]
    Utf8(String),
}

/// Reads and validates every `data/encounters/*.ron` table of the project in `vfs`; each
/// slot's species must satisfy `known_species`. No folder means no tables.
pub fn load_encounter_tables(
    vfs: &dyn Vfs,
    known_species: impl Fn(&str) -> bool,
) -> Result<EncounterTables, EncounterLoadError> {
    let mut tables = EncounterTables::new();
    let dir = VfsPath::new(ENCOUNTERS_DIR)?;
    let Ok(entries) = vfs.list(&dir) else {
        return Ok(tables);
    };
    let mut files: Vec<VfsPath> = entries
        .into_iter()
        .filter(|e| e.kind == EntryKind::File && e.path.as_str().ends_with(".ron"))
        .map(|e| e.path)
        .collect();
    files.sort_by(|a, b| a.as_str().cmp(b.as_str()));
    for path in files {
        let source = String::from_utf8(vfs.read(&path)?)
            .map_err(|_| EncounterLoadError::Utf8(path.to_string()))?;
        tables.insert_ron(&source, path.as_str())?;
    }
    tables.check_species(known_species)?;
    Ok(tables)
}

#[derive(Debug, Default)]
enum Tables {
    /// Not needed yet.
    #[default]
    Unloaded,
    Loaded(EncounterTables),
    /// Loading failed (already logged): no encounters.
    Failed,
}

/// The overworld's encounter state: the tables (loaded on first use) and the roll stream.
#[derive(Debug)]
pub struct Encounters {
    tables: Tables,
    rng: Pcg64Rng,
    /// Maps whose unknown table references were already logged.
    warned_maps: Vec<String>,
}

impl Default for Encounters {
    fn default() -> Self {
        Self::new(DEFAULT_ENCOUNTER_SEED)
    }
}

impl Encounters {
    /// No tables loaded yet; rolls draw from a stream seeded with `seed`.
    #[must_use]
    pub fn new(seed: u64) -> Self {
        Self {
            tables: Tables::Unloaded,
            rng: Pcg64Rng::from_seed(seed),
            warned_maps: Vec::new(),
        }
    }

    /// Reseeds the roll stream (scenario runs pin it, §15.2).
    pub fn set_seed(&mut self, seed: u64) {
        self.rng = Pcg64Rng::from_seed(seed);
    }

    /// Uses `tables` instead of loading them from the project (tests, tools).
    pub fn set_tables(&mut self, tables: EncounterTables) {
        self.tables = Tables::Loaded(tables);
    }

    /// `true` once the tables were loaded (or set) successfully.
    #[must_use]
    pub fn tables_loaded(&self) -> bool {
        matches!(self.tables, Tables::Loaded(_))
    }

    /// `true` when the tables still have to be loaded before `pos` on `map` can roll: the
    /// tile lies in an `encounters` rect and no load was attempted yet.
    #[must_use]
    pub fn needs_tables(&self, map: &Map, pos: GridPos) -> bool {
        matches!(self.tables, Tables::Unloaded) && map.encounters_at(pos).next().is_some()
    }

    /// Loads the tables with [`load_encounter_tables`] unless that was already attempted. A
    /// failure is logged and disables encounters.
    pub fn ensure_loaded(&mut self, vfs: &dyn Vfs, known_species: impl Fn(&str) -> bool) {
        if !matches!(self.tables, Tables::Unloaded) {
            return;
        }
        self.tables = match load_encounter_tables(vfs, known_species) {
            Ok(tables) => {
                tracing::info!(target: "encounter", tables = tables.len(), "encounter tables loaded");
                Tables::Loaded(tables)
            }
            Err(err) => {
                tracing::error!(target: "encounter", "encounters disabled: {err}");
                Tables::Failed
            }
        };
    }

    /// Turns encounters off (e.g. the battle content they need failed to load); later
    /// [`ensure_loaded`](Self::ensure_loaded) calls do nothing.
    pub fn disable(&mut self) {
        self.tables = Tables::Failed;
    }

    /// Rolls for a wild encounter after the player completed a step onto `pos` (§6.3):
    /// `Some` with the battle to start on a hit. Does nothing (and draws nothing) off an
    /// encounter rect or before the tables are loaded. A rect naming an unknown table is
    /// logged once per map and never rolls.
    pub fn on_step(&mut self, map: &Map, pos: GridPos) -> Option<WildEncounter> {
        let Tables::Loaded(tables) = &self.tables else {
            return None;
        };
        match roll_step_encounter(map, pos, tables, &mut self.rng) {
            Ok(hit) => hit,
            Err(err) => {
                if !self.warned_maps.contains(&map.id) {
                    let missing = unknown_encounter_tables(map, tables).join(", ");
                    tracing::warn!(target: "encounter", map = %map.id, %missing, "{err}");
                    self.warned_maps.push(map.id.clone());
                }
                None
            }
        }
    }
}

/// The battle request a wild encounter starts (the `battle.start_wild` path, §7.2).
#[must_use]
pub fn wild_battle_request(encounter: &WildEncounter) -> BattleRequest {
    BattleRequest::Wild {
        species: encounter.species.clone(),
        level: encounter.level,
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;
    use pkrs_assets::MemoryVfs;
    use pkrs_world::{EncounterZone, LogicalGrid, Properties, Terrain, TileRect};

    const TABLE: &str = r#"EncounterTable(
        id: "route_grass",
        terrains: ["tall_grass"],
        slots: [(species: "sproutling", min_level: 2, max_level: 4, weight: 1)],
    )"#;

    fn vfs(files: &[(&str, &str)]) -> MemoryVfs {
        let mut vfs = MemoryVfs::new();
        for (path, text) in files {
            vfs.insert(path, text.as_bytes().to_vec()).unwrap();
        }
        vfs
    }

    fn map(table: &str) -> Map {
        let mut grid = LogicalGrid::new(4, 1);
        for x in 0..3 {
            grid.cell_mut(GridPos::new(x, 0)).unwrap().terrain = Some(Terrain::TallGrass);
        }
        Map {
            id: "route".into(),
            width: 4,
            height: 1,
            tile_width: 16,
            tile_height: 16,
            properties: Properties::new(),
            tilesets: vec![],
            layers: vec![],
            grid,
            objects: vec![],
            encounters: vec![EncounterZone {
                id: 9,
                rect: TileRect::new(0, 0, 4, 1),
                table: table.into(),
                rate: 50,
            }],
        }
    }

    #[test]
    fn loads_tables_and_checks_species() {
        let files = vfs(&[("data/encounters/route.ron", TABLE)]);
        let tables = load_encounter_tables(&files, |s| s == "sproutling").unwrap();
        assert!(tables.get("route_grass").is_some());
        let err = load_encounter_tables(&files, |_| false).unwrap_err();
        assert_eq!(err.to_string(), "unknown species `sproutling`");
        assert!(
            load_encounter_tables(&MemoryVfs::new(), |_| true)
                .unwrap()
                .is_empty()
        );
        let dup = vfs(&[
            ("data/encounters/a.ron", TABLE),
            ("data/encounters/b.ron", TABLE),
        ]);
        assert_eq!(
            load_encounter_tables(&dup, |_| true)
                .unwrap_err()
                .to_string(),
            "duplicate encounter table `route_grass`"
        );
    }

    #[test]
    fn steps_roll_only_on_loaded_matching_tiles() {
        let map = map("route_grass");
        let mut enc = Encounters::new(1);
        assert!(enc.needs_tables(&map, GridPos::new(0, 0)));
        assert!(
            !enc.needs_tables(&map, GridPos::new(0, 1)),
            "outside the rect"
        );
        assert_eq!(
            enc.on_step(&map, GridPos::new(0, 0)),
            None,
            "not loaded yet"
        );
        enc.ensure_loaded(&vfs(&[("data/encounters/route.ron", TABLE)]), |_| true);
        assert!(enc.tables_loaded());
        assert!(!enc.needs_tables(&map, GridPos::new(0, 0)));

        let hits: Vec<WildEncounter> = (0..200)
            .filter_map(|_| enc.on_step(&map, GridPos::new(1, 0)))
            .collect();
        assert!(
            (60..=140).contains(&hits.len()),
            "rate 50: {} hits",
            hits.len()
        );
        for hit in &hits {
            assert_eq!((hit.species.as_str(), hit.zone), ("sproutling", 9));
            assert!(
                matches!(wild_battle_request(hit), BattleRequest::Wild { level, .. } if (2..=4).contains(&level))
            );
        }
        // x = 3 lies in the rect but is not tall grass.
        assert!((0..200).all(|_| enc.on_step(&map, GridPos::new(3, 0)).is_none()));
    }

    #[test]
    fn same_seed_same_encounters() {
        let map = map("route_grass");
        let run = |seed| {
            let mut enc = Encounters::new(seed);
            enc.ensure_loaded(&vfs(&[("data/encounters/route.ron", TABLE)]), |_| true);
            (0..100)
                .map(|_| enc.on_step(&map, GridPos::new(0, 0)))
                .collect::<Vec<_>>()
        };
        assert_eq!(run(3), run(3));
        assert_ne!(run(3), run(4));
    }

    #[test]
    fn failures_disable_encounters() {
        let mut enc = Encounters::default();
        enc.ensure_loaded(&vfs(&[("data/encounters/bad.ron", "(id: 1)")]), |_| true);
        assert!(!enc.tables_loaded());
        let route = map("route_grass");
        assert!(
            !enc.needs_tables(&route, GridPos::new(0, 0)),
            "no second attempt"
        );
        assert_eq!(enc.on_step(&route, GridPos::new(0, 0)), None);

        let mut unknown = Encounters::default();
        unknown.set_tables(EncounterTables::new());
        let nowhere = map("nowhere");
        assert!((0..20).all(|_| unknown.on_step(&nowhere, GridPos::new(0, 0)).is_none()));
        assert_eq!(unknown.warned_maps, ["route"]);
    }
}
