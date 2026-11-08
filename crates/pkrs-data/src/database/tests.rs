#![allow(clippy::unwrap_used, clippy::expect_used)]

use super::*;
use crate::content::{ContentSource, Record};
use crate::files::MemoryDataFiles;
use crate::growth::GrowthRate;
use crate::intern::Handle;
use crate::move_def::{MoveCategory, MoveTargetScope};
use crate::species::BaseStats;
use crate::trainer::AiTier;

const SPECIES_NESTED: &str = r#"
// §8.1 form
Species(
  id: "emberkit",
  name: "text:species.emberkit",
  types: ["fire"],
  base_stats: (hp: 45, atk: 52, def: 40, spa: 60, spd: 50, spe: 65),
  abilities: ["kindle"],
  learnset: [ (level: 1, move: "scratch"), (level: 7, move: "ember") ],
  evolutions: [ (into: "blazecat", trigger: Level(16)) ],
  sprites: (front: "sprites/creatures/emberkit_front.png", back: "sprites/creatures/emberkit_back.png", icon: "sprites/icons/emberkit.png"),
  catch_rate: 45, exp_yield: 62, growth: MediumFast,
)"#;

const SPECIES_FLAT: &str =
    r#"(id: "leafkit", types: ["grass"], hp: 40, attack: 30, defense: 31, speed: 32)"#;

const MOVES: &str = r#"[
  Move(
    id: "ember", name: "text:moves.ember", type: "fire", category: Special,
    power: 40, accuracy: 100, pp: 25, priority: 0, target: SingleOpponent,
    effects: [ (effect: "damage"), (effect: "apply_status", status: "burn", chance: 10) ],
  ),
  (id: "scratch", type: "normal", category: physical, power: 40, accuracy: 100, priority: 0,
   effects: [(effect: "damage")], flags: ["contact"]),
]"#;

fn files() -> MemoryDataFiles {
    MemoryDataFiles::new()
        .with(
            "data/types.ron",
            r#"TypeChart(types: ["normal", "fire", "grass"], chart: { ("fire", "grass"): 2.0 })"#,
        )
        .with("data/species/emberkit.ron", SPECIES_NESTED)
        .with("data/species/leafkit.ron", SPECIES_FLAT)
        .with("data/species/notes.txt", "not data")
        .with("data/moves/moves.ron", MOVES)
        .with(
            "data/abilities/kindle.ron",
            r#"Ability(id: "kindle", name: "text:abilities.kindle")"#,
        )
        .with(
            "data/items/items.ron",
            r#"[Item(id: "potion", price: 300), (id: "town_map", pocket: "key_items")]"#,
        )
        .with(
            "data/statuses/drowsy.ron",
            r#"Status(id: "drowsy", kind: volatile, volatile_turns: 3)"#,
        )
        .with("data/terrains/mud.ron", r#"Terrain(id: "mud", passable: false)"#)
        .with("data/effects/echo.ron", r#"Effect(id: "echo")"#)
        .with(
            "data/trainers/rival.ron",
            r#"Trainer(id: "rival", party: [(species: "leafkit", level: 5)], ai_profile: "profiles/rival_ai")"#,
        )
        .with(
            "data/trainers/profiles/rival_ai.ron",
            r#"AiProfile(id: "rival_ai", tier: Smart)"#,
        )
        .with(
            "data/encounters/route.ron",
            r#"[EncounterTable(id: "route_grass", terrains: ["tall_grass"],
                 slots: [(species: "leafkit", min_level: 2, max_level: 4, weight: 1)])]"#,
        )
        .with(
            "data/text/en/names.ron",
            r#"{ "species.emberkit": "Emberkit", "moves.ember": "Ember" }"#,
        )
        .with("data/text/it/names.ron", r#"{ "moves.ember": "Brace" }"#)
}

#[test]
fn every_schema_loads_and_is_interned() {
    let data = GameData::load(&files(), LoadOptions::default()).unwrap();
    assert!(data.battles_enabled());
    assert!((data.type_chart().multiplier("fire", "grass") - 2.0).abs() < f64::EPSILON);

    let ember_kit = data.species().by_id("emberkit").unwrap();
    assert_eq!(ember_kit.name, "text:species.emberkit");
    assert_eq!(
        ember_kit.base_stats,
        BaseStats {
            hp: 45,
            atk: 52,
            def: 40,
            spa: 60,
            spd: 50,
            spe: 65
        }
    );
    assert_eq!(ember_kit.abilities, ["kindle"]);
    assert_eq!(
        ember_kit.sprites.icon.as_deref(),
        Some("sprites/icons/emberkit.png")
    );
    assert_eq!(
        (ember_kit.catch_rate, ember_kit.growth),
        (45, GrowthRate::MediumFast)
    );
    let leaf = data.species().by_id("leafkit").unwrap();
    assert_eq!((leaf.base_stats.spa, leaf.base_stats.spd), (30, 31));
    assert_eq!(leaf.catch_rate, crate::DEFAULT_CATCH_RATE);

    // Files are read in name order, records in file order.
    let ids: Vec<&str> = data.species().iter().map(|(_, s)| s.id.as_str()).collect();
    assert_eq!(ids, ["emberkit", "leafkit"]);
    let h = data.species().handle("leafkit").unwrap();
    assert_eq!(h.index(), 1);
    assert_eq!(data.species().file(h), Some("data/species/leafkit.ron"));

    let ember = data.moves().by_id("ember").unwrap();
    assert_eq!(
        (ember.category, ember.pp, ember.targeting),
        (MoveCategory::Special, Some(25), MoveTargetScope::SingleFoe)
    );
    assert_eq!(data.moves().by_id("scratch").unwrap().flags, ["contact"]);
    assert!(data.abilities().contains("kindle"));
    assert_eq!(data.items().by_id("potion").unwrap().price, 300);
    assert_eq!(data.items().by_id("town_map").unwrap().pocket, "key_items");
    assert!(data.statuses().contains("drowsy"));
    assert!(!data.terrains().by_id("mud").unwrap().passable);
    assert!(data.effects().contains("echo"));
    assert_eq!(
        data.trainers().by_id("rival").unwrap().ai_profile_id(),
        "rival_ai"
    );
    assert_eq!(
        data.ai_profiles().by_id("rival_ai").unwrap().tier,
        AiTier::Smart
    );
    assert!(data.encounter_tables().get("route_grass").is_some());
    assert_eq!(data.text().get("it", "moves.ember"), Some("Brace"));
    assert_eq!(
        data.text().resolve("it", "en", &ember_kit.name).unwrap(),
        "Emberkit"
    );

    let counts = data.record_counts();
    assert_eq!(counts[&DataKind::Move], 2);
    assert_eq!(counts[&DataKind::Text], 3);
    let pack = data.battle_pack();
    assert_eq!(pack.species.len(), 2);
    assert!(pack.move_by_id("ember").is_some());
}

#[test]
fn loading_is_deterministic() {
    let a = GameData::load(&files(), LoadOptions::default()).unwrap();
    let b = GameData::load(&files(), LoadOptions::default()).unwrap();
    assert_eq!(a, b);
}

#[test]
fn battles_off_skips_battle_data() {
    let broken = files()
        .with("data/species/broken.ron", "(id: ")
        .with("data/moves/broken.ron", "[")
        .with("data/types.ron", "nope");
    let data = GameData::load(&broken, LoadOptions { battles: false }).unwrap();
    assert!(!data.battles_enabled());
    assert!(data.species().is_empty() && data.moves().is_empty());
    assert!(data.trainers().is_empty() && data.encounter_tables().is_empty());
    assert!(data.type_chart().types().is_empty());
    // Story data still loads.
    assert_eq!(data.items().len(), 2);
    assert!(data.terrains().contains("mud"));
    assert_eq!(data.text().len("en"), 2);
    // A story-only project with no data folder at all.
    let empty = GameData::load(&MemoryDataFiles::new(), LoadOptions { battles: false }).unwrap();
    assert_eq!(empty, GameData::empty(LoadOptions { battles: false }));
    // With battles on, the broken files are reported.
    let errors = GameData::load(&broken, LoadOptions::default()).unwrap_err();
    assert_eq!(errors.len(), 3, "{errors}");
}

#[test]
fn features_map_to_options() {
    let manifest = pkrs_core::ProjectManifest::from_toml_str(
        "[game]\nname = \"T\"\nid = \"com.example.t\"\n[engine]\nversion = \"0.0\"\napi = 1\n\
         [start]\nmap = \"m\"\nposition = [0, 0]\n[features]\nbattles = false\n",
    )
    .unwrap();
    assert_eq!(
        LoadOptions::from_features(&manifest.features),
        LoadOptions { battles: false }
    );
    assert!(LoadOptions::default().battles);
}

#[test]
fn duplicate_ids_name_the_file_and_the_id() {
    let dup = files()
        .with(
            "data/species/z_again.ron",
            r#"[(id: "leafkit", types: ["grass"], hp: 1, attack: 1, defense: 1, speed: 1)]"#,
        )
        .with(
            "data/moves/z_more.ron",
            r#"(id: "ember", type: "fire", category: special, power: 1, accuracy: 100, priority: 0, effects: [])"#,
        )
        .with("data/text/en/z_more.ron", r#"{ "moves.ember": "Again" }"#);
    let errors: Vec<String> = GameData::load(&dup, LoadOptions::default())
        .unwrap_err()
        .errors()
        .iter()
        .map(ToString::to_string)
        .collect();
    assert_eq!(
        errors,
        [
            "duplicate species `leafkit` in `data/species/z_again.ron` (already defined in `data/species/leafkit.ron`)",
            "duplicate move `ember` in `data/moves/z_more.ron` (already defined in `data/moves/moves.ron`)",
            "duplicate text key (en) `moves.ember` in `data/text/en/z_more.ron` (already defined in `data/text/en/names.ron`)",
        ]
    );
}

#[test]
fn override_replaces_within_the_game_folders() {
    let over = files().with(
        "data/species/z_over.ron",
        r#"(id: "leafkit", types: ["grass"], hp: 99, attack: 1, defense: 1, speed: 1, override: true)"#,
    );
    let data = GameData::load(&over, LoadOptions::default()).unwrap();
    let h = data.species().handle("leafkit").unwrap();
    assert_eq!(h.index(), 1, "the handle stays");
    assert_eq!(data.species().get(h).unwrap().base_stats.hp, 99);
    assert_eq!(data.species().file(h), Some("data/species/z_over.ron"));
}

#[test]
fn schema_and_id_errors_are_collected_with_their_file() {
    let bad = files()
        .with(
            "data/species/bad.ron",
            r#"(id: "budling", types: ["grass"], hp: 1, attack: 1, speed: 1)"#,
        )
        .with(
            "data/species/both.ron",
            r#"(id: "twin", types: ["grass"], hp: 1, attack: 1, defense: 1, speed: 1,
                base_stats: (hp: 1, atk: 1, def: 1, spa: 1, spd: 1, spe: 1))"#,
        )
        .with(
            "data/moves/wild.ron",
            r#"(id: "wild", type: "normal", category: physical, power: 1, accuracy: 101, priority: 0, effects: [])"#,
        )
        .with(
            "data/trainers/Bad.ron",
            r#"Trainer(id: "Bad Id", party: [(species: "leafkit", level: 5)], ai_profile: "basic")"#,
        )
        .with("data/items/broken.ron", "(id: ");
    let errors: Vec<String> = GameData::load(&bad, LoadOptions::default())
        .unwrap_err()
        .errors()
        .iter()
        .map(ToString::to_string)
        .collect();
    assert_eq!(errors.len(), 5, "{errors:#?}");
    assert!(
        errors[0].contains("data/species/bad.ron") && errors[0].contains("missing `base_stats`")
    );
    assert!(errors[1].contains("data/species/both.ron") && errors[1].contains("not both"));
    assert!(
        errors[2].starts_with("data/moves/wild.ron: invalid accuracy"),
        "{}",
        errors[2]
    );
    assert!(
        errors[3].starts_with("data/items/broken.ron")
            || errors[3].contains("data/items/broken.ron")
    );
    assert!(
        errors[4].starts_with("data/trainers/Bad.ron: invalid id"),
        "{}",
        errors[4]
    );
}

#[test]
fn plugin_content_merges_before_freezing() {
    let mut db = ContentDb::new();
    let mut parse = |kind, src: &str, source: ContentSource| {
        for record in crate::content::parse_content(kind, src, "x.ron").unwrap() {
            db.insert(record, source.clone()).unwrap();
        }
    };
    // The game's own move is in the content database too (unchanged: no replacement).
    parse(ContentKind::Move, MOVES, ContentSource::Game);
    parse(
        ContentKind::Move,
        r#"(id: "mudworks:mud_slap", type: "normal", category: special, power: 20, accuracy: 100, priority: 0, effects: [])"#,
        ContentSource::Plugin("mudworks".into()),
    );
    parse(
        ContentKind::Move,
        r#"(id: "scratch", type: "normal", category: physical, power: 90, accuracy: 100, priority: 0, effects: [], override: true)"#,
        ContentSource::Plugin("mudworks".into()),
    );
    let mut builder = GameDataBuilder::new(LoadOptions::default());
    builder.load_dir(&files(), DATA_ROOT).merge_content(&db);
    let data = builder.build().unwrap();
    assert_eq!(data.moves().len(), 3);
    let ember = data.moves().handle("ember").unwrap();
    assert_eq!(data.moves().file(ember), Some("data/moves/moves.ron"));
    let scratch = data.moves().handle("scratch").unwrap();
    assert_eq!(scratch.index(), 1);
    assert_eq!(data.moves().get(scratch).unwrap().power, 90);
    assert_eq!(data.moves().file(scratch), Some("plugin `mudworks`"));
    assert_eq!(
        data.moves().handle("mudworks:mud_slap").map(Handle::index),
        Some(2)
    );

    // Battle content of plugins is skipped with battles off; items are not.
    let mut items = ContentDb::new();
    items
        .insert(
            Record {
                def: ContentDef::Item(ItemDef {
                    id: "rope".into(),
                    name: String::new(),
                    description: String::new(),
                    price: 1,
                    pocket: "items".into(),
                }),
                override_existing: false,
            },
            ContentSource::Game,
        )
        .unwrap();
    let mut builder = GameDataBuilder::new(LoadOptions { battles: false });
    builder.merge_content(&db).merge_content(&items);
    let story = builder.build().unwrap();
    assert!(story.moves().is_empty());
    assert!(story.items().contains("rope"));
}
