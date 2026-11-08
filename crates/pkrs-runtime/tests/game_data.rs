//! Integration (§8.1): fixture projects' `data/` folders load through the assets VFS
//! (`FolderVfs`) into the immutable content database, honouring `features.battles`.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::{Path, PathBuf};

use pkrs_assets::FolderVfs;
use pkrs_battle::RngPolicy;
use pkrs_core::ProjectManifest;
use pkrs_data::{DataKind, GameData, LoadOptions, MoveCategory, MoveTargetScope};
use pkrs_runtime::battle::{BattleContent, BattleContentError, BattleKind};
use pkrs_runtime::data::load_game_data;
use pkrs_script::BattleRequest;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn project(name: &str) -> PathBuf {
    root().join("tests/fixtures/projects").join(name)
}

/// Loads `dir` with the options of its own `pkrs.toml`.
fn load(dir: &Path) -> GameData {
    let manifest = ProjectManifest::load(dir).expect("manifest");
    let vfs = FolderVfs::new(dir).expect("mount");
    load_game_data(&vfs, LoadOptions::from_features(&manifest.features), None)
        .unwrap_or_else(|e| panic!("{}: {e}", dir.display()))
}

#[test]
fn full_game_fixture_loads_every_schema() {
    let data = load(&project("full"));
    assert!(data.battles_enabled());
    let counts = data.record_counts();
    assert_eq!(counts[&DataKind::TypeChart], 4);
    assert_eq!(counts[&DataKind::Species], 4);
    assert_eq!(counts[&DataKind::Move], 8);
    assert_eq!(counts[&DataKind::Item], 3);
    assert_eq!(counts[&DataKind::Ability], 3);
    assert_eq!(counts[&DataKind::Trainer], 2);
    assert_eq!(counts[&DataKind::AiProfile], 1);
    assert_eq!(counts[&DataKind::Encounter], 2);
    assert_eq!(counts[&DataKind::Text], 22);

    // Folder merge order: files by name, records in file order.
    let species: Vec<&str> = data.species().iter().map(|(_, s)| s.id.as_str()).collect();
    assert_eq!(species, ["emberkit", "sproutling", "blazecat", "puddlefin"]);
    let puddlefin = data.species().by_id("puddlefin").unwrap();
    assert_eq!(puddlefin.base_stats.spa, 30, "flat form: spa = attack");
    assert_eq!(puddlefin.catch_rate, 190);
    let emberkit = data.species().by_id("emberkit").unwrap();
    assert_eq!(emberkit.base_stats.spe, 65);
    assert_eq!(emberkit.abilities, ["kindle"]);
    assert!(
        emberkit
            .abilities
            .iter()
            .all(|a| data.abilities().contains(a))
    );

    let burst = data.moves().by_id("flame_burst").unwrap();
    assert_eq!(burst.category, MoveCategory::Special);
    assert_eq!(burst.targeting, MoveTargetScope::AllFoes);
    assert_eq!(burst.pp, Some(15));
    let quick = data.moves().handle("quick_attack").unwrap();
    assert_eq!(
        data.moves().file(quick),
        Some("data/moves/quick_attack.ron")
    );
    assert_eq!(data.moves().get(quick).unwrap().priority, 1);

    assert_eq!(data.items().by_id("capture_orb").unwrap().pocket, "balls");
    assert_eq!(
        data.text().resolve("it", "en", &emberkit.name),
        Some("Braciolino")
    );
    assert_eq!(
        data.text().resolve("it", "en", "text:moves.scratch"),
        Some("Scratch"),
        "missing Italian text falls back to English"
    );

    // The runtime's battle content and encounters come from the same database.
    let content = BattleContent::from_data(&data).unwrap();
    assert!(
        content.check_species().is_empty(),
        "learnsets and evolutions resolve"
    );
    let prepared = content
        .prepare(
            &BattleRequest::Trainer {
                id: "keeper_mara".into(),
            },
            &RngPolicy::Fixed { seed: 3 },
        )
        .unwrap();
    assert_eq!(
        prepared.kind,
        BattleKind::Trainer {
            name: "Keeper Mara".into()
        }
    );
    let ember = prepared.setup.player.party[0].moves[1].as_ref().unwrap();
    assert_eq!(
        (ember.id.as_str(), ember.max_pp),
        ("ember", 25),
        "pp from data"
    );
    let tables = pkrs_runtime::encounters::load_encounter_tables(&data).unwrap();
    assert!(tables.get("shore_water").is_some());
}

#[test]
fn story_only_projects_load_without_battle_data() {
    let data = load(&project("story-only"));
    assert!(!data.battles_enabled());
    assert!(data.species().is_empty() && data.moves().is_empty());
    assert!(matches!(
        BattleContent::from_data(&data),
        Err(BattleContentError::BattlesDisabled)
    ));
    // The full fixture's battle data is skipped entirely when battles are off.
    let vfs = FolderVfs::new(project("full")).unwrap();
    let story = load_game_data(&vfs, LoadOptions { battles: false }, None).unwrap();
    assert!(story.species().is_empty() && story.trainers().is_empty());
    assert_eq!(story.items().len(), 3);
    assert_eq!(story.text().len("en"), 18);
}

#[test]
fn every_fixture_project_and_template_loads() {
    for name in [
        "battle-town",
        "doubles-arena",
        "full",
        "grass-route",
        "minimal",
        "resonance-town",
        "scenario-town",
        "story-only",
        "training-grounds",
    ] {
        load(&project(name));
    }
    // Templates (their pkrs.toml has placeholders): battle data on for `battle`, off for
    // the story-only `one-shot`.
    for (template, battles) in [("battle", true), ("one-shot", false)] {
        let dir = root().join("crates/pkrs-cli/templates").join(template);
        let vfs = FolderVfs::new(&dir).unwrap();
        let data = load_game_data(&vfs, LoadOptions { battles }, None)
            .unwrap_or_else(|e| panic!("template {template}: {e}"));
        assert_eq!(data.battles_enabled(), battles);
        if battles {
            assert_eq!(data.species().len(), 3, "{template}");
            assert!(data.trainers().contains("player"));
        }
    }
}
