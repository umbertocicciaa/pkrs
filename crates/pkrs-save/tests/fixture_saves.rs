//! Integration (§12, §15.2): frozen fixture saves, one per format version, migrate to the
//! current format and load.
//!
//! `tests/fixtures/saves/v<N>/slot1.sav` is a save as format version `N` wrote it. Never edit
//! a fixture once committed; bumping [`CURRENT_FORMAT_VERSION`] adds a new `v<N>` folder (the
//! first test fails until it exists).

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::{Path, PathBuf};

use pkrs_core::{Direction, GridPos};
use pkrs_save::{
    CURRENT_FORMAT_VERSION, GameMigration, RNG_BATTLE, RNG_ENCOUNTER, SaveData, SaveLoader,
    SaveSlot, SaveStore, SlotInfo, Value,
};

fn fixture_dir(version: u32) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/saves")
        .join(format!("v{version}"))
}

fn load(version: u32, loader: &mut SaveLoader<'_>) -> SaveData {
    let store = SaveStore::new(fixture_dir(version));
    let loaded = store.load(SaveSlot::Slot(1), loader).unwrap();
    assert!(!loaded.from_backup);
    loaded.data
}

/// What every fixture holds, whatever its version.
fn assert_common(save: &SaveData) {
    assert_eq!(save.header.format_version, CURRENT_FORMAT_VERSION);
    assert_eq!(save.header.game_id, "com.example.harbor");
    assert_eq!(save.header.play_time_ticks, 5400);
    assert_eq!(save.header.summary.player_name, "Robin");
    let world = &save.world;
    assert!(world.flag("got_starter"));
    assert_eq!(world.var("rival_name"), Some(&Value::from("Sky")));
    assert_eq!(world.item_count("potion"), 3);
    assert_eq!(world.money, 500);
    let harbor = world.map("harbor").unwrap();
    assert!(harbor.has_fired("intro"));
    assert!(harbor.object("lamp_oil").picked_up);
    assert_eq!(
        harbor.object("guard").moved_to,
        Some((GridPos::new(4, 6), Direction::Left))
    );
    assert_eq!(harbor.var("visits"), Some(&Value::Int(2)));
    let player = world.player.as_ref().unwrap();
    assert_eq!(
        (player.map.as_str(), player.pos(), player.facing),
        ("harbor", GridPos::new(5, 7), Direction::Down)
    );
    assert_eq!(world.party.len(), 1);
    assert_eq!(world.party[0].species, "sproutle");
    assert_eq!(world.party[0].moves, ["tackle", "leaf_cut"]);
    let battle = &world.rng[RNG_BATTLE];
    assert_eq!((battle.seed, battle.steps), (4242, 99));
    assert_eq!(
        save.plugins["resonance"].get("charges"),
        Some(&Value::Int(2))
    );
}

#[test]
fn every_format_version_has_a_fixture() {
    for version in 1..=CURRENT_FORMAT_VERSION {
        let file = fixture_dir(version).join("slot1.sav");
        assert!(file.is_file(), "missing frozen fixture {}", file.display());
    }
}

#[test]
fn v1_fixture_migrates_and_loads() {
    let save = load(1, &mut SaveLoader::new());
    assert_common(&save);
    // v1 had a single game stream; it became the battle stream.
    assert_eq!(save.world.rng.len(), 1);
    assert_eq!(save.world.language, None);
}

#[test]
fn v2_fixture_loads() {
    let save = load(2, &mut SaveLoader::new());
    assert_common(&save);
    // Seeds use the whole `u64` range (stored bit-cast).
    assert_eq!(save.world.rng[RNG_ENCOUNTER].seed, u64::MAX - 1);
    assert_eq!(save.world.language.as_deref(), Some("en"));
    assert_eq!(
        save.world.player.as_ref().unwrap().field_states.get("surf"),
        Some(&false)
    );
}

#[test]
fn fixtures_list_on_the_slot_screen() {
    for version in 1..=CURRENT_FORMAT_VERSION {
        let store = SaveStore::new(fixture_dir(version));
        let list = store.list(&SaveLoader::new());
        match &list[0] {
            (SaveSlot::Slot(1), SlotInfo::Saved { header, .. }) => {
                assert_eq!(header.summary.location, "harbor");
                assert_eq!(header.format_version, CURRENT_FORMAT_VERSION);
            }
            other => panic!("v{version}: {other:?}"),
        }
        assert!(matches!(list[1], (SaveSlot::Slot(2), SlotInfo::Empty)));
    }
}

#[test]
fn migrated_saves_round_trip_in_the_current_format() {
    for version in 1..=CURRENT_FORMAT_VERSION {
        let save = load(version, &mut SaveLoader::new());
        let text = save.to_ron().unwrap();
        let again = SaveLoader::new().decode(&text, "again").unwrap();
        assert_eq!(again, save, "v{version}");
    }
}

/// A game migration like `scripts/migrate.lua` would do: a flag rename for game 0.2.0.
struct RenameRivalFlag;

impl GameMigration for RenameRivalFlag {
    fn migrate(&mut self, mut save: Value, from: &str) -> pkrs_save::Result<Value> {
        assert_eq!(from, "0.1.0");
        let flags = save.pointer_mut("world.flags").unwrap();
        if let Some(v) = flags.remove("old_rival_flag") {
            flags.insert("met_rival", v);
        }
        Ok(save)
    }
}

#[test]
fn game_migration_runs_on_every_fixture_after_engine_migrations() {
    for version in 1..=CURRENT_FORMAT_VERSION {
        let mut game = RenameRivalFlag;
        let mut loader = SaveLoader::new().with_game_migration(&mut game, "0.2.0");
        let save = load(version, &mut loader);
        assert!(save.world.flag("met_rival"), "v{version}");
        assert!(!save.world.flags().contains_key("old_rival_flag"));
        assert_eq!(save.header.game_version, "0.2.0");
    }
}
